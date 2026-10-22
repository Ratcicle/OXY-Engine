//! The lint of names (design §6.4): what a script names with a literal text, checked from its
//! bytecode without running it, so the tools warn of `audio.play("Checkpointt")` before the
//! game plays. It reads the global paths the script imports, checked against the globals scripts
//! have (`audio.paly`, with the runtime's own message), and the texts it gives where the
//! registry puts a name: a parameter of a [`NameKind`] (`audio.play("Bipe")`,
//! `self:get_child("Maçaneta")`), a key of `obj.attr`, a surface's name written to a property
//! (`obj.collider.surface = "Gelo"`) and a callback's parameter it compares (`action ==
//! "Pular"` in `on_input`), checked against the project by the rules the runtime
//! looks each one up with, and the texts of a parameter whose type is a union of texts
//! (`camera.set_mode(cam, "orbita")`), checked against the union. Every finding is a warning:
//! a name can be right where the lint cannot see it (a sound imported later), and a wrong one
//! still stops the call at run time, with the same name. Only texts written where they are used
//! count (constants of the module fold into them): a name built at run time or passed in a table
//! of options is not read. Tools read the same names ([`references`]) to find where a script
//! uses a sound, a model or a scene.
use super::bytecode::{Literal, Report, Site};
use super::errors::{ScriptError, closest};
use super::registry::{self, ApiFn, ApiKind, ApiParam, NameKind, View};
use super::{ScriptHost, ScriptSources, api, load, sandbox};
use crate::document::{AssetKind, Id, Project};
use crate::input_actions::{self, every_entity};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, LazyLock};

/// Names a message lists before it counts the rest.
const LISTED: usize = 10;
/// Other lines a finding names before it counts the rest.
const OTHER_LINES: usize = 5;
/// What a copy made during the game adds to the name of its root (`obj:clone`).
const COPY: &str = " (cópia)";

/// Texts a parameter takes that its type leaves out: the node's old name of a profile (design
/// §5.1), by row and parameter.
const ACCEPTED: [(&str, &str, &str); 1] = [("Character:apply_profile", "profile", "parkour")];

/// A name of the project a script gives with a literal text, where the API takes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub kind: NameKind,
    pub name: String,
    /// Its line, from 1; 0 when the bytecode carries none.
    pub line: u32,
    /// Where it goes, as scripts write it: `audio.play`, `obj:get_child`,
    /// `obj.animation:play`, `obj.attr`, `obj.collider.surface`, `on_input`.
    pub function: String,
    /// Whether the script compares it with what the engine gives (a callback's parameter),
    /// which matches only the exact name, rather than giving it to a function, which looks it
    /// up as the runtime does (most in any case).
    pub exact: bool,
}

/// What a [`Reference`] names in a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// The sound or the model (its asset), the scene, the game variable, the input action or
    /// the surface.
    Id(Id),
    /// A name objects have: an object's, an attribute's, a clip's or a marker's.
    Name,
}

/// A project's names as scripts give them: what [`ProjectNames::find`] looks a script's
/// references up in, with the rules the runtime looks each kind up with.
pub struct ProjectNames<'a> {
    project: &'a Project,
    /// The attributes objects have, in scenes and models, and those scripts declare.
    attributes: BTreeSet<String>,
    objects: BTreeSet<&'a str>,
    clips: BTreeSet<&'a str>,
    markers: BTreeSet<&'a str>,
}

impl<'a> ProjectNames<'a> {
    /// The names of `project`, with the attributes the scripts declare, `declared` (see
    /// [`declared_attribute_names`]): attaching a script gives them to its objects.
    pub fn new(project: &'a Project, declared: impl IntoIterator<Item = String>) -> Self {
        let mut names = Self {
            project,
            attributes: declared.into_iter().collect(),
            objects: BTreeSet::new(),
            clips: BTreeSet::new(),
            markers: BTreeSet::new(),
        };
        for (_, e) in every_entity(project) {
            names.objects.insert(&e.name);
            names.attributes.extend(e.attributes.keys().cloned());
            for clip in &e.clips {
                names.clips.insert(&clip.name);
                names
                    .markers
                    .extend(clip.events.iter().map(|marker| marker.name.as_str()));
            }
        }
        names
    }

