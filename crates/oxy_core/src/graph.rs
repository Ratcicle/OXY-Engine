//! Native action descriptions are the single contract shared by graph editing and runtime.
//! IDs are persisted. A future Lua adapter can use this contract without changing documents.
use crate::document::{Value, new_id};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortType {
    Exec,
    Number,
    Text,
    Bool,
    Object,
    Any,
}
impl PortType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Exec => "Execução",
            Self::Number => "Número",
            Self::Text => "Texto",
            Self::Bool => "Booleano",
            Self::Object => "Objeto",
            Self::Any => "Qualquer dado",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PortDef {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: PortType,
}
#[derive(Clone, Debug)]
pub struct ParamDef {
    pub id: &'static str,
    pub label: &'static str,
    pub default: Value,
}
#[derive(Clone, Debug)]
pub struct OperationDef {
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub inputs: Vec<PortDef>,
    pub outputs: Vec<PortDef>,
    pub params: Vec<ParamDef>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub operation: String,
    pub position: [f32; 2],
    pub params: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
}

impl Node {
    pub fn new(operation: &str, position: [f32; 2]) -> Self {
        let params = registry()
            .iter()
            .find(|item| item.id == operation)
            .map(|item| {
                item.params
                    .iter()
                    .map(|param| (param.id.into(), param.default.clone()))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            id: new_id(),
            operation: operation.into(),
            position,
            params,
        }
    }
    pub fn text(&self, key: &str) -> &str {
        if let Some(Value::Text(value)) = self.params.get(key) {
            value
        } else {
            ""
        }
    }
    pub fn number(&self, key: &str, fallback: f64) -> f64 {
        self.params
            .get(key)
            .and_then(Value::number)
            .unwrap_or(fallback)
    }
    pub fn boolean(&self, key: &str, fallback: bool) -> bool {
        self.params
            .get(key)
            .and_then(Value::boolean)
            .unwrap_or(fallback)
    }
}

fn port(id: &'static str, label: &'static str, kind: PortType) -> PortDef {
    PortDef { id, label, kind }
}
fn param(id: &'static str, label: &'static str, default: Value) -> ParamDef {
    ParamDef { id, label, default }
}
fn text(id: &'static str, label: &'static str, value: &str) -> ParamDef {
    param(id, label, Value::Text(value.into()))
}
fn number(id: &'static str, label: &'static str, value: f64) -> ParamDef {
    param(id, label, Value::Number(value))
}

pub fn registry() -> &'static [OperationDef] {
    static REGISTRY: std::sync::OnceLock<Vec<OperationDef>> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(build_registry)
}
fn build_registry() -> Vec<OperationDef> {
    use PortType::*;
    let mut result = Vec::new();
    for (id, label, params) in [
        ("event.scene_start", "Ao iniciar cena", vec![]),
        (
            "event.input",
            "Ao pressionar ação",
            vec![text("action", "Ação", "atacar")],
        ),
        ("event.click", "Ao clicar", vec![]),
        ("event.area_enter", "Ao entrar na área", vec![]),
        (
            "event.animation",
            "Marcador de animação",
            vec![text("marker", "Marcador", "Impacto")],
        ),
    ] {
        result.push(OperationDef {
            id,
            label,
            category: "Eventos",
            inputs: vec![],
            outputs: vec![
                port("exec", "Executar", Exec),
                port("context", "Objeto do evento", Object),
                port("self", "Este objeto", Object),
            ],
            params,
        });
    }
    for (id, label, kind, default) in [
        ("value.number", "Número", Number, Value::Number(0.0)),
        ("value.text", "Texto", Text, Value::Text(String::new())),
        ("value.bool", "Booleano", Bool, Value::Bool(false)),
        (
            "value.object",
            "Referência de objeto",
            Object,
            Value::Object(None),
        ),
    ] {
        result.push(OperationDef {
            id,
            label,
            category: "Dados",
            inputs: vec![],
            outputs: vec![port("value", "Valor", kind)],
            params: vec![param("value", "Valor", default)],
        });
    }
    result.push(OperationDef {
        id: "attribute.get",
        label: "Ler atributo",
        category: "Dados",
        inputs: vec![port("target", "Objeto (vazio = este)", Object)],
        outputs: vec![port("value", "Valor", Any)],
        params: vec![
            param("target", "Objeto", Value::Object(None)),
            text("attribute", "Atributo", "Vida"),
        ],
    });
    result.push(OperationDef {
        id: "math.binary",
        label: "Operação aritmética",
        category: "Dados",
        inputs: vec![port("a", "A", Number), port("b", "B", Number)],
        outputs: vec![port("value", "Resultado", Number)],
        params: vec![
            number("a", "A", 0.0),
            number("b", "B", 1.0),
            text("operator", "Operação (+ - * /)", "+"),
        ],
    });
    result.push(OperationDef {
        id: "condition.compare",
        label: "Comparar",
        category: "Condições",
        inputs: vec![port("a", "A", Any), port("b", "B", Any)],
        outputs: vec![port("result", "Resultado", Bool)],
        params: vec![
            number("a", "A", 0.0),
            number("b", "B", 0.0),
            text("operator", "Comparação (== != > >= < <=)", ">="),
        ],
    });
    result.push(OperationDef {
        id: "condition.branch",
        label: "Se / Senão",
        category: "Condições",
        inputs: vec![
            port("exec", "Executar", Exec),
            port("condition", "Condição", Bool),
        ],
        outputs: vec![
            port("then", "Verdadeiro", Exec),
            port("else", "Falso", Exec),
        ],
        params: vec![param("condition", "Condição", Value::Bool(true))],
    });
    let exec = || vec![port("exec", "Executar", Exec)];
    let exec_target = || {
        vec![
            port("exec", "Executar", Exec),
            port("target", "Objeto (vazio = este)", Object),
        ]
    };
    let self_param = || param("target", "Objeto", Value::Object(None));
    result.push(OperationDef {
        id: "attribute.set",
        label: "Alterar atributo",
        category: "Ações",
        inputs: {
            let mut inputs = exec_target();
            inputs.push(port("value", "Novo valor", Any));
            inputs
        },
        outputs: exec(),
        params: vec![
            self_param(),
            text("attribute", "Atributo", "Vida"),
            number("value", "Novo valor", 100.0),
        ],
    });
    result.push(OperationDef {
        id: "action.damage",
        label: "Aplicar dano",
        category: "Ações",
        inputs: {
            let mut inputs = exec_target();
            inputs.push(port("amount", "Quantidade", Number));
            inputs
        },
        outputs: exec(),
        params: vec![
            self_param(),
            text("attribute", "Atributo numérico", "Vida"),
            number("amount", "Quantidade", 10.0),
            param("once", "Uma vez por ativação/alvo", Value::Bool(true)),
        ],
    });
    result.push(OperationDef {
        id: "action.animation",
        label: "Reproduzir animação",
        category: "Ações",
        inputs: exec_target(),
        outputs: exec(),
        params: vec![
            self_param(),
            text("clip", "Animação", ""),
            param("restart", "Reiniciar", Value::Bool(true)),
        ],
    });
    result.push(OperationDef {
        id: "action.sound",
        label: "Reproduzir WAV",
        category: "Ações",
        inputs: exec(),
        outputs: exec(),
        params: vec![
            text("asset", "Áudio", ""),
            number("volume", "Volume (0 a 1)", 1.0),
        ],
    });
    result.push(OperationDef {
        id: "action.spawn",
        label: "Criar cópia de objeto",
        category: "Ações",
        inputs: exec_target(),
        outputs: vec![
            port("exec", "Executar", Exec),
            port("created", "Objeto criado", Object),
        ],
        params: vec![
            self_param(),
            number("x", "Deslocamento X", 0.0),
            number("y", "Deslocamento Y", 0.0),
            number("z", "Deslocamento Z", 0.0),
        ],
    });
    result.push(OperationDef {
        id: "action.remove",
        label: "Remover objeto",
        category: "Ações",
        inputs: exec_target(),
        outputs: exec(),
        params: vec![self_param()],
    });
    result.push(OperationDef {
        id: "action.component",
        label: "Ativar / desativar componente",
        category: "Ações",
        inputs: exec_target(),
        outputs: exec(),
        params: vec![
            self_param(),
            text("component", "Componente", "collider"),
            param("enabled", "Ativo", Value::Bool(true)),
        ],
    });
    result.push(OperationDef {
        id: "action.scene",
        label: "Mudar de cena",
        category: "Ações",
        inputs: exec(),
        outputs: vec![],
        params: vec![text("scene", "Cena de destino", "")],
    });
    result.push(OperationDef {
        id: "control.sequence",
        label: "Sequência",
        category: "Controle",
        inputs: exec(),
        outputs: vec![
            port("first", "1", Exec),
            port("second", "2", Exec),
            port("third", "3", Exec),
        ],
        params: vec![],
    });
    result.push(OperationDef {
        id: "control.wait",
        label: "Esperar sem bloquear",
        category: "Controle",
        inputs: {
            let mut inputs = exec();
            inputs.push(port("seconds", "Segundos", Number));
            inputs
        },
        outputs: exec(),
        params: vec![number("seconds", "Segundos", 0.5)],
    });
    result.push(OperationDef {
        id: "debug.message",
        label: "Mensagem de diagnóstico",
        category: "Controle",
        inputs: exec(),
        outputs: exec(),
        params: vec![text("message", "Mensagem", "Ação executada")],
    });
    result
}

