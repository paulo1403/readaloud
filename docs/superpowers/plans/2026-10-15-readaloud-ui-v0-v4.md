# readaloud M5 (V0–V4) — Plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Una ventana GTK4 que abre un EPUB, lo compone con Pango, y lee en voz alta iluminando la palabra exacta que suena — con la vista siguiéndola.

**Architecture:** El render es un widget propio sobre Pango, alimentado por un modelo de layout puro (`word_id → Rect`) que se testea sin GTK. La orquestación de lectura se extrae del CLI a `session/` y vive entera en un hilo trabajador, porque el motor de voz no es reentrante. La GUI solo lee un atómico de posición y pinta.

**Tech Stack:** Rust 2024 · `gtk4` · `pangocairo` · `cpal` · `rusqlite` (V5) · el motor FFI a `libpiper` ya existente.

**Spec:** `docs/superpowers/specs/2026-10-15-readaloud-ui-design.md` (y el diseño padre `2026-10-13-readaloud-design.md` como autoridad de lo ya decidido)

## Global Constraints

- Un crate, un binario. Sin workspace. Licencia `GPL-3.0-or-later`.
- Linux de escritorio. **GTK4 + Pango. Sin WebView.**
- **Scroll continuo. Sin paginado.**
- **El motor NO es reentrante:** un `Engine` por proceso; un segundo `load` se cuelga (`src/tts/engine.rs`). Nunca dos.
- `word_id: u32` es la moneda única: global, contiguo desde 0, estable para el mismo fichero.
- **`Span.exact` es contractual:** si es `false` no se resalta palabra, solo la frase. La UI no inventa resaltado donde el motor dijo "no sé".
- La velocidad se controla con `length_scale` de piper y re-síntesis. **Prohibido el estiramiento temporal.**
- Nada fuera de alcance puede tumbar la ventana: se degrada y se registra cobertura.
- Ficheros: config `~/.config/readaloud/config.json` · datos `~/.local/share/readaloud/library.db` · caché de audio `~/.cache/readaloud/audio/` · portadas `~/.cache/readaloud/covers/`.
- Colores: los del tema de Omarchy; **si no hay paleta, una por defecto**. La app nunca falla por un tema.
- Tests: cada pieza no trivial deja **un** check ejecutable. Los tests del motor que necesitan voz se corren con `READALOUD_TEST_VOICE=~/.local/share/piper-voices/es_ES-davefx-medium.onnx`; `READALOUD_REQUIRE_VOICE=1` convierte el salto en fallo.
- **Este plan cubre V0–V4.** La librería, el store, el tema y las anotaciones (V5–V6) van en un plan posterior. En consecuencia, aquí **no** hay SQLite ni importación de libros: el libro se abre por argumento o diálogo de fichero.

## Review Focus

Clases de entrada que la spec implica y que ningún task cubre por defecto. Cada una lleva su test en el task dueño.

1. **CSS fuera del subconjunto** (selectores con descendencia `p em`, `#id`, shorthand `font:`, `!important`, `@media`): un libro real los usa y no puede perder el estilo en silencio ni romper el layout. Esperado: se aplica lo que casa por tag/clase, lo demás cuenta cobertura, nada revienta. → Task 5.
2. **Fuente que no existe** (familia no instalada, o `font-family` con una lista entera ausente): el texto debe seguir legible con el fallback. → Task 7.
3. **Capítulo enorme** (decenas de miles de caracteres, un solo párrafo): el layout debe terminar y producir rects, no colgarse. → Task 7.
4. **Redimensionar la ventana**: los rects se recalculan y el resaltado sigue sobre la palabra correcta, no en la posición vieja. → Task 7.
5. **Una palabra nunca se parte entre runs** (énfasis o cursiva a mitad de palabra: `pa<b>la</b>bra`): si se partiera, el resaltado pintaría medio resaltado. → Task 1.

---

## File Structure

```
src/
  doc/
    mod.rs        # + Block, Run, Word.block   (structure: blocks, words, sin estilos)
  epub/
    mod.rs        # + resolve() normalizado
    toc.rs        # NUEVO: nav (EPUB3) / NCX (EPUB2) → índice
  player/
    mod.rs        # seek_to_word coherente
    sink.rs       # trait Sink sin el parámetro rate
  session/
    mod.rs        # NUEVO: ReadSession (extraído de cli/play)
  render/
    css.rs        # NUEVO: subconjunto de CSS → Properties
    style.rs      # NUEVO: Properties + tag/clase → atributos de Pango · Palette
    layout.rs     # NUEVO: modelo puro: bloques → líneas → word_id → Rect
    view.rs       # NUEVO: el widget GTK4 (pinta el modelo, scroll, selección, auto-scroll)
  app/
    mod.rs        # NUEVO (Task 8): GApplication, ventana, Stack · Task 11: barras · Task 14: instancia unica
    worker.rs     # NUEVO: el hilo trabajador que posee la ReadSession
  cli/
    play.rs       # pasa a ser cáscara sobre session/
    mod.rs        # + OpenArgs (Task 8) · PlayArgs igual
tests/
  doc.rs epub.rs timeline.rs script.rs play.rs say.rs   # existentes, se amplían
  render_css.rs render_layout.rs                        # NUEVOS
```

