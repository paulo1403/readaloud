# R1 — Integración TTS: FFI a `libpiper` (ruta (a))

**Fecha:** 2026-10-13
**Task:** 5 (Motor TTS: enlace a piper y síntesis de una frase)
**Estado:** decidido y verificado en esta máquina

## Decisión

Se elige la **ruta (a): FFI a `libpiper`** (piper1-gpl v1.8.0), construida desde
fuente por `build.rs` con el crate `cmake`. Se descarta (b) `ort` + runner
propio: (a) enlaza y sintetiza, y deja la fonemización y el troceado en frases
en el mismo código C++ que usa piper, que es lo que garantiza que la alineación
coincida con el protocolo documentado.

## Contexto que obligó a medir (no re-derivar)

- `espeak-ng` 1.52.0 está instalado, datos en `/usr/share/espeak-ng-data`.
- `cmake` 4.4.4 (mise), `/usr/bin/c++` (GCC 16.2.1), `git` y `curl` presentes.
- **`onnxruntime` NO está instalado como librería de sistema** (ningún
  `libonnxruntime*` en `ldconfig`). `libpiper` lo necesita, así que la ruta (a)
  solo podía funcionar trayendo onnxruntime prebuilt.
- Sin `sudo`; el enlace no lo necesita.

## Cómo se construye

`build.rs`:

1. Clona `https://github.com/OHF-Voice/piper1-gpl.git` en la etiqueta fijada
   **v1.8.0**, en `OUT_DIR` (no se vendoriza el C++: son ~3 MB de cabeceras
   *header-only* grandes y el CMake ya descarga lo demás).
2. Ejecuta el CMake de `libpiper` con `CMAKE_BUILD_TYPE=Release` **en un
   directorio corto de scratch bajo `/tmp`** (`/tmp/ra-piper-<hash>`). Ese CMake,
   por su cuenta, hace `ExternalProject` de **espeak-ng** (commit fijado
   `212928b3…`, compilado estático dentro de `libpiper.so`) y descarga
   **onnxruntime-linux-x64-1.22.0.tgz** prebuilt desde
   `github.com/microsoft/onnxruntime/releases`. El directorio corto no es un
   capricho: el compilador de datos de espeak-ng usa un buffer de ruta fijo de
   ~180 bytes y el `OUT_DIR` de cargo lo desborda (ver desviación 1).
3. Copia `libpiper.so`, `libonnxruntime.so.1` y
   `libonnxruntime_providers_shared.so` desde el scratch de `/tmp` a
   `<OUT_DIR>/lib` (persistente, bajo `target/`) y **enlaza y pone el `rpath`
   desde ahí**: `-lpiper` + `-Wl,-rpath,<OUT_DIR>/lib`. `/tmp` es tmpfs: enlazar
   directamente contra él dejaría los binarios ya construidos sin sus `.so` tras
   un reinicio. `libpiper.so` lleva `RUNPATH=$ORIGIN`, así que encuentra
   `libonnxruntime.so.1`, que está a su lado. `build.rs` emite
   `rerun-if-changed` sobre un marcador `.complete` dentro de `<OUT_DIR>/lib`:
   si el directorio persistente desaparece (p. ej. `cargo clean`), el build
   script se vuelve a ejecutar en lugar de enlazar contra rutas inexistentes.

`cargo build` necesita por tanto **red y `git`**, y nada instalado por sudo.

## Evidencia

### (a) enlaza y sintetiza

- Prueba directa contra la librería compilada (programa C de humo):

  ```
  libpiper version: 1.8.0        (vía crate cmake desde build.rs)
  ```

- Síntesis de `"Hola mundo."` con `es_ES-davefx-medium` parcheada:

  ```
  nph=41 nids=27 nalign=27 nsamp=17408 rate=22050
  ```

  Es decir: la API C devuelve PCM y, con la salida `w_ceil` expuesta,
  `num_alignments == num_phoneme_ids`.

### R3 — el campo de velocidad existe

Confirmado en `libpiper/include/piper.h`: `piper_synthesize_options` tiene el
campo `float length_scale` (documentado: `0.5` = el doble de rápido, `2.0` = el
doble de lento, por defecto `1.0`). Medido con la misma frase:

