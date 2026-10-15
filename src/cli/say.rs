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
    let chosen = select(&voices::default_dir(), voice)?;

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

/// Elige de `dir` la voz llamada `name`, o la primera descubierta si es `None`.
/// Un nombre que no existe es error, nunca cae en la primera.
fn select(dir: &Path, name: Option<&str>) -> Result<voices::Voice, TtsError> {
    let available = voices::discover(dir);
    match name {
        Some(name) => available.into_iter().find(|v| v.name == name),
        None => available.into_iter().next(),
    }
    .ok_or(TtsError::VoiceNotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_elige_la_voz_pedida_y_no_la_primera() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["aaa-voz", "zzz-voz"] {
            std::fs::write(dir.path().join(format!("{name}.onnx")), b"").unwrap();
            std::fs::write(dir.path().join(format!("{name}.onnx.json")), b"{}").unwrap();
        }

        // Sin nombre: la primera en orden.
        assert_eq!(select(dir.path(), None).unwrap().name, "aaa-voz");
        // Con nombre: la pedida, aunque no sea la primera.
        assert_eq!(select(dir.path(), Some("zzz-voz")).unwrap().name, "zzz-voz");
        // Nombre inexistente: error, no la primera.
        assert!(matches!(
            select(dir.path(), Some("no-existe")),
            Err(TtsError::VoiceNotFound)
        ));
    }
}
