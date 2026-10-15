//! Task 11: `play` lee un EPUB en voz alta desde el terminal.
//!
//! Los tests sintetizan de verdad, así que llevan puerta: se saltan si no hay
//! `READALOUD_TEST_VOICE`. Usan `NullSink::instant()`: con un sink que nunca
//! avanza, el bucle de `play` no terminaría (Task 10).

mod common;

use std::path::{Path, PathBuf};

use readaloud::cli::play;
use readaloud::player::{NullSink, PlayerState};
use tempfile::tempdir;

/// Voz piper para los tests que sintetizan; `None` (skip) si no está definida.
fn test_voice() -> Option<PathBuf> {
    std::env::var("READALOUD_TEST_VOICE").ok().map(Into::into)
}

/// Ficheros escritos en la caché (`.pcm` + `.json` por entrada).
fn count_entries(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|it| it.count()).unwrap_or(0)
}

/// Un libro de cuatro frases para que el tope de tres frases sea observable.
fn libro_de_cuatro_frases(dir: &Path) -> PathBuf {
    common::one_chapter_epub(dir, "<p>Uno. Dos. Tres. Cuatro.</p>")
}

#[test]
fn lee_tres_frases_sin_dispositivo_de_audio() {
    let Some(voz) = test_voice() else { return };
    let dir = tempdir().unwrap();
    // `run_with` usa la caché por defecto (`$HOME/.cache/readaloud/audio`); las
    // entradas quedan fuera del tempdir, pero el camino real del envoltorio se
    // ejercita.
    let s = play::run_with(
        &libro_de_cuatro_frases(dir.path()),
        &voz,
        1.0,
        Box::new(NullSink::instant()),
        3,
    )
    .unwrap();
    assert_eq!(s.sentences, 3, "el tope de frases corta la lectura");
    assert!(s.words_covered > 0);
    assert_eq!(s.final_state, PlayerState::Ended);
}

#[test]
fn la_cache_evita_resintetizar() {
    let Some(voz) = test_voice() else { return };
    let dir = tempdir().unwrap();
    let cache = tempdir().unwrap();
    let libro = libro_de_cuatro_frases(dir.path());

    let a = play::run_with_in(
        &libro,
        &voz,
        1.0,
        Box::new(NullSink::instant()),
        3,
        cache.path(),
    )
    .unwrap();
    let tras_la_primera = count_entries(cache.path());
    assert!(tras_la_primera > 0, "la primera pasada escribe la caché");

    let b = play::run_with_in(
        &libro,
        &voz,
        1.0,
        Box::new(NullSink::instant()),
        3,
        cache.path(),
    )
    .unwrap();
    assert_eq!(a.sentences, b.sentences);
    assert_eq!(a.words_covered, b.words_covered);
    assert_eq!(
        count_entries(cache.path()),
        tras_la_primera,
        "la segunda pasada no escribe nada"
    );
}
