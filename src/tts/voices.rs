//! Registro de voces piper: descubrirlas y preparar su alineación por fonema.

use std::fs;
use std::path::{Path, PathBuf};

use onnx_protobuf::{Message, ModelProto};

use super::{TtsError, alignment, config_path};

/// Una voz piper instalada: `<name>.onnx` con su `<name>.onnx.json` hermano.
#[derive(Debug, Clone)]
pub struct Voice {
    pub name: String,
    pub lang: String,
    pub onnx: PathBuf,
}

/// Directorio por defecto de voces.
///
/// `~/.local/share/readaloud/voices/`; si no existe, se usa
/// `~/.local/share/piper-voices/` (donde suelen vivir las voces piper ya
/// instaladas). Si no existe ninguno, se devuelve el primero.
pub fn default_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let readaloud = home.join(".local/share/readaloud/voices");
    if readaloud.is_dir() {
        return readaloud;
    }
    let piper = home.join(".local/share/piper-voices");
    if piper.is_dir() {
        return piper;
    }
    readaloud
}

/// Lista las voces de `dir`: cada `*.onnx` con su `.onnx.json` hermano,
/// ordenadas por nombre. Un `dir` ilegible da una lista vacía.
pub fn discover(dir: &Path) -> Vec<Voice> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut voices: Vec<Voice> = entries
        .flatten()
        .filter_map(|entry| {
            let onnx = entry.path();
            if onnx.extension().and_then(|e| e.to_str()) != Some("onnx") {
                return None;
            }
            if !config_path(&onnx).is_file() {
                return None;
            }
            let name = onnx.file_stem()?.to_string_lossy().into_owned();
            Some(Voice {
                lang: lang_of(&name),
                name,
                onnx,
            })
        })
        .collect();
    voices.sort_by(|a, b| a.name.cmp(&b.name));
    voices
}

/// Parchea `dir/<name>.onnx` en su sitio para exponer la salida de alineación.
///
/// Idempotente: una voz ya parcheada (o sin nodo `Ceil`, nada que parchear) no
/// se reescribe.
pub fn prepare(dir: &Path, name: &str) -> Result<(), TtsError> {
    let onnx = dir.join(format!("{name}.onnx"));
    let bytes = fs::read(&onnx).map_err(|_| TtsError::VoiceNotFound)?;
    let mut model = ModelProto::parse_from_bytes(&bytes).map_err(|_| TtsError::Engine)?;
    if !alignment::add_alignment_output(&mut model) {
        return Ok(());
    }
    let patched = model.write_to_bytes().map_err(|_| TtsError::Engine)?;
    fs::write(&onnx, patched).map_err(|_| TtsError::Engine)?;
    Ok(())
}

/// ¿Expone ya `onnx` la salida de alineación?
pub fn is_prepared(onnx: &Path) -> Result<bool, TtsError> {
    let bytes = fs::read(onnx).map_err(|_| TtsError::VoiceNotFound)?;
    let model = ModelProto::parse_from_bytes(&bytes).map_err(|_| TtsError::Engine)?;
    Ok(alignment::has_alignment_output(&model))
}

/// Idioma a partir del nombre de la voz: `es_ES-davefx-medium` → `es-ES`.
fn lang_of(name: &str) -> String {
    name.split('-').next().unwrap_or(name).replace('_', "-")
}
