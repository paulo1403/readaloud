//! Caché en disco de [`PhonemeChunk`] por `(voz, length_scale, texto)`.
//!
//! La síntesis de una novela entera tarda ~40 min; la caché hace instantánea la
//! segunda escucha. Dos ficheros por entrada: `<key>.pcm` con los `i16` crudos en
//! little-endian y `<key>.json` con el resto. El `.json` se escribe **después**
//! del `.pcm`: su presencia declara la entrada completa, así una escritura
//! interrumpida nunca se sirve.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

use super::PhonemeChunk;

/// Clave de caché: sha256 hex de `(voz, length_scale, texto)`.
///
/// Son exactamente las entradas que cambian el audio. Las longitudes se
/// serializan antes del contenido para que fronteras como `("ab","c")` y
/// `("a","bc")` no colisionen.
pub fn key(voice: &str, length_scale: f32, text: &str) -> String {
    let mut h = Sha256::new();
    h.update((voice.len() as u64).to_le_bytes());
    h.update(voice.as_bytes());
    h.update(length_scale.to_bits().to_le_bytes());
    h.update((text.len() as u64).to_le_bytes());
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())
}

/// Parte de la entrada que va al `.json` (las muestras van al `.pcm`).
#[derive(serde::Serialize, serde::Deserialize)]
struct Meta {
    rate: u32,
    phonemes: Vec<char>,
    phoneme_ids: Vec<i32>,
    alignments: Vec<i32>,
}

/// Caché de `PhonemeChunk` bajo un directorio raíz.
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(root: PathBuf) -> Cache {
        Cache { root }
    }

    /// Directorio por defecto: `~/.cache/readaloud/audio/`.
    pub fn default_root() -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        home.join(".cache/readaloud/audio")
    }

    /// Devuelve la entrada completa o `None`. Nunca crea nada ni hace panic:
    /// un fichero ausente, corrupto o truncado es un miss.
    pub fn get(&self, key: &str) -> Option<PhonemeChunk> {
        // El `.json` existe solo si la entrada se terminó de escribir, así que
        // leerlo primero basta para descartar escrituras a medias.
        let meta: Meta =
            serde_json::from_slice(&std::fs::read(self.root.join(format!("{key}.json"))).ok()?)
                .ok()?;
        let bytes = std::fs::read(self.root.join(format!("{key}.pcm"))).ok()?;
        if bytes.len() % 2 != 0 {
            return None;
        }

        let samples: Vec<i16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect();

        // El contrato del seam: `sum(alignments) == samples.len()`. Un `.pcm`
        // truncado a un número par de bytes rompe la suma: es un miss.
        if meta.phonemes.len() != meta.alignments.len() {
            return None;
        }
        let total: i64 = meta.alignments.iter().map(|&a| i64::from(a)).sum();
        if total != samples.len() as i64 {
            return None;
        }

        Some(PhonemeChunk {
            samples,
            rate: meta.rate,
            phonemes: meta.phonemes,
            phoneme_ids: meta.phoneme_ids,
            alignments: meta.alignments,
        })
    }

    /// Escribe la entrada. El `.json` va **después** del `.pcm` y declara la
    /// entrada completa; si algo falla, no queda una entrada servible.
    pub fn put(&self, key: &str, chunk: &PhonemeChunk) {
        if std::fs::create_dir_all(&self.root).is_err() {
            return;
        }
        let pcm = self.root.join(format!("{key}.pcm"));
        let json = self.root.join(format!("{key}.json"));
        // Si ya había una entrada, se retira el `.json` antes de reescribir el
        // `.pcm`: una reescritura interrumpida no puede servir el `.pcm` nuevo
        // a medias emparejado con el `.json` viejo.
        let _ = std::fs::remove_file(&json);

        let mut bytes = Vec::with_capacity(chunk.samples.len() * 2);
        for s in &chunk.samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        if std::fs::write(&pcm, &bytes).is_err() {
            return;
        }

        let meta = Meta {
            rate: chunk.rate,
            phonemes: chunk.phonemes.clone(),
            phoneme_ids: chunk.phoneme_ids.clone(),
            alignments: chunk.alignments.clone(),
        };
        let Ok(text) = serde_json::to_vec(&meta) else {
            return;
        };
        let _ = std::fs::write(&json, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::PhonemeChunk;

    /// Trozo sintético que respeta los invariantes del contrato:
    /// `phonemes.len() == alignments.len()` y `sum(alignments) == samples.len()`.
    fn chunk_de_prueba() -> PhonemeChunk {
        PhonemeChunk {
            samples: vec![1, -2, 3, -4, i16::MIN, i16::MAX],
            rate: 22050,
            phonemes: vec!['^', '^', 'h', 'h', ' ', ' ', 'a', 'a'],
            phoneme_ids: vec![1, 0, 3, 0, 0, 0, 4, 0],
            alignments: vec![2, 0, 1, 0, 1, 0, 1, 1],
        }
    }

    #[test]
    fn roundtrip_conserva_muestras_y_alineaciones() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let original = chunk_de_prueba();

        cache.put("k", &original);
        let got = cache.get("k").expect("entrada recién escrita");

        assert_eq!(got.samples, original.samples);
        assert_eq!(got.rate, original.rate);
        assert_eq!(got.phonemes, original.phonemes);
        assert_eq!(got.phoneme_ids, original.phoneme_ids);
        assert_eq!(got.alignments, original.alignments);
        assert!(dir.path().join("k.pcm").is_file());
        assert!(dir.path().join("k.json").is_file());
    }

    #[test]
    fn la_clave_depende_de_voz_rate_y_texto() {
        assert_ne!(key("v1", 1.0, "hola"), key("v2", 1.0, "hola"));
        assert_ne!(key("v1", 1.0, "hola"), key("v1", 1.3, "hola"));
        assert_ne!(key("v1", 1.0, "hola"), key("v1", 1.0, "adios"));
        assert_eq!(key("v1", 1.0, "hola"), key("v1", 1.0, "hola"));
    }

    #[test]
    fn un_get_sin_entrada_es_none_y_no_crea_nada() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());

        assert!(cache.get("nada").is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn una_entrada_incompleta_o_corrupta_es_miss_y_no_panico() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());

        // Solo el `.pcm`, sin `.json`: la entrada no está completa.
        std::fs::write(dir.path().join("solo-pcm.pcm"), [0u8, 0]).unwrap();
        assert!(cache.get("solo-pcm").is_none());

        // `.json` corrupto con `.pcm` presente.
        std::fs::write(dir.path().join("roto.pcm"), [0u8, 0]).unwrap();
        std::fs::write(dir.path().join("roto.json"), b"{no es json").unwrap();
        assert!(cache.get("roto").is_none());

        // `.pcm` truncado a un número impar de bytes pese al `.json` válido.
        cache.put("trunc", &chunk_de_prueba());
        let pcm = dir.path().join("trunc.pcm");
        let mut bytes = std::fs::read(&pcm).unwrap();
        bytes.pop();
        std::fs::write(&pcm, bytes).unwrap();
        assert!(cache.get("trunc").is_none());
    }
}
