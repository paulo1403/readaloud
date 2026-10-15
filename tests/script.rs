mod common;

use common::{minimal_epub, one_chapter_epub};
use readaloud::test_support::test_voice;
use readaloud::{cli, doc, epub};
use tempfile::tempdir;

#[test]
fn vuelca_una_linea_por_palabra_en_orden() {
    let dir = tempdir().unwrap();
    let path = minimal_epub(dir.path());
    let out = cli::script::run_text(&path).unwrap();
    assert_eq!(out.lines().next().unwrap(), "0\t0\t0\tHola");
    let ids: Vec<u32> = out
        .lines()
        .map(|l| l.split('\t').next().unwrap().parse().unwrap())
        .collect();
    assert_eq!(ids, (0..ids.len() as u32).collect::<Vec<_>>());
}

#[test]
fn un_archivo_que_no_es_epub_da_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("no.epub");
    std::fs::write(&path, b"esto no es un epub").unwrap();
    assert!(cli::script::run_text(&path).is_err());
}

#[test]
fn emite_timeline_valido_por_capitulo() {
    let Some(voz) = test_voice() else {
        eprintln!("skip: define READALOUD_TEST_VOICE");
        return;
    };
    let dir = tempdir().unwrap();
    let out = cli::script::run_timeline(&minimal_epub(dir.path()), &voz).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let span = &v[0]["spans"][0];
    assert!(span["start_sample"].as_u64().unwrap() <= span["end_sample"].as_u64().unwrap());
    assert!(!v[0]["spans"].as_array().unwrap().is_empty());
}

/// Los `word_id` de un capítulo, según el documento, ordenados.
fn words_del_capitulo(document: &doc::Document, chapter: u32) -> Vec<u32> {
    let mut ids: Vec<u32> = document
        .words
        .iter()
        .filter(|w| w.chapter == chapter)
        .map(|w| w.id)
        .collect();
    ids.sort_unstable();
    ids
}

/// La tesis, contra el motor de verdad: una frase sin números casa 1:1 (todos
/// los spans `exact`), cada rango es no vacío y ordenado, y cada capítulo cubre
/// exactamente sus `word_id` — sin duplicados ni pérdidas.
#[test]
fn la_tesis_se_cumple_contra_el_motor_real() {
    let Some(voz) = test_voice() else {
        eprintln!("skip: define READALOUD_TEST_VOICE");
        return;
    };
    let dir = tempdir().unwrap();
    let path = minimal_epub(dir.path());
    let out = cli::script::run_timeline(&path, &voz).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let chapters = v.as_array().unwrap();

    let book = epub::open(&path).unwrap();
    let document = doc::build(&book).unwrap();

    // Capítulo 1 (`<p>Hola mundo</p>`): sin números, tiene que ser 1:1.
    let spans1 = chapters[0]["spans"].as_array().unwrap();
    assert!(!spans1.is_empty());
    for s in spans1 {
        assert_eq!(s["exact"], true, "una frase sin números casa 1:1: {s}");
        let (a, b) = (
            s["start_sample"].as_u64().unwrap(),
            s["end_sample"].as_u64().unwrap(),
        );
        assert!(a < b, "el rango de la palabra no es vacío: {s}");
    }
    for w in spans1.windows(2) {
        assert!(
            w[0]["end_sample"].as_u64().unwrap() <= w[1]["start_sample"].as_u64().unwrap(),
            "spans ordenados y sin solape"
        );
    }

    // Cobertura en cada capítulo: exactamente sus `word_id`, una vez cada uno.
    for (i, cap) in chapters.iter().enumerate() {
        let mut ids: Vec<u32> = cap["spans"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["word_id"].as_u64().unwrap() as u32)
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, words_del_capitulo(&document, i as u32), "cobertura del capítulo {i}");
    }
}

/// Un número que espeak expande a varias palabras separadas ("2020" → "dos mil
/// veinte") hace que el conteo de grupos no case con el de palabras: la
/// degradación honesta deja al menos un span `exact: false`. Sintetiza de
/// verdad, no es un chunk a mano.
///
/// Se usa "2020" y no "42": espeak sintetiza "42" ("cuarenta y dos") como una
/// sola tirada de fonemas sin espacios, así que casa 1:1 y `exact: true` es
/// correcto ahí.
#[test]
fn un_numero_degrada_a_frase_honesta() {
    let Some(voz) = test_voice() else {
        eprintln!("skip: define READALOUD_TEST_VOICE");
        return;
    };
    let dir = tempdir().unwrap();
    let path = one_chapter_epub(dir.path(), "<p>Tengo 2020 años.</p>");
    let out = cli::script::run_timeline(&path, &voz).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let spans = v[0]["spans"].as_array().unwrap();
    assert!(!spans.is_empty());
    assert!(
        spans.iter().any(|s| s["exact"] == false),
        "un número expande y no puede afirmar exactitud: {spans:?}"
    );
}
