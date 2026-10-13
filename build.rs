// Construye `libpiper` (piper1-gpl v1.8.0) y enlaza su API C (R1, ruta (a)).
//
// El código C++ no viene vendorizado: se clona la etiqueta fijada dentro de
// OUT_DIR. El CMake de libpiper se encarga por su cuenta de compilar espeak-ng
// (ExternalProject) y de descargar onnxruntime prebuilt, así que `cargo build`
// necesita red y `git`, y no necesita nada instalado por sudo.
//
// ponytail: el compilador de datos de espeak-ng usa un buffer de ruta fijo
// (~180 bytes) y el OUT_DIR de cargo lo desborda, truncando los nombres de
// `phsource` y abortando el build. Por eso el CMake se ejecuta en un
// directorio corto en /tmp (derivado del OUT_DIR). Cuando espeak-ng arregle su
// buffer, se puede volver a construir dentro de OUT_DIR.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;

const TAG: &str = "v1.8.0";
const REPO: &str = "https://github.com/OHF-Voice/piper1-gpl.git";
const ONNXRUNTIME_VERSION: &str = "1.22.0";

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
    let short = PathBuf::from(format!("/tmp/ra-piper-{:016x}", hasher.finish()));

    let mut config = cmake::Config::new(src_root.join("libpiper"));
    config
        .out_dir(&short)
        .define("CMAKE_BUILD_TYPE", "Release")
        .define("ONNXRUNTIME_VERSION", ONNXRUNTIME_VERSION)
        // libpiper.so y libonnxruntime.so.1 se instalan juntos en <dst>/lib;
        // $ORIGIN hace que libpiper encuentre onnxruntime sin ruta absoluta
        // (y sin depender de la ruta del build).
        .define("CMAKE_INSTALL_RPATH", "$ORIGIN");
    let dst = config.build();

    // libpiper.so y libonnxruntime.so.1 quedan en <dst>/lib tras el install.
    println!("cargo:rustc-link-search=native={}/lib", dst.display());
    println!("cargo:rustc-link-lib=dylib=piper");
    // El binario de test debe encontrar libpiper en tiempo de ejecución.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}/lib", dst.display());
    println!("cargo:rustc-link-arg-tests=-Wl,-rpath,{}/lib", dst.display());
}
