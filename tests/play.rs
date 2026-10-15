//! Task 11: `play` lee un EPUB en voz alta desde el terminal.
//!
//! Los tests sintetizan de verdad, así que llevan puerta: se saltan si no hay
//! `READALOUD_TEST_VOICE`. Usan `NullSink::instant()`: con un sink que nunca
//! avanza, el bucle de `play` no terminaría (Task 10).

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use readaloud::cli::play;
use readaloud::player::{NullSink, PlayerState};
use readaloud::test_support::test_voice;
use tempfile::tempdir;

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
    let cache = tempdir().unwrap();
    let s = play::run_with_in(
        &libro_de_cuatro_frases(dir.path()),
        &voz,
        1.0,
        Box::new(NullSink::instant()),
        3,
        cache.path(),
    )
    .unwrap();
    assert_eq!(s.sentences, 3, "el tope de frases corta la lectura");
    assert!(s.words_covered > 0);
    assert_eq!(s.final_state, PlayerState::Ended);
}

/// Una única llamada al envoltorio sin `_in`, que usa la caché por defecto de
/// `$HOME`: cubre ese camino. Es el único test que escribe fuera del tempdir.
#[test]
fn el_envoltorio_sin_in_usa_la_cache_por_defecto() {
    let Some(voz) = test_voice() else { return };
    let dir = tempdir().unwrap();
    let s = play::run_with(
        &libro_de_cuatro_frases(dir.path()),
        &voz,
        1.0,
        Box::new(NullSink::instant()),
        1,
    )
    .unwrap();
    assert_eq!(s.sentences, 1);
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

/// Un dispositivo que nunca avanza (el callback murió, el aparato se
/// desconectó) debe rendirse con un error legible, no girar para siempre. El
/// sink no avanza nunca; el tope de estancamiento se acorta por entorno para
/// que el test no tarde.
#[test]
fn un_dispositivo_que_no_avanza_da_error_en_vez_de_colgarse() {
    let Some(voz) = test_voice() else { return };
    // `set_var` es `unsafe` en edición 2024 por el estado global del entorno;
    // sólo lo lee el bucle de `play` y ningún otro test se estanca.
    unsafe { std::env::set_var("READALOUD_STALL_POLLS", "5") };

    let dir = tempdir().unwrap();
    let cache = tempdir().unwrap();
    let err = play::run_with_in(
        &libro_de_cuatro_frases(dir.path()),
        &voz,
        1.0,
        Box::new(NullSink::new(Arc::new(AtomicUsize::new(0)))),
        1,
        cache.path(),
    )
    .unwrap_err();

    let msg = format!("{err:#}");
    assert!(msg.contains("no avanza"), "mensaje inesperado: {msg}");
}
