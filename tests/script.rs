mod common;

use std::path::PathBuf;

use common::minimal_epub;
use readaloud::cli;
use tempfile::tempdir;

/// Voz piper para los tests que sintetizan; `None` (skip) si no está definida.
fn test_voice() -> Option<PathBuf> {
    std::env::var("READALOUD_TEST_VOICE").ok().map(Into::into)
}

#[test]
fn vuelca_una_linea_por_palabra_en_orden() {
    let dir = tempdir().unwrap();
    let path = minimal_epub(dir.path());
    let out = cli::script::run_text(&path).unwrap();
    assert_eq!(out.lines().next().unwrap(), "0\t0\t0\tHola");
    let ids: Vec<u32> = out
        .lines()
        .map(|l| l.split('\t').next().unwrap().parse().unwrap())
        .collect();
    assert_eq!(ids, (0..ids.len() as u32).collect::<Vec<_>>());
}

#[test]
fn un_archivo_que_no_es_epub_da_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("no.epub");
    std::fs::write(&path, b"esto no es un epub").unwrap();
    assert!(cli::script::run_text(&path).is_err());
}

#[test]
fn emite_timeline_valido_por_capitulo() {
    let Some(voz) = test_voice() else {
        eprintln!("skip: define READALOUD_TEST_VOICE");
        return;
    };
    let dir = tempdir().unwrap();
    let out = cli::script::run_timeline(&minimal_epub(dir.path()), &voz).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let span = &v[0]["spans"][0];
    assert!(span["start_sample"].as_u64().unwrap() <= span["end_sample"].as_u64().unwrap());
    assert!(!v[0]["spans"].as_array().unwrap().is_empty());
}
