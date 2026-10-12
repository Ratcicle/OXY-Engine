//! OXY Engine MCP server (`oxy-mcp`): lets AI agents read, edit, test and capture OXY projects.
//!
//! It speaks the Model Context Protocol over stdio (one JSON-RPC message per line). Tools act
//! on an open editor through its local channel when "Permitir agentes de IA" is on, or on the
//! project files when no editor has the project open. Configure it in Claude Code with
//! `claude mcp add oxy -- "<folder>\oxy-mcp.exe"`.
mod editors;
mod tools;

use serde_json::{Value as Json, json};
use std::io::{BufRead, Write};

const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "OXY Engine is a desktop game editor (Rust). A project has scenes \
(2D or 3D); a scene is a flat list of objects with parent links; objects have a transform, an \
optional shape, material, attributes, components (colliders, character, platform, camera \
control, 2D movement), a logic graph of nodes and animation clips. Start with oxy_status, then \
oxy_project and oxy_scene. Read oxy_catalog before building logic. Reads are compact JSON with \
floats rounded to 4 decimals: long lists come in pages (limit, offset, next_offset), oxy_entity \
summarizes logic and geometry unless asked in full, and world positions and boxes (oxy_scene \
fields, oxy_entity) measure distances across parent groups. Edit only through oxy_apply \
(atomic batches, validated with the editor's own rules). When the project is open in the editor \
your edits appear live and the person can undo them; you never save there, the person does. \
After changes, check them with oxy_simulate and oxy_screenshot. Messages from the engine are in \
Portuguese; answer the person in their language.";

