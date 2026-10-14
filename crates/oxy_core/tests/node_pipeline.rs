//! The contract every registered node keeps, and the integrity checks a node gets from
//! `graph::reference` alone: a scene, audio, clip, input action or game variable it names is
//! guarded like those of the nodes that came first.
use oxy_core::{
    command::{self, Command},
    document::*,
    editing,
    graph::{self, Graph, Node, PortType, Reference},
    guide, input_actions, query, variables,
};
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// Every (operation, parameter) whose parameter names a `kind` of the project.
fn naming(kind: Reference) -> Vec<(&'static str, &'static str)> {
    let found: Vec<_> = graph::registry()
        .iter()
        .flat_map(|def| {
            def.params
                .iter()
                .filter(move |param| graph::reference(def.id, param) == Some(kind))
                .map(move |param| (def.id, param.id))
        })
        .collect();
    assert!(!found.is_empty(), "nenhum nó nomeia {kind:?}");
    found
}

fn label(operation: &str) -> &'static str {
    graph::definition(operation).unwrap().label
}

/// A node of `operation` whose `param` names `id`.
fn node(operation: &str, param: &str, id: &str) -> Node {
    let mut node = Node::new(operation, [0.; 2]);
    node.params.insert(param.into(), Value::Text(id.into()));
    node
}

fn model(name: &str, pieces: Vec<Entity>) -> Asset {
    Asset {
        id: new_id(),
        name: name.into(),
        path: String::new(),
        kind: AssetKind::Model,
        model: Some(pieces),
    }
}

fn batch(value: serde_json::Value) -> Vec<Command> {
    command::parse(&value).unwrap()
}

#[test]
fn every_registered_operation_keeps_the_registry_contract() {
    let unknown = graph::description("operação.inexistente");
    let mut ids = HashSet::new();
    for def in graph::registry() {
        let id = def.id;
        assert!(ids.insert(id), "{id}: ID repetido no registro");
        assert!(
            std::ptr::eq(graph::definition(id).unwrap(), def),
            "{id}: definition"
        );
        if id.starts_with("event.") {
            assert!(def.inputs.is_empty(), "{id}: evento com entradas");
            assert!(
                def.outputs
                    .first()
                    .is_some_and(|port| port.id == "exec" && port.kind == PortType::Exec),
                "{id}: a primeira saída de um evento é Executar"
            );
        }
        let node = Node::new(id, [0.; 2]);
        let graph = Graph {
            nodes: vec![node.clone()],
            edges: vec![],
        };
        graph
            .validate(&HashSet::new())
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        graph::check_choices(id, &node.params).unwrap_or_else(|error| panic!("{id}: {error}"));
        for param in &def.params {
            if let Some(options) = graph::choices(id, param.id) {
                assert!(
                    matches!(&param.default, Value::Text(value)
                        if options.iter().any(|(option, _)| option == value)),
                    "{id}.{}: padrão fora das opções",
                    param.id
                );
            }
        }
        assert_ne!(graph::description(id), unknown, "{id}: sem descrição");
        let topic = guide::topics()
            .iter()
            .find(|topic| topic.operation.id == id)
            .unwrap_or_else(|| panic!("{id}: sem tópico no guia"));
        assert_ne!(
            topic.purpose,
            guide::UNDOCUMENTED,
            "{id}: sem ajuda no guia"
        );
    }
    assert!(graph::definition("operação.inexistente").is_none());
}