| `length_scale` | samples |
|---|---|
| 0.5 | 32768 |
| 1.0 | 52992 |
| 2.0 | 88576 |

No hace falta re-sintetizar con la config de la voz: el campo de la API C
funciona. La estrategia de velocidad del plan (§5, D9) queda validada.

### `ldd` del binario de test

`libpiper.so` declara `DT_NEEDED libonnxruntime.so.1` y se le da un
`RUNPATH` de `$ORIGIN` (los dos ficheros acaban juntos), de modo que libpiper
encuentra onnxruntime sin ruta absoluta. El binario de test añade un `RUNPATH`
absoluto a `<OUT_DIR>/lib` para encontrar `libpiper.so`:

```
libpiper.so         => <OUT_DIR>/lib/libpiper.so
libonnxruntime.so.1 => <OUT_DIR>/lib/libonnxruntime.so.1
libpiper.so RUNPATH => $ORIGIN
```

El `install` de libpiper deja en su scratch de `/tmp` (`<scratch>/lib`):

```
libpiper.so
libonnxruntime.so -> libonnxruntime.so.1 -> libonnxruntime.so.1.22.0
libonnxruntime_providers_shared.so
```

de donde `build.rs` copia las tres que se necesitan a `<OUT_DIR>/lib`
(`libonnxruntime.so.1` se copia resolviendo el symlink). Se comprobó con
`readelf -d`: el `RUNPATH` del binario apunta a
`…/target/debug/build/readaloud-<hash>/out/lib`, **no** a `/tmp`.

### Qué necesita en tiempo de ejecución

- `libpiper.so` + `libonnxruntime.so.1` (ambas bajo `<OUT_DIR>/lib`, resueltas
  por `RUNPATH`; para un AppImage habría que empaquetarlas).
- Los datos de `espeak-ng` (`/usr/share/espeak-ng-data`, o
  `READALOUD_ESPEAK_DATA`, o los que libpiper instala en `<scratch>/share`).

### Riesgo: excepciones C++ cruzando el FFI

`piper_create` y `piper_synthesize_*` son C++ y **pueden lanzar excepciones**
(JSON de la voz corrupto, ONNX inválido…). Dejar que una excepción C++ cruce la
frontera `extern "C"` hacia Rust es **comportamiento indefinido** (en la
práctica, `abort`). Mitigación actual: `Engine::load` comprueba que el `.onnx` y
su `.onnx.json` existen antes de llamar. Endurecimiento futuro (no en este
task): un shim C++ con `catch (...)` que traduzca a un código de error.

## Qué necesitaría una máquina limpia

- Para **construir**: red, `git`, `cmake`, compilador de C++ (todo salvo `git`
  ya estaba). Sin `sudo`.
- Para **ejecutar**: las dos `.so` (empaquetadas) y los datos de espeak-ng. No
  requiere ningún paquete del sistema instalado por el usuario.
- **La comprobación de AppImage (arranque en una máquina limpia de verdad) es
  M7 y NO se responde aquí.** Lo medido es que (a) construye de forma razonable
  y no pide dependencias de sistema en tiempo de ejecución *en esta máquina*.

## Desviaciones respecto al plan (anotadas)

1. **El compilador de datos de espeak-ng trunca rutas.** Usa un buffer fijo de
   ~180 bytes; el `OUT_DIR` de cargo
   (`…/target/debug/build/readaloud-<hash>/out`, ~94 chars de prefijo) lo
   desborda, truncando los nombres de `phsource` y abortando el build con *Bad
   vowel file*. Por eso el CMake se construye en un directorio corto de scratch
   en `/tmp` (`/tmp/ra-piper-<hash>`). **Consecuencia real de `/tmp`:** es tmpfs;
   tras un reinicio desaparece, `target/` sobrevive y el build script no se
   re-ejecuta, así que cualquier ruta en `/tmp` (link search path, `-rpath`)
   quedaría rota y los binarios ya construidos no arrancarían. **Resolución:**
   las `.so` se copian a `<OUT_DIR>/lib` (persistente bajo `target/`) y el enlace
   y el `rpath` apuntan ahí; `/tmp` es solo scratch de compilación, con un
   `rerun-if-changed` sobre un marcador persistente para forzar rebuild si ese
   directorio falta. Verificado borrando `/tmp/ra-piper-*`: el binario ya
   construido sigue arrancando y los tests pasan. Cuando espeak-ng arregle su
   buffer, el CMake podrá construirse dentro de `OUT_DIR` y la copia sobrará.
