# readaloud

Lector de EPUB **local** que lee en voz alta con voces neuronales y mapea cada
palabra a sus samples de audio, para poder resaltarla exactamente cuando suena.
Sin nube, sin cuenta, sin WebView: voces y síntesis en la máquina.

## Estado

M0–M4 es el **motor headless** (biblioteca + CLI). Todavía **no hay GUI**: el
render propio y la ventana son milestones posteriores. Lo que existe hoy se
controla desde el terminal con los cuatro subcomandos de abajo.

## Compilar

Requisitos:

- **Rust** fijado en `mise.toml` (`mise install`); o el `rust-toolchain` de tu
  elección compatible con edition 2024.
- **`cmake`**, un compilador de **C++** y **`git`**.
- **Red** en el primer build: `build.rs` clona `piper1-gpl` **v1.8.0** y el
  CMake de `libpiper` descarga un **onnxruntime 1.22.0** prebuilt. No hace falta
  instalar nada con `sudo`.

```bash
cargo build --release
```

## Ejecutar

Requisitos en tiempo de ejecución:

- **espeak-ng** y sus datos (`/usr/share/espeak-ng-data`, o la ruta que indique
  `READALOUD_ESPEAK_DATA`).
- Una **voz piper** (`.onnx` con su `.onnx.json` hermano) en
  `~/.local/share/readaloud/voices/` o, si ese directorio no existe, en
  `~/.local/share/piper-voices/`.

```bash
# Vuelca el libro como texto: word_id, capítulo, párrafo y palabra.
readaloud script libro.epub --text

# Mapea word_id → samples (JSON con exact por palabra). Sintetiza de verdad.
readaloud script libro.epub --timeline

# Sintetiza un texto a un WAV (o a stdout si se omite -o).
readaloud say "Hola mundo." -o hola.wav --voice es_ES-davefx-medium

# Lista las voces instaladas y prepara su salida de alineación.
readaloud voices list
readaloud voices prepare es_ES-davefx-medium

# Lee un EPUB en voz alta por el dispositivo de audio por defecto.
readaloud play libro.epub --voice es_ES-davefx-medium
```

`play` y `script --timeline` cargan la voz y, si le falta la salida de
alineación (`w_ceil`), la parchean en una copia temporal; `voices prepare` la
parchea en su sitio.

## Tests

Los tests que sintetizan de verdad llevan puerta por entorno: sin voz se
**saltan**. Define `READALOUD_TEST_VOICE` para ejercitar el motor; define
`READALOUD_REQUIRE_VOICE` para que la falta de voz sea un **fallo** en vez de un
skip (útil en CI, donde un verde sin motor no probaría nada).

```bash
# Sin voz: los tests de síntesis se saltan, el resto corre.
cargo test

# Con voz: el motor se ejercita de verdad.
READALOUD_TEST_VOICE=~/.local/share/piper-voices/es_ES-davefx-medium.onnx cargo test

# Exigiendo voz: si no está definida, el test sintetizador falla.
READALOUD_TEST_VOICE=~/.local/share/piper-voices/es_ES-davefx-medium.onnx \
  READALOUD_REQUIRE_VOICE=1 cargo test
```

## Alcance

**Dentro** (v1): EPUB 2 y 3 reflowable, spine lineal; XHTML y un subconjunto de
CSS; imágenes; enlaces internos y notas al pie; índice por `nav` o NCX.

**Fuera** (v1), por decisión escrita y no por bug:

- fixed-layout, **SVG**, **MathML**, `grid`/`flex`, columnas múltiples,
  `media`, **DRM**.
- Otros formatos: **PDF**, **MOBI**, **CBZ**, audiolibros.
- Lectura paginada; GUI (llega tras M4); sincronización entre dispositivos,
  cuentas, OPDS y escritura de metadatos.

Nada fuera de alcance puede crashear ni degradar la app: se procesa lo que se
puede y se registra la cobertura de lo ignorado.

## Licencia

**GPL-3.0-or-later.** La imponen `libpiper` (piper1-gpl) y `espeak-ng`, ambos
GPL, que este binario enlaza directamente; es además la licencia habitual en
este espacio.