/// The catalog's description is all an agent reads to choose a node, and the guide's purpose
/// opens its help: each says what that node does, never a text shared with the nodes of its
/// family, and names the units its ports and parameters carry.
#[test]
fn every_node_has_its_own_description_with_its_units() {
    let unknown = graph::description("operação.inexistente");
    let mut descriptions = HashMap::new();
    let mut purposes = HashMap::new();
    for def in graph::registry() {
        let id = def.id;
        let description = graph::description(id);
        assert!(
            description != unknown && description != guide::UNDOCUMENTED,
            "{id}: descrição provisória"
        );
        if let Some(other) = descriptions.insert(description, id) {
            panic!("{id} e {other} têm a mesma descrição: {description}");
        }
        let topic = guide::topics()
            .iter()
            .find(|topic| topic.operation.id == id)
            .unwrap();
        if let Some(other) = purposes.insert(topic.purpose, id) {
            panic!(
                "{id} e {other} têm o mesmo propósito no guia: {}",
                topic.purpose
            );
        }
        // A choice may carry the unit too: the rotation of Mover / girar / escalar objeto.
        let labels = def
            .inputs
            .iter()
            .chain(&def.outputs)
            .map(|port| port.label)
            .chain(def.params.iter().map(|param| param.label))
            .chain(def.params.iter().flat_map(|param| {
                graph::choices(id, param.id)
                    .into_iter()
                    .flatten()
                    .map(|(_, label)| *label)
            }));
        for label in labels {
            for (unit, word) in [
                ("(°)", "graus"),
                ("(°/s)", "graus por segundo"),
                ("(m/s)", "m/s"),
                ("(m/s;", "m/s"),
                ("(m/s²)", "m/s²"),
                ("(m)", "metros"),
                ("(m;", "metros"),
                ("(s)", "segundos"),
                ("(ms)", "milissegundos"),
            ] {
                assert!(
                    !label.contains(unit) || description.contains(word),
                    "{id}: a descrição não diz a unidade de {label} ({word})"
                );
            }
        }
    }
}

#[test]
fn node_references_report_every_parameter_that_names_a_project_item() {
    for def in graph::registry() {
        let node = Node::new(def.id, [0.; 2]);
        let reported: Vec<_> = graph::node_references(&node)
            .map(|(param, reference, value)| {
                assert_eq!(value, &node.params[param.id], "{}.{}", def.id, param.id);
                (param.id, reference)
            })
            .collect();
        let expected: Vec<_> = def
            .params
            .iter()
            .filter_map(|param| Some((param.id, graph::reference(def.id, param)?)))
            .collect();
        assert_eq!(reported, expected, "{}", def.id);
    }
    // Every kind the guards check is named by some node, so the tests below check something.
    for kind in [
        Reference::Scene,
        Reference::Audio,
        Reference::Clip,
        Reference::InputAction,
        Reference::Variable,
    ] {
        for (operation, param) in naming(kind) {
            let node = Node::new(operation, [0.; 2]);
            assert!(
                graph::node_references(&node)
                    .any(|(found, reference, _)| found.id == param && reference == kind),
                "{operation}.{param}"
            );
        }
    }
    // A node the registry does not know names nothing.
    let mut stray = Node::new("debug.message", [0.; 2]);
    stray.operation = "operação.inexistente".into();
    assert_eq!(graph::node_references(&stray).count(), 0);
}

