//! File operations of the project tree that never write over a file: moves that refuse a taken
//! name (in any case, or as a short 8.3 name), files and copies that are only created, the
//! project's flat trash, batches that undo what they did when a step fails, a retry while
//! another program holds a file, and the lock the tree's scan shares with them. Direct calls to
//! Windows take paths in the `\\?\` form ([`canonical`]), as [`names::resolve`] gives them.
use crate::{
    document::{Id, new_id},
    names,
    persistence::{self, FileWrite},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, SystemTime},
};

#[cfg(test)]
mod tests;

/// The project's trash, from its folder: one flat folder, each file in the slot its history
/// step fixed, so the paths stay short.
pub const TRASH: &str = ".oxy/lixeira";
/// What the trash notes of each file: the asset it is, where it was and when it went.
pub const TRASH_INDEX: &str = ".oxy/lixeira/indice.json";
/// The tries a busy file gets after the first, and the wait before each.
const RETRIES: u32 = 8;
const RETRY_WAIT: Duration = Duration::from_millis(100);

/// What tells that a file changed without reading it: its size and when it was last written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
}
impl Stamp {
    pub fn of(meta: &fs::Metadata) -> Self {
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        }
    }
    pub fn read(path: &Path) -> io::Result<Self> {
        fs::metadata(path).map(|meta| Self::of(&meta))
    }
}

/// Whether `error` comes from a file or folder another program holds: a file open in it, a
/// folder with a file open inside or listed (ERROR_ACCESS_DENIED), a folder that is a
/// terminal's current one (ERROR_SHARING_VIOLATION), a locked region (ERROR_LOCK_VIOLATION).
pub fn busy(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied || matches!(error.raw_os_error(), Some(32 | 33))
}
/// `action`, tried again for a moment while what it touches is [`busy`].
pub fn retrying<T>(mut action: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut attempts = 0;
    loop {
        match action() {
            Err(error) if busy(&error) && attempts < RETRIES => {
                attempts += 1;
                std::thread::sleep(RETRY_WAIT);
            }
            result => return result,
        }
    }
}

/// Moves or renames the file or folder `from` to `to` in one step, never over anything: a name
/// taken there in any case (or as a short 8.3 name), or an empty folder, fails with
/// [`io::ErrorKind::AlreadyExists`]. A change of case alone (`pulo.wav` to `Pulo.wav`) is that
/// one step too. Both stay on one disk. A folder with a file open inside, or listed by another
/// program, fails with ERROR_ACCESS_DENIED, and one that is a program's current folder with
/// ERROR_SHARING_VIOLATION: [`retrying`] waits for them.
#[cfg(windows)]
pub fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let (from, to) = (wide(from)?, wide(to)?);
    // Both paths are owned, terminated UTF-16 buffers alive for this synchronous call. Flags 0:
    // without MOVEFILE_REPLACE_EXISTING the call never takes a name that is in use.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(not(windows))]
#[expect(
    clippy::disallowed_methods,
    reason = "the target is checked first; Windows, which OXY ships for, has the atomic call"
)]
pub fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "o destino já existe",
        ));
    }
    fs::rename(from, to)
}

/// `path` as a terminated UTF-16 buffer for a direct call to Windows, in [`canonical`] form.
#[cfg(windows)]
pub(crate) fn wide(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = canonical(path)?.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "caminho com caractere nulo",
        ));
    }
    Ok(wide.into_iter().chain(Some(0)).collect())
}

