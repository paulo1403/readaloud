//! Documento: capítulos en texto plano y `word_id` globales, contiguos y en
//! orden de lectura. Es la única fuente de verdad de "qué es una palabra".

mod tokenize;

use std::ops::Range;

use thiserror::Error;

use crate::epub;

#[derive(Debug, Error)]
pub enum DocError {
    #[error("el libro tiene más palabras de las que caben en un word_id de 32 bits")]
    TooManyWords,
}

/// Un capítulo convertido a texto plano. Los `Word.range` apuntan aquí.
#[derive(Debug)]
pub struct ChapterText {
    pub id: u32,
    pub text: String,
}

/// Una palabra resaltable. `range` es el span de bytes dentro del
/// `ChapterText::text` de su capítulo; puede cubrir puntuación adyacente
/// fusionada (regla 5), nunca el token siguiente.
#[derive(Debug)]
pub struct Word {
    pub id: u32,
    pub chapter: u32,
    pub paragraph: u32,
    pub text: String,
    pub range: Range<usize>,
}

#[derive(Debug)]
pub struct Document {
    pub chapters: Vec<ChapterText>,
    pub words: Vec<Word>,
    /// Capítulos cuyo XHTML no parseó y cayeron al eliminador de etiquetas.
    pub coverage_bad_xhtml: u32,
}

impl Document {
    pub fn words(&self) -> &[Word] {
        &self.words
    }

    pub fn word(&self, id: u32) -> Option<&Word> {
        self.words.get(id as usize)
    }
}

/// Convierte un libro en capítulos de texto y una secuencia estable de palabras.
///
/// Nunca falla por contenido: un capítulo ilegible degrada. El único error es
/// agotar el espacio de `word_id`.
pub fn build(book: &epub::Book) -> Result<Document, DocError> {
    let mut chapters = Vec::with_capacity(book.chapters.len());
    let mut words: Vec<Word> = Vec::new();
    let mut coverage_bad_xhtml = 0u32;

    for (index, chapter) in book.chapters.iter().enumerate() {
        let chapter_id = index as u32;
        let (text, tokens, valid) = tokenize::tokenize(&chapter.bytes);
        if !valid {
            coverage_bad_xhtml += 1;
        }
        collect_words(chapter_id, tokens, &mut words)?;
        chapters.push(ChapterText {
            id: chapter_id,
            text,
        });
    }

    Ok(Document {
        chapters,
        words,
        coverage_bad_xhtml,
    })
}

/// Asigna `word_id` recorriendo los tokens: los que parecen palabras entran;
/// los de solo puntuación se fusionan en el rango de la palabra vecina.
fn collect_words(
    chapter: u32,
    tokens: Vec<tokenize::Token>,
    words: &mut Vec<Word>,
) -> Result<(), DocError> {
    let mut paragraph = 0u32;
    let mut in_paragraph = false;
    // Inicio del run de puntuación inicial del párrafo, a fusionar en la
    // siguiente palabra. Al agotarse el párrafo sin palabra, se descarta.
    let mut pending_start: Option<usize> = None;
    let mut previous: Option<usize> = None;

    for token in tokens {
        if token.paragraph_start {
            if in_paragraph {
                paragraph += 1;
                in_paragraph = false;
            }
            pending_start = None;
        }

        if token.has_alnum {
            let start = pending_start
                .take()
                .map_or(token.range.start, |p| p.min(token.range.start));
            if words.len() > u32::MAX as usize {
                return Err(DocError::TooManyWords);
            }
            words.push(Word {
                id: words.len() as u32,
                chapter,
                paragraph,
                text: token.text,
                range: start..token.range.end,
            });
            previous = Some(words.len() - 1);
            in_paragraph = true;
        } else if in_paragraph {
            if let Some(p) = previous {
                words[p].range.end = words[p].range.end.max(token.range.end);
            }
        } else {
            pending_start =
                Some(pending_start.map_or(token.range.start, |p| p.min(token.range.start)));
        }
    }
    Ok(())
}
