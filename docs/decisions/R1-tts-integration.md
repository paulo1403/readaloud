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
2. Ejecuta el CMake de `libpiper` con `CMAKE_BUILD_TYPE=Release`. Ese CMake, por
   su cuenta, hace `ExternalProject` de **espeak-ng** (commit fijado
   `212928b3…`, compilado estático dentro de `libpiper.so`) y descarga
   **onnxruntime-linux-x64-1.22.0.tgz** prebuilt desde
   `github.com/microsoft/onnxruntime/releases`.
3. Enlaza con `-lpiper` y añade `-Wl,-rpath,<OUT_DIR>/lib` para que el binario
   encuentre `libpiper.so` y `libonnxruntime.so.1` en tiempo de ejecución.

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
`RUNPATH` de `$ORIGIN` (los dos ficheros se instalan juntos), de modo que
libpiper encuentra onnxruntime sin ruta absoluta. El binario de test añade un
`RUNPATH` absoluto a `<dst>/lib` para encontrar `libpiper.so`:

```
libpiper.so         => /tmp/ra-piper-<hash>/lib/libpiper.so
libonnxruntime.so.1 => /tmp/ra-piper-<hash>/lib/libonnxruntime.so.1
libpiper.so RUNPATH => $ORIGIN
```

En `<dst>/lib` el `install` de libpiper deja:

```
libpiper.so
libonnxruntime.so -> libonnxruntime.so.1 -> libonnxruntime.so.1.22.0
libonnxruntime_providers_shared.so
```

### Qué necesita en tiempo de ejecución

- `libpiper.so` + `libonnxruntime.so.1` (ambas bajo `<dst>/lib`, resueltas por
  `RUNPATH` en esta máquina; para un AppImage habría que empaquetarlas).
- Los datos de `espeak-ng` (`/usr/share/espeak-ng-data`, o
  `READALOUD_ESPEAK_DATA`, o los que libpiper instala en `<dst>/share`).

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
   ~180 bytes; el `OUT_DIR` de cargo (`…/target/debug/build/readaloud-<hash>/out`)
   lo desborda, truncando los nombres de `phsource` y abortando el build con
   *Bad vowel file*. Por eso `build.rs` construye el CMake en un directorio corto
   en `/tmp` (`/tmp/ra-piper-<hash>`), derivado del `OUT_DIR`. Es un
   `ponytail:`-style workaround; cuando espeak-ng arregle el buffer se puede
   volver a construir dentro de `OUT_DIR`.
2. **La voz de test no trae alineación.** `es_ES-davefx-medium.onnx` solo tiene
   una salida (`output`); sin `w_ceil` expuesta, `libpiper` devuelve
   `num_alignments = 0` mientras `num_phonemes > 0`, y el test
   `phonemes.len() == alignments.len()` fallaría. Por eso `Engine::load`
   **parchea una copia temporal** del modelo (añade el `ValueInfoProto` del
   tensor `Ceil` a `graph.output`) si hace falta, y carga esa copia. El fichero
   original nunca se modifica. La Task 6 (`voices prepare`) hará el parche
   persistente sobre la voz del usuario; aquí basta con que el motor sea
   autosuficiente.
3. **Semántica de `PhonemeChunk`.** Los tres arrays (`phonemes`,
   `phoneme_ids`, `alignments`) son **paralelos, un elemento por id de fonema**,
   de modo que `phonemes[i]` corresponde a `phoneme_ids[i]` y `alignments[i]`.
   Los separadores internos de la API C (codepoint `0`) se eliminan; los límites
   de palabra quedan visibles como `' '`. Esto hace que las tres longitudes
   coincidan y da a `timeline/` (Task 7) una proyección directa y sin pérdida.
   El algoritmo del Task 7 descrito en el plan (leer hasta un `0`) hay que
   adaptarlo a esta costura: agrupar por `' '` y sumar `alignments`.

## Coste de construcción (medido)

- `cargo test` en frío con espeak-ng y libpiper sin construir: **≈2m37s**
  (incluye clonar piper1-gpl, compilar espeak-ng, compilar libpiper y compilar
  `onnx-protobuf`); con onnxruntime 1.22.0 ya descargado.
- `cargo build --release` en frío (perfil nuevo, espeak-ng + libpiper + todo):
  **3m16s**. Sin red no construye.
- Rebuild incremental tras tocar `build.rs` (solo relink): segundos.
