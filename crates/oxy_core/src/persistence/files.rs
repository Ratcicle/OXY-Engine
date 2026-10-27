//! What a save writes besides the pixels painted: the project file and, in the tree layout, the
//! file of each model whose bytes changed in OXY (never over a file that is not the model's, and
//! keeping first a copy of one changed outside OXY since it last read it), and the files whose
//! assets left the project since it was last saved, sent to the project's trash so that the
//! next opening does not register them again. All of it or nothing.
use super::{
    Expected, FileWrite, KnownFiles, PersistenceError, WriteMode, copy_write, open::name_of,
    resolve_asset_path, schema_backup, transaction_write,
};
use crate::{
    document::{Asset, AssetKind, Entity, Id, Layout, Project, new_id},
    fs_ops::{self, Op},
    model_file, names, project_tree,
};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::Path,
    sync::Arc,
    time::SystemTime,
};

/// Where the copies of models' files changed outside OXY go, from the project's folder.
pub const MODEL_BACKUPS: &str = ".oxy/backup/modelos";

/// What a save of a project writes, planned before anything is written.
pub(super) struct Plan {
    /// The models' files, the backup of the project file's format and the project file, last.
    writes: Vec<FileWrite>,
    /// The files whose assets left the project, to the project's trash before the writes.
    trash: Vec<Op>,
    pub(super) warnings: Vec<String>,
    /// The models' files as the save leaves them, by model ID: path and bytes.
    models: Vec<(Id, String, Arc<[u8]>)>,
    /// The models of the project.
    ids: HashSet<Id>,
    /// The assets of the project file as last saved in the tree layout: each one's path and
    /// kind by ID.
    saved: HashMap<Id, (String, Option<AssetKind>)>,
    /// Whether the save is a copy to another folder.
    copy: bool,
    /// The images whose painted pixels the save writes, by path.
    painted: Vec<String>,
    /// The hash of the project file the save writes (see [`crate::journal::hash`]).
    manifest: String,
}

impl Plan {
    /// The write of the pixels painted on the image `asset` of `project`: over its file in the
    /// old layout, and in the tree layout when the project file as last saved has the image
    /// there, or while the file is as opening found it (`known`, for an image registered then);
    /// else the file is only created, so a file someone put at its name meanwhile stays.
    pub(super) fn painted(
        &mut self,
        root: &Path,
        project: &Project,
        asset: &Asset,
        bytes: Vec<u8>,
        known: &KnownFiles,
    ) -> Result<FileWrite, PersistenceError> {
        let target = resolve_asset_path(root, &asset.path)?;
        self.painted.push(asset.path.clone());
        let saved = project.layout == Layout::Legacy
            || self
                .saved
                .get(&asset.id)
                .is_some_and(|(path, _)| names::same_name(path, &asset.path));
        if saved {
            return Ok(FileWrite::replace(target, bytes));
        }
        Ok(match known.image(&asset.id) {
            Some((path, stamp)) if names::same_name(path, &asset.path) => FileWrite {
                target,
                bytes,
                mode: WriteMode::Replace {
                    expected: Some(Expected::Stamp(stamp)),
                },
            },
            _ => FileWrite::create(target, bytes),
        })
    }