---

### Task 1: `doc/` — bloques y runs del DOM

Resuelve la deuda §14.1: `paragraph` cuenta párrafos *con palabras*, y el render necesita los
bloques reales. Además los runs deben existir para el énfasis sin partir palabras.

**Files:**
- Modify: `src/doc/mod.rs`, `src/doc/tokenize.rs`
- Test: `tests/doc.rs`

**Interfaces:**
- Consumes: nada nuevo.
- Produces:
  - `doc::BlockKind { Paragraph, Heading(u8), Quote, ListItem, Image }`
  - `doc::Block { pub kind: BlockKind, pub tag: String, pub class: Option<String>, pub word_ids: Range<u32>, pub runs: Vec<Run> }`
  - `doc::Run { pub text: String, pub bold: bool, pub italic: bool, pub first_word: Option<u32> }`
  - `doc::Word { pub id, pub chapter, pub block: u32, pub text, pub range }` — **`paragraph` se renombra a `block`** y cuenta bloques del DOM.
  - `doc::Document { pub chapters, pub blocks: Vec<Block>, pub words, pub coverage_bad_xhtml }`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn el_bloque_avanza_con_bloques_vacios() {          // deuda §14.1
    let d = build_from_html(&["<p>Uno</p>", "<div></div>", "<p>Dos</p>"]);
    assert_eq!(d.words()[0].block, 0);
    assert_eq!(d.words()[1].block, 2, "el div vacio, bloque 1, cuenta");
}

#[test]
fn una_palabra_no_se_parte_entre_runs() {            // Review Focus 5
    let d = build_from_html(&["<p>pa<b>la</b>bra</p>"]);
    let runs: Vec<&str> = d.blocks[0].runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(runs, vec!["palabra"], "el enfasis a mitad de palabra no la parte");
    assert_eq!(d.words().len(), 1);
}

#[test]
fn los_bloques_cubren_todas_las_palabras_en_orden() {
    let d = build_from_html(&["<h1>Titulo</h1>", "<p>Dos palabras</p>"]);
    assert!(matches!(d.blocks[0].kind, doc::BlockKind::Heading(1)));
    let cubiertos: Vec<u32> = d.blocks.iter().flat_map(|b| b.word_ids.clone()).collect();
    assert_eq!(cubiertos, (0..d.words().len() as u32).collect::<Vec<_>>());
}
```

*Los tests existentes de `doc` y `script` usan `paragraph`: hay que actualizarlos al nombre nuevo y
al formato `word_id\tchapter\tblock\ttext`. Ese cambio de formato es deliberado y va en este commit.*

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test doc`
Expected: FAIL — no existe `block`, ni `Block`, ni `Run`

- [ ] **Step 3: Implement**

`tokenize.rs` deja de emitir solo palabras: recorre el XHTML produciendo **bloques** (con su tag y
clase) y, dentro, **runs** (tiradas de texto con estilo inline uniforme) y palabras. Un run se
cierra en un cambio de estilo inline (`em`, `strong`) **y en todo límite de palabra**: si el
cambio cae a mitad de palabra, el run no se parte y el último estilo gana para esa palabra
(Review Focus 5). `Block.word_ids` es el rango contiguo que le toca.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test doc --test script && cargo test`
Expected: PASS; la suite de M0–M4 sigue verde

- [ ] **Step 5: Commit**

```bash
git add src/doc tests/doc.rs tests/script.rs src/cli/script.rs
git commit -m "feat(doc): bloques y runs del DOM; paragraph pasa a block"
```

---

### Task 2: `epub::resolve` — abrir los libros reales

Resuelve §14.2: hoy un href con `../` o `%20` no encuentra la entrada del ZIP y **el libro entero
falla al abrir**.

**Files:**
- Modify: `src/epub/mod.rs`
- Test: `tests/epub.rs`

**Interfaces:**
- Produces: `epub::resolve(base: &str, href: &str) -> String` (pública, testeable) y el OPF usando
  el resultado.
- Consumes: nada.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn resuelve_punto_punto() {
    assert_eq!(epub::resolve("OEBPS/content.opf", "../Text/cap.xhtml"), "Text/cap.xhtml");
    assert_eq!(epub::resolve("content.opf", "OEBPS/cap.xhtml"), "OEBPS/cap.xhtml");
}

#[test]
fn decodifica_porcentajes() {
    assert_eq!(epub::resolve("c.opf", "Text/cap%201.xhtml"), "Text/cap 1.xhtml");
}

#[test]
fn un_libro_con_hrefs_punto_punto_abre() {
    let dir = tempdir();
    let path = common::epub_con_href_relativo(&dir);   // spine: ../Text/cap1.xhtml
    let book = epub::open(&path).unwrap();
    assert_eq!(book.chapters.len(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test epub`
Expected: FAIL — `resolve` no existe

- [ ] **Step 3: Implement**

