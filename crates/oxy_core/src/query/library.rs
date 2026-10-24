//! The project's Library as agents read it (oxy_project, oxy_scripts list): the folders of its
//! tree, what each holds (the assets and scripts its files are, with how each stands on the
//! disk) and which objects run each script. The MCP server on files and the editor's live
//! channel both answer through these, from the project's folder, so the two read alike.
use super::{Page, model_parts, project_summary};
use crate::{
    command::ItemKind,
    document::{Layout, Project},
    input_actions::{Use, every_entity},
    names,
    persistence::{self, AssetState, Checked},
    project_tree::{FileKind, Tree},
    script,
};
use serde_json::{Map, Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Uses a list names before it counts the rest.
const USES_LISTED: usize = 10;

/// Why a read of the project's folders waits for the project to have one.
const NO_ROOT: &str = "O projeto ainda não foi salvo numa pasta, e não tem pastas para listar";

/// The summary request with the project's folder `root` (none for a project never saved):
/// one model's parts with `model`, what the folder `folder` holds with `folder` (see
/// [`folder`]), else the summary (see [`project_summary`], with the project's layout) with
/// whether an old project waits to be organized, how each asset stands on the disk (`state` when
/// not fine, see [`add_state`]), its folders (paged by `offset` and `limit`) and its scripts with
/// the objects that attach them. `unsaved` says which images have pixels the next save writes.
pub fn summary_in(
    project: &Project,
    root: Option<&Path>,
    args: &Json,
    unsaved: &dyn Fn(&str) -> bool,
) -> Result<Json, String> {
    let query = super::ProjectQuery::parse(args)?;
    if let Some(model) = &query.model {
        return model_parts(project, model, query.page);
    }
    if let Some(wanted) = &query.folder {
        return folder(
            project,
            root.ok_or(NO_ROOT)?,
            wanted,
            query.kind,
            query.page,
            unsaved,
        );
    }
    let mut answer = project_summary(project);
    if project.layout == Layout::Legacy && root.is_some() {
        // The next save in the editor, or an agent's next write, organizes it in folders.
        answer["conversion_pending"] = json!(true);
    }
    let Some(root) = root else {
        return Ok(answer);
    };
    let checked = persistence::check_assets(project, root, unsaved);
    if let Some(assets) = answer["assets"].as_array_mut() {
        for (entry, asset) in assets.iter_mut().zip(&project.assets) {
            if let Some(checked) = checked.get(&asset.id) {
                add_state(entry, checked);
            }
        }
    }
    let (sources, tree) = match persistence::load_scripts_in_tree(root) {
        Ok(read) => read,
        Err(error) => {
            answer["tree_error"] = json!(error.to_string());
            return Ok(answer);
        }
    };
    // What the scripts name with literal texts counts as a use: a sound only a script plays.
    let cited = script::citations(project, &sources);
    for list in ["assets", "input_actions", "surfaces", "variables"] {
        for entry in answer[list].as_array_mut().into_iter().flatten() {
            let id = entry["id"].as_str().unwrap_or_default();
            if let Some(citations) = cited.get(id) {
                add_citations(entry, citations);
            }
        }
    }
    if project.layout == Layout::Tree {
        let folders: Vec<Json> = tree
            .folders
            .iter()
            .map(|path| json!({"path": path, "items": children(&tree, path)}))
            .collect();
        let (folders, about) = query.page.cut(folders);
        answer["folders"] = json!(folders);
        answer["folders_total"] = about["total"].clone();
        if let Some(next) = about.get("next_offset") {
            answer["folders_next_offset"] = next.clone();
        }
    }
    answer["scripts"] = tree
        .scripts()
        .map(|file| {
            let mut entry = json!({"path": file.path});
            uses_json(&mut entry, script_uses(project, &file.path));
            entry
        })
        .collect();
    Ok(answer)
}

/// What oxy_status says of the scripts of `project`, saved in the folder `root`: `files`, the
/// `.luau` files of its tree; `objects`, how many objects attach one; `errors`, what keeps them
/// from running as oxy_scripts check finds it (a script that does not load, a file an object
/// attaches that the tree lacks or cannot read), the first [`USES_LISTED`] with
/// `errors_total` counting them when there are more; and `checker`. A tree that cannot be
/// walked answers `error`.
pub fn scripts_status(project: &Project, root: &Path) -> Json {
    let sources = match persistence::load_scripts(root) {
        Ok(sources) => sources,
        Err(error) => return json!({"error": error.to_string()}),
    };
    let attached = script::attached_paths(project);
    let objects = every_entity(project)
        .filter(|(_, e)| !e.scripts.is_empty())
        .count();
    let reports = script::check_loading(&sources, sources.files.keys().map(String::as_str), |p| {
        attached.contains(p)
    });
    let mut errors: Vec<String> = script::absent_by_file(project, &sources)
        .into_iter()
        .map(|(line, _)| line)
        .collect();
    errors.extend(
        reports
            .iter()
            .filter_map(|report| report.error.as_ref().map(ToString::to_string)),
    );
    let mut answer = json!({
        "files": sources.files.len() + sources.unreadable.len(),
        "objects": objects,
        "checker": {"name": "oxy", "version": crate::export::engine_version()},
    });
    if errors.len() > USES_LISTED {
        answer["errors_total"] = json!(errors.len());
        errors.truncate(USES_LISTED);
    }
    if !errors.is_empty() {
        answer["errors"] = json!(errors);
    }
    answer
}

/// How the summary names a layout.
pub fn layout_name(layout: Layout) -> &'static str {
    match layout {
        Layout::Legacy => "legacy",
        Layout::Tree => "tree",
    }
}

