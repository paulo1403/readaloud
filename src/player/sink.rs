//! La costura `Sink`: el `Player` habla con un dispositivo abstracto.
//!
//! `Player` sólo necesita tres cosas del mundo del audio —entregar samples,
//! preguntar cuántos han sonado y parar—; todo lo demás (formato, canales,
//! buffering, hilos) es asunto del sink. Los tests usan [`NullSink`], que no
//! toca dispositivo alguno.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Destino de audio. `Player` es el único consumidor.
pub trait Sink {
    /// Encola `samples` (PCM mono i16) para reproducir a `rate` Hz.
    fn write(&mut self, samples: &[i16], rate: u32);
    /// Total de samples ya reproducidos desde el inicio de la sesión.
    fn played_samples(&self) -> usize;
    /// Detiene la reproducción y descarta lo que quede pendiente.
    fn stop(&mut self);
}

/// Sink mudo: no hay dispositivo, alguien externo lleva la cuenta.
///
/// Tiene dos modos porque los tests necesitan dos relojes distintos:
///
/// - [`NullSink::new`] **no** avanza el contador: el test lo mueve a mano
///   (`store`). Es la forma de fijar una posición concreta y comprobar cómo se
///   mapea a palabra.
/// - [`NullSink::instant`] cuenta todo lo escrito como reproducido al instante.
///   Es la forma de que el FSM llegue a `Ended` sin dispositivo real.
pub struct NullSink {
    played: Arc<AtomicUsize>,
    immediate: bool,
}

impl NullSink {
    /// Sink mudo con un contador que **sólo** mueve quien lo posee.
    pub fn new(counter: Arc<AtomicUsize>) -> Self {
        Self {
            played: counter,
            immediate: false,
        }
    }

    /// Sink mudo que reproduce al instante: lo escrito es lo ya sonado.
    pub fn instant() -> Self {
        Self {
            played: Arc::new(AtomicUsize::new(0)),
            immediate: true,
        }
    }
}

impl Sink for NullSink {
    fn write(&mut self, samples: &[i16], _rate: u32) {
        if self.immediate {
            self.played.fetch_add(samples.len(), Ordering::SeqCst);
        }
    }

    fn played_samples(&self) -> usize {
        self.played.load(Ordering::SeqCst)
    }

    fn stop(&mut self) {}
}