impl Graph {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }
    pub fn connect(&mut self, edge: &Edge, object_ids: &HashSet<String>) -> Result<(), String> {
        let mut draft = self.clone();
        draft.edges.push(edge.clone());
        draft.validate(object_ids)?;
        self.edges.push(edge.clone());
        Ok(())
    }
    pub fn remap_ids(&mut self, ids: &HashMap<String, String>) {
        for node in &mut self.nodes {
            for value in node.params.values_mut() {
                if let Value::Object(Some(id)) = value
                    && let Some(new) = ids.get(id)
                {
                    *id = new.clone();
                }
            }
        }
    }
    pub fn validate(&self, object_ids: &HashSet<String>) -> Result<(), String> {
        if self.nodes.len() > 4096 || self.edges.len() > 16384 {
            return Err("Grafo excede o limite de 4096 nós / 16384 conexões.".into());
        }
        let definitions = registry();
        let mut node_ids = HashSet::new();
        for node in &self.nodes {
            if node.id.is_empty() || !node_ids.insert(node.id.as_str()) {
                return Err("ID de nó vazio ou duplicado.".into());
            }
            let definition = definitions
                .iter()
                .find(|def| def.id == node.operation)
                .ok_or_else(|| {
                    format!("Nó {}: operação desconhecida {}", node.id, node.operation)
                })?;
            if !node.position.iter().all(|value| value.is_finite()) {
                return Err(format!("Nó {}: posição inválida", node.id));
            }
            for param in &definition.params {
                if !node.params.contains_key(param.id) {
                    return Err(format!(
                        "Nó {}: parâmetro obrigatório ausente {}",
                        node.id, param.id
                    ));
                }
            }
            for (key, value) in &node.params {
                if let Value::Object(Some(id)) = value
                    && !object_ids.contains(id)
                {
                    return Err(format!(
                        "Nó {}: referência de objeto inexistente em {key}: {id}",
                        node.id
                    ));
                }
                if let Value::Number(value) = value
                    && !value.is_finite()
                {
                    return Err(format!(
                        "Nó {}: parâmetro numérico inválido em {key}",
                        node.id
                    ));
                }
                if let Some(param) = definition.params.iter().find(|param| param.id == key) {
                    // Any inputs deliberately accept all editable attribute value types.
                    let any_input = definition
                        .inputs
                        .iter()
                        .any(|port| port.id == key && port.kind == PortType::Any);
                    if !any_input && value_type(value) != value_type(&param.default) {
                        return Err(format!(
                            "Nó {}: tipo inválido para parâmetro {key}",
                            node.id
                        ));
                    }
                } else {
                    return Err(format!("Nó {}: parâmetro desconhecido {key}", node.id));
                }
            }
        }
        let mut input_connections = HashSet::new();
        let mut unique_edges = HashSet::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &self.edges {
            if !unique_edges.insert((
                &edge.from_node,
                &edge.from_port,
                &edge.to_node,
                &edge.to_port,
            )) {
                return Err("A mesma conexão já existe no grafo.".into());
            }
            let from = self
                .node(&edge.from_node)
                .ok_or_else(|| format!("Conexão sai de nó ausente: {}", edge.from_node))?;
            let to = self
                .node(&edge.to_node)
                .ok_or_else(|| format!("Conexão chega a nó ausente: {}", edge.to_node))?;
            let from_def = definitions
                .iter()
                .find(|def| def.id == from.operation)
                .unwrap();
            let to_def = definitions
                .iter()
                .find(|def| def.id == to.operation)
                .unwrap();
            let output = from_def
                .outputs
                .iter()
                .find(|port| port.id == edge.from_port)
                .ok_or_else(|| {
                    format!("Nó {}: porta de saída ausente {}", from.id, edge.from_port)
                })?;
            let input = to_def
                .inputs
                .iter()
                .find(|port| port.id == edge.to_port)
                .ok_or_else(|| {
                    format!("Nó {}: porta de entrada ausente {}", to.id, edge.to_port)
                })?;
            if !compatible(output.kind, input.kind) {
                return Err(format!(
                    "Conexão incompatível: {} → {} (nó {})",
                    output.kind.label(),
                    input.kind.label(),
                    to.id
                ));
            }
            if input.kind != PortType::Exec
                && !input_connections.insert((&edge.to_node, &edge.to_port))
            {
                return Err(format!(
                    "Nó {}: entrada {} já possui uma conexão de dados",
                    to.id, edge.to_port
                ));
            }
            adjacency
                .entry(&edge.from_node)
                .or_default()
                .push(&edge.to_node);
        }
        fn visit<'a>(
            id: &'a str,
            adjacency: &HashMap<&'a str, Vec<&'a str>>,
            active: &mut HashSet<&'a str>,
            finished: &mut HashSet<&'a str>,
        ) -> bool {
            if finished.contains(id) {
                return true;
            }
            if !active.insert(id) {
                return false;
            }
            if let Some(next) = adjacency.get(id) {
                for id in next {
                    if !visit(id, adjacency, active, finished) {
                        return false;
                    }
                }
            }
            active.remove(id);
            finished.insert(id);
            true
        }
        let mut active = HashSet::new();
        let mut finished = HashSet::new();
        for node in &self.nodes {
            if !visit(&node.id, &adjacency, &mut active, &mut finished) {
                return Err(format!(
                    "Nó {}: ciclo não suportado; use eventos e Esperar para retomadas finitas",
                    node.id
                ));
            }
        }
        Ok(())
    }
}