    /// Plans the file of the model `asset`, whose pieces are `entities`: nothing when the file
    /// has these bytes, or when they are what OXY last read or wrote at its path (the model did
    /// not change in OXY; a file changed outside it stays, with a warning); only created when
    /// there is none (or it leaves for the trash first); replaced while it is the model's (OXY
    /// read it at its path, or the ID inside it is the model's), first kept in
    /// [`MODEL_BACKUPS`] when it changed since OXY read it; refused when it is another's. A copy
    /// only creates.
    fn model(
        &mut self,
        root: &Path,
        asset: &Asset,
        entities: &[Entity],
        known: &KnownFiles,
    ) -> Result<(), PersistenceError> {
        let failed = |reason: String| PersistenceError::ModelFile {
            name: asset.name.clone(),
            path: asset.path.clone(),
            reason,
        };
        let target = resolve_asset_path(root, &asset.path)
            .map_err(|error| failed(format!("não pode ser gravado: {error}")))?;
        let bytes: Arc<[u8]> = model_file::to_bytes(&asset.id, entities)?.into();
        let leaving = self
            .trash
            .iter()
            .any(|op| matches!(op, Op::Trash { from, .. } if names::same_name(from, &asset.path)));
        let disk = if leaving {
            None
        } else {
            match fs_ops::retrying(|| fs::read(&target)) {
                Ok(disk) => Some(disk),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(failed(project_tree::now_unreadable(&error))),
            }
        };
        self.models
            .push((asset.id.clone(), asset.path.clone(), bytes.clone()));
        let Some(disk) = disk else {
            self.writes.push(FileWrite::create(target, bytes.to_vec()));
            return Ok(());
        };
        if *disk == *bytes {
            return Ok(());
        }
        if self.copy {
            return Err(PersistenceError::DestinationConflict(asset.path.clone()));
        }
        let read_here = known
            .get(&asset.id)
            .filter(|(path, _)| names::same_name(path, &asset.path))
            .map(|(_, bytes)| bytes);
        if read_here == Some(&*bytes) {
            self.warnings.push(format!(
                "{} (modelo): o arquivo {} mudou fora da OXY desde que ela o leu; o salvamento o \
                 deixou como está, e reabrir o projeto carrega a versão de fora",
                asset.name, asset.path
            ));
            return Ok(());
        }
        let its_own = || model_file::from_bytes(&disk).is_ok_and(|(id, _)| id == asset.id);
        if read_here.is_none() && !its_own() {
            return Err(PersistenceError::ModelFileTaken {
                name: asset.name.clone(),
                path: asset.path.clone(),
            });
        }
        if read_here != Some(disk.as_slice()) {
            let backup = self.backup_path(root, &asset.name)?;
            self.warnings.push(format!(
                "{} (modelo): o arquivo {} mudou fora da OXY desde que ela o leu; a versão de \
                 fora foi guardada em {backup} antes de ser substituída",
                asset.name, asset.path
            ));
            self.writes.push(FileWrite::create(
                resolve_asset_path(root, &backup)?,
                disk.clone(),
            ));
        }
        self.writes.push(FileWrite {
            target,
            bytes: bytes.to_vec(),
            mode: WriteMode::Replace {
                expected: Some(Expected::Bytes(disk.into())),
            },
        });
        Ok(())
    }

    /// A free path in [`MODEL_BACKUPS`] for a copy of the file of the model `name`:
    /// `<name>-<date>.modelo.json`, numbered when taken.
    fn backup_path(&self, root: &Path, name: &str) -> Result<String, PersistenceError> {
        let name = names::fitted_name(name, "Modelo");
        let date = fs_ops::utc(SystemTime::now()).replace(':', "-");
        for n in 1.. {
            let path = if n == 1 {
                format!("{MODEL_BACKUPS}/{name}-{date}{}", model_file::EXTENSION)
            } else {
                format!("{MODEL_BACKUPS}/{name}-{date}-{n}{}", model_file::EXTENSION)
            };
            let planned = self.writes.iter().any(|write| {
                resolve_asset_path(root, &path).is_ok_and(|target| target == write.target)
            });
            if !planned && fs::symlink_metadata(resolve_asset_path(root, &path)?).is_err() {
                return Ok(path);
            }
        }
        unreachable!("a free number")
    }

    /// In a copy, the file of the model `asset`, which has no pieces (its file could not be
    /// used), copied as it is from the project's folder `source`: only created; left out, with
    /// a warning, when `source` lacks it.
    fn unavailable_model(
        &mut self,
        root: &Path,
        source: &Path,
        asset: &Asset,
    ) -> Result<(), PersistenceError> {
        let bytes = match fs::read(resolve_asset_path(source, &asset.path)?) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.warnings.push(format!(
                    "{}: o arquivo {} não existe na pasta de origem e ficou fora da cópia",
                    asset.name, asset.path
                ));
                return Ok(());
            }
            Err(source) => {
                return Err(PersistenceError::CopySource {
                    name: asset.name.clone(),
                    source,
                });
            }
        };
        self.writes.extend(copy_write(
            resolve_asset_path(root, &asset.path)?,
            bytes,
            || PersistenceError::DestinationConflict(asset.path.clone()),
        )?);
        Ok(())
    }
}