    /// What `reference` names in the project: a sound, a model or a surface by its name, else
    /// in any case and without the spaces at its ends; a scene, a variable or an action also by
    /// its ID; an attribute, an object or a marker by the exact name (an object made by
    /// `obj:clone` adds " (cópia)" to it); a clip by its name in any case. A compared name
    /// matches only the name as it is (`on_input` gives an action's name, never its ID). None
    /// when the project has nothing of the name.
    pub fn find(&self, reference: &Reference) -> Option<Found> {
        let (name, exact) = (reference.name.as_str(), reference.exact);
        let p = self.project;
        let id = |found: Option<&Id>| found.cloned().map(Found::Id);
        match reference.kind {
            NameKind::Sound => id(by_name(assets(p, AssetKind::Audio), name, exact)),
            NameKind::Model => id(by_name(assets(p, AssetKind::Model), name, exact)),
            NameKind::Scene => {
                let scenes = p.scenes.iter().map(|s| (s.name.as_str(), &s.id));
                id(by_id(scenes.clone(), name).or_else(|| by_name(scenes, name, exact)))
            }
            NameKind::Variable => {
                let variables = p.variables.iter().map(|v| (v.name.as_str(), &v.id));
                id(by_id(variables.clone(), name).or_else(|| by_name(variables, name, exact)))
            }
            NameKind::Action => {
                let labels: Vec<(String, &Id)> = p
                    .input_bindings
                    .keys()
                    .map(|id| (input_actions::label(p, id).into_owned(), id))
                    .collect();
                let actions = labels.iter().map(|(label, id)| (label.as_str(), *id));
                // `on_input` gives the name, never the ID.
                let by_id = (!exact).then(|| by_id(actions.clone(), name)).flatten();
                id(by_id.or_else(|| by_name(actions, name, exact)))
            }
            NameKind::Surface => {
                let surfaces = p.surfaces.iter().map(|s| (s.name.as_str(), &s.id));
                id(by_name(surfaces, name, exact))
            }
            NameKind::Attribute => self.attributes.contains(name).then_some(Found::Name),
            NameKind::Object => {
                let mut name = name;
                while !self.objects.contains(name) {
                    name = name.strip_suffix(COPY)?;
                }
                Some(Found::Name)
            }
            NameKind::Clip => {
                let wanted = name.trim().to_lowercase();
                let known = self.clips.contains(name)
                    || (!exact && self.clips.iter().any(|c| c.trim().to_lowercase() == wanted));
                known.then_some(Found::Name)
            }
            NameKind::Marker => self.markers.contains(name).then_some(Found::Name),
        }
    }

    /// The names of `kind` the project has, for the nearest name and the lists.
    fn names(&self, kind: NameKind) -> Vec<String> {
        let p = self.project;
        let of = |kind| assets(p, kind).map(|(name, _)| name.to_owned()).collect();
        match kind {
            NameKind::Sound => of(AssetKind::Audio),
            NameKind::Model => of(AssetKind::Model),
            NameKind::Scene => p.scenes.iter().map(|s| s.name.clone()).collect(),
            NameKind::Variable => p.variables.iter().map(|v| v.name.clone()).collect(),
            NameKind::Action => p
                .input_bindings
                .keys()
                .map(|id| input_actions::label(p, id).into_owned())
                .collect(),
            NameKind::Surface => p.surfaces.iter().map(|s| s.name.clone()).collect(),
            NameKind::Attribute => self.attributes.iter().cloned().collect(),
            NameKind::Object => self.objects.iter().map(|&n| n.to_owned()).collect(),
            NameKind::Clip => self.clips.iter().map(|&n| n.to_owned()).collect(),
            NameKind::Marker => self.markers.iter().map(|&n| n.to_owned()).collect(),
        }
    }
}

