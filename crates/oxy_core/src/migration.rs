//! Versioned in-memory conversion. Loading never rewrites the source document.
use crate::document::{Project, SCHEMA_VERSION};
use std::path::{Path, PathBuf};

/// The schema whose node parameters are the registry's: the node registry is frozen since 0.7,
/// so only older documents lack parameters, and a later one that lacks some is broken.
pub const NODE_REGISTRY_SCHEMA: u64 = 5;

/// Why a document could not be read or converted. Messages are shown to the user as-is.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("Documento OXY inválido: {0}")]
    InvalidDocument(serde_json::Error),
    #[error("Versão de projeto ausente ou inválida")]
    MissingVersion,
    #[error(
        "Versão de projeto {found} incompatível; esta OXY Engine lê 1 a {max}. O arquivo foi preservado.",
        max = SCHEMA_VERSION
    )]
    UnsupportedVersion { found: u64 },
    #[error("Ações de entrada inválidas")]
    InvalidInputBindings,
    /// A legacy 3D character cannot be converted without guessing.
    #[error(
        "Não foi possível converter Personagem 3D ‘{name}’ ({id}): {reason}. Documento original preservado."
    )]
    Character {
        name: String,
        id: String,
        reason: String,
    },
    #[error(transparent)]
    Serialize(serde_json::Error),
    #[error("Arquivo anterior inválido; salve uma cópia em outra pasta: {0}")]
    PreviousInvalid(serde_json::Error),
    #[error("Arquivo anterior sem versão; salve em outra pasta.")]
    PreviousWithoutVersion,
    #[error(
        "O arquivo de destino pertence a uma versão mais recente. Salve uma cópia em outra pasta."
    )]
    PreviousNewer,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
impl From<MigrationError> for String {
    fn from(error: MigrationError) -> Self {
        error.to_string()
    }
}

