//! Reproducción de audio: máquina de estados, abstracción del dispositivo y
//! posición por sample.
//!
//! Lo único que importa de este módulo es **saber en qué sample va**: de esa
//! posición sale la palabra que se resalta. Por eso el dispositivo es un
//! [`Sink`] abstracto y el FSM es puro: `tick` no duerme ni espera al audio,
//! sólo mira cuánto ha sonado y avanza. Los tests corren headless con
//! [`NullSink`]; [`CpalSink`] es la implementación real, sobre `cpal`.

mod cpal;
mod sink;

pub use cpal::CpalSink;
pub use sink::{NullSink, Sink};

use std::collections::VecDeque;

use crate::timeline::Timeline;
use crate::tts::PhonemeChunk;

/// Estado de la máquina de reproducción.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    /// Nada encolado todavía.
    Idle,
    /// Hay audio pendiente de entregar al dispositivo.
    Buffering,
    /// Sonando.
    Playing,
    /// Detenido por el usuario.
    Paused,
    /// Todo lo encolado se ha reproducido.
    Ended,
}

/// Una frase lista para sonar: su audio, su alineación y su primera palabra.
///
/// `timeline.start_sample` está referido al inicio de `chunk.samples` (0), no a
/// la posición global del libro.
#[derive(Debug)]
pub struct Sentence {
    pub first_word: u32,
    pub chunk: PhonemeChunk,
    pub timeline: Timeline,
}

/// Error del subsistema de reproducción.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("no hay dispositivo de salida por defecto")]
    Device,
    #[error("configuración de audio no soportada por el dispositivo")]
    Config,
    #[error("el dispositivo no soporta ninguna salida a {rate} Hz")]
    Rate { rate: u32 },
    #[error("no se pudo abrir el stream de audio")]
    Stream,
}

/// Reproductor: cola de frases sobre un [`Sink`].
pub struct Player {
    sink: Box<dyn Sink>,
    queue: VecDeque<Sentence>,
    state: PlayerState,
    /// Sample inicial dentro de la frase actual (desplazamiento por `seek`).
    offset: usize,
    /// Samples del flujo global anteriores al comienzo de la escritura actual.
    finished: usize,
    /// `sink.played_samples()` en el momento de empezar la escritura actual.
    sink_base: usize,
    /// Si la frase actual ya se ha entregado al sink.
    written: bool,
}

impl Player {
    /// Crea un reproductor sobre `sink`.
    pub fn new(sink: Box<dyn Sink>) -> Self {
        Self {
            sink,
            queue: VecDeque::new(),
            state: PlayerState::Idle,
            offset: 0,
            finished: 0,
            sink_base: 0,
            written: false,
        }
    }

    /// Encola una frase al final.
    pub fn enqueue(&mut self, s: Sentence) {
        if self.queue.is_empty() {
            self.state = PlayerState::Buffering;
        }
        self.queue.push_back(s);
    }

    /// Avanza la máquina de estados y devuelve el estado resultante.
    ///
    /// **No bloquea**: no duerme ni espera al dispositivo. Si el sink ya consumió
    /// la frase actual, pasa a la siguiente; si no queda ninguna, `Ended`.
    pub fn tick(&mut self) -> PlayerState {
        if self.state == PlayerState::Paused {
            return self.state;
        }

        // Salta las frases ya consumidas por el sink.
        while let Some(pendiente) = self.pendiente_actual() {
            let consumido = self.written
                && self.sink.played_samples().saturating_sub(self.sink_base) >= pendiente;
            if !consumido {
                break;
            }
            self.finished += pendiente;
            self.queue.pop_front();
            self.offset = 0;
            self.written = false;
            self.sink_base = self.sink.played_samples();
        }

        match self.queue.front() {
            Some(s) => {
                if !self.written {
                    let inicio = self.offset.min(s.chunk.samples.len());
                    self.sink_base = self.sink.played_samples();
                    self.sink.write(&s.chunk.samples[inicio..], s.chunk.rate);
                    self.written = true;
                }
                self.state = PlayerState::Playing;
            }
            // Sin frases: `Idle` si nunca hubo nada, `Ended` si ya sonó algo.
            None => {
                if self.state != PlayerState::Idle {
                    self.state = PlayerState::Ended;
                }
            }
        }
        self.state
    }

