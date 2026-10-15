//! Task 7: mapeo `word_id` → rango de samples.
//!
//! Los chunks son sintéticos, hechos a mano: cero audio, cero piper. La forma
//! de los tres arrays paralelos es la documentada en `tts::PhonemeChunk`.

use readaloud::doc;
use readaloud::timeline::build;
use readaloud::tts::PhonemeChunk;

/// Construye un `PhonemeChunk` desde entradas `(phoneme, id, alignment)`
/// paralelas. `samples` se rellena a mano con la longitud pedida.
fn chunk(entradas: &[(char, i32, i32)], samples_len: usize) -> PhonemeChunk {
    PhonemeChunk {
        samples: vec![0i16; samples_len],
        rate: 22050,
        phonemes: entradas.iter().map(|e| e.0).collect(),
        phoneme_ids: entradas.iter().map(|e| e.1).collect(),
        alignments: entradas.iter().map(|e| e.2).collect(),
    }
}

/// Construye las palabras de un capítulo, con ids contiguos.
fn palabras(textos: &[&str]) -> Vec<doc::Word> {
    textos
        .iter()
        .enumerate()
        .map(|(i, t)| doc::Word {
            id: i as u32,
            chapter: 0,
            paragraph: 0,
            text: (*t).to_string(),
            range: 0..0,
        })
        .collect()
}

/// 2 palabras, 2 grupos. El par BOS (`'^'`) y el EOS (`'$'`) llevan alineación
/// 0; cada fonema va real + relleno (`id 0`); la frontera es una tirada de dos
/// espacios con alineación 0 (pausa nula). "uno" suma 10+10+5 = 25, "dos"
/// suma 20+20+5 = 45; el total 70 == samples.len().
fn chunk_de_dos_palabras() -> PhonemeChunk {
    chunk(
        &[
            ('^', 1, 0),
            ('^', 0, 0), // BOS
            ('a', 10, 10),
            ('a', 0, 10),
            ('b', 11, 5),
            ('b', 0, 0), // "uno"
            (' ', 3, 0),
            (' ', 0, 0), // frontera
            ('c', 12, 20),
            ('c', 0, 20),
            ('d', 13, 5),
            ('d', 0, 0), // "dos"
            ('$', 2, 0), // EOS
        ],
        70,
    )
}

/// 2 palabras ("son" "42") pero 4 grupos: "42" se expande a "cuarenta y dos".
/// Tramos: (0,20) (20,30) (30,35) (35,40).
fn chunk_dos_palabras_cuatro_grupos() -> PhonemeChunk {
    chunk(
        &[
            ('^', 1, 0),
            ('^', 0, 0),
            ('s', 20, 10),
            ('s', 0, 5),
            ('o', 21, 5),
            ('o', 0, 0), // "son"
            (' ', 3, 0),
            (' ', 0, 0),
            ('k', 22, 10),
            ('k', 0, 0), // "cuarenta"
            (' ', 3, 0),
            (' ', 0, 0),
            ('i', 23, 5),
            ('i', 0, 0), // "y"
            (' ', 3, 0),
            (' ', 0, 0),
            ('d', 24, 5),
            ('d', 0, 0), // "dos"
            ('$', 2, 0),
        ],
        40,
    )
}

/// 4 palabras ("yo" "tengo" "42" "años") pero 6 grupos: "42" (posición 2)
/// expande a "cuarenta y dos". Tramos: (0,6) (6,14) (14,30) (30,36) (36,49)
/// (49,70); el total 70 == samples.len(). La expansión cae en medio.
fn chunk_con_expansion_en_medio() -> PhonemeChunk {
    chunk(
        &[
            ('^', 1, 0),
            ('^', 0, 0),
            ('y', 10, 4),
            ('y', 0, 2), // "yo" 6
            (' ', 3, 0),
            (' ', 0, 0),
            ('t', 11, 5),
            ('t', 0, 3), // "tengo" 8
            (' ', 3, 0),
            (' ', 0, 0),
            ('k', 12, 10),
            ('k', 0, 6), // "cuarenta" 16
            (' ', 3, 0),
            (' ', 0, 0),
            ('i', 13, 4),
            ('i', 0, 2), // "y" 6
            (' ', 3, 0),
            (' ', 0, 0),
            ('d', 14, 8),
            ('d', 0, 5), // "dos" 13
            (' ', 3, 0),
            (' ', 0, 0),
            ('a', 15, 15),
            ('a', 0, 6), // "años" 21
            ('$', 2, 0),
        ],
        70,
    )
}

