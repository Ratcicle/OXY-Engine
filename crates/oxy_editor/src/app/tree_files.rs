//! The project's files as the editor follows them without a watcher always on: the tree is
//! walked again when the window gets the focus back, with F5, around OXY's own operations and
//! before an agent's command that names a path, and what changed outside OXY reaches the
//! project between two steps of the undo history (a new file registered, a path in the disk's
//! case, a model found by its ID, a file missing told, an image changed read again).
//! "Descartar alterações" also brings the files back to the last save.
use super::*;
use oxy_core::{journal, project_tree};

impl Editor {
    /// The folder of the open project, when it was saved in one.
    pub(crate) fn project_root(&self) -> Option<PathBuf> {
        self.path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_owned)
    }

    /// Walks the project's tree again and applies what changed outside OXY since the last walk;
    /// waits for the step in progress and the test to end. A project of the old layout is not
    /// walked (its first save organizes it), nor, but with F5 (`scan_forced`), a tree the last
    /// walk could not take (too large for a project's folder).
    pub(crate) fn scan_project(&mut self) {
        let forced = std::mem::take(&mut self.scan_forced);
        self.scan_requested = false;
        let Some(path) = self.path.clone() else {
            return;
        };
        if self.state.project.layout == Layout::Legacy {
            self.tree_scan = None;
            return;
        }
        if self.history.is_pending() || self.playing() {
            self.scan_requested = true;
            self.scan_forced = forced;
            return;
        }
        let Some(before) = self.tree_scan.as_ref() else {
            if forced {
                let root = self.root();
                match project_tree::walk(&root) {
                    Ok(tree) => self.tree_scan = Some(tree),
                    Err(error) => {
                        self.log(format!("A OXY não conferiu a pasta do projeto: {error}"));
                        self.notice_last(true);
                    }
                }
            }
            return;
        };
        let (after, changes) =
            match persistence::scan(&path, before, &self.state.project, &self.known_files) {
                Ok(scanned) => scanned,
                Err(error) => {
                    let line = format!("A OXY não conferiu a pasta do projeto: {error}");
                    if self.console.messages.last() != Some(&line) {
                        self.log(line);
                        self.notice_last(true);
                    }
                    return;
                }
            };
        self.tree_scan = Some(after);
        if changes.is_empty() {
            return;
        }
        let root = self.root();
        let adopted = persistence::adopt(
            &mut self.state.project,
            &root,
            &changes,
            &mut self.known_files,
        );
        self.state.images.configure(&root, &self.state.project);
        for fix in adopted.fixes {
            self.log(fix);
        }
        for warning in adopted.warnings {
            self.log(warning);
            self.notice_last(true);
        }
        for id in adopted.images {
            let Some(asset) = self.state.project.asset(&id).cloned() else {
                continue;
            };
            if self.state.images.is_dirty(&id) {
                self.log(format!(
                    "{}: o arquivo {} mudou fora da OXY; a sua pintura, ainda não salva, \
                     continua na OXY, e salvar grava a pintura por cima do arquivo (desfaça a \
                     pintura antes de salvar para ficar com a versão de fora)",
                    asset.name, asset.path
                ));
                self.notice_last(true);
                continue;
            }
            self.state.images.remove(&id);
            self.state.images.configure(&root, &self.state.project);
            self.renderer.clear_texture_override(&id);
            self.play.game_ui.clear_texture_override(&id);
            if self.ensure_texture(&id) {
                self.refresh_texture(&id);
                self.log(format!(
                    "{}: o arquivo {} mudou fora da OXY e foi lido de novo",
                    asset.name, asset.path
                ));
            }
        }
        self.asset_ui.thumbnail = None;
    }

    /// Takes the tree as it is now as the last walk, after OXY's own operations: they are no
    /// change from outside.
    pub(crate) fn rebase_scan(&mut self) {
        self.tree_scan = self
            .project_root()
            .filter(|_| self.state.project.layout == Layout::Tree)
            .and_then(|root| project_tree::walk(&root).ok());
    }

    /// "Descartar alterações": brings the files OXY moved, deleted or imported since the last
    /// save back to how it was, telling what stayed (a file open in another program). Answers
    /// whether it touched the files: the project open no longer matches them.
    pub(crate) fn discard_files(&mut self) -> bool {
        let Some(root) = self.project_root() else {
            return false;
        };
        if self.known_files.journal().is_clean() {
            return false;
        }
        let discarded = journal::discard(&root, &mut self.known_files);
        if discarded.restored > 0 {
            self.log(format!(
                "Descartar: {} {} do projeto voltaram a como estavam no último salvamento.",
                discarded.restored,
                if discarded.restored == 1 {
                    "arquivo ou pasta"
                } else {
                    "arquivos e pastas"
                }
            ));
        }
        if !discarded.kept.is_empty() {
            self.log(format!(
                "Descartar: alguns arquivos ficaram como estão, e a próxima abertura do projeto \
                 os encontra onde estão: {}",
                discarded.kept.join("; ")
            ));
            self.notice_last(true);
        }
        true
    }
}

/// The warning, said once when the project opens, about a project inside a folder that OneDrive
/// keeps in sync: it can make copies on conflicts and fight over renamed files.
pub(crate) fn synced_folder(root: &Path) -> Option<String> {
    let folded = |path: &Path| -> Option<String> {
        let path = oxy_core::fs_ops::canonical(path).ok()?;
        Some(
            path.to_string_lossy()
                .chars()
                .map(oxy_core::names::fold)
                .collect(),
        )
    };
    let root = folded(root)?;
    let synced = ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]
        .iter()
        .filter_map(std::env::var_os)
        .filter_map(|folder| folded(Path::new(&folder)))
        .any(|folder| {
            let folder = folder.trim_end_matches(['\\', '/']);
            root.strip_prefix(folder)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(['\\', '/']))
        });
    synced.then(|| {
        "O projeto está numa pasta que o OneDrive sincroniza: a sincronização pode criar cópias \
         de arquivos em conflito e disputar arquivos renomeados ou movidos pela OXY; prefira uma \
         pasta fora dela."
            .to_owned()
    })
}
