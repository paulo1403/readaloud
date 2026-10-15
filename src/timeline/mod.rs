//! Línea de tiempo: traduce fonemas alineados a rangos de samples por palabra.
//!
//! Función pura sobre [`tts::PhonemeChunk`]: no conoce audio, dispositivo ni
//! piper. El walk sobre los tres arrays paralelos está documentado en
//! `PhonemeChunk`; aquí se recorre en lockstep, se saltan el par BOS (`'^'`) y
//! el EOS (`'$'`), se agrupa por tiradas de espacio y se suma **toda** la
//! alineación de cada grupo (rellenos incluidos, o la duración se partiría por
//! la mitad).
//!
//! Si el número de grupos no coincide con el de palabras (p. ej. "42" →
//! "cuarenta y dos"), el prefijo casa 1:1 y las palabras restantes se reparten
//! el tramo que queda, marcadas `exact: false`: el desfase no se propaga y
//! ninguna palabra se pierde.

use crate::doc::Word;
use crate::tts::PhonemeChunk;

/// Rango de samples `[start_sample, end_sample)` de una palabra.
#[derive(Debug)]
pub struct Span {
    pub word_id: u32,
    pub start_sample: usize,
    pub end_sample: usize,
    /// `true` si la palabra casó 1:1 con un grupo de fonemas.
    pub exact: bool,
}

/// Mapeo palabra → samples de un trozo sintetizado.
#[derive(Debug)]
pub struct Timeline {
    pub spans: Vec<Span>,
}

impl Timeline {
    /// Palabra que cubre `sample`, o `None` en silencios y fuera del rango.
    pub fn word_at(&self, sample: usize) -> Option<u32> {
        self.spans
            .iter()
            .find(|s| sample >= s.start_sample && sample < s.end_sample)
            .map(|s| s.word_id)
    }
}

/// Construye el timeline. Nunca falla: un desajuste degrada a `exact: false`.
pub fn build(chunk: &PhonemeChunk, words: &[Word]) -> Timeline {
    let groups = grupos(&chunk.phonemes, &chunk.alignments);
    Timeline {
        spans: casar(&groups, words),
    }
}

/// Rango `(start, end)` de samples de cada palabra de fonemas, en orden.
///
/// El límite es una tirada de `' '`: los espacios no suman a ningún grupo, pero
/// su pausa avanza el reloj, dejando entre palabras un hueco sin palabra.
fn grupos(phonemes: &[char], alignments: &[i32]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut acc = 0usize;
    let mut abierto: Option<usize> = None;
    for (i, &ph) in phonemes.iter().enumerate() {
        let a = alignments.get(i).copied().unwrap_or(0).max(0) as usize;
        if ph == '^' || ph == '$' {
            acc += a; // BOS/EOS: no abren grupo
        } else if ph == ' ' {
            if let Some(s) = abierto.take() {
                out.push((s, acc));
            }
            acc += a;
        } else {
            abierto.get_or_insert(acc);
            acc += a;
        }
    }
    if let Some(s) = abierto.take() {
        out.push((s, acc));
    }
    out
}

/// Casa los grupos con las palabras.
///
/// Mismo número → 1:1 exacto. Distinto → prefijo 1:1 (`min(g, w) - 1`) y las
/// palabras restantes se reparten el tramo que va del primer grupo no casado al
/// último, todas `exact: false`.
fn casar(groups: &[(usize, usize)], words: &[Word]) -> Vec<Span> {
    let (g, w) = (groups.len(), words.len());
    let mut spans = Vec::with_capacity(w);

    if g == w {
        for (k, &(s, e)) in groups.iter().enumerate() {
            spans.push(Span {
                word_id: words[k].id,
                start_sample: s,
                end_sample: e,
                exact: true,
            });
        }
        return spans;
    }

    let k = g.min(w).saturating_sub(1);
    for (i, &(s, e)) in groups.iter().take(k).enumerate() {
        spans.push(Span {
            word_id: words[i].id,
            start_sample: s,
            end_sample: e,
            exact: true,
        });
    }

    let (inicio, fin) = match groups.get(k).zip(groups.last()) {
        Some((&(s, _), &(_, e))) => (s, e),
        None => (0, 0),
    };
    let restantes = w - k;
    let largo = fin.saturating_sub(inicio);
    for t in 0..restantes {
        spans.push(Span {
            word_id: words[k + t].id,
            start_sample: inicio + largo * t / restantes,
            end_sample: inicio + largo * (t + 1) / restantes,
            exact: false,
        });
    }
    spans
}
