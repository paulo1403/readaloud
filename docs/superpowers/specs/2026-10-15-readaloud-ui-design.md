# readaloud — Diseño de M5: la aplicación

**Fecha:** 2026-10-15
**Estado:** diseño aprobado; spec pendiente de revisión
**Diseño padre:** `docs/superpowers/specs/2026-10-13-readaloud-design.md` — es la autoridad para lo ya decidido (GTK4, Pango, scroll continuo, alcance de formatos, tema por hook). Esta spec no lo repite: lo aplica.
**Motor previo:** `docs/superpowers/plans/2026-10-13-readaloud-motor-m0-m4.md` (M0–M4, mergeado en `paulo1403/readaloud`).

---

## 1. Objetivo

Convertir el motor en **una aplicación de escritorio para leer**, donde además la voz va enganchada al texto: leer en voz alta con la palabra exacta iluminada, y leer en silencio cuando apetezca.

Criterio de éxito: abrir la app, ver mis libros, elegir uno, darle a play, y **la palabra que suena está iluminada y la vista la sigue** — sin tocar un terminal.

## 2. Alcance

### Dentro

- **Ventana de lectura**: render propio con Pango, scroll continuo, resaltado de dos niveles, auto-scroll, selección de texto, índice por capítulos, tipografía configurable.
- **Reproductor**: play/pausa, frase anterior/siguiente, velocidad, progreso por capítulo, indicador de que sigue la voz.
- **Librería completa**: escanear carpetas, metadatos y portada del OPF, «Añadir carpeta», reescanear, «Seguir leyendo» con el libro en curso, rejilla con progreso por portada.
- **Persistencia**: SQLite con libros, progreso y anotaciones; preferencias en JSON.
- **Tema de Omarchy** por el hook, con paleta por defecto si no existe.
- **Anotaciones**: resaltados sobre rangos de `word_id` (crear, ver, borrar). Se crean
  seleccionando texto y aplicando la acción «resaltar» (atajo de teclado y menú contextual); se
  guardan como `(start_word_id, end_word_id)`. Sin notas de texto en M5 (ver §12).
- **`readaloud open <libro>`**: lanza la ventana con ese libro abierto. Es el subcomando que el
  diseño padre (§3) dejó para M5, junto con `reload-themes`.

### Fuera

- **Búsqueda** — no está en el diseño padre; es un feature propio con su índice.
- **SSH/SFTP** — es M6 (de dónde vienen los libros, no la librería).
- Paginado, escritura de metadatos, trazo libre, sync, OPDS.
- Notas de texto en anotaciones (M5 solo marca rangos).

## 3. Decisiones visuales aprobadas

Aprobadas sobre mockups en el navegador, en los colores reales del tema del autor.

| # | Decisión | Por qué |
|---|---|---|
| V1 | **Dos vistas completas** (`GtkStack`): la librería ocupa la ventana; al abrir un libro cambia a la lectura. Sin sidebar. | En un tiling WM como Hyprland la ventana única se lleva mejor que dos, y el sidebar come 70–200 px de ancho de texto. Máximo ancho para leer. |
| V2 | **Librería = «Seguir leyendo» + rejilla con progreso.** Arriba el libro en curso con su botón; debajo la rejilla, cada portada con una línea de progreso en el canto. | «Seguir donde ibas» es la acción real del 90 % de las veces; la rejilla queda para elegir otro. |
| V3 | **Resaltado de dos niveles**: la frase con tinte suave, la palabra actual con tinte fuerte. | El proyecto existe para leer *más rápido*: con la frase tintada lees con el rabillo del ojo y la voz confirma el sitio. Solo-palabra obliga a perseguirla y se pierde el hilo. |
| V4 | Columna de **~60 caracteres**, centrada, serif para el cuerpo y sans para la interfaz. Primera línea con sangría, sin espacio entre párrafos. | Legibilidad de libro; el ancho completo en un monitor grande con serif es incómodo. La sangría es lo que ya produce el mapeo de CSS del motor. |
| V5 | **Auto-scroll activo por defecto**, con botón para apagarlo. | La vista se desplaza detrás de la voz; sin eso acabas scrolleando a mano con el libro sonando. Se puede apagar porque a algunas personas les marea. |
| V6 | **Anterior/Siguiente mueven por frase**, no por párrafo. | Es la unidad que el motor ya usa para sintetizar y para el timeline. Mover por párrafo obligaría a traducir entre dos granularidades, y ahí nacen los desfases. |