/// The assets of `kind` of `project`, as (name, ID).
fn assets(project: &Project, kind: AssetKind) -> impl Iterator<Item = (&str, &Id)> + Clone {
    project
        .assets
        .iter()
        .filter(move |a| a.kind == kind)
        .map(|a| (a.name.as_str(), &a.id))
}

/// The ID of the item of `items` (name, ID) named `name`; unless `exact`, else in any case and
/// without the spaces at its ends.
fn by_name<'b>(
    mut items: impl Iterator<Item = (&'b str, &'b Id)> + Clone,
    name: &str,
    exact: bool,
) -> Option<&'b Id> {
    if let Some((_, id)) = items.clone().find(|(n, _)| *n == name) {
        return Some(id);
    }
    let wanted = name.trim().to_lowercase();
    items
        .find(|(n, _)| !exact && n.trim().to_lowercase() == wanted)
        .map(|(_, id)| id)
}

/// The item of `items` (name, ID) whose ID is `id`.
fn by_id<'b>(mut items: impl Iterator<Item = (&'b str, &'b Id)>, id: &str) -> Option<&'b Id> {
    items.find(|(_, item)| *item == id).map(|(_, item)| item)
}

/// The names of the attributes the scripts of `sources` declare in `Module.attributes`, which
/// attaching one gives its objects; a script that does not load declares none.
pub fn declared_attribute_names(sources: &ScriptSources) -> BTreeSet<String> {
    if sources.files.is_empty() {
        return BTreeSet::new();
    }
    let Ok(host) = ScriptHost::new(Arc::new(sources.clone())) else {
        return BTreeSet::new();
    };
    sources
        .files
        .keys()
        .filter_map(|path| host.load(path).ok())
        .filter_map(|module| module.attributes.clone().ok())
        .flatten()
        .map(|(name, _)| name)
        .collect()
}