/// Adds to an asset's entry how it stands, when not fine: `state` (missing, unreadable,
/// invalid_path, error, or unsaved: its file is made by the next save) and, but for a missing
/// file, `problem`, why.
fn add_state(entry: &mut Json, checked: &Checked) {
    let (state, problem) = match checked {
        Checked::Unsaved => ("unsaved", None),
        Checked::Problem(state, _) => match state {
            AssetState::Missing => ("missing", None),
            AssetState::Unreadable(why) => ("unreadable", Some(why)),
            AssetState::InvalidPath(why) => ("invalid_path", Some(why)),
            AssetState::Broken(why) => ("error", Some(why)),
        },
    };
    entry["state"] = json!(state);
    if let Some(problem) = problem {
        entry["problem"] = json!(problem);
    }
}

/// `used_by` with the first uses, plus `used_by_total` when some are left out.
fn uses_json(entry: &mut Json, uses: Vec<Use>) {
    if uses.len() > USES_LISTED {
        entry["used_by_total"] = json!(uses.len());
    }
    entry["used_by"] = json!(uses.into_iter().take(USES_LISTED).collect::<Vec<_>>());
}

/// Adds to an entry's `used_by` (see [`uses_json`]) the script lines `citations` that name it,
/// `{script, line, role}` after the objects, `used_by_total` counting both when they pass
/// [`USES_LISTED`].
fn add_citations(entry: &mut Json, citations: &[script::Citation]) {
    let Some(listed) = entry["used_by"].as_array().map(Vec::len) else {
        return;
    };
    let uses = entry["used_by_total"]
        .as_u64()
        .map_or(listed, |total| total as usize);
    let total = uses + citations.len();
    if let Some(list) = entry["used_by"].as_array_mut() {
        list.extend(
            citations
                .iter()
                .take(USES_LISTED.saturating_sub(listed))
                .map(|citation| json!(citation)),
        );
    }
    if total > USES_LISTED {
        entry["used_by_total"] = json!(total);
    }
}

/// The objects of `project` that attach the script `path`, in scenes and models.
fn script_uses(project: &Project, path: &str) -> Vec<Use> {
    every_entity(project)
        .filter(|(_, entity)| entity.scripts.iter().any(|s| s.path == path))
        .map(|(scope, entity)| Use::new(scope, entity, "script"))
        .collect()
}

/// The folder `path` is in ("" at the root).
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// How many folders and files the Library shows the folder `folder` of `tree` holds directly.
fn children(tree: &Tree, folder: &str) -> usize {
    let folders = tree.folders.iter().filter(|f| parent(f) == folder).count();
    let files = tree
        .files
        .iter()
        .filter(|f| f.kind != FileKind::Other && parent(&f.path) == folder)
        .count();
    folders + files
}