#[test]
fn a_scene_named_by_any_node_is_guarded_in_scenes_and_models() {
    for (operation, param) in naming(Reference::Scene) {
        let mut project = editing::blank_project("Cenas", SceneKind::ThreeD).unwrap();
        let first = project.start_scene.clone();
        let level = Scene::new("Fase 2", SceneKind::ThreeD);
        let second = level.id.clone();
        project.scenes.push(level);
        let mut owner = Entity::new("Portal", None);
        owner.graph.nodes.push(node(operation, param, &second));
        let owner_id = owner.id.clone();
        project.scene_mut(&first).unwrap().entities.push(owner);
        validate_project(&project).unwrap();
        let before = project.clone();
        let error = editing::delete_scene(&mut project, &second).unwrap_err();
        assert!(
            error.contains(&format!("Cena 3D → Portal → {}", label(operation))),
            "{operation}: {error}"
        );
        assert_eq!(project, before);

        // The same node kept in a model, which can be placed anywhere later.
        let pieces = std::mem::take(&mut project.scene_mut(&first).unwrap().entities);
        project.assets.push(model("Portal salvo", pieces));
        validate_project(&project).unwrap();
        let error = editing::delete_scene(&mut project, &second).unwrap_err();
        assert!(
            error.contains(&format!("Portal salvo → Portal → {}", label(operation))),
            "{operation}: {error}"
        );

        // A scene that does not exist is a broken reference, in a scene as in a model.
        let unknown = Value::Text("cena-inexistente".into());
        let mut in_scene = before.clone();
        in_scene.scene_mut(&first).unwrap().entities[0].graph.nodes[0]
            .params
            .insert(param.into(), unknown.clone());
        let mut in_model = project.clone();
        in_model.assets.last_mut().unwrap().model.as_mut().unwrap()[0]
            .graph
            .nodes[0]
            .params
            .insert(param.into(), unknown);
        for (broken, holder) in [(in_scene, ""), (in_model, "modelo Portal salvo, ")] {
            let error = validate_project(&broken).unwrap_err();
            assert!(
                error.starts_with(&format!("Referência quebrada: {holder}objeto Portal, nó ")),
                "{operation}: {error}"
            );
        }

        // A copy into the other level still opens the level it left.
        let mut project = before;
        project
            .scene_mut(&first)
            .unwrap()
            .entity_mut(&owner_id)
            .unwrap()
            .graph
            .nodes[0]
            .params
            .insert(param.into(), Value::Text(first.clone()));
        let applied = command::apply(
            &mut project,
            &first,
            &batch(json!([{"op": "copy_entities", "ids": [owner_id], "to_scene": second}])),
        )
        .unwrap();
        let copy = &applied[0].created[0];
        let copied = &project
            .scene(&second)
            .unwrap()
            .entity(copy)
            .unwrap()
            .graph
            .nodes[0];
        let warning = applied[0].warnings.join("\n");
        assert!(
            warning.contains(&format!("→ nó {operation} ({})", copied.id))
                && warning.contains(&format!("set_node_params com {param} ")),
            "{operation}: {warning}"
        );
    }
}

#[test]
fn an_input_action_named_by_any_node_is_listed_where_it_is_used() {
    for (operation, param) in naming(Reference::InputAction) {
        let mut project = editing::blank_project("Entradas", SceneKind::TwoD).unwrap();
        let action = input_actions::create(&mut project, "Pular", "Space").unwrap();
        let mut owner = Entity::new("Herói", None);
        owner.graph.nodes.push(node(operation, param, &action));
        let owner_id = owner.id.clone();
        project.scenes[0].entities.push(owner.clone());
        let mut piece = owner;
        piece.id = new_id();
        let piece_id = piece.id.clone();
        project.assets.push(model("Herói salvo", vec![piece]));
        validate_project(&project).unwrap();
        assert!(command::warnings(&project).is_empty(), "{operation}");

        let summary = query::project_summary(&project);
        assert_eq!(
            summary["input_actions"][0]["used_by"],
            json!([
                {"entity": owner_id, "name": "Herói", "role": operation},
                {"entity": piece_id, "name": "Herói", "role": operation, "model": "Herói salvo"},
            ]),
            "{operation}"
        );
        let role = format!("nó {}", label(operation));
        assert_eq!(
            input_actions::references(&project, &action),
            [
                format!("Cena 2D → Herói → {role}"),
                format!("Herói salvo → Herói → {role}"),
            ],
            "{operation}"
        );
        let before = project.clone();
        assert!(
            input_actions::remove(&mut project, &action)
                .unwrap_err()
                .contains(&role),
            "{operation}"
        );
        assert_eq!(project, before);

        // Naming an action the project lacks is only a warning: the node never fires.
        project.scenes[0].entities[0].graph.nodes[0]
            .params
            .insert(param.into(), Value::Text("acao-inexistente".into()));
        let warnings = command::warnings(&project);
        assert!(
            warnings.iter().any(|w| w
                .message
                .contains("a ação de entrada \"acao-inexistente\" não existe no projeto")),
            "{operation}: {warnings:?}"
        );
    }
}

