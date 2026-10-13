# readaloud — Diseño

**Fecha:** 2026-10-13
**Estado:** diseño aprobado por §s; spec pendiente de revisión
**Nombre de trabajo:** `readaloud` (ver Preguntas abiertas, Q1)
**Licencia prevista:** GPL-3.0-or-later (ver Decisión D5)

---

## 1. Objetivo y tesis

Un lector de EPUB de escritorio para Linux que **lee el libro en voz alta con voces
neuronales locales y resalta la palabra exacta que está sonando**. El resaltado es la
razón de ser del proyecto: es lo que permite leer más rápido (leer y escuchar a la vez)
y lo que hoy no existe bien en Linux.

Tres convicciones, que son la identidad del proyecto y no features sueltas:

1. **Local.** Sin nube, sin cuenta, sin cuota, sin paywall. Voces en disco, síntesis en
   la máquina.
2. **Nativo.** Sin WebView. Render propio, toolkit de plataforma.
3. **Propio.** Nada de depender de que otra app te deje usar tu propio hardware.

El criterio de éxito no es "tiene más features que Readest", es: **la sincronización
palabra↔audio es exacta y demostrable**, y el proyecto está publicado y se instala.

## 2. Alcance

Bajo este diseño el render es el subsistema más caro, así que el alcance de formato se
fija por escrito y la app lo respeta.

### Dentro (v1)

- EPUB 2 y 3, **reflowable**, spine lineal.
- XHTML y un subconjunto de CSS: familia, tamaño, peso, cursiva, color, márgenes,
  interlineado, listas, `blockquote`.
- Imágenes.
- Enlaces internos y notas al pie (popup).
- Índice por `nav` (EPUB 3) o NCX (EPUB 2).

### Fuera (v1)

Se documenta explícitamente en el README, para que la ausencia sea una decisión y no
un bug.

- fixed-layout, SVG, MathML, `grid`/`flex`, columnas múltiples, `media`, DRM.
- Otros formatos: PDF, MOBI, CBZ, audiobooks.
- Lectura paginada (ver Decisión D4).
- Sync entre dispositivos, cuentas, OPDS, escritura de metadatos.

### Regla de oro

Nada fuera de alcance puede crashear ni degradar la app. Se renderiza lo que se puede y
se registra una entrada de **cobertura** (contador + log) por cada construcción
ignorada. Un lector que se cae con un EPUB raro es un bug, no una limitación.

## 3. Arquitectura

**Un crate, un binario.** Sin workspace: la separación es por módulos, no por artefactos.
El coste de tener cinco crates no lo paga nadie en un proyecto de un autor.

```
readaloud/
  Cargo.toml
  build.rs              # compilación de las dependencias nativas (ver D5 / R1)
  src/
    main.rs             # clap: subcomandos + arranque de la GUI
    epub/               # contenedor ZIP, OPF, spine, nav/NCX, CSS crudo, imágenes
    doc/                # documento: capítulos → párrafos → palabras (word_id)
    tts/                # voces, síntesis, parche de alineación ONNX
    timeline/           # word_id ↔ rangos de sample; seek
    player/             # salida de audio, cola, FSM de reproducción
    render/             # layout Pango, pintado, resaltado, mapeo CSS→Pango
    sources/            # local + SSH
    store/              # SQLite: librería, progreso, anotaciones
    theme/              # paleta de Omarchy → hoja CSS de GTK
    app/                # GApplication, estado, ventana, miniplayer
  omarchy/
    readaloud.json.tpl  # plantilla de tema para Omarchy
    readaloud-theme     # hook, registrado con `omarchy hook install theme-set`
```

### Los tres modos del mismo binario

Es la jugada que hace que el motor sea verificable antes de que exista UI, y por tanto
la que evita que el proyecto se hunda en el render.

```
readaloud open   <libro>    # GUI
readaloud script <libro>    # headless: texto, y con --timeline el mapeo word_id→samples
readaloud say    <texto>    # sintetiza a la salida de audio (humo del motor)
readaloud voices            # listar/descargar/parchear voces
readaloud reload-themes     # acción que invoca el hook de Omarchy (ver §6)
```

