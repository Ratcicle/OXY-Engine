//! What the API's registry gives besides the VM (design §5.3, §5.7): the definitions file
//! luau-lsp reads, the plan-B module (`require("@oxy")`, unused by default), the data of the
//! Guide's pages and docs/api-scripts.md. All of it comes from [`registry::API`] and
//! [`registry::TYPES`], so a new row reaches every one of them; the reference also lists what
//! plain Luau does of the nodes ([`registry::SYNTAX`]).
//!
//! The definitions keep to the one form both of Luau's solvers read in luau-lsp 1.70.1 (S2): only
//! `export type`, `declare name: type` and `declare function`; objects and their views as table
//! types whose methods are function fields with an explicit `self`; `AnyObject` after
//! `ScriptObject`. Luau's own functions (`error`) and the names of 0.8 are not declared.
use super::registry::{API, ApiFn, ApiKind, ApiParam, Area, SYNTAX, TYPES, TypeDef, View};
use crate::graph;
use serde::Serialize;

/// How the types of one object type read: the object itself, its attributes and its memory.
struct Shape {
    object: &'static str,
    attr: &'static str,
    state: &'static str,
}

const PLAIN: Shape = Shape {
    object: "Object",
    attr: "{ [string]: any }",
    state: "any",
};

const TYPED: Shape = Shape {
    object: "ScriptObject<A, S>",
    attr: "A & {}",
    state: "S & {}",
};

/// `ty` with the registry's marks replaced for `shape`.
fn resolve(ty: &str, shape: &Shape) -> String {
    ty.replace("@self", shape.object)
        .replace("@attr", shape.attr)
        .replace("@state", shape.state)
}

/// A parameter as a function type lists it: `name: type`, or the rest as `...type` (a generic
/// pack as it is).
fn typed_param(param: &ApiParam, shape: &Shape) -> String {
    let ty = resolve(param.ty, shape);
    match param.name {
        "..." if ty.ends_with("...") => ty,
        "..." => format!("...{ty}"),
        name => format!("{name}: {ty}"),
    }
}

/// A parameter as a function's definition lists it: `name: type`, the rest as `...: type`.
fn defined_param(param: &ApiParam, shape: &Shape) -> String {
    let ty = resolve(param.ty, shape);
    match param.name {
        "..." => format!("...: {ty}"),
        name => format!("{name}: {ty}"),
    }
}

/// The type of a function or method: `<X...>(self: Object, seconds: number, …) -> number`; a
/// view's method takes the view (`self: Character`).
fn function_type(f: &ApiFn, shape: &Shape) -> String {
    let mut params: Vec<String> = Vec::new();
    if f.kind == ApiKind::Method {
        match f.view() {
            Some(View::Object) | None => params.push(format!("self: {}", shape.object)),
            Some(view) => params.push(format!("self: {}", view.name())),
        }
    }
    params.extend(f.params.iter().map(|p| typed_param(p, shape)));
    format!(
        "{}({}) -> {}",
        f.generics,
        params.join(", "),
        resolve(f.returns, shape)
    )
}

/// The `---` lines of help before a declaration, indented by `indent`.
fn help(out: &mut String, indent: &str, lines: &[&str]) {
    for line in lines.iter().filter(|line| !line.is_empty()) {
        out.push_str(&format!("{indent}--- {line}\n"));
    }
}

/// The members of `view` as a table type for `shape`.
fn view_body(out: &mut String, view: View, shape: &Shape) {
    for f in API.iter().filter(|f| f.view() == Some(view)) {
        help(out, "    ", &[f.summary, f.doc]);
        let ty = match f.kind {
            ApiKind::Method => function_type(f, shape),
            _ => resolve(f.returns, shape),
        };
        out.push_str(&format!("    {}: {ty},\n", f.member()));
    }
}

/// The shared types, as both the definitions and the module write them.
fn types(out: &mut String) {
    for t in TYPES {
        help(out, "", &[t.doc]);
        match t.def {
            TypeDef::Alias(def) => {
                out.push_str(&format!("export type {}{} = {def}\n\n", t.name, t.generics));
            }
            TypeDef::Table(fields) => {
                out.push_str(&format!("export type {}{} = {{\n", t.name, t.generics));
                for field in fields {
                    help(out, "    ", &[field.doc]);
                    out.push_str(&format!("    {}: {},\n", field.name, field.ty));
                }
                out.push_str("}\n\n");
            }
            TypeDef::Object { typed } => {
                out.push_str(&format!("export type {}{} = {{\n", t.name, t.generics));
                view_body(out, View::Object, if typed { &TYPED } else { &PLAIN });
                out.push_str("}\n\n");
            }
            TypeDef::View(view) => {
                out.push_str(&format!("export type {}{} = {{\n", t.name, t.generics));
                view_body(out, view, &PLAIN);
                out.push_str("}\n\n");
            }
        }
    }
}

