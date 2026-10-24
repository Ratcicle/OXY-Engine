//! The locks that keep two writers off one project. The editor holds `.oxy/editor.lock` open
//! while a project is open in it, agents allowed or not, so nobody can remove it while the
//! editor lives (the system lets go of it if the editor falls). The agents' tools on files remove
//! a lock an editor left behind before they write, and refuse to write while they cannot (reading
//! is still allowed); each of their calls that writes holds `.oxy/agente.lock` meanwhile, which
//! the editor waits for when it opens or saves the project. Organizing a project in folders
//! holds `.oxy/conversao.lock` while it works, so that opening the project meanwhile (an agent's
//! read, for one) does not finish or undo it halfway. Outside Windows a file open does not keep
//! others from removing it, and the locks only note who has the project.
use crate::fs_ops;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Seek, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

/// The editor's lock, from the project's folder.
pub const EDITOR_LOCK: &str = ".oxy/editor.lock";
/// The lock an agent's tool holds while it writes the project, from the project's folder.
pub const AGENT_LOCK: &str = ".oxy/agente.lock";
/// The lock held while the project is organized in folders, or such work that stopped is
/// finished or undone (see `crate::conversion`), from the project's folder.
pub const CONVERSION_LOCK: &str = ".oxy/conversao.lock";
/// How long the editor waits for an agent's write to end.
pub const AGENT_WAIT: Duration = Duration::from_secs(2);
/// Why the agents' tools do not write a project open in the editor.
pub const EDITOR_OPEN: &str = "O projeto está aberto no editor da OXY com os agentes desligados; \
    ligue Permitir agentes de IA nas Preferências do editor (as mudanças passam a aparecer ao \
    vivo) ou feche o projeto no editor. Ler o projeto continua permitido.";

/// A lock file held open: nobody else may remove or write it meanwhile. Dropping it closes the
/// file and removes it (and OXY's folder, when that is left empty).
#[derive(Debug)]
struct Held {
    file: Option<File>,
    path: PathBuf,
}
impl Held {
    /// Creates the lock `relative` of the project at `root` and holds it, after removing one
    /// left behind by a process that ended. Fails when another process holds it.
    fn take(root: &Path, relative: &str) -> io::Result<Self> {
        let path = root.join(relative);
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        let file = match open_held(&path) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // Left behind, unless someone holds it, which keeps it from going.
                fs::remove_file(&path)?;
                open_held(&path)?
            }
            opened => opened?,
        };
        Ok(Self {
            file: Some(file),
            path,
        })
    }
    fn write(&mut self, text: &str) -> io::Result<()> {
        let file = self.file.as_mut().expect("held until dropped");
        file.set_len(0)?;
        file.rewind()?;
        file.write_all(text.as_bytes())?;
        file.sync_all()
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        // Closed first: the file is held without letting anyone remove it, this process too.
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
        if let Some(folder) = self.path.parent() {
            let _ = fs_ops::remove_empty_dir(folder);
        }
    }
}

/// Creates the file `path` held so that others may only read it.
fn open_held(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ: nobody may write, rename or remove it while it is open.
        options.share_mode(0x1);
    }
    options.open(path)
}

/// The editor's hold on the project open in it, let go when dropped.
#[derive(Debug)]
pub struct EditorLock(Held);
impl EditorLock {
    /// Holds the lock of the project at `root` for the editor, noting this process, since when
    /// and whether it allows agents. Fails when another editor holds it.
    pub fn hold(root: &Path, agents: bool) -> io::Result<Self> {
        let mut lock = Self(Held::take(root, EDITOR_LOCK)?);
        lock.set_agents(agents)?;
        Ok(lock)
    }
    /// Notes again whether the editor allows agents.
    pub fn set_agents(&mut self, agents: bool) -> io::Result<()> {
        let note = serde_json::json!({
            "pid": std::process::id(),
            "since": fs_ops::utc(SystemTime::now()),
            "agents": agents,
        });
        self.0.write(&format!("{note:#}\n"))
    }
}

/// An agent's hold on the project it writes, let go when dropped.
#[derive(Debug)]
pub struct AgentLock {
    _held: Held,
}
impl AgentLock {
    /// Holds the project at `root` for an agent's write: refused while the editor has it open
    /// ([`EDITOR_OPEN`]) or another agent's write holds it.
    pub fn hold(root: &Path) -> Result<Self, String> {
        if editor_holds(root) {
            return Err(EDITOR_OPEN.into());
        }
        Held::take(root, AGENT_LOCK)
            .map(|held| Self { _held: held })
            .map_err(|error| {
                if fs_ops::busy(&error) {
                    "Outro agente está gravando este projeto agora; tente de novo em seguida."
                        .into()
                } else {
                    format!("Não foi possível reservar o projeto para gravar: {error}")
                }
            })
    }
}