`script --timeline` emite **exactamente el mismo mapeo que consume la GUI**. Los tests
del motor no necesitan ventana, y se puede dogfoodear leyendo desde el terminal mientras
la UI no existe.

### Flujo de datos

```
EPUB ──epub/──> doc/(word_id bien definidos) ──tts/──> PCM + alineación por fonema
                                    │                              │
                                    │                        timeline/  word_id → samples
                                    │                              │
                              render/ (word_id → rect)       player/ (sample actual)
                                    └────────── app/ ─────────────┘
                                          resaltado + audio
```

## 4. Modelo de anclas: `word_id`

Una sola moneda interna. Todo se define contra ella.

- **Definición.** `word_id: u32`, índice global de palabra dentro del libro. Se asigna
  en el parseo, en orden de lectura, de forma estable para un mismo archivo.
- **`doc/` produce** `Vec<Word>` plano por capítulo: `word_id`, párrafo, texto, y rango
  de bytes en el origen.
- **`render/` produce**, en cada pasada de layout, el mapeo `word_id → rect` (extents).
  No se persiste: se recalcula. Es barato porque el layout ya se hizo.
- **`timeline/` produce** `word_id → (start_sample, end_sample)`.
- **`store/` persiste** posiciones como `(book_id, word_id)` y anotaciones como rangos de
  `word_id`. Para interoperar con el ecosistema se exporta además el CFI.

**Por qué esta es la frontera correcta:** el motor no sabe nada de píxeles y la UI no
sabe nada de fonemas. Cada lado se puede testear por separado, y el mapeo por el que
existe el proyecto queda aislado en un módulo con invariantes comprobables.

**Invariantes (asserts en tests):**

1. Monotonía: `end_sample(n) <= start_sample(n+1)`.
2. No solape.
3. Cada `word_id` aparece exactamente una vez: sea con rango propio, sea dentro del
   rango de una unidad degradada a frase (§5). No puede quedar ninguna palabra sin
   ubicar ni repetida.
4. El último `end_sample` no excede el total de samples del capítulo.
5. Cualquier palabra sin rango 1:1 está marcada como tal (ver §5, expansión).

## 5. Motor TTS y alineación

### Integración

**FFI a `libpiper`** (la librería del proyecto piper1-gpl). Se descarta spawnear el CLI
de piper: pierdes el control de la fonemización y con él la alineación exacta, que es la
tesis del proyecto. Ver R1 para la alternativa y su momento de decisión.

### La alineación viene dada por la API

El header público de `libpiper` documenta el protocolo completo. Textual:

```
Phonemes look like [p1, p1, 0, p2, p2, 0, ...]
1. Read N repeated codepoints until a 0
2. The next N phoneme ids correspond to that phoneme
3. The next N alignments (sample counts) correspond to that phoneme
alignments: Audio sample count for each phoneme id
```

`piper_audio_chunk` expone `samples`, `num_samples`, `sample_rate`, `phonemes`,
`num_phonemes`, `phoneme_ids`, `num_phoneme_ids`, `alignments`, `num_alignments`.

### De fonema a palabra

El stream de fonemas de espeak separa palabras con un espacio literal. Verificado:

```
['e', 'l', ' ', 'p', 'ˈ', 'e', 'r', 'o', ' ', 'k', 'ˈ', 'o', ...]
```

Así que la palabra es la suma de los samples de sus fonemas: rangos **exactos**, no
estimados. Esto es la diferencia concreta con el prototipo que motivó el proyecto (que
repartía el audio proporcionalmente al número de caracteres).

### El caso que rompe el 1:1 — y la regla

Números y abreviaturas se **expanden** al fonemizar (`"42"` → `"cuarenta y dos"`). Ahí
una palabra del texto no corresponde a un grupo de fonemas, y cualquier alineación por
índice a partir de ese punto arrastra el desfase a todo el párrafo.

