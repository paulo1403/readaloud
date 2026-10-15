# readaloud — Plan de implementación (M0–M4: el motor)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Un binario Rust que convierte un EPUB en audio con voces neuronales locales y un
mapeo exacto `palabra → rango de samples`, verificable sin ventana.

**Architecture:** Un crate, un binario. `epub/` abre el contenedor; `doc/` produce una
secuencia estable de palabras con `word_id`; `tts/` sintetiza por frase y devuelve
fonemas + alineación; `timeline/` traduce eso a rangos de sample por palabra; `player/`
reproduce y sabe en qué sample va. La GUI (M5) es una carcasa que consume exactamente los
mismos datos que el subcomando `script`.

**Tech Stack:** Rust · `clap` · `zip` · `roxmltree` · `serde`/`serde_json` · `onnx-protobuf`
· FFI a `libpiper` (C) · `cpal`. Licencia GPL-3.0-or-later.

**Spec:** `docs/superpowers/specs/2026-10-13-readaloud-design.md`

## Prerequisites

Comprobado en la máquina del autor. Antes del task 1:

- **Toolchain de Rust asignada.** `cargo` y `rustc` son shims de mise sin versión fijada
  (`mise ERROR No version is set for shim: cargo`). Ejecutar `mise use -g rust@1.98.0` (o la
  última estable) y confirmar con `cargo --version`. Sin esto, el task 1 falla por un error
  de mise, no de Rust.
- **Una voz para los tests con puerta.** Los tasks 5, 6, 8 y 11 se saltan (con `skip:`) si no
  existe `READALOUD_TEST_VOICE`. Exportar la ruta a un `.onnx`, por ejemplo el que ya está en
  la máquina: `export READALOUD_TEST_VOICE=~/.local/share/piper-voices/es_ES-davefx-medium.onnx`.
- Ya presentes: `pkg-config`, `cmake`, `c++` (`/usr/bin/c++`).

## Global Constraints

Requisitos que aplican a **todas** las tareas. Valores copiados de la spec.

- Un crate, un binario. Sin workspace.
- Licencia `GPL-3.0-or-later` en `Cargo.toml`.
- Solo Linux de escritorio. Sin WebView.
- `word_id: u32`, índice **global** de palabra en el libro, contiguo desde 0 en orden de
  lectura, estable para el mismo archivo.
- `word_id` es la única moneda entre motor y UI. `timeline/` no conoce píxeles; `render/`
  no conoce fonemas.
- EPUB: solo reflowable, spine lineal. **Nada fuera de §2 del spec puede provocar un panic
  ni un backtrace**: se devuelve `Err` o se degrada y se registra cobertura.
- Voces en `~/.local/share/readaloud/voices/` (formato piper: `<voz>.onnx` + `.onnx.json`).
- Caché de audio en disco, clave `(voz, length_scale, texto)`.
- La velocidad se controla con `length_scale` de piper y re-síntesis. Prohibido
  estiramiento temporal (WSOLA/vocoder).
- Invariantes del timeline, verificados en tests (spec §4): monotonía, sin solape, cada
  `word_id` una única vez (con rango propio o dentro de una unidad degradada), el último
  `end_sample` no excede el total, y lo no-1:1 queda marcado.
- CLI en M0–M4: `script`, `say`, `voices`, `play`. **`open` y `reload-themes` no se
  implementan en este plan** (son M5): no se deja scaffolding para más tarde.
- Refinamiento de §3 del spec: los subcomandos viven en `src/cli/`, que la lista de
  módulos del spec no contemplaba. El resto de módulos se respeta tal cual.

## Review Focus

Clases de entrada que la spec implica pero cuyo comportamiento ningún task cubre con
tests. Cada una lleva su test en el task dueño del código.

1. **XHTML no bien formado**, que es lo normal en EPUBs reales. Parsear con un lector XML
   estricto tiraría libros que otros lectores abren. → Task 3.
2. **Idiomas sin espacios (CJK)**: una tokenización por espacios da un token de párrafo
   entero. Debe degradar sin romper ni alucinar resaltado por palabra. → Task 3.
3. **Fichero que no es un ZIP, o ZIP sin OPF**: error legible de usuario, nunca un panic ni
   un backtrace. → Task 2.
4. **Capítulo sin texto** (solo imágenes o vacío): `word_id` no puede quedar hueco y un
   timeline vacío debe ser válido. → Tasks 3 y 7.
5. **Entidades HTML y comillas tipográficas** (`&amp;`, `&#233;`, `«»`): sin decodificar,
   espeak lee "amp" y la alineación arranca desfasada. → Task 3.

---

## File Structure

```
Cargo.toml
build.rs                        enlaza libpiper (o la ruta elegida en R1)
src/
  main.rs                       parsea la CLI y despacha
  cli/
    mod.rs                      tipos clap (Cli, ScriptArgs, SayArgs, VoicesArgs, PlayArgs)
    script.rs                   subcomando: volcar texto o timeline
    say.rs                      subcomando: texto -> WAV
    voices.rs                   subcomando: listar y preparar voces
    play.rs                     subcomando: leer un EPUB en voz alta
  epub/
    mod.rs                      Book, Chapter, open()
    opf.rs                      lectura del OPF: metadatos y spine
  doc/
    mod.rs                      Document, Word, build()
    tokenize.rs                 texto XHTML -> párrafos -> palabras
  tts/
    mod.rs                      PhonemeChunk, TtsError
    engine.rs                   Engine: carga de voz y síntesis
    voices.rs                   Voice, discover(), prepare() (parche ONNX)
    cache.rs                    Cache de PCM y timeline en disco
  timeline/
    mod.rs                      Span, Timeline, build(), word_at()
  player/
    mod.rs                      Player, PlayerState
    sink.rs                     trait Sink, NullSink
    cpal.rs                     CpalSink
  wav.rs                        escritor WAV mínimo (sin dependencia)
omarchy/                        (M5; no en este plan)
tests/
  common/mod.rs                 fixture: EPUB mínimo de 2 capítulos
  epub.rs
  doc.rs
  timeline.rs
  script.rs
  play.rs
docs/
  decisions/R1-tts-integration.md
```

