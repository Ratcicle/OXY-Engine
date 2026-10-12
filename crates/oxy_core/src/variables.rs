//! Game variables: values declared for the whole project that logic reads and changes while
//! the game runs, kept across scene changes. Each has a stable ID, which logic nodes store,
//! and a name people and agents use; its initial value gives its type.
use crate::{
    document::{Id, Project, Value, new_id},
    graph::{self, Reference},
    input_actions::{Use, every_entity},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Most variables a project declares.
pub const MAX_VARIABLES: usize = 256;
/// Longest text a variable holds, and longest key, in bytes.
pub const MAX_TEXT: usize = 4096;
/// Most keys one variable holds a value for while the game runs.
pub const MAX_KEYS: usize = 1024;
/// What reports put between a variable's name and a key ([`label`]); names never have it, so
/// two values never share a label.
const KEY_SEPARATOR: char = '·';

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameVariable {
    pub id: Id,
    pub name: String,
    /// The value the game starts with; its type is the variable's type.
    pub value: Value,
    /// Kept between runs of the game ("Salva entre partidas").
    #[serde(default)]
    pub persistent: bool,
}
impl GameVariable {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || self.name.trim().is_empty() {
            return Err("Variável do jogo precisa de identificador e nome.".into());
        }
        if self.name.contains(KEY_SEPARATOR) {
            return Err(format!(
                "Variável do jogo {}: o nome não pode ter {KEY_SEPARATOR}.",
                self.name
            ));
        }
        check_value(&self.value)
            .map_err(|error| format!("Variável do jogo {}: {error}.", self.name))
    }
}

/// Why a variable cannot hold `value`: it takes numbers, texts, true or false and vectors,
/// finite, with texts of at most [`MAX_TEXT`] bytes.
pub fn check_value(value: &Value) -> Result<(), String> {
    match value {
        // An object's ID belongs to its scene: after a scene change it would name nothing.
        Value::Object(_) => Err(
            "uma variável do jogo não guarda objetos, que deixam de existir ao mudar de cena; \
             use número, texto, booleano, Vetor2 ou Vetor3"
                .into(),
        ),
        Value::Surface(_) => Err(
            "uma variável do jogo não guarda superfícies físicas; use número, texto, booleano, \
             Vetor2 ou Vetor3"
                .into(),
        ),
        Value::Text(text) if text.len() > MAX_TEXT => {
            Err(format!("o texto passa de {MAX_TEXT} bytes"))
        }
        value if !value.is_finite() => Err("o valor precisa ser finito".into()),
        _ => Ok(()),
    }
}

/// The type of `value` as values are tagged: Number, Text, Bool, Vector2 or Vector3.
pub fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Number(_) => "Number",
        Value::Text(_) => "Text",
        Value::Bool(_) => "Bool",
        Value::Object(_) => "Object",
        Value::Vector2(_) => "Vector2",
        Value::Vector3(_) => "Vector3",
        Value::Surface(_) => "Surface",
    }
}

/// The type of `value` in a sentence: "número", "texto"…
pub fn kind(value: &Value) -> String {
    graph::value_type(value).label().to_lowercase()
}

/// How reports name a value of a variable: its name, or "name · key" for a key's value.
pub fn label(name: &str, key: &str) -> String {
    if key.is_empty() {
        name.into()
    } else {
        format!("{name} {KEY_SEPARATOR} {key}")
    }
}

/// Checks the variables of a project: how many, each one, and IDs and names (in any case)
/// apart. Their IDs join `identifiers`, the project's.
pub(crate) fn validate_all(
    variables: &[GameVariable],
    identifiers: &mut HashSet<Id>,
) -> Result<(), String> {
    if variables.len() > MAX_VARIABLES {
        return Err(format!(
            "Limite de {MAX_VARIABLES} variáveis do jogo excedido."
        ));
    }
    let mut names = HashSet::new();
    for variable in variables {
        variable.validate()?;
        if !identifiers.insert(variable.id.clone()) {
            return Err(format!(
                "Identificador da variável do jogo {} repetido.",
                variable.name
            ));
        }
        if !names.insert(variable.name.trim().to_lowercase()) {
            return Err(format!(
                "Há duas variáveis do jogo chamadas {}.",
                variable.name.trim()
            ));
        }
    }
    Ok(())
}

pub fn get<'a>(project: &'a Project, id: &str) -> Option<&'a GameVariable> {
    project.variables.iter().find(|v| v.id == id)
}
fn get_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut GameVariable, String> {
    project
        .variables
        .iter_mut()
        .find(|v| v.id == id)
        .ok_or_else(|| "Variável do jogo não encontrada.".into())
}