**Regla:** se alinea por orden y, si una palabra no casa 1:1, se marca y **se degrada a
resaltado por frase** para esa unidad, en lugar de propagar el desfase. Preferimos un
resaltado a nivel de frase a un resaltado por palabra que miente.

### Parche de alineación de una voz

Las voces no traen alineación (`phoneme_id_samples` vacío) hasta que se exponen el tensor
`w_ceil` del grafo ONNX como salida. `piper` trae la herramienta y solo necesita el
paquete `onnx` (no `onnxruntime`). Se hace una vez por voz, offline:

```
readaloud voices prepare <voz>
```

### Velocidad

Se usa el `length_scale` de piper y se re-sintetiza, con caché por `(voz, rate, frase)`.
No se usa estiramiento temporal (WSOLA/vocoder): mete artefactos y complica el mapeo de
timings. Con `length_scale` los timings siguen siendo exactos y el problema desaparece.

*Verificar en el hito 1:* el campo exacto de `length_scale` dentro de
`piper_synthesize_options` en la API C (en la API Python existe; no se verificó el
struct C).

### Caché

En disco: PCM y timeline por `(voz, rate, frase)`. Consecuencias: seek instantáneo,
re-escucha instantánea, y `script --timeline` con salida determinista para los tests.

## 6. Render y tema

### Toolkit: GTK4

Razón principal: **Pango**. Pango resuelve shaping, bidi, corte de línea, justificación y
—lo decisivo— el mapeo posición↔índice de texto (`index_to_pos`, `xy_to_index`), que es
exactamente el `word_id → rect` de §4. No lo escribimos, lo consultamos. Un motor de
layout propio es el trabajo de meses que este proyecto no necesita.

GTK4 es el toolkit de la plataforma, no un atajo: igual que en §5 se acepta un FFI en C,
"Nativo" aquí significa **sin WebView**, no "escrito por mí".

### Vista de lectura: scroll continuo

v1 pinta el capítulo como un documento de párrafos con Pango y scrollea. Paginar exige
medir y balancear alturas, no tiene relación con la tesis, y hoy el lector de referencia
usa scroll por defecto. Ver D4.

### Resaltado

Un rango de `word_id` → extents de Pango → rect redondeado por detrás de los glifos. Sale
del mismo hit-testing que la selección de texto. Las anotaciones se almacenan como rangos
de `word_id`, la misma moneda que todo lo demás.

### CSS de EPUB → Pango

Una tabla de mapeo pequeña y explícita para el subconjunto de §2. Lo que no está en la
tabla se ignora y cuenta como cobertura perdida. Sin motor de CSS.

### Tema de Omarchy

El mecanismo del escritorio, copiado del patrón que usa una app ya instalada:

1. La app publica `omarchy/readaloud.json.tpl` con **tokens semánticos de lector**
   (`page`, `text`, `highlight`, `highlight_active`, `chrome`, `rule`, …), y el
   instalador (PKGBUILD, o el primer arranque si no es paquete) lo copia a
   `~/.config/omarchy/themed/` — que es donde el escritorio los busca. Sin ese paso la
   plantilla no existe para Omarchy.
2. `omarchy-theme-set-templates` lo renderiza con la paleta del tema activo (soporta
   `{{ key }}`, `{{ key_strip }}`, `{{ key_rgb }}` y `{{ mix A B 15% }}`).
3. El hook `readaloud-theme`, registrado con `omarchy hook install theme-set`, copia el
   resultado con `mktemp` + `mv` a `~/.config/readaloud/themes/omarchy.json` y avisa a la
   instancia viva con `readaloud reload-themes`.
4. La app genera su hoja CSS de GTK desde ese JSON.

Reglas heredadas del patrón, que son requisitos y no detalles:

- Si no hay `omarchy.json`, se usa una paleta por defecto. La app nunca falla por un tema.
- `reload-themes` solo contacta una instancia viva; si la app está cerrada, no la arranca.
  `GApplication` da single-instance y esta acción por D-Bus sin escribir IPC a mano.
- El hook sale 0 si el archivo de origen no existe o es un enlace simbólico.

