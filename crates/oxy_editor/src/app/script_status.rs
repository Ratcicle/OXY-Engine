//! What the editor knows of the scripts the project's objects attach, for the Scripts card and
//! the { } mark of the Hierarchy: the project's scripts as its folder last held them (the list
//! "+ Anexar script" offers), the attached ones the folder lacks or that do not load (checked as
//! the runtime loads them, again when what is attached changes and when the files may have
//! changed: the window got the focus back, F5, an agent's write), the attributes each one
//! declares, and where the current or last test's scripts failed.
use super::library::ScriptFacts;
use super::*;
use oxy_core::script::{self, ScriptRef};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct ScriptStatus {
    /// What the last check looked at: the attached scripts, and how many reasons to look again
    /// there had been.
    checked: Option<(BTreeSet<String>, u64)>,
    reasons: u64,
    /// The project's scripts, by path, as the last check found them in its folder.
    pub(crate) files: Vec<String>,
    /// Why each attached script does not load or cannot be read: the script the error points at
    /// (it or one it requires), its line (0 when the error names none) and the message.
    load: BTreeMap<String, (String, u32, String)>,
    /// The attached scripts the folder lacks.
    missing: BTreeSet<String>,
    /// The attributes each attached script declares.
    declared: BTreeMap<String, Vec<String>>,
    /// The scripts each attached script requires, as far as its `require`s name them.
    uses: BTreeMap<String, BTreeSet<String>>,
    /// The hash of each script's text at the last check: a script changed since fails no more
    /// as it did in the last test.
    hashes: BTreeMap<String, u64>,
    /// Where the current or last test's scripts failed.
    test: Vec<TestError>,
    /// What compiling found of every script of the folder, for the Library, by path, with the
    /// hash of the text it was compiled at: a script whose text did not change is not compiled
    /// again. Whether an attached one loads is not kept here: it changes with what it requires.
    facts: BTreeMap<String, (u64, ScriptFacts)>,
}

/// An error of a test's script.
struct TestError {
    /// The object, empty for a script that did not load.
    owner: Id,
    /// The script of the object the card shows it under (empty with no object): the one it
    /// happened in or, for one that script requires, the attached script that requires it.
    script: String,
    /// The script it happened in, its line (0 when the error names none) and what the log
    /// told.
    path: String,
    line: u32,
    text: String,
}

impl ScriptStatus {
    /// The files may have changed: the next refresh reads them again.
    pub(crate) fn look_again(&mut self) {
        self.reasons += 1;
    }

    /// Checks the scripts of `project`, saved in `root`, again when what its objects attach
    /// changed or there is a reason to look.
    pub(crate) fn refresh(&mut self, project: &Project, root: Option<&Path>) {
        let attached = script::attached_paths(project);
        if self
            .checked
            .as_ref()
            .is_some_and(|(checked, reasons)| *checked == attached && *reasons == self.reasons)
        {
            return;
        }
        self.check(project, root, &attached, false);
        self.checked = Some((attached, self.reasons));
    }

    /// A test starts: what the last one found goes.
    pub(crate) fn start_test(&mut self) {
        self.test.clear();
    }

    /// Where the scripts of the test `runtime` runs failed so far.
    pub(crate) fn take_test(&mut self, runtime: &Runtime) {
        let mut test = Vec::new();
        for (object, path, line, text) in runtime.script_error_lines() {
            let script = match runtime.scene().entity(object) {
                Some(e) => self.shown_under(&e.scripts, path),
                // Gone from the test since (removed, or in another scene): where it showed.
                None => self
                    .test
                    .iter()
                    .find(|e| e.owner == object && e.path == path)
                    .map_or(if object.is_empty() { "" } else { path }, |e| {
                        e.script.as_str()
                    }),
            };
            test.push(TestError {
                owner: object.to_owned(),
                script: script.to_owned(),
                path: path.to_owned(),
                line,
                text: text.to_owned(),
            });
        }
        self.test = test;
    }

    /// The script of `scripts` an error in the script `path` shows under: that one, else the
    /// first that requires it, else the first (a `require` whose path the script builds).
    fn shown_under<'a>(&self, scripts: &'a [ScriptRef], path: &'a str) -> &'a str {
        let requires = |s: &&ScriptRef| self.uses.get(&s.path).is_some_and(|u| u.contains(path));
        scripts
            .iter()
            .find(|s| s.path == path)
            .or_else(|| scripts.iter().find(requires))
            .or(scripts.first())
            .map_or(path, |s| s.path.as_str())
    }