Reparto por responsabilidad, no por capa: `epub/` sabe de ZIP y OPF y nada más; `doc/`
sabe de texto y `word_id` y nada más; `timeline/` es una función pura sobre fonemas.

---

### Task 1: Esqueleto y contrato de la CLI

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/cli/mod.rs`

**Interfaces:**
- Consumes: nada.
- Produces: `cli::Cli`, `cli::ScriptArgs`, `cli::SayArgs`, `cli::VoicesArgs`, `cli::VoicesCmd`
  y `cli::PlayArgs`, usados por los tasks 4, 6, 8 y 11.

- [ ] **Step 1: Escribir el test que falla**

En `src/cli/mod.rs`, al final:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn subcomandos_del_motor_existen() {
        let cli = Cli::parse_from(["readaloud", "script", "libro.epub", "--timeline"]);
        assert!(matches!(cli.command, Command::Script(a) if a.timeline));
    }

    #[test]
    fn no_hay_subcomandos_de_la_gui_todavia() {
        assert!(Cli::try_parse_from(["readaloud", "open", "x.epub"]).is_err());
    }

    #[test]
    fn la_cli_es_bien_formada() {
        Cli::command().debug_assert();
    }
}
```

- [ ] **Step 2: Ejecutar el test y verlo fallar**

Run: `cargo test --lib cli:: -- --nocapture`
Expected: no compila (`Cli` no existe) o falla con "no such subcommand"

- [ ] **Step 3: Implementar el crate y la CLI**

`Cargo.toml`: `name = "readaloud"`, `edition = "2024"`, `license = "GPL-3.0-or-later"`,
dependencias `clap` (última estable, features `["derive"]`), `anyhow`, `thiserror`.

`src/cli/mod.rs` define:

```rust
pub enum Command { Script(ScriptArgs), Say(SayArgs), Voices(VoicesArgs), Play(PlayArgs) }

#[derive(clap::Args)]
pub struct ScriptArgs { pub path: PathBuf, #[arg(long, conflicts_with = "text")] pub timeline: bool, #[arg(long)] pub text: bool }

#[derive(clap::Args)]
pub struct SayArgs { pub text: String, #[arg(short, long)] pub out: Option<PathBuf>, #[arg(long, default_value = "1.0")] pub length_scale: f32 }

#[derive(clap::Args)]
pub struct VoicesArgs { #[command(subcommand)] pub cmd: VoicesCmd }

#[derive(clap::Subcommand)]
pub enum VoicesCmd { List { #[arg(long)] dir: Option<PathBuf> }, Prepare { name: String, #[arg(long)] dir: Option<PathBuf> } }

#[derive(clap::Args)]
pub struct PlayArgs { pub path: PathBuf, #[arg(long)] pub voice: String, #[arg(long, default_value = "1.0")] pub length_scale: f32, #[arg(long, hide = true)] pub max_sentences: Option<usize> }
```

`main.rs` despacha y por ahora devuelve `anyhow::bail!("no implementado")` en cada rama.

Deliberadamente **no** existen `open` ni `reload-themes`.

- [ ] **Step 4: Ejecutar los tests y verlos pasar**

Run: `cargo test --lib cli:: && cargo run -- --help`
Expected: 3 tests PASS; `--help` lista `script`, `say`, `voices`, `play`

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/cli/mod.rs
git commit -m "feat(cli): esqueleto del crate y contrato de subcomandos del motor"
```

---

### Task 2: Contenedor EPUB y OPF

**Files:**
- Create: `src/epub/mod.rs`, `src/epub/opf.rs`, `tests/common/mod.rs`, `tests/epub.rs`

**Interfaces:**
- Consumes: nada.
- Produces:
  - `epub::Book { pub title: Option<String>, pub author: Option<String>, pub lang: Option<String>, pub chapters: Vec<Chapter> }`
  - `epub::Chapter { pub id: String, pub href: String, pub media_type: String, pub bytes: Vec<u8> }`
  - `epub::open(path: &Path) -> Result<Book, EpubError>`
  - `epub::EpubError` (`NotAZip`, `NoOpf`, `NoContainer`, `BadXml`, `Missing(String)`), con `thiserror`
  - `tests::common::minimal_epub(dir: &Path) -> PathBuf`
  - `tests::common::write_zip(path: &Path, entries: &[(&str, &str)]) -> PathBuf` (para el caso "ZIP sin OPF")

- [ ] **Step 1: Escribir el fixture y los tests que fallan**

`tests/common/mod.rs` escribe un EPUB de verdad con el crate `zip`: `mimetype` (primero y
sin comprimir, como manda la norma), `META-INF/container.xml` apuntando a `OEBPS/content.opf`,
un OPF con título `Fixture`, autor `Autor`, `dc:language` `es`, y un spine con
`cap1.xhtml` (`linear` por defecto) y `cap2.xhtml` (`linear="no"`).

`tests/epub.rs`:

```rust
#[test]
fn lee_metadatos_y_spine_lineal() {
    let dir = tempdir();
    let path = common::minimal_epub(&dir);
    let book = epub::open(&path).unwrap();
    assert_eq!(book.title.as_deref(), Some("Fixture"));
    assert_eq!(book.lang.as_deref(), Some("es"));
    assert_eq!(book.chapters.len(), 1, "linear=\"no\" se salta");
    assert_eq!(book.chapters[0].href, "cap1.xhtml");
}

#[test]
fn un_archivo_que_no_es_zip_da_error_no_panic() {   // Review Focus 3
    let dir = tempdir();
    let p = dir.path().join("no.epub");
    std::fs::write(&p, b"esto no es un zip").unwrap();
    assert!(matches!(epub::open(&p), Err(epub::EpubError::NotAZip)));
}