#[test]
fn a_clip_named_by_any_node_is_guarded_in_its_scene() {
    for (operation, param) in naming(Reference::Clip) {
        let mut project = editing::blank_project("Animações", SceneKind::ThreeD).unwrap();
        let scene = project.start_scene.clone();
        let mut actor = Entity::new("Ator", None);
        let clip = oxy_core::animation::Clip::new("Ataque");
        let clip_id = clip.id.clone();
        actor.clips.push(clip);
        let mut hand = Entity::new("Mão", None);
        hand.parent = Some(actor.id.clone());
        hand.graph.nodes.push(node(operation, param, &clip_id));
        let (actor_id, hand_id) = (actor.id.clone(), hand.id.clone());
        project.scenes[0].entities.extend([actor, hand]);
        validate_project(&project).unwrap();

        let before = project.clone();
        let error = command::apply(
            &mut project,
            &scene,
            &batch(json!([{"op": "delete_clip", "entity": actor_id, "clip": "Ataque"}])),
        )
        .unwrap_err();
        assert!(
            error.message.contains(&format!(
                "Cena 3D → Mão → {} ({})",
                label(operation),
                project.scenes[0].entities[1].graph.nodes[0].id
            )),
            "{operation}: {}",
            error.message
        );
        assert_eq!(project, before);

        // The hand alone would play a clip it does not take along.
        let error = command::apply(
            &mut project,
            &scene,
            &batch(json!([{"op": "save_model", "entity": hand_id}])),
        )
        .unwrap_err();
        assert!(
            error.message.contains(&format!(
                "Mão → nó {operation} → animação de outro objeto ({clip_id})"
            )),
            "{operation}: {}",
            error.message
        );

        // A model may still name the clip of an object it is placed with, as the editor
        // saves it (save_model validates the project); in a scene, a clip nobody has is a
        // broken reference.
        let saved = project.save_model(&scene, &hand_id, "Mão salva").unwrap();
        let pieces = project.asset(&saved).unwrap().model.as_ref().unwrap();
        assert_eq!(pieces[0].graph.nodes[0].text(param), clip_id, "{operation}");
        assert!(pieces.iter().all(|piece| piece.clips.is_empty()));
        let mut broken = before;
        broken.scenes[0].entities[1].graph.nodes[0]
            .params
            .insert(param.into(), Value::Text("clip-inexistente".into()));
        assert!(
            validate_project(&broken)
                .unwrap_err()
                .contains("Referência quebrada"),
            "{operation}"
        );
    }
}

#[test]
fn an_audio_named_by_any_node_must_be_a_project_wav() {
    for (operation, param) in naming(Reference::Audio) {
        let mut project = editing::blank_project("Sons", SceneKind::TwoD).unwrap();
        let mut sounds = |kind: AssetKind| {
            let asset = Asset {
                id: new_id(),
                name: format!("{kind:?}"),
                path: "assets/som.wav".into(),
                kind,
                model: None,
            };
            let id = asset.id.clone();
            project.assets.push(asset);
            id
        };
        let wav = sounds(AssetKind::Audio);
        let picture = sounds(AssetKind::Texture);
        let mut owner = Entity::new("Sino", None);
        owner.graph.nodes.push(node(operation, param, &wav));
        let pieces = vec![owner.clone()];
        project.scenes[0].entities.push(owner);
        validate_project(&project).unwrap();
        let before = project.clone();
        assert!(
            editing::remove_asset(&mut project, &wav)
                .unwrap_err()
                .contains(label(operation)),
            "{operation}"
        );
        assert_eq!(project, before);
        for named in [picture.as_str(), "som-inexistente"] {
            let mut broken = before.clone();
            broken.scenes[0].entities[0].graph.nodes[0]
                .params
                .insert(param.into(), Value::Text(named.into()));
            assert!(
                validate_project(&broken)
                    .unwrap_err()
                    .contains("Referência quebrada"),
                "{operation}: {named}"
            );
            // Kept in a model, it is checked all the same.
            let mut broken = before.clone();
            let mut pieces = pieces.clone();
            pieces[0].graph.nodes[0]
                .params
                .insert(param.into(), Value::Text(named.into()));
            broken.assets.push(model("Sino salvo", pieces));
            assert!(
                validate_project(&broken)
                    .unwrap_err()
                    .starts_with("Referência quebrada: modelo Sino salvo, objeto Sino, nó "),
                "{operation}: {named}"
            );
        }
    }
}