/// `path` as direct calls to Windows take it: absolute, in the `\\?\` form that reaches past
/// 260 characters and keeps every name as written (the standard library adds it by itself; a
/// direct call does not). A path not yet in that form is first worked out as Windows works out
/// the ones the standard library hands it (`.`, `..`, `/`, and the dot a name ends with), so
/// both name the same file; a path already in it stays.
#[cfg(windows)]
pub fn canonical(path: &Path) -> io::Result<PathBuf> {
    use std::{
        ffi::OsString,
        path::{Component, Prefix},
    };
    // GetFullPathNameW, which leaves a path in the `\\?\` form as it is.
    let absolute = std::path::absolute(path)?;
    let mut components = absolute.components();
    let mut text = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) => OsString::from(format!(r"\\?\{}:", char::from(letter))),
            Prefix::UNC(server, share) => {
                let mut text = OsString::from(r"\\?\UNC\");
                text.push(server);
                text.push(r"\");
                text.push(share);
                text
            }
            _ => return Ok(absolute),
        },
        _ => return Ok(absolute),
    };
    let mut root = true;
    for component in components {
        if let Component::Normal(name) = component {
            text.push(r"\");
            text.push(name);
            root = false;
        }
    }
    if root {
        text.push(r"\");
    }
    Ok(PathBuf::from(text))
}
#[cfg(not(windows))]
pub fn canonical(path: &Path) -> io::Result<PathBuf> {
    std::path::absolute(path)
}

/// `path` as people read it, without the `\\?\` of [`canonical`].
pub fn shown(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else if let Some(local) = text.strip_prefix(r"\\?\") {
        local.to_owned()
    } else {
        text
    }
}

/// Writes `bytes` as a new file at `path`, synced to the disk. A file already there (with the
/// name in any case, or as a short 8.3 name) is an error and stays as it was; what a failure
/// left half written goes.
pub fn create_new_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}
/// Copies the file `from` as a new file at `to`, as [`create_new_file`] writes one, keeping its
/// write time. Answers the bytes copied.
pub fn copy_new(from: &Path, to: &Path) -> io::Result<u64> {
    let mut source = File::open(from)?;
    let modified = source.metadata()?.modified().ok();
    let mut file = OpenOptions::new().write(true).create_new(true).open(to)?;
    match io::copy(&mut source, &mut file).and_then(|bytes| file.sync_all().map(|()| bytes)) {
        Ok(bytes) => {
            if let Some(modified) = modified {
                let _ = file.set_modified(modified);
            }
            Ok(bytes)
        }
        Err(error) => {
            drop(file);
            let _ = fs::remove_file(to);
            Err(error)
        }
    }
}
/// Gives the file `from` a second name `to` (a hard link, so nothing is copied), or a copy
/// ([`copy_new`]) on a disk without links (FAT, exFAT). Only creates: a taken `to` is an error.
pub fn link_or_copy(from: &Path, to: &Path) -> io::Result<()> {
    match fs::hard_link(from, to) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::AlreadyExists | io::ErrorKind::NotFound
            ) =>
        {
            Err(error)
        }
        Err(_) => copy_new(from, to).map(drop),
    }
}
/// Makes the folder `path`, whose parent exists; one already there is an error.
pub fn create_dir_new(path: &Path) -> io::Result<()> {
    fs::create_dir(path)
}
/// Removes the folder `path` when it is empty; one with anything in it is an error and stays.
pub fn remove_empty_dir(path: &Path) -> io::Result<()> {
    fs::remove_dir(path)
}

static TREE: Mutex<()> = Mutex::new(());
/// The lock the tree's scan and these operations share while it lives: listing a subfolder
/// keeps Windows from moving the folder above it, so OXY never does both at once.
pub fn tree_lock() -> MutexGuard<'static, ()> {
    TREE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `time` in UTC, as "2026-10-14T01:02:03Z".
pub fn utc(time: SystemTime) -> String {
    let seconds = time
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // The civil date of a day count (H. Hinnant's days_from_civil, reversed).
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let second = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second / 3600,
        second % 3600 / 60,
        second % 60
    )
}

/// Where the trash keeps the `n`th file the history step `step` sends to it, from the project's
/// folder: `.oxy/lixeira/<step>-<n>.<ext>`, with the extension of `from` (`modelo.json` for a
/// model). Fixed when the step is planned, so undoing and redoing it find the file.
pub fn trash_slot(step: &str, n: usize, from: &str) -> String {
    let name = from.rsplit(['/', '\\']).next().unwrap_or(from);
    let lower = name.to_lowercase();
    let extension = match lower.strip_suffix(".modelo.json") {
        Some(stem) if !stem.is_empty() => Some("modelo.json".to_owned()),
        _ => Path::new(&lower)
            .extension()
            .map(|extension| extension.to_string_lossy().into_owned()),
    };
    match extension {
        Some(extension) => format!("{TRASH}/{step}-{n}.{extension}"),
        None => format!("{TRASH}/{step}-{n}"),
    }
}
/// The file name of the trash's `slot` (`.oxy/lixeira/<name>`); none for a path elsewhere.
fn slot_name(slot: &str) -> Option<&str> {
    slot.strip_prefix(TRASH)?
        .strip_prefix('/')
        .filter(|name| !name.is_empty() && !name.contains(['/', '\\']))
}

