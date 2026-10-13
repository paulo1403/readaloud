//! `script --text`: vuelca el libro como una línea por palabra.

use std::fmt::Write;
use std::path::Path;

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