#[test]
fn zip_sin_opf_da_error_no_panic() {                 // Review Focus 3
    let dir = tempdir();
    let p = dir.path().join("vacio.epub");
    write_zip(&p, &[("mimetype", "application/epub+zip")]);
    assert!(matches!(epub::open(&p), Err(epub::EpubError::NoOpf)));
}
```

- [ ] **Step 2: Ejecutar y ver fallar**

Run: `cargo test --test epub`
Expected: FAIL — `epub` no existe

- [ ] **Step 3: Implementar `epub::open`**

Añadir dependencias `zip` y `roxmltree`. `open` lee `META-INF/container.xml` con roxmltree,
resuelve la ruta del OPF, lo parsea, extrae `dc:title`/`dc:creator`/`dc:language` y recorre
`<spine>` saltándose `linear="no"`, tomando los bytes de cada XHTML del ZIP.

Un error de ZIP (`zip::result::ZipError`) se mapea a `EpubError::NotAZip`; un XML inválido al
`BadXml`. Ningún camino usa `unwrap` ni `panic!` sobre datos del archivo.

- [ ] **Step 4: Ejecutar y ver pasar**

Run: `cargo test --test epub`
Expected: 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/epub tests/common tests/epub.rs
git commit -m "feat(epub): contenedor ZIP, OPF y spine lineal"
```

---

### Task 3: Documento y `word_id`

**Files:**
- Create: `src/doc/mod.rs`, `src/doc/tokenize.rs`, `tests/doc.rs`

**Interfaces:**
- Consumes: `epub::Book`, `epub::Chapter`.
- Produces:
  - `doc::Word { pub id: u32, pub chapter: u32, pub paragraph: u32, pub text: String, pub range: Range<usize> }`
  - `doc::ChapterText { pub id: u32, pub text: String }`
  - `doc::Document { pub chapters: Vec<ChapterText>, pub words: Vec<Word>, pub coverage_bad_xhtml: u32 }`
  - `doc::Document::words(&self) -> &[Word]`, `doc::Document::word(&self, id: u32) -> Option<&Word>`
  - `doc::build(book: &epub::Book) -> Result<Document, DocError>`

- [ ] **Step 1: Escribir los tests que fallan**

`tests/doc.rs`, sobre HTML en memoria (sin EPUB, para aislar):

```rust
#[test]
fn ids_contiguos_y_en_orden_de_lectura() {
    let d = build_from_html(&["<p>Hola mundo</p><p>Otra frase</p>"]);
    let ids: Vec<u32> = d.words().iter().map(|w| w.id).collect();
    assert_eq!(ids, vec![0, 1, 2, 3]);
    assert_eq!(d.words()[0].text, "Hola");
    assert_eq!(d.words()[2].paragraph, 1);
}

#[test]
fn decodifica_entidades_y_conserva_la_puntuacion() {   // Review Focus 5
    let d = build_from_html(&["<p>Pan &amp; vino, &#233;nfasis.</p>"]);
    assert_eq!(d.words()[0].text, "Pan");
    assert_eq!(d.words()[1].text, "&");
    assert_eq!(d.words()[2].text, "vino,");
}

#[test]
fn un_token_de_solo_puntuacion_no_es_palabra() {
    let d = build_from_html(&["<p>Fin — luego</p>"]);
    let textos: Vec<&str> = d.words().iter().map(|w| w.text.as_str()).collect();
    assert_eq!(textos, vec!["Fin", "luego"]);
}

#[test]
fn xhtml_mal_formado_no_pierde_el_texto() {           // Review Focus 1
    let d = build_from_html(&["<p>Uno <b>dos<p>Tres"]);
    assert!(d.words().len() >= 3);
    assert_eq!(d.coverage_bad_xhtml, 1);
}

#[test]
fn sin_espacios_no_rompe() {                          // Review Focus 2
    let d = build_from_html(&["<p>这是一个没有空格的段落</p>"]);
    assert_eq!(d.words().len(), 1);
}

#[test]
fn capitulo_sin_texto_deja_la_secuencia_intacta() {    // Review Focus 4
    let d = build_from_html(&["<p>Antes</p>", "<img src=\"x.png\"/>", "<p>Después</p>"]);
    let ids: Vec<u32> = d.words().iter().map(|w| w.id).collect();
    assert_eq!(ids, vec![0, 1]);
}
```

`build_from_html` es un helper del propio test que construye un `epub::Book` en memoria.

- [ ] **Step 2: Ejecutar y ver fallar**

Run: `cargo test --test doc`
Expected: FAIL — `doc` no existe

- [ ] **Step 3: Implementar `doc::build`**

Reglas que quedan fijadas (el implementador no puede adivinarlas):

1. **Extracción de texto.** Se recorren los nodos de texto del XHTML. Se ignoran
   `script`, `style` y `head`. Los elementos `br`, `p`, `div`, `h1..h6`, `li`, `blockquote`
   y `td` cierran párrafo (insertan `\n`).
2. **Fallback tolerante (Review Focus 1).** Se intenta el parseo XML con `roxmltree`. Si
   falla, se extrae el texto con un eliminador de etiquetas simple sobre el HTML crudo y se
   incrementa `Document::coverage_bad_xhtml`. Nunca se propaga el error de XML.
3. **Entidades (Review Focus 5).** Se decodifican antes de tokenizar (`&amp;` → `&`,
   `&#233;` → `é`).
4. **Token.** Un token es una tirada maximal sin espacios en blanco (se usa
   `char::is_whitespace`). `range` es el span de bytes del token **completo** dentro de
   `ChapterText::text`.
5. **Palabra.** Un token que no contiene ningún `char::is_alphanumeric` no es palabra: su
   rango se fusiona en el `range` de la palabra anterior. Si es el primero del párrafo, se
   fusiona en la siguiente.