2. **La voz de test no trae alineación.** `es_ES-davefx-medium.onnx` solo tiene
   una salida (`output`); sin `w_ceil` expuesta, `libpiper` devuelve
   `num_alignments = 0` mientras `num_phonemes > 0`, y el test
   `phonemes.len() == alignments.len()` fallaría. Por eso `Engine::load`
   **parchea una copia temporal** del modelo (añade el `ValueInfoProto` del
   tensor `Ceil` a `graph.output`) si hace falta, y carga esa copia. El fichero
   original nunca se modifica. La Task 6 (`voices prepare`) hará el parche
   persistente sobre la voz del usuario; aquí basta con que el motor sea
   autosuficiente.
3. **Semántica de `PhonemeChunk`.** El contrato completo está documentado en
   `src/tts/mod.rs` (struct `PhonemeChunk`). En resumen: `phonemes`,
   `phoneme_ids` y `alignments` son **arrays paralelos de la misma longitud**, un
   elemento por id de fonema del modelo; los separadores internos de la API C
   (codepoint `0`) se eliminan. Cada fonema real aparece **dos veces** (su id de
   modelo + el id de relleno `0`), el flujo va enmarcado por `'^'`/`'^'` (BOS y
   su relleno) al principio y `'$'` (EOS) al final, y el límite de palabra es una
   **tirada** de `' '` (el espacio emite dos entradas). Invariante:
   `sum(alignments) == samples.len()`, fijada por un test. El algoritmo del
   Task 7 descrito en el plan (leer hasta un `0`) hay que adaptarlo a esta
   costura: recorrer los tres arrays en lockstep, saltar el par de BOS y el EOS,
   delimitar palabras por tiradas de `' '` y sumar **todas** las alineaciones de
   cada grupo (incluidos los rellenos).

## Coste de construcción (medido)

- `cargo test` en frío con espeak-ng y libpiper sin construir: **≈2m37s**
  (incluye clonar piper1-gpl, compilar espeak-ng, compilar libpiper y compilar
  `onnx-protobuf`); con onnxruntime 1.22.0 ya descargado.
- `cargo build --release` en frío (perfil nuevo, espeak-ng + libpiper + todo):
  **3m16s**. Sin red no construye.
- Rebuild incremental tras tocar `build.rs` (solo relink): segundos.

## Actualización — Task 6 (registro de voces y parche persistente)

Lo que añade el Task 6 a esta decisión (nada de lo medido arriba cambia):

- **Una sola implementación del parche ONNX.** La edición del grafo que expone
  `w_ceil` (buscar el nodo `Ceil` y añadir su salida a `graph.output`) vive
  ahora en `src/tts/alignment.rs` y la comparten `Engine::load` (copia temporal
  para voces sin preparar) y `voices::prepare` (parche persistente sobre el
  fichero de la voz). Antes estaba duplicada en `engine.rs`.
- **`prepare` es persistente e idempotente.** Escribe el parche en el `.onnx`
  del usuario; `voices::is_prepared(onnx)` (parsea el grafo y comprueba si
  `graph.output` ya contiene la salida de `Ceil`) lo hace idempotente: una voz
  ya parcheada no se reescribe. El fichero temporal de `Engine::load` se
  mantiene como red de seguridad para voces aún no preparadas.
- **La voz de test `es_ES-davefx-medium.onnx` tiene un nodo `Ceil`** cuya salida
  no está expuesta, que es lo que permite parchearla. Una voz sin nodo `Ceil` no
  se puede preparar: `is_prepared` devuelve `false` y `prepare` es un no-op
  (documentado, no un error).