fn handle(tools: &mut tools::Tools, request: &Json) -> Option<Json> {
    let id = request.get("id").cloned()?;
    let method = request["method"].as_str().unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or(Json::Null);
    let result = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or_default();
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| **v == asked)
                .unwrap_or(&PROTOCOL_VERSIONS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "oxy", "title": "OXY Engine", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools::definitions()})),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            Ok(match tools.call(name, &args) {
                Ok(output) => {
                    let mut content = vec![json!({"type": "text", "text": output.text})];
                    if let Some(data) = output.image {
                        content
                            .push(json!({"type": "image", "data": data, "mimeType": "image/png"}));
                    }
                    json!({"content": content})
                }
                Err(error) => json!({
                    "content": [{"type": "text", "text": error}],
                    "isError": true,
                }),
            })
        }
        other => Err(json!({"code": -32601, "message": format!("Method not found: {other}")})),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    })
}

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut tools = tools::Tools::default();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Json>(&line) {
            Ok(Json::Array(batch)) => {
                let responses: Vec<Json> =
                    batch.iter().filter_map(|r| handle(&mut tools, r)).collect();
                (!responses.is_empty()).then_some(Json::Array(responses))
            }
            Ok(request) => handle(&mut tools, &request),
            Err(error) => Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {"code": -32700, "message": format!("Parse error: {error}")},
            })),
        };
        if let Some(response) = response
            && (writeln!(stdout, "{response}").is_err() || stdout.flush().is_err())
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_lists_tools_and_answers_errors_as_tool_results() {
        let mut tools = tools::Tools::default();
        let init = handle(
            &mut tools,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": {"protocolVersion": "2025-03-26"}}),
        )
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(
            handle(
                &mut tools,
                &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
            )
            .is_none()
        );
        let list = handle(
            &mut tools,
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        )
        .unwrap();
        let names: Vec<_> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect();
        assert!(
            names.contains(&"oxy_apply".to_owned()) && names.len() == 14,
            "{names:?}"
        );
        let call = handle(
            &mut tools,
            &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                    "params": {"name": "oxy_project", "arguments": {"project": "Z:/nao/existe"}}}),
        )
        .unwrap();
        assert_eq!(call["result"]["isError"], true);
        let unknown = handle(
            &mut tools,
            &json!({"jsonrpc": "2.0", "id": 4, "method": "nada"}),
        )
        .unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    /// An editor stand-in: announces itself, checks the token and answers like the editor.
    fn fake_editor(dir: &std::path::Path) -> std::thread::JoinHandle<Vec<String>> {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("editor-1.json"),
            json!({"port": port, "token": "segredo", "project_name": "Ao vivo",
                   "project_path": "C:/ao-vivo/project.oxy.json"})
            .to_string(),
        )
        .unwrap();
        std::thread::spawn(move || {
            let mut methods = Vec::new();
            // One connection per tool call is allowed; serve until the client goes away.
            listener.set_nonblocking(false).unwrap();
            for stream in listener.incoming().take(2) {
                let stream = stream.unwrap();
                let mut writer = stream.try_clone().unwrap();
                let mut lines = BufReader::new(stream).lines();
                let hello: Json = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
                if hello["token"] != "segredo" {
                    writeln!(writer, "{}", json!({"error": "Token inválido."})).unwrap();
                    continue;
                }
                writeln!(writer, "{}", json!({"ok": true})).unwrap();
                for line in lines {
                    let Ok(line) = line else { break };
                    let request: Json = serde_json::from_str(&line).unwrap();
                    let method = request["method"].as_str().unwrap().to_owned();
                    let result = match method.as_str() {
                        "status" => json!({"project": {"name": "Ao vivo"}}),
                        "apply" => json!({"applied": [{"op": "create_entity", "created": ["x"]}],
                                          "commands": request["params"]["commands"]}),
                        _ => json!({"method": method, "params": request["params"]}),
                    };
                    methods.push(method);
                    writeln!(writer, "{}", json!({"id": request["id"], "result": result})).unwrap();
                }
                if methods.iter().any(|m| m == "apply") {
                    break;
                }
            }
            methods
        })
    }

    #[test]
    fn live_mode_finds_the_open_editor_and_fills_the_default_scene() {
        let dir =
            std::env::temp_dir().join(format!("oxy-mcp-live-{}", oxy_core::document::new_id()));
        let server = fake_editor(&dir);
        // SAFETY: this test is the only one that reads OXY_AGENT_DIR in this process.
        unsafe { std::env::set_var("OXY_AGENT_DIR", &dir) };
        let mut tools = tools::Tools::default();
        // Reads reach the editor with their options as given, for the editor to answer through
        // the same query::answer as files; bad options never reach it.
        let mut echo = |tool: &str, args: Json| {
            let output = tools.call(tool, &args).unwrap();
            serde_json::from_str::<Json>(&output.text).unwrap()
        };
        let tree = echo(
            "oxy_scene",
            json!({"fields": ["bounds"], "root": "grupo", "depth": 1, "limit": 5}),
        );
        assert_eq!(tree["method"], "scene_tree");
        assert_eq!(
            tree["params"],
            json!({"fields": ["bounds"], "root": "grupo", "depth": 1, "limit": 5})
        );
        let entity = echo(
            "oxy_entity",
            json!({"ids": ["a", "b"], "fields": ["world"], "graph": "full"}),
        );
        assert_eq!(entity["method"], "entity");
        assert_eq!(
            entity["params"],
            json!({"ids": ["a", "b"], "fields": ["world"], "graph": "full"})
        );
        let found = echo("oxy_find", json!({"text": "porta", "offset": 2}));
        assert_eq!(found["params"], json!({"text": "porta", "offset": 2}));
        let error = tools
            .call("oxy_entity", &json!({"id": "a", "geometry": "toda"}))
            .unwrap_err();
        assert!(error.contains("geometry inválido"), "{error}");
        let output = tools
            .call(
                "oxy_apply",
                &json!({"scene": "cena-1", "commands": [
                    {"op": "create_entity", "kind": {"type": "group"}},
                    {"op": "rename_project", "name": "Novo"},
                ]}),
            )
            .unwrap();
        drop(tools);
        let methods = server.join().unwrap();
        assert!(methods.contains(&"status".to_owned()) && methods.contains(&"apply".to_owned()));
        assert_eq!(
            methods.iter().filter(|m| *m == "entity").count(),
            1,
            "{methods:?}"
        );
        let result: Json = serde_json::from_str(&output.text).unwrap();
        assert_eq!(result["commands"][0]["scene"], "cena-1");
        assert!(
            result["commands"][1].get("scene").is_none(),
            "project commands take no scene"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn file_mode_applies_saves_with_a_backup_and_simulates() {
        use oxy_core::{document::SceneKind, editing, persistence};
        let dir = std::env::temp_dir().join(format!("oxy-mcp-{}", oxy_core::document::new_id()));
        let file = dir.join(persistence::PROJECT_FILE);
        let project = editing::blank_project("Arquivo", SceneKind::ThreeD).unwrap();
        persistence::save_project(&file, &project).unwrap();
        let mut tools = tools::Tools::default();
        let project_arg = dir.display().to_string();
        let applied = tools
            .call(
                "oxy_apply",
                &json!({"project": project_arg, "commands": [
                    {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Caixa"},
                    {"op": "add_node", "entity": "$0", "operation": "event.scene_start"},
                    {"op": "add_node", "entity": "$0", "operation": "debug.message",
                     "params": {"message": {"Text": "pelo MCP"}}},
                    {"op": "connect", "entity": "$0", "from_node": "$1", "from_port": "exec",
                     "to_node": "$2", "to_port": "exec"},
                ]}),
            )
            .unwrap();
        assert!(applied.text.contains("agente.bak"), "{}", applied.text);
        let saved = persistence::load_project(&file).unwrap();
        assert_eq!(saved.scenes[0].entities.len(), 1);
        let simulated = tools
            .call(
                "oxy_simulate",
                &json!({"project": project_arg, "seconds": 0.5}),
            )
            .unwrap();
        assert!(simulated.text.contains("pelo MCP"), "{}", simulated.text);
        let tree = tools
            .call("oxy_scene", &json!({"project": project_arg}))
            .unwrap();
        assert!(tree.text.contains("Caixa"));
        let error = tools
            .call(
                "oxy_apply",
                &json!({"project": project_arg, "commands": [{"op": "rename_entity",
                        "entity": "inexistente", "name": "X"}]}),
            )
            .unwrap_err();
        assert!(error.contains("Nada foi alterado"), "{error}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn simulation_setup_and_spawn_never_touch_the_project_file() {
        use oxy_core::{document::SceneKind, editing, persistence};
        let simulate = tools::definitions()
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "oxy_simulate")
            .cloned()
            .unwrap();
        for option in [
            "setup",
            "spawn",
            "until",
            "actions",
            "actions_filter",
            "watch",
        ] {
            assert!(
                simulate["inputSchema"]["properties"].get(option).is_some(),
                "{option}"
            );
        }
        // The game starts in the start scene, live too; editing tools default to the open one.
        let scene_of = |tool: &str| {
            tools::definitions()
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == tool)
                .unwrap()["inputSchema"]["properties"]["scene"]["description"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        for tool in ["oxy_simulate", "oxy_screenshot"] {
            let scene = scene_of(tool);
            assert!(
                scene.contains("start scene, also live") && scene.contains("setup"),
                "{tool}: {scene}"
            );
        }
        assert!(scene_of("oxy_apply").contains("open in the editor (live)"));
        let dir =
            std::env::temp_dir().join(format!("oxy-mcp-sim-{}", oxy_core::document::new_id()));
        let file = dir.join(persistence::PROJECT_FILE);
        let project = editing::blank_project("Simulação", SceneKind::ThreeD).unwrap();
        persistence::save_project(&file, &project).unwrap();
        let saved = std::fs::read(&file).unwrap();
        let mut tools = tools::Tools::default();
        let project_arg = dir.display().to_string();
        let output = tools
            .call(
                "oxy_simulate",
                &json!({"project": project_arg, "seconds": 5.0,
                    "setup": [
                        {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"},
                         "name": "Teste"},
                        {"op": "add_node", "entity": "$0", "operation": "event.scene_start"},
                        {"op": "add_node", "entity": "$0", "operation": "control.wait",
                         "params": {"seconds": {"Number": 0.25}}},
                        {"op": "add_node", "entity": "$0", "operation": "debug.message",
                         "params": {"message": {"Text": "pronto"}}},
                        {"op": "connect", "entity": "$0", "from_node": "$1", "from_port": "exec",
                         "to_node": "$2", "to_port": "exec"},
                        {"op": "connect", "entity": "$0", "from_node": "$2", "from_port": "exec",
                         "to_node": "$3", "to_port": "exec"},
                    ],
                    "spawn": {"entity": "$0", "position": [1.0, 2.0, 3.0]},
                    "watch": [{"id": "$0", "fields": ["position"]}],
                    "until": {"log_contains": "pronto"}}),
            )
            .unwrap();
        let report: Json = serde_json::from_str(&output.text).unwrap();
        assert_eq!(report["stopped_at"]["reason"], "log_contains", "{report}");
        assert!((report["stopped_at"]["time"].as_f64().unwrap() - 0.25).abs() < 0.05);
        assert_eq!(report["logs"][0]["text"], "pronto");
        assert_eq!(report["logs"][0]["object"]["name"], "Teste");
        assert_eq!(report["setup"].as_array().unwrap().len(), 6);
        let position = &report["watched"][0]["samples"][0]["position"];
        for (axis, expected) in [1.0, 2.0, 3.0].into_iter().enumerate() {
            assert!(
                (position[axis].as_f64().unwrap() - expected).abs() < 1e-4,
                "{position}"
            );
        }
        assert!(
            report["actions"][0]["count"].is_number(),
            "summary by default"
        );
        assert_eq!(
            std::fs::read(&file).unwrap(),
            saved,
            "setup changes only the copy"
        );
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "no backup either"
        );
        for (args, expected) in [
            (
                json!({"setup": [{"op": "voar"}]}),
                "Preparação (setup): Comando 1",
            ),
            (
                json!({"watch": [{"id": "x", "campos": []}]}),
                "Objetos observados inválidos",
            ),
            (json!({"until": {"cena": true}}), "until"),
            (json!({"actions": "tudo"}), "Modo de ações inválido"),
            (json!({"spawn": {"entity": "x"}}), "spawn inválido"),
        ] {
            let mut args = args;
            args["project"] = json!(project_arg);
            args["seconds"] = json!(1.0);
            let error = tools.call("oxy_simulate", &args).unwrap_err();
            assert!(error.contains(expected), "{error}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