6. **Orden.** Los `word_id` se asignan recorriendo capítulos y párrafos en orden, contiguos
   desde 0.

Un párrafo sin espacios produce un único token y por tanto una única palabra (Review
Focus 2). Es la degradación aceptada en v1: no se rompe y no se inventa resaltado.

- [ ] **Step 4: Ejecutar y ver pasar**

Run: `cargo test --test doc`
Expected: 6 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/doc tests/doc.rs
git commit -m "feat(doc): extracción de texto, tokens y word_id estables con fallback tolerante"
```

---

### Task 4: `script --text`

**Files:**
- Create: `src/cli/script.rs`, `tests/script.rs`
- Modify: `src/main.rs`, `src/cli/mod.rs`

**Interfaces:**
- Consumes: `epub::open`, `doc::build`.
- Produces: `cli::script::run_text(path: &Path) -> anyhow::Result<String>`

- [ ] **Step 1: Escribir el test que falla**

`tests/script.rs`, sobre `common::minimal_epub`:

```rust
#[test]
fn vuelca_una_linea_por_palabra_en_orden() {
    let dir = tempdir();
    let path = common::minimal_epub(&dir);
    let out = cli::script::run_text(&path).unwrap();
    assert_eq!(out.lines().next().unwrap(), "0\t0\t0\tHola");
    let ids: Vec<u32> = out.lines().map(|l| l.split('\t').next().unwrap().parse().unwrap()).collect();
    assert_eq!(ids, (0..ids.len() as u32).collect::<Vec<_>>());
}
```

Formato fijado: `word_id\tchapter\tparagraph\ttext`, una línea por palabra, `\n` final.

- [ ] **Step 2: Ejecutar y ver fallar**

Run: `cargo test --test script`
Expected: FAIL — `cli::script` no existe

- [ ] **Step 3: Implementar y cablear**

`run_text` = `epub::open` → `doc::build` → formateo. `main.rs` enruta `Command::Script`
a `run_text` cuando no se pide `--timeline` (el timeline es el task 8; hasta entonces
`--timeline` devuelve `bail!("no implementado")`).

- [ ] **Step 4: Ejecutar y ver pasar**

Run: `cargo test --test script && cargo run -- script <tu-epub>`
Expected: PASS; el comando imprime las líneas del libro real

- [ ] **Step 5: Commit**

```bash
git add src/cli/script.rs src/main.rs src/cli/mod.rs tests/script.rs
git commit -m "feat(cli): subcomando script --text (EPUB a palabras con id)"
```

---

### Task 5: Motor TTS: enlace a piper y síntesis de una frase

**Files:**
- Create: `build.rs`, `src/tts/mod.rs`, `src/tts/engine.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: nada.
- Produces:
  - `tts::PhonemeChunk { pub samples: Vec<i16>, pub rate: u32, pub phonemes: Vec<char>, pub phoneme_ids: Vec<i32>, pub alignments: Vec<i32> }`
  - `tts::TtsError` (`VoiceNotFound`, `Engine`, `Synth`)
  - `tts::Engine::load(voice_onnx: &Path) -> Result<Engine, TtsError>`
  - `tts::Engine::synthesize(&mut self, text: &str, length_scale: f32) -> Result<PhonemeChunk, TtsError>`

`PhonemeChunk` es la costura pura del proyecto: el timeline (task 7) se construye sobre él
sin tocar audio, dispositivo, ni piper.

- [ ] **Step 1: Escribir el test que falla**

En `src/tts/engine.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn voz_de_test() -> Option<std::path::PathBuf> {
        // Los tests de integración no deben exigir modelos de 60 MB en disco.
        std::env::var("READALOUD_TEST_VOICE").ok().map(Into::into)
    }

    #[test]
    fn sintetiza_una_frase_y_devuelve_fonemas() {
        let Some(voz) = voz_de_test() else { eprintln!("skip: define READALOUD_TEST_VOICE"); return };
        let mut e = Engine::load(&voz).unwrap();
        let c = e.synthesize("Hola mundo.", 1.0).unwrap();
        assert!(c.samples.len() > 0);
        assert_eq!(c.rate, 22050);
        assert_eq!(c.phonemes.len(), c.alignments.len());
    }
}
```

- [ ] **Step 2: Ejecutar y ver fallar**

Run: `READALOUD_TEST_VOICE=~/.local/share/piper-voices/es_ES-davefx-medium.onnx cargo test --lib tts::`
Expected: FAIL — `Engine` no existe

- [ ] **Step 3: Implementar el enlace y la síntesis**

`build.rs` localiza `libpiper` con `pkg-config`/`cmake` y emite `cargo:rustc-link-lib`. Se
implementa la ruta **(a)**, la aprobada en D6; el task 6 mide si aguanta el criterio de R1.
`Engine::load` llama a `piper_create` y guarda el
puntero; `synthesize` llama a `piper_synthesize`, copia `samples` a `Vec<i16>`, y traduce
`phonemes` (codepoints `char32`) y `alignments` a los `Vec` de `PhonemeChunk`.

Envolver el recurso en un `Drop` que llame a `piper_free`. Ninguna función de la API C se
llama sin comprobar el puntero nulo.

*Verificar aquí, si no se hizo antes:* el campo exacto de `length_scale` en
`piper_synthesize_options` (spec, R3). Si no existiera, la velocidad se aplica re-sintetizando
con la config de la voz y se anota en la decisión R1.

- [ ] **Step 4: Ejecutar y ver pasar**

Run: `READALOUD_TEST_VOICE=... cargo test --lib tts::`
Expected: PASS con la voz del entorno; `skip:` sin ella

- [ ] **Step 5: Commit**

```bash
git add build.rs Cargo.toml src/tts
git commit -m "feat(tts): enlace a libpiper y sintesis por frase con fonemas y alineacion"
```

---

### Task 6: Registro de voces, parche ONNX y `say`; decisión R1

