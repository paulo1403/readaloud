//! Escritura de WAV PCM 16 bits mono, sin dependencias.
//!
//! Son 44 bytes de cabecera RIFF/WAVE/fmt/data detrás de un test: no merece un
//! crate.

use std::io::{self, Write};

/// Escribe `samples` como WAV PCM 16 bits mono a `rate` Hz.
pub fn write_pcm16_mono(w: &mut impl Write, samples: &[i16], rate: u32) -> io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = rate * 2;

    let mut header = [0u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes()); // tamaño del bloque fmt
    header[20..22].copy_from_slice(&1u16.to_le_bytes()); // formato PCM
    header[22..24].copy_from_slice(&1u16.to_le_bytes()); // canales: mono
    header[24..28].copy_from_slice(&rate.to_le_bytes());
    header[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    header[32..34].copy_from_slice(&2u16.to_le_bytes()); // alineación de bloque
    header[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits por muestra
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&data_len.to_le_bytes());
    w.write_all(&header)?;

    for sample in samples {
        w.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}
