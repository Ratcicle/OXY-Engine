//! The scripts' API as the catalog lists it (oxy_catalog), from the registry the host installs
//! and the types are written from (see [`crate::script::registry`]): by default each area with
//! its names, and the rows asked for, by name, prefix or area, in full with their signature and
//! their help in English.
use super::Page;
use crate::script::{
    generate,
    registry::{API, ApiFn, ApiKind, Area, View},
};
use crate::script_files::TYPES_PATH;
use serde_json::{Map, Value as Json, json};

/// What a row is, in the catalog.
fn kind_name(f: &ApiFn) -> &'static str {
    match f.kind {
        ApiKind::Function => "function",
        ApiKind::Method => "method",
        ApiKind::Property { .. } => "property",
        ApiKind::Callback => "callback",
        ApiKind::Declaration => "declaration",
        ApiKind::Constant => "constant",
        ApiKind::Reserved => "reserved",
    }
}

/// An area as the catalog names it (`audio`, `character`).
fn area_name(area: Area) -> String {
    serde_json::to_value(area)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A row in full: its name, signature, kind, area and help in English, whether a script may
/// assign it, what its texts name in the project (the lint of names checks them) and the legacy
/// nodes that do the same.
fn row(f: &ApiFn) -> Json {
    let mut entry = json!({
        "name": f.name,
        "signature": generate::signature(f),
        "kind": kind_name(f),
        "area": area_name(f.area),
        "help": f.help_en,
    });
    if f.kind == (ApiKind::Property { write: true }) {
        entry["writable"] = json!(true);
    }
    let names = f.names();
    if !names.is_empty() {
        entry["names"] = json!(names);
    }
    if !f.nodes.is_empty() {
        entry["nodes"] = json!(f.nodes);
    }
    entry
}

/// A name as rows have it, from the forms scripts write: `obj.position` and `self:after` are
/// `Object.position` and `Object:after`, `obj.character:jump` is `Character:jump`, and
/// `Modulo.attributes` is `attributes`.
fn row_name(text: &str) -> String {
    let text = text.trim();
    for prefix in ["Modulo.", "Module.", "M."] {
        if let Some(rest) = text.strip_prefix(prefix) {
            return rest.to_owned();
        }
    }
    let mut views: Vec<View> = View::ALL.to_vec();
    views.sort_by_key(|view| std::cmp::Reverse(view.path().len()));
    for view in views {
        for path in [
            view.path().to_owned(),
            view.path().replacen("obj", "self", 1),
        ] {
            for separator in ['.', ':'] {
                if let Some(rest) = text.strip_prefix(&format!("{path}{separator}")) {
                    return format!("{}{separator}{rest}", view.name());
                }
            }
        }
    }
    text.to_owned()
}

/// The rows `asked` names: each entry a row's name (in any case, in the forms [`row_name`]
/// reads) or a prefix ending in `.`, `:` or `*` (`audio.`, `Object:`, `obj.character.`); an
/// entry no row matches is refused with the name meant.
fn named_rows(asked: &[String]) -> Result<Vec<&'static ApiFn>, String> {
    let mut wanted: Vec<&'static ApiFn> = Vec::new();
    for entry in asked {
        let text = row_name(entry.trim_end_matches('*'));
        let prefix = entry.ends_with(['.', ':', '*']);
        let found: Vec<&'static ApiFn> = API
            .iter()
            .filter(|f| {
                if prefix {
                    f.name.to_lowercase().starts_with(&text.to_lowercase())
                } else {
                    f.name.eq_ignore_ascii_case(&text)
                }
            })
            .collect();
        if found.is_empty() {
            return Err(if prefix {
                format!("Nenhum nome da API dos scripts começa com {entry}")
            } else {
                let hint = crate::script::closest(&text, API.iter().map(|f| f.name))
                    .map_or_else(String::new, |near| format!("; quis dizer {near}?"));
                format!("{entry} não é um nome da API dos scripts{hint}")
            });
        }
        for f in found {
            if !wanted.iter().any(|known| std::ptr::eq(*known, f)) {
                wanted.push(f);
            }
        }
    }
    Ok(wanted)
}

/// The area `asked` names, by its name in the catalog or its title, in any case.
fn area_named(asked: &str) -> Result<Area, String> {
    let wanted = asked.trim().to_lowercase();
    Area::ALL
        .into_iter()
        .find(|area| area_name(*area) == wanted || area.title().to_lowercase() == wanted)
        .ok_or_else(|| {
            let areas: Vec<String> = Area::ALL.into_iter().map(area_name).collect();
            format!(
                "Área desconhecida da API dos scripts: {asked}. Áreas: {}",
                areas.join(", ")
            )
        })
}

