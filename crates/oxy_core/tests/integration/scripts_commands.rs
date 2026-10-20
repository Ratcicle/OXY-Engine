//! attach_script and detach_script (decision Q6 of 0.7): a script declares the attributes it
//! expects, `Module.attributes = { Nome = padrão }`, and attaching it gives the object those it
//! lacks in the same undo step; type conflicts are warnings that name the scripts, and
//! detaching keeps the attributes.
use oxy_core::{
    command::{self, Command, CommandError},
    document::*,
    edit_history::CommandHistory,
    runtime::{Runtime, RuntimeOptions},
    script::ScriptSources,
    texture_cache::TextureCache,
};
use std::sync::Arc;

const SPIN: &str = r#"local Spin = {}
Spin.attributes = { Velocidade = 90, Eixo = vector.create(0, 1, 0), Ativo = true, Rotulo = "giro" }
function Spin.on_step(self, dt) end
return Spin
"#;

/// A door, and the scripts a project's folder could hold.
fn door() -> (Project, Id, ScriptSources) {
    let mut project = Project::new("Anexar");
    let door = Entity::new("Porta", None);
    let id = door.id.clone();
    project.scenes[0].entities.push(door);
    let sources = ScriptSources::from_texts([
        ("scripts/girar.luau", SPIN),
        (
            "scripts/numero.luau",
            "local M = {}\nM.attributes = { Velocidade = 10, Vida = 3 }\nreturn M\n",
        ),
        (
            "scripts/texto.luau",
            "local M = {}\nM.attributes = { Velocidade = 'alta' }\nreturn M\n",
        ),
        ("scripts/sem.luau", "return {}\n"),
        (
            "scripts/quebrado.luau",
            "local M = {}\nM.attributes = {\nreturn M\n",
        ),
        (
            "scripts/tabela.luau",
            "local M = {}\nM.attributes = { Alvo = {} }\nreturn M\n",
        ),
    ]);
    (project, id, sources)
}

fn attach(entity: &Id, path: &str, index: Option<usize>) -> Command {
    Command::AttachScript {
        scene: None,
        entity: entity.clone(),
        path: path.into(),
        index,
    }
}

fn detach(entity: &Id, path: &str) -> Command {
    Command::DetachScript {
        scene: None,
        entity: entity.clone(),
        path: path.into(),
    }
}

/// Applies `commands` with `sources`, answering each command's warnings.
fn apply(
    project: &mut Project,
    sources: &ScriptSources,
    commands: &[Command],
) -> Result<Vec<Vec<String>>, CommandError> {
    let scene = project.start_scene.clone();
    command::apply_with_scripts(project, &scene, commands, sources)
        .map(|applied| applied.into_iter().map(|a| a.warnings).collect())
}

fn entity<'a>(project: &'a Project, id: &str) -> &'a Entity {
    project.scenes[0].entity(id).unwrap()
}

fn scripts(project: &Project, id: &str) -> Vec<String> {
    entity(project, id)
        .scripts
        .iter()
        .map(|s| s.path.clone())
        .collect()
}

