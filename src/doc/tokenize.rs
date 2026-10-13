//! Texto XHTML → texto plano → tokens.
//!
//! El tokenizador es deliberadamente tonto: parte por espacios y se queda con
//! los tokens que parecen palabras. No inventa resaltado ni segmenta idiomas.

use std::ops::Range;

/// Un token maximal sin espacios dentro del texto decodificado de un capítulo.
pub(crate) struct Token {
    /// Texto decodificado del token (`&amp;` → `&`).
    pub text: String,
    /// Span de bytes del token completo dentro del texto del capítulo.
    pub range: Range<usize>,
    /// ¿El token (en su forma cruda, antes de decodificar entidades) tenía algún
    /// alfanumérico? `&amp;` cuenta como palabra porque en el origen lee "amp".
    pub has_alnum: bool,
    /// ¿Venía precedido de un salto de párrafo?
    pub paragraph_start: bool,
}

/// Convierte los bytes de un capítulo en (texto decodificado, tokens, ¿XML válido?).
///
/// Un XHTML mal formado nunca propaga el error: se degrada a un eliminador de
/// etiquetas y se marca `valid = false`.
pub(crate) fn tokenize(bytes: &[u8]) -> (String, Vec<Token>, bool) {
    let raw = String::from_utf8_lossy(bytes);
    let (raw_text, valid) = match roxmltree::Document::parse(&raw) {
        Ok(doc) => (extract(doc.root_element(), &raw), true),
        Err(_) => (strip_tags(&raw), false),
    };
    let (text, tokens) = split(&raw_text);
    (text, tokens, valid)
}

/// ¿La etiqueta cierra párrafo?
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "br" | "p" | "div" | "li" | "blockquote" | "td" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
    )
}

/// Recorre los nodos de texto del árbol, ignorando `script`, `style` y `head`,
/// e inserta `\n` al entrar en un elemento de bloque. Devuelve el texto *crudo*
/// (con entidades sin decodificar); el parseo de roxmltree ya las decodificó,
/// así que se trocea el origen por rango para poder clasificar el token original.
fn extract(node: roxmltree::Node, src: &str) -> String {
    let mut out = String::new();
    walk(node, src, &mut out);
    out
}

fn walk(node: roxmltree::Node, src: &str, out: &mut String) {
    for child in node.children() {
        if child.is_text() {
            if let Some(s) = src.get(child.range()) {
                out.push_str(s);
            }
        } else if child.is_element() {
            let name = child.tag_name().name();
            if matches!(name, "script" | "style" | "head") {
                continue;
            }
            if is_block(name) {
                out.push('\n');
            }
            walk(child, src, out);
        }
    }
}

/// Fallback tolerante: eliminador de etiquetas sobre HTML crudo. Inserta `\n` en
/// las mismas etiquetas de bloque y salta el contenido de `script`/`style`/`head`.
// ponytail: sin comentarios `<!--` ni CDATA; añadir si un EPUB roto los necesita.
fn strip_tags(raw: &str) -> String {
    let mut out = String::new();
    let mut skip: Option<String> = None;
    let mut chars = raw.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c != '<' {
            if skip.is_none() {
                out.push(c);
            }
            continue;
        }
        let mut tag = String::new();
        for (_, tc) in chars.by_ref() {
            if tc == '>' {
                break;
            }
            tag.push(tc);
        }
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect::<String>()
            .to_ascii_lowercase();
        match &skip {
            Some(open) => {
                if closing && &name == open {
                    skip = None;
                }
            }
            None if !closing && matches!(name.as_str(), "script" | "style" | "head") => {
                skip = Some(name);
            }
            None if is_block(&name) => out.push('\n'),
            None => {}
        }
    }
    out
}

/// Parte el texto crudo por espacios, decodifica cada token y arma el texto
/// decodificado del capítulo. Los rangos son spans de bytes en ese texto.
fn split(raw: &str) -> (String, Vec<Token>) {
    let mut text = String::new();
    let mut tokens = Vec::new();
    let mut paragraph_start = false;
    let mut chars = raw.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c.is_whitespace() {
            if c == '\n' {
                paragraph_start = true;
            }
            text.push(c);
            continue;
        }
        let mut raw_token = String::new();
        raw_token.push(c);
        while let Some(&(_, next)) = chars.peek() {
            if next.is_whitespace() {
                break;
            }
            chars.next();
            raw_token.push(next);
        }
        let has_alnum = raw_token.chars().any(char::is_alphanumeric);
        let decoded = decode_entities(&raw_token);
        let start = text.len();
        text.push_str(&decoded);
        let end = text.len();
        tokens.push(Token {
            text: decoded,
            range: start..end,
            has_alnum,
            paragraph_start: std::mem::take(&mut paragraph_start),
        });
    }
    (text, tokens)
}

/// Decodifica entidades XML/named conocidas; lo desconocido se deja literal.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        if let Some(semi) = rest.find(';')
            && let Some(c) = decode_entity(&rest[1..semi])
        {
            out.push(c);
            rest = &rest[semi + 1..];
            continue;
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

fn decode_entity(e: &str) -> Option<char> {
    match e {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => {
            let code = if let Some(hex) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                e.strip_prefix('#').and_then(|d| d.parse().ok())
            };
            code.and_then(char::from_u32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodifica_amp_y_numericas() {
        assert_eq!(
            decode_entities("Pan &amp; vino, &#233;nfasis."),
            "Pan & vino, énfasis."
        );
        assert_eq!(decode_entities("&#x41;&#66;"), "AB");
        assert_eq!(decode_entities("suelto & x"), "suelto & x");
    }

    #[test]
    fn separa_por_espacios_y_marca_parrafos() {
        let (text, tokens) = split("\nHola mundo\nOtra");
        let textos: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(textos, vec!["Hola", "mundo", "Otra"]);
        assert!(tokens[0].paragraph_start);
        assert!(!tokens[1].paragraph_start);
        assert!(tokens[2].paragraph_start);
        assert_eq!(&text[tokens[0].range.clone()], "Hola");
    }
}
