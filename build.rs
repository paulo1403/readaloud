// Construye `libpiper` (piper1-gpl v1.8.0) y enlaza su API C (R1, ruta (a)).
//
// El código C++ no viene vendorizado: se clona la etiqueta fijada dentro de
// OUT_DIR. El CMake de libpiper se encarga por su cuenta de compilar espeak-ng
// (ExternalProject) y de descargar onnxruntime prebuilt, así que `cargo build`
// necesita red y `git`, y no necesita nada instalado por sudo.
//
// Dos restricciones reales obligan a este baile de directorios:
//
// 1. El compilador de datos de espeak-ng usa un buffer de ruta fijo (~180
//    bytes) y el OUT_DIR de cargo lo desborda, truncando los nombres de
//    `phsource` y abortando el build (*Bad vowel file*). Por eso el CMake se
//    ejecuta en un directorio corto. Ese directorio es /tmp: es solo
//    **scratch**.
// 2. /tmp es tmpfs: tras un reinicio desaparece, `target/` sobrevive y el
//    build script no se vuelve a ejecutar, así que cualquier ruta en /tmp
//    (link search path, `-rpath`) quedaría rota y los binarios ya enlazados no
//    arrancarían. Por eso, tras construir, las `.so` se copian a OUT_DIR
//    (persistente bajo `target/`) y se enlaza/rpath desde ahí.
//
// ponytail: cuando espeak-ng arregle su buffer, el CMake podrá construirse
// dentro de OUT_DIR y este paso de copia sobrará.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

const TAG: &str = "v1.8.0";
const REPO: &str = "https://github.com/OHF-Voice/piper1-gpl.git";
const ONNXRUNTIME_VERSION: &str = "1.22.0";

/// Bibliotecas que el binario necesita en tiempo de ejecución. `libonnxruntime.so`
/// (sin versión) no hace falta: el `DT_NEEDED` de libpiper es `.so.1`.
const RUNTIME_LIBS: &[&str] = &[
    "libpiper.so",
    "libonnxruntime.so.1",
    "libonnxruntime_providers_shared.so",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let src_root = out.join(format!("piper1-gpl-{TAG}"));

    if !src_root.join("libpiper/CMakeLists.txt").exists() {
        let status = Command::new("git")
            .args(["clone", "--depth", "1", "--branch", TAG, REPO])
            .arg(&src_root)
            .status()
            .expect("no se pudo ejecutar git (se necesita para traer piper1-gpl)");
        assert!(status.success(), "git clone de piper1-gpl {TAG} falló");
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    out.hash(&mut hasher);
    let scratch = PathBuf::from(format!("/tmp/ra-piper-{:016x}", hasher.finish()));

    let mut config = cmake::Config::new(src_root.join("libpiper"));
    config
        .out_dir(&scratch)
        .define("CMAKE_BUILD_TYPE", "Release")
        .define("ONNXRUNTIME_VERSION", ONNXRUNTIME_VERSION)
        // libpiper.so y libonnxruntime.so.1 acaban juntos en <scratch>/lib y,
        // tras copiarlos, juntos en OUT_DIR/lib; $ORIGIN los enlaza sin ruta
        // absoluta.
        .define("CMAKE_INSTALL_RPATH", "$ORIGIN");
    let dst = config.build();

    // Copia persistente fuera de /tmp y enlace desde ahí.
    let persistent = out.join("lib");
    fs::create_dir_all(&persistent).expect("crear OUT_DIR/lib");
    copy_libs(&dst.join("lib"), &persistent);

    // Marcador escrito al final: si el directorio persistente desaparece (p.ej.
    // `cargo clean`), el `rerun-if-changed` sobre él fuerza a re-ejecutar el
    // build script en vez de enlazar contra rutas inexistentes.
    let sentinel = persistent.join(".complete");
    if !sentinel.exists() {
        fs::write(&sentinel, b"readaloud libpiper\n").expect("escribir marcador de libs");
    }
    println!("cargo:rerun-if-changed={}", sentinel.display());

    println!("cargo:rustc-link-search=native={}", persistent.display());
    println!("cargo:rustc-link-lib=dylib=piper");
    // El binario (y los tests) deben encontrar libpiper en OUT_DIR, no en /tmp.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", persistent.display());
    println!("cargo:rustc-link-arg-tests=-Wl,-rpath,{}", persistent.display());
}

/// Copia las `.so` de `src` a `dst` si faltan o cambian de tamaño. Evitar copias
/// innecesarias mantiene estable el mtime de `dst`, de modo que el
/// `rerun-if-changed` del marcador no provoca reconstrucciones en bucle.
fn copy_libs(src: &Path, dst: &Path) {
    for name in RUNTIME_LIBS {
        let from = src.join(name);
        if !from.exists() {
            panic!("libpiper no instaló {}", from.display());
        }
        let to = dst.join(name);
        let needs_copy = match (fs::metadata(&from), fs::metadata(&to)) {
            (Ok(a), Ok(b)) => a.len() != b.len(),
            (Ok(_), Err(_)) => true,
            (Err(e), _) => panic!("metadata de {}: {e}", from.display()),
        };
        if needs_copy {
            fs::copy(&from, &to)
                .unwrap_or_else(|e| panic!("copiar {} -> {}: {e}", from.display(), to.display()));
        }
    }
}