#[test]
fn attaching_creates_the_declared_attributes_in_one_undo_step() {
    let (mut project, id, sources) = door();
    // An attribute the object has keeps its value.
    project.scenes[0]
        .entity_mut(&id)
        .unwrap()
        .attributes
        .insert("Velocidade".into(), Value::Number(45.));
    let before = project.clone();
    let mut images = TextureCache::default();
    let mut history = CommandHistory::new();
    let commands = [attach(&id, "scripts/girar.luau", None)];
    history.begin(commands[0].label(), &project, &images);
    let warnings = apply(&mut project, &sources, &commands).unwrap();
    assert_eq!(warnings, [Vec::<String>::new()]);
    history.commit(&project, &mut images).unwrap();
    let attached = project.clone();
    assert_eq!(scripts(&project, &id), ["scripts/girar.luau"]);
    let attributes = &entity(&project, &id).attributes;
    assert_eq!(
        attributes.iter().collect::<Vec<_>>(),
        [
            (&"Ativo".to_owned(), &Value::Bool(true)),
            (&"Eixo".to_owned(), &Value::Vector3([0., 1., 0.])),
            (&"Rotulo".to_owned(), &Value::Text("giro".into())),
            (&"Velocidade".to_owned(), &Value::Number(45.)),
        ]
    );
    assert_eq!(history.undo_label(), Some("Anexar script"));
    // One undo takes the script and the attributes it created away; redo brings both.
    history.undo(&mut project, &mut images).unwrap();
    assert_eq!(project, before);
    history.redo(&mut project, &mut images).unwrap();
    assert_eq!(project, attached);
    // The document keeps the reference.
    let json = serde_json::to_value(entity(&project, &id)).unwrap();
    assert_eq!(
        json["scripts"],
        serde_json::json!([{"path": "scripts/girar.luau"}])
    );
}

#[test]
fn type_conflicts_are_warned_naming_both_scripts_or_the_attribute() {
    let (mut project, id, sources) = door();
    apply(
        &mut project,
        &sources,
        &[attach(&id, "scripts/numero.luau", None)],
    )
    .unwrap();
    let warnings = apply(
        &mut project,
        &sources,
        &[attach(&id, "scripts/texto.luau", None)],
    )
    .unwrap();
    assert_eq!(
        warnings,
        [vec![
            "Comando 0 (attach_script, o 1º do lote): Porta: os scripts declaram o atributo \
             Velocidade com tipos diferentes (scripts/numero.luau: número; scripts/texto.luau: \
             texto); o atributo ficou como estava (número)"
                .to_owned()
        ]]
    );
    assert_eq!(
        entity(&project, &id).attributes["Velocidade"],
        Value::Number(10.)
    );
    // Both in one batch: one warning, on the last attach.
    let (mut project, id, sources) = door();
    let warnings = apply(
        &mut project,
        &sources,
        &[
            attach(&id, "scripts/numero.luau", None),
            attach(&id, "scripts/texto.luau", None),
        ],
    )
    .unwrap();
    assert!(warnings[0].is_empty(), "{warnings:?}");
    assert_eq!(warnings[1].len(), 1, "{warnings:?}");
    // Once the other script goes, the conflict goes with it.
    let warnings = apply(
        &mut project,
        &sources,
        &[
            detach(&id, "scripts/numero.luau"),
            attach(&id, "scripts/texto.luau", None),
        ],
    )
    .unwrap();
    assert_eq!(
        warnings[1],
        [
            "Comando 1 (attach_script, o 2º do lote): Porta: o atributo Velocidade é número, mas \
             scripts/texto.luau o declara como texto; o atributo ficou como estava"
        ]
    );
}

#[test]
fn attaching_again_moves_and_fills_and_detaching_keeps_the_attributes() {
    let (mut project, id, sources) = door();
    apply(
        &mut project,
        &sources,
        &[
            attach(&id, "scripts/numero.luau", None),
            // Written as Windows writes it.
            attach(&id, r"scripts\girar.luau", None),
        ],
    )
    .unwrap();
    assert_eq!(
        scripts(&project, &id),
        ["scripts/numero.luau", "scripts/girar.luau"]
    );
    // Again with an index: it moves, and nothing doubles.
    apply(
        &mut project,
        &sources,
        &[attach(&id, "scripts/girar.luau", Some(0))],
    )
    .unwrap();
    assert_eq!(
        scripts(&project, &id),
        ["scripts/girar.luau", "scripts/numero.luau"]
    );
    // An attribute deleted since comes back, with its default; the others keep their values.
    let e = project.scenes[0].entity_mut(&id).unwrap();
    e.attributes.remove("Vida");
    e.attributes.insert("Ativo".into(), Value::Bool(false));
    apply(
        &mut project,
        &sources,
        &[attach(&id, "scripts/numero.luau", None)],
    )
    .unwrap();
    assert_eq!(
        scripts(&project, &id),
        ["scripts/girar.luau", "scripts/numero.luau"]
    );
    let attributes = &entity(&project, &id).attributes;
    assert_eq!(attributes["Vida"], Value::Number(3.));
    assert_eq!(attributes["Ativo"], Value::Bool(false));
    // A new one at an index.
    apply(
        &mut project,
        &sources,
        &[attach(&id, "scripts/sem.luau", Some(1))],
    )
    .unwrap();
    assert_eq!(
        scripts(&project, &id),
        [
            "scripts/girar.luau",
            "scripts/sem.luau",
            "scripts/numero.luau"
        ]
    );
    // Detaching keeps every attribute.
    let kept = entity(&project, &id).attributes.clone();
    apply(&mut project, &sources, &[detach(&id, "scripts/girar.luau")]).unwrap();
    assert_eq!(
        scripts(&project, &id),
        ["scripts/sem.luau", "scripts/numero.luau"]
    );
    assert_eq!(entity(&project, &id).attributes, kept);
}