pub fn value_type(value: &Value) -> PortType {
    match value {
        Value::Number(_) => PortType::Number,
        Value::Text(_) => PortType::Text,
        Value::Bool(_) => PortType::Bool,
        Value::Object(_) => PortType::Object,
    }
}
pub fn compatible(output: PortType, input: PortType) -> bool {
    if output == PortType::Exec || input == PortType::Exec {
        output == input
    } else {
        output == input || output == PortType::Any || input == PortType::Any
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_port_mismatch_and_cycles() {
        let event = Node::new("event.scene_start", [0.0; 2]);
        let value = Node::new("value.number", [0.0; 2]);
        let action = Node::new("debug.message", [0.0; 2]);
        let mut graph = Graph {
            nodes: vec![event.clone(), value.clone(), action.clone()],
            edges: vec![],
        };
        let mut edge = Edge {
            from_node: value.id,
            from_port: "value".into(),
            to_node: action.id.clone(),
            to_port: "exec".into(),
        };
        assert!(
            graph
                .connect(&edge, &HashSet::new())
                .unwrap_err()
                .contains("Conexão incompatível: Número → Execução")
        );
        edge.from_node = event.id;
        edge.from_port = "exec".into();
        graph.connect(&edge, &HashSet::new()).unwrap();
        edge.from_node = action.id;
        assert!(
            graph
                .connect(&edge, &HashSet::new())
                .unwrap_err()
                .contains("ciclo")
        );
    }
    #[test]
    fn references_are_checked_and_remapped_without_changing_labels() {
        let mut node = Node::new("value.object", [12.0, 35.0]);
        node.params
            .insert("value".into(), Value::Object(Some("a".into())));
        let mut graph = Graph {
            nodes: vec![node],
            edges: vec![],
        };
        assert!(graph.validate(&HashSet::new()).is_err());
        graph.remap_ids(&HashMap::from([("a".into(), "b".into())]));
        graph.validate(&HashSet::from(["b".into()])).unwrap();
        assert_eq!(
            serde_json::from_str::<Graph>(&serde_json::to_string(&graph).unwrap()).unwrap(),
            graph
        );
    }
}
