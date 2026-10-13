//! Síntesis de voz (piper) y la costura `PhonemeChunk` del motor.
//!
//! `PhonemeChunk` es la frontera pura del proyecto: `timeline/` construye el
//! mapeo palabra → samples sobre él, sin conocer audio, dispositivo ni piper.
//!
//! Semántica de los vectores de `PhonemeChunk`: son tres arrays **paralelos**,
//! un elemento por id de fonema del modelo (los ids de relleno incluidos), de
//! modo que `phonemes[i]` corresponde a `phoneme_ids[i]` y a `alignments[i]`.
//! Los separadores internos de la API C (codepoint 0) se han eliminado; los
//! límites de palabra quedan visibles como codepoints `' '`.

mod engine;
pub use engine::Engine;

/// Trozo de audio con su alineación por fonema.
///
/// `phonemes`, `phoneme_ids` y `alignments` tienen la misma longitud y están
/// indexados de forma paralela.
#[derive(Debug, Clone)]
pub struct PhonemeChunk {
    /// Muestras PCM mono a 16 bits.
    pub samples: Vec<i16>,
    /// Frecuencia de muestreo en Hz.
    pub rate: u32,
    /// Codepoint de cada id de fonema.
    pub phonemes: Vec<char>,
    /// Id de fonema del modelo para cada muestra de `phonemes`.
    pub phoneme_ids: Vec<i32>,
    /// Número de samples de audio atribuidos a cada id de fonema.
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
