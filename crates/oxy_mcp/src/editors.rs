//! Open OXY editors that allow agents, found through the files they write to the agents
//! folder, and the line-based JSON connection to each.
use serde_json::{Value as Json, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

/// Folder where open editors announce their channel (same rule as the editor).
pub fn discovery_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OXY_AGENT_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("OXY Engine").join("agents"))
}

#[derive(Clone, Debug)]
pub struct EditorInfo {
    pub port: u16,
    pub token: String,
    pub project_name: Option<String>,
    pub project_path: Option<PathBuf>,
}

/// Editors that announced themselves. Stale files (closed editors) fail to connect later.
pub fn discover() -> Vec<EditorInfo> {
    let Some(dir) = discovery_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut editors: Vec<EditorInfo> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name().to_string_lossy().starts_with("editor-")
                && e.path().extension().is_some_and(|x| x == "json")
        })
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<Json>(&bytes).ok())
        .filter_map(|info| {
            Some(EditorInfo {
                port: u16::try_from(info["port"].as_u64()?).ok()?,
                token: info["token"].as_str()?.to_owned(),
                project_name: info["project_name"].as_str().map(str::to_owned),
                project_path: info["project_path"].as_str().map(PathBuf::from),
            })
        })
        .collect();
    editors.sort_by_key(|e| e.port);
    editors
}

/// Same project file, comparing canonical paths when both exist.
pub fn same_project(a: &Path, b: &Path) -> bool {
    let canonical = |p: &Path| {
        let file = if p.is_dir() {
            p.join(oxy_core::persistence::PROJECT_FILE)
        } else {
            p.to_owned()
        };
        file.canonicalize().unwrap_or(file)
    };
    canonical(a) == canonical(b)
}

/// The answer that never came: the connection may still deliver it, after the next request.
pub const NO_ANSWER: &str = "O editor não respondeu.";

/// How long the editor may take to answer `method`: a little more than it gives itself (60
/// seconds, 300 for an export, which copies and runs the game before it answers).
fn answer_timeout(method: &str) -> Duration {
    Duration::from_secs(if method == "export" { 330 } else { 90 })
}

pub struct LiveEditor {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    next_id: u64,
}
impl LiveEditor {
    pub fn connect(info: &EditorInfo) -> Result<Self, String> {
        let address = SocketAddr::from(([127, 0, 0, 1], info.port));
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .map_err(|_| "O editor não está respondendo (pode ter sido fechado).".to_owned())?;
        stream
            .set_read_timeout(Some(answer_timeout("")))
            .map_err(|e| e.to_string())?;
        let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        writeln!(writer, "{}", json!({"token": info.token})).map_err(|e| e.to_string())?;
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let hello: Json = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if hello["ok"] != true {
            return Err(hello["error"]
                .as_str()
                .unwrap_or("O editor recusou a conexão.")
                .to_owned());
        }
        Ok(Self {
            reader,
            writer,
            next_id: 1,
        })
    }
    pub fn request(&mut self, method: &str, params: Json) -> Result<Json, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.reader
            .get_ref()
            .set_read_timeout(Some(answer_timeout(method)))
            .map_err(|e| e.to_string())?;
        writeln!(
            self.writer,
            "{}",
            json!({"id": id, "method": method, "params": params})
        )
        .map_err(|_| "A conexão com o editor caiu.".to_owned())?;
        let mut line = String::new();
        let read = self
            .reader
            .read_line(&mut line)
            .map_err(|_| NO_ANSWER.to_owned())?;
        if read == 0 {
            return Err("A conexão com o editor caiu.".into());
        }
        let response: Json = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        match response.get("error").and_then(Json::as_str) {
            Some(error) => Err(error.to_owned()),
            None => Ok(response.get("result").cloned().unwrap_or(Json::Null)),
        }
    }
}