/// Plans the save of `project` at `path` (its project file), with `known`, what OXY last read or
/// wrote of the models' files; `source` is the folder of the project being copied, for a copy.
/// The old layout writes the project file alone, the models' pieces inside it. The tree layout
/// writes it without them and each model's file (see [`Plan::model`]; a copy takes the file of
/// a model without pieces as it is) and, but in a copy, sends to the project's trash the files
/// of the assets the project file last saved lists that the project no longer has (see
/// [`trash`]).
pub(super) fn plan(
    path: &Path,
    project: &Project,
    known: &KnownFiles,
    source: Option<&Path>,
) -> Result<Plan, PersistenceError> {
    let root = path.parent().unwrap_or(Path::new("."));
    let mut plan = Plan {
        writes: Vec::new(),
        trash: Vec::new(),
        warnings: Vec::new(),
        models: Vec::new(),
        ids: HashSet::new(),
        saved: HashMap::new(),
        copy: source.is_some(),
        painted: Vec::new(),
        manifest: String::new(),
    };
    let manifest = if project.layout == Layout::Legacy {
        serde_json::to_vec_pretty(project)?
    } else {
        if !plan.copy {
            plan.saved = saved_assets(path);
            trash(root, project, known, &mut plan);
        }
        let mut manifest = project.clone();
        for (asset, written) in project.assets.iter().zip(&mut manifest.assets) {
            if asset.kind != AssetKind::Model {
                continue;
            }
            plan.ids.insert(asset.id.clone());
            if let Some(entities) = written.model.take() {
                plan.model(root, asset, &entities, known)?;
            } else if let Some(source) = source {
                plan.unavailable_model(root, source, asset)?;
            }
        }
        serde_json::to_vec_pretty(&manifest)?
    };
    plan.writes.extend(schema_backup(path)?);
    plan.manifest = crate::journal::hash(&manifest);
    plan.writes
        .push(FileWrite::replace(path.to_owned(), manifest));
    Ok(plan)
}

/// The assets the project file at `path` lists, when it is of the tree layout: each one's path
/// and kind by ID. None when there is no such file or it cannot be read.
fn saved_assets(path: &Path) -> HashMap<Id, (String, Option<AssetKind>)> {
    let Some(saved) = fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    else {
        return HashMap::new();
    };
    if saved["layout"] != "Tree" {
        return HashMap::new();
    }
    saved["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|asset| {
            Some((
                asset["id"].as_str()?.to_owned(),
                (
                    asset["path"].as_str()?.to_owned(),
                    serde_json::from_value(asset["kind"].clone()).ok(),
                ),
            ))
        })
        .collect()
}

/// The files the project file at `path` lists whose assets `project` no longer has, by their
/// paths now (`journal` says where OXY moved them since): the next save sends them to the
/// project's trash, so a scan of the tree never takes them again.
pub fn departed(path: &Path, project: &Project, journal: &crate::journal::Journal) -> Vec<String> {
    let mut gone: Vec<String> = saved_assets(path)
        .into_iter()
        .filter(|(id, _)| project.asset(id).is_none())
        .filter_map(|(_, (saved, _))| journal.now_of(&saved))
        .collect();
    gone.sort();
    gone
}

