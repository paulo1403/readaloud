//! Parseo XML del contenedor y del paquete OPF.

use crate::epub::EpubError;
use roxmltree::Document;

const DC: &str = "http://purl.org/dc/elements/1.1/";

pub struct Item {
    pub id: String,
    pub href: String,
    pub media_type: String,
}

pub struct Package {
    pub title: Option<String>,
    pub author: Option<String>,
    pub lang: Option<String>,
    pub manifest: Vec<Item>,
    pub spine: Vec<String>,
}

/// `full-path` del `rootfile` en `META-INF/container.xml`.
pub fn container_rootfile(xml: &[u8]) -> Result<String, EpubError> {
    let doc = parse(xml)?;
    doc.descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "rootfile")
        .and_then(|n| n.attribute("full-path"))
        .map(str::to_string)
        .ok_or(EpubError::NoOpf)
}

/// Metadatos, manifest y spine (saltando `linear="no"`) del OPF.
pub fn parse_package(xml: &[u8]) -> Result<Package, EpubError> {
    let doc = parse(xml)?;

    let manifest = doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "item")
        .filter_map(|n| {
            Some(Item {
                id: n.attribute("id")?.to_string(),
                href: n.attribute("href")?.to_string(),
                media_type: n.attribute("media-type").unwrap_or_default().to_string(),
            })
        })
        .collect();

    let spine = doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "itemref")
        .filter(|n| n.attribute("linear") != Some("no"))
        .filter_map(|n| n.attribute("idref").map(str::to_string))
        .collect();

    Ok(Package {
        title: dc(&doc, "title"),
        author: dc(&doc, "creator"),
        lang: dc(&doc, "language"),
        manifest,
        spine,
    })
}

fn parse(xml: &[u8]) -> Result<Document<'_>, EpubError> {
    let text = std::str::from_utf8(xml).map_err(|_| EpubError::BadXml)?;
    Document::parse(text).map_err(|_| EpubError::BadXml)
}

fn dc(doc: &Document<'_>, name: &str) -> Option<String> {
    doc.descendants()
        .find(|n| {
            n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(DC)
        })
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
}