/// Igual que `chunk_de_dos_palabras`, pero el `'$'` (EOS) lleva una alineación
/// de 10: el silencio final. `samples.len() == 70 + 10 = 80`.
fn chunk_con_silencio_final() -> PhonemeChunk {
    chunk(
        &[
            ('^', 1, 0),
            ('^', 0, 0),
            ('a', 10, 10),
            ('a', 0, 10),
            ('b', 11, 5),
            ('b', 0, 0),
            (' ', 3, 0),
            (' ', 0, 0),
            ('c', 12, 20),
            ('c', 0, 20),
            ('d', 13, 5),
            ('d', 0, 0),
            ('$', 2, 10), // silencio final
        ],
        80,
    )
}

#[test]
fn el_silencio_final_del_eos_no_pertenece_a_la_ultima_palabra() {
    let words = palabras(&["uno", "dos"]);
    let t = build(&chunk_con_silencio_final(), &words);
    let ultimo = t.spans.last().unwrap();
    assert_eq!(ultimo.end_sample, 70, "el silencio del EOS no se cuenta");
    assert_eq!(t.word_at(70), None, "durante el silencio final no hay palabra");
    assert_eq!(t.word_at(69), Some(1));
}

#[test]
fn rangos_exactos_por_palabra() {
    let t = build(&chunk_de_dos_palabras(), &palabras(&["uno", "dos"]));
    assert_eq!(t.spans.len(), 2);
    assert!(t.spans.iter().all(|s| s.exact));
    assert_eq!((t.spans[0].start_sample, t.spans[0].end_sample), (0, 25));
    assert_eq!((t.spans[1].start_sample, t.spans[1].end_sample), (25, 70));
}

#[test]
fn invariantes_monotonia_sin_solape_y_cobertura() {
    let t = build(&chunk_de_dos_palabras(), &palabras(&["uno", "dos"]));
    for w in t.spans.windows(2) {
        assert!(w[0].end_sample <= w[1].start_sample); // monotonía y sin solape
    }
    assert!(t.spans.last().unwrap().end_sample <= 70); // no excede el total
    let ids: Vec<u32> = t.spans.iter().map(|s| s.word_id).collect();
    assert_eq!(ids, vec![0, 1]); // cada palabra una vez
}

#[test]
fn expansion_de_numeros_degrada_a_frase_sin_desfasar() {
    // el texto tiene 2 palabras ("son" "42") pero el chunk trae 4 grupos de fonemas
    let t = build(&chunk_dos_palabras_cuatro_grupos(), &palabras(&["son", "42"]));
    assert_eq!(t.spans.len(), 2);
    assert!(t.spans.iter().all(|s| !s.exact), "un conteo distinto no permite afirmar exactitud");
    for w in t.spans.windows(2) {
        assert!(w[0].end_sample <= w[1].start_sample);
    }
}

#[test]
fn expansion_en_medio_no_deja_palabras_exactas() {
    // 4 palabras; el numero de la posicion 2 expande a varios grupos, luego hay 6
    let words = palabras(&["yo", "tengo", "42", "años"]);
    let t = build(&chunk_con_expansion_en_medio(), &words);
    assert_eq!(t.spans.len(), 4);
    assert!(t.spans.iter().all(|s| !s.exact), "un conteo distinto no permite afirmar exactitud");
    for w in t.spans.windows(2) {
        assert!(w[0].end_sample <= w[1].start_sample);
    }
    // y el ultimo no se pasa del total
    assert!(t.spans.last().unwrap().end_sample <= 70);
    // cobertura: cada word_id aparece exactamente una vez
    let mut ids: Vec<u32> = t.spans.iter().map(|s| s.word_id).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![0, 1, 2, 3], "cada palabra una vez, ninguna perdida");
}

#[test]
fn un_chunk_sin_palabras_da_un_timeline_vacio() {
    let t = build(&chunk_de_dos_palabras(), &[]);
    assert!(t.spans.is_empty());
}

#[test]
fn word_at_mapea_sample_a_palabra() {
    let t = build(&chunk_de_dos_palabras(), &palabras(&["uno", "dos"]));
    assert_eq!(t.word_at(0), Some(0));
    assert_eq!(t.word_at(24), Some(0));
    assert_eq!(t.word_at(25), Some(1));
    assert_eq!(t.word_at(9999), None);
}
