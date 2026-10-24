//! The lint of names (design §6.4, friction A14): the names a script gives with literal texts
//! (sounds, game variables, attributes, scenes, models, input actions, clips, markers, objects
//! and surfaces), checked against the project with the nearest name, as warnings in
//! oxy_scripts check and write, in oxy_validate and in the answer of attach_script; and the
//! names each script gives, which tools read to see where a script uses a sound or a scene.
use oxy_core::{
    animation::{AnimationEvent, Clip},
    command::{self, Command, ParamValue},
    document::*,
    input_actions, query,
    script::{self, Found, NameKind, ProjectNames, ScriptSources},
    script_files::{self, Request},
    surface::{SurfaceMaterial, SurfacePreset},
    variables,
};
use serde_json::{Value as Json, json};
use std::collections::HashSet;
use std::path::PathBuf;

const PATH: &str = "scripts/porta.luau";

/// A door script that misspells one name of each kind, and spells the rest right.
const DOOR: &str = r#"local M = {}
M.attributes = { Aberta = false }

function M.on_start(self)
    audio.play("Checkpointt")
    audio.play("Bipe")
    vars.set("Pontoss", vars.get("Pontos") + 1)
    self.attr.Aberta = true
    self.attr.Vidaa = 1
    if self.attr.Ordem then print(self.attr.Ordem) end
    scene.get("Portãoo"):get_child("Maçaneta").visible = false
    self:get_child("Macaneta")
    self.animation:play("Abirr")
    scene.spawn_model("Bala")
    if input.pressed("Pulrr") then scene.change("Menuu") end
end

function M.on_input(self, action, phase)
    if action == "pular" and phase == "pressed" then
        print("pulou")
    end
end

function M.on_marker(self, marker, clip)
    if marker == "Meio" or marker == "Fim" then print(clip) end
end

return M
"#;

/// Another script, which declares the attribute `Ordem` the door reads.
const ORDER: (&str, &str) = (
    "scripts/ordem.luau",
    "local M = {}\nM.attributes = { Ordem = 1 }\nreturn M\n",
);

fn sound(project: &mut Project, name: &str) -> Id {
    let id = new_id();
    project.assets.push(Asset {
        id: id.clone(),
        name: name.into(),
        path: format!("assets/{}.wav", name.to_lowercase()),
        kind: AssetKind::Audio,
        model: None,
    });
    id
}

/// A project with what the door names: the sounds `Checkpoint` and `Bipe`, the variable
/// `Pontos`, the scene `Menu`, the surface `Gelo`, the door with its clip `Abrir` (marker
/// `Meio`) and its child `Maçaneta`, and the gate `Portão`; no models.
fn project() -> (Project, Id) {
    let mut project = Project::new("Nomes");
    sound(&mut project, "Checkpoint");
    sound(&mut project, "Bipe");
    variables::create(&mut project, "Pontos", Value::Number(0.), false).unwrap();
    project.scenes.push(Scene::new("Menu", SceneKind::TwoD));
    project
        .surfaces
        .push(SurfaceMaterial::preset(SurfacePreset::Ice));
    let mut door = Entity::new("Porta", Some(Primitive::Cube));
    let mut clip = Clip::new("Abrir");
    clip.events.push(AnimationEvent {
        time: 0.5,
        name: "Meio".into(),
    });
    door.clips.push(clip);
    let id = door.id.clone();
    let mut handle = Entity::new("Maçaneta", None);
    handle.parent = Some(id.clone());
    project.scenes[0]
        .entities
        .extend([door, handle, Entity::new("Portão", None)]);
    (project, id)
}