/// Plans sending to the project's trash the file of each asset `plan.saved` lists that
/// `project` no longer has, once per file, when the file is there and is still that asset's:
/// its saved path is one OXY reads for its kind, and no asset of the project has the path but,
/// for a model saved again under the name of one that left, while the file holds the ID of the
/// one that left and OXY did not read it as the new one's (whose file is created after).
fn trash(root: &Path, project: &Project, known: &KnownFiles, plan: &mut Plan) {
    let step = format!("salvar-{}", &new_id()[..8]);
    let mut gone: Vec<(&Id, &String, AssetKind)> = plan
        .saved
        .iter()
        .filter(|(id, _)| project.asset(id).is_none())
        .filter_map(|(id, (path, kind))| Some((id, path, (*kind)?)))
        .collect();
    gone.sort_by(|a, b| a.1.cmp(b.1).then_with(|| a.0.cmp(b.0)));
    let mut ops = Vec::new();
    let mut taken: Vec<&str> = Vec::new();
    for (id, path, kind) in gone {
        if names::check_path(path).is_err()
            || name_of(path, kind).is_none()
            || taken.iter().any(|other| names::same_name(other, path))
        {
            continue;
        }
        let Ok(file) = resolve_asset_path(root, path) else {
            continue;
        };
        if !fs::symlink_metadata(&file).is_ok_and(|meta| meta.is_file()) {
            continue;
        }
        let claimant = project
            .assets
            .iter()
            .find(|asset| names::same_name(&asset.path, path));
        if let Some(claimant) = claimant {
            let read_as_claimant = known
                .get(&claimant.id)
                .is_some_and(|(read, _)| names::same_name(read, path));
            let departed_inside = || {
                fs_ops::retrying(|| fs::read(&file))
                    .ok()
                    .and_then(|bytes| model_file::from_bytes(&bytes).ok())
                    .is_some_and(|(inside, _)| inside == *id)
            };
            if kind != AssetKind::Model
                || claimant.kind != AssetKind::Model
                || read_as_claimant
                || !departed_inside()
            {
                continue;
            }
        }
        taken.push(path);
        plan.warnings.push(if claimant.is_some() {
            format!(
                "A versão de {path} de um modelo que saiu do projeto foi para a lixeira do \
                 projeto ({}); o modelo novo ganhou o arquivo",
                fs_ops::TRASH
            )
        } else {
            format!(
                "{path} não é mais do projeto e foi para a lixeira do projeto ({})",
                fs_ops::TRASH
            )
        });
        ops.push(Op::Trash {
            from: path.clone(),
            slot: fs_ops::trash_slot(&step, ops.len() + 1, path),
            id: Some(id.clone()),
        });
    }
    plan.trash = ops;
}

/// Does what `plan` planned, after `writes` (the pixels painted), in the project at `root`: the
/// files to the trash first, then every write in one transaction; when the writes fail, the
/// files come back from the trash. Notes in `known` the models' files as the save left them
/// (only those, in a copy) and the stamps of the files it wrote, and the journal of what OXY did
/// to the files goes: the project file says it all now. Answers what the save tells besides.
pub(super) fn commit(
    root: &Path,
    mut writes: Vec<FileWrite>,
    plan: Plan,
    known: &mut KnownFiles,
) -> Result<Vec<String>, PersistenceError> {
    writes.extend(plan.writes);
    if !plan.trash.is_empty() {
        fs_ops::batch(root, &plan.trash)
            .map_err(|error| PersistenceError::Trash(error.to_string()))?;
    }
    if let Err(error) = transaction_write(writes) {
        let back: Vec<Op> = plan.trash.iter().rev().map(Op::inverse).collect();
        if !back.is_empty()
            && let Err(restore) = fs_ops::batch(root, &back)
        {
            return Err(PersistenceError::SaveRecoveryKept {
                error: error.to_string(),
                restore: restore.to_string(),
            });
        }
        return Err(error);
    }
    if plan.copy {
        *known = KnownFiles::default();
    }
    known.retain(|id| plan.ids.contains(id));
    for path in &plan.painted {
        known.note_stamp(root, path);
    }
    for (id, path, bytes) in plan.models {
        known.note_stamp(root, &path);
        known.note(id, path, bytes);
    }
    known.saved_hash(plan.manifest);
    crate::journal::forget(root);
    Ok(plan.warnings)
}