**Files:**
- Create: `src/tts/voices.rs`, `src/cli/say.rs`, `src/wav.rs`, `docs/decisions/R1-tts-integration.md`, `tests/say.rs`
- Modify: `src/main.rs`, `src/cli/mod.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: `tts::Engine`.
- Produces:
  - `tts::voices::Voice { pub name: String, pub lang: String, pub onnx: PathBuf }`
  - `tts::voices::default_dir() -> PathBuf` (`~/.local/share/readaloud/voices/`, con fallback a `~/.local/share/piper-voices/` si existe)
  - `tts::voices::discover(dir: &Path) -> Vec<Voice>`
  - `tts::voices::prepare(dir: &Path, name: &str) -> Result<(), TtsError>` (idempotente)
  - `tts::voices::is_prepared(onnx: &Path) -> Result<bool, TtsError>`
  - `cli::say::run(text: &str, out: Option<&Path>, length_scale: f32) -> anyhow::Result<()>`
  - `wav::write_pcm16_mono(w: &mut impl Write, samples: &[i16], rate: u32) -> io::Result<()>`

- [ ] **Step 1: Escribir los tests que fallan**

`tests/say.rs`:

```rust
#[test]
fn el_wav_tiene_cabecera_valida() {
    let mut buf = Vec::new();
    wav::write_pcm16_mono(&mut buf, &[0i16; 100], 22050).unwrap();
    assert_eq!(&buf[0..4], b"RIFF");
    assert_eq!(&buf[8..12], b"WAVE");
    assert_eq!(&buf[36..40], b"data");
    assert_eq!(u32::from_le_bytes(buf[40..44].try_into().unwrap()), 200);
}

#[test]
fn prepare_marca_el_tensor_de_alineacion() {
    let Some(voz) = test_voice() else { return };
    let dir = copia_temporal_de(voz);
    voices::prepare(&dir, "voz").unwrap();
    assert!(voices::is_prepared(&dir.join("voz.onnx")).unwrap());
    voices::prepare(&dir, "voz").unwrap();   // idempotente
}

#[test]
fn say_escribe_un_wav_no_vacio() {
    let Some(_) = test_voice() else { return };
    let out = tempfile();
    cli::say::run("Hola mundo.", Some(&out), 1.0).unwrap();
    assert!(std::fs::metadata(&out).unwrap().len() > 44);
}
```

- [ ] **Step 2: Ejecutar y ver fallar**

Run: `cargo test --test say`
Expected: FAIL — `wav`, `voices`, `cli::say` no existen

- [ ] **Step 3: Implementar**

`src/wav.rs` escribe la cabecera de 44 bytes a mano (RIFF/WAVE/fmt/data, PCM 16 bits, 1
canal). Sin dependencia: son 44 bytes detrás de un test.

`tts/voices.rs`: `discover` lista `*.onnx` con su `.onnx.json` hermano y saca el idioma del
nombre (`es_ES-…` → `es-ES`). `prepare` abre el ONNX con `onnx-protobuf`, busca el nodo
`Ceil`, añade su salida a `graph.output` si no está, y reescribe el archivo. `is_prepared`
es la comprobación que hace idempotente a `prepare`.

`cli/say.rs`: `Engine::load` → `synthesize` → `wav::write_pcm16_mono` a `--out` o a stdout.

- [ ] **Step 4: Ejecutar y ver pasar**

Run: `cargo test --test say && cargo run -- say "Hola mundo" -o /tmp/hola.wav && aplay /tmp/hola.wav`
Expected: 3 tests PASS (o `skip` sin voz); el WAV suena

- [ ] **Step 5: Medir R1 y escribir la decisión**

`docs/decisions/R1-tts-integration.md` debe contener: la ruta elegida (a) FFI a `libpiper`
o (b) `ort` + runner propio; el tiempo de `cargo build --release` limpio; si `ldd` del
binario no pide nada que no se empaquete; y el estado de la comprobación de AppImage
(que se cierra en M7, no aquí). El criterio del spec: si (a) construye de forma razonable y
no exige dependencias del sistema en tiempo de ejecución, se queda (a).

- [ ] **Step 6: Commit**

```bash
git add src/tts/voices.rs src/cli/say.rs src/wav.rs src/main.rs src/cli/mod.rs Cargo.toml tests/say.rs docs/decisions/
git commit -m "feat(tts): registro de voces, parche ONNX de alineacion, say y decision R1"
```

---

### Task 7: Timeline: `word_id` → rango de samples (la tesis)

**Files:**
- Create: `src/timeline/mod.rs`, `tests/timeline.rs`

**Interfaces:**
- Consumes: `tts::PhonemeChunk`, `doc::Word`.
- Produces:
  - `timeline::Span { pub word_id: u32, pub start_sample: usize, pub end_sample: usize, pub exact: bool }`
  - `timeline::Timeline { pub spans: Vec<Span> }`
  - `timeline::build(chunk: &tts::PhonemeChunk, words: &[doc::Word]) -> Timeline`
  - `timeline::Timeline::word_at(&self, sample: usize) -> Option<u32>`

- [ ] **Step 1: Escribir los tests que fallan**

`tests/timeline.rs`. Los chunks son sintéticos, hechos a mano: **cero audio, cero piper**.

```rust
/// 2 palabras. fonemas: [a,a,0] [_] [b,b,0]; ids: [1,1, 2, 3,3]; alineaciones: [10,10, 5, 20,20]
fn chunk_de_dos_palabras() -> PhonemeChunk { /* construir a mano */ }

#[test]
fn rangos_exactos_por_palabra() {
    let t = build(&chunk_de_dos_palabras(), &palabras(&["uno", "dos"]));
    assert_eq!(t.spans.len(), 2);
    assert!(t.spans.iter().all(|s| s.exact));
    assert_eq!((t.spans[0].start_sample, t.spans[0].end_sample), (0, 25));
    assert_eq!((t.spans[1].start_sample, t.spans[1].end_sample), (25, 70));
}

