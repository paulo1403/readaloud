//! `script`: vuelca el libro como texto (`--text`) o como mapeo palabra →
//! samples (`--timeline`).

use std::fmt::Write;
use std::ops::Range;
use std::path::Path;

use serde::Serialize;

use crate::timeline;
use crate::tts::Engine;
use crate::{doc, epub};

/// Una línea por palabra: `word_id\tchapter\tparagraph\ttext`.
pub fn run_text(path: &Path) -> anyhow::Result<String> {
    let book = epub::open(path)?;
    let document = doc::build(&book)?;

    let mut out = String::new();
    for word in &document.words {
        writeln!(
            out,
            "{}\t{}\t{}\t{}",
            word.id, word.chapter, word.paragraph, word.text
        )?;
    }
    Ok(out)
}

/// Tope de caracteres por frase sintetizada: acota el audio y la latencia.
const MAX_FRASE: usize = 400;

/// Un capítulo del libro con su mapeo `word_id → samples`.
#[derive(Serialize)]
struct ChapterTimeline<'a> {
    chapter: u32,
    href: &'a str,
    spans: Vec<timeline::Span>,
}

/// Mapeo `word_id → samples` de todo el libro, en JSON: un objeto por capítulo
/// con `chapter`, `href` y `spans` (`word_id`, `start_sample`, `end_sample`,
/// `exact`). Los `word_id` son globales; los samples se acumulan dentro del
/// capítulo, de modo que los spans son monótonos y sin solape.
///
/// Carga **una** voz (libpiper no es reentrante) y sintetiza por frase: un grupo
/// por párrafo hasta puntuación final o `MAX_FRASE` caracteres.
pub fn run_timeline(path: &Path, voice_onnx: &Path) -> anyhow::Result<String> {
    let book = epub::open(path)?;
    let document = doc::build(&book)?;
    let mut engine = Engine::load(voice_onnx)?;

    let mut chapters: Vec<ChapterTimeline> = Vec::with_capacity(book.chapters.len());
    for (index, chapter) in book.chapters.iter().enumerate() {
        let words = chapter_words(&document.words, index as u32);
        let mut spans = Vec::new();
        let mut offset = 0usize;
        for group in grupos_de_frase(words) {
            let group_words = &words[group];
            let text = group_words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let chunk = engine.synthesize(&text, 1.0)?;
            for mut span in timeline::build(&chunk, group_words).spans {
                span.start_sample += offset;
                span.end_sample += offset;
                spans.push(span);
            }
            offset += chunk.samples.len();
        }
        chapters.push(ChapterTimeline {
            chapter: index as u32,
            href: &chapter.href,
            spans,
        });
    }
    Ok(serde_json::to_string_pretty(&chapters)?)
}

/// Las palabras de un capítulo: contiguas en `words`, porque `doc::build` las
/// añade capítulo a capítulo en orden.
fn chapter_words(words: &[doc::Word], chapter: u32) -> &[doc::Word] {
    let start = words.partition_point(|w| w.chapter < chapter);
    let end = words.partition_point(|w| w.chapter <= chapter);
    &words[start..end]
}

/// Corta las palabras de un capítulo en frases sintetizables: cada párrafo se
/// parte al alcanzar puntuación final o `MAX_FRASE` caracteres. Devuelve
/// rangos sobre `words`.
fn grupos_de_frase(words: &[doc::Word]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut len = 0usize;
    for (i, word) in words.iter().enumerate() {
        // Fin de párrafo: cierra la frase abierta antes de empezar la siguiente.
        if i > start && word.paragraph != words[i - 1].paragraph {
            out.push(start..i);
            start = i;
            len = 0;
        }
        if len > 0 {
            len += 1; // el espacio que separa de la palabra anterior
        }
        len += word.text.chars().count();
        if termina_frase(&word.text) || len >= MAX_FRASE {
            out.push(start..i + 1);
            start = i + 1;
            len = 0;
        }
    }
    if start < words.len() {
        out.push(start..words.len());
    }
    out
}

/// ¿La palabra cierra frase? Su último carácter es `. ! ? … :`, quizá tras
/// comillas o cierres de paréntesis/corchetes.
fn termina_frase(text: &str) -> bool {
    for c in text.trim_end().chars().rev() {
        if matches!(c, '"' | '\'' | '»' | '”' | '’' | ')' | ']' | '}') {
            continue;
        }
        return matches!(c, '.' | '!' | '?' | '…' | ':');
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palabra(paragraph: u32, text: &str) -> doc::Word {
        doc::Word {
            id: 0,
            chapter: 0,
            paragraph,
            text: text.to_string(),
            range: 0..0,
        }
    }

    #[test]
    fn agrupa_por_puntuacion_final_y_por_parrafo() {
        let words = [
            palabra(0, "Hola"),
            palabra(0, "mundo."),
            palabra(0, "Otra"),
            palabra(0, "frase"),
            palabra(0, "sin"),
            palabra(0, "punto"),
            palabra(1, "Nuevo"),
            palabra(1, "párrafo."),
        ];
        assert_eq!(grupos_de_frase(&words), vec![0..2, 2..6, 6..8]);
    }

    #[test]
    fn corta_al_llegar_al_tope_de_caracteres() {
        // 200 + 1 + 200 = 401 > 400: cierra a las dos palabras.
        let largo = "a".repeat(200);
        let words = [
            palabra(0, &largo),
            palabra(0, &largo),
            palabra(0, &largo),
            palabra(0, &largo),
        ];
        assert_eq!(grupos_de_frase(&words), vec![0..2, 2..4]);
    }

    #[test]
    fn reconoce_puntuacion_final_tras_comillas() {
        assert!(termina_frase("dijo:"));
        assert!(termina_frase("¿qué?"));
        assert!(termina_frase("sí…"));
        assert!(termina_frase("adiós.\""));
        assert!(termina_frase("fin.»"));
        assert!(!termina_frase("sin"));
        assert!(!termina_frase("coma,"));
    }
}