    /// The test of `runtime` stops: what it found stays until a script it ran changes, so the
    /// files are read again now, as the test last ran them.
    pub(crate) fn end_test(&mut self, runtime: &Runtime, project: &Project, root: Option<&Path>) {
        self.take_test(runtime);
        let attached = script::attached_paths(project);
        self.check(project, root, &attached, true);
        self.checked = Some((attached, self.reasons));
    }

    fn check(
        &mut self,
        project: &Project,
        root: Option<&Path>,
        attached: &BTreeSet<String>,
        keep_test: bool,
    ) {
        let (sources, tree) = match root.map(persistence::load_scripts_in_tree) {
            Some(Ok(read)) => read,
            // A folder that cannot be read now tells nothing new.
            Some(Err(_)) => return,
            None => Default::default(),
        };
        self.files = tree.scripts().map(|file| file.path.clone()).collect();
        self.missing = attached
            .iter()
            .filter(|path| {
                root.is_some() && sources.get(path).is_none() && sources.why_none(path).is_none()
            })
            .cloned()
            .collect();
        self.load = attached
            .iter()
            .filter_map(|path| {
                let why = sources.why_none(path)?;
                let message = format!("o script {path} {why}");
                Some((path.clone(), (path.clone(), 0, message)))
            })
            .collect();
        let present: Vec<&str> = attached
            .iter()
            .map(String::as_str)
            .filter(|path| sources.get(path).is_some())
            .collect();
        for report in script::check_loading(&sources, present.iter().copied(), |path| {
            attached.contains(path)
        }) {
            if let Some(error) = report.error {
                self.load
                    .insert(report.path, (error.path, error.line, error.message));
            }
        }
        self.uses = present
            .iter()
            .map(|path| {
                let mut used = script::used_scripts(&sources, [path.to_string()]);
                used.remove(*path);
                (path.to_string(), used)
            })
            .collect();
        self.declared =
            script::declared_attributes(&sources, &project.surfaces, present.iter().copied())
                .into_iter()
                .filter_map(|(path, declared)| {
                    let names = declared.ok()?.into_iter().map(|(name, _)| name).collect();
                    Some((path.to_owned(), names))
                })
                .collect();
        let hashes: BTreeMap<String, u64> = sources
            .files
            .iter()
            .map(|(path, file)| (path.clone(), file.hash))
            .collect();
        if !keep_test {
            // The error goes when the script it happened in or the one it shows under changed.
            let before = &self.hashes;
            let same = |path: &String| hashes.get(path) == before.get(path);
            self.test.retain(|e| same(&e.path) && same(&e.script));
        }
        self.hashes = hashes;
        self.check_files(&sources, attached);
    }

    /// Compiles every script of `sources` whose text changed since the last check, for the
    /// Library: whether it compiles, and the sounds it plays by name. A script that cannot be
    /// read, an `attached` one in a folder that cannot be read included, is listed with why.
    fn check_files(&mut self, sources: &ScriptSources, attached: &BTreeSet<String>) {
        let mut facts = BTreeMap::new();
        for (path, file) in &sources.files {
            let known = self
                .facts
                .remove(path)
                .filter(|(hash, _)| *hash == file.hash);
            let found = match known {
                Some((_, found)) => found,
                None => {
                    let mut found = ScriptFacts::default();
                    match script::check(path, &file.text) {
                        Ok(checked) => {
                            found.sounds = checked
                                .references
                                .into_iter()
                                .filter(|r| r.kind == script::NameKind::Sound)
                                .map(|r| r.name)
                                .collect();
                        }
                        Err(error) => found.error = Some((error.line, error.message)),
                    }
                    found
                }
            };
            facts.insert(path.clone(), (file.hash, found));
        }
        let unreadable = sources
            .unreadable
            .iter()
            .map(|(path, why)| (path, why.as_str()));
        let unlisted = attached
            .iter()
            .filter(|path| !sources.unreadable.contains_key(*path))
            .filter_map(|path| Some((path, sources.why_none(path)?)));
        for (path, why) in unreadable.chain(unlisted) {
            let found = ScriptFacts {
                unreadable: Some(why.to_owned()),
                ..ScriptFacts::default()
            };
            facts.insert(path.clone(), (0, found));
        }
        self.facts = facts;
    }

