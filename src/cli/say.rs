//! `say`: sintetiza un texto a un WAV (o a stdout).

use std::path::Path;

use crate::tts::{Engine, TtsError, voices};
use crate::wav;

/// Sintetiza `text` con la primera voz disponible y escribe el WAV en `out`,
/// o en stdout si `out` es `None`.
pub fn run(text: &str, out: Option<&Path>, length_scale: f32) -> anyhow::Result<()> {
    let dir = voices::default_dir();
    let voice = voices::discover(&dir)
        .into_iter()
        .next()
        .ok_or(TtsError::VoiceNotFound)?;

    let mut engine = Engine::load(&voice.onnx)?;
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