#[test]
fn what_cannot_be_attached_is_refused_and_a_broken_script_is_warned_about() {
    let (mut project, id, sources) = door();
    let refused = |project: &mut Project, command: Command| {
        let before = project.clone();
        let error = apply(project, &sources, &[command])
            .unwrap_err()
            .to_string();
        assert_eq!(*project, before);
        error
    };
    assert_eq!(
        refused(&mut project, attach(&id, "scripts/nada.luau", None)),
        "Comando 0 (attach_script, o 1º do lote): o script scripts/nada.luau não existe na pasta \
         do projeto. Nada foi alterado."
    );
    assert!(
        refused(&mut project, attach(&id, "../girar.luau", None))
            .contains("caminho de script inválido \"../girar.luau\"")
    );
    assert_eq!(
        refused(&mut project, attach(&id, "scripts/girar.luau", Some(1))),
        "Comando 0 (attach_script, o 1º do lote): index 1 fica fora da lista de scripts de Porta \
         (de 0 a 0). Nada foi alterado."
    );
    assert_eq!(
        refused(&mut project, detach(&id, "scripts/girar.luau")),
        "Comando 0 (detach_script, o 1º do lote): Porta não tem o script scripts/girar.luau. \
         Nada foi alterado."
    );
    // Without the project's scripts, nothing is attached.
    let scene = project.start_scene.clone();
    let error = command::apply(
        &mut project,
        &scene,
        &[attach(&id, "scripts/girar.luau", None)],
    )
    .unwrap_err();
    assert!(
        error.message.contains("não existe na pasta do projeto"),
        "{error}"
    );
    assert!(!command::needs_scripts(&[detach(
        &id,
        "scripts/girar.luau"
    )]));
    assert!(command::needs_scripts(&[attach(
        &id,
        "scripts/girar.luau",
        None
    )]));
    // A script that does not load, or whose declaration is no table of defaults, is attached,
    // its attributes unread, and the warning says why: fixed, attaching it again creates them.
    for (path, reason) in [
        (
            "scripts/quebrado.luau",
            "scripts/quebrado.luau:3: erro de sintaxe",
        ),
        (
            "scripts/tabela.luau",
            "Modulo.attributes.Alvo: uma tabela não serve de valor padrão",
        ),
    ] {
        let warnings = apply(&mut project, &sources, &[attach(&id, path, None)]).unwrap();
        assert_eq!(warnings[0].len(), 1, "{warnings:?}");
        let warning = &warnings[0][0];
        assert!(
            warning.contains(&format!(
                "Porta: os atributos que {path} declara não foram criados"
            )) && warning.contains(reason),
            "{warning}"
        );
    }
    assert_eq!(
        scripts(&project, &id),
        ["scripts/quebrado.luau", "scripts/tabela.luau"]
    );
    let fixed = ScriptSources::from_texts([(
        "scripts/tabela.luau",
        "local M = {}\nM.attributes = { Alvo = 'ninguém' }\nreturn M\n",
    )]);
    let warnings = apply(
        &mut project,
        &fixed,
        &[attach(&id, "scripts/tabela.luau", None)],
    )
    .unwrap();
    assert!(warnings[0].is_empty(), "{warnings:?}");
    assert_eq!(
        entity(&project, &id).attributes["Alvo"],
        Value::Text("ninguém".into())
    );
    // The commands are data, as agents write them.
    let parsed = command::parse(&serde_json::json!([
        {"op": "attach_script", "entity": &id, "path": "scripts/sem.luau", "index": 0},
        {"op": "detach_script", "entity": &id, "path": "scripts/sem.luau"},
    ]))
    .unwrap();
    assert_eq!(
        parsed,
        [
            attach(&id, "scripts/sem.luau", Some(0)),
            detach(&id, "scripts/sem.luau")
        ]
    );
    assert_eq!(parsed[1].label(), "Remover script");
}

