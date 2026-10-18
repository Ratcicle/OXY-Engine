//! Scripts: Luau files in the project's `scripts` folder, with human names and subfolders, that
//! objects attach. The disk is the truth: the folder is the list of scripts, and the project
//! file keeps only each object's references, in the order they run. A script is known, in the
//! project, logs and agents, by its path from the project's folder with `/`, as
//! `scripts/porta.luau`. Values per object stay in its attributes.
use crate::{document::Project, input_actions::every_entity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// The scripts' folder, in the project's folder (`data` in an exported game).
pub const FOLDER: &str = "scripts";
/// The extension of script files.
pub const EXTENSION: &str = "luau";
/// Largest script file, in bytes.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// A script an object attaches, by its path in [`normalize_path`]'s form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptRef {
    pub path: String,
}
impl ScriptRef {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// The project's scripts as its folder holds them: each file's text by path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptSources {
    pub files: BTreeMap<String, ScriptFile>,
}
impl ScriptSources {
    pub fn get(&self, path: &str) -> Option<&ScriptFile> {
        self.files.get(path)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFile {
    pub text: String,
    /// The portable hash of the text: the same text has the same one on every machine, which
    /// tells a changed file and a file moved with the same contents.
    pub hash: u64,
}
impl ScriptFile {
    pub fn new(text: String) -> Self {
        let hash = crate::random::hash(&[text.as_bytes()]);
        Self { text, hash }
    }
}

/// `path` as scripts are named: from the project's folder with `/`, inside [`FOLDER`] and ending
/// in `.luau`, as `scripts/fases/porta.luau`. `\` reads as `/`, and `.` and empty parts go;
/// `..`, absolute paths, drives and names Windows would not keep are refused.
pub fn normalize_path(path: &str) -> Result<String, String> {
    let invalid = || {
        format!(
            "caminho de script inválido \"{path}\"; use um arquivo .{EXTENSION} dentro da pasta \
             {FOLDER}, como {FOLDER}/porta.{EXTENSION}"
        )
    };
    let unified = path.trim().replace('\\', "/");
    if unified.starts_with('/') {
        return Err(invalid());
    }
    let mut parts = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(invalid()),
            part if part.ends_with(['.', ' '])
                || part
                    .chars()
                    .any(|c| c.is_control() || r#"<>:"|?*"#.contains(c)) =>
            {
                return Err(invalid());
            }
            part => parts.push(part),
        }
    }
    match parts.as_slice() {
        [FOLDER, .., file]
            if file
                .strip_suffix(EXTENSION)
                .and_then(|stem| stem.strip_suffix('.'))
                .is_some_and(|stem| !stem.is_empty()) =>
        {
            Ok(parts.join("/"))
        }
        _ => Err(invalid()),
    }
}

/// Checks the scripts an object attaches: each path written as [`normalize_path`] writes it,
/// and none twice.
pub fn validate(scripts: &[ScriptRef]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for script in scripts {
        let normal = normalize_path(&script.path)?;
        if normal != script.path {
            return Err(format!(
                "o caminho de script \"{}\" deve ser escrito {normal}",
                script.path
            ));
        }
        if !seen.insert(script.path.as_str()) {
            return Err(format!("o script {} está anexado duas vezes", script.path));
        }
    }
    Ok(())
}

/// Whether an object of `project`, in a scene or a model, attaches a script.
pub fn any_attached(project: &Project) -> bool {
    every_entity(project).any(|(_, e)| !e.scripts.is_empty())
}

/// The first script an object of `project` attaches, in a scene or a model, that `exists` does
/// not find: the object as messages name it (a model's piece after its model) and the path.
pub fn first_missing(project: &Project, exists: impl Fn(&str) -> bool) -> Option<(String, &str)> {
    every_entity(project).find_map(|(holder, e)| {
        let script = e.scripts.iter().find(|s| !exists(&s.path))?;
        let object = if holder.model {
            format!("Modelo {holder}, {}", e.name)
        } else {
            e.name.clone()
        };
        Some((object, script.path.as_str()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Entity, Primitive, validate_project};

    #[test]
    fn paths_are_normalized_into_the_scripts_folder() {
        for (given, normal) in [
            ("scripts/porta.luau", "scripts/porta.luau"),
            (
                r"scripts\fases\Porta com chave.luau",
                "scripts/fases/Porta com chave.luau",
            ),
            (
                "./scripts//dados/./cartas.luau",
                "scripts/dados/cartas.luau",
            ),
            ("  scripts/ação.luau ", "scripts/ação.luau"),
        ] {
            assert_eq!(normalize_path(given).as_deref(), Ok(normal), "{given}");
        }
        for refused in [
            "",
            "scripts",
            "scripts/",
            "porta.luau",
            "outros/porta.luau",
            "Scripts/porta.luau",
            "scripts/porta.lua",
            "scripts/porta.luau.txt",
            "scripts/.luau",
            "scripts/../porta.luau",
            "../scripts/porta.luau",
            "/scripts/porta.luau",
            r"\\servidor\scripts\porta.luau",
            "C:/projeto/scripts/porta.luau",
            "C:scripts/porta.luau",
            "scripts/por?ta.luau",
            "scripts/fases./porta.luau",
            "scripts/fase /porta.luau",
            "scripts/linha\n.luau",
        ] {
            let error = normalize_path(refused).unwrap_err();
            assert!(
                error.starts_with("caminho de script inválido") && error.contains("scripts/"),
                "{refused}: {error}"
            );
        }
    }

    #[test]
    fn objects_keep_their_scripts_in_order_and_leave_the_field_out_without_any() {
        let mut e = Entity::new("Porta", Some(Primitive::Cube));
        let json = serde_json::to_value(&e).unwrap();
        assert!(json.get("scripts").is_none(), "{json}");
        e.scripts = vec![
            ScriptRef::new("scripts/porta.luau"),
            ScriptRef::new("scripts/comum/som.luau"),
        ];
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(
            json["scripts"],
            serde_json::json!([{"path": "scripts/porta.luau"}, {"path": "scripts/comum/som.luau"}])
        );
        assert_eq!(serde_json::from_value::<Entity>(json.clone()).unwrap(), e);
        let mut unknown = json;
        unknown["scripts"][0]["enabled"] = true.into();
        assert!(serde_json::from_value::<Entity>(unknown).is_err());
    }

    #[test]
    fn projects_refuse_scripts_outside_the_folder_unnormalized_or_twice() {
        let attach = |paths: &[&str]| {
            let mut project = Project::new("Scripts");
            let mut e = Entity::new("Porta", Some(Primitive::Cube));
            e.scripts = paths.iter().copied().map(ScriptRef::new).collect();
            project.scenes[0].entities.push(e);
            validate_project(&project)
        };
        attach(&[]).unwrap();
        attach(&["scripts/porta.luau", "scripts/comum/som.luau"]).unwrap();
        assert_eq!(
            attach(&["../porta.luau"]).unwrap_err(),
            "Porta: caminho de script inválido \"../porta.luau\"; use um arquivo .luau dentro \
             da pasta scripts, como scripts/porta.luau"
        );
        assert_eq!(
            attach(&[r"scripts\porta.luau"]).unwrap_err(),
            "Porta: o caminho de script \"scripts\\porta.luau\" deve ser escrito \
             scripts/porta.luau"
        );
        assert_eq!(
            attach(&["scripts/porta.luau", "scripts/porta.luau"]).unwrap_err(),
            "Porta: o script scripts/porta.luau está anexado duas vezes"
        );
        // A model's pieces follow the same rules.
        let mut project = Project::new("Modelo");
        let scene = project.scenes[0].id.clone();
        let mut piece = Entity::new("Peça", Some(Primitive::Cube));
        piece.scripts = vec![ScriptRef::new("scripts/peça.luau")];
        let id = piece.id.clone();
        project.scenes[0].entities.push(piece);
        project.save_model(&scene, &id, "Bloco").unwrap();
        project.scenes[0].entities.clear();
        validate_project(&project).unwrap();
        project.assets[0].model.as_mut().unwrap()[0].scripts = vec![ScriptRef::new("peça.luau")];
        assert!(
            validate_project(&project)
                .unwrap_err()
                .contains("caminho de script inválido \"peça.luau\"")
        );
    }

    #[test]
    fn the_first_missing_script_names_its_object_and_model() {
        let mut project = Project::new("Ausentes");
        let scene = project.scenes[0].id.clone();
        let mut piece = Entity::new("Peça", Some(Primitive::Cube));
        piece.scripts = vec![ScriptRef::new("scripts/peça.luau")];
        let id = piece.id.clone();
        project.scenes[0].entities.push(piece);
        project.save_model(&scene, &id, "Bloco").unwrap();
        assert!(any_attached(&project));
        assert_eq!(first_missing(&project, |_| true), None);
        assert_eq!(
            first_missing(&project, |_| false),
            Some(("Peça".to_owned(), "scripts/peça.luau"))
        );
        project.scenes[0].entities.clear();
        assert_eq!(
            first_missing(&project, |path| path != "scripts/peça.luau"),
            Some(("Modelo Bloco, Peça".to_owned(), "scripts/peça.luau"))
        );
        project.assets.clear();
        assert!(!any_attached(&project));
    }

    #[test]
    fn a_text_hashes_the_same_wherever_it_is_read() {
        let file = ScriptFile::new("print(\"oi\")\n".into());
        assert_eq!(file, ScriptFile::new("print(\"oi\")\n".into()));
        assert_ne!(file.hash, ScriptFile::new("print(\"olá\")\n".into()).hash);
        assert_eq!(file.hash, crate::random::hash(&[b"print(\"oi\")\n"]));
    }
}