/// What the folder `wanted` (its path from the project's folder in any case; "" is the root)
/// of the project saved at `root` holds, as the Library lists it: its folders first, then its
/// images, sounds, models and scripts, in tree order, each `{kind, name, path}` (a folder with
/// `items`, how many it holds; an asset with its `id`, `state` when not fine and
/// `used_by_count`; a script with `used_by_count`), an asset whose file is missing in the folder
/// of its path; `kind` keeps the files of one kind alone. Paged by `page`; `other_files` counts
/// the files OXY does not use, which the Library leaves out.
pub fn folder(
    project: &Project,
    root: &Path,
    wanted: &str,
    kind: Option<ItemKind>,
    page: Page,
    unsaved: &dyn Fn(&str) -> bool,
) -> Result<Json, String> {
    let (sources, tree) = persistence::load_scripts_in_tree(root).map_err(|e| e.to_string())?;
    let cited = script::citations(project, &sources);
    let text = wanted.trim().replace('\\', "/");
    let text = text.trim_matches('/');
    let path = if text.is_empty() {
        String::new()
    } else {
        let mut alike = tree.folders.iter().filter(|f| names::same_name(f, text));
        match (alike.next(), alike.next()) {
            (Some(found), None) => found.clone(),
            _ => {
                let mut listed: Vec<&str> = tree.folders.iter().map(String::as_str).collect();
                let more = listed.len().saturating_sub(USES_LISTED);
                listed.truncate(USES_LISTED);
                let listed = match (listed.is_empty(), more) {
                    (true, _) => "o projeto não tem pastas".to_owned(),
                    (false, 0) => format!("pastas: {}", listed.join(", ")),
                    (false, more) => format!("pastas: {} e mais {more}", listed.join(", ")),
                };
                return Err(format!("A pasta {text} não existe no projeto; {listed}"));
            }
        }
    };
    let checked = persistence::check_assets(project, root, unsaved);
    let mut items: Vec<Json> = Vec::new();
    if kind.is_none() {
        for sub in tree.folders.iter().filter(|f| parent(f) == path) {
            items.push(json!({
                "kind": "Folder",
                "name": sub.rsplit('/').next().unwrap_or(sub),
                "path": sub,
                "items": children(&tree, sub),
            }));
        }
    }
    let mut other_files = 0;
    let mut listed: BTreeSet<&str> = BTreeSet::new();
    for file in tree.files.iter().filter(|f| parent(&f.path) == path) {
        let file_kind = match file.kind {
            FileKind::Image => ItemKind::Texture,
            FileKind::Sound => ItemKind::Audio,
            FileKind::Model => ItemKind::Model,
            FileKind::Script => ItemKind::Script,
            FileKind::Other => {
                other_files += 1;
                continue;
            }
        };
        if kind.is_some_and(|kind| kind != file_kind) {
            continue;
        }
        let name = file.path.rsplit('/').next().unwrap_or(&file.path);
        if file_kind == ItemKind::Script {
            items.push(json!({
                "kind": "Script",
                "name": name.strip_suffix(".luau").unwrap_or(name),
                "path": file.path,
                "used_by_count": script_uses(project, &file.path).len(),
            }));
            continue;
        }
        let asset = project
            .assets
            .iter()
            .find(|a| !a.path.is_empty() && names::same_name(&a.path, &file.path));
        let Some(asset) = asset else {
            // The old layout's files no asset names are none of the project's.
            if project.layout == Layout::Tree {
                items.push(json!({"kind": kind_name(file_kind), "name": name, "path": file.path}));
            }
            continue;
        };
        listed.insert(asset.id.as_str());
        items.push(asset_item(project, asset, checked.get(&asset.id), &cited));
    }
    // Assets whose files are not there, in the folder of their paths.
    for asset in &project.assets {
        if asset.path.is_empty()
            || listed.contains(asset.id.as_str())
            || !names::same_name(parent(&asset.path), &path)
            || kind.is_some_and(|kind| kind.asset() != Some(asset.kind))
        {
            continue;
        }
        items.push(asset_item(project, asset, checked.get(&asset.id), &cited));
    }
    let (items, about) = page.cut(items);
    let mut answer = Map::new();
    answer.insert("folder".into(), json!(path));
    answer.insert("items".into(), json!(items));
    answer.extend(about);
    if other_files > 0 {
        answer.insert("other_files".into(), json!(other_files));
    }
    Ok(Json::Object(answer))
}

/// The kind as the Library's entries name it.
fn kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Texture => "Texture",
        ItemKind::Audio => "Audio",
        ItemKind::Model => "Model",
        ItemKind::Script => "Script",
    }
}