/// Where a literal goes, by the registry.
enum Goes {
    /// A parameter of a row.
    Param(&'static ApiFn, &'static ApiParam),
    /// A property whose texts name something of the project ([`ApiFn::names`]): a key of
    /// `obj.attr`, or a name written to `obj.collider.surface`.
    Property(&'static ApiFn, NameKind),
}

/// The views of objects by the field that reaches them (`animation` for `obj.animation`).
fn view_of(field: &str) -> Option<View> {
    View::ALL
        .into_iter()
        .find(|view| view.path().strip_prefix("obj.") == Some(field))
}

/// Where `site` goes, when the registry takes a name or a choice there.
fn goes(site: &Site) -> Option<Goes> {
    let param = |f: &'static ApiFn, index: usize| Some(Goes::Param(f, f.params.get(index)?));
    match site {
        Site::Call { function, arg } => {
            let f = registry::find(function).filter(|f| f.kind == ApiKind::Function)?;
            param(f, *arg)
        }
        Site::Method { name, field, arg } => {
            let view = field.as_deref().and_then(view_of).unwrap_or(View::Object);
            let f = registry::find(&format!("{}:{name}", view.name()))
                .filter(|f| f.kind == ApiKind::Method)?;
            param(f, *arg)
        }
        Site::Key { field } => {
            let f = registry::API.iter().find(|f| f.is_attributes())?;
            (f.member() == field).then_some(Goes::Property(f, NameKind::Attribute))
        }
        Site::Set { field, key } => {
            let view = view_of(field)?;
            let f = registry::find(&format!("{}.{key}", view.name()))
                .filter(|f| f.kind == ApiKind::Property { write: true })?;
            Some(Goes::Property(f, *f.names().first()?))
        }
        Site::Compare {
            function,
            param: index,
        } => {
            let f = registry::find(function).filter(|f| f.kind == ApiKind::Callback)?;
            param(f, index.checked_sub(1)?)
        }
    }
}

/// How messages name where `f` takes it: `audio.play`, `obj:get_child`, `obj.animation:play`,
/// `obj.attr`, `obj.collider.surface`, `on_input`.
fn label(f: &ApiFn) -> String {
    match (f.kind, f.view()) {
        (ApiKind::Method, Some(view)) => format!("{}:{}", view.path(), f.member()),
        (ApiKind::Property { .. }, Some(view)) => format!("{}.{}", view.path(), f.member()),
        _ => f.name.to_owned(),
    }
}

/// The names of the project `literals` give, in the order of their lines.
pub(crate) fn references_in(literals: &[Literal]) -> Vec<Reference> {
    let mut found: Vec<Reference> = literals
        .iter()
        .filter_map(|literal| {
            let (f, kind) = match goes(&literal.site)? {
                Goes::Param(f, param) => (f, param.names?),
                Goes::Property(f, kind) => (f, kind),
            };
            Some(Reference {
                kind,
                name: literal.text.clone(),
                line: literal.line.unwrap_or(0),
                function: label(f),
                exact: f.kind == ApiKind::Callback,
            })
        })
        .collect();
    found.sort_by_key(|r| r.line);
    found
}

/// The names of the project the script `path` gives with literal texts (see [`Reference`]), in
/// the order of their lines; why it does not compile, otherwise.
pub fn references(path: &str, source: &str) -> Result<Vec<Reference>, ScriptError> {
    let (_, report) = load::check(path, source)?;
    Ok(references_in(&report.literals))
}

/// The globals scripts have, each with the names its table has, as the sandbox builds them.
static GLOBALS: LazyLock<BTreeMap<String, Option<BTreeSet<String>>>> = LazyLock::new(|| {
    ScriptHost::new(Arc::default())
        .ok()
        .and_then(|host| host.globals().ok())
        .unwrap_or_default()
});

/// A finding before it is told: its line, what tells it apart from the others (one finding per
/// name, at its first line) and its message.
struct Finding {
    line: u32,
    key: String,
    message: String,
}

/// Why the import `path` reads nothing: a global or a member of a table scripts lack.
fn import_finding(path: &str) -> Option<String> {
    let globals = &*GLOBALS;
    let mut parts = path.split('.');
    let root = parts.next()?;
    let Some(members) = globals.get(root) else {
        return Some(sandbox::unknown_global(
            root,
            globals.keys().map(String::as_str),
        ));
    };
    let (Some(members), Some(member)) = (members, parts.next()) else {
        return None;
    };
    (!members.contains(member))
        .then(|| api::unknown_member(root, member, members.iter().map(String::as_str)))
}

/// `"a"`, `"a" ou "b"`, `"a", "b" ou "c"`.
fn or_list(choices: &[&str]) -> String {
    let quoted: Vec<String> = choices.iter().map(|c| format!("\"{c}\"")).collect();
    match quoted.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [first @ .., last] => format!("{} ou {last}", first.join(", ")),
    }
}

/// The names of a list, the first [`LISTED`] and how many more.
fn listed(names: &[String]) -> String {
    let mut shown: Vec<&str> = names.iter().take(LISTED).map(String::as_str).collect();
    let more = format!("e mais {}", names.len().saturating_sub(LISTED));
    if names.len() > LISTED {
        shown.push(&more);
    }
    shown.join(", ")
}

/// A choice `literal` gives a parameter whose type is a union of texts that leaves it out.
fn choice_finding(literal: &Literal) -> Option<Finding> {
    let Goes::Param(f, param) = goes(&literal.site)? else {
        return None;
    };
    let choices = param.choices()?;
    let text = literal.text.as_str();
    if choices.contains(&text) || ACCEPTED.contains(&(f.name, param.name, text)) {
        return None;
    }
    let hint = closest(text, choices.iter().copied()).map_or_else(
        || format!("; use {}", or_list(&choices)),
        |near| format!("; quis dizer \"{near}\"?"),
    );
    let verb = if f.kind == ApiKind::Callback {
        "nunca é"
    } else {
        "não aceita"
    };
    Some(Finding {
        line: literal.line.unwrap_or(0),
        key: format!("{}.{}={text}", f.name, param.name),
        message: format!("{}: {} {verb} \"{text}\"{hint}", label(f), param.name),
    })
}

