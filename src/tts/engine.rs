//! Enlace FFI a `libpiper` y síntesis de una frase.
//!
//! R1 eligió la ruta (a): FFI a `libpiper` (piper1-gpl v1.8.0), construida por
//! `build.rs`. Ver `docs/decisions/R1-tts-integration.md`.

use std::ffi::{CString, c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Mutex, MutexGuard};

use onnx_protobuf::{Message, ModelProto};

use super::{PhonemeChunk, TtsError, alignment, config_path};

/// Lock global de `libpiper`.
///
/// `libpiper` mantiene estado **global de proceso**, no thread-safe y además no
/// reentrante entre motores:
/// - un `static Ort::Env` creado en el primer `piper_create` (`piper.cpp:32`);
/// - espeak-ng, que `piper_create` inicializa (`espeak_Initialize`) y
///   `piper_synthesize_start` usa para fonemizar (`espeak_SetVoiceByName`), y
///   **que `piper_free` termina** (`espeak_Terminate`, `piper.cpp:301`).
///
/// Por ese `espeak_Terminate` no basta con serializar las llamadas: si un motor
/// se libera mientras otro vive, espeak queda apagado y el siguiente
/// `espeak_SetVoiceByName` del superviviente hace segfault (medido: dentro de
/// `LoadVoice`/`LoadLanguageOptions`). Además, dos llamadas concurrentes
/// corrompen el heap (SIGSEGV, o SIGABRT «double free or corruption»). Por eso
/// el lock se mantiene **toda la vida del `Engine`**: `load` lo toma y el motor
/// lo suelta al caer, de modo que nunca hay dos motores vivos ni llamadas
/// solapadas.
///
/// ponytail: un motor por proceso. El techo es que dos voces no cargan ni
/// sintetizan en paralelo (irrelevante hoy: `say`/`play` usan una voz por
/// proceso y el precache es de un solo hilo). Para paralelizar hará falta un
/// libpiper que no termine espeak en cada `piper_free` (o un fonemizador
/// thread-safe); entonces este lock se puede relajar a uno por motor.
fn ffi_lock() -> MutexGuard<'static, ()> {
    static FFI: Mutex<()> = Mutex::new(());
    FFI.lock().unwrap_or_else(|e| e.into_inner())
}

// De piper.h.
const PIPER_OK: c_int = 0;
const PIPER_DONE: c_int = 1;

#[repr(C)]
struct PiperSynthesizeOptions {
    speaker_id: c_int,
    length_scale: f32,
    noise_scale: f32,
    noise_w_scale: f32,
}

#[repr(C)]
struct PiperAudioChunk {
    samples: *const f32,
    num_samples: usize,
    sample_rate: c_int,
    is_last: bool,
    phonemes: *const u32,
    num_phonemes: usize,
    phoneme_ids: *const c_int,
    num_phoneme_ids: usize,
    alignments: *const c_int,
    num_alignments: usize,
}

unsafe extern "C" {
    // AVISO: `piper_create` y `piper_synthesize_*` son C++ y pueden lanzar
    // excepciones (JSON/ONNX corruptos, etc.). Dejar que una excepción C++
    // cruce la frontera FFI es comportamiento indefinido (en la práctica,
    // `abort`). Aquí se mitiga comprobando que el modelo y su `.onnx.json`
    // existen antes de llamar; un endurecimiento futuro sería un shim C++ con
    // `catch (...)` que devuelva un código de error en vez de excepcionar.
    fn piper_create(
        model_path: *const c_char,
        config_path: *const c_char,
        espeak_data_path: *const c_char,
    ) -> *mut c_void;
    fn piper_free(synth: *mut c_void);
    fn piper_default_synthesize_options(synth: *mut c_void) -> PiperSynthesizeOptions;
    fn piper_synthesize_start(
        synth: *mut c_void,
        text: *const c_char,
        options: *const PiperSynthesizeOptions,
    ) -> c_int;
    fn piper_synthesize_next(synth: *mut c_void, chunk: *mut PiperAudioChunk) -> c_int;
}

/// Motor piper para una voz concreta.
///
/// Retiene el lock global de `libpiper` mientras vive: solo puede existir un
/// `Engine` a la vez en todo el proceso (ver `ffi_lock`).
pub struct Engine {
    synth: *mut c_void,
    _ffi: MutexGuard<'static, ()>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        if !self.synth.is_null() {
            // piper_free ya comprueba el nulo, pero no delegamos en eso. Corre
            // bajo `_ffi`, que se libera al soltar el struct (después de esto).
            unsafe { piper_free(self.synth) };
            self.synth = ptr::null_mut();
        }
    }
}