    /// What the Library shows of each script of the folder, and of each attached one that cannot
    /// be read, by path: an attached script that compiles but does not load fails as the check of
    /// the attached ones says now.
    pub(crate) fn library_facts(&self) -> BTreeMap<String, ScriptFacts> {
        self.facts
            .iter()
            .map(|(path, (_, compiled))| {
                let mut found = compiled.clone();
                if found.error.is_none()
                    && found.unreadable.is_none()
                    && let Some((at, line, message)) = self.load.get(path)
                {
                    found.error = Some((if at == path { *line } else { 0 }, message.clone()));
                }
                (path.clone(), found)
            })
            .collect()
    }

    /// How many scripts the Library lists: those [`Self::library_facts`] has and the attached
    /// ones the folder lacks.
    pub(crate) fn library_len(&self) -> usize {
        let lacking = self
            .missing
            .iter()
            .filter(|path| !self.facts.contains_key(*path))
            .count();
        self.facts.len() + lacking
    }

    /// The attached scripts the project's folder lacks.
    pub(crate) fn missing_paths(&self) -> &BTreeSet<String> {
        &self.missing
    }

    /// Whether the project's folder lacks the script `path`.
    pub(crate) fn missing(&self, path: &str) -> bool {
        self.missing.contains(path)
    }

    /// The hash of the text of the script `path` at the last check; none when the folder lacked
    /// it.
    pub(crate) fn hash(&self, path: &str) -> Option<u64> {
        self.hashes.get(path).copied()
    }

    /// The attached scripts whose `require`s name the script `path`, by path.
    pub(crate) fn required_by(&self, path: &str) -> Vec<&str> {
        self.uses
            .iter()
            .filter(|(_, used)| used.contains(path))
            .map(|(script, _)| script.as_str())
            .collect()
    }

    /// Where the script `path` of `entity` fails on it: the script each error points at (that
    /// one or one it requires) and its line (0 for an error that names none), each place once,
    /// those of `path` first, then by line, with a message.
    pub(crate) fn errors(&self, entity: &Entity, path: &str) -> Vec<(&str, u32, &str)> {
        let load = self
            .load
            .get(path)
            .map(|(at, line, message)| (at.as_str(), *line, message.as_str()));
        let uses = self.uses.get(path);
        let test = self
            .test
            .iter()
            .filter(|e| {
                if e.owner.is_empty() {
                    e.path == path || uses.is_some_and(|used| used.contains(&e.path))
                } else {
                    e.owner == entity.id && e.script == path
                }
            })
            .map(|e| (e.path.as_str(), e.line, e.text.as_str()));
        let mut errors: Vec<(&str, u32, &str)> = load.into_iter().chain(test).collect();
        errors.sort_by_key(|(at, line, _)| (*at != path, *at, *line));
        errors.dedup_by_key(|(at, line, _)| (*at, *line));
        errors
    }

    /// Whether a script `entity` attaches is missing or fails on it.
    pub(crate) fn failing(&self, entity: &Entity) -> bool {
        entity
            .scripts
            .iter()
            .any(|s| self.missing(&s.path) || !self.errors(entity, &s.path).is_empty())
    }

    /// The scripts of `scripts` that declare the attribute `name`, in their order.
    pub(crate) fn declaring<'a>(&self, scripts: &'a [ScriptRef], name: &str) -> Vec<&'a str> {
        scripts
            .iter()
            .filter(|s| {
                self.declared
                    .get(&s.path)
                    .is_some_and(|names| names.iter().any(|n| n == name))
            })
            .map(|s| s.path.as_str())
            .collect()
    }
}

impl Editor {
    /// Once a frame: where the running test's scripts failed (for the Console too), and the
    /// check of the attached scripts when it is due.
    pub(crate) fn refresh_scripts(&mut self) {
        if let Some(runtime) = &self.play.runtime {
            self.script_status.take_test(runtime);
            super::console_panel::remember_script_errors(&mut self.console_errors, runtime);
        }
        let root = self.project_root();
        self.script_status
            .refresh(&self.state.project, root.as_deref());
    }
}