## 7. Player

**Salida: `cpal` con cola propia**, no una librería de alto nivel. La única
responsabilidad del player que importa es **saber en qué sample va**, porque de ahí sale
el `word_id` que se ilumina. Delegar eso en una librería de alto nivel delega justo el
dato que da sentido al proyecto.

- **Prefetch**: se sintetiza la frase N+1 mientras suena la N, con dos frases de ventana.
  La reproducción arranca cuando la primera frase está lista, no al final del capítulo.
- **Seek por palabra**: `word_id` → frase → PCM cacheado + offset. Si la frase no está en
  caché, se sintetiza primero y la UI muestra estado pendiente. Nunca un silencio mudo.
- **Controles**: play/pausa, frase siguiente/anterior, velocidad, avance automático al
  terminar el capítulo.
- **FSM explícita**: `idle → buffering → playing → paused → ended`. Es una unidad con un
  propósito y se testea sola; fuera, nada de condiciones repartidas por la UI.

**Testabilidad**: la FSM y el mapeo `word_id → sample` se testean headless, con la salida
de audio detrás de un trait y un sink nulo en tests.

## 8. Fuentes, librería y almacenamiento

### Un EPUB es un ZIP

Necesita acceso aleatorio (`Seek`). Hacer `Seek` sobre SFTP es posible pero charlatán.

**v1: descarga a caché local.** `~/.cache/readaloud/<host>/<ruta>`, validada por tamaño
y mtime; luego se abre desde ahí. Un `read` físico y, a partir de ahí, zip, imágenes y
seek son locales. Acceso aleatorio remoto es v2 y solo si aparece el caso.

### SSH

`russh` + `russh-sftp`, con claves y **agent** de `~/.ssh` reutilizados. Sin prompts ni
gestión de contraseñas. Config: host, usuario, ruta. En v1, un diálogo "añadir desde SSH"
con selector de directorio remoto; no un explorador SFTP completo.

### Librería

Un directorio escaneado (configurable) más lo añadido por SSH. Título, autor y portada se
leen del OPF; miniaturas en caché.

### Almacenamiento: SQLite

`rusqlite` con la feature `bundled` (sin dependencia del sistema). Es el hogar de las dos
cosas que sí necesitan consulta y atomicidad: el progreso `(book_id, word_id)` y las
anotaciones como rangos de `word_id`.

## 9. Empaquetado y publicación

- **AppImage** para repartir sin root en cualquier distro, y **PKGBUILD para AUR** como
  camino correcto en Omarchy. `.deb`/`.rpm` después, si acaso.
- **`libpiper` y `espeak-ng-data` van dentro.** Sin el `data` de espeak las voces no
  fonemizan y el fallo es opaco. Sin `libpiper` incluida, el AppImage se rompe en una
  máquina sin el paquete de AUR. Ver R1 para el detalle de onnxruntime.
- **Voces propias** en `~/.local/share/readaloud/voices/`, descargables desde la app.
  Primer arranque = preparar voces (descarga + parche ONNX) con progreso visible.
- **Integración con Omarchy**: instalar `omarchy/readaloud.json.tpl` y registrar el hook
  `readaloud-theme` en `theme-set`.

## 10. Estrategia de tests

Sin frameworks pesados ni suites por función. Lo que se verifica:

| Qué | Cómo | Dónde |
|---|---|---|
| Motor completo, sin UI | `script --timeline` sobre EPUBs de fixture | tests de integración |
| Invariantes del timeline | los 5 asserts de §4 | unitarios |
| Fallback de expansión de números | caso con `"42"` en el texto | unitario, uno solo |
| Determinismo | misma entrada → mismo timeline | unitario |
| FSM del player | sink nulo, transiciones | unitario |
| Mapeo CSS→Pango | tabla: CSS entra, atributos salen | unitario |
| Cobertura de formato | EPUB con `grid`/SVG → no crashea y cuenta | integración |

Regla: cada pieza no trivial (un parser, una rama, un mapeo de dinero/seguridad) deja
**un** check ejecutable. Lo trivial no lleva test.

