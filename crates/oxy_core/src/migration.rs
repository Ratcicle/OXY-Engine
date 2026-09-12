//! Versioned in-memory conversion. Loading never rewrites the source document.
use crate::document::{Project, SCHEMA_VERSION};
use std::path::{Path, PathBuf};

pub fn read(bytes: &[u8]) -> Result<Project, String> {
    read_report(bytes).map(|(project, _)| project)
}
pub fn read_report(bytes: &[u8]) -> Result<(Project, Option<u32>), String> {
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("Documento OXY inválido: {e}"))?;
    let version = value["schema_version"]
        .as_u64()
        .ok_or("Versão de projeto ausente ou inválida")?;
    if !(1..=u64::from(SCHEMA_VERSION)).contains(&version) {
        return Err(format!(
            "Versão de projeto {version} incompatível; esta OXY Engine lê 1 a {SCHEMA_VERSION}. O arquivo foi preservado."
        ));
    }
    if version == 1 {
        value["schema_version"] = SCHEMA_VERSION.into();
        let bindings = value["input_bindings"]
            .as_object()
            .ok_or("Ações de entrada inválidas")?;
        let labels: serde_json::Map<String, serde_json::Value> = bindings
            .keys()
            .map(|id| (id.clone(), crate::input_actions::legacy_label(id).into()))
            .collect();
        value["input_labels"] = labels.into();
        if let Some(scenes) = value["scenes"].as_array_mut() {
            for scene in scenes {
                migrate_entities(&mut scene["entities"]);
            }
        }
        if let Some(assets) = value["assets"].as_array_mut() {
            for asset in assets {
                migrate_entities(&mut asset["model"]);
            }
        }
    }
    if version < u64::from(SCHEMA_VERSION) {
        if let Some(scenes) = value["scenes"].as_array_mut() {
            for scene in scenes {
                migrate_movement(&mut scene["entities"])?;
            }
        }
        if let Some(assets) = value["assets"].as_array_mut() {
            for asset in assets {
                migrate_movement(&mut asset["model"])?;
            }
        }
        value["schema_version"] = SCHEMA_VERSION.into();
    }
    let project =
        serde_json::from_value(value).map_err(|e| format!("Documento OXY inválido: {e}"))?;
    Ok((
        project,
        (version < u64::from(SCHEMA_VERSION)).then_some(version as u32),
    ))
}
fn migrate_movement(value: &mut serde_json::Value) -> Result<(), String> {
    let Some(entities) = value.as_array_mut() else {
        return Ok(());
    };
    for entity in entities {
        if entity["character3d"].is_null() {
            continue;
        }
        let error = |message: &str| {
            format!(
                "Não foi possível converter Personagem 3D ‘{}’ ({}): {message}. Documento original preservado.",
                entity["name"].as_str().unwrap_or("?"),
                entity["id"].as_str().unwrap_or("?")
            )
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
        entity["character3d"] = serde_json::to_value(config).map_err(|e| e.to_string())?;
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
pub fn backup(path: &Path) -> Result<Option<(PathBuf, Vec<u8>)>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Arquivo anterior inválido; salve uma cópia em outra pasta: {e}"))?;
    let Some(version) = value["schema_version"].as_u64() else {
        return Err("Arquivo anterior sem versão; salve em outra pasta.".into());
    };
    if version > u64::from(SCHEMA_VERSION) {
        return Err("O arquivo de destino pertence a uma versão mais recente. Salve uma cópia em outra pasta.".into());
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