#[test]
fn a_variable_named_by_any_node_is_listed_where_it_is_used_and_must_exist() {
    for (operation, param) in naming(Reference::Variable) {
        let mut project = editing::blank_project("Variáveis", SceneKind::TwoD).unwrap();
        let start = project.start_scene.clone();
        let points = variables::create(&mut project, "Pontos", Value::Number(0.), false).unwrap();
        let mut owner = Entity::new("Placar", None);
        owner.graph.nodes.push(node(operation, param, &points));
        let owner_id = owner.id.clone();
        project.scenes[0].entities.push(owner.clone());
        let mut piece = owner;
        piece.id = new_id();
        let piece_id = piece.id.clone();
        project.assets.push(model("Placar salvo", vec![piece]));
        validate_project(&project).unwrap();
        assert!(command::warnings(&project).is_empty(), "{operation}");

        let summary = query::project_summary(&project);
        assert_eq!(
            summary["variables"][0]["used_by"],
            json!([
                {"entity": owner_id, "name": "Placar", "role": operation},
                {"entity": piece_id, "name": "Placar", "role": operation, "model": "Placar salvo"},
            ]),
            "{operation}"
        );
        let role = format!("nó {}", label(operation));
        assert_eq!(
            variables::references(&project, &points),
            [
                format!("Cena 2D → Placar → {role}"),
                format!("Placar salvo → Placar → {role}"),
            ],
            "{operation}"
        );
        let before = project.clone();
        let error = command::apply(
            &mut project,
            &start,
            &batch(json!([{"op": "delete_variable", "variable": "pontos"}])),
        )
        .unwrap_err();
        assert!(error.message.contains(&role), "{operation}: {error}");
        assert_eq!(project, before);

        // A variable the project lacks is a broken reference, in a scene as in a model.
        let unknown = Value::Text("variavel-inexistente".into());
        let mut in_scene = before.clone();
        in_scene.scenes[0].entities[0].graph.nodes[0]
            .params
            .insert(param.into(), unknown.clone());
        let mut in_model = before.clone();
        in_model.assets[0].model.as_mut().unwrap()[0].graph.nodes[0]
            .params
            .insert(param.into(), unknown);
        for (broken, holder) in [(in_scene, ""), (in_model, "modelo Placar salvo, ")] {
            let error = validate_project(&broken).unwrap_err();
            assert!(
                error.starts_with(&format!("Referência quebrada: {holder}objeto Placar, nó ")),
                "{operation}: {error}"
            );
        }
        // Commands take the name and keep the ID; an unknown name is refused with the list.
        let applied = command::apply(
            &mut project,
            &start,
            &batch(
                json!([{"op": "add_node", "entity": owner_id, "operation": operation,
                           "params": {param: "PONTOS"}}]),
            ),
        )
        .unwrap();
        let added = &project.scenes[0].entities[0].graph.nodes[1];
        assert_eq!(added.id, applied[0].created[0]);
        assert_eq!(added.text(param), points, "{operation}");
        let error = command::apply(
            &mut project,
            &start,
            &batch(
                json!([{"op": "add_node", "entity": owner_id, "operation": operation,
                           "params": {param: "Vidas"}}]),
            ),
        )
        .unwrap_err();
        assert!(
            error
                .message
                .contains("A variável do jogo \"Vidas\" não existe no projeto. Variáveis: Pontos"),
            "{operation}: {error}"
        );
    }
}