/// The API section of the catalog. Without `names` and `area`, each area with the names of its
/// rows, `total` and the file of the types; with them, those rows in full (see [`row`]), in the
/// registry's order, paged by `page`.
pub(super) fn section(names: &[String], area: Option<&str>, page: Page) -> Result<Json, String> {
    if names.is_empty() && area.is_none() {
        let areas: Vec<Json> = Area::ALL
            .into_iter()
            .filter_map(|area| {
                let names: Vec<&str> = API
                    .iter()
                    .filter(|f| f.area == area)
                    .map(|f| f.name)
                    .collect();
                (!names.is_empty()).then(
                    || json!({"area": area_name(area), "title": area.title(), "names": names}),
                )
            })
            .collect();
        return Ok(json!({
            "areas": areas,
            "total": API.len(),
            "types_file": TYPES_PATH,
        }));
    }
    let mut rows: Vec<&'static ApiFn> = if names.is_empty() {
        API.iter().collect()
    } else {
        named_rows(names)?
    };
    if let Some(area) = area {
        let area = area_named(area)?;
        if !names.is_empty() && rows.iter().all(|f| f.area != area) {
            return Err(format!(
                "Nenhum dos nomes pedidos é da área {}",
                area_name(area)
            ));
        }
        rows.retain(|f| f.area == area);
    }
    // In the registry's order, as the reference has them.
    rows.sort_by_key(|f| API.iter().position(|row| std::ptr::eq(row, *f)));
    let (rows, about) = page.cut(rows.into_iter().map(row).collect());
    let mut answer = Map::new();
    answer.insert("api".into(), Json::Array(rows));
    answer.extend(about);
    answer.insert("types_file".into(), json!(TYPES_PATH));
    Ok(Json::Object(answer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_scripts_write_reach_their_rows() {
        assert_eq!(row_name("obj.position"), "Object.position");
        assert_eq!(row_name("self:after"), "Object:after");
        assert_eq!(row_name("obj.character:jump"), "Character:jump");
        assert_eq!(row_name("Modulo.attributes"), "attributes");
        assert_eq!(row_name("audio.play"), "audio.play");
        let found = named_rows(&["Audio.Play".into(), "obj:after".into()]).unwrap();
        let names: Vec<&str> = found.iter().map(|f| f.name).collect();
        assert_eq!(names, ["audio.play", "Object:after"]);
        let character = named_rows(&["obj.character:".into()]).unwrap();
        assert!(character.iter().all(|f| f.name.starts_with("Character:")));
        assert!(character.len() > 3);
        assert_eq!(
            named_rows(&["audio.paly".into()]).unwrap_err(),
            "audio.paly não é um nome da API dos scripts; quis dizer audio.play?"
        );
        assert!(named_rows(&["nada.".into()]).is_err());
        assert_eq!(area_named("Áudio").unwrap(), Area::Audio);
        assert!(area_named("som").unwrap_err().contains("audio"));
    }

    #[test]
    fn the_section_lists_areas_by_default_and_rows_in_full_when_asked() {
        let brief = section(&[], None, Page::ALL).unwrap();
        assert_eq!(brief["total"], API.len());
        assert_eq!(brief["types_file"], TYPES_PATH);
        let areas = brief["areas"].as_array().unwrap();
        assert_eq!(areas[0]["area"], "events");
        let listed: usize = areas
            .iter()
            .map(|area| area["names"].as_array().unwrap().len())
            .sum();
        assert_eq!(listed, API.len(), "every row once");
        let play = section(&["audio.play".into()], None, Page::ALL).unwrap();
        let entry = &play["api"][0];
        assert_eq!(entry["name"], "audio.play");
        assert_eq!(entry["kind"], "function");
        assert_eq!(entry["area"], "audio");
        assert_eq!(entry["names"], json!(["sound"]));
        assert!(
            entry["signature"]
                .as_str()
                .unwrap()
                .starts_with("audio.play(")
        );
        assert!(!entry["help"].as_str().unwrap().is_empty());
        assert_eq!(entry["nodes"], json!(["action.sound"]));
        let position = section(&["obj.position".into()], None, Page::ALL).unwrap();
        assert_eq!(position["api"][0]["writable"], true);
        let audio = section(
            &[],
            Some("audio"),
            Page {
                offset: 1,
                limit: 2,
            },
        )
        .unwrap();
        assert_eq!(audio["api"].as_array().unwrap().len(), 2);
        assert_eq!(audio["next_offset"], 3);
        assert!(
            audio["api"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["area"] == "audio")
        );
        assert!(section(&["audio.play".into()], Some("events"), Page::ALL).is_err());
    }
}