#[test]
fn invariantes_monotonia_sin_solape_y_cobertura() {
    let t = build(&chunk_de_dos_palabras(), &palabras(&["uno", "dos"]));
    for w in t.spans.windows(2) {
        assert!(w[0].end_sample <= w[1].start_sample);          // monotonía y sin solape
    }
    assert!(t.spans.last().unwrap().end_sample <= 70);           // no excede el total
    let ids: Vec<u32> = t.spans.iter().map(|s| s.word_id).collect();
    assert_eq!(ids, vec![0, 1]);                                 // cada palabra una vez
}

#[test]
fn expansion_de_numeros_degrada_a_frase_sin_desfasar() {         // la regla del §5
    // el texto tiene 2 palabras ("son" "42") pero el chunk trae 4 grupos de fonemas
    let t = build(&chunk_dos_palabras_cuatro_grupos(), &palabras(&["son", "42"]));
    assert_eq!(t.spans.len(), 2);
    assert!(t.spans.iter().any(|s| !s.exact), "lo no-1:1 debe quedar marcado");
    for w in t.spans.windows(2) { assert!(w[0].end_sample <= w[1].start_sample); }
}

#[test]
fn un_chunk_sin_palabras_da_un_timeline_vacio() {                // Review Focus 4
    let t = build(&chunk_de_dos_palabras(), &[]);
    assert!(t.spans.is_empty());
}

#[test]
fn word_at_mapea_sample_a_palabra() {
    let t = build(&chunk_de_dos_palabras(), &palabras(&["uno", "dos"]));
    assert_eq!(t.word_at(0), Some(0));
    assert_eq!(t.word_at(24), Some(0));
    assert_eq!(t.word_at(25), Some(1));
    assert_eq!(t.word_at(9999), None);
}
```

- [ ] **Step 2: Ejecutar y ver fallar**

Run: `cargo test --test timeline`
Expected: FAIL — `timeline` no existe

- [ ] **Step 3: Implementar `timeline::build`**

Algoritmo fijado (el implementador no puede adivinarlo):

1. **Recorrer los tres arrays en paralelo (lockstep)**, no el protocolo de `0`s: el
   `PhonemeChunk` ya viene aplanado (ver el contrato documentado en `src/tts/mod.rs`).
   Recorrer `i` sobre `(phonemes[i], phoneme_ids[i], alignments[i])` a la vez, y:
   - **excluir** el par BOS inicial (`phonemes[0]` = `'^'` con id 1, `phonemes[1]` = `'^'`
     con id 0) y el `'$'` final (id 2, EOS);
   - tener en cuenta que **cada fonema real aparece dos veces** (su id real y el id de pad
     `0`), así que la duración de un fonema está partida entre dos entradas.
2. **Agrupar en palabras de fonemas:** la frontera de palabra es una **tirada** de espacios
   (`' '`, U+0020), no un espacio suelto — el fonema espacio también emite su entrada real y
   su entrada de pad, o sea dos espacios seguidos. Cada grupo acumula la suma de **todas** sus
   alineaciones, incluidas las de pad (filtrar los pad partiría cada duración por la mitad).
   Invariante disponible para test: `sum(alignments) == samples.len()`.
3. **Casar con `words` en orden.**
   - Mismo número de grupos que de palabras → cada palabra es `exact: true`, con
     `start_sample` acumulado y `end_sample = start + suma`.
   - Números distintos → **con solo el conteo no se sabe dónde cayó la expansión**, así que
     **ninguna** palabra del trozo puede afirmar exactitud: el tramo entero se reparte entre
     todas las palabras, contiguo y sin solape, todas con `exact: false`. Así ninguna palabra
     se pierde y el desfase no se propaga.
4. `word_at(sample)` es búsqueda lineal (los capítulos son cortos; búsqueda binaria si algún
   día se mide que importa).

- [ ] **Step 4: Ejecutar y ver pasar**

Run: `cargo test --test timeline`
Expected: 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/timeline tests/timeline.rs
git commit -m "feat(timeline): mapeo exacto word_id a samples con degradacion a frase"
```

---

### Task 8: `script --timeline`

**Files:**
- Modify: `src/cli/script.rs`, `src/main.rs`, `tests/script.rs`
- Create: `Cargo` dep `serde_json`

**Interfaces:**
- Consumes: `doc::build`, `tts::Engine`, `timeline::build`.
- Produces: `cli::script::run_timeline(path: &Path, voice_onnx: &Path) -> anyhow::Result<String>`

- [ ] **Step 1: Escribir el test que falla**

En `tests/script.rs`:

```rust
#[test]
fn emite_timeline_valido_por_capitulo() {
    let Some(voz) = test_voice() else { return };
    let dir = tempdir();
    let out = cli::script::run_timeline(&common::minimal_epub(&dir), voz).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let span = &v[0]["spans"][0];
    assert!(span["start_sample"].as_u64().unwrap() <= span["end_sample"].as_u64().unwrap());
    assert!(v[0]["spans"].as_array().unwrap().len() >= 1);
}
```

Formato fijado: un array JSON, un objeto por capítulo, con `chapter`, `href`, `spans`
(`word_id`, `start_sample`, `end_sample`, `exact`).

- [ ] **Step 2: Ejecutar y ver fallar**

Run: `cargo test --test script emite_timeline`
Expected: FAIL — `run_timeline` no existe

- [ ] **Step 3: Implementar y cablear**

Carga la voz una vez, sintetiza **por frase** (agrupar las palabras de un párrafo hasta
puntuación final o 400 caracteres, igual que la agrupación ya probada del prototipo),
construye el timeline y serializa. `main.rs` enruta `--timeline`.

- [ ] **Step 4: Ejecutar y ver pasar**

Run: `cargo test --test script && cargo run -- script <tu-epub> --timeline | head`
Expected: PASS; JSON legible del libro real

- [ ] **Step 5: Commit**