/// The hold of a conversion of the project to folders, let go when dropped.
#[derive(Debug)]
pub struct ConversionLock {
    _held: Held,
}
impl ConversionLock {
    /// Holds the project at `root` for organizing it in folders; fails while another conversion
    /// holds it.
    pub fn hold(root: &Path) -> io::Result<Self> {
        Held::take(root, CONVERSION_LOCK).map(|held| Self { _held: held })
    }
}

/// Whether a conversion of the project at `root` to folders is at work now (a lock left behind
/// is removed).
pub fn converting(root: &Path) -> bool {
    held(&root.join(CONVERSION_LOCK))
}

/// Whether an editor has the project at `root` open: its lock is there and cannot be removed.
/// A lock an editor that ended left behind is removed.
pub fn editor_holds(root: &Path) -> bool {
    held(&root.join(EDITOR_LOCK))
}

/// Whether an agent's tool is writing the project at `root` now (a lock left behind is
/// removed).
pub fn agent_writing(root: &Path) -> bool {
    held(&root.join(AGENT_LOCK))
}

/// Waits up to [`AGENT_WAIT`] for an agent's write to the project at `root` to end; whether one
/// was still going when the wait ended.
pub fn wait_for_agents(root: &Path) -> bool {
    let start = Instant::now();
    while agent_writing(root) {
        if start.elapsed() >= AGENT_WAIT {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Whether the lock file `path` is held by a process: it is there and cannot be removed.
fn held(path: &Path) -> bool {
    match fs::remove_file(path) {
        Ok(()) => false,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(_) => path.exists(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::new_id;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oxy-lock-{}", new_id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(windows)]
    #[test]
    fn an_editor_keeps_agents_from_writing_until_it_lets_go() {
        let root = temp();
        let lock = EditorLock::hold(&root, false).unwrap();
        let note: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(EDITOR_LOCK)).unwrap()).unwrap();
        assert_eq!(note["pid"], std::process::id());
        assert_eq!(note["agents"], false);
        assert!(editor_holds(&root));
        assert_eq!(AgentLock::hold(&root).unwrap_err(), EDITOR_OPEN);
        // Nobody removes it, and a second editor does not take it.
        assert!(fs::remove_file(root.join(EDITOR_LOCK)).is_err());
        assert!(EditorLock::hold(&root, true).is_err());
        drop(lock);
        assert!(
            !root.join(".oxy").exists(),
            "the lock and OXY's empty folder go"
        );
        let agent = AgentLock::hold(&root).unwrap();
        assert!(agent_writing(&root));
        assert!(AgentLock::hold(&root).unwrap_err().contains("Outro agente"));
        drop(agent);
        assert!(!agent_writing(&root));
        assert!(!wait_for_agents(&root));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn one_conversion_at_a_time() {
        let root = temp();
        let lock = ConversionLock::hold(&root).unwrap();
        assert!(converting(&root));
        assert!(fs_ops::busy(&ConversionLock::hold(&root).unwrap_err()));
        drop(lock);
        assert!(!converting(&root));
        assert!(!root.join(".oxy").exists());
        fs::create_dir_all(root.join(".oxy")).unwrap();
        fs_ops::create_new_file(&root.join(CONVERSION_LOCK), b"").unwrap();
        assert!(!converting(&root), "a lock left behind is no conversion");
        drop(ConversionLock::hold(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_lock_left_behind_is_removed() {
        let root = temp();
        let left = |relative: &str| {
            fs::create_dir_all(root.join(".oxy")).unwrap();
            fs_ops::create_new_file(&root.join(relative), b"{\"pid\": 1}").unwrap();
        };
        left(EDITOR_LOCK);
        left(AGENT_LOCK);
        assert!(!agent_writing(&root));
        assert!(!root.join(AGENT_LOCK).exists());
        let agent = AgentLock::hold(&root).unwrap();
        assert!(!root.join(EDITOR_LOCK).exists());
        drop(agent);
        left(EDITOR_LOCK);
        let mut lock = EditorLock::hold(&root, true).unwrap();
        lock.set_agents(false).unwrap();
        drop(lock);
        fs::remove_dir_all(root).unwrap();
    }
}