#[test]
fn only_an_attached_script_declares_attributes() {
    // A table of content, only required, may hold an attributes of its own.
    let files = [
        (
            "scripts/dados/classes.luau",
            "return { attributes = { \"Força\", \"Agilidade\" } }\n",
        ),
        (
            "scripts/heroi.luau",
            "local Classes = require('./dados/classes')\nlocal M = {}\nM.attributes = { Vida = 3 \
             }\nfunction M.on_start(self) print(Classes.attributes[1], self.attr.Vida) end\n\
             return M\n",
        ),
        (
            "scripts/solto.luau",
            "local M = {}\nM.attributes = { Alvo = {} }\nfunction M.on_start(self) print('roda') \
             end\nreturn M\n",
        ),
    ];
    let sources = ScriptSources::from_texts(files);
    let mut project = Project::new("Dados");
    let hero = Entity::new("Herói", None);
    let loose = Entity::new("Solto", None);
    let (hero_id, loose_id) = (hero.id.clone(), loose.id.clone());
    project.scenes[0].entities.extend([hero, loose]);
    let warnings = apply(
        &mut project,
        &sources,
        &[
            attach(&hero_id, "scripts/heroi.luau", None),
            attach(&loose_id, "scripts/solto.luau", None),
        ],
    )
    .unwrap();
    assert_eq!(
        warnings,
        [
            vec![],
            vec![
                "Comando 1 (attach_script, o 2º do lote): Solto: os atributos que \
                 scripts/solto.luau declara não foram criados (scripts/solto.luau: \
                 Modulo.attributes.Alvo: uma tabela não serve de valor padrão; use um número, um \
                 texto, um booleano, um vetor, param.object() (um objeto escolhido depois) ou \
                 param.vector2(x, y)); corrija o script e anexe-o de novo para criá-los"
                    .to_owned()
            ]
        ]
    );
    assert_eq!(
        entity(&project, &hero_id).attributes["Vida"],
        Value::Number(3.)
    );
    // Attached, the same table is a declaration, and not one attaching can create.
    let warnings = apply(
        &mut project,
        &sources,
        &[attach(&loose_id, "scripts/dados/classes.luau", None)],
    )
    .unwrap();
    assert!(
        warnings[0][0].contains("Modulo.attributes: os nomes dos atributos são textos"),
        "{warnings:?}"
    );
    // Every script runs; where it is attached, a declaration that creates nothing is told once.
    let options = RuntimeOptions {
        scripts: Arc::new(sources),
        ..Default::default()
    };
    let rt = Runtime::with_options(&project, &project.start_scene, options).unwrap();
    let lines: Vec<&str> = rt.logs.iter().map(|l| l.text.as_str()).collect();
    const CREATES_NOTHING: &str = "; anexar o script não cria os atributos que ele declara.";
    assert_eq!(
        lines,
        [
            format!(
                "scripts/solto.luau: Modulo.attributes.Alvo: uma tabela não serve de valor \
                 padrão; use um número, um texto, um booleano, um vetor, param.object() (um \
                 objeto escolhido depois) ou param.vector2(x, y){CREATES_NOTHING}"
            ),
            format!(
                "scripts/dados/classes.luau: Modulo.attributes: os nomes dos atributos são \
                 textos, como {{ Velocidade = 90 }}{CREATES_NOTHING}"
            ),
            "Herói · Força 3".to_owned(),
            "Solto · roda".to_owned(),
        ]
    );
}