## 11. No objetivos

Explícito, para poder decir no sin discutirlo dos veces:

- Sustituir a Calibre en gestión de biblioteca.
- Ser multiplataforma. Linux de escritorio primero; si algún día compila en macOS, bien.
- Sincronización, cuentas, cloud, telemetría.
- Formatos que no sean EPUB.
- Paginado, anotaciones a mano alzada (trazo libre; los resaltados y notas de texto sí
  entran, en §6 y §8), RSS, OPDS.
- TTS "con voces clonadas" o multi-personaje.

## 12. Riesgos y preguntas abiertas

| # | Riesgo | Impacto | Mitigación |
|---|---|---|---|
| R1 | `libpiper` es C++ y enlaza **onnxruntime**, una librería C++ grande. El FFI es poco código, pero el empaquetado se complica | Alto en §9, nulo en el código | Punto de decisión en el hito 2, con medición real (ver abajo) |
| R2 | Expansión de números/abreviaturas rompe la alineación 1:1 | Medio | Regla de §5: degradar a frase y marcarlo |
| R3 | `length_scale` en la API C de `piper_synthesize_options` sin verificar | Bajo | Verificar en hito 1 |
| R4 | El render de EPUB se come el proyecto | Alto | Alcance de §2 por escrito + motor primero y testeable |
| R5 | EPUBs grandes por SSH | Bajo | Descarga a caché; acceso aleatorio remoto diferido |

### R1 en detalle — el momento de decisión

Dos caminos, y se elige con un número, no con una opinión, al terminar el hito 2:

- **(a) FFI a `libpiper`** (lo aprobado). Menos código Rust. Requiere cmake y compilador
  de C++ en build, y arrastra onnxruntime dentro del AppImage.
- **(b) `ort` + runner propio.** Más código Rust (reimplementar el paso por el modelo),
  pero sin C++ propio en el build; `ort` es ONNX Runtime (MIT). La alineación sigue siendo
  alcanzable porque el parche ONNX ya expone `w_ceil` como salida, y lo leeríamos nosotros.

El criterio de decisión: **¿el AppImage resultante arranca en una máquina limpia sin
`libpiper` ni `onnxruntime` del sistema?** Si (a) lo consigue con un build razonable, se
queda (a). Licencia GPL-3 en ambos caminos, porque espeak-ng lo es.

### Preguntas abiertas

- **Q1 — Nombre.** `readaloud` es nombre de trabajo. Es el nombre del crate, del repositorio
  y del binario, así que conviene fijarlo antes del plan. Todas las rutas de esta spec
  (`~/.config/readaloud/`, `~/.cache/readaloud/`, `/usr/share/readaloud/`) siguen el nombre
  de trabajo y se renombran con él.
- **Q2 — Repositorio.** Se asume `paulo1403/<nombre>` en GitHub. ¿Público desde el primer
  commit? (afecta a si el README promete cosas que aún no hay)

## 13. Decisiones

| # | Decisión | Por qué |
|---|---|---|
| D1 | Rust, todo nativo, sin WebView | Elección del autor; el mérito es el objetivo, no el atajo |
| D2 | Motor primero, headless y testeable | Única forma de que el render no se coma el proyecto |
| D3 | Un crate, un binario, tres modos | Separación por módulos, no por artefactos |
| D4 | Scroll continuo en v1; paginado diferido | Paginar no toca la tesis y hoy no es lo que usa la referencia |
| D5 | GPL-3.0-or-later | Impuesta por `libpiper` y `espeak-ng`; es lo normal en este espacio |
| D6 | FFI a `libpiper` en vez de spawnear el CLI | El CLI te quita la fonemización, y con ella la alineación exacta |
| D7 | GTK4 + Pango | Pango da el layout y el hit-testing; escribirlo es el coste de meses |
| D8 | `cpal` con cola propia | El player existe para reportar el sample actual |
| D9 | `length_scale` en vez de time-stretch | Timings exactos y sin artefactos |
| D10 | Descarga a caché en vez de `Seek` remoto | Un ZIP necesita acceso aleatorio; SFTP se vuelve charlatán |
| D11 | SQLite con `rusqlite` (bundled) | Progreso y anotaciones necesitan consulta y atomicidad |
| D12 | Vendorizar `libpiper` y `espeak-ng-data` **si R1 se resuelve por (a)** | Si no, el AppImage se rompe fuera de Arch. Si gana (b), no hay `libpiper` que vendorizar |
| D13 | Tema vía hook de Omarchy, la app lee su propio JSON | Desacopla la app del escritorio; el tema se sigue en caliente |