/// The functions and the values of global tables the types declare: all but those luau-lsp
/// types itself (`require`), Luau's own and the names of 0.8.
fn declared_functions() -> impl Iterator<Item = &'static ApiFn> {
    API.iter()
        .filter(|f| matches!(f.kind, ApiKind::Function | ApiKind::Constant) && f.in_types())
}

/// A member of a global table as its type declares it: a function's type, a value's.
fn table_member(f: &ApiFn) -> String {
    match f.kind {
        ApiKind::Constant => resolve(f.returns, &PLAIN),
        _ => function_type(f, &PLAIN),
    }
}

/// The global tables, in the order of their first row, each with its functions and values.
fn tables() -> Vec<(&'static str, Vec<&'static ApiFn>)> {
    let mut tables: Vec<(&'static str, Vec<&'static ApiFn>)> = Vec::new();
    for f in declared_functions() {
        let holder = f.holder();
        if holder.is_empty() {
            continue;
        }
        match tables.iter_mut().find(|(name, _)| *name == holder) {
            Some((_, rows)) => rows.push(f),
            None => tables.push((holder, vec![f])),
        }
    }
    tables
}

/// The functions of the base the types declare (`print`, `warn`).
fn base_functions() -> impl Iterator<Item = &'static ApiFn> {
    declared_functions().filter(|f| f.kind == ApiKind::Function && f.holder().is_empty())
}

