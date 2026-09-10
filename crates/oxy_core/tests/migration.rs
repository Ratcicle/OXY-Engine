use oxy_core::{document::*, migration, persistence};

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