## 4. Arquitectura

Módulos nuevos, alineados con la lista del diseño padre (`render/`, `theme/`, `app/`, `store/`) más uno que no estaba: `session/`.

```
src/
  session/     leer un libro en voz alta: agrupado de frases, caché, motor, player
  store/       SQLite: libros, progreso, anotaciones  +  config.json
  theme/       paleta de Omarchy → hoja CSS de GTK
  render/      widget: layout Pango, pintado, word_id → rect, resaltado, auto-scroll
  app/         GApplication, ventana, Stack, librería, lector, barras, diálogos
```

### 4.1 `session/` es un refactor, no código nuevo

Hoy la orquestación de lectura en voz alta vive en `src/cli/play.rs`: agrupar el párrafo en
frases, consultar la caché, sintetizar, construir el timeline, encolar en el `Player` con
prefetch y estancarse si el sink no avanza. Eso se extrae a `session/` y lo consumen **los dos**:
`readaloud play` (CLI) y la GUI.

Si no se hace, hay dos implementaciones de "cómo se lee un libro" y divergen en el primer
cambio de seek — que es justo donde están los bugs sutiles.

```rust
// session/session.rs
pub struct ReadSession { /* motor, caché, player, grupo actual */ }
impl ReadSession {
    pub fn open(book: &Book, voice: &Path, length_scale: f32, sink: Box<dyn Sink>, cache: Cache) -> Result<Self>;
    pub fn next_sentence(&mut self) -> Result<Option<Sentence>>;  // sintetiza (o tira de caché) y encola
    pub fn tick(&mut self) -> PlayerState;
    pub fn position_sample(&self) -> usize;
    pub fn current_word(&self) -> Option<u32>;
    pub fn set_rate(&mut self, length_scale: f32);
    pub fn seek_to_word(&mut self, word_id: u32) -> Result<()>;
}
```

`cli/play.rs` queda como una cáscara que construye una `ReadSession` con `CpalSink` y la itera.
Sus tests actuales deben seguir pasando sin cambios de comportamiento.

### 4.2 El motor vive en un hilo trabajador

Sintetizar una frase tarda segundos y el bucle de GTK no puede bloquearse. Y el motor **no es
reentrante**: `piper_free` llama a `espeak_Terminate()` sobre estado global de espeak, por lo que
hay un `Engine` por proceso y un segundo `load` se cuelga (documentado en `src/tts/engine.rs`).

Consecuencia arquitectónica: **el `ReadSession` vive entero en un hilo trabajador dedicado**,
que es además el único dueño del `Engine`, de la `Cache` y de la conexión SQLite. La GUI nunca
toca el motor: le manda órdenes por canal y lee un contador atómico de posición.

```
GTK (hilo principal)                    trabajador (sesión)
────────────────────                    ───────────────────
play / pause / next      ──orden──▶     ReadSession
seek_to_word(word_id)    ──orden──▶     Engine + Cache + SQLite
                                         │
current_word()  ◀──atómico───────────────┘  (position_sample)
```

El render lee `current_word()` en un timeout de GTK (~30 ms) y pinta. Es el mismo patrón que ya
usa el `Player` con su `Sink`: la posición es un atómico, no un callback en el hilo de dibujo.

**Esto es la decisión de M5.** Si el motor viviera en el hilo de GTK, la ventana se congelaría
en cada frase.

### 4.3 Flujo de datos del resaltado

```
timeline: word_id → (start_sample, end_sample, exact)     [ya existe]
player:   sample actual → word_id                          [ya existe]
render:   word_id → rect (extents de Pango)                [nuevo]
```

El render mantiene su propio mapa `word_id → Rect` construido en cada pasada de layout. No se
persiste: se recalcula, y es barato porque el layout ya se hizo. Al pintar:

1. Si el span actual es `exact`, tinta la palabra y su frase.
2. Si **no** es `exact`, tinta **solo la frase entera** y no finge ninguna palabra.

El punto 2 es obligatorio y es la regla que el motor ya testea: cuando el motor no puede afirmar
palabra (un número que expande), la UI no puede inventar una.

## 5. Contrato con el motor: `exact` manda

`timeline::Span.exact` es un booleano con significado contractual, no un adorno. La UI tiene tres
estados, no dos:

| Estado | Cuándo | Qué pinta |
|---|---|---|
| `exact` | el motor casó palabra ↔ fonemas 1:1 | palabra con tinte fuerte + frase con tinte suave |
| degradado | conteo de grupos ≠ palabras en ese chunk | frase entera con tinte, sin palabra |
| sin audio | pausado o sin voz | nada |

Y «sin audio» no es una cuarta pintura: es el mismo resaltado congelado donde quedó.

## 6. Librería

- **Carpetas escaneadas**: lista en `config.json`. Al arrancar se escanea; los libros se cachean
  en SQLite por `(path, mtime, size)` para no releer el OPF de lo que no cambió.
- **Metadatos**: título, autor, idioma y portada del OPF del EPUB. La portada se extrae una vez y
  se guarda como miniatura en `~/.cache/readaloud/covers/`.
- **«Añadir carpeta»**: diálogo de selección de directorio; se añade a la lista y se escanea.
- **«Reescanear»**: fuerza la relectura de metadatos. Sin vigilancia de ficheros en M5 (`notify`
  añade piezas móviles para un caso que casi nunca pasa).
- **«Seguir leyendo»**: el libro con `progress.updated_at` más reciente, con su botón.
- Un libro que falla al leer (EPUB roto) **no rompe la librería**: se muestra con un aviso y el
  error concreto.

## 7. Persistencia

### SQLite (`~/.local/share/readaloud/library.db`)

```sql
books(id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL, title TEXT, author TEXT,
      lang TEXT, cover_path TEXT, mtime INTEGER, size INTEGER, added_at INTEGER)
progress(book_id INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
         word_id INTEGER NOT NULL, voice TEXT, updated_at INTEGER NOT NULL)
annotations(id INTEGER PRIMARY KEY, book_id INTEGER REFERENCES books(id) ON DELETE CASCADE,
            start_word_id INTEGER NOT NULL, end_word_id INTEGER NOT NULL,
            note TEXT, color TEXT, created_at INTEGER NOT NULL)
```

`voice` es la voz elegida para ese libro (Q2): un EPUB en inglés y otro en español no deberían
obligar a reelegir voz cada vez. `note` queda en el esquema aunque M5 no la use, para no migrar la
tabla cuando lleguen las notas.

`rusqlite` con la feature `bundled` (sin dependencia del sistema), como ya decidió el diseño
padre. `PRAGMA foreign_keys = ON` al abrir.

El progreso es `(book_id, word_id)` — la misma moneda que todo el sistema. Por eso «seguir
leyendo» reabre exactamente donde ibas y no "por el capítulo".

### Preferencias (`~/.config/readaloud/config.json`)

Carpetas escaneadas, tamaño de letra, familia, interlineado, márgenes, y si el auto-scroll arranca
activo. Ajustes, no datos que consultar: por eso no van a la base de datos.

## 8. Tema

El mecanismo es el que ya verificamos en el diseño padre, copiado del patrón de `zapfast`:

1. La app publica `omarchy/readaloud.json.tpl` con tokens de **lector**, no de chat.
2. `omarchy-theme-set-templates` lo renderiza con la paleta del tema activo.
3. El hook `readaloud-theme` copia el resultado a `~/.config/readaloud/themes/omarchy.json` y
   avisa a la instancia viva con `readaloud reload-themes`.
4. La app genera su hoja CSS de GTK desde ese JSON.

```json
{ "page":      "{{ background }}",
  "text":      "{{ foreground }}",
  "muted":     "{{ mix background foreground 55% }}",
  "accent":    "{{ accent }}",
  "highlight_sentence": "{{ mix accent background 72% }}",
  "highlight_word":     "{{ mix accent background 35% }}",
  "chrome":    "{{ mix background foreground 5% }}",
  "rule":      "{{ mix background foreground 26% }}" }
```

Los dos tokens de resaltado salen del acento mezclado con el fondo: así el resaltado funciona en
cualquier tema claro u oscuro sin que nadie elija colores a mano.

`reload-themes` se implementa con `GApplication` (acción por D-Bus), que además da
single-instance gratis. Sin `omarchy.json`, paleta por defecto; la app **nunca** falla por un tema.

## 9. Render

### Widget

Un `GtkWidget` propio, subclase, dentro de un `GtkScrolledWindow`:

- Compone el capítulo como una **lista de bloques** (párrafo, encabezado, imagen, cita, lista),
  cada uno con su `PangoLayout`.
- Mide con Pango; pinta en `snapshot()`.
- Mantiene `word_id → Rect` calculado durante el layout.
- Selección de texto y hit-testing salen de las mismas llamadas de Pango (`index_to_pos`,
  `xy_to_index`) que dan el mapa de palabras.

### CSS del EPUB → atributos de Pango

Tabla explícita y testeable, ampliando el subconjunto de §2 del diseño padre: familia, tamaño,
peso, cursiva, color, alineación, sangría, márgenes de bloque, interlineado. Lo que no está en la
tabla se ignora y se cuenta como cobertura perdida (la regla de cobertura de M0–M4 se mantiene).

### Auto-scroll

El render recibe el `word_id` actual; si está fuera del área visible, desplaza con una animación
corta para que la palabra quede en el tercio superior. Apagable con el botón «🎧 siguiendo», que
además indica el estado del audio.

## 10. Errores y degradación

| Caso | Comportamiento |
|---|---|
| EPUB ilegible | diálogo con el motivo; el libro queda en la librería marcado |
| Voz ausente | aviso con la acción de prepararla; la lectura en silencio sigue funcionando |
| Sin dispositivo de audio | aviso; el texto se lee y se scrollea, sin voz |
| `library.db` corrupta | se renombra y se reconstruye; se avisa |
| Tema sin paleta | paleta por defecto |
| Span `exact: false` | resaltado de frase, nunca de palabra |
| Fichero de la librería borrado | se marca como ausente, no se borra la fila (conserva progreso y anotaciones) |

Nada de esto puede tumbar la ventana.

## 11. Tests

| Qué | Cómo |
|---|---|
| `session/` | unitarios e integración, con `NullSink::instant()` — igual que hoy `cli/play`, que debe seguir pasando |
| `store/` | tempdir + SQLite: alta, progreso, anotaciones, reescaneo por mtime, cascada al borrar |
| CSS → Pango | tabla: CSS entra, atributos salen; lo no soportado cuenta cobertura |
| Tema | JSON de paleta → hoja CSS; sin JSON → paleta por defecto |
| `word_id → rect` | se testea el **modelo** del layout (bloques → posiciones) sin GTK |
| Índice de capítulos | del nav/NCX del EPUB |
| UI de GTK | **manual**, y se dice aquí en vez de fingir cobertura |

La regla de M0–M4 se mantiene: cada pieza no trivial deja **un** check ejecutable; lo trivial no
lleva test.

## 12. No objetivos

- Búsqueda (en el libro o en la librería).
- Notas de texto en anotaciones: en M5 los resaltados marcan rangos; escribir y editar notas es
  otra UI.
- SSH/SFTP (M6), empaquetado (M7).
- Paginado, dos columnas, vista de dos libros.
- Escritura de metadatos en el EPUB, sync, cuentas, OPDS.

## 13. Riesgos y preguntas abiertas

| # | Riesgo | Mitigación |
|---|---|---|
| R1 | **El widget de render es el subsistema más grande del proyecto.** Puede comerse el calendario. | Se construye **primero y mínimo** (un capítulo, sin librería), para tener la tesis en pantalla antes de cualquier otra pieza. Ver §16. |
| R2 | **El motor no es reentrante**; una decisión ingenua de hilos congela la ventana o cuelga. | `ReadSession` entero en un hilo trabajador (§4.2), una sola instancia. |
| R3 | **La UI puede romper la degradación** pintando palabra donde el motor dijo frase. | `exact` es contractual (§5) y el render lo consulta; se testea el modelo de layout. |
| R4 | **`resolve()` de `epub/` no normaliza `../` ni `%20`**: un libro así **no abre**. | Arreglarlo en M5 (§14): un lector de verdad tiene que abrir los libros reales. |
| R5 | **`doc::Word.paragraph` cuenta solo párrafos con palabras, no bloques del DOM.** El render necesita los bloques. | Arreglarlo en M5 (§14). Era un minor diferido de M0–M4 que predijo exactamente este choque. |

### Preguntas abiertas

- **Q1 — Notas.** ¿Los resaltados llevan nota de texto en M5 o se quedan como marcas? (asumido: sin notas).
- **Q2 — ¿Voz por libro?** Si un EPUB está en inglés y otro en español, ¿la app recuerda la voz elegida por libro? (asumido: sí, una columna en `progress`).