/// What the lint warns of the door in `project`, with the scripts of `sources`.
fn lint(sources: &ScriptSources, project: &Project) -> Vec<String> {
    script::lint(sources, [PATH], Some(project))
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The door's warnings in the project above, each with the nearest name or the list.
fn door_warnings() -> Vec<String> {
    [
        "scripts/porta.luau:5: audio.play: o som \"Checkpointt\" não existe no projeto; quis \
         dizer Checkpoint?",
        "scripts/porta.luau:7: vars.set: a variável do jogo \"Pontoss\" não existe no projeto; \
         quis dizer Pontos?",
        "scripts/porta.luau:9: obj.attr: nenhum objeto do projeto tem o atributo \"Vidaa\" e \
         nenhum script o declara. Atributos: Aberta, Ordem",
        "scripts/porta.luau:11: scene.get: nenhum objeto do projeto se chama \"Portãoo\"; quis \
         dizer Portão?",
        "scripts/porta.luau:12: obj:get_child: nenhum objeto do projeto se chama \"Macaneta\"; \
         quis dizer Maçaneta?",
        "scripts/porta.luau:13: obj.animation:play: nenhum objeto do projeto tem o clipe \
         \"Abirr\"; quis dizer Abrir?",
        "scripts/porta.luau:14: scene.spawn_model: o modelo \"Bala\" não existe no projeto: o \
         projeto não tem modelos (salve um objeto como modelo na Biblioteca)",
        "scripts/porta.luau:15: input.pressed: a ação de entrada \"Pulrr\" não existe no \
         projeto; quis dizer Pular?",
        "scripts/porta.luau:15: scene.change: a cena \"Menuu\" não existe no projeto; quis \
         dizer Menu?",
        "scripts/porta.luau:19: on_input: a ação de entrada \"pular\" não existe no projeto; \
         quis dizer Pular? A comparação é com o nome exato, com maiúsculas e espaços",
        "scripts/porta.luau:25: on_marker: nenhum clipe do projeto tem o marcador \"Fim\". \
         Marcadores: Meio",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[test]
fn the_lint_names_what_the_project_lacks_with_the_nearest_name() {
    let (project, _) = project();
    let sources = ScriptSources::from_texts([(PATH, DOOR), ORDER]);
    assert_eq!(lint(&sources, &project), door_warnings());
    // Without the script that declares it, no object has Ordem: told once for its line.
    let alone = ScriptSources::from_texts([(PATH, DOOR)]);
    let warnings = lint(&alone, &project);
    assert_eq!(warnings.len(), door_warnings().len() + 1, "{warnings:?}");
    assert_eq!(
        warnings[3],
        "scripts/porta.luau:10: obj.attr: nenhum objeto do projeto tem o atributo \"Ordem\" e \
         nenhum script o declara. Atributos: Aberta"
    );
    // A project that has them all: nothing to tell.
    let mut complete = project.clone();
    sound(&mut complete, "Checkpointt");
    variables::create(&mut complete, "Pontoss", Value::Number(0.), false).unwrap();
    let door = &mut complete.scenes[0].entities[0];
    door.attributes.insert("Vidaa".into(), Value::Number(0.));
    door.clips.push(Clip::new("Abirr"));
    door.clips[0].events.push(AnimationEvent {
        time: 0.9,
        name: "Fim".into(),
    });
    complete.scenes[0]
        .entities
        .extend([Entity::new("Portãoo", None), Entity::new("Macaneta", None)]);
    let bullet = Entity::new("Bala", None);
    let (scene, bullet_id) = (complete.start_scene.clone(), bullet.id.clone());
    complete.scenes[0].entities.push(bullet);
    complete.save_model(&scene, &bullet_id, "Bala").unwrap();
    input_actions::create(&mut complete, "Pulrr", "P").unwrap();
    complete.scenes[1].name = "Menuu".into();
    let warnings = lint(&sources, &complete);
    assert_eq!(
        warnings,
        [
            "scripts/porta.luau:19: on_input: a ação de entrada \"pular\" não existe no projeto; \
          quis dizer Pular? A comparação é com o nome exato, com maiúsculas e espaços"
        ]
    );
}

/// A surface's name written to a property that takes one is checked as `param.surface`'s, in
/// any case.
#[test]
fn surface_names_written_to_properties_are_checked() {
    let (project, _) = project();
    let text = "local M = {}\nfunction M.on_start(self)\n    self.collider.surface = 'Gelu'\n    \
                self.character.body_surface = ' gelo'\nend\nreturn M\n";
    let sources = ScriptSources::from_texts([(PATH, text)]);
    assert_eq!(
        lint(&sources, &project),
        [
            "scripts/porta.luau:3: obj.collider.surface: a superfície \"Gelu\" não existe no \
             projeto; quis dizer Gelo?"
        ]
    );
}

/// A project folder of its own, with the scripts `files` in it (the tools take the project as
/// the editor has it).
fn folder(files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("oxy-scripts-names-{}", new_id()));
    for (path, text) in files {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn scripts(root: &std::path::Path, project: &Project, args: Json) -> Json {
    let request = Request::parse(&args).unwrap();
    script_files::answer(root, Some(project), &request, &mut HashSet::new())
        .unwrap()
        .json
}

#[test]
fn oxy_scripts_check_and_write_warn_of_the_names_the_project_lacks() {
    let (project, _) = project();
    let root = folder(&[(PATH, DOOR), ORDER]);
    let checked = scripts(&root, &project, json!({"action": "check", "path": PATH}));
    assert_eq!(checked["ok"], true);
    assert_eq!(checked["warnings"], json!(door_warnings()));
    // Every script: the other one names nothing wrong.
    let all = scripts(&root, &project, json!({"action": "check"}));
    assert_eq!(all["warnings"], json!(door_warnings()));
    // A text as the script, without saving it; the loading warnings come first.
    let text = "local M = {}\nfunction M.on_aera_enter(self) end\nfunction M.on_start(self)\n    \
                audio.play('Bipee')\nend\nreturn M\n";
    let unsaved = scripts(
        &root,
        &project,
        json!({"action": "check", "path": PATH, "text": text}),
    );
    assert_eq!(
        unsaved["warnings"],
        json!([
            "scripts/porta.luau:2: on_aera_enter não é um evento dos scripts e nunca roda \
             sozinha; quis dizer on_area_enter?",
            "scripts/porta.luau:4: audio.play: o som \"Bipee\" não existe no projeto; quis dizer \
             Bipe?"
        ])
    );
    // The write saves it all the same, warning the same way.
    let written = scripts(
        &root,
        &project,
        json!({"action": "write", "path": "scripts/nova.luau", "text": text}),
    );
    assert_eq!(written["created"], true);
    assert_eq!(
        written["warnings"][1],
        "scripts/nova.luau:4: audio.play: o som \"Bipee\" não existe no projeto; quis dizer Bipe?"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn oxy_validate_names_the_misspelled_names_of_the_folders_scripts() {
    let (mut project, door) = project();
    project.scenes[0].entity_mut(&door).unwrap().scripts = vec![script::ScriptRef::new(PATH)];
    let root = folder(&[(PATH, DOOR), ORDER]);
    let validation = query::validation_in(&project, Some(&root));
    assert_eq!(validation["valid"], true);
    // The sounds whose files the folder lacks come first, as opening tells them.
    let mut warnings: Vec<String> = ["Checkpoint", "Bipe"]
        .iter()
        .map(|name| {
            format!(
                "{name} (som): o arquivo assets/{}.wav não existe na pasta do projeto; jogar e \
                 exportar ficam recusados enquanto o recurso for usado",
                name.to_lowercase()
            )
        })
        .collect();
    warnings.extend(door_warnings());
    assert_eq!(validation["warnings"], json!(warnings));
    // A project of nodes only, without the folder, validates as before.
    let (plain, _) = project_without_scripts();
    let nowhere = folder(&[]);
    assert_eq!(
        query::validation_in(&plain, Some(&nowhere)),
        json!({"valid": true})
    );
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::remove_dir_all(&nowhere).unwrap();
}

fn project_without_scripts() -> (Project, Id) {
    let mut project = Project::new("Só nós");
    let door = Entity::new("Porta", None);
    let id = door.id.clone();
    project.scenes[0].entities.push(door);
    (project, id)
}

fn attach(entity: &Id, path: &str) -> Command {
    Command::AttachScript {
        scene: None,
        entity: entity.clone(),
        path: path.into(),
        index: None,
    }
}

#[test]
fn attach_script_tells_the_names_the_script_gives_that_the_project_lacks() {
    let (mut project, door) = project();
    let gate = project.scenes[0].entities[2].id.clone();
    let sources = ScriptSources::from_texts([(PATH, DOOR), ORDER]);
    let scene = project.start_scene.clone();
    // Attached to two objects: told once, on the last attach of the script.
    let applied = command::apply_with_scripts(
        &mut project,
        &scene,
        &[attach(&door, PATH), attach(&gate, PATH)],
        &sources,
    )
    .unwrap();
    assert!(applied[0].warnings.is_empty(), "{:?}", applied[0].warnings);
    let told: Vec<String> = door_warnings()
        .into_iter()
        .map(|w| format!("Comando 1 (attach_script, o 2º do lote): {w}"))
        .collect();
    assert_eq!(applied[1].warnings, told);
    // A variable created later in the batch counts: the batch is checked as it ends.
    let text = "local M = {}\nfunction M.on_start(self)\n    vars.set('Recorde', 1)\nend\nreturn \
                M\n";
    let sources = ScriptSources::from_texts([("scripts/recorde.luau", text)]);
    let applied = command::apply_with_scripts(
        &mut project,
        &scene,
        &[
            attach(&door, "scripts/recorde.luau"),
            Command::CreateVariable {
                name: "Recorde".into(),
                value: ParamValue::Tagged(Value::Number(0.)),
                persistent: false,
            },
        ],
        &sources,
    )
    .unwrap();
    assert!(applied.iter().all(|a| a.warnings.is_empty()), "{applied:?}");
}

/// Tools see what each script names (stage 5: where a sound is used, what deleting it breaks).
#[test]
fn tools_see_the_names_each_script_gives_and_what_they_name() {
    let (project, _) = project();
    let checkpoint = project.assets[0].id.clone();
    let checked = script::check(PATH, DOOR).unwrap();
    let sounds: Vec<(&str, u32)> = checked
        .references
        .iter()
        .filter(|r| r.kind == NameKind::Sound)
        .map(|r| (r.name.as_str(), r.line))
        .collect();
    assert_eq!(sounds, [("Checkpointt", 5), ("Bipe", 6)]);
    assert_eq!(script::references(PATH, DOOR).unwrap(), checked.references);
    let sources = ScriptSources::from_texts([(PATH, DOOR), ORDER]);
    let declared = script::declared_attribute_names(&sources);
    assert_eq!(
        declared.iter().map(String::as_str).collect::<Vec<_>>(),
        ["Aberta", "Ordem"]
    );
    let names = ProjectNames::new(&project, declared);
    let mut text =
        "local M = {}\nfunction M.on_start(self)\n    audio.play(' checkpoint')\nend\n".to_owned();
    text.push_str("return M\n");
    let given = script::references("scripts/som.luau", &text).unwrap();
    assert_eq!(names.find(&given[0]), Some(Found::Id(checkpoint)));
    // Every kind of name a parameter takes is one of NameKind.
    for kind in NameKind::ALL {
        assert!(!kind.what().is_empty() && kind.checked().ends_with('.'));
    }
    // A script that does not compile names nothing, and says why.
    assert!(script::references(PATH, "local x = (\n").is_err());
}
