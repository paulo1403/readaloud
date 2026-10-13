use readaloud::doc;
use readaloud::epub;

/// Construye un `Book` en memoria: cada `&str` es el cuerpo de un capítulo, o un
/// documento XHTML completo si ya trae un elemento `<html>` (p. ej. con DOCTYPE).
fn build_from_html(bodies: &[&str]) -> doc::Document {
    let chapters = bodies
        .iter()
        .enumerate()
        .map(|(i, body)| {
            let bytes = if body.contains("<html") {
                body.to_string()
            } else {
                format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>{body}</body></html>"#)
            };
            epub::Chapter {
                id: format!("c{i}"),
                href: format!("c{i}.xhtml"),
                media_type: "application/xhtml+xml".to_string(),
                bytes: bytes.into_bytes(),
            }
        })
        .collect();
    let book = epub::Book {
        title: None,
        author: None,
        lang: None,
        chapters,
    };
    doc::build(&book).expect("build del documento")
}

#[test]
fn ids_contiguos_y_en_orden_de_lectura() {
    let d = build_from_html(&["<p>Hola mundo</p><p>Otra frase</p>"]);
    let ids: Vec<u32> = d.words().iter().map(|w| w.id).collect();
    assert_eq!(ids, vec![0, 1, 2, 3]);
    assert_eq!(d.words()[0].text, "Hola");
    assert_eq!(d.words()[2].paragraph, 1);
}

#[test]
fn decodifica_entidades_y_conserva_la_puntuacion() {
    let d = build_from_html(&["<p>Pan &amp; vino, &#233;nfasis.</p>"]);
    assert_eq!(d.words()[0].text, "Pan");
    assert_eq!(d.words()[1].text, "&");
    assert_eq!(d.words()[2].text, "vino,");
}

#[test]
fn un_token_de_solo_puntuacion_no_es_palabra() {
    let d = build_from_html(&["<p>Fin — luego</p>"]);
    let textos: Vec<&str> = d.words().iter().map(|w| w.text.as_str()).collect();
    assert_eq!(textos, vec!["Fin", "luego"]);
}

#[test]
fn xhtml_mal_formado_no_pierde_el_texto() {
    let d = build_from_html(&["<p>Uno <b>dos<p>Tres"]);
    assert!(d.words().len() >= 3);
    assert_eq!(d.coverage_bad_xhtml, 1);
}

#[test]
fn sin_espacios_no_rompe() {
    let d = build_from_html(&["<p>这是一个没有空格的段落</p>"]);
    assert_eq!(d.words().len(), 1);
}

#[test]
fn capitulo_sin_texto_deja_la_secuencia_intacta() {
    let d = build_from_html(&["<p>Antes</p>", "<img src=\"x.png\"/>", "<p>Después</p>"]);
    let ids: Vec<u32> = d.words().iter().map(|w| w.id).collect();
    assert_eq!(ids, vec![0, 1]);
}

#[test]
fn una_entidad_html_comun_no_cuenta_como_xhtml_roto() {
    let d = build_from_html(&["<p>Uno&nbsp;dos&nbsp;tres</p>"]);
    assert_eq!(d.coverage_bad_xhtml, 0);
    let textos: Vec<&str> = d.words().iter().map(|w| w.text.as_str()).collect();
    assert_eq!(textos, vec!["Uno", "dos", "tres"]);
}

#[test]
fn una_entidad_de_simbolo_es_palabra_pero_una_raya_suelta_no() {
    let d = build_from_html(&["<p>Pan &amp; vino — luego</p>"]);
    let textos: Vec<&str> = d.words().iter().map(|w| w.text.as_str()).collect();
    assert_eq!(textos, vec!["Pan", "&", "vino", "luego"]);
}

#[test]
fn un_doctype_no_cuenta_como_xhtml_roto() {
    let d =
        build_from_html(&["<!DOCTYPE html><html><body><p>Con doctype y texto</p></body></html>"]);
    assert_eq!(d.coverage_bad_xhtml, 0);
    let textos: Vec<&str> = d.words().iter().map(|w| w.text.as_str()).collect();
    assert_eq!(textos, vec!["Con", "doctype", "y", "texto"]);
}