## 14. Verificado durante el diseño

Hechos comprobados en la máquina del autor, no asumidos:

- `omarchy-theme-set-templates` es un script sed/awk que renderiza `.tpl` desde
  `~/.config/omarchy/themed/` y `$OMARCHY_PATH/default/themed/`; implementa `mix` en awk
  (línea 25) y soporta `_strip`/`_rgb`.
- El estado del tema vive en `~/.local/state/omarchy/current/theme/`, con paletas
  renderizadas por app (`claude.json`, `pi.json`, `t3code.json`, `colors.toml`).
- El patrón de integración de una app está en `/usr/share/zapfast/omarchy/`: un `.tpl` y
  un hook `zapfast-theme` que copia atómicamente y notifica con `zapfast reload-themes`.
- La paleta del tema activo del autor: `mode=dark`, `background=#222222`,
  `foreground=#c2c2b0`, `accent=#78824b`, más `red`…`bright_*`.
- `piper_audio_chunk` expone `alignments` (samples por id de fonema) y `phonemes`
  repetidos separados por `0`; el header documenta el recorrido.
- El stream de fonemas separa palabras con un `' '` literal (comprobado con
  `es_ES-davefx-medium`).
- Las tres voces instaladas **no** traen alineación hasta parchear el ONNX.
- `patch_voice_with_alignment.py` solo depende del paquete `onnx` y marca `w_ceil` como
  salida.
- Crates comprobados en crates.io: `gtk4`, `pango`, `glib`, `cpal`, `rusqlite`,
  `russh`, `russh-sftp`, `zip`, `roxmltree`, `quick-xml`, `notify`, `clap`, `anyhow`,
  `thiserror`, `serde`, `cmake`, `zbus`.

## 15. Hitos

Cada hito termina con algo ejecutable y verificable. Los cuatro primeros son la tesis y se
verifican **sin ventana**; la UI entra cuando el motor ya es sólido.

**El plan de implementación cubre M0–M4.** Ahí está la tesis completa: EPUB → palabras →
audio → timeline exacto → reproducción con seek, todo verificable headless. M5 (UI), M6
(fuentes) y M7 (empaquetado) son una fase posterior con su propio ciclo de
spec/plan, porque dependen de decisiones que M0–M4 todavía no acotan (toolkit en uso
real, forma de la librería, resultado de R1).

| H | Qué | Verificación |
|---|---|---|
| M0 | Esqueleto: crate, binario con subcomandos, `--help` | `cargo build` y `readaloud --help` |
| M1 | EPUB → `doc/` con `word_id` estables | `script --text` sobre un EPUB de fixture; asserts de conteo y orden |
| M2 | Piper FFI, `voices prepare`, síntesis de una frase. **Decisión R1** | `say` produce PCM; medición de build y arranque en máquina limpia |
| M3 | Alineación: `word_id → samples` **exactos** + fallback de frase | `script --timeline`; los 5 invariantes de §4; caso `"42"` |
| M4 | Player con FSM, prefetch y caché | `play` lee el libro desde el terminal; tests headless con sink nulo |
| M5 | UI GTK4: layout Pango, resaltado, miniplayer, tema Omarchy | Leer un epub con el dedo en la palabra |
| M6 | Fuentes SSH, librería, SQLite | Añadir un libro por SSH y conservar el progreso |
| M7 | AppImage + PKGBUILD + hook de tema | Instalar en una máquina limpia y arrancar |
