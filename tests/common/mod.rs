//! Fixtures EPUB compartidos por los tests de integración.
#![allow(dead_code)]

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

const OPF: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:fixture</dc:identifier>
    <dc:title>Fixture</dc:title>
    <dc:creator>Autor</dc:creator>
    <dc:language>es</dc:language>
  </metadata>
  <manifest>
    <item id="cap1" href="cap1.xhtml" media-type="application/xhtml+xml"/>
    <item id="cap2" href="cap2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="cap1"/>
    <itemref idref="cap2" linear="no"/>
  </spine>
</package>
"#;

const CAP1: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <head><title>Uno</title></head>
  <body><p>Hola mundo</p></body>
</html>
"#;

const CAP2: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <head><title>Dos</title></head>
  <body><p>Adiós mundo</p></body>
</html>
"#;

/// Escribe un ZIP con las entradas dadas, todas sin comprimir.
pub fn write_zip(path: &Path, entries: &[(&str, &str)]) -> PathBuf {
    let file = File::create(path).expect("crear zip");
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, content) in entries {
        zip.start_file(*name, opts).expect("entrada zip");
        zip.write_all(content.as_bytes()).expect("contenido zip");
    }
    zip.finish().expect("cerrar zip");
    path.to_path_buf()
}

/// EPUB mínimo: `mimetype` primero y sin comprimir, `container.xml` -> `OEBPS/content.opf`,
/// spine con `cap1.xhtml` lineal y `cap2.xhtml` con `linear="no"`.
pub fn minimal_epub(dir: &Path) -> PathBuf {
    let path = dir.join("fixture.epub");
    write_zip(
        &path,
        &[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", CONTAINER_XML),
            ("OEBPS/content.opf", OPF),
            ("OEBPS/cap1.xhtml", CAP1),
            ("OEBPS/cap2.xhtml", CAP2),
        ],
    )
}

/// EPUB de un único capítulo lineal con el cuerpo XHTML dado. Controla cuántas
/// frases tiene el libro sin tocar el fixture compartido.
pub fn one_chapter_epub(dir: &Path, body: &str) -> PathBuf {
    const OPF_UNA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:fixture-una</dc:identifier>
    <dc:title>Una</dc:title>
    <dc:language>es</dc:language>
  </metadata>
  <manifest>
    <item id="cap" href="cap.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="cap"/>
  </spine>
</package>
"#;
    let cap = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <head><title>Uno</title></head>
  <body>{body}</body>
</html>
"#
    );
    write_zip(
        &dir.join("una.epub"),
        &[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", CONTAINER_XML),
            ("OEBPS/content.opf", OPF_UNA),
            ("OEBPS/cap.xhtml", &cap),
        ],
    )
}