    /// Samples que quedan de la frase en curso (descontado el `offset`).
    fn pendiente_actual(&self) -> Option<usize> {
        self.queue
            .front()
            .map(|s| s.chunk.samples.len().saturating_sub(self.offset))
    }

    /// Sample del flujo global en el que va la reproducción.
    pub fn position_sample(&self) -> usize {
        self.finished + self.sink.played_samples().saturating_sub(self.sink_base)
    }

    /// Palabra que cubre la posición actual, según el timeline de la frase en
    /// curso. `None` en silencios o sin frase.
    pub fn current_word(&self) -> Option<u32> {
        let s = self.queue.front()?;
        let local = self.offset + self.sink.played_samples().saturating_sub(self.sink_base);
        s.timeline.word_at(local)
    }

    /// Detiene la reproducción. `tick` devolverá `Paused` hasta `resume`.
    ///
    /// Rebobina el puntero de escritura a la posición real del sink y descarta
    /// la cola no reproducida, de modo que el `played_samples()` que crezca
    /// mientras la pausa dure no haga que `resume` dé la frase por terminada.
    /// Al reanudar, el `tick` reescribe sólo el resto desde `offset`.
    pub fn pause(&mut self) {
        if self.state != PlayerState::Playing {
            return;
        }
        let local = self.sink.played_samples().saturating_sub(self.sink_base);
        self.offset += local;
        self.finished += local;
        self.written = false;
        self.sink.stop();
        // Recaptura tras `stop()` para que `position_sample` siga cuadrando
        // mientras esté en pausa.
        self.sink_base = self.sink.played_samples();
        self.state = PlayerState::Paused;
    }

    /// Reanuda tras `pause`. El siguiente `tick` reescribe el resto de la frase.
    pub fn resume(&mut self) {
        if self.state == PlayerState::Paused {
            self.state = PlayerState::Playing;
        }
    }

