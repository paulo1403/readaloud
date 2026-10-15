//! `play`: lee un EPUB en voz alta desde el terminal.
//!
//! Compone todo el motor: `epub` → `doc` → por frase caché/[`Engine`] →
//! `timeline` → [`Player`]. El motor piper **no es reentrante** (un `Engine` por
//! proceso, ver `tts::engine`), así que el prefetch no es un hilo: se sintetiza
//! la frase siguiente mientras suena la actual, en este mismo hilo.

use std::ops::Range;
use std::path::Path;
use std::time::Duration;

use crate::player::{CpalSink, Player, PlayerState, Sentence, Sink};
use crate::tts::cache::{self, Cache};
use crate::tts::{Engine, voices};
use crate::{doc, epub, timeline};

use super::script;

/// Cada cuánto se sondea el reproductor mientras suena una frase.
const POLL: Duration = Duration::from_millis(10);

/// Resumen de una lectura: frases sonadas, palabras cubiertas y estado final.
#[derive(Debug)]
pub struct PlaySummary {
    pub sentences: usize,
    pub words_covered: usize,
    pub final_state: PlayerState,
}

/// Lee `path` con la voz `voice` (`.onnx`) y la caché por defecto, sobre el
/// `sink` dado. Encola con una frase de prefetch y no vuelve hasta `Ended`.
pub fn run_with(
    path: &Path,
    voice: &Path,
    length_scale: f32,
    sink: Box<dyn Sink>,
    max_sentences: usize,
) -> anyhow::Result<PlaySummary> {
    run_with_in(
        path,
        voice,
        length_scale,
        sink,
        max_sentences,
        &Cache::default_root(),
    )
}

/// Igual que [`run_with`], pero con una raíz de caché explícita (costura de
/// test: la caché por defecto vive en `$HOME`).
pub fn run_with_in(
    path: &Path,
    voice: &Path,
    length_scale: f32,
    sink: Box<dyn Sink>,
    max_sentences: usize,
    cache_root: &Path,
) -> anyhow::Result<PlaySummary> {
    let book = epub::open(path)?;
    let document = doc::build(&book)?;
    let cache = Cache::new(cache_root.to_path_buf());
    let voice_key = voice.to_string_lossy().into_owned();

    let frases = frases(&document, &book);
    let total = frases.len().min(max_sentences);
    if total == 0 {
        return Ok(PlaySummary {
            sentences: 0,
            words_covered: 0,
            final_state: PlayerState::Idle,
        });
    }

    // La caché se consulta antes que el motor: una segunda pasada entera de
    // caché no carga libpiper siquiera. El motor se carga en la primera síntesis.
    let mut engine: Option<Engine> = None;
    let mut player = Player::new(sink);

    let mut next = 0usize;
    let mut words_covered = 0usize;
    // Sample global en el que termina cada frase ya encolada, para saber cuántas
    // han sonado sin depender de la cola interna del `Player`.
    let mut ends: Vec<usize> = Vec::new();
    let mut enqueued_samples = 0usize;
    let mut finished = 0usize;
    let mut final_state;

    loop {
        // Prefetch: como mucho dos frases en vuelo (la que suena y la siguiente).
        while next < total && ends.len() - finished < 2 {
            let (rango, texto) = &frases[next];
            let key = cache::key(&voice_key, length_scale, texto);
            let chunk = match cache.get(&key) {
                Some(c) => c,
                None => {
                    let e = match &mut engine {
                        Some(e) => e,
                        None => engine.insert(Engine::load(voice)?),
                    };
                    let c = e.synthesize(texto, length_scale)?;
                    cache.put(&key, &c);
                    c
                }
            };
            let group_words = &document.words[rango.clone()];
            enqueued_samples += chunk.samples.len();
            ends.push(enqueued_samples);
            words_covered += rango.len();
            player.enqueue(Sentence {
                first_word: group_words[0].id,
                timeline: timeline::build(&chunk, group_words),
                chunk,
            });
            next += 1;
        }

        let state = player.tick();
        final_state = state;
        if state == PlayerState::Ended {
            break;
        }

        let pos = player.position_sample();
        while finished < ends.len() && pos >= ends[finished] {
            finished += 1;
        }

        std::thread::sleep(POLL);
    }

    Ok(PlaySummary {
        sentences: next,
        words_covered,
        final_state,
    })
}

/// Lee `path` en voz alta con la voz `voice` (por nombre), abriendo el
/// dispositivo de salida por defecto a la tasa del motor.
pub fn run(path: &Path, voice: &str, length_scale: f32) -> anyhow::Result<()> {
    let chosen = super::say::select(&voices::default_dir(), Some(voice))?;
    let sink = CpalSink::new(voice_rate(&chosen.onnx)?)?;
    run_with_in(
        path,
        &chosen.onnx,
        length_scale,
        Box::new(sink),
        usize::MAX,
        &Cache::default_root(),
    )?;
    Ok(())
}

/// Tasa de muestreo que declara la voz (`audio.sample_rate` de su `.onnx.json`).
/// El stream debe correr a esa tasa o el audio se desafina.
fn voice_rate(onnx: &Path) -> anyhow::Result<u32> {
    let config = std::fs::read(crate::tts::config_path(onnx))?;
    let json: serde_json::Value = serde_json::from_slice(&config)?;
    json["audio"]["sample_rate"]
        .as_u64()
        .map(|r| r as u32)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "la voz {} no declara audio.sample_rate",
                onnx.display()
            )
        })
}

/// Las frases del libro, ya agrupadas, con su rango sobre `document.words`.
///
/// Se agrupa capítulo a capítulo porque los `paragraph` de `doc` se numeran por
/// capítulo; agrupar el vector entero fusionaría el último párrafo de un
/// capítulo con el primero del siguiente.
fn frases(document: &doc::Document, book: &epub::Book) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    for chapter in 0..book.chapters.len() {
        let rango = script::chapter_words(&document.words, chapter as u32);
        for g in script::grupos_de_frase(&document.words[rango.clone()]) {
            let inicio = rango.start + g.start;
            let fin = rango.start + g.end;
            let texto = document.words[inicio..fin]
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            out.push((inicio..fin, texto));
        }
    }
    out
}