/// What a message says the project lacks of `kind`, and what it says when it has none.
fn lacking(kind: NameKind) -> (&'static str, &'static str) {
    match kind {
        NameKind::Sound => (
            "Sons",
            "o projeto não tem sons (importe um WAV na Biblioteca)",
        ),
        NameKind::Variable => (
            "Variáveis",
            "o projeto não tem variáveis do jogo (crie em Lógica > Variáveis do jogo)",
        ),
        NameKind::Attribute => ("Atributos", "nenhum objeto do projeto tem atributos"),
        NameKind::Scene => ("Cenas", "o projeto não tem cenas"),
        NameKind::Model => (
            "Modelos",
            "o projeto não tem modelos (salve um objeto como modelo na Biblioteca)",
        ),
        NameKind::Action => (
            "Ações",
            "o projeto não tem ações de entrada (crie em Lógica > Ações de entrada)",
        ),
        NameKind::Clip => ("Clipes", "nenhum objeto do projeto tem clipes"),
        NameKind::Marker => ("Marcadores", "os clipes do projeto não têm marcadores"),
        NameKind::Object => ("Objetos", "o projeto não tem objetos"),
        NameKind::Surface => ("Superfícies", "o projeto não tem superfícies"),
    }
}

/// `reference`, which `names` lacks, as a finding: what is missing, with the nearest name or
/// the names there are.
fn missing_finding(reference: &Reference, names: &ProjectNames) -> Finding {
    let name = reference.name.as_str();
    let missing = match reference.kind {
        NameKind::Attribute => {
            format!("nenhum objeto do projeto tem o atributo \"{name}\" e nenhum script o declara")
        }
        NameKind::Object => format!("nenhum objeto do projeto se chama \"{name}\""),
        NameKind::Clip => format!("nenhum objeto do projeto tem o clipe \"{name}\""),
        NameKind::Marker => format!("nenhum clipe do projeto tem o marcador \"{name}\""),
        kind => format!("{} \"{name}\" não existe no projeto", kind.what()),
    };
    let known = names.names(reference.kind);
    let (plural, none) = lacking(reference.kind);
    let hint = match closest(name, known.iter().map(String::as_str)) {
        // A compared name differs only in case or spaces from the one the engine gives.
        Some(near)
            if reference.exact && near.trim().to_lowercase() == name.trim().to_lowercase() =>
        {
            format!(
                "; quis dizer {near}? A comparação é com o nome exato, com maiúsculas e espaços"
            )
        }
        Some(near) => format!("; quis dizer {near}?"),
        None if known.is_empty() => format!(": {none}"),
        None => format!(". {plural}: {}", listed(&known)),
    };
    Finding {
        line: reference.line,
        key: format!("{:?}={name}", reference.kind),
        message: format!("{}: {missing}{hint}", reference.function),
    }
}

/// The findings of the lint in the bytecode `report` of one script: its imports, its choices
/// and, with `names`, its names of the project.
fn findings(report: &Report, names: Option<&ProjectNames>) -> Vec<Finding> {
    let mut found: Vec<Finding> = Vec::new();
    for path in &report.imports {
        let Some(message) = import_finding(path) else {
            continue;
        };
        // One per line it is read at, which `told` gathers; line 0 without lines.
        let lines: Vec<u32> = match report.import_lines.get(path) {
            Some(lines) => lines.iter().copied().collect(),
            None => vec![0],
        };
        found.extend(lines.into_iter().map(|line| Finding {
            line,
            key: format!("import={path}"),
            message: message.clone(),
        }));
    }
    found.extend(report.literals.iter().filter_map(choice_finding));
    if let Some(names) = names {
        found.extend(
            references_in(&report.literals)
                .iter()
                .filter(|reference| names.find(reference).is_none())
                .map(|reference| missing_finding(reference, names)),
        );
    }
    found
}