pub fn read(bytes: &[u8]) -> Result<Project, MigrationError> {
    read_report(bytes).map(|(project, _)| project)
}
pub fn read_report(bytes: &[u8]) -> Result<(Project, Option<u32>), MigrationError> {
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(MigrationError::InvalidDocument)?;
    let version = value["schema_version"]
        .as_u64()
        .ok_or(MigrationError::MissingVersion)?;
    if !(1..=u64::from(SCHEMA_VERSION)).contains(&version) {
        return Err(MigrationError::UnsupportedVersion { found: version });
    }
    if version == 1 {
        value["schema_version"] = SCHEMA_VERSION.into();
        let bindings = value["input_bindings"]
            .as_object()
            .ok_or(MigrationError::InvalidInputBindings)?;
        let labels: serde_json::Map<String, serde_json::Value> = bindings
            .keys()
            .map(|id| (id.clone(), crate::input_actions::legacy_label(id).into()))
            .collect();
        value["input_labels"] = labels.into();
        for_each_entity_list(&mut value, |entities| {
            migrate_entities(entities);
            Ok(())
        })?;
    }
    if version < 4 {
        for_each_entity_list(&mut value, migrate_movement)?;
    }
    if version < NODE_REGISTRY_SCHEMA {
        // Registry-driven: every older document gets the node parameters added since.
        for_each_entity_list(&mut value, fill_node_params)?;
    }
    // 5 to 6 adds the objects' scripts, which documents leave out while there are none.
    value["schema_version"] = SCHEMA_VERSION.into();
    let project = serde_json::from_value(value).map_err(MigrationError::InvalidDocument)?;
    Ok((
        project,
        (version < u64::from(SCHEMA_VERSION)).then_some(version as u32),
    ))
}
/// Converts the pieces of a model kept in its own file, written in the format `version`, as
/// [`read_report`] converts the pieces of a model inside a project file of that format.
pub fn migrate_pieces(version: u64, pieces: &mut serde_json::Value) -> Result<(), MigrationError> {
    if !(1..=u64::from(SCHEMA_VERSION)).contains(&version) {
        return Err(MigrationError::UnsupportedVersion { found: version });
    }
    if version == 1 {
        migrate_entities(pieces);
    }
    if version < 4 {
        migrate_movement(pieces)?;
    }
    if version < NODE_REGISTRY_SCHEMA {
        fill_node_params(pieces)?;
    }
    Ok(())
}
/// Runs `migrate` on the entity list of every scene, then on the pieces of every model.
fn for_each_entity_list(
    document: &mut serde_json::Value,
    mut migrate: impl FnMut(&mut serde_json::Value) -> Result<(), MigrationError>,
) -> Result<(), MigrationError> {
    for (holders, field) in [("scenes", "entities"), ("assets", "model")] {
        let holders = document
            .get_mut(holders)
            .and_then(serde_json::Value::as_array_mut);
        for holder in holders.into_iter().flatten() {
            if let Some(entities) = holder.get_mut(field) {
                migrate(entities)?;
            }
        }
    }
    Ok(())
}
/// Gives each node the registry parameters it lacks, with their defaults, and never touches
/// the values it has: a parameter added to an existing node must default to what the node
/// did before it. Unknown operations are left for validation to report.
fn fill_node_params(value: &mut serde_json::Value) -> Result<(), MigrationError> {
    let Some(entities) = value.as_array_mut() else {
        return Ok(());
    };
    for entity in entities {
        let nodes = entity
            .get_mut("graph")
            .and_then(|graph| graph.get_mut("nodes"))
            .and_then(serde_json::Value::as_array_mut);
        for node in nodes.into_iter().flatten() {
            let Some(definition) = node
                .get("operation")
                .and_then(serde_json::Value::as_str)
                .and_then(crate::graph::definition)
            else {
                continue;
            };
            let Some(params) = node
                .get_mut("params")
                .and_then(serde_json::Value::as_object_mut)
            else {
                continue;
            };
            for param in &definition.params {
                if !params.contains_key(param.id) {
                    let default =
                        serde_json::to_value(&param.default).map_err(MigrationError::Serialize)?;
                    params.insert(param.id.into(), default);
                }
            }
        }
    }
    Ok(())
}
fn migrate_movement(value: &mut serde_json::Value) -> Result<(), MigrationError> {
    let Some(entities) = value.as_array_mut() else {
        return Ok(());
    };
    for entity in entities {
        if entity["character3d"].is_null() {
            continue;
        }
        let error = |reason: &str| MigrationError::Character {
            name: entity["name"].as_str().unwrap_or("?").to_owned(),
            id: entity["id"].as_str().unwrap_or("?").to_owned(),
            reason: reason.to_owned(),
        };
        if !entity["character3d"]["body"].is_null() {
            return Err(error(
                "o documento antigo já contém um corpo de movimento; conversão ambígua",
            ));
        }
        let collider: crate::physics3d::Collider3d =
            serde_json::from_value(entity["physics3d"].clone())
                .map_err(|_| error("cápsula de origem ausente ou inválida"))?;
        let crate::physics3d::CollisionShape::Capsule { height, .. } = collider.shape else {
            return Err(error(
                "a forma de origem não é a cápsula vertical suportada na versão anterior",
            ));
        };
        if collider.sensor
            || !glam::Vec3::from(collider.center).abs_diff_eq(glam::Vec3::Y * height * 0.5, 1e-5)
        {
            return Err(error(
                "a cápsula é uma área ou seu centro não corresponde à origem nos pés",
            ));
        }
        let body = crate::movement_body::MovementBody {
            standing: collider.shape,
            crouched: None,
            offset: [0.; 3],
            filter: collider.filter,
            surface: collider.surface,
            enabled: collider.enabled,
        };
        let mut config: crate::character::CharacterConfig =
            serde_json::from_value(entity["character3d"].clone())
                .map_err(|_| error("configuração de movimento inválida"))?;
        config.body = body;
        config
            .body
            .validate(config.crouch_height)
            .map_err(|e| error(&e))?;
        entity["character3d"] = serde_json::to_value(config).map_err(MigrationError::Serialize)?;
        entity["physics3d"] = serde_json::Value::Null;
    }
    Ok(())
}
fn migrate_entities(value: &mut serde_json::Value) {
    if let Some(entities) = value.as_array_mut() {
        for entity in entities {
            if let Some(nodes) = entity["graph"]["nodes"].as_array_mut() {
                for node in nodes {
                    if node["operation"] == "event.input"
                        && let Some(params) = node["params"].as_object_mut()
                    {
                        params
                            .entry("mode")
                            .or_insert_with(|| serde_json::json!({"Text":"pressed"}));
                    }
                }
            }
        }
    }
}

/// Prepare a permanent source-manifest backup in the same transaction as the save.
/// IDs, relative paths and source bytes are kept verbatim. Never overwrites a backup.
/// A change of layout keeps a full copy of the project instead (see `crate::conversion`), and a
/// future migration's backup covers every file its first save rewrites because of it: a 6 to 7
/// that changed the models' pieces would keep each `.modelo.json` too.
pub fn backup(path: &Path) -> Result<Option<(PathBuf, Vec<u8>)>, MigrationError> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(MigrationError::PreviousInvalid)?;
    let Some(version) = value["schema_version"].as_u64() else {
        return Err(MigrationError::PreviousWithoutVersion);
    };
    if version > u64::from(SCHEMA_VERSION) {
        return Err(MigrationError::PreviousNewer);
    }
    if version == u64::from(SCHEMA_VERSION) {
        return Ok(None);
    }
    // Unknown old versions must not be overwritten through a save operation either.
    read(&bytes)?;
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    let destination = path.with_file_name(format!(
        "{file}.schema-{version}.{}.backup.json",
        crate::document::new_id()
    ));
    Ok(Some((destination, bytes)))
}