/// The file name of the script `path`, as the card and the dock show it.
pub(crate) fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The first place of `places` (as [`ScriptStatus::errors`] gives them) with a line: where Ver
/// código opens.
pub(crate) fn first_line<'a>(places: &[(&'a str, u32)]) -> Option<(&'a str, u32)> {
    places.iter().copied().find(|(_, line)| *line > 0)
}

/// How the Scripts card says that the script `path` fails, in red: missing, or how many places
/// fail and where the first is (`places` as [`ScriptStatus::errors`] gives them), in another
/// script when it is one `path` requires; none when it does not.
pub(crate) fn failure(path: &str, missing: bool, places: &[(&str, u32)]) -> Option<String> {
    if missing {
        return Some("Arquivo não encontrado".into());
    }
    let first = first_line(places).map(|(at, line)| {
        if at == path {
            format!("na linha {line}")
        } else {
            format!("em {}, linha {line}", file_name(at))
        }
    });
    Some(match (places.len(), first) {
        (0, _) => return None,
        (1, Some(at)) => format!("1 erro {at}"),
        (n, Some(at)) => format!("{n} erros, o primeiro {at}"),
        (1, None) => "1 erro".into(),
        (n, None) => format!("{n} erros"),
    })
}

/// How many objects use a script, for the Scripts card.
pub(crate) fn users(count: usize) -> String {
    match count {
        1 => "Usado por 1 objeto".into(),
        n => format!("Usado por {n} objetos"),
    }
}

