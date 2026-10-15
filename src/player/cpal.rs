//! Sink real: `cpal` sobre el dispositivo de salida por defecto.
//!
//! Delgado a propósito. `Player` ya posee la máquina de estados, la posición y
//! el mapeo a palabra; aquí sólo hay una cola compartida con el callback de
//! audio y un contador de samples sonados. Nada de esta lógica decide *qué*
//! suena ni *cuándo*: eso es del FSM.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::player::{PlayerError, Sink};

/// Tipo de la cola compartida entre `write` (hilo de UI) y el callback de audio.
type Cola = Arc<Mutex<VecDeque<i16>>>;

/// Salida de audio sobre `cpal`. Mantiene el stream vivo mientras exista.
pub struct CpalSink {
    played: Arc<AtomicUsize>,
    queue: Cola,
    _stream: cpal::Stream,
}

impl CpalSink {
    /// Abre el dispositivo de salida por defecto a `rate` Hz.
    ///
    /// `rate` es la tasa del audio ya sintetizado (piper emite 22050 Hz; la
    /// lleva el `sample_rate` del `.onnx.json` de la voz y el `PhonemeChunk`).
    /// Se elige el primer formato del dispositivo que soporte esa tasa: el
    /// stream debe correr a la tasa del contenido, o éste sonaría con el tono
    /// y la velocidad cambiados. Si ningún formato la soporta, se devuelve
    /// [`PlayerError::Rate`] en vez de desafinar en silencio.
    pub fn new(rate: u32) -> Result<Self, PlayerError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(PlayerError::Device)?;

        let configs: Vec<_> = device
            .supported_output_configs()
            .map_err(|_| PlayerError::Config)?
            .filter(|c| soportado(c.sample_format()))
            .collect();
        let rangos: Vec<(u32, u32)> = configs
            .iter()
            .map(|c| (c.min_sample_rate(), c.max_sample_rate()))
            .collect();
        let idx = elegir(&rangos, rate).ok_or(PlayerError::Rate { rate })?;
        let supported = configs[idx]
            .try_with_sample_rate(rate)
            .expect("elegir() ya comprobó que la tasa cabe en el rango");

        let format = supported.sample_format();
        let config = supported.config();

        let played = Arc::new(AtomicUsize::new(0));
        let queue: Cola = Arc::new(Mutex::new(VecDeque::new()));

        let (pc, qc) = (played.clone(), queue.clone());
        let stream = match format {
            SampleFormat::F32 => construir::<f32>(&device, &config, pc, qc),
            SampleFormat::I16 => construir::<i16>(&device, &config, pc, qc),
            SampleFormat::U16 => construir::<u16>(&device, &config, pc, qc),
            _ => return Err(PlayerError::Config),
        }?;

        stream.play().map_err(|_| PlayerError::Stream)?;

        Ok(Self {
            played,
            queue,
            _stream: stream,
        })
    }
}

/// ¿`construir` sabe abrir un stream en este formato?
///
/// El dispositivo anuncia muchos formatos (`U8`, `I24`, `I32`…) en el mismo
/// rango de tasas y `supported_output_configs` no los ordena por calidad: el
/// primero puede ser `U8`. Elegir a ciegas abriría un stream de 8 bits o, peor,
/// un formato que `construir` no sabe rellenar (→ [`PlayerError::Config`]).
/// Sólo se consideran los formatos que el sink soporta de verdad.
fn soportado(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
    )
}

/// Índice del primer rango `(min, max)` que contiene `rate`, o `None`.
///
/// Pura a propósito: es la lógica de selección de tasa y se prueba sin
/// dispositivo.
fn elegir(rangos: &[(u32, u32)], rate: u32) -> Option<usize> {
    rangos
        .iter()
        .position(|&(min, max)| min <= rate && rate <= max)
}

/// Construye el stream para el formato `T`: consume de la cola, replica cada
/// sample mono a todos los canales y cuenta los samples sonados.
fn construir<T: SizedSample + FromSample<i16>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    played: Arc<AtomicUsize>,
    queue: Cola,
) -> Result<cpal::Stream, PlayerError> {
    let channels = config.channels as usize;
    let err_fn = |e| eprintln!("readaloud: error en el stream de audio: {e}");
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let mut cola = queue.lock().unwrap_or_else(|e| e.into_inner());
                let mut sonados = 0usize;
                for frame in data.chunks_mut(channels) {
                    let s = cola.pop_front();
                    if s.is_some() {
                        sonados += 1;
                    }
                    let v = s.map_or_else(|| T::EQUILIBRIUM, T::from_sample);
                    for out in frame.iter_mut() {
                        *out = v;
                    }
                }
                drop(cola);
                played.fetch_add(sonados, Ordering::Relaxed);
            },
            err_fn,
            None,
        )
        .map_err(|_| PlayerError::Stream)
}

impl Sink for CpalSink {
    // ponytail: sin remuestreo. El stream corre a la tasa de `new(rate)`; una
    // frase con otra `rate` sonaría con el tono cambiado. Añadir resample si
    // alguna vez conviven frases de distinta tasa.
    fn write(&mut self, samples: &[i16], _rate: u32) {
        let mut cola = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        cola.extend(samples.iter().copied());
    }

    fn played_samples(&self) -> usize {
        self.played.load(Ordering::Relaxed)
    }

    fn stop(&mut self) {
        let mut cola = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        cola.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La selección de tasa no toca el dispositivo: el stream debe correr a la
    /// tasa del contenido (22050 Hz, la de piper), o el audio se desafina.
    #[test]
    fn elige_una_tasa_que_soporta_la_del_motor() {
        // Ningún formato soporta 22050 → error, nunca reproducir a 44100/48000.
        assert_eq!(elegir(&[(44100, 48000)], 22050), None);
        // Un rango que lo contiene → se usa.
        assert_eq!(elegir(&[(16000, 48000)], 22050), Some(0));
        assert_eq!(elegir(&[(44100, 48000)], 44100), Some(0));
        // Varios rangos: gana el primero que contiene la tasa.
        assert_eq!(elegir(&[(44100, 48000), (22050, 22050)], 22050), Some(1));
    }

    /// El dispositivo anuncia `U8`, `I24`, `I32`… al mismo rango: sólo valen los
    /// formatos que `construir` sabe rellenar. Sin este filtro, el `U8` que ALSA
    /// lista primero rompía `CpalSink::new` con `PlayerError::Config`.
    #[test]
    fn solo_se_consideran_los_formatos_soportados() {
        assert!(soportado(SampleFormat::F32));
        assert!(soportado(SampleFormat::I16));
        assert!(soportado(SampleFormat::U16));
        assert!(!soportado(SampleFormat::U8));
        assert!(!soportado(SampleFormat::I24));
        assert!(!soportado(SampleFormat::I32));
    }
}
