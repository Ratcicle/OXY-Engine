//! Native action descriptions are the single contract shared by graph editing and runtime.
//! IDs are persisted. A future Lua adapter can use this contract without changing documents.
use crate::document::{Value, new_id};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
mod audio;
mod game;
pub mod layout;
mod math;
mod movement;
pub use audio::{SOUND_CHANNELS, STOP_CHANNELS, VOLUME_CHANNELS, audio_choices, audio_help};
pub use game::{game_choices, game_help};
pub use layout::{arrange, node_extent};
pub use math::{ARITHMETIC, math_choices, math_help};
pub use movement::{movement_choices, movement_help};

/// When an input event fires: value stored and label shown.
pub const INPUT_MODES: [(&str, &str); 3] = [
    ("pressed", "Pressionar"),
    ("held", "Manter"),
    ("released", "Soltar"),
];
/// Which change of the game pause Ao pausar ou retomar answers.
pub const PAUSE_CHANGES: [(&str, &str); 2] = [("pause", "Ao pausar"), ("resume", "Ao retomar")];
/// Parts of an object that Ativar / desativar componente switches.
pub const COMPONENTS: [(&str, &str); 6] = [
    ("collider", "Colisão / área (2D e 3D)"),
    ("controller", "Controlador de movimento"),
    ("visible", "Visibilidade"),
    ("behavior", "Comportamento visual"),
    ("animation", "Reprodução da animação"),
    ("platform", "Plataforma móvel"),
];
pub const COMPARISONS: [(&str, &str); 6] = [
    ("==", "Igual · A = B"),
    ("!=", "Diferente · A ≠ B"),
    (">", "Maior · A > B"),
    (">=", "Maior ou igual · A ≥ B"),
    ("<", "Menor · A < B"),
    ("<=", "Menor ou igual · A ≤ B"),
];

/// Text parameters that store one of a few fixed values, with their labels. Any other value
/// is never matched by the runtime, so the node would silently do nothing.
pub fn choices(
    operation: &str,
    parameter: &str,
) -> Option<&'static [(&'static str, &'static str)]> {
    movement_choices(operation, parameter)
        .or_else(|| math_choices(operation, parameter))
        .or_else(|| game_choices(operation, parameter))
        .or_else(|| audio_choices(operation, parameter))
        .or(match (operation, parameter) {
            ("event.input", "mode") => Some(&INPUT_MODES),
            ("event.pause", "when") => Some(&PAUSE_CHANGES),
            ("action.component", "component") => Some(&COMPONENTS),
            ("condition.compare", "operator") => Some(&COMPARISONS),
            _ => None,
        })
}

