//! Edición del grafo ONNX que expone la salida `w_ceil` (alineaciones).
//!
//! Es la única implementación del parche. La usan `Engine::load` (sobre una
//! copia temporal, para voces sin preparar) y `voices::prepare` (persistente,
//! sobre el fichero de la voz).

use onnx_protobuf::{ModelProto, ValueInfoProto};

/// Nombre de la salida del nodo `Ceil` (`w_ceil`) que da las alineaciones.
pub(crate) fn ceil_output_name(model: &ModelProto) -> Option<String> {
    let graph = model.graph.as_ref()?;
    for node in &graph.node {
        if node.op_type == "Ceil" {
            for name in &node.output {
                if !name.is_empty() {
                    return Some(name.clone());
                }
            }
        }
    }
    None
}

/// ¿Expone ya el grafo la salida de `Ceil`?
pub(crate) fn has_alignment_output(model: &ModelProto) -> bool {
    let Some(ceil) = ceil_output_name(model) else {
        return false;
    };
    model
        .graph
        .as_ref()
        .is_some_and(|g| g.output.iter().any(|o| o.name == ceil))
}

/// Añade la salida de `Ceil` a `graph.output` si falta.
///
/// Devuelve `true` si modificó el grafo. Devuelve `false` si ya estaba expuesta
/// o si el grafo no tiene ningún nodo `Ceil` (nada que parchear).
pub(crate) fn add_alignment_output(model: &mut ModelProto) -> bool {
    let Some(ceil) = ceil_output_name(model) else {
        return false;
    };
    let Some(graph) = model.graph.as_mut() else {
        return false;
    };
    if graph.output.iter().any(|o| o.name == ceil) {
        return false;
    }
    let mut info = ValueInfoProto::new();
    info.name = ceil;
    graph.output.push(info);
    true
}