/// The variable `text` names: its ID, or its name (in any case).
pub fn find(project: &Project, text: &str) -> Result<Id, String> {
    find_named(project, text)?.ok_or_else(|| {
        format!(
            "A variável do jogo \"{text}\" não existe no projeto. Variáveis: {}",
            summary(project)
        )
    })
}
/// [`find`], with `None` when no variable has that ID or name.
pub fn find_named(project: &Project, text: &str) -> Result<Option<Id>, String> {
    if get(project, text).is_some() {
        return Ok(Some(text.into()));
    }
    let wanted = text.trim().to_lowercase();
    let named: Vec<&GameVariable> = project
        .variables
        .iter()
        .filter(|v| v.name.trim().to_lowercase() == wanted)
        .collect();
    match named.as_slice() {
        [one] => Ok(Some(one.id.clone())),
        [] => Ok(None),
        // Validation keeps names apart; an old or hand-edited document may not.
        many => Err(format!(
            "Há {} variáveis do jogo chamadas \"{text}\"; use o ID: {}",
            many.len(),
            many.iter()
                .map(|v| v.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}
/// Up to 30 variable names, for messages.
fn summary(project: &Project) -> String {
    if project.variables.is_empty() {
        return "nenhuma".into();
    }
    let mut names: Vec<String> = project
        .variables
        .iter()
        .take(30)
        .map(|v| v.name.clone())
        .collect();
    if project.variables.len() > 30 {
        names.push(format!("e mais {}", project.variables.len() - 30));
    }
    names.join(", ")
}

/// Two variables with one name could not be told apart by name, and a name that is another
/// variable's ID would reach that variable, which [`find`] takes first.
fn check_name(project: &Project, name: &str, renamed: Option<&str>) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Informe o nome da variável do jogo".into());
    }
    if name.contains(KEY_SEPARATOR) {
        return Err(format!(
            "O nome da variável do jogo não pode ter {KEY_SEPARATOR}, que separa o nome da chave \
             nos relatórios"
        ));
    }
    let wanted = name.to_lowercase();
    for other in project
        .variables
        .iter()
        .filter(|v| Some(v.id.as_str()) != renamed)
    {
        if other.name.trim().to_lowercase() == wanted {
            return Err(format!(
                "Já existe uma variável do jogo chamada \"{}\"",
                other.name
            ));
        }
        if other.id.to_lowercase() == wanted {
            return Err(format!(
                "\"{name}\" é o identificador da variável do jogo {}; escolha outro nome",
                other.name
            ));
        }
    }
    Ok(())
}

pub fn create(
    project: &mut Project,
    name: &str,
    value: Value,
    persistent: bool,
) -> Result<Id, String> {
    if project.variables.len() >= MAX_VARIABLES {
        return Err(format!(
            "O projeto já tem {MAX_VARIABLES} variáveis do jogo, o limite"
        ));
    }
    check_name(project, name, None)?;
    check_value(&value).map_err(|error| format!("Variável do jogo {}: {error}", name.trim()))?;
    let id = new_id();
    project.variables.push(GameVariable {
        id: id.clone(),
        name: name.trim().into(),
        value,
        persistent,
    });
    Ok(id)
}
pub fn rename(project: &mut Project, id: &str, name: &str) -> Result<(), String> {
    get_mut(project, id)?;
    check_name(project, name, Some(id))?;
    get_mut(project, id)?.name = name.trim().into();
    Ok(())
}
/// Changes the initial value (and with it, the type) and whether the variable is kept between
/// runs; what is `None` stays.
pub fn update(
    project: &mut Project,
    id: &str,
    value: Option<Value>,
    persistent: Option<bool>,
) -> Result<(), String> {
    let variable = get_mut(project, id)?;
    if let Some(value) = value {
        check_value(&value)
            .map_err(|error| format!("Variável do jogo {}: {error}", variable.name))?;
        variable.value = value;
    }
    if let Some(persistent) = persistent {
        variable.persistent = persistent;
    }
    Ok(())
}

/// The logic nodes that name the variable `id`, in scenes and models, in each object's order.
pub fn uses(project: &Project, id: &str) -> Vec<Use> {
    let mut found = Vec::new();
    for (scope, entity) in every_entity(project) {
        for node in &entity.graph.nodes {
            if graph::node_references(node).any(|(param, reference, _)| {
                reference == Reference::Variable && node.text(param.id) == id
            }) {
                found.push(Use::new(scope, entity, &node.operation));
            }
        }
    }
    found
}
pub fn references(project: &Project, id: &str) -> Vec<String> {
    uses(project, id)
        .into_iter()
        .map(|found| {
            let label = graph::definition(&found.role).map_or(found.role.as_str(), |d| d.label);
            format!("{} → {} → nó {label}", found.scope, found.name)
        })
        .collect()
}
pub fn remove(project: &mut Project, id: &str) -> Result<(), String> {
    let name = get_mut(project, id)?.name.clone();
    let uses = references(project, id);
    if !uses.is_empty() {
        return Err(format!(
            "Altere estes nós antes de excluir a variável do jogo {name}:\n{}",
            uses.join("\n")
        ));
    }
    project.variables.retain(|v| v.id != id);
    Ok(())
}