/// Objects go by name in the commands, as in the other tools: attach_script and detach_script,
/// the batches of oxy_simulate's setup and every command but the legacy node ones, which take
/// IDs. A name two objects of the command's scene share is refused with their IDs, and one only
/// other scenes have names those scenes; a name given to a node command says where its object
/// is and that they take its ID.
#[test]
fn commands_take_objects_by_name_but_the_legacy_node_ones() {
    let (mut project, id, sources) = door();
    let mut lever = Entity::new("Alavanca", None);
    lever.parent = Some(id.clone());
    let mut twin = Entity::new("Alavanca", None);
    twin.attributes.insert("Vida".into(), Value::Number(1.));
    let (lever_id, twin_id) = (lever.id.clone(), twin.id.clone());
    project.scenes[0].entities.extend([lever, twin]);
    // Other scenes have a button, another door and two signs.
    let mut menu = Scene::new("Menu", SceneKind::ThreeD);
    let (button, sign) = (Entity::new("Botão", None), Entity::new("Placa", None));
    let (button_id, sign_id) = (button.id.clone(), sign.id.clone());
    menu.entities.extend([button, sign]);
    let mut stage = Scene::new("Fase", SceneKind::TwoD);
    let other_sign = Entity::new("Placa", None);
    let other_sign_id = other_sign.id.clone();
    stage
        .entities
        .extend([Entity::new("Porta", None), other_sign]);
    project.scenes.extend([menu, stage]);
    let commands = |json: serde_json::Value| command::parse(&json).unwrap();
    let mut applied = project.clone();
    apply(
        &mut applied,
        &sources,
        &commands(serde_json::json!([
            {"op": "attach_script", "entity": " porta ", "path": "scripts/girar.luau"},
            {"op": "attach_script", "entity": "PORTA", "path": "scripts/numero.luau"},
            {"op": "detach_script", "entity": "Porta", "path": "scripts/girar.luau"},
            {"op": "update_entity", "entity": "Porta", "patch": {"visible": false}},
            {"op": "duplicate_entities", "ids": ["Porta"]},
        ])),
    )
    .unwrap();
    assert_eq!(scripts(&applied, &id), ["scripts/numero.luau"]);
    let door = entity(&applied, &id);
    assert_eq!(door.attributes["Vida"], Value::Number(3.));
    assert!(!door.visible);
    // The door and the lever under it, copied.
    assert_eq!(
        applied.scenes[0].entities.len(),
        project.scenes[0].entities.len() + 2
    );
    // The usual refusals, with nothing changed.
    let refused = |command: serde_json::Value| {
        let mut copy = project.clone();
        let error = apply(&mut copy, &sources, &commands(command))
            .unwrap_err()
            .to_string();
        assert_eq!(copy, project);
        error
    };
    let error = refused(serde_json::json!(
        {"op": "attach_script", "entity": "Alavanca", "path": "scripts/girar.luau"}
    ));
    assert_eq!(
        error,
        format!(
            "Comando 0 (attach_script, o 1º do lote): entity: há 2 objetos chamados \"Alavanca\" \
             na cena Cena 2D; use o ID: {lever_id} (em Porta), {twin_id} (na raiz da cena). \
             Nada foi alterado."
        )
    );
    assert!(
        refused(serde_json::json!({"op": "delete_entities", "ids": ["Porta", "alavanca"]}))
            .contains("ids: há 2 objetos chamados \"alavanca\""),
    );
    assert_eq!(
        refused(serde_json::json!({"op": "set_visible", "entity": "Botão", "visible": false})),
        "Comando 0 (set_visible, o 1º do lote): Botão não existe na cena Cena 2D (a do comando); \
         ele está na cena Menu: passe scene. Nada foi alterado."
    );
    assert_eq!(
        refused(serde_json::json!(
            {"op": "detach_script", "entity": "Janela", "path": "scripts/girar.luau"}
        )),
        "Comando 0 (detach_script, o 1º do lote): Objeto Janela não existe na cena Cena 2D (a do \
         comando) nem em outra cena do projeto. Nada foi alterado."
    );
    // A name objects of other scenes share lists them.
    assert_eq!(
        refused(serde_json::json!({"op": "set_visible", "entity": "Placa", "visible": false})),
        format!(
            "Comando 0 (set_visible, o 1º do lote): Placa não existe na cena Cena 2D (a do \
             comando); outras cenas têm 2 objetos com esse nome: {sign_id} (cena Menu), \
             {other_sign_id} (cena Fase): passe scene. Nada foi alterado."
        )
    );
    // The legacy node commands keep taking IDs, and say so, the object of the command's scene
    // first (another scene has a door too), and never only "pass scene".
    let node = |entity: &str, scene: Option<&str>| {
        let mut node = serde_json::json!(
            {"op": "add_node", "entity": entity, "operation": "event.scene_start"}
        );
        if let Some(scene) = scene {
            node["scene"] = scene.into();
        }
        refused(node)
    };
    assert_eq!(
        node("Porta", None),
        format!(
            "Comando 0 (add_node, o 1º do lote): Porta é o nome de um objeto da cena Cena 2D; os \
             comandos de nós (legado) pedem o ID dele: {id}. Nada foi alterado."
        )
    );
    assert_eq!(
        node("Botão", None),
        format!(
            "Comando 0 (add_node, o 1º do lote): Botão é o nome de um objeto da cena Menu, e não \
             da Cena 2D (a do comando); os comandos de nós (legado) pedem o ID dele ({button_id}) \
             e a cena dele em scene. Nada foi alterado."
        )
    );
    assert_eq!(
        node("Botão", Some("Menu")),
        format!(
            "Comando 0 (add_node, o 1º do lote): Botão é o nome de um objeto da cena Menu; os \
             comandos de nós (legado) pedem o ID dele: {button_id}. Nada foi alterado."
        )
    );
    assert!(
        node("alavanca", None)
            .contains("entity: há 2 objetos chamados \"alavanca\" na cena Cena 2D; use o ID: ")
    );
    assert_eq!(
        node("Placa", None),
        format!(
            "Comando 0 (add_node, o 1º do lote): Placa não existe na cena Cena 2D (a do comando); \
             outras cenas têm 2 objetos com esse nome: {sign_id} (cena Menu), {other_sign_id} \
             (cena Fase); os comandos de nós (legado) pedem o ID e a cena do objeto em scene. \
             Nada foi alterado."
        )
    );
    // In the scene the command names (copy_entities' parent, in the scene it copies to), and by
    // the IDs as ever, which the node commands take with their scene.
    let mut moved = project.clone();
    apply(
        &mut moved,
        &sources,
        &commands(serde_json::json!([
            {"op": "set_visible", "scene": "Menu", "entity": "botão", "visible": false},
            {"op": "copy_entities", "ids": [&twin_id], "to_scene": "Menu", "parent": "Botão"},
            {"op": "set_parent", "ids": [&twin_id], "parent": "Porta"},
            {
                "op": "add_node", "scene": "Menu", "entity": &button_id,
                "operation": "event.scene_start"
            },
        ])),
    )
    .unwrap();
    let menu = &moved.scenes[1].entities;
    assert!(!menu[0].visible);
    assert_eq!(menu[0].graph.nodes.len(), 1);
    assert_eq!(menu[2].parent.as_deref(), Some(menu[0].id.as_str()));
    assert_eq!(
        entity(&moved, &twin_id).parent.as_deref(),
        Some(id.as_str())
    );
}