    /// Salta a la frase que contiene `word_id` y al sample inicial de esa
    /// palabra. Las frases anteriores se descartan.
    pub fn seek_to_word(&mut self, word_id: u32) {
        let Some(idx) = self
            .queue
            .iter()
            .position(|s| s.timeline.spans.iter().any(|sp| sp.word_id == word_id))
        else {
            return;
        };
        self.sink.stop();
        self.queue.drain(..idx);
        let s = self.queue.front().expect("la frase existe: la acabamos de buscar");
        let start = s
            .timeline
            .spans
            .iter()
            .find(|sp| sp.word_id == word_id)
            .map_or(0, |sp| sp.start_sample);
        self.offset = start;
        self.finished = start;
        self.sink_base = self.sink.played_samples();
        self.written = false;
        self.state = PlayerState::Buffering;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Span;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Chunk sintético de `samples` samples de silencio, sin alineación.
    fn chunk(samples: usize) -> PhonemeChunk {
        PhonemeChunk {
            samples: vec![0i16; samples],
            rate: 22050,
            phonemes: Vec::new(),
            phoneme_ids: Vec::new(),
            alignments: Vec::new(),
        }
    }

    /// Sentencia sin timeline (no se consulta `current_word`).
    fn sentencia(first_word: u32, samples: usize) -> Sentence {
        Sentence {
            first_word,
            chunk: chunk(samples),
            timeline: Timeline { spans: Vec::new() },
        }
    }

    /// Sentencia con timeline explícito `(word_id, start_sample, end_sample)`.
    fn sentencia_con_timeline(first_word: u32, spans: &[(u32, usize, usize)]) -> Sentence {
        let len = spans.iter().map(|&(_, _, e)| e).max().unwrap_or(0);
        Sentence {
            first_word,
            chunk: chunk(len),
            timeline: Timeline {
                spans: spans
                    .iter()
                    .map(|&(word_id, start_sample, end_sample)| Span {
                        word_id,
                        start_sample,
                        end_sample,
                        exact: true,
                    })
                    .collect(),
            },
        }
    }

    #[test]
    fn la_fsm_recorre_idle_buffering_playing_ended() {
        let mut p = Player::new(Box::new(NullSink::instant()));
        assert_eq!(p.tick(), PlayerState::Idle); // sin nada encolado
        p.enqueue(sentencia(0, 50)); // 50 samples
        assert_eq!(p.tick(), PlayerState::Playing);
        p.tick();
        assert_eq!(p.tick(), PlayerState::Ended);
    }

    #[test]
    fn pausa_y_reanuda() {
        let mut p = Player::new(Box::new(NullSink::new(Arc::new(AtomicUsize::new(0)))));
        p.enqueue(sentencia(0, 100));
        assert_eq!(p.tick(), PlayerState::Playing);
        p.pause();
        assert_eq!(p.tick(), PlayerState::Paused);
        p.resume();
        assert_eq!(p.tick(), PlayerState::Playing);
    }

    #[test]
    fn pausa_detiene_el_sink_y_reanuda_desde_el_offset() {
        let jugado = Arc::new(AtomicUsize::new(0));
        let sink = NullSink::new(jugado.clone());
        let paradas = sink.stop_counter();
        let mut p = Player::new(Box::new(sink));
        // 100 samples en dos palabras: [0,50) y [50,100).
        p.enqueue(sentencia_con_timeline(0, &[(0, 0, 50), (1, 50, 100)]));
        assert_eq!(p.tick(), PlayerState::Playing);
        jugado.store(40, Ordering::SeqCst); // el dispositivo va por el sample 40

        p.pause();
        assert_eq!(paradas.load(Ordering::SeqCst), 1, "pause debe detener el sink");
        assert_eq!(p.tick(), PlayerState::Paused);
        assert_eq!(p.position_sample(), 40, "la posición no salta al pausar");
        assert_eq!(p.current_word(), Some(0), "sigue la palabra en la que íbamos");

        // Sink defectuoso: sigue drenando durante la pausa.
        jugado.store(100, Ordering::SeqCst);
        assert_eq!(p.tick(), PlayerState::Paused);

        p.resume();
        assert_eq!(p.tick(), PlayerState::Playing, "resume no debe dar por Ended");
        // La posición y la palabra siguen siendo las de la pausa: el FSM no
        // saltó el resto, lo reescribe desde `offset`.
        assert_eq!(p.position_sample(), 40);
        assert_eq!(p.current_word(), Some(0));

        // El resto (60 samples) suena y la frase termina una sola vez.
        jugado.store(160, Ordering::SeqCst);
        assert_eq!(p.tick(), PlayerState::Ended);
        assert_eq!(p.position_sample(), 100);
    }

    #[test]
    fn current_word_sigue_la_posicion() {
        let avanzado = Arc::new(AtomicUsize::new(0));
        let mut p = Player::new(Box::new(NullSink::new(avanzado.clone())));
        p.enqueue(sentencia_con_timeline(0, &[(0, 0, 25), (1, 25, 70)]));
        p.tick();
        assert_eq!(p.current_word(), Some(0));
        avanzado.store(30, Ordering::SeqCst); // el dispositivo ya reprodujo 30 samples
        p.tick();
        assert_eq!(p.current_word(), Some(1));
    }

    #[test]
    fn seek_a_palabra_salta_al_sample_correcto() {
        let mut p = Player::new(Box::new(NullSink::new(Arc::new(AtomicUsize::new(0)))));
        p.enqueue(sentencia_con_timeline(0, &[(0, 0, 25), (1, 25, 70)]));
        p.seek_to_word(1);
        p.tick();
        assert_eq!(p.position_sample(), 25);
    }
}