```bash
git add src/cli/script.rs src/main.rs tests/script.rs Cargo.toml
git commit -m "feat(cli): script --timeline (mapeo palabra a samples en JSON)"
```

---

### Task 9: Caché de PCM y timeline en disco

**Files:**
- Create: `src/tts/cache.rs`

**Interfaces:**
- Consumes: `tts::PhonemeChunk`, `timeline::Timeline`.
- Produces:
  - `tts::cache::key(voice: &str, length_scale: f32, text: &str) -> String` (hex)
  - `tts::cache::Cache::new(root: PathBuf) -> Cache`
  - `tts::cache::Cache::default_root() -> PathBuf` (`~/.cache/readaloud/audio/`)
  - `tts::cache::Cache::get(&self, key: &str) -> Option<(PhonemeChunk, Option<Timeline>)>`
  - `tts::cache::Cache::put(&self, key: &str, chunk: &PhonemeChunk)`

- [ ] **Step 1: Escribir los tests que fallan**

En `src/tts/cache.rs`:

```rust
#[test]
fn roundtrip_conserva_muestras_y_alineaciones() { /* put + get + assert_eq de samples, phonemes, alignments */ }

#[test]
fn la_clave_depende_de_voz_rate_y_texto() {
    assert_ne!(key("v1", 1.0, "hola"), key("v2", 1.0, "hola"));
    assert_ne!(key("v1", 1.0, "hola"), key("v1", 1.3, "hola"));
    assert_ne!(key("v1", 1.0, "hola"), key("v1", 1.0, "adios"));
    assert_eq!(key("v1", 1.0, "hola"), key("v1", 1.0, "hola"));
}

#[test]
fn un_get_sin_entrada_es_none_y_no_crea_nada() { /* get en cache vacia -> None; el dir sigue vacio */ }
```

- [ ] **Step 2: Ejecutar y ver fallar**

Run: `cargo test --lib tts::cache::`
Expected: FAIL — `cache` no existe

- [ ] **Step 3: Implementar**

Dos archivos por entrada (dependencia `sha2`): `<key>.pcm` con los `i16` en little-endian
crudo, y `<key>.json` con el resto (`rate`, `phonemes`, `phoneme_ids`, `alignments`) vía
`serde_json`. `.json` se escribe **después** del `.pcm` y su presencia es lo que declara la
entrada completa: una entrada interrumpida a medias nunca se sirve.

- [ ] **Step 4: Ejecutar y ver pasar**

Run: `cargo test --lib tts::cache::`
Expected: 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/tts/cache.rs Cargo.toml
git commit -m "feat(tts): cache en disco de pcm y timeline por voz, rate y texto"
```

---

### Task 10: Player: FSM, sink y posición

**Files:**
- Create: `src/player/mod.rs`, `src/player/sink.rs`, `src/player/cpal.rs`

**Interfaces:**
- Consumes: `tts::PhonemeChunk`, `timeline::Timeline`.
- Produces:
  - `player::Sink` (`fn write(&mut self, samples: &[i16], rate: u32)`, `fn played_samples(&self) -> usize`, `fn stop(&mut self)`)
  - `player::NullSink::{new(counter: Arc<AtomicUsize>) -> NullSink, default() -> NullSink}`, `player::CpalSink::new() -> Result<CpalSink, PlayerError>`
  - `player::PlayerState { Idle, Buffering, Playing, Paused, Ended }`
  - `player::Sentence { pub first_word: u32, pub chunk: PhonemeChunk, pub timeline: Timeline }`
  - `player::Player::new(sink: Box<dyn Sink>) -> Player`
  - `player::Player::enqueue(&mut self, s: Sentence)`
  - `player::Player::tick(&mut self) -> PlayerState`
  - `player::Player::position_sample(&self) -> usize`
  - `player::Player::current_word(&self) -> Option<u32>`
  - `player::Player::pause(&mut self)`, `player::Player::resume(&mut self)`, `player::Player::seek_to_word(&mut self, word_id: u32)`

- [ ] **Step 1: Escribir los tests que fallan**

En `src/player/mod.rs`:

```rust
#[test]
fn la_fsm_recorre_idle_buffering_playing_ended() {
    let mut p = Player::new(Box::new(NullSink::default()));
    assert_eq!(p.tick(), PlayerState::Idle);          // sin nada encolado
    p.enqueue(sentencia(0, 50));                      // 50 samples
    assert_eq!(p.tick(), PlayerState::Playing);
    p.tick();
    assert_eq!(p.tick(), PlayerState::Ended);
}

#[test]
fn pausa_y_reanuda() {
    let mut p = Player::new(Box::new(NullSink::default()));
    p.enqueue(sentencia(0, 100));
    p.tick();
    p.pause();
    assert_eq!(p.tick(), PlayerState::Paused);
    p.resume();
    assert_eq!(p.tick(), PlayerState::Playing);
}

#[test]
fn current_word_sigue_la_posicion() {
    let avanzado = Arc::new(AtomicUsize::new(0));
    let mut p = Player::new(Box::new(NullSink::new(avanzado.clone())));
    p.enqueue(sentencia_con_timeline(0, &[(0, 0, 25), (1, 25, 70)]));
    p.tick();
    assert_eq!(p.current_word(), Some(0));
    avanzado.store(30, Ordering::SeqCst);   // el dispositivo ya reprodujo 30 samples
    p.tick();
    assert_eq!(p.current_word(), Some(1));
}