/// The comment that opens the definitions: the callbacks and the declarations, which are the
/// module's own functions and fields, not globals.
fn header(out: &mut String) {
    let line = |f: &ApiFn| match f.kind {
        ApiKind::Callback => {
            let mut names = vec!["self"];
            names.extend(f.params.iter().map(|p| p.name));
            format!("{}({})", f.name, names.join(", "))
        }
        _ => format!("Modulo.{}", f.name),
    };
    let width = API
        .iter()
        .filter(|f| matches!(f.kind, ApiKind::Callback | ApiKind::Declaration))
        .map(|f| line(f).chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    out.push_str("-- Eventos: funções do módulo com estes nomes, que a engine chama sozinha.\n");
    for f in API.iter().filter(|f| f.kind == ApiKind::Callback) {
        out.push_str(&format!("--   {:width$}{}\n", line(f), f.summary));
    }
    out.push_str("-- Campos do módulo que a engine lê ao carregar o script:\n");
    for f in API.iter().filter(|f| f.kind == ApiKind::Declaration) {
        out.push_str(&format!("--   {:width$}{}\n", line(f), f.summary));
    }
    out.push('\n');
}

/// The definitions file for luau-lsp (`--definitions=@oxy=…`), without the line with the
/// engine's version that `script_files` writes before it.
pub fn definitions() -> String {
    let mut out = String::new();
    header(&mut out);
    types(&mut out);
    for (name, rows) in tables() {
        out.push_str(&format!("declare {name}: {{\n"));
        for f in rows {
            help(&mut out, "    ", &[f.summary, f.doc]);
            out.push_str(&format!("    {}: {},\n", f.member(), table_member(f)));
        }
        out.push_str("}\n\n");
    }
    for f in base_functions() {
        help(&mut out, "", &[f.summary, f.doc]);
        let params: Vec<String> = f.params.iter().map(|p| defined_param(p, &PLAIN)).collect();
        out.push_str(&format!(
            "declare function {}{}({}): {}\n\n",
            f.name,
            f.generics,
            params.join(", "),
            f.returns
        ));
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// The plan-B module (design §5.3): the same types as a Luau module a script requires as
/// `require("@oxy")`, through an alias of `.luaurc`. Unused by default: the runtime does not
/// serve `@oxy`, and the definitions are the form OXY writes.
pub fn module() -> String {
    let mut out = String::from(
        "--!strict\n-- Os tipos da API dos scripts como módulo (plano B): gerado pela OXY da \
         tabela da API, como o\n-- oxy.d.luau. Uso: local oxy = require(\"@oxy\"); type Object \
         = oxy.Object; local audio = oxy.audio\n\n",
    );
    types(&mut out);
    out.push_str("--- A API dos scripts.\nexport type Oxy = {\n");
    for (name, rows) in tables() {
        out.push_str(&format!("    {name}: {{\n"));
        for f in rows {
            help(&mut out, "        ", &[f.summary, f.doc]);
            out.push_str(&format!("        {}: {},\n", f.member(), table_member(f)));
        }
        out.push_str("    },\n");
    }
    for f in base_functions() {
        help(&mut out, "    ", &[f.summary, f.doc]);
        out.push_str(&format!("    {}: {},\n", f.name, function_type(f, &PLAIN)));
    }
    out.push_str(
        "}\n\n-- A engine serve os valores; para o analisador basta o tipo.\nlocal oxy: Oxy = \
         (nil :: any)\n\nreturn oxy\n",
    );
    out
}

/// A row as the Guide and the reference write it: `audio.play(name: string, options:
/// PlayOptions?)`, `obj:after<X...>(…): number`, `obj.position: vector`,
/// `obj.character:jump()`, `on_start(self: Object)`, `Modulo.attributes: { [string]: any }`,
/// `time.dt: number`; a name of 0.8 as it will be used.
pub fn signature(f: &ApiFn) -> String {
    let params = |with_self: bool| {
        let mut params: Vec<String> = Vec::new();
        if with_self {
            params.push("self: Object".into());
        }
        params.extend(f.params.iter().map(|p| defined_param(p, &PLAIN)));
        params.join(", ")
    };
    let returns = match resolve(f.returns, &PLAIN) {
        unit if unit == "()" => String::new(),
        returns => format!(": {returns}"),
    };
    let path = f.view().map_or("obj", View::path);
    match f.kind {
        ApiKind::Function => format!("{}{}({}){returns}", f.name, f.generics, params(false)),
        ApiKind::Method => format!(
            "{path}:{}{}({}){returns}",
            f.member(),
            f.generics,
            params(false)
        ),
        ApiKind::Property { .. } => format!("{path}.{}{returns}", f.member()),
        ApiKind::Callback => format!("{}({})", f.name, params(true)),
        ApiKind::Declaration => format!("Modulo.{}{returns}", f.name),
        ApiKind::Constant => format!("{}{returns}", f.name),
        ApiKind::Reserved if f.holder() == "Modulo" => format!("{}({})", f.name, params(true)),
        ApiKind::Reserved => f.name.to_owned(),
    }
}

/// The nodes that do what `f` does, as the Guide names them: `Tocar som (action.sound)`.
fn nodes(f: &ApiFn) -> Vec<String> {
    f.nodes
        .iter()
        .map(|id| {
            let label = graph::registry()
                .iter()
                .find(|op| op.id == *id)
                .map_or(*id, |op| op.label);
            format!("{label} ({id})")
        })
        .collect()
}

/// A parameter on a page of the Guide.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GuideParam {
    pub name: &'static str,
    #[serde(rename = "type")]
    pub ty: String,
    pub doc: &'static str,
}

/// A row on a page of the Guide.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GuideEntry {
    pub name: &'static str,
    pub signature: String,
    /// Whether a script may assign it (properties).
    pub writable: bool,
    pub summary: &'static str,
    pub doc: &'static str,
    pub params: Vec<GuideParam>,
    pub errors: &'static [&'static str],
    pub example: &'static str,
    /// The equivalent nodes, by label and operation ID.
    pub nodes: Vec<String>,
    /// Where it runs in 0.8, as the Guide says it.
    pub domain: &'static str,
    pub since: &'static str,
}

/// One page of the Guide's section on scripts: an area of the API.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GuidePage {
    pub area: Area,
    pub title: &'static str,
    pub intro: &'static str,
    pub entries: Vec<GuideEntry>,
}

/// The pages of the Guide's section on scripts (stage 7 shows them), one per area with rows,
/// in the areas' order and the rows' order. `serde_json::to_value` gives them as JSON.
pub fn guide() -> Vec<GuidePage> {
    Area::ALL
        .into_iter()
        .filter_map(|area| {
            let entries: Vec<GuideEntry> = API
                .iter()
                .filter(|f| f.area == area)
                .map(|f| GuideEntry {
                    name: f.name,
                    signature: signature(f),
                    writable: f.kind == ApiKind::Property { write: true },
                    summary: f.summary,
                    doc: f.doc,
                    params: f
                        .params
                        .iter()
                        .map(|p| GuideParam {
                            name: p.name,
                            ty: resolve(p.ty, &PLAIN),
                            doc: p.doc,
                        })
                        .collect(),
                    errors: f.errors,
                    example: f.example,
                    nodes: nodes(f),
                    domain: f.domain.label(),
                    since: f.since,
                })
                .collect();
            (!entries.is_empty()).then(|| GuidePage {
                area,
                title: area.title(),
                intro: area.intro(),
                entries,
            })
        })
        .collect()
}