/// What the trash notes of a file in it. Fields a newer OXY adds are read past.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashEntry {
    /// Where the file was, from the project's folder.
    pub path: String,
    /// The asset it is, for an image, a sound or a model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// When it went, in UTC.
    pub date: String,
}
/// What the trash of the project at `root` notes, by the file name of each slot; none when its
/// index is missing or unreadable (the files in it stay all the same).
pub fn trash_index(root: &Path) -> BTreeMap<String, TrashEntry> {
    names::resolve(root, TRASH_INDEX)
        .ok()
        .and_then(|file| fs::read(file).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// One step of a [`batch`], with paths from the project's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Moves or renames a file or folder, never over anything ([`move_no_replace`]).
    Move { from: String, to: String },
    /// Makes a folder that does not exist yet.
    CreateDir { path: String },
    /// Removes a folder that is empty.
    RemoveDir { path: String },
    /// Sends the file `from` to its `slot` in the trash ([`trash_slot`]), noting the asset `id`
    /// it is, if any, where it was and when.
    Trash {
        from: String,
        slot: String,
        id: Option<Id>,
    },
    /// Brings the file in the trash's `slot` back to `to`.
    Untrash {
        slot: String,
        to: String,
        id: Option<Id>,
    },
}
impl Op {
    /// The step that undoes this one.
    pub fn inverse(&self) -> Self {
        match self.clone() {
            Self::Move { from, to } => Self::Move { from: to, to: from },
            Self::CreateDir { path } => Self::RemoveDir { path },
            Self::RemoveDir { path } => Self::CreateDir { path },
            Self::Trash { from, slot, id } => Self::Untrash { slot, to: from, id },
            Self::Untrash { slot, to, id } => Self::Trash { from: to, slot, id },
        }
    }
    /// What the step does, as messages say it.
    fn action(&self) -> String {
        match self {
            Self::Move { from, to } => format!("mover {from} para {to}"),
            Self::CreateDir { path } => format!("criar a pasta {path}"),
            Self::RemoveDir { path } => format!("apagar a pasta vazia {path}"),
            Self::Trash { from, .. } => format!("mandar {from} para a lixeira do projeto"),
            Self::Untrash { to, .. } => format!("trazer {to} de volta da lixeira do projeto"),
        }
    }
}

/// Why a [`batch`] did not finish. Messages name the project's paths, as people know them.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BatchError {
    /// The operation `step` failed (noting the trash, when `step` is the number of operations)
    /// and every one done before it was undone: the disk is as it was.
    #[error("{message}")]
    RolledBack { step: usize, message: String },
    /// Undoing failed as well: the first `kept` operations stayed done, and none after them.
    #[error("{message}; também não foi possível desfazer o que já tinha sido feito ({restore})")]
    RecoveryKept {
        step: usize,
        message: String,
        restore: String,
        kept: usize,
    },
}

