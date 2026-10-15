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
//! "cuarenta y dos"), **ninguna** palabra de ese trozo puede decir `exact:
//! true`: no sabemos dónde cayó la expansión, así que no hay base para fiarse de
//! ningún emparejamiento 1:1. El tramo entero se reparte entre todas las
//! palabras, contiguo y sin solape, marcado `exact: false`. Se cambia precisión
//! a nivel de palabra en los párrafos raros que contienen un token que expande
//! —a cambio de no mentir nunca: un resaltado que se desfasa en silencio es peor
//! que uno honesto y grueso. Mejora futura: un detector localizado de expansión
//! (dígitos, abreviaturas, moneda, fechas) que conserve exacta la cabeza del
//! trozo; tal detector es incompleto por naturaleza, y por eso **no** es el
//! defecto.

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
        if ph == ' ' || ph == '$' {
            // Frontera: cierra el grupo abierto ANTES de sumar la pausa, para
            // que el silencio de la frontera (y el del EOS) quede fuera de todo
            // span y `word_at` devuelva `None` ahí.
            if let Some(s) = abierto.take() {
                out.push((s, acc));
            }
            acc += a;
        } else if ph == '^' {
            acc += a; // BOS: no abre grupo (silencio de cabecera = hueco)
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
/// Mismo número → 1:1 exacto (el único caso con emparejamiento conocido).
/// Distinto → ninguna palabra afirma exactitud: el tramo completo se reparte
/// entre todas las palabras, `exact: false`. No se intenta adivinar dónde
/// quedó la cabeza exacta: la expansión puede caer en cualquier punto.
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

    let (inicio, fin) = match (groups.first(), groups.last()) {
        (Some(&(s, _)), Some(&(_, e))) => (s, e),
        _ => (0, 0),
    };
    let largo = fin.saturating_sub(inicio);
    for (t, word) in words.iter().enumerate() {
        spans.push(Span {
            word_id: word.id,
            start_sample: inicio + largo * t / w,
            end_sample: inicio + largo * (t + 1) / w,
            exact: false,
        });
    }
    spans
}