/// The scripts of `files` whose path has `query` (in any case), by folder: those at the root
/// first, under `root`, then each folder by name, as "Pista › Checkpoints"; each script by its
/// path, in order.
pub(crate) fn by_folder<'a>(
    files: &'a [String],
    query: &str,
    root: &str,
) -> Vec<(String, Vec<&'a str>)> {
    let query = query.trim().to_lowercase();
    let mut folders: BTreeMap<(bool, String), Vec<&str>> = BTreeMap::new();
    for path in files {
        if !path.to_lowercase().contains(&query) {
            continue;
        }
        let key = match path.rsplit_once('/') {
            Some((folder, _)) => (true, folder.to_owned()),
            None => (false, String::new()),
        };
        folders.entry(key).or_default().push(path);
    }
    folders
        .into_iter()
        .map(|((_, folder), mut paths)| {
            paths.sort_by_key(|path| path.to_lowercase());
            let caption = if folder.is_empty() {
                root.to_owned()
            } else {
                folder.replace('/', " › ")
            };
            (caption, paths)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::runtime::{FIXED_DT, InputFrame};

    #[test]
    fn failures_say_where_the_first_line_is() {
        const DOOR: &str = "scripts/porta.luau";
        assert_eq!(failure(DOOR, false, &[]), None);
        assert_eq!(
            failure(DOOR, false, &[(DOOR, 25)]).as_deref(),
            Some("1 erro na linha 25")
        );
        assert_eq!(
            failure(DOOR, false, &[(DOOR, 0), (DOOR, 12), (DOOR, 40)]).as_deref(),
            Some("3 erros, o primeiro na linha 12")
        );
        assert_eq!(
            failure(DOOR, false, &[(DOOR, 0)]).as_deref(),
            Some("1 erro")
        );
        assert_eq!(
            failure(DOOR, true, &[(DOOR, 3)]).as_deref(),
            Some("Arquivo não encontrado")
        );
        // In a script it requires, the card names that one.
        let places = [(DOOR, 0), ("scripts/comum/util.luau", 5)];
        assert_eq!(
            failure(DOOR, false, &places[1..]).as_deref(),
            Some("1 erro em util.luau, linha 5")
        );
        assert_eq!(
            failure(DOOR, false, &places).as_deref(),
            Some("2 erros, o primeiro em util.luau, linha 5")
        );
        assert_eq!(first_line(&places), Some(("scripts/comum/util.luau", 5)));
        assert_eq!(users(1), "Usado por 1 objeto");
        assert_eq!(users(6), "Usado por 6 objetos");
        assert_eq!(
            file_name("Pista/Checkpoints/checkpoint.luau"),
            "checkpoint.luau"
        );
        assert_eq!(file_name("porta.luau"), "porta.luau");
    }

    #[test]
    fn the_scripts_to_attach_go_by_folder() {
        let files: Vec<String> = [
            "Pista/contagem.luau",
            "Menu/menu_principal.luau",
            "Pista/Checkpoints/checkpoint.luau",
            "solto.luau",
            "Pista/Arquibancada.luau",
        ]
        .map(String::from)
        .to_vec();
        let grouped = by_folder(&files, "", "Parkour");
        assert_eq!(
            grouped,
            [
                ("Parkour".to_owned(), vec!["solto.luau"]),
                ("Menu".to_owned(), vec!["Menu/menu_principal.luau"]),
                (
                    "Pista".to_owned(),
                    vec!["Pista/Arquibancada.luau", "Pista/contagem.luau"]
                ),
                (
                    "Pista › Checkpoints".to_owned(),
                    vec!["Pista/Checkpoints/checkpoint.luau"]
                ),
            ]
        );
        // The search looks in the whole path, in any case.
        assert_eq!(
            by_folder(&files, " CHECK", "Parkour"),
            [(
                "Pista › Checkpoints".to_owned(),
                vec!["Pista/Checkpoints/checkpoint.luau"]
            )]
        );
        assert!(by_folder(&files, "nada", "Parkour").is_empty());
    }

    /// A project in a new folder whose door attaches `scripts/porta.luau` (it declares Aberta
    /// and fails at line 3 when it starts) and `scripts/sumiu.luau`, which the folder lacks.
    fn door() -> (Project, PathBuf, Id) {
        let root = std::env::temp_dir().join(format!("oxy-editor-script-status-{}", new_id()));
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(
            root.join("scripts/porta.luau"),
            "local M = {}\nM.attributes = { Aberta = false }\nfunction M.on_start(self) error('quebrou') end\nreturn M\n",
        )
        .unwrap();
        std::fs::write(root.join("scripts/outro.luau"), "return {}\n").unwrap();
        let mut project = Project::new("Porta");
        let mut door = Entity::new("Porta", None);
        door.attributes.insert("Aberta".into(), Value::Bool(false));
        door.scripts = vec![
            ScriptRef::new("scripts/porta.luau"),
            ScriptRef::new("scripts/sumiu.luau"),
        ];
        let id = door.id.clone();
        project.scenes[0].entities.push(door);
        (project, root, id)
    }

    /// A test of `project` with the scripts of `root`, one step in.
    fn test_run(project: &Project, root: &Path) -> Runtime {
        let options = RuntimeOptions {
            scripts: std::sync::Arc::new(persistence::load_scripts(root).unwrap()),
            ..Default::default()
        };
        let mut runtime = Runtime::with_options(project, &project.start_scene, options).unwrap();
        runtime.advance(FIXED_DT, &InputFrame::default());
        runtime
    }

    #[test]
    fn an_error_in_a_required_script_shows_under_the_script_that_requires_it() {
        let root = std::env::temp_dir().join(format!("oxy-editor-script-status-{}", new_id()));
        std::fs::create_dir_all(root.join("scripts/comum")).unwrap();
        std::fs::write(
            root.join("scripts/comum/util.luau"),
            "local util = {}\nfunction util.abrir()\n    error('emperrou')\nend\nreturn util\n",
        )
        .unwrap();
        std::fs::write(
            root.join("scripts/porta.luau"),
            "local util = require(\"./comum/util\")\nlocal M = {}\n\
             function M.on_start(self) util.abrir() end\nreturn M\n",
        )
        .unwrap();
        std::fs::write(root.join("scripts/luz.luau"), "return {}\n").unwrap();
        let mut project = Project::new("Porta");
        let mut door = Entity::new("Porta", None);
        door.scripts = vec![
            ScriptRef::new("scripts/luz.luau"),
            ScriptRef::new("scripts/porta.luau"),
        ];
        let mut lamp = Entity::new("Luz", None);
        lamp.scripts = vec![ScriptRef::new("scripts/luz.luau")];
        project.scenes[0]
            .entities
            .extend([door.clone(), lamp.clone()]);
        let mut status = ScriptStatus::default();
        status.refresh(&project, Some(&root));
        assert!(!status.failing(&door));
        // The code tab says who uses util.luau, which no object attaches.
        assert_eq!(
            status.required_by("scripts/comum/util.luau"),
            ["scripts/porta.luau"]
        );
        assert!(status.required_by("scripts/porta.luau").is_empty());
        let runtime = test_run(&project, &root);
        status.start_test();
        status.take_test(&runtime);
        // The error is in util.luau, line 3, on the door, which runs it through porta.luau.
        let errors = status.errors(&door, "scripts/porta.luau");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(
            (errors[0].0, errors[0].1),
            ("scripts/comum/util.luau", 3),
            "{errors:?}"
        );
        assert!(status.errors(&door, "scripts/luz.luau").is_empty());
        assert!(status.failing(&door) && !status.failing(&lamp));
        // It goes once the script it happened in changes.
        std::fs::write(
            root.join("scripts/comum/util.luau"),
            "local util = {}\nfunction util.abrir() end\nreturn util\n",
        )
        .unwrap();
        status.look_again();
        status.refresh(&project, Some(&root));
        assert!(!status.failing(&door));
        // A required script that does not load fails the script that requires it at its
        // require, whose message says where.
        std::fs::write(root.join("scripts/comum/util.luau"), "local util = {\n").unwrap();
        status.look_again();
        status.refresh(&project, Some(&root));
        let errors = status.errors(&door, "scripts/porta.luau");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(
            (errors[0].0, errors[0].1),
            ("scripts/porta.luau", 1),
            "{errors:?}"
        );
        assert!(errors[0].2.contains("util.luau:2"), "{errors:?}");
        assert!(status.failing(&door) && !status.failing(&lamp));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_library_learns_what_every_script_of_the_folder_is() {
        let root = std::env::temp_dir().join(format!("oxy-editor-script-facts-{}", new_id()));
        std::fs::create_dir_all(root.join("Pista")).unwrap();
        std::fs::write(
            root.join("Pista/checkpoint.luau"),
            "local M = {}\nfunction M.on_start(self)\n    audio.play(\"Bipe\")\nend\nreturn M\n",
        )
        .unwrap();
        std::fs::write(root.join("Pista/quebrado.luau"), "local M = {\nreturn M\n").unwrap();
        std::fs::write(root.join("Pista/binario.luau"), [0xff, 0xfe, 0x00]).unwrap();
        let project = Project::new("Fatos");
        let mut status = ScriptStatus::default();
        status.refresh(&project, Some(&root));
        let facts = status.library_facts();
        assert_eq!(
            facts.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "Pista/binario.luau",
                "Pista/checkpoint.luau",
                "Pista/quebrado.luau"
            ]
        );
        // No object attaches them: each is compiled all the same.
        let checkpoint = &facts["Pista/checkpoint.luau"];
        assert_eq!(checkpoint.sounds, ["Bipe"]);
        assert_eq!(checkpoint.error, None);
        let broken = &facts["Pista/quebrado.luau"];
        assert_eq!(broken.error.as_ref().map(|(line, _)| *line), Some(2));
        assert!(broken.sounds.is_empty());
        assert!(
            facts["Pista/binario.luau"]
                .unreadable
                .as_deref()
                .is_some_and(|why| why.contains("UTF-8")),
            "{facts:?}"
        );
        // Fixed outside OXY, it compiles once the files are looked at again.
        std::fs::write(root.join("Pista/quebrado.luau"), "return {}\n").unwrap();
        status.look_again();
        status.refresh(&project, Some(&root));
        assert_eq!(status.library_facts()["Pista/quebrado.luau"].error, None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_library_forgets_why_a_script_did_not_load_once_that_goes() {
        let root = std::env::temp_dir().join(format!("oxy-editor-script-facts-{}", new_id()));
        std::fs::create_dir_all(root.join("scripts/comum")).unwrap();
        std::fs::write(root.join("scripts/comum/util.luau"), "local util = {\n").unwrap();
        std::fs::write(
            root.join("scripts/porta.luau"),
            "local util = require(\"./comum/util\")\nreturn {}\n",
        )
        .unwrap();
        let mut project = Project::new("Porta");
        let mut door = Entity::new("Porta", None);
        door.scripts = vec![ScriptRef::new("scripts/porta.luau")];
        project.scenes[0].entities.push(door);
        let mut status = ScriptStatus::default();
        let error_line = |status: &ScriptStatus, path: &str| {
            status.library_facts()[path]
                .error
                .as_ref()
                .map(|(line, _)| *line)
        };
        status.refresh(&project, Some(&root));
        // porta.luau compiles, but does not load: it fails at its require.
        assert_eq!(error_line(&status, "scripts/porta.luau"), Some(1));
        assert!(error_line(&status, "scripts/comum/util.luau").is_some());
        // util.luau fixed: porta.luau, whose text did not change, fails no more.
        std::fs::write(root.join("scripts/comum/util.luau"), "return {}\n").unwrap();
        status.look_again();
        status.refresh(&project, Some(&root));
        assert_eq!(error_line(&status, "scripts/porta.luau"), None);
        // Broken again, then taken off the door: nothing loads it, and it fails no more.
        std::fs::write(root.join("scripts/comum/util.luau"), "local util = {\n").unwrap();
        status.look_again();
        status.refresh(&project, Some(&root));
        assert_eq!(error_line(&status, "scripts/porta.luau"), Some(1));
        project.scenes[0].entities[0].scripts.clear();
        status.refresh(&project, Some(&root));
        assert_eq!(error_line(&status, "scripts/porta.luau"), None);
        assert_eq!(status.library_len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_check_tells_missing_scripts_declared_attributes_and_the_test_errors() {
        let (mut project, root, id) = door();
        let mut status = ScriptStatus::default();
        status.refresh(&project, Some(&root));
        assert_eq!(status.files, ["scripts/outro.luau", "scripts/porta.luau"]);
        assert!(status.missing("scripts/sumiu.luau") && !status.missing("scripts/porta.luau"));
        let door = project.scenes[0].entity(&id).unwrap().clone();
        assert!(status.errors(&door, "scripts/porta.luau").is_empty());
        assert!(status.failing(&door));
        assert_eq!(
            status.declaring(&door.scripts, "Aberta"),
            ["scripts/porta.luau"]
        );
        assert!(status.declaring(&door.scripts, "Vida").is_empty());
        // The test that runs the door's script finds its error, on that object and line.
        project.scenes[0].entity_mut(&id).unwrap().scripts.pop();
        let door = project.scenes[0].entity(&id).unwrap().clone();
        let other = Entity::new("Outro", None);
        let runtime = test_run(&project, &root);
        status.start_test();
        status.take_test(&runtime);
        let errors = status.errors(&door, "scripts/porta.luau");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!((errors[0].0, errors[0].1), ("scripts/porta.luau", 3));
        assert!(errors[0].2.contains("quebrou"), "{errors:?}");
        assert!(status.errors(&other, "scripts/porta.luau").is_empty());
        // The errors stay after the test, until the script changes.
        status.end_test(&runtime, &project, Some(&root));
        assert_eq!(status.errors(&door, "scripts/porta.luau").len(), 1);
        status.look_again();
        status.refresh(&project, Some(&root));
        assert_eq!(status.errors(&door, "scripts/porta.luau").len(), 1);
        std::fs::write(
            root.join("scripts/porta.luau"),
            "local M = {}\nM.attributes = { Aberta = false }\nreturn M\n",
        )
        .unwrap();
        status.look_again();
        status.refresh(&project, Some(&root));
        assert!(status.errors(&door, "scripts/porta.luau").is_empty());
        // A script that does not load fails where the check says, on every object.
        std::fs::write(root.join("scripts/porta.luau"), "local M = {\nreturn M\n").unwrap();
        status.look_again();
        status.refresh(&project, Some(&root));
        let errors = status.errors(&other, "scripts/porta.luau");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(
            (errors[0].0, errors[0].1),
            ("scripts/porta.luau", 2),
            "{errors:?}"
        );
        // Nothing is read again without a reason.
        std::fs::remove_file(root.join("scripts/porta.luau")).unwrap();
        status.refresh(&project, Some(&root));
        assert!(!status.missing("scripts/porta.luau"));
        status.look_again();
        status.refresh(&project, Some(&root));
        assert!(status.missing("scripts/porta.luau"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
