//! `say`: sintetiza un texto a un WAV (o a stdout).

use std::path::Path;

use crate::tts::{Engine, TtsError, voices};
use crate::wav;

/// Sintetiza `text` y escribe el WAV en `out`, o en stdout si `out` es `None`.
///
/// `voice` es el nombre de la voz (sin `.onnx`); si es `None`, se usa la primera
/// descubierta en `voices::default_dir()`.
pub fn run(
    text: &str,
    out: Option<&Path>,
    length_scale: f32,
    voice: Option<&str>,
) -> anyhow::Result<()> {
    let dir = voices::default_dir();
    let available = voices::discover(&dir);
    let chosen = match voice {
        Some(name) => available
            .iter()
            .find(|v| v.name == name)
            .ok_or(TtsError::VoiceNotFound)?,
        None => available.first().ok_or(TtsError::VoiceNotFound)?,
    };

    let mut engine = Engine::load(&chosen.onnx)?;
    let chunk = engine.synthesize(text, length_scale)?;

    match out {
        Some(path) => {
            let mut file = std::fs::File::create(path)?;
            wav::write_pcm16_mono(&mut file, &chunk.samples, chunk.rate)?;
        }
        None => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            wav::write_pcm16_mono(&mut lock, &chunk.samples, chunk.rate)?;
        }
    }
    Ok(())
}