/// `found`, one warning of `path` per finding's key, at its first line, naming the other lines.
fn told(path: &str, mut found: Vec<Finding>) -> Vec<ScriptError> {
    found.sort_by_key(|finding| finding.line);
    let mut keys: Vec<&str> = Vec::new();
    let mut lines: Vec<Vec<u32>> = Vec::new();
    let mut first: Vec<&Finding> = Vec::new();
    for finding in &found {
        match keys.iter().position(|key| *key == finding.key) {
            Some(at) => {
                if !lines[at].contains(&finding.line) {
                    lines[at].push(finding.line);
                }
            }
            None => {
                keys.push(&finding.key);
                lines.push(vec![finding.line]);
                first.push(finding);
            }
        }
    }
    first
        .into_iter()
        .zip(lines)
        .map(|(finding, lines)| {
            let others: Vec<String> = lines
                .iter()
                .skip(1)
                .filter(|&&line| line > 0)
                .map(u32::to_string)
                .collect();
            let also = match others.as_slice() {
                [] => String::new(),
                [one] => format!(" (também na linha {one})"),
                many if many.len() > OTHER_LINES => format!(
                    " (também nas linhas {} e mais {})",
                    many[..OTHER_LINES].join(", "),
                    many.len() - OTHER_LINES
                ),
                [first @ .., last] => format!(" (também nas linhas {} e {last})", first.join(", ")),
            };
            ScriptError::new(path, finding.line, format!("{}{also}", finding.message))
        })
        .collect()
}