/// An asset's entry in a folder's list, its uses counting the script lines that name it
/// (`cited`, see [`script::citations`]).
fn asset_item(
    project: &Project,
    asset: &crate::document::Asset,
    checked: Option<&Checked>,
    cited: &BTreeMap<String, Vec<script::Citation>>,
) -> Json {
    let uses = crate::editing::asset_uses(project, &asset.id).len()
        + cited.get(&asset.id).map_or(0, Vec::len);
    let mut entry = json!({
        "kind": format!("{:?}", asset.kind),
        "name": asset.name,
        "path": asset.path,
        "id": asset.id,
        "used_by_count": uses,
    });
    if let Some(checked) = checked {
        add_state(&mut entry, checked);
    }
    entry
}

/// The project's scripts as oxy_scripts list answers them, from the tree of the project saved at
/// `root`: each `{path, lines, kind, used_by, used_by_total?, requires?, required_by?, error?}`
/// in tree order, kind being script (an object of `project` attaches it), module (a script an
/// object runs requires it, as far as written whole) or unused (the game never runs it; the
/// export takes it all the same when it compiles), requires the scripts it requires (written
/// whole), required_by those that require it, and error why it does not load. Without
/// `project` (it could not be read), no script is attached.
pub fn scripts_list(project: Option<&Project>, root: &Path) -> Result<Json, String> {
    let (sources, tree) = persistence::load_scripts_in_tree(root).map_err(|e| e.to_string())?;
    let attached = project.map(script::attached_paths).unwrap_or_default();
    let used = script::used_scripts(&sources, attached.iter().cloned());
    let mut requires: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut required_by: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for (path, file) in &sources.files {
        for found in script::lexer::requires(&file.text) {
            let script::lexer::RequireKind::Literal { path: name, .. } = found.kind else {
                continue;
            };
            let Ok(target) = script::require_path(path, &name) else {
                continue;
            };
            let target = sources.known(&target).map_or(target.clone(), str::to_owned);
            let listed = requires.entry(path).or_default();
            if !listed.contains(&target) {
                listed.push(target.clone());
                required_by.entry(target).or_default().push(path);
            }
        }
    }
    let reports = script::check_loading(&sources, sources.files.keys().map(String::as_str), |p| {
        attached.contains(p)
    });
    let errors: BTreeMap<&str, String> = reports
        .iter()
        .filter_map(|report| Some((report.path.as_str(), report.error.as_ref()?.to_string())))
        .collect();
    let scripts: Vec<Json> = tree
        .scripts()
        .map(|file| {
            let path = file.path.as_str();
            let kind = if attached.contains(path) {
                "script"
            } else if used.contains(path) {
                "module"
            } else {
                "unused"
            };
            let lines = sources.get(path).map_or(0, |f| f.text.lines().count());
            let mut entry = json!({"path": path, "lines": lines, "kind": kind});
            uses_json(
                &mut entry,
                project.map(|p| script_uses(p, path)).unwrap_or_default(),
            );
            if let Some(listed) = requires.get(path) {
                entry["requires"] = json!(listed);
            }
            if let Some(listed) = required_by.get(path) {
                entry["required_by"] = json!(listed);
            }
            if let Some(why) = sources.unreadable.get(path) {
                entry["error"] = json!(format!("O script {path} {why}"));
            } else if let Some(error) = errors.get(path) {
                entry["error"] = json!(error);
            }
            entry
        })
        .collect();
    Ok(json!({"total": scripts.len(), "scripts": scripts}))
}

/// The scripts of `sources` no object of `project` runs: neither attached nor required, written
/// whole, by one that is (see [`script::used_scripts`]).
pub fn unused_scripts(project: &Project, sources: &script::ScriptSources) -> Vec<String> {
    let used = script::used_scripts(sources, script::attached_paths(project));
    sources
        .files
        .keys()
        .chain(sources.unreadable.keys())
        .filter(|path| !used.contains(*path))
        .cloned()
        .collect()
}

/// The warnings of how the assets of `project`, saved at `root`, stand on the disk (see
/// [`persistence::check_assets`]) and of the names two files of a kind share, as opening tells
/// them.
pub fn asset_warnings(
    project: &Project,
    root: &Path,
    unsaved: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let checked = persistence::check_assets(project, root, unsaved);
    let mut lines: Vec<String> = project
        .assets
        .iter()
        .filter_map(|asset| match checked.get(&asset.id)? {
            Checked::Problem(_, line) => Some(line.clone()),
            Checked::Unsaved => None,
        })
        .collect();
    if project.layout == Layout::Tree {
        lines.extend(persistence::shared_names(project));
    }
    lines
}
