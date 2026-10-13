mod common;

use common::minimal_epub;
use readaloud::cli;
use tempfile::tempdir;

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