`resolve` une por directorio, normaliza `.` y `../` con lógica de pila, y decodifica `%XX`. Un
`../` que suba por encima de la raíz se recorta a la raíz (no se sale del ZIP). Añadir
`common::epub_con_href_relativo` al fixture.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test epub`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/epub tests/epub.rs tests/common/mod.rs
git commit -m "fix(epub): resolve normaliza ../ y decodifica porcentajes"
```

---

### Task 3: `Player::seek_to_word` — posición coherente

Resuelve §14.3: hoy el seek deja `finished` con la posición **local** de la frase, así que
`position_sample` deja de ser la posición global del stream. La UI busca de verdad (arrastrar el
progreso, saltar por el índice).

**Files:**
- Modify: `src/player/mod.rs`
- Test: `src/player/mod.rs` (inline)

**Interfaces:**
- Produces: `Player::position_sample()` que tras `seek_to_word` es la posición global coherente
  con `current_word()`; `Player::seek_to_word(&mut self, word_id: u32) -> bool` (devuelve si lo
  encontró).
- Consumes: `Sentence { first_word, chunk, timeline }`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn tras_seek_la_posicion_y_la_palabra_concuerdan() {
    let mut p = Player::new(Box::new(NullSink::instant()));
    p.enqueue(sentencia_con_timeline(0, &[(0, 0, 25), (1, 25, 70)]));
    p.enqueue(sentencia_con_timeline(2, &[(2, 0, 40), (3, 40, 90)]));
    assert!(p.seek_to_word(3));
    p.tick();
    assert_eq!(p.current_word(), Some(3));
    // la posicion global debe caer dentro del span de la palabra 3, no reiniciarse a 0
    let s = p.position_sample();
    assert!(s >= 40, "posicion global coherente, no local: {s}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib player::tras_seek`
Expected: FAIL — la posición vuelve a un valor local

- [ ] **Step 3: Implement**

`seek_to_word` acumula en `finished` los samples de las frases descartadas más el desplazamiento
dentro de la frase destino, de forma que `position_sample()` y `current_word()` midan lo mismo.
Documentar en el doc comment que la posición es **global del libro**.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib player::`
Expected: PASS, incluidos los tests previos de FSM y pausa

- [ ] **Step 5: Commit**

```bash
git add src/player/mod.rs
git commit -m "fix(player): seek_to_word deja la posicion global coherente con current_word"
```

---

### Task 4: Limpieza mecánica — `Sink::write` sin `rate` y ayuda de `say`

§14.4 y §14.5. Van juntas porque son del mismo tipo: ediciones pequeñas y mecánicas.

**Files:**
- Modify: `src/player/sink.rs`, `src/player/cpal.rs`, `src/player/mod.rs`, `src/cli/mod.rs`
- Test: `src/player/mod.rs` (inline)

**Interfaces:**
- Cambia: `Sink::write(&mut self, samples: &[i16])` — **sin** el parámetro `rate`.
- `SayArgs.length_scale` gana `#[arg(help = "por debajo de 1 habla más rápido y por encima de 1 más despacio (1.3 = más lento); no cambia el tono")]`.

- [ ] **Step 1: Write the failing test**

Añadir a `src/player/mod.rs`:

```rust
#[test]
fn el_sink_no_recibe_la_tasa() {
    // El trait ya no la declara: este test no compila si alguien la vuelve a meter
    fn acepta_un_sink_de_un_solo_argumento(s: &mut dyn Sink, pcm: &[i16]) { s.write(pcm) }
    let mut n = NullSink::instant();
    acepta_un_sink_de_un_solo_argumento(&mut n, &[0i16; 4]);
    assert_eq!(n.played_samples(), 4);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib player::el_sink_no_recibe`
Expected: FAIL — `write` pide dos argumentos

- [ ] **Step 3: Implement**

Quitar el parámetro del trait y de las dos implementaciones y de todas las llamadas. `CpalSink`
sigue abriendo el stream a la tasa que recibe en `new(rate)` — eso no cambia. Añadir la ayuda al
`--length-scale` de `say` (no cambia comportamiento).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test && cargo run -- say --help | grep -A1 length-scale`
Expected: PASS; el `--help` explica la dirección

- [ ] **Step 5: Commit**

```bash
git add src/player src/cli/mod.rs
git commit -m "refactor(player): quitar el rate muerto de Sink::write; documentar length_scale en say"
```

---

### Task 5: `render/css` — subconjunto de CSS → propiedades

**Files:**
- Create: `src/render/mod.rs`, `src/render/css.rs`
- Test: `tests/render_css.rs`

**Interfaces:**
- Produces:
  - `render::css::Stylesheet::parse(css: &str) -> Stylesheet`
  - `render::css::Stylesheet::properties_for(&self, tag: &str, class: Option<&str>) -> Properties`
  - `render::css::Properties { family: Option<String>, size_pt: Option<f32>, weight: Option<u16>, italic: Option<bool>, color: Option<String>, align: Option<Align>, text_indent_em: Option<f32>, margin_top_pt: Option<f32>, margin_bottom_pt: Option<f32>, line_height: Option<f32> }`
  - `render::css::Stylesheet::coverage_missed(&self) -> u32`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn aplica_por_tag_y_por_clase() {
    let s = Stylesheet::parse("p { font-size: 12pt; } .nota { color: #888; font-style: italic; }");
    assert_eq!(s.properties_for("p", None).size_pt, Some(12.0));
    assert_eq!(s.properties_for("p", Some("nota")).color.as_deref(), Some("#888"));
    assert_eq!(s.properties_for("p", Some("nota")).italic, Some(true));
}

#[test]
fn lo_ultimo_que_casa_gana() {
    let s = Stylesheet::parse("p { color: #111; } p { color: #222; }");
    assert_eq!(s.properties_for("p", None).color.as_deref(), Some("#222"));
}

#[test]
fn el_css_fuera_del_subconjunto_cuenta_y_no_rompe() {     // Review Focus 1
    let s = Stylesheet::parse("@media print { p { color: #000 } } p em { color: #f00 } #x { color: #0f0 } p { font: 12pt/1.5 serif }");
    let p = s.properties_for("p", None);
    assert!(p.color.is_none(), "ni @media ni el shorthand se aplican a p");
    assert!(s.coverage_missed() >= 3, "cuenta lo ignorado");
    assert_eq!(s.properties_for("p", None).size_pt, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test render_css`
Expected: FAIL — `render::css` no existe

- [ ] **Step 3: Implement**

Selectores soportados: `tag`, `.clase`, `tag.clase`. Sin descendencia, sin combinadores, sin `#id`;
sin `!important`; sin shorthands (`font:`, `margin:`) ni at-rules. Gana el **último que casa** en
orden de documento. Unidades: `pt` y `px` (1px = 0.75pt); `em` en `text-indent` y `line-height`
como múltiplo sin unidad; `%` no soportado. Todo lo no soportado incrementa `coverage_missed` y se
ignora — sin pánicos y sin `unwrap` sobre el texto de entrada.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test render_css`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/render/mod.rs src/render/css.rs tests/render_css.rs src/lib.rs
git commit -m "feat(render): subconjunto de CSS a propiedades, con cobertura de lo ignorado"
```

---

### Task 6: `render/style` — propiedades a Pango, y la paleta

**Files:**
- Create: `src/render/style.rs`
- Test: `tests/render_style.rs` (o inline)

**Interfaces:**
- Consumes: `render::css::{Properties, Align}`, `doc::Block`.
- Produces:
  - `render::style::PangoAttrs { family: String, size_pt: f32, weight: u16, italic: bool, color: String, align: Align, text_indent_em: f32, margin_top_pt: f32, margin_bottom_pt: f32, line_height: f32 }`
  - `render::style::resolve(props: &Properties, base: &PangoAttrs) -> PangoAttrs` (la cascada: lo que el bloque trae gana sobre la base)
  - `render::style::base_attrs(font_family: &str, size_pt: f32, line_height: f32) -> PangoAttrs`
  - `render::style::Palette { page, text, muted, accent, highlight_sentence, highlight_word, chrome, rule: String }` con `Palette::default_dark()`

`Palette` es la costura que V5 llenará desde el JSON de Omarchy. En este plan, `default_dark()` usa
los valores del tema verificado en el diseño `#222222 / #c2c2b0 / #8a8a7e / #78824b`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn lo_del_bloque_gana_sobre_la_base() {
    let base = base_attrs("Georgia", 12.0, 1.75);
    let props = Properties { size_pt: Some(15.0), italic: Some(true), ..Default::default() };
    let a = resolve(&props, &base);
    assert_eq!(a.size_pt, 15.0);
    assert!(a.italic);
    assert_eq!(a.family, "Georgia", "lo que el bloque no dice se hereda");
}

#[test]
fn la_paleta_por_defecto_tiene_los_ocho_tokens() { /* no vacios, y distintos entre si */ }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test render_style`
Expected: FAIL

- [ ] **Step 3: Implement**

`Default` para `Properties` (todo `None`). `resolve` copia la base y sobreescribe lo presente.
`Palette::default_dark` devuelve los valores del tema verificado.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test render_style`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/render/style.rs tests/render_style.rs
git commit -m "feat(render): resolucion de estilo a atributos Pango y paleta por defecto"
```

---

### Task 7: `render/layout` — el modelo puro: `word_id → Rect`

Esta es **la tesis en su forma testeable**: sin GTK, con Pango, produce dónde va cada palabra.

**Files:**
- Create: `src/render/layout.rs`
- Test: `tests/render_layout.rs`

**Interfaces:**
- Consumes: `doc::{Block, Word}`, `render::style::PangoAttrs`.
- Produces:
  - `render::layout::Layout { pub lines: Vec<Line>, pub words: HashMap<u32, Rect> }`
  - `render::layout::Line { pub rect: Rect, pub text: String }`
  - `render::layout::Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }`
  - `render::layout::layout_chapter(blocks: &[Block], words: &[Word], attrs: &PangoAttrs, width: f64) -> Layout`
  - `render::layout::Layout::word_at(&self, rect: Rect) -> Option<u32>` (para el hit-testing de selección)

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn cada_palabra_recibe_un_rect_dentro_del_ancho() {
    let l = layout_de("Un texto corto de prueba.", &base_attrs("Georgia", 12.0, 1.75), 300.0);
    assert_eq!(l.words.len(), 5);
    for r in l.words.values() { assert!(r.x >= 0.0 && r.x + r.w <= 300.0 + 0.5); }
}

#[test]
fn una_fuente_inexistente_no_rompe_el_layout() {          // Review Focus 2
    let l = layout_de("Texto de prueba.", &base_attrs("NoExisteEstaFuenteXYZ", 12.0, 1.75), 300.0);
    assert_eq!(l.words.len(), 3);
    assert!(l.lines[0].rect.h > 0.0);
}

#[test]
fn un_capitulo_enorme_termina() {                          // Review Focus 3
    let texto = "palabra ".repeat(20_000);
    let l = layout_de(&texto, &base_attrs("Georgia", 12.0, 1.75), 600.0);
    assert_eq!(l.words.len(), 20_000);
}

#[test]
fn al_redimensionar_los_rects_se_recalculan() {            // Review Focus 4
    let a = layout_de("Una frase larga que ocupa varias lineas de texto.", &base_attrs("Georgia", 12.0, 1.75), 600.0);
    let b = layout_de("Una frase larga que ocupa varias lineas de texto.", &base_attrs("Georgia", 12.0, 1.75), 260.0);
    assert!(b.lines.len() > a.lines.len(), "menos ancho, mas lineas");
    assert_ne!(a.words[&0].y, b.words[&0].y, "el rect de la palabra 0 cambio de sitio");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test render_layout`
Expected: FAIL — `render::layout` no existe

- [ ] **Step 3: Implement**

Con `pangocairo` pero **sin GTK**: un `FontMap` de `pangocairo::FontMap::default()`, un
`Layout` por bloque, envoltura a `width`. Recorrer las líneas y las palabras del layout de Pango
(por los índices de texto que devuelve `Layout::line`/`Layout::xy_to_index`) para poblar
`words: word_id → Rect`. Los `word_id` vienen de `Block.word_ids` y del orden de las palabras en
`Word.text`, así que el índice del recorrido se mapea al `word_id` real. Añadir `pangocairo` a
`Cargo.toml`.

Documentar el techo: el layout es **por capítulo completo** y se recalcula al cambiar ancho o
tipografía; un capítulo patológico de un solo párrafo enorme tarda lo que tarde — medido en el
test de arriba.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test render_layout`
Expected: PASS (el de 20 000 palabras es el más lento; debe terminar en segundos)

- [ ] **Step 5: Commit**

```bash
git add src/render/layout.rs tests/render_layout.rs Cargo.toml
git commit -m "feat(render): layout puro con Pango, word_id a Rect, probado sin GTK"
```

---

### Task 8: `render/view` — el widget que pinta

Sin tests unitarios: es GTK. La verificación es ver la ventana.

**Files:**
- Create: `src/render/view.rs`, `src/app/mod.rs`
- Modify: `src/main.rs`, `src/cli/mod.rs` (el subcomando `open`, mínimo)

**Interfaces:**
- Consumes: `render::layout::{Layout, layout_chapter}`, `doc::{Document, Block, Word}`, `render::style::{PangoAttrs, Palette}`.
- Produces:
  - `render::view::ReaderView::new(palette: Palette)` (subclase de `gtk4::Widget`)
  - `render::view::ReaderView::set_chapter(&self, blocks: &[Block], words: &[Word], attrs: &PangoAttrs)`
  - `render::view::ReaderView::scroll_to_word(&self, word_id: u32)`
  - `app::window_with_reader(document: &Document) -> gtk4::ApplicationWindow` (Stack con solo el lector; la librería llega en V5)
  - `app::run(path: Option<&Path>) -> anyhow::Result<()>`
  - `cli::OpenArgs { pub path: Option<PathBuf> }` y `Command::Open(OpenArgs)`

- [ ] **Step 1: Implement the widget**

Subclase de `gtk4::Widget` con `measure` (alto según el layout) y `snapshot` (pinta las líneas:
texto, y los rects que luego usará el resaltado). Vive dentro de un `GtkScrolledWindow`. El ancho
disponible viene del `measure`/`allocate`; al cambiar, se recalcula el layout de Task 7 y se
invalida el widget.

- [ ] **Step 2: Verify by running it**

Run: `cargo run -- open "/home/paulo/Documents/ebooks/¿Sueñan los Androides con Ovejas Eléctricas (Dick, Philip K) (z-library.sk, 1lib.sk, z-lib.sk).epub"`
Expected: la ventana muestra el capítulo con la tipografía del libro, se puede scrollear, y al
redimensionar el texto se reflowea sin quedarse en blanco

- [ ] **Step 3: Commit**

```bash
git add src/render/view.rs src/app/mod.rs src/main.rs src/cli/mod.rs
git commit -m "feat(render): widget GTK4 que pinta el layout y reflowea al redimensionar"
```

---

### Task 9: `render/view` — el resaltado, con la regla `exact`

El corazón del proyecto. La regla: si el span es `exact` se pinta la palabra dentro de su frase; si
**no**, solo la frase.

**Files:**
- Modify: `src/render/view.rs`
- Test: `src/render/view.rs` (solo el trozo puro: qué se pinta)

**Interfaces:**
- Produces:
  - `render::view::Highlight { pub sentence: Option<(u32, u32)>, pub word: Option<u32> }`
  - `render::view::highlight_for(span: &timeline::Span, sentence: &[timeline::Span]) -> Highlight`
  - `ReaderView::set_highlight(&self, h: &Highlight, palette: &Palette)`

- [ ] **Step 1: Write the failing test** (puro, sin GTK)

```rust
#[test]
fn exact_pinta_palabra_y_frase() {
    let frase = [span(0, true), span(1, true), span(2, true)];
    let h = highlight_for(&frase[1], &frase);
    assert_eq!(h.word, Some(1));
    assert_eq!(h.sentence, Some((0, 2)));
}

#[test]
fn degradado_pinta_solo_la_frase() {              // la regla contractual del motor
    let frase = [span(0, false), span(1, false), span(2, false)];
    let h = highlight_for(&frase[1], &frase);
    assert_eq!(h.word, None, "el motor no afirma palabra: la UI no la inventa");
    assert_eq!(h.sentence, Some((0, 2)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib render::view`
Expected: FAIL — `highlight_for` no existe

- [ ] **Step 3: Implement**

`highlight_for` es puro: si el span que suena es `exact`, devuelve palabra y el par que delimita la
frase; si no, solo la frase. En `snapshot`, pintar primero el rect de la frase en
`highlight_sentence` y encima el de la palabra en `highlight_word`, ambos como rectángulos
redondeados de `Rect` de Task 7. Sin palabra actual, no se pinta nada.

- [ ] **Step 4: Verify**

Run: `cargo test --lib render::view` y luego, con un timeline real, ver la ventana
Expected: PASS; al reproducir, la frase se tinta y la palabra que suena va más fuerte; en un
párrafo con un número (degradado) se tinta la frase sin palabra

- [ ] **Step 5: Commit**

```bash
git add src/render/view.rs
git commit -m "feat(render): resaltado de dos niveles que respeta la bandera exact"
```

---

### Task 10: `epub/toc` — el índice

**Files:**
- Create: `src/epub/toc.rs`
- Test: `tests/epub.rs`

**Interfaces:**
- Consumes: `epub::Book`.
- Produces:
  - `epub::toc::Entry { pub label: String, pub href: String, pub depth: u8 }`
  - `epub::toc::entries(book: &Book) -> Vec<Entry>` — lee `nav` (EPUB 3) o NCX (EPUB 2); si no hay ninguno, una entrada por capítulo con el href como etiqueta

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn saca_el_indice_del_nav_y_si_no_hay_del_spine() {
    let dir = tempdir();
    let b = epub::open(&common::minimal_epub(&dir)).unwrap();
    let e = epub::toc::entries(&b);
    assert!(!e.is_empty());
    assert_eq!(e[0].href, "cap1.xhtml", "sin nav: una entrada por capitulo");
}

#[test]
fn saca_el_indice_de_un_nav_de_verdad() {
    let dir = tempdir();
    let b = epub::open(&common::epub_con_nav(&dir)).unwrap();
    let e = epub::toc::entries(&b);
    assert_eq!(e[0].label, "Capitulo uno");
    assert_eq!(e[0].depth, 0);
    assert_eq!(e[1].depth, 1, "el anidado lleva profundidad");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test epub saca_el_indice`
Expected: FAIL — `epub::toc` no existe

- [ ] **Step 3: Implement**

Añadir `common::epub_con_nav` al fixture. Parsear con `roxmltree`: el `nav` con
`epub:type="toc"`, o el NCX (`navMap` → `navPoint`), con `depth` por anidamiento. Los `href` pasan
por `epub::resolve`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test epub`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/epub/toc.rs src/lib.rs tests/epub.rs tests/common/mod.rs
git commit -m "feat(epub): indice desde nav/NCX con fallback al spine"
```

---

### Task 11: `render/view` — auto-scroll, selección e índice

**Files:**
- Modify: `src/render/view.rs`, `src/app/mod.rs`
- Test: `src/render/view.rs` (la decisión del auto-scroll, pura)

**Interfaces:**
- Produces:
  - `render::view::auto_scroll_target(word_rect: Rect, visible: Rect, viewport_h: f64) -> Option<f64>` — `None` si ya está visible
  - `ReaderView::set_follow(&self, on: bool)`
  - `ReaderView::selection_words(&self, a: Point, b: Point) -> Option<(u32, u32)>`
  - `app::window(book: &Book, document: &Document) -> gtk4::ApplicationWindow` con `GtkStack` (lector) y una barra superior con `← Mi biblioteca` (inactivo aquí), `☰ Índice`, `Aa`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn no_desplaza_si_la_palabra_ya_es_visible() {
    let r = Rect { x: 10.0, y: 100.0, w: 40.0, h: 20.0 };
    let v = Rect { x: 0.0, y: 0.0, w: 600.0, h: 800.0 };
    assert_eq!(auto_scroll_target(r, v, 800.0), None);
}

#[test]
fn desplaza_dejando_la_palabra_en_el_tercio_superior() {
    let r = Rect { x: 10.0, y: 1200.0, w: 40.0, h: 20.0 };
    let v = Rect { x: 0.0, y: 0.0, w: 600.0, h: 800.0 };
    let t = auto_scroll_target(r, v, 800.0).unwrap();
    assert!((t - (r.y - 800.0 / 3.0)).abs() < 1.0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib render::view::auto_scroll`
Expected: FAIL

- [ ] **Step 3: Implement**

`auto_scroll_target` es puro. `set_follow(on)` decide si el timeout que sigue la voz aplica el
desplazamiento. La selección se resuelve con `Layout::word_at` sobre los dos extremos del arrastre.
El popover del índice lista `epub::toc::entries` y al elegir una entrada salta a la primera palabra
de ese capítulo.

- [ ] **Step 4: Verify**

Run: `cargo test --lib render::view && cargo run -- open <libro>`
Expected: al dar play, si la palabra sale de pantalla la vista baja sola y la deja arriba; arrastrar
selecciona texto; el índice salta de capítulo

- [ ] **Step 5: Commit**

```bash
git add src/render/view.rs src/app/mod.rs
git commit -m "feat(render): auto-scroll que sigue la voz, seleccion e indice"
```

---

### Task 12: `session/` — extraer la orquestación del CLI

Refactor puro: `cli/play` queda como cáscara. Sus tests actuales **no cambian de comportamiento**.

**Files:**
- Create: `src/session/mod.rs`
- Modify: `src/cli/play.rs`, `src/lib.rs`
- Test: `tests/play.rs` (debe seguir verde sin tocar sus asserts)

**Interfaces:**
- Consumes: `tts::{Engine, voices::Voice}`, `tts::cache::Cache`, `timeline::build`, `player::{Player, Sink, PlayerState}`, `doc::Document`.
- Produces:
  - `session::ReadSession`
  - `session::ReadSession::open(book: &Book, document: &Document, voice: &Path, length_scale: f32, sink: Box<dyn Sink>, cache: Cache) -> Result<Self>`
  - `ReadSession::{next_sentence() -> Result<bool>, tick() -> PlayerState, position_sample() -> usize, current_word() -> Option<u32>, set_rate(f32), seek_to_word(u32) -> bool, state() -> PlayerState}`
  - `session::sentence_groups(document: &Document) -> Vec<Vec<u32>>` (los `word_id` de cada frase)

- [ ] **Step 1: Verify the existing tests are the spec**

Run: `READALOUD_TEST_VOICE=~/.local/share/piper-voices/es_ES-davefx-medium.onnx cargo test --test play`
Expected: PASS antes de empezar (línea base)

- [ ] **Step 2: Move the code**

Mover a `session/` el agrupado de frases (`frases`/`grupos_de_frase` de `cli/play.rs`), el bucle de
prefetch con una sola `Engine`, la consulta a la caché, el `timeline::build` y el guardián de
estancamiento. `cli/play.rs` construye una `ReadSession` y la itera con su `CpalSink`, conservando
su interfaz pública (`run`, `run_with`, `run_with_in`, `PlaySummary`) y sus mensajes.

- [ ] **Step 3: Run the tests**

Run: `READALOUD_TEST_VOICE=… cargo test --test play && cargo test`
Expected: PASS sin cambiar ni un assert de `tests/play.rs`

- [ ] **Step 4: Verify the CLI still plays**

Run: `cargo run -- play <libro> --voice es_ES-davefx-medium` y Ctrl-C
Expected: suena igual que antes

- [ ] **Step 5: Commit**

```bash
git add src/session src/cli/play.rs src/lib.rs
git commit -m "refactor(session): extraer la orquestacion de lectura del CLI a session/"
```

---

### Task 13: `app/worker` — el motor fuera del hilo de dibujo

**Files:**
- Create: `src/app/worker.rs`
- Modify: `src/app/mod.rs`
- Test: `src/app/worker.rs` (el protocolo, con sesión falsa)

**Interfaces:**
- Consumes: `session::ReadSession`, `player::Sink`, `tts::cache::Cache`.
- Produces:
  - `app::worker::Command { Play, Pause, Next, Prev, Seek(u32), SetRate(f32), Stop }`
  - `app::worker::Handles { pub commands: Sender<Command>, pub word: Arc<AtomicU32>, pub state: Arc<AtomicU8> }` — `word` usa `u32::MAX` para "ninguna"; `state` es el `PlayerState` como byte
  - `app::worker::spawn(session: ReadSession) -> Handles` — el hilo consume órdenes, hace `tick()` y publica palabra y estado en los atómicos; la GUI solo lee

- [ ] **Step 1: Write the failing test** (con un doble de sesión, sin audio ni motor)

```rust
#[test]
fn el_worker_publica_la_palabra_actual_y_obedece_pausa() {
    let h = spawn_con_sesion_falsa(&[(0, 0, 25), (1, 25, 70)]);
    h.commands.send(Command::Play).unwrap();
    esperar_a_que(|| h.word.load(Ordering::SeqCst) != u32::MAX);
    assert_eq!(h.word.load(Ordering::SeqCst), 0);
    h.commands.send(Command::Pause).unwrap();
    esperar_a_que(|| h.state.load(Ordering::SeqCst) == PlayerState::Paused as u8);
}
```

*El doble de sesión es un `ReadSession` construido con `NullSink::instant()` y un timeline
sintético: el protocolo del worker se prueba sin motor ni dispositivo.*

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib app::worker`
Expected: FAIL — `app::worker` no existe

- [ ] **Step 3: Implement**

Un hilo `std::thread` que posee la `ReadSession`; el bucle `recv_timeout(~30 ms)`: aplica órdenes,
`tick()`, y publica `position_sample` y `current_word` en dos atómicos. La GUI **nunca** toca la
sesión. La barra inferior de `app::window` manda órdenes y lee los atómicos en su timeout.

- [ ] **Step 4: Verify**

Run: `cargo test --lib app::worker && cargo run -- open <libro>`
Expected: PASS; con la barra inferior, dar play suena y el resaltado avanza **sin que la ventana se
congele** (probar a redimensionar mientras suena)

- [ ] **Step 5: Commit**

```bash
git add src/app/worker.rs src/app/mod.rs
git commit -m "feat(app): motor en hilo trabajador; la GUI solo lee atomics"
```

---

### Task 14: `readaloud open` y la instancia única

**Files:**
- Modify: `src/main.rs`, `src/app/mod.rs`
- Test: `src/cli/mod.rs` (ampliar los inline existentes)

**Interfaces:**
- Consumes: `cli::OpenArgs` y `Command::Open` (creados en el Task 8).
- Produces:
  - `app::run` con `ApplicationFlags::HANDLES_COMMAND_LINE`: una segunda invocación **no** arranca otro proceso, entrega la ruta a la instancia viva
  - Acción de aplicación `reload-themes` (hoy: relee la paleta y la aplica)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn reload_themes_ya_no_es_un_subcomando_de_terminal() {
    // paso a ser una accion de GApplication, no una orden de shell
    assert!(Cli::try_parse_from(["readaloud", "reload-themes"]).is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib cli::reload_themes`
Expected: FAIL — hoy sí existe como subcomando

- [ ] **Step 3: Implement**

`app::run` pasa a `HANDLES_COMMAND_LINE` y reenvía la ruta a la instancia existente. Registrar la
acción `reload-themes` en la `Application` y quitar su rama de la CLI.

- [ ] **Step 4: Run tests and verify the single instance**

Run: `cargo test --lib cli::` y luego, con la ventana abierta, `cargo run -- open <otro-libro>` en
otra terminal
Expected: PASS; **no** se abre una segunda ventana, el libro nuevo se carga en la existente

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/app/mod.rs src/cli/mod.rs
git commit -m "feat(app): instancia unica con HANDLES_COMMAND_LINE y accion reload-themes"
```

---

## Self-Review

**Cobertura de la spec.** §1/§2 (ventana, reproductor, tipografía, índice, anotaciones…): este plan
cubre ventana, render, resaltado, auto-scroll, selección, índice, reproductor y `open`; **librería,
SQLite, tema, tipografía configurable y anotaciones son V5–V6** y están declaradas fuera en Global
Constraints. §3 V1–V6 (decisiones visuales) → Tasks 8 (el `GtkStack`), 9 (el resaltado de dos niveles) y 11
(auto-scroll y selección).
§4.1 (session) → Task 12. §4.2 (hilo) → Task 13. §4.3/§5 (`exact`) → Task 9.
§9 (render, CSS, auto-scroll) → Tasks 5–9 y 11. §10 (errores) → Tasks 2, 7 y el fallback del 8.
§14 (deudas) → Tasks 1–4. §16 (hitos) → el orden de los tasks es V0–V4.

**Consistencia de tipos.** `doc::Block`/`Run` (T1) los consumen 6, 7, 8, 11. `Word.block` sustituye
a `paragraph` en T1 y se arrastra a `cli/script` y a los tests. `render::css::Properties` (T5) →
`render::style::resolve` (T6) → `render::style::PangoAttrs` (T6) → `layout_chapter` (T7) →
`ReaderView` (T8) y `Highlight` (T9). `Palette` se define en T6 y lo usan 8 y 9; **V5 lo llenará
desde el JSON de Omarchy** sin tocar el render. `session::ReadSession` (T12) → `app::worker` (T13).

**Proporción.** Sin cuerpos de implementación salvo las dos secuencias que son decisiones y no
código adivinable: el recorrido del layout de Pango (T7) y el cierre de runs por palabra (T1). El
resto son firmas y aserciones.

**Review Focus.** 1→T5 · 2→T7 · 3→T7 · 4→T7 · 5→T1. Los cinco tienen su test en el task dueño.
