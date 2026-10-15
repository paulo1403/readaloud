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
    /// Abre el dispositivo de salida por defecto y arranca el stream.
    pub fn new() -> Result<Self, PlayerError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(PlayerError::Device)?;
        let supported = device
            .default_output_config()
            .map_err(|_| PlayerError::Config)?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();

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
    // ponytail: sin remuestreo. Las frases se sintetizan ya a la tasa del
    // dispositivo; si alguna trae otra `rate`, sonará con el tono cambiado.
    // Añadir resample cuando `rate != device_rate` importe de verdad.
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