/// The lint of names of the scripts `paths` of `sources`, as warnings in the order of the paths
/// and the lines: the global paths each imports that scripts lack, the texts it gives a
/// parameter whose type is a union of texts that leaves them out and, with `project`, the names
/// of the project it gives that the project lacks (attributes the scripts of `sources` declare
/// count as the project's). Each name is told once, at its first line, with the others. A
/// script that does not compile, or that `sources` lacks, gets none: the check says why.
pub fn lint<'a>(
    sources: &ScriptSources,
    paths: impl IntoIterator<Item = &'a str>,
    project: Option<&Project>,
) -> Vec<ScriptError> {
    let reports: Vec<(&str, Report)> = paths
        .into_iter()
        .filter_map(|path| {
            let (_, report) = load::check(path, &sources.get(path)?.text).ok()?;
            Some((path, report))
        })
        .collect();
    if reports.is_empty() {
        return Vec::new();
    }
    let names =
        project.map(|project| ProjectNames::new(project, declared_attribute_names(sources)));
    reports
        .iter()
        .flat_map(|(path, report)| told(path, findings(report, names.as_ref())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{AnimationEvent, Clip as AnimationClip};
    use crate::document::{Asset, Entity, Value};

    const PATH: &str = "scripts/teste.luau";

    type Named = (NameKind, String, u32, String, bool);

    fn names_in(source: &str) -> Vec<Named> {
        references(PATH, source)
            .unwrap()
            .into_iter()
            .map(|r| (r.kind, r.name, r.line, r.function, r.exact))
            .collect()
    }

    fn warnings(source: &str, project: Option<&Project>) -> Vec<String> {
        let sources = ScriptSources::from_texts([(PATH, source)]);
        lint(&sources, [PATH], project)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// The texts a script gives where the registry takes names: calls of global functions
    /// (through a local too), methods of objects and of their views, keys of `obj.attr`,
    /// surfaces written to the properties that take them and what a callback compares its
    /// parameters with, each at its line.
    #[test]
    fn literal_texts_are_read_where_the_registry_takes_names() {
        let source = r#"local M = {}
M.attributes = { Piso = param.surface("Gelo") }
local SOM = "Bipe"
function M.on_start(self)
    audio.play(SOM, { volume = 0.5 })
    vars.set("Pontos", vars.get("Pontos") + 1)
    self.attr.Vida -= 1
    self.attr["Vida máxima"] = 2
    scene.get("Porta"):get_child("Maçaneta").visible = false
    self.animation:play("Abrir")
    self.attr.Alvo:find_child("Luz", true)
    if input.pressed("Pular") then scene.change("Menu") end
    local s = scene
    s.spawn_model("Bala")
    self.collider.surface = "Gelo"
    scene.get("Herói").character.body_surface = "Lama"
end
function M.on_input(self, action, phase)
    if action == "Pular" and phase == "pressed" then
        print("pulou")
    end
end
function M.on_marker(self, marker, clip)
    if clip ~= "Abrir" then return end
    print(marker)
end
return M
"#;
        use NameKind::*;
        let given = |kind, name: &str, line, function: &str| -> Named {
            (kind, name.to_owned(), line, function.to_owned(), false)
        };
        let compared = |kind, name: &str, line, function: &str| -> Named {
            (kind, name.to_owned(), line, function.to_owned(), true)
        };
        assert_eq!(
            names_in(source),
            [
                given(Surface, "Gelo", 2, "param.surface"),
                given(Sound, "Bipe", 5, "audio.play"),
                given(Variable, "Pontos", 6, "vars.get"),
                given(Variable, "Pontos", 6, "vars.set"),
                given(Attribute, "Vida", 7, "obj.attr"),
                given(Attribute, "Vida", 7, "obj.attr"),
                given(Attribute, "Vida máxima", 8, "obj.attr"),
                given(Object, "Porta", 9, "scene.get"),
                given(Object, "Maçaneta", 9, "obj:get_child"),
                given(Clip, "Abrir", 10, "obj.animation:play"),
                given(Attribute, "Alvo", 11, "obj.attr"),
                given(Object, "Luz", 11, "obj:find_child"),
                given(Action, "Pular", 12, "input.pressed"),
                given(Scene, "Menu", 12, "scene.change"),
                given(Model, "Bala", 14, "scene.spawn_model"),
                given(Surface, "Gelo", 15, "obj.collider.surface"),
                given(Object, "Herói", 16, "scene.get"),
                given(Surface, "Lama", 16, "obj.character.body_surface"),
                compared(Action, "Pular", 19, "on_input"),
                compared(Clip, "Abrir", 24, "on_marker"),
            ]
        );
    }

    /// Only a text loaded on the way to its use counts: one either branch of an `if` may give,
    /// one built at run time, one in a table of options, a method of something that is no
    /// object and a parameter changed before the comparison are left alone, as is a text
    /// written to a property that takes no name.
    #[test]
    fn texts_the_bytecode_cannot_vouch_for_are_left_alone() {
        let source = r#"local M = {}
local Tabela = {}
function Tabela.get_child(self, nome) return nome end
function M.on_start(self)
    audio.play(if self.attr.Vida > 0 then "Bipe" else "Fim")
    audio.play("Som " .. tostring(1))
    audio.stop({ sound = "Musica" })
    scene:find("Porta")
    self.ui.text = "Gelo"
end
function M.on_input(self, action, phase)
    action = string.lower(action)
    if action == "pular" then print(phase) end
end
return M
"#;
        assert_eq!(
            names_in(source),
            [(
                NameKind::Attribute,
                "Vida".to_owned(),
                5,
                "obj.attr".to_owned(),
                false
            )]
        );
    }

    /// The globals and the members a script reads are checked against those scripts have,
    /// with the runtime's own messages, once with their other lines; a text outside a
    /// parameter's union of texts is warned of, an accepted old name excepted; without a
    /// project, nothing else is.
    #[test]
    fn api_names_and_choices_are_checked_without_a_project() {
        let source = r#"local M = {}
function M.on_start(self)
    audio.paly("Bipe")
    prnt("oi")
    print(math.flor(1.5))
    print(game.online)
    camera.set_mode(self, "orbita")
    camera.set_mode(self, "fixd")
    self.character:apply_profile("parkour")
    audio.play("Som que falta")
end
function M.on_input(self, action, phase)
    if phase == "presed" then print(action) end
    audio.paly("Bipe")
end
return M
"#;
        assert_eq!(
            warnings(source, None),
            [
                "scripts/teste.luau:3: audio não tem paly; quis dizer play? (também na linha 14)",
                "scripts/teste.luau:4: variável global desconhecida: prnt; quis dizer print?",
                "scripts/teste.luau:5: math não tem flor; quis dizer floor?",
                "scripts/teste.luau:6: game.online chega na 0.8, com a rede e vários jogadores; \
                 na 0.7 ainda não existe",
                "scripts/teste.luau:7: camera.set_mode: mode não aceita \"orbita\"; use \
                 \"first_person\", \"third_person\", \"fixed\" ou \"fixed_object\"",
                "scripts/teste.luau:8: camera.set_mode: mode não aceita \"fixd\"; quis dizer \
                 \"fixed\"?",
                "scripts/teste.luau:13: on_input: phase nunca é \"presed\"; quis dizer \
                 \"pressed\"?",
            ]
        );
    }

    /// The names follow the runtime's rules: sounds, models, scenes and surfaces in any case,
    /// scenes, variables and actions by their IDs too, attributes, objects and markers as they
    /// are (a copy's " (cópia)" included), clips in any case, and a compared name as it is.
    #[test]
    fn project_names_are_found_by_the_runtimes_rules() {
        let mut project = Project::new("Nomes");
        project.assets.push(Asset {
            id: "som".into(),
            name: "Checkpoint".into(),
            path: "assets/checkpoint.wav".into(),
            kind: AssetKind::Audio,
            model: None,
        });
        let mut door = Entity::new("Porta", None);
        door.attributes.insert("Vida".into(), Value::Number(1.));
        let mut clip = AnimationClip::new("Abrir");
        clip.events.push(AnimationEvent {
            time: 0.5,
            name: "Meio".into(),
        });
        door.clips.push(clip);
        project.scenes[0].entities.push(door);
        let scene = project.scenes[0].id.clone();
        let names = ProjectNames::new(&project, ["Ordem".to_owned()]);
        let found = |kind, name: &str, exact| {
            names.find(&Reference {
                kind,
                name: name.into(),
                line: 1,
                function: String::new(),
                exact,
            })
        };
        use NameKind::*;
        assert_eq!(
            found(Sound, " checkpoint", false),
            Some(Found::Id("som".into()))
        );
        assert_eq!(found(Sound, "Checkpointt", false), None);
        assert_eq!(found(Attribute, "Vida", false), Some(Found::Name));
        assert_eq!(found(Attribute, "vida", false), None);
        assert_eq!(found(Attribute, "Ordem", false), Some(Found::Name));
        assert_eq!(
            found(Object, "Porta (cópia) (cópia)", false),
            Some(Found::Name)
        );
        assert_eq!(found(Object, "porta", false), None);
        assert_eq!(found(Clip, "abrir", false), Some(Found::Name));
        assert_eq!(found(Clip, "abrir", true), None);
        assert_eq!(found(Marker, "Meio", true), Some(Found::Name));
        assert_eq!(found(Scene, &scene, false), Some(Found::Id(scene.clone())));
        // An action by its ID in a call; compared, only by its name.
        assert!(found(Action, "pular", false).is_some());
        assert_eq!(found(Action, "pular", true), None);
        assert!(found(Action, "Pular", true).is_some());
    }

    /// A name is told once, at its first line, with the other lines it comes at.
    #[test]
    fn each_name_is_told_once_with_its_other_lines() {
        let mut source = String::from("local M = {}\nfunction M.on_start(self)\n");
        for _ in 0..8 {
            source.push_str("    audio.play(\"Bipe\")\n");
        }
        source.push_str("    audio.loop(\"Bipe\")\nend\nreturn M\n");
        assert_eq!(
            warnings(&source, Some(&Project::new("Sem sons"))),
            [
                "scripts/teste.luau:3: audio.play: o som \"Bipe\" não existe no projeto: o \
                 projeto não tem sons (importe um WAV na Biblioteca) (também nas linhas 4, 5, 6, \
                 7, 8 e mais 3)"
            ]
        );
    }
}
