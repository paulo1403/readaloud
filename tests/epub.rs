mod common;

use common::{minimal_epub, write_zip};
use readaloud::epub;
use tempfile::tempdir;

#[test]
fn lee_metadatos_y_spine_lineal() {
    let dir = tempdir().unwrap();
    let path = minimal_epub(dir.path());
    let book = epub::open(&path).unwrap();
    assert_eq!(book.title.as_deref(), Some("Fixture"));
    assert_eq!(book.lang.as_deref(), Some("es"));
    assert_eq!(book.chapters.len(), 1, "linear=\"no\" se salta");
    assert_eq!(book.chapters[0].href, "cap1.xhtml");
}

#[test]
fn un_archivo_que_no_es_zip_da_error_no_panic() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("no.epub");
    std::fs::write(&p, b"esto no es un zip").unwrap();
    assert!(matches!(epub::open(&p), Err(epub::EpubError::NotAZip)));
}

#[test]
fn zip_sin_opf_da_error_no_panic() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("vacio.epub");
    write_zip(
        &p,
        &[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", common::CONTAINER_XML),
        ],
    );
    assert!(matches!(epub::open(&p), Err(epub::EpubError::NoOpf)));
}
