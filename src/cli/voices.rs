//! `voices`: listar las voces instaladas y preparar su alineación.

use std::io::Write;
use std::path::Path;

use crate::tts::voices;

/// `voices list [--dir]`: una línea por voz, `name<TAB>lang`.
pub fn list(dir: Option<&Path>) -> anyhow::Result<()> {
    let dir = dir.map_or_else(voices::default_dir, Path::to_path_buf);
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for voice in voices::discover(&dir) {
        writeln!(out, "{}\t{}", voice.name, voice.lang)?;
    }
    Ok(())
}

/// `voices prepare <name> [--dir]`: parchea la voz y reporta el resultado.
pub fn prepare(dir: Option<&Path>, name: &str) -> anyhow::Result<()> {
    let dir = dir.map_or_else(voices::default_dir, Path::to_path_buf);
    let onnx = dir.join(format!("{name}.onnx"));

    let already = voices::is_prepared(&onnx)?;
    voices::prepare(&dir, name)?;

    if !voices::is_prepared(&onnx)? {
        println!("{name}: sin tensor de alineación, nada que parchear");
    } else if already {
        println!("{name}: ya estaba preparada");
    } else {
        println!("{name}: preparada");
    }
    Ok(())
}