impl Engine {
    /// Carga una voz piper (`.onnx` con su `.onnx.json` hermano).
    ///
    /// Si la voz no expone la salida de alineación (`w_ceil`), se parchea una
    /// copia en un fichero temporal y se carga esa. El original nunca se toca.
    pub fn load(voice_onnx: &Path) -> Result<Engine, TtsError> {
        if !voice_onnx.is_file() {
            return Err(TtsError::VoiceNotFound);
        }
        let config = config_path(voice_onnx);
        if !config.is_file() {
            return Err(TtsError::VoiceNotFound);
        }

        // `piper_create` inicializa el `static Ort::Env` y espeak-ng (globales).
        // El lock se queda dentro del `Engine`: cubre también el parche temporal
        // de `ensure_alignment_output` y garantiza que no haya dos motores vivos
        // (piper_free termina espeak-ng, que es un singleton de proceso).
        let ffi = ffi_lock();
        let model = ensure_alignment_output(voice_onnx)?;
        let c_model = cstring(&model)?;
        let c_config = cstring(&config)?;
        let espeak = espeak_data_path();

        let synth = unsafe {
            piper_create(
                c_model.as_ptr(),
                c_config.as_ptr(),
                espeak.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
            )
        };
        if synth.is_null() {
            return Err(TtsError::Engine);
        }
        Ok(Engine { synth, _ffi: ffi })
    }

    /// Sintetiza `text` a PCM mono con la alineación por fonema.
    ///
    /// `length_scale` es el campo homónimo de `piper_synthesize_options`
    /// (R3 confirmado): menor que 1 habla más rápido, mayor que 1 más despacio.
    pub fn synthesize(&mut self, text: &str, length_scale: f32) -> Result<PhonemeChunk, TtsError> {
        if self.synth.is_null() {
            return Err(TtsError::Engine);
        }
        let c_text = CString::new(text).map_err(|_| TtsError::Synth)?;

        // Ya tenemos el lock global en `self._ffi`: nadie más puede estar en
        // libpiper mientras este motor vive.
        let mut options = unsafe { piper_default_synthesize_options(self.synth) };
        options.length_scale = length_scale;

        if unsafe { piper_synthesize_start(self.synth, c_text.as_ptr(), &options) } != PIPER_OK {
            return Err(TtsError::Synth);
        }

        let mut samples: Vec<i16> = Vec::new();
        let mut rate = 0u32;
        let mut phonemes: Vec<char> = Vec::new();
        let mut phoneme_ids: Vec<i32> = Vec::new();
        let mut alignments: Vec<i32> = Vec::new();

        loop {
            let mut chunk: PiperAudioChunk = unsafe { std::mem::zeroed() };
            let rc = unsafe { piper_synthesize_next(self.synth, &mut chunk) };
            if rc < 0 {
                return Err(TtsError::Synth);
            }

            if !chunk.samples.is_null() && chunk.num_samples > 0 {
                let raw = unsafe { std::slice::from_raw_parts(chunk.samples, chunk.num_samples) };
                samples.extend(raw.iter().map(|&v| {
                    (v.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
                }));
            }
            if chunk.sample_rate > 0 {
                rate = chunk.sample_rate as u32;
            }
            // Los codepoints 0 son separadores internos del protocolo C: se
            // descartan para dejar arrays paralelos de un elemento por id.
            if !chunk.phonemes.is_null() {
                let raw =
                    unsafe { std::slice::from_raw_parts(chunk.phonemes, chunk.num_phonemes) };
                for &cp in raw {
                    if cp != 0 {
                        phonemes.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                    }
                }
            }
            if !chunk.phoneme_ids.is_null() {
                let raw =
                    unsafe { std::slice::from_raw_parts(chunk.phoneme_ids, chunk.num_phoneme_ids) };
                phoneme_ids.extend(raw.iter().copied());
            }
            if !chunk.alignments.is_null() {
                let raw =
                    unsafe { std::slice::from_raw_parts(chunk.alignments, chunk.num_alignments) };
                alignments.extend(raw.iter().copied());
            }

            if rc == PIPER_DONE {
                break;
            }
        }

        // Un modelo sin la salida w_ceil deja `alignments` vacío: no hay
        // alineación real que devolver, así que la síntesis falla explícita.
        if phonemes.len() != alignments.len() {
            return Err(TtsError::Synth);
        }

        Ok(PhonemeChunk {
            samples,
            rate,
            phonemes,
            phoneme_ids,
            alignments,
        })
    }
}

fn cstring(path: &Path) -> Result<CString, TtsError> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| TtsError::Engine)
}

/// Directorio de datos de espeak-ng: variable de entorno o el del sistema.
fn espeak_data_path() -> Option<CString> {
    if let Ok(p) = std::env::var("READALOUD_ESPEAK_DATA") {
        if let Ok(c) = CString::new(p) {
            return Some(c);
        }
    }
    let default = Path::new("/usr/share/espeak-ng-data");
    if default.is_dir() {
        if let Ok(c) = CString::new(default.as_os_str().as_bytes()) {
            return Some(c);
        }
    }
    None
}

