//! Síntesis de voz (piper) y la costura `PhonemeChunk` del motor.
//!
//! `PhonemeChunk` es la frontera pura del proyecto: `timeline/` construye el
//! mapeo palabra → samples sobre él, sin conocer audio, dispositivo ni piper.
//!
//! La semántica exacta de los vectores de `PhonemeChunk` está documentada en
//! el propio struct: es el contrato que consume `timeline/`.

mod alignment;
pub mod cache;
mod engine;
pub mod voices;

pub use engine::Engine;

use std::path::{Path, PathBuf};

/// Ruta del `.onnx.json` hermano de una voz `.onnx`.
pub(crate) fn config_path(onnx: &Path) -> PathBuf {
    let mut name = onnx.as_os_str().to_os_string();
    name.push(".json");
    PathBuf::from(name)
}

/// Trozo de audio con su alineación por fonema.
///
/// `phonemes`, `phoneme_ids` y `alignments` son tres arrays **paralelos de la
/// misma longitud**: `phonemes[i]` corresponde a `phoneme_ids[i]` y a
/// `alignments[i]`. Los separadores internos de la API C (codepoint `0`) ya se
/// han eliminado.
///
/// # Cómo lo recorre un consumidor
///
/// Para traducir esto a tiempos por palabra, se recorre `i` en lockstep sobre
/// los tres arrays:
///
/// 1. **Extremos.** `phonemes[0]` es `'^'` (id `1`, inicio de frase) y
///    `phonemes[1]` es `'^'` (id `0`, su relleno); la última entrada es `'$'`
///    (id `2`, fin de frase). Ni el par de BOS ni el EOS cuentan para la
///    alineación de ninguna palabra: se saltan.
/// 2. **Cada fonema real aparece dos veces**: una con su id de modelo y otra con
///    el id de relleno `0`, de modo que `alignments` reparte la duración del
///    fonema entre esas dos entradas. Hay que sumar **todas** las entradas de un
///    grupo, rellenos incluidos; filtrar los rellenos reduciría a la mitad la
///    duración de cada fonema.
/// 3. **El límite de palabra es una tirada de `' '`**: el fonema espacio emite
///    su entrada real y su relleno, es decir dos espacios consecutivos (no un
///    espacio suelto).
/// 4. **Invariante:** `sum(alignments) == samples.len()`. La duración de una
///    palabra en samples es la suma de `alignments[i]` de todas las entradas
///    entre el límite anterior y el siguiente.
///
/// Ejemplo (`"Hola mundo."`, recortado): `phonemes = ['^','^','ˈ','ˈ','o','o',
/// …,' ',' ','m','m',…,'$']`, con los rellenos (`id 0`) intercalados.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PhonemeChunk {
    /// Muestras PCM mono a 16 bits.
    pub samples: Vec<i16>,
    /// Frecuencia de muestreo en Hz.
    pub rate: u32,
    /// Codepoint de cada id de fonema (incluidos BOS `'^'`, EOS `'$'` y los
    /// codepoints repetidos de los rellenos).
    pub phonemes: Vec<char>,
    /// Id de fonema del modelo para cada entrada (`1` BOS, `2` EOS, `0` relleno).
    pub phoneme_ids: Vec<i32>,
    /// Samples de audio atribuidos a cada entrada; sumar **todas** las de un
    /// grupo (rellenos incluidos). `sum(alignments) == samples.len()`.
    pub alignments: Vec<i32>,
}

/// Error del subsistema TTS.
#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("voz no encontrada")]
    VoiceNotFound,
    #[error("no se pudo cargar el motor piper")]
    Engine,
    #[error("fallo de síntesis")]
    Synth,
}