/// Does `ops` in order in the project at `root`, then notes in the trash's index what went to
/// it or came back. When a step fails, the steps done are undone in reverse order and the error
/// says which failed; when undoing fails too, the steps before it stay done
/// ([`BatchError::RecoveryKept`]), never part of one. Busy files get [`retrying`]; holds
/// [`tree_lock`] meanwhile.
pub fn batch(root: &Path, ops: &[Op]) -> Result<(), BatchError> {
    run_batch(root, ops, |_| {})
}
/// [`batch`], calling `before` with each operation's index before doing it.
fn run_batch(root: &Path, ops: &[Op], mut before: impl FnMut(usize)) -> Result<(), BatchError> {
    let _tree = tree_lock();
    let failed = |op: &Op, reason: String| format!("Não foi possível {}: {reason}", op.action());
    let mut done = 0;
    let mut failure = None;
    for (step, op) in ops.iter().enumerate() {
        before(step);
        if let Err(reason) = run(root, op) {
            failure = Some((step, failed(op, reason)));
            break;
        }
        done += 1;
    }
    if failure.is_none()
        && let Err(reason) = note_trash(root, ops)
    {
        failure = Some((
            ops.len(),
            format!("Não foi possível anotar a lixeira do projeto: {reason}"),
        ));
    }
    let Some((step, message)) = failure else {
        return Ok(());
    };
    while done > 0 {
        let undo = ops[done - 1].inverse();
        if let Err(reason) = run(root, &undo) {
            // The files it left in the trash say where they came from all the same.
            let _ = note_trash(root, &ops[..done]);
            return Err(BatchError::RecoveryKept {
                step,
                message,
                restore: format!("{}: {reason}", undo.action()),
                kept: done,
            });
        }
        done -= 1;
    }
    Err(BatchError::RolledBack { step, message })
}
/// Does `op` in the project at `root`; why not, when it fails.
fn run(root: &Path, op: &Op) -> Result<(), String> {
    let path = |relative: &str| names::resolve(root, relative).map_err(|e| e.to_string());
    let slot = |slot: &str| match slot_name(slot) {
        Some(_) => path(slot),
        None => Err(format!("{slot} não é uma vaga da lixeira do projeto")),
    };
    let (source, done) = match op {
        Op::Move { from, to } => {
            let (from, to) = (path(from)?, path(to)?);
            let done = retrying(|| move_no_replace(&from, &to));
            (from, done)
        }
        Op::CreateDir { path: folder } => {
            let folder = path(folder)?;
            (folder.clone(), retrying(|| create_dir_new(&folder)))
        }
        Op::RemoveDir { path: folder } => {
            let folder = path(folder)?;
            (folder.clone(), retrying(|| remove_empty_dir(&folder)))
        }
        Op::Trash { from, slot: to, .. } => {
            let (from, to) = (path(from)?, slot(to)?);
            if let Some(trash) = to.parent() {
                fs::create_dir_all(trash).map_err(|e| e.to_string())?;
            }
            let done = retrying(|| move_no_replace(&from, &to));
            (from, done)
        }
        Op::Untrash { slot: from, to, .. } => {
            let (from, to) = (slot(from)?, path(to)?);
            let done = retrying(|| move_no_replace(&from, &to));
            (from, done)
        }
    };
    done.map_err(|error| reason(op, &error, &source))
}
/// Why `op` failed with `error`, its `source` being the file or folder it acts on.
fn reason(op: &Op, error: &io::Error, source: &Path) -> String {
    let (name, target) = match op {
        Op::Move { from, to } => (from, to),
        Op::CreateDir { path } | Op::RemoveDir { path } => (path, path),
        Op::Trash { from, slot, .. } => (from, slot),
        Op::Untrash { to, .. } => (to, to),
    };
    if busy(error) {
        return if source.is_dir() {
            format!(
                "um arquivo dentro de {name} está aberto em outro programa (ou um terminal está \
                 nessa pasta)"
            )
        } else {
            format!("{name} está aberto em outro programa")
        };
    }
    match (op, error.kind()) {
        (Op::Trash { .. }, io::ErrorKind::AlreadyExists) => {
            "a vaga dele na lixeira do projeto está ocupada".into()
        }
        (_, io::ErrorKind::AlreadyExists) => format!("já existe {target}"),
        (Op::Untrash { .. }, io::ErrorKind::NotFound) if !source.exists() => {
            "ele não está mais na lixeira do projeto, que foi esvaziada".into()
        }
        (Op::CreateDir { .. }, io::ErrorKind::NotFound) => {
            format!("a pasta onde {target} ficaria não existe")
        }
        (Op::Move { .. } | Op::Untrash { .. }, io::ErrorKind::NotFound) if source.exists() => {
            format!("a pasta onde {target} ficaria não existe")
        }
        (_, io::ErrorKind::NotFound) => format!("{name} não existe"),
        (_, io::ErrorKind::DirectoryNotEmpty) => format!("a pasta {name} não está vazia"),
        _ => error.to_string(),
    }
}
/// Notes in the trash's index the files `ops` sent to it and takes out those they brought back.
/// An index OXY cannot make sense of is set aside first ([`set_aside_index`]), never written
/// over; one it cannot read fails.
fn note_trash(root: &Path, ops: &[Op]) -> Result<(), String> {
    if !ops
        .iter()
        .any(|op| matches!(op, Op::Trash { .. } | Op::Untrash { .. }))
    {
        return Ok(());
    }
    let file = names::resolve(root, TRASH_INDEX).map_err(|e| e.to_string())?;
    let mut index: BTreeMap<String, TrashEntry> = match retrying(|| fs::read(&file)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return Err(error.to_string()),
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(index) => index,
            Err(_) => {
                set_aside_index(root, &file)?;
                BTreeMap::new()
            }
        },
    };
    let date = utc(SystemTime::now());
    for op in ops {
        match op {
            Op::Trash { from, slot, id } => {
                let entry = TrashEntry {
                    path: from.clone(),
                    id: id.clone(),
                    date: date.clone(),
                };
                index.insert(slot_name(slot).unwrap_or(slot).to_owned(), entry);
            }
            Op::Untrash { slot, .. } => {
                index.remove(slot_name(slot).unwrap_or(slot));
            }
            _ => {}
        }
    }
    let bytes = serde_json::to_vec_pretty(&index).map_err(|e| e.to_string())?;
    persistence::transaction_write(vec![FileWrite::replace(file, bytes)]).map_err(|e| e.to_string())
}
/// Moves the trash's index `file`, which OXY cannot make sense of (damaged, edited by hand,
/// written by a newer OXY), beside it as `indice.<date>.json`, so the notes in it stay.
fn set_aside_index(root: &Path, file: &Path) -> Result<(), String> {
    let date = utc(SystemTime::now()).replace(':', "-");
    for name in [
        format!("indice.{date}.json"),
        format!("indice.{date}-{}.json", new_id()),
    ] {
        let aside = names::resolve(root, &format!("{TRASH}/{name}")).map_err(|e| e.to_string())?;
        match retrying(|| move_no_replace(file, &aside)) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            moved => {
                return moved.map_err(|error| {
                    format!("o índice dela não pôde ser guardado à parte ({error})")
                });
            }
        }
    }
    Err("o índice dela não pôde ser guardado à parte".into())
}
