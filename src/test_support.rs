//! Ayuda compartida por los tests que sintetizan de verdad.
//!
//! La voz piper se pide por entorno (`READALOUD_TEST_VOICE`) para no exigir
//! modelos de decenas de MB. Por defecto, sin voz los tests se **saltan**; con
//! `READALOUD_REQUIRE_VOICE` definida, la falta de voz es un **fallo**: un
//! `cargo test` verde sin motor no prueba nada.
//!
//! Vive en el crate (y no en `tests/`) porque los tests unitarios del motor
//! (`src/tts/engine.rs`) y los de integración deben compartir la **misma**
//! puerta: no puede saltarse en un fichero y fallar en otro.

use std::path::PathBuf;

/// Decide a partir de valores ya leídos (para poder probarlo sin tocar el
/// entorno global del proceso).
///
/// `Some(voz)` si hay voz; `None` para saltar el test; pánico si el entorno
/// exige voz y no la hay.
fn resolve(voice: Option<String>, require: bool) -> Option<PathBuf> {
    match voice {
        Some(v) if !v.is_empty() => Some(v.into()),
        _ if require => panic!(
            "READALOUD_REQUIRE_VOICE está definida pero READALOUD_TEST_VOICE no: \
             define una voz piper para que los tests del motor se ejecuten de verdad"
        ),
        _ => None,
    }
}

/// Voz piper para los tests que sintetizan, o `None` para saltárselos.
///
/// Con `READALOUD_REQUIRE_VOICE` definida y `READALOUD_TEST_VOICE` sin definir,
/// entra en pánico: el entorno exige que el motor se ejercite de verdad.
pub fn test_voice() -> Option<PathBuf> {
    let voice = std::env::var("READALOUD_TEST_VOICE").ok();
    let require = std::env::var("READALOUD_REQUIRE_VOICE").is_ok_and(|v| !v.is_empty());
    resolve(voice, require)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn con_voz_devuelve_la_voz() {
        assert_eq!(resolve(Some("/voz.onnx".into()), false), Some("/voz.onnx".into()));
        assert_eq!(resolve(Some("/voz.onnx".into()), true), Some("/voz.onnx".into()));
    }

    #[test]
    fn sin_voz_y_sin_exigir_es_skip() {
        assert_eq!(resolve(None, false), None);
        assert_eq!(resolve(Some(String::new()), false), None);
    }

    #[test]
    #[should_panic(expected = "READALOUD_REQUIRE_VOICE")]
    fn sin_voz_exigiendo_es_fallo() {
        resolve(None, true);
    }
}