## 14. Deudas de M0–M4 que M5 tiene que resolver

Salieron de las revisiones del motor y ahora chocan con la UI:

1. **`doc::Word.paragraph`** cuenta solo párrafos con palabras. El render necesita bloques → se
   añade el bloque real (o se cambia el campo para que cuente bloques del DOM).
2. **`epub::resolve()`** no normaliza `../` ni decodifica `%20`. Un libro con esos hrefs no abre.
3. **`Player::seek_to_word`** mezcla posición global y local de frase. La UI busca de verdad
   (arrastrar el progreso, saltar por el índice) → hay que dejarlo coherente antes.
4. **`Sink::write` recibe `rate` y ningún sink lo usa.** Se **quita** el parámetro: `CpalSink`
   abre el stream a la tasa de la voz y no remuestrea nada, así que hoy es un dato muerto. Si
   algún día entra remuestreo, vuelve con una implementación detrás en vez de con un hueco.
5. **`say --length-scale`** sin texto de ayuda, misma trampa invertida que se arregló en `play`.

## 15. Decisiones

| # | Decisión | Por qué |
|---|---|---|
| D1 | `session/` extraído de `cli/play` y compartido | Una sola implementación de "cómo se lee un libro"; si no, divergen |
| D2 | Motor, caché y SQLite en un hilo trabajador dedicado | El motor no es reentrante y sintetizar bloquea |
| D3 | Posición como atómico, leída por timeout de GTK | Mismo patrón que el `Sink`; no hace callback en el hilo de dibujo |
| D4 | `Span.exact` conduce la granularidad del resaltado | Es la invariante que el motor ya testea; la UI no puede inventar |
| D5 | Preferencias en JSON, datos en SQLite | Ajustes vs datos que consultar |
| D6 | Progreso como `(book_id, word_id)` | La moneda del sistema; reabrir es exacto, no "por el capítulo" |
| D7 | Sin vigilancia de ficheros en M5 | `notify` añade piezas móviles para un caso raro |
| D8 | Resaltados sin notas de texto en M5 | La UI de escribir/editar notas es un feature propio |
| D9 | El render se construye primero y mínimo | Es el mayor riesgo; hay que ver la tesis en pantalla antes |
| D10 | `reload-themes` vía `GApplication` | Da single-instance y la acción por D-Bus sin escribir IPC |

## 16. Hitos

Ordenado para que **lo arriesgado se pruebe primero**. El plazo de M5 no depende de la librería:
depende de que el widget de render funcione.

| H | Qué | Verificación |
|---|---|---|
| V0 | Arreglos de §14 (bloques, `resolve`, seek, `rate`, ayuda) | La suite de M0–M4 sigue verde; un EPUB con `../` abre |
| V1 | CSS del EPUB → Pango, sobre un capítulo | Tabla de mapeo testeada; un capítulo real se compone con estilos |
| V2 | **Widget de render mínimo**: un capítulo, scroll, resaltado por `word_id` | **La tesis en pantalla**: se ilumina la palabra correcta con el timeline real |
| V3 | Auto-scroll + selección + índice | Leer un libro entero con la vista siguiendo |
| V4 | `session/` extraído, hilo trabajador, reproductor en la ventana | Suena desde la GUI sin congelar; `readaloud play` intacto |
| V5 | `store/` + `theme/` + librería + «Seguir leyendo» | Mis libros en la ventana; reabrir cae donde ibas |
| V6 | Añadir carpeta, reescanear, tipografía configurable, anotaciones | Todo el §2 verificado a mano |

**En V2 el motor se usa de forma síncrona, desde el hilo de GTK.** Es una prueba y la ventana
puede dar tirones entre frases; lo que se verifica ahí es que la palabra correcta se ilumine, no
la fluidez. **V4 es precisamente lo que lo saca del hilo de dibujo** (§4.2). No se construye el
hilo trabajador antes porque hasta V2 no hay nada que descongelar.

**El primer plan cubre V0–V4**: la ventana que lee, con la tesis en pantalla y sonando sin
congelarse. V5–V6 (librería, tema, tipografía, anotaciones) van en un plan posterior, igual que
M0–M4 se dividió en motor-primero. M5 no depende de la librería: depende de que el widget de
render funcione.
