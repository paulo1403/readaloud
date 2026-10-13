//! Contenedor EPUB: la única parte del motor que conoce ZIP y OPF.
//!
//! Todo lo que hay aguas abajo trabaja sobre texto plano y `word_id`, no sobre
//! el formato del libro.

mod opf;

use std::io::{Read, Seek};
use std::path::Path;

use thiserror::Error;
use zip::ZipArchive;

#[derive(Debug, Error)]
pub enum EpubError {
    #[error("el archivo no es un ZIP válido")]
    NotAZip,
    #[error("no se encontró el OPF declarado en container.xml")]
    NoOpf,
    #[error("no se encontró META-INF/container.xml")]
    NoContainer,
    #[error("XML inválido")]
    BadXml,
    #[error("falta {0}")]
    Missing(String),
}

#[derive(Debug)]
pub struct Book {
    pub title: Option<String>,
    pub author: Option<String>,
    pub lang: Option<String>,
    pub chapters: Vec<Chapter>,
}

#[derive(Debug)]
pub struct Chapter {
    pub id: String,
    pub href: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

pub fn open(path: &Path) -> Result<Book, EpubError> {
    let file = std::fs::File::open(path)
        .map_err(|e| EpubError::Missing(format!("{}: {e}", path.display())))?;
    let mut zip = ZipArchive::new(file).map_err(|_| EpubError::NotAZip)?;

    let container = read_entry(&mut zip, "META-INF/container.xml")?.ok_or(EpubError::NoContainer)?;
    let opf_path = opf::container_rootfile(&container)?;
    let opf_xml = read_entry(&mut zip, &opf_path)?.ok_or(EpubError::NoOpf)?;
    let package = opf::parse_package(&opf_xml)?;

    let mut chapters = Vec::new();
    for idref in &package.spine {
        let item = package
            .manifest
            .iter()
            .find(|i| &i.id == idref)
            .ok_or_else(|| EpubError::Missing(idref.clone()))?;
        let zip_name = resolve(&opf_path, &item.href);
        let bytes =
            read_entry(&mut zip, &zip_name)?.ok_or_else(|| EpubError::Missing(zip_name.clone()))?;
        chapters.push(Chapter {
            id: item.id.clone(),
            href: item.href.clone(),
            media_type: item.media_type.clone(),
            bytes,
        });
    }

    Ok(Book {
        title: package.title,
        author: package.author,
        lang: package.lang,
        chapters,
    })
}

/// Lee una entrada del ZIP. `Ok(None)` si no existe; `NotAZip` si el ZIP está corrupto.
fn read_entry<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Option<Vec<u8>>, EpubError> {
    match zip.by_name(name) {
        Ok(mut entry) => {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(|_| EpubError::NotAZip)?;
            Ok(Some(buf))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(_) => Err(EpubError::NotAZip),
    }
}

/// Ruta del recurso dentro del ZIP: el `href` es relativo a la carpeta del OPF.
// ponytail: sin normalizar `../` ni rutas absolutas; añadir si aparece un EPUB que lo use.
fn resolve(opf_path: &str, href: &str) -> String {
    match opf_path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{href}"),
        None => href.to_string(),
    }
}
