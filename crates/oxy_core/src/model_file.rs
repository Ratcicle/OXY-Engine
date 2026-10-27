//! A model's own file in a project of the tree layout: `<Name>.modelo.json` in any folder of the
//! project, named as the Library shows the model, with the model's ID inside, so a model moved
//! or renamed outside OXY is still found. Only the project's files split: in memory, in the
//! history, on the live channel and in an exported game the pieces stay in the model's asset.
use crate::{
    document::{Entity, Id, SCHEMA_VERSION},
    migration,
};
use serde::{Deserialize, Serialize};

/// The extension of a model's file, compared in any case.
pub const EXTENSION: &str = ".modelo.json";
/// What the field `oxy` of a model's file says the file is.
const MARK: &str = "modelo";
/// Largest model file OXY reads, as the project file.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// A model's file as it is read: what it is, the format it was written in, the model's ID and
/// its pieces, as a scene's objects. No name: the name is the file's.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    #[serde(rename = "oxy")]
    _mark: String,
    #[serde(rename = "schema_version")]
    _version: u32,
    id: Id,
    entities: Vec<Entity>,
}

/// A model's file as it is written.
#[derive(Serialize)]
struct Written<'a> {
    oxy: &'a str,
    schema_version: u32,
    id: &'a str,
    entities: &'a [Entity],
}

/// The bytes of the file of the model `id` whose pieces are `entities`: indented JSON in the
/// current format.
pub fn to_bytes(id: &str, entities: &[Entity]) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec_pretty(&Written {
        oxy: MARK,
        schema_version: SCHEMA_VERSION,
        id,
        entities,
    })
}

/// The model the bytes of a file hold: its ID and its pieces, converted from the format the file
/// was written in. Why not, said after the file's path, when they hold none: not JSON, not a
/// model of OXY, made by a newer OXY, a field OXY does not know, no ID or no pieces.
pub fn from_bytes(bytes: &[u8]) -> Result<(Id, Vec<Entity>), String> {
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("não é JSON válido ({e})"))?;
    if value.get("oxy").and_then(serde_json::Value::as_str) != Some(MARK) {
        return Err("não é um modelo da OXY (falta \"oxy\": \"modelo\")".into());
    }
    let version = value["schema_version"]
        .as_u64()
        .ok_or("não diz a versão do formato (schema_version)")?;
    if version > u64::from(SCHEMA_VERSION) {
        return Err(format!(
            "foi feito por uma OXY mais nova (formato {version}); esta lê até o formato \
             {SCHEMA_VERSION}"
        ));
    }
    if let Some(pieces) = value.get_mut("entities") {
        migration::migrate_pieces(version, pieces).map_err(|e| e.to_string())?;
    }
    let read: Read =
        serde_json::from_value(value).map_err(|e| format!("não é um modelo válido ({e})"))?;
    if read.id.trim().is_empty() {
        return Err("não tem o ID do modelo".into());
    }
    if read.entities.is_empty() {
        return Err("não tem peças".into());
    }
    Ok((read.id, read.entities))
}

/// Whether `path` (or a file name) ends in [`EXTENSION`], in any case, with a name before it.
pub fn is_model_file(path: &str) -> bool {
    stem(path).is_some()
}

/// The name of the model whose file is `path`: the file's name without [`EXTENSION`]
/// (`Pista/Ladrilho v1.2.modelo.json` is `Ladrilho v1.2`); none for another file.
pub fn stem(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let cut = name.len().checked_sub(EXTENSION.len())?;
    (cut > 0 && name.is_char_boundary(cut) && name[cut..].eq_ignore_ascii_case(EXTENSION))
        .then(|| &name[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_file_holds_the_id_and_the_pieces_but_not_the_name() {
        let piece = Entity::new("Checkpoint", None);
        let bytes = to_bytes("60e9", std::slice::from_ref(&piece)).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.starts_with("{\n  \"oxy\": \"modelo\",\n  \"schema_version\": 6,"));
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let mut keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["entities", "id", "oxy", "schema_version"]);
        assert_eq!(from_bytes(&bytes).unwrap(), ("60e9".into(), vec![piece]));
    }

    #[test]
    fn a_file_that_is_no_model_says_why() {
        let piece = serde_json::to_value(Entity::new("Peça", None)).unwrap();
        for (value, why) in [
            (
                serde_json::json!({"schema_version": 6}),
                "não é um modelo da OXY",
            ),
            (
                serde_json::json!({"oxy": "modelo", "id": "a", "entities": [piece]}),
                "não diz a versão",
            ),
            (
                serde_json::json!({"oxy": "modelo", "schema_version": 7, "id": "a", "entities": [piece]}),
                "OXY mais nova (formato 7)",
            ),
            (
                serde_json::json!({"oxy": "modelo", "schema_version": 6, "id": "a", "entities": [piece], "name": "X"}),
                "não é um modelo válido",
            ),
            (
                serde_json::json!({"oxy": "modelo", "schema_version": 6, "id": " ", "entities": [piece]}),
                "não tem o ID",
            ),
            (
                serde_json::json!({"oxy": "modelo", "schema_version": 6, "id": "a", "entities": []}),
                "não tem peças",
            ),
        ] {
            let error = from_bytes(&serde_json::to_vec(&value).unwrap()).unwrap_err();
            assert!(error.contains(why), "{error}");
        }
        assert!(
            from_bytes(b"{")
                .unwrap_err()
                .starts_with("não é JSON válido")
        );
    }

    #[test]
    fn the_name_is_the_file_name_without_the_extension_in_any_case() {
        for (path, name) in [
            ("Checkpoint.modelo.json", Some("Checkpoint")),
            ("Pista/Ladrilho v1.2.modelo.json", Some("Ladrilho v1.2")),
            ("Pista/Torre.Modelo.JSON", Some("Torre")),
            ("Música.modelo.json", Some("Música")),
            (".modelo.json", None),
            ("Pista/.modelo.json", None),
            ("modelo.json", None),
            ("Pulo.wav", None),
        ] {
            assert_eq!(stem(path), name, "{path}");
            assert_eq!(is_model_file(path), name.is_some(), "{path}");
        }
    }
}
