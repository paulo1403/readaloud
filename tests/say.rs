//! Task 6: cabecera WAV, parche ONNX persistente y `say`.
//!
//! Los tests que sintetizan llevan puerta: se saltan si no hay
//! `READALOUD_TEST_VOICE`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use readaloud::test_support::test_voice;
use readaloud::{cli, tts::voices, wav};

/// `say` lee `voices::default_dir()`, no `READALOUD_TEST_VOICE`: el test de `say`
/// se salta si ese directorio no tiene voces, para que la puerta y la búsqueda
/// coincidan y no falle por el entorno.
fn hay_voces_en_default_dir() -> bool {
    !voices::discover(&voices::default_dir()).is_empty()
}

/// Copia la voz a un temporal como `voz.onnx` (+ su `.onnx.json`) para
/// parchearla sin tocar el original.
fn copia_temporal_de(voz: &Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(voz, dir.path().join("voz.onnx")).unwrap();
    let mut config = voz.as_os_str().to_os_string();
    config.push(".json");
    std::fs::copy(Path::new(&config), dir.path().join("voz.onnx.json")).unwrap();
    dir
}

/// Ruta única para que `say` escriba; no usa `TempDir` para que el fichero no
/// desaparezca antes de leerlo.
fn tempfile() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("readaloud-say-{}-{n}.wav", std::process::id()))
}

#[test]
fn el_wav_tiene_cabecera_valida() {
    let mut buf = Vec::new();
    wav::write_pcm16_mono(&mut buf, &[0i16; 100], 22050).unwrap();
    assert_eq!(&buf[0..4], b"RIFF");
    assert_eq!(&buf[8..12], b"WAVE");
    assert_eq!(&buf[36..40], b"data");
    assert_eq!(u32::from_le_bytes(buf[40..44].try_into().unwrap()), 200);
}

#[test]
fn prepare_marca_el_tensor_de_alineacion() {
    let Some(voz) = test_voice() else { return };
    let dir = copia_temporal_de(&voz);
    voices::prepare(dir.path(), "voz").unwrap();
    assert!(voices::is_prepared(&dir.path().join("voz.onnx")).unwrap());
    voices::prepare(dir.path(), "voz").unwrap(); // idempotente
}

#[test]
fn say_escribe_un_wav_no_vacio() {
    if !hay_voces_en_default_dir() {
        return;
    }
    let out = tempfile();
    cli::say::run("Hola mundo.", Some(&out), 1.0, None).unwrap();
    assert!(std::fs::metadata(&out).unwrap().len() > 44);
    let _ = std::fs::remove_file(&out);
}