/// docs/api-scripts.md: the reference of the API, page by page as the Guide has it, and the
/// shared types.
pub fn markdown() -> String {
    let mut out = String::from(
        "# API dos scripts\n\n\
         <!-- Gerado da tabela da API (crates/oxy_core/src/script/registry.rs). Não edite: rode \
         os testes do oxy_core com OXY_UPDATE_API=1 para gravar de novo. -->\n\n\
         A referência das funções que os scripts em Luau alcançam, gerada da mesma tabela que \
         instala a API no jogo e que escreve os tipos do VS Code (`.oxy/tipos/oxy.d.luau`). Os \
         nomes da API são em inglês; os de objetos, atributos, sons, ações e variáveis são os do \
         projeto, e as mensagens de erro são em português, com o arquivo e a linha. Como \
         escrever, anexar e testar scripts está em [Agentes de IA \
         (MCP)](agentes-mcp.md#scripts-com-o-agente).\n\n\
         Cada exemplo usa um módulo chamado `M` e roda num teste da engine. **0.8** diz onde a \
         função vai rodar com vários jogadores.\n\n",
    );
    for page in guide() {
        out.push_str(&format!("## {}\n\n{}\n\n", page.title, page.intro));
        for entry in page.entries {
            out.push_str(&format!("### `{}`\n\n{}", entry.signature, entry.summary));
            if entry.writable {
                out.push_str(" Leitura e escrita.");
            }
            out.push_str("\n\n");
            if !entry.doc.is_empty() {
                out.push_str(&format!("{}\n\n", entry.doc));
            }
            for param in &entry.params {
                out.push_str(&format!(
                    "- `{}` (`{}`): {}\n",
                    param.name, param.ty, param.doc
                ));
            }
            if !entry.errors.is_empty() {
                out.push_str(&format!("- Para com erro: {}.\n", entry.errors.join("; ")));
            }
            if !entry.nodes.is_empty() {
                out.push_str(&format!("- Nós: {}.\n", entry.nodes.join(", ")));
            }
            out.push_str(&format!(
                "- 0.8: {}. Desde a {}.\n\n```lua\n{}\n```\n\n",
                entry.domain, entry.since, entry.example
            ));
        }
    }
    out.push_str(
        "## Nós que viram Luau\n\nO que estas operações dos nós fazem o próprio Luau faz, sem \
         uma função da API; as outras estão nas páginas acima, no item **Nós**.\n\n",
    );
    for (id, luau) in SYNTAX {
        let label = graph::registry()
            .iter()
            .find(|op| op.id == *id)
            .map_or(*id, |op| op.label);
        out.push_str(&format!("- {label} (`{id}`): {luau}\n"));
    }
    out.push_str("\n## Tipos\n\n```lua\n");
    let mut shared = String::new();
    types(&mut shared);
    out.push_str(shared.trim_end());
    out.push_str("\n```\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::load;
    use std::path::Path;

    /// The files made from the table that the repository keeps, from the crate's folder.
    fn references() -> [(&'static str, String); 2] {
        [
            ("src/script/oxy.d.luau", definitions()),
            ("../../docs/api-scripts.md", markdown()),
        ]
    }

    /// The definitions and the reference change only with the table: a change shows in review.
    /// `OXY_UPDATE_API=1` writes them again.
    #[test]
    fn the_reference_files_match_the_table() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let update = std::env::var_os("OXY_UPDATE_API").is_some();
        for (path, text) in references() {
            let file = root.join(path);
            if update {
                std::fs::write(&file, &text).unwrap();
                continue;
            }
            let kept = std::fs::read_to_string(&file)
                .unwrap_or_default()
                .replace("\r\n", "\n");
            assert!(
                kept == text,
                "{path} não é o que a tabela da API gera; rode os testes do oxy_core com \
                 OXY_UPDATE_API=1 para gravá-lo de novo e revise a diferença"
            );
        }
    }

    #[test]
    fn the_definitions_keep_to_the_syntax_both_solvers_read() {
        let text = definitions();
        for line in text
            .lines()
            .filter(|l| !l.starts_with(' ') && !l.starts_with("--"))
        {
            assert!(
                line.is_empty()
                    || line == "}"
                    || line.starts_with("export type ")
                    || line.starts_with("declare function ")
                    || (line.starts_with("declare ") && line.ends_with(": {")),
                "{line}"
            );
        }
        assert!(!text.contains("declare class") && !text.contains("declare extern"));
        assert!(!text.contains('@'), "uma marca da tabela ficou sem trocar");
        // AnyObject after ScriptObject: before it, the old solver refuses the recursion.
        let at = |name: &str| text.find(&format!("export type {name}")).unwrap();
        assert!(at("ScriptObject") < at("AnyObject"));
        // The typed object seals what the script declares, which luau-lsp then checks.
        assert!(text.contains("    attr: A & {},\n") && text.contains("    state: S & {},\n"));
        assert!(text.contains("    attr: { [string]: any },\n"));
        // Every row reaches the file: globals, methods and properties, callbacks in the header;
        // only require is luau-lsp's, which knows the type of the module it loads.
        assert!(!text.contains("function require("));
        for f in API.iter().filter(|f| f.in_types()) {
            let found = match f.kind {
                ApiKind::Function if f.holder().is_empty() => {
                    format!("declare function {}(", f.name)
                }
                ApiKind::Function
                | ApiKind::Method
                | ApiKind::Property { .. }
                | ApiKind::Constant => format!("    {}: ", f.member()),
                ApiKind::Callback => format!("--   {}(self", f.name),
                ApiKind::Declaration => format!("--   Modulo.{}", f.name),
                ApiKind::Reserved => unreachable!("not in the types"),
            };
            assert!(text.contains(&found), "{}: {found}", f.name);
        }
        // Luau's own functions and the names of 0.8 are left out.
        for name in ["function error(", "function assert(", "players", "online:"] {
            assert!(!text.contains(name), "{name}");
        }
        assert!(text.contains(
            "    after: <X...>(self: ScriptObject<A, S>, seconds: number, fn: (ScriptObject<A, \
             S>, X...) -> (), X...) -> number,"
        ));
        assert!(text.contains("declare function warn(...: any): ()"));
        assert!(text.contains("    object: () -> Object?,"));
        // Views are types of their own, whose methods take the view.
        assert!(text.contains("export type Character = {\n"));
        assert!(text.contains("    character: Character,\n"));
        assert!(text.contains("    jump: (self: Character) -> (),\n"));
        assert!(text.contains("    dt: number,\n"));
    }

    #[test]
    fn the_module_compiles_and_has_every_global() {
        let text = module();
        load::check("scripts/oxy.luau", &text).unwrap();
        assert!(
            ["@self", "@attr", "@state"]
                .iter()
                .all(|mark| !text.contains(mark))
        );
        for (name, rows) in tables() {
            assert!(text.contains(&format!("    {name}: {{\n")), "{name}");
            for f in rows {
                assert!(
                    text.contains(&format!("        {}: ", f.member())),
                    "{}",
                    f.name
                );
            }
        }
        for f in base_functions() {
            assert!(text.contains(&format!("    {}: (", f.name)), "{}", f.name);
        }
        assert!(text.ends_with("local oxy: Oxy = (nil :: any)\n\nreturn oxy\n"));
    }

    #[test]
    fn the_guide_has_a_page_per_area_with_every_row() {
        let pages = guide();
        let rows: usize = pages.iter().map(|page| page.entries.len()).sum();
        assert_eq!(rows, API.len());
        let areas: Vec<Area> = pages.iter().map(|page| page.area).collect();
        let mut sorted = areas.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(areas, sorted);
        let play = pages
            .iter()
            .flat_map(|page| &page.entries)
            .find(|entry| entry.name == "audio.play")
            .unwrap();
        assert_eq!(
            play.signature,
            "audio.play(name: string, options: PlayOptions?)"
        );
        assert_eq!(play.nodes, ["Reproduzir WAV (action.sound)"]);
        let json = serde_json::to_value(&pages).unwrap();
        assert_eq!(json[0]["area"], "events");
        assert_eq!(json[0]["entries"][0]["signature"], "on_start(self: Object)");
        let after = crate::script::registry::find("Object:after").unwrap();
        assert_eq!(
            signature(after),
            "obj:after<X...>(seconds: number, fn: (Object, X...) -> (), ...: X...): number"
        );
        assert_eq!(
            signature(crate::script::registry::find("Object.position").unwrap()),
            "obj.position: vector"
        );
        let find = |name| crate::script::registry::find(name).unwrap();
        assert_eq!(signature(find("Character:jump")), "obj.character:jump()");
        assert_eq!(signature(find("UiElement.text")), "obj.ui.text: string");
        assert_eq!(signature(find("time.dt")), "time.dt: number");
        assert_eq!(signature(find("players")), "players");
        assert_eq!(
            signature(find("on_player_join")),
            "on_player_join(self: Object, player: any)"
        );
    }
}