#[test]
fn seek_a_palabra_salta_al_sample_correcto() {
    let mut p = Player::new(Box::new(NullSink::default()));
    p.enqueue(sentencia_con_timeline(0, &[(0, 0, 25), (1, 25, 70)]));
    p.seek_to_word(1);
    p.tick();
    assert_eq!(p.position_sample(), 25);
}
```

- [ ] **Step 2: Ejecutar y ver fallar**

Run: `cargo test --lib player::`
Expected: FAIL — `player` no existe

- [ ] **Step 3: Implementar**

`Player` guarda una cola de `Sentence`, un índice y el sink. `tick` es **no bloqueante**: si
el sink ya consumió los samples de la frase actual, pasa a la siguiente y, si no queda
ninguna, `Ended`. `position_sample` es el contador acumulado más `sink.played_samples()`.
`current_word` delega en el `timeline` de la frase actual. `seek_to_word` busca la frase que
contiene el `word_id`, descarta las anteriores y fija el offset.

`CpalSink` abre el dispositivo por defecto con `cpal` y escribe en un buffer compartido; su
`played_samples` sale del contador del callback. Que el FSM no sepa nada de `cpal` es el
punto: los tests corren con `NullSink`.

- [ ] **Step 4: Ejecutar y ver pasar**

Run: `cargo test --lib player::`
Expected: 4 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/player Cargo.toml
git commit -m "feat(player): fsm de reproduccion, sink abstraido y posicion por sample"
```

---

### Task 11: `play` — leer un EPUB en voz alta desde el terminal

**Files:**
- Create: `src/cli/play.rs`, `tests/play.rs`
- Modify: `src/main.rs`, `src/cli/mod.rs`

**Interfaces:**
- Consumes: todo lo anterior.
- Produces:
  - `cli::play::run_with(path: &Path, voice: &Path, length_scale: f32, sink: Box<dyn Sink>, max_sentences: usize) -> anyhow::Result<PlaySummary>`
  - `cli::play::run_with_in(path: &Path, voice: &Path, length_scale: f32, sink: Box<dyn Sink>, max_sentences: usize, cache_root: &Path) -> anyhow::Result<PlaySummary>`
  - `cli::play::PlaySummary { pub sentences: usize, pub words_covered: usize, pub final_state: PlayerState }`
  - `cli::play::run(path: &Path, voice: &str, length_scale: f32) -> anyhow::Result<()>` (envoltorio con `CpalSink`)

- [ ] **Step 1: Escribir el test que falla**

`tests/play.rs`:

```rust
#[test]
fn lee_tres_frases_sin_dispositivo_de_audio() {
    let Some(voz) = test_voice() else { return };
    let dir = tempdir();
    let s = cli::play::run_with(&common::minimal_epub(&dir), voz, 1.0,
                                Box::new(NullSink::default()), 3).unwrap();
    assert_eq!(s.sentences, 3);
    assert!(s.words_covered > 0);
    assert_eq!(s.final_state, PlayerState::Ended);
}

#[test]
fn la_cache_evita_resintetizar() {
    let Some(voz) = test_voice() else { return };
    let dir = tempdir();
    let cache = tempdir();
    let libro = common::minimal_epub(&dir);
    let a = cli::play::run_with_in(&libro, voz, 1.0, Box::new(NullSink::default()), 3, &cache).unwrap();
    let tras_la_primera = count_entries(&cache);
    let b = cli::play::run_with_in(&libro, voz, 1.0, Box::new(NullSink::default()), 3, &cache).unwrap();
    assert_eq!(a.sentences, b.sentences);
    assert_eq!(count_entries(&cache), tras_la_primera, "la segunda pasada no escribe nada");
}
```

- [ ] **Step 2: Ejecutar y ver fallar**

Run: `cargo test --test play`
Expected: FAIL — `cli::play` no existe

- [ ] **Step 3: Implementar y cablear**

`play` recorre el documento por frases. Para cada frase: `cache::key` → `Cache::get`; si no
está, `Engine::synthesize` + `timeline::build` + `Cache::put`. Encola en el `Player` con una
frase de prefetch (sintetiza la N+1 antes de que la N termine). Bucle de `tick` hasta
`Ended` o hasta `max_sentences`. `main.rs` enruta `Command::Play`.

- [ ] **Step 4: Ejecutar y ver pasar**

Run: `cargo test --test play`
Expected: 2 tests PASS

- [ ] **Step 5: Verificación manual (M4)**

Run: `cargo run -- play <tu-epub> --voice es_ES-davefx-medium`
Expected: se oye el libro en voz alta a velocidad normal y sin cambio de tono; con
`--length-scale 0.77` va **más rápido** sin subir el tono (`length_scale` de piper escala
duración: **>1 es más lento, <1 es más rápido** — la formulación anterior de este paso estaba
invertida y el implementador la corrigió midiendo); Ctrl-C corta limpio.

- [ ] **Step 6: Commit**

```bash
git add src/cli/play.rs src/main.rs src/cli/mod.rs tests/play.rs
git commit -m "feat(cli): play, lectura en voz alta con prefetch y cache desde el terminal"
```

---

## Self-Review

**Cobertura de la spec.** §2 alcance → Tasks 2/3 (y `EpubError`/`coverage_bad_xhtml`).
§3 arquitectura y modos → Tasks 1/4/6/8/11. §4 `word_id` e invariantes → Tasks 3/7.
§5 motor y alineación → Tasks 5/6/7. §6 render/tema → **M5, fuera de este plan** (correcto).
§7 player → Task 10. §8 fuentes/storage → **M6, fuera** (correcto). §9 empaquetado → M7.
§10 tests → cada task. §12 R3 → Task 5, R1 → Task 6.

**Consistencia de tipos.** `PhonemeChunk` se define en el task 5 y lo consumen 7, 8, 9, 10,
11 con los mismos nombres. `Word`/`Document` del task 3 los consumen 4, 7, 8, 11. `Span`/
`Timeline` del task 7 los consumen 8, 9, 10. `Sink`/`NullSink` del task 10 los consume 11.

**Proporción.** Sin cuerpos de implementación salvo el algoritmo del timeline (task 7) y las
reglas de tokenización (task 3), que son decisiones que el implementador no puede adivinar.

**Review Focus.** Cada una de las cinco líneas tiene su test en el task dueño (1→Task 3,
2→Task 3, 3→Task 2, 4→Tasks 3 y 7, 5→Task 3).