/// Checks the fixed choices of `params` given to a node of `operation`.
pub fn check_choices<'a>(
    operation: &str,
    params: impl IntoIterator<Item = (&'a String, &'a Value)>,
) -> Result<(), String> {
    for (key, value) in params {
        let (Some(options), Value::Text(text)) = (choices(operation, key), value) else {
            continue;
        };
        if !options.iter().any(|(id, _)| id == text) {
            return Err(format!(
                "O parâmetro {key} do nó {operation} aceita {}; recebeu \"{text}\"",
                options
                    .iter()
                    .map(|(id, label)| format!("{id} ({label})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    Ok(())
}

/// What a parameter names in the project, besides its own value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reference {
    /// A scene ID.
    Scene,
    /// The ID of a WAV asset.
    Audio,
    /// The ID of an animation clip of an object in the scene.
    Clip,
    /// An object ID (empty: the node's own object).
    Object,
    /// The name of an attribute of the target object.
    Attribute,
    /// The ID of an input action (Ações de entrada).
    InputAction,
    /// The ID of a physics surface.
    Surface,
    /// The name of a marker in an animation clip.
    Marker,
    /// The ID of a game variable (Variáveis do jogo).
    Variable,
}

/// What the parameter `param` of `operation` refers to, if anything.
pub fn reference(operation: &str, param: &ParamDef) -> Option<Reference> {
    Some(match (operation, param.id, &param.default) {
        (_, _, Value::Object(_)) => Reference::Object,
        (_, _, Value::Surface(_)) => Reference::Surface,
        ("action.scene" | "value.scene", "scene", _) => Reference::Scene,
        ("action.sound" | "action.sound_loop" | "action.sound_stop", "asset", _) => {
            Reference::Audio
        }
        ("action.animation", "clip", _) => Reference::Clip,
        ("event.input" | "input.read", "action", _) => Reference::InputAction,
        ("event.animation", "marker", _) => Reference::Marker,
        ("variable.get" | "variable.set", "variable", _) => Reference::Variable,
        (_, "attribute", Value::Text(_)) => Reference::Attribute,
        _ => return None,
    })
}

/// What an empty value of a parameter that names something of the project ([`reference`])
/// stands for, when it is a choice of its own: Parar som's audio stops every loop of the
/// channel. Other empty references are left unset.
pub fn empty_choice(operation: &str, parameter: &str) -> Option<&'static str> {
    match (operation, parameter) {
        ("action.sound_stop", "asset") => Some("Todos os sons do canal"),
        _ => None,
    }
}

/// The parameters of `node` that name something of the project ([`reference`]), in registry
/// order, with what they name. Integrity checks go through here, so a new node that names a
/// scene, an audio, a clip, an input action or a game variable needs only its line in
/// [`reference`].
pub fn node_references(
    node: &Node,
) -> impl Iterator<Item = (&'static ParamDef, Reference, &Value)> {
    definition(&node.operation)
        .into_iter()
        .flat_map(|def| def.params.iter().map(move |param| (def.id, param)))
        .filter_map(move |(operation, param)| {
            Some((
                param,
                reference(operation, param)?,
                node.params.get(param.id)?,
            ))
        })
}

/// The registry entry of `operation`.
pub fn definition(operation: &str) -> Option<&'static OperationDef> {
    static INDEX: std::sync::OnceLock<HashMap<&str, &OperationDef>> = std::sync::OnceLock::new();
    INDEX
        .get_or_init(|| registry().iter().map(|def| (def.id, def)).collect())
        .get(operation)
        .copied()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortType {
    Exec,
    Number,
    Text,
    Bool,
    Object,
    Vector2,
    Vector3,
    Surface,
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
            Self::Vector2 => "Vetor2",
            Self::Vector3 => "Vetor3",
            Self::Surface => "Superfície física",
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

/// What a node does, for catalogs, tooltips and agents.
pub fn description(operation: &str) -> &'static str {
    match operation {
        "event.scene_start" => "Executa quando a cena começa.",
        "event.input" => {
            "Executa ao pressionar, manter ou soltar uma ação configurada em Ações de entrada."
        }
        "event.click" => "Executa ao clicar neste objeto ou em seu botão de interface.",
        "event.area_enter" => {
            "Executa quando um objeto entra na área e fornece esse objeto na saída."
        }
        "event.animation" => "Executa quando a reprodução atravessa o marcador escolhido.",
        "value.number" => "Fornece um número definido no painel de parâmetros.",
        "value.text" => "Fornece um texto definido no painel de parâmetros.",
        "value.bool" => "Fornece um valor verdadeiro ou falso.",
        "value.object" => "Fornece a referência de um objeto da cena.",
        "value.scene" => {
            "Fornece o identificador da cena escolhida pelo nome, para Mudar de cena e atributos de texto."
        }
        "attribute.get" => "Lê o atributo escolhido do objeto informado.",
        "attribute.set" => "Altera o atributo escolhido do objeto informado.",
        "math.binary" => {
            "Soma, subtrai, multiplica, divide, tira o resto ou escolhe o menor ou o maior entre os números A e B."
        }
        "condition.compare" => "Compara A e B e fornece verdadeiro ou falso.",
        "condition.branch" => "Segue a saída Verdadeiro ou Falso conforme a condição.",
        "action.damage" => {
            "Reduz o atributo numérico configurado no alvo. Pode limitar a um acerto por ativação."
        }
        "action.animation" => "Reproduz uma animação do objeto escolhido.",
        "action.sound" => {
            "Toca um áudio WAV do projeto uma vez, no canal de efeitos, no volume configurado."
        }
        "action.spawn" => "Cria uma cópia da hierarquia do objeto durante o jogo.",
        "action.remove" => "Remove o objeto e seus descendentes durante o jogo.",
        "action.component" => {
            "Ativa ou desativa o componente escolhido do objeto: colisor ou área (2D e 3D), controlador, visibilidade, comportamento, animação ou plataforma móvel."
        }
        "action.scene" => {
            "Encerra a cena atual e abre a cena de destino, ou a que a entrada Cena receber: ligada a Cena atual, reinicia a cena."
        }
        "control.sequence" => "Executa as saídas 1, 2 e 3 nessa ordem.",
        "control.wait" => "Continua após o tempo indicado, sem interromper o jogo ou o editor.",
        "debug.message" => "Exibe uma mensagem no diagnóstico durante a execução.",
        // The guide documents every registered operation, movement nodes included.
        _ => crate::guide::topics()
            .iter()
            .find(|topic| topic.operation.id == operation)
            .map_or(
                "Operação desconhecida. Verifique a compatibilidade do projeto.",
                |topic| topic.purpose,
            ),
    }
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
fn v3(id: &'static str, label: &'static str, value: [f32; 3]) -> ParamDef {
    param(id, label, Value::Vector3(value))
}
fn flag(id: &'static str, label: &'static str, value: bool) -> ParamDef {
    param(id, label, Value::Bool(value))
}
/// The object a node acts on or reads (empty: the node's own object).
fn target() -> ParamDef {
    param("target", "Objeto", Value::Object(None))
}
/// An operation whose non-text parameters are also input ports of the same ID; an action
/// gets the Executar input and the Continuar output first.
fn op(
    id: &'static str,
    label: &'static str,
    category: &'static str,
    action: bool,
    params: Vec<ParamDef>,
    outputs: Vec<PortDef>,
) -> OperationDef {
    let mut inputs: Vec<_> = params
        .iter()
        .filter(|p| !matches!(p.default, Value::Text(_)))
        .map(|p| port(p.id, p.label, value_type(&p.default)))
        .collect();
    let mut outputs = outputs;
    if action {
        inputs.insert(0, port("exec", "Executar", PortType::Exec));
        outputs.insert(0, port("exec", "Continuar", PortType::Exec));
    }
    OperationDef {
        id,
        label,
        category,
        inputs,
        outputs,
        params,
    }
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
            "Ação de entrada",
            vec![
                text("action", "Ação", "atacar"),
                text("mode", "Quando", "pressed"),
            ],
        ),
        ("event.click", "Ao clicar", vec![]),
        ("event.area_enter", "Ao entrar na área", vec![]),
        ("event.area_exit", "Ao sair da área", vec![]),
        (
            "event.animation",
            "Marcador de animação",
            vec![text("marker", "Marcador", "Impacto")],
        ),
        (
            "event.pause",
            "Ao pausar ou retomar",
            vec![text("when", "Quando", "pause")],
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
    // A scene picked by name in the editor, given as the ID that Mudar de cena takes.
    result.push(OperationDef {
        id: "value.scene",
        label: "Referência de cena",
        category: "Dados",
        inputs: vec![],
        outputs: vec![port("value", "Cena", Text)],
        params: vec![text("scene", "Cena", "")],
    });
    result.push(OperationDef {
        id: "attribute.get",
        label: "Ler atributo",
        category: "Dados",
        inputs: vec![port("target", "Objeto (vazio = este)", Object)],
        outputs: vec![port("value", "Valor", Any)],
        params: vec![target(), text("attribute", "Atributo", "Vida")],
    });
    result.extend(math::definitions());
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
            target(),
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
            target(),
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
            target(),
            text("clip", "Animação", ""),
            param("restart", "Reiniciar", Value::Bool(true)),
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
            target(),
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
        params: vec![target()],
    });
    result.push(OperationDef {
        id: "action.component",
        label: "Ativar / desativar componente",
        category: "Ações",
        inputs: exec_target(),
        outputs: exec(),
        params: vec![
            target(),
            text("component", "Componente", "collider"),
            param("enabled", "Ativo", Value::Bool(true)),
        ],
    });
    result.push(OperationDef {
        id: "action.scene",
        label: "Mudar de cena",
        category: "Ações",
        // A connected Cena replaces the parameter, which keeps the scene it names.
        inputs: vec![port("exec", "Executar", Exec), port("scene", "Cena", Text)],
        outputs: vec![],
        params: vec![text("scene", "Cena de destino", "")],
    });
    result.extend(math::actions());
    result.extend(game::definitions());
    result.extend(audio::definitions());
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
    result.extend(movement::definitions());
    result
}

impl Graph {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }
    /// Whether a connection reaches the input `port` of `node`, which then gives the value in
    /// place of the parameter of the same id.
    pub fn input_connected(&self, node: &str, port: &str) -> bool {
        self.edges
            .iter()
            .any(|edge| edge.to_node == node && edge.to_port == port)
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
                if !value.is_finite() {
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
        Value::Vector2(_) => PortType::Vector2,
        Value::Vector3(_) => PortType::Vector3,
        Value::Surface(_) => PortType::Surface,
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