/// Devuelve una ruta a un modelo que expone `w_ceil` como salida.
///
/// Si el modelo ya la expone, devuelve la ruta original. Si no, parchea una
/// copia en el directorio temporal (cacheada por ruta y tamaño) y devuelve esa.
/// Si el grafo no tiene ningún nodo `Ceil`, no hay nada que parchear y se
/// devuelve el original (la síntesis fallará al no haber alineación).
fn ensure_alignment_output(onnx: &Path) -> Result<PathBuf, TtsError> {
    let bytes = std::fs::read(onnx).map_err(|_| TtsError::VoiceNotFound)?;
    let mut model = ModelProto::parse_from_bytes(&bytes).map_err(|_| TtsError::Engine)?;

    if !alignment::add_alignment_output(&mut model) {
        return Ok(onnx.to_path_buf());
    }

    let patched = patched_path(onnx);
    if !patched.exists() {
        let out = model.write_to_bytes().map_err(|_| TtsError::Engine)?;
        std::fs::write(&patched, out).map_err(|_| TtsError::Engine)?;
    }
    Ok(patched)
}

/// Ruta cacheada del modelo parcheado, derivada de la ruta y tamaño originales.
fn patched_path(onnx: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    onnx.hash(&mut h);
    if let Ok(meta) = std::fs::metadata(onnx) {
        meta.len().hash(&mut h);
        if let Ok(m) = meta.modified() {
            if let Ok(d) = m.duration_since(std::time::UNIX_EPOCH) {
                d.as_secs().hash(&mut h);
            }
        }
    }
    std::env::temp_dir().join(format!("readaloud-{:016x}.onnx", h.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voz_de_test() -> Option<PathBuf> {
        // Los tests de integración no deben exigir modelos de 60 MB en disco.
        std::env::var("READALOUD_TEST_VOICE").ok().map(Into::into)
    }

    #[test]
    fn sintetiza_una_frase_y_devuelve_fonemas() {
        let Some(voz) = voz_de_test() else {
            eprintln!("skip: define READALOUD_TEST_VOICE");
            return;
        };
        let mut e = Engine::load(&voz).unwrap();
        let c = e.synthesize("Hola mundo.", 1.0).unwrap();
        assert!(c.samples.len() > 0);
        assert_eq!(c.rate, 22050);
        assert_eq!(c.phonemes.len(), c.alignments.len());
    }

    /// El contrato documentado en `PhonemeChunk`: sumar todas las alineaciones
    /// (rellenos incluidos) da el total de muestras. Si el aplanado cambiara,
    /// esto lo detecta.
    #[test]
    fn las_alineaciones_suman_las_muestras() {
        let Some(voz) = voz_de_test() else {
            eprintln!("skip: define READALOUD_TEST_VOICE");
            return;
        };
        let mut e = Engine::load(&voz).unwrap();
        let c = e.synthesize("Hola mundo, esta es una frase de prueba.", 1.0).unwrap();
        let total: i64 = c.alignments.iter().map(|&a| i64::from(a)).sum();
        assert_eq!(total as usize, c.samples.len());
    }

    #[test]
    fn una_voz_inexistente_da_error() {
        assert!(matches!(
            Engine::load(Path::new("/no/existe/voz.onnx")),
            Err(TtsError::VoiceNotFound)
        ));
    }

    /// Varios motores en hilos distintos, cargando/sintetizando/librando a la vez.
    ///
    /// `libpiper` no es thread-safe (estado global de espeak-ng y un `static
    /// Ort::Env`), así que sin serializar esto hace segfault de forma
    /// intermitente. El test no debe caerse nunca.
    #[test]
    fn dos_motores_en_paralelo_no_segfaultean() {
        use std::sync::{Arc, Barrier};

        let Some(voz) = voz_de_test() else {
            eprintln!("skip: define READALOUD_TEST_VOICE");
            return;
        };
        const HILOS: usize = 4;
        let barrera = Arc::new(Barrier::new(HILOS));
        let handles: Vec<_> = (0..HILOS)
            .map(|_| {
                let voz = voz.clone();
                let barrera = Arc::clone(&barrera);
                std::thread::spawn(move || {
                    // Todas arrancan a la vez: maximiza el solape de las llamadas.
                    barrera.wait();
                    let mut total = 0usize;
                    for _ in 0..3 {
                        let mut engine = Engine::load(&voz).unwrap();
                        for texto in ["Hola mundo.", "Esta es una prueba."] {
                            let chunk = engine.synthesize(texto, 1.0).unwrap();
                            assert!(!chunk.samples.is_empty());
                            total += chunk.samples.len();
                        }
                        // `engine` se libera aquí: también compite `piper_free`.
                    }
                    total
                })
            })
            .collect();
        for handle in handles {
            assert!(handle.join().unwrap() > 0);
        }
    }
}
