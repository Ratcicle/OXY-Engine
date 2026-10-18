use oxy_core::{character::CharacterConfig, document::*, graph::Node, migration, persistence};

#[test]
fn legacy_load_never_writes_and_save_keeps_exact_backup_and_ids() {
    let root = std::env::temp_dir().join(format!("oxy-migration-{}", new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join(persistence::PROJECT_FILE);
    let mut original = Project::new("Antigo");
    original.schema_version = 1;
    original.scenes[0]
        .entities
        .push(Entity::new("Peça", Some(Primitive::Cube)));
    let bytes = serde_json::to_vec_pretty(&original).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    let converted = persistence::load_project_lazy(&file).unwrap();
    assert_eq!(converted.schema_version, SCHEMA_VERSION);
    assert_eq!(converted.id, original.id);
    assert_eq!(converted.scenes, original.scenes);
    assert_eq!(converted.input_bindings, original.input_bindings);
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    persistence::save_project(&file, &converted).unwrap();
    let backup = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("schema-1")
        })
        .unwrap();
    assert_eq!(std::fs::read(&backup).unwrap(), bytes);
    assert_eq!(persistence::load_project_lazy(&file).unwrap(), converted);
    persistence::save_project(&file, &converted).unwrap();
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn future_and_unknown_geometry_cannot_be_silently_ignored() {
    let mut json = serde_json::to_value(Project::new("Seguro")).unwrap();
    json["schema_version"] = 999.into();
    assert!(migration::read(&serde_json::to_vec(&json).unwrap()).is_err());
    json["schema_version"] = SCHEMA_VERSION.into();
    let mut entity = serde_json::to_value(Entity::new("Novo", None)).unwrap();
    entity["unknown_geometry"] = true.into();
    json["scenes"][0]["entities"] = serde_json::json!([entity]);
    assert!(migration::read(&serde_json::to_vec(&json).unwrap()).is_err());
}

#[test]
fn errors_are_typed_and_keep_their_messages() {
    let future = format!(r#"{{"schema_version": {}}}"#, SCHEMA_VERSION + 1);
    let error = migration::read(future.as_bytes()).unwrap_err();
    assert!(matches!(
        error,
        migration::MigrationError::UnsupportedVersion { found } if found == u64::from(SCHEMA_VERSION) + 1
    ));
    assert_eq!(
        error.to_string(),
        format!(
            "Versão de projeto {} incompatível; esta OXY Engine lê 1 a {SCHEMA_VERSION}. O arquivo foi preservado.",
            SCHEMA_VERSION + 1
        )
    );
    assert!(matches!(
        migration::read(br#"{"name": "sem versao"}"#),
        Err(migration::MigrationError::MissingVersion)
    ));
    let root = std::env::temp_dir();
    let error = persistence::resolve_asset_path(&root, "../fora.png").unwrap_err();
    assert!(matches!(
        error,
        persistence::PersistenceError::InvalidAssetPath(_)
    ));
    assert_eq!(error.to_string(), "Caminho de asset inválido: ../fora.png");
    let missing = root.join(format!("oxy-ausente-{}", new_id()));
    assert!(matches!(
        persistence::load_project(&missing),
        Err(persistence::PersistenceError::Open { .. })
    ));
    // Callers that still report text keep using `?` into String.
    let text: Result<(), String> = (|| Ok(persistence::load_project(&missing).map(drop)?))();
    assert!(text.unwrap_err().starts_with("Não foi possível abrir"));
}

/// A project whose damage nodes, in a scene and in a model, carry what the registry declares,
/// with a 3D character whose movement body schema 4 already holds.
fn spikes() -> Project {
    let mut project = Project::new("Espinhos");
    let scene = project.scenes[0].id.clone();
    let mut spikes = Entity::new("Espinhos", Some(Primitive::Cube));
    let mut damage = Node::new("action.damage", [0.; 2]);
    damage.params.insert("amount".into(), Value::Number(3.));
    let mut repeated = Node::new("action.damage", [0., 80.]);
    repeated.params.insert("once".into(), Value::Bool(false));
    spikes.graph.nodes.extend([damage, repeated]);
    let id = spikes.id.clone();
    project.scenes[0].entities.push(spikes);
    project.save_model(&scene, &id, "Espinhos salvos").unwrap();
    let mut course = Scene::new("Pista", SceneKind::ThreeD);
    let mut runner = Entity::new("Corredor", None);
    runner.character3d = Some(CharacterConfig::default());
    course.entities.push(runner);
    project.scenes.push(course);
    validate_project(&project).unwrap();
    project
}

/// `project` as a schema 4 file whose first damage nodes lack `once`, as a node lacks the
/// parameters added after its file was saved.
fn schema_4_without_once(project: &Project) -> Vec<u8> {
    let mut json = serde_json::to_value(project).unwrap();
    json["schema_version"] = 4.into();
    for params in [
        "/scenes/0/entities/0/graph/nodes/0/params",
        "/assets/0/model/0/graph/nodes/0/params",
    ] {
        let params = json.pointer_mut(params).unwrap().as_object_mut().unwrap();
        assert!(params.remove("once").is_some());
    }
    serde_json::to_vec_pretty(&json).unwrap()
}

#[test]
fn schema_4_nodes_get_the_registry_parameters_they_lack_and_keep_their_own() {
    assert_eq!(SCHEMA_VERSION, 5);
    let project = spikes();
    let (converted, from) = migration::read_report(&schema_4_without_once(&project)).unwrap();
    assert_eq!(from, Some(4));
    validate_project(&converted).unwrap();
    // The missing `once` reads its default; `amount` and the other node's `once` stay, and
    // the movement body schema 4 already holds is not converted again.
    assert_eq!(converted, project);
    for nodes in [
        &converted.scenes[0].entities[0].graph.nodes,
        &converted.assets[0].model.as_ref().unwrap()[0].graph.nodes,
    ] {
        assert_eq!(nodes[0].params["once"], Value::Bool(true));
        assert_eq!(nodes[0].params["amount"], Value::Number(3.));
        assert_eq!(nodes[1].params["once"], Value::Bool(false));
    }
}

#[test]
fn a_diagnostic_message_from_before_its_value_input_reads_and_runs_unchanged() {
    let mut project = Project::new("Mensagem antiga");
    let mut logic = Entity::new("Lógica", None);
    let start = Node::new("event.scene_start", [0.; 2]);
    let mut message = Node::new("debug.message", [260., 0.]);
    message
        .params
        .insert("message".into(), Value::Text("Olá".into()));
    logic.graph.edges.push(oxy_core::graph::Edge {
        from_node: start.id.clone(),
        from_port: "exec".into(),
        to_node: message.id.clone(),
        to_port: "exec".into(),
    });
    logic.graph.nodes = vec![start, message];
    project.scenes[0].entities.push(logic);
    for schema in [4, 5] {
        let mut json = serde_json::to_value(&project).unwrap();
        json["schema_version"] = schema.into();
        let converted = migration::read(&serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(converted, project, "schema {schema}");
        let node = &converted.scenes[0].entities[0].graph.nodes[1];
        assert_eq!(node.params.keys().collect::<Vec<_>>(), ["message"]);
        let runtime = oxy_core::runtime::Runtime::new(&converted, &converted.start_scene).unwrap();
        assert_eq!(runtime.logs.len(), 1, "{:?}", runtime.logs);
        assert!(
            runtime.logs[0].text.ends_with("] Olá"),
            "{:?}",
            runtime.logs
        );
    }
}

#[test]
fn unknown_operations_are_left_for_validation_to_report() {
    let mut project = spikes();
    let mut unknown = Node::new("action.damage", [0.; 2]);
    unknown.operation = "acao.do.futuro".into();
    unknown.params.clear();
    project.scenes[0].entities[0].graph.nodes.push(unknown);
    let mut json = serde_json::to_value(&project).unwrap();
    json["schema_version"] = 4.into();
    let converted = migration::read(&serde_json::to_vec(&json).unwrap()).unwrap();
    let node = converted.scenes[0].entities[0].graph.nodes.last().unwrap();
    assert!(node.params.is_empty());
    let error = validate_project(&converted).unwrap_err();
    assert!(
        error.contains("operação desconhecida acao.do.futuro"),
        "{error}"
    );
}

#[test]
fn schema_4_file_is_kept_on_load_and_its_first_save_writes_5_with_a_backup() {
    let root = std::env::temp_dir().join(format!("oxy-migration-4-{}", new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join(persistence::PROJECT_FILE);
    let project = spikes();
    let bytes = schema_4_without_once(&project);
    std::fs::write(&file, &bytes).unwrap();
    let (loaded, from) = persistence::load_project_lazy_report(&file).unwrap();
    assert_eq!((&loaded, from), (&project, Some(4)));
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    persistence::save_project(&file, &loaded).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved["schema_version"], 5);
    assert_eq!(
        saved
            .pointer("/scenes/0/entities/0/graph/nodes/0/params/once")
            .unwrap(),
        &serde_json::json!({"Bool": true})
    );
    let backups: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p != &file)
        .collect();
    assert_eq!(backups.len(), 1);
    let name = backups[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with("project.oxy.json.schema-4.") && name.ends_with(".backup.json"),
        "{name}"
    );
    assert_eq!(std::fs::read(&backups[0]).unwrap(), bytes);
    assert_eq!(
        persistence::load_project_lazy_report(&file).unwrap(),
        (project, None)
    );
    persistence::save_project(&file, &loaded).unwrap();
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn schema_5_documents_round_trip_unchanged() {
    let project = spikes();
    let bytes = serde_json::to_vec_pretty(&project).unwrap();
    let (read, from) = migration::read_report(&bytes).unwrap();
    assert_eq!((&read, from), (&project, None));
    assert_eq!(serde_json::to_vec_pretty(&read).unwrap(), bytes);
    let root = std::env::temp_dir().join(format!("oxy-migration-5-{}", new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join(persistence::PROJECT_FILE);
    std::fs::write(&file, &bytes).unwrap();
    persistence::save_project(&file, &persistence::load_project(&file).unwrap()).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn schema_5_nodes_are_not_filled_and_a_missing_parameter_is_refused() {
    // Only older files are filled: a current file without a parameter is broken, not old.
    let mut json: serde_json::Value =
        serde_json::from_slice(&schema_4_without_once(&spikes())).unwrap();
    json["schema_version"] = 5.into();
    let bytes = serde_json::to_vec_pretty(&json).unwrap();
    let (read, from) = migration::read_report(&bytes).unwrap();
    assert_eq!(from, None);
    assert!(
        !read.scenes[0].entities[0].graph.nodes[0]
            .params
            .contains_key("once")
    );
    let error = validate_project(&read).unwrap_err();
    assert!(
        error.contains("parâmetro obrigatório ausente once"),
        "{error}"
    );
    let root = std::env::temp_dir().join(format!("oxy-migration-5-incompleto-{}", new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join(persistence::PROJECT_FILE);
    std::fs::write(&file, &bytes).unwrap();
    assert!(persistence::load_project(&file).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn documents_newer_than_5_are_refused_and_never_overwritten() {
    let mut json = serde_json::to_value(spikes()).unwrap();
    json["schema_version"] = 6.into();
    let bytes = serde_json::to_vec_pretty(&json).unwrap();
    let error = migration::read(&bytes).unwrap_err();
    assert!(matches!(
        error,
        migration::MigrationError::UnsupportedVersion { found: 6 }
    ));
    assert_eq!(
        error.to_string(),
        "Versão de projeto 6 incompatível; esta OXY Engine lê 1 a 5. O arquivo foi preservado."
    );
    let root = std::env::temp_dir().join(format!("oxy-migration-6-{}", new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join(persistence::PROJECT_FILE);
    std::fs::write(&file, &bytes).unwrap();
    assert!(persistence::load_project(&file).is_err());
    assert!(matches!(
        persistence::save_project(&file, &spikes()),
        Err(persistence::PersistenceError::Migration(
            migration::MigrationError::PreviousNewer
        ))
    ));
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}
