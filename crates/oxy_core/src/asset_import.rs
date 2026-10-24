//! PNG images and WAV sounds imported by AI agents: the editor's Importar without the file
//! dialog. The source, a file the person pointed to or bytes sent inline, is checked first (its
//! type, size and header), then copied into the project: in the old layout into its assets
//! folder under a new name, as the editor does; in the tree layout as `<name>.<ext>` in the
//! folder asked for (the root by default, made when missing), a name an asset of its kind has
//! refused with a free one suggested, and a file already in the project's tree registered where
//! it is. Nothing is written over. One call may import several files (`paths`), all of them or
//! none. The MCP server imports this way on files and the editor on its live channel, so both
//! accept and refuse the same sources.
use crate::{
    document::*,
    names::{self, MAX_NAME_CHARS},
    painting::PaintImage,
    persistence::{self, resolve_asset_path},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::path::{Component, Path, PathBuf, Prefix};

/// Largest file imported: a stereo WAV of about 6 minutes at 44.1 kHz and 16 bits.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Largest content sent inline (`data_base64`), once decoded.
pub const MAX_INLINE_BYTES: usize = 16 * 1024 * 1024;
/// Files one call imports at most (`paths`).
pub const MAX_FILES: usize = 32;
/// Most bytes the files of one call add up to.
pub const MAX_BATCH_BYTES: u64 = 4 * MAX_FILE_BYTES;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// What an imported file becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    /// A PNG image, for materials, sprites and interface images.
    Texture,
    /// A WAV sound, for action.sound nodes.
    Audio,
}
impl ImportKind {
    fn asset(self) -> AssetKind {
        match self {
            Self::Texture => AssetKind::Texture,
            Self::Audio => AssetKind::Audio,
        }
    }
    /// The kind a file name's extension says, if it is .png or .wav.
    fn of_extension(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "png" => Some(Self::Texture),
            "wav" => Some(Self::Audio),
            _ => None,
        }
    }
    /// The kind the content's header says.
    fn of_header(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(PNG_SIGNATURE) {
            Some(Self::Texture)
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
            Some(Self::Audio)
        } else {
            None
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Texture => "imagem PNG",
            Self::Audio => "som WAV",
        }
    }
}

/// An import as the tool takes it: exactly one of `path` and `data_base64`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    /// Absolute path of a .png or .wav file on this computer.
    #[serde(default)]
    pub path: Option<String>,
    /// The file's content in base64 (a data URL too).
    #[serde(default)]
    pub data_base64: Option<String>,
    /// Checked against the extension and the header; found from them when absent.
    #[serde(default)]
    pub kind: Option<ImportKind>,
    /// The asset's name; by default the file's name without the extension.
    #[serde(default)]
    pub name: Option<String>,
    /// An object of `scene` that shows the image: its material, or its image when it is an
    /// interface element. A sprite takes the image's proportions, as in the editor.
    #[serde(default)]
    pub apply_to: Option<Id>,
    /// The scene of `apply_to`, by its ID or name.
    #[serde(default)]
    pub scene: Option<Id>,
    /// In the tree layout, the folder the file goes to, by its path from the project's folder
    /// (the root by default), made when missing.
    #[serde(default)]
    pub folder: Option<String>,
}

/// A source read and checked, ready to be copied into a project.
#[derive(Clone, Debug)]
pub struct ImportSource {
    pub bytes: Vec<u8>,
    pub kind: ImportKind,
    pub name: String,
    /// The decoded pixels of an image, for the editor's texture cache.
    pub image: Option<PaintImage>,
    /// Length of a sound in seconds.
    pub seconds: Option<f64>,
}

/// What an import added, as the tool answers it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Imported {
    pub asset: Id,
    pub name: String,
    pub kind: ImportKind,
    /// Where the copy is, relative to the project folder.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_to: Option<Id>,
    /// The file was in the project's tree already: registered where it is, nothing copied.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub in_place: bool,
    /// The folders the import made for the file, each before those inside it.
    #[serde(skip)]
    pub made: Vec<String>,
}

impl ImportRequest {
    /// The request from the tool's arguments, with what can be checked without reading files:
    /// one source, an absolute local path to a .png or .wav, a kind that agrees with it.
    pub fn parse(args: &Json) -> Result<Self, String> {
        let mut request: Self = serde_json::from_value(args.clone())
            .map_err(|e| format!("Importação inválida: {e}"))?;
        match (&request.path, &request.data_base64) {
            (Some(_), Some(_)) => return Err("Informe path ou data_base64, não os dois.".into()),
            (None, None) => {
                return Err(
                    "Informe path (um arquivo .png ou .wav), data_base64 ou paths \
                     (vários arquivos)."
                        .into(),
                );
            }
            (Some(path), None) => {
                let found = check_path(path)?;
                if let Some(kind) = request.kind
                    && kind != found
                {
                    return Err(format!(
                        "O arquivo é {} pela extensão, mas kind pede {}.",
                        found.label(),
                        kind.label()
                    ));
                }
            }
            (None, Some(data)) => {
                // Base64 grows 4/3, and line breaks a little more.
                if data.len() > MAX_INLINE_BYTES * 2 {
                    return Err(too_large_inline());
                }
            }
        }
        if request.apply_to.is_some() && request.kind() == Some(ImportKind::Audio) {
            return Err("apply_to só vale para imagens: um som toca pelo nó action.sound.".into());
        }
        // The root is where a file goes without a folder ("" or "/" name it).
        if let Some(folder) = &request.folder
            && folder_parts(folder)?.is_empty()
        {
            request.folder = None;
        }
        Ok(request)
    }

    /// The kind asked for or, for a path, the one its extension gives.
    fn kind(&self) -> Option<ImportKind> {
        self.kind.or_else(|| {
            self.path
                .as_deref()
                .and_then(|p| ImportKind::of_extension(Path::new(p.trim())))
        })
    }

    /// Reads the source and checks it in full: size, header and content (the whole image is
    /// decoded, the sound's format read). Nothing is written.
    pub fn read(&self) -> Result<ImportSource, String> {
        let (bytes, file_name) = match (&self.path, &self.data_base64) {
            (Some(path), _) => {
                check_path(path)?;
                // The path checked, without the spaces around it.
                let path = Path::new(path.trim());
                let meta = std::fs::metadata(path)
                    .map_err(|e| format!("Não foi possível abrir {}: {e}", path.display()))?;
                if !meta.is_file() {
                    return Err(format!("{} não é um arquivo.", path.display()));
                }
                if meta.len() > MAX_FILE_BYTES {
                    return Err(format!(
                        "{} tem mais de {} MB, o limite de importação.",
                        path.display(),
                        MAX_FILE_BYTES / (1024 * 1024)
                    ));
                }
                let bytes = std::fs::read(path)
                    .map_err(|e| format!("Não foi possível ler {}: {e}", path.display()))?;
                // The file may have grown since the size was read.
                if bytes.len() as u64 > MAX_FILE_BYTES {
                    return Err(format!(
                        "{} tem mais de {} MB, o limite de importação.",
                        path.display(),
                        MAX_FILE_BYTES / (1024 * 1024)
                    ));
                }
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                (bytes, stem)
            }
            (None, Some(data)) => {
                if data.len() > MAX_INLINE_BYTES * 2 {
                    return Err(too_large_inline());
                }
                let bytes = crate::encoding::decode_base64(data)
                    .map_err(|e| format!("data_base64 inválido: {e}"))?;
                if bytes.len() > MAX_INLINE_BYTES {
                    return Err(too_large_inline());
                }
                (bytes, None)
            }
            (None, None) => return Err("Informe path ou data_base64.".into()),
        };
        let found = ImportKind::of_header(&bytes)
            .ok_or("O conteúdo não é uma imagem PNG nem um som WAV.")?;
        if let Some(kind) = self.kind()
            && kind != found
        {
            return Err(format!(
                "O conteúdo é {}, mas {} pede {}.",
                found.label(),
                if self.kind.is_some() {
                    "kind"
                } else {
                    "a extensão"
                },
                kind.label()
            ));
        }
        if self.apply_to.is_some() && found == ImportKind::Audio {
            return Err("apply_to só vale para imagens: um som toca pelo nó action.sound.".into());
        }
        let (image, seconds) = match found {
            ImportKind::Texture => (Some(PaintImage::from_png(&bytes)?), None),
            ImportKind::Audio => {
                persistence::validate_wav(&bytes)?;
                let reader = hound::WavReader::new(std::io::Cursor::new(&bytes))
                    .map_err(|e| format!("WAV inválido: {e}"))?;
                let rate = f64::from(reader.spec().sample_rate.max(1));
                (None, Some(f64::from(reader.duration()) / rate))
            }
        };
        let name = clean_name(self.name.as_deref().or(file_name.as_deref()).unwrap_or(""));
        let name = if name.is_empty() {
            match found {
                ImportKind::Texture => "Imagem importada".to_owned(),
                ImportKind::Audio => "Som importado".to_owned(),
            }
        } else {
            name
        };
        Ok(ImportSource {
            bytes,
            kind: found,
            name,
            image,
            seconds,
        })
    }
}

/// What one call imports: one source (`path` or `data_base64`), or several files in `paths`,
/// each a path or `{path, name?, kind?, apply_to?}`, imported together or not at all.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportBatch {
    pub requests: Vec<ImportRequest>,
    /// Given as `paths`: errors name the file, and the answer lists them.
    pub listed: bool,
}

impl ImportBatch {
    /// The batch from the tool's arguments, each file checked as [`ImportRequest::parse`]
    /// checks one. With `paths`, `scene` is the one of every `apply_to`.
    pub fn parse(args: &Json) -> Result<Self, String> {
        let Some(paths) = args.get("paths").filter(|paths| !paths.is_null()) else {
            // A null paths, as some clients send for what they leave out, is no field at all.
            let mut args = args.clone();
            if let Some(map) = args.as_object_mut() {
                map.remove("paths");
            }
            return ImportRequest::parse(&args).map(|request| Self {
                requests: vec![request],
                listed: false,
            });
        };
        const ENTRY: &str = "{path, name?, kind?, apply_to?, folder?}";
        let Json::Array(paths) = paths else {
            return Err(format!(
                "paths precisa ser uma lista de arquivos: caminhos ou {ENTRY}."
            ));
        };
        for (key, value) in args.as_object().into_iter().flatten() {
            match key.as_str() {
                "paths" | "scene" | "folder" => {}
                _ if value.is_null() => {}
                "path" | "data_base64" => {
                    return Err("Informe path, data_base64 ou paths, só um deles.".into());
                }
                "name" | "kind" | "apply_to" => {
                    return Err(format!(
                        "Com paths, {key} vai em cada arquivo ({ENTRY}), não fora da lista."
                    ));
                }
                other => return Err(format!("Importação inválida: campo `{other}` desconhecido")),
            }
        }
        if paths.is_empty() {
            return Err("paths está vazia: informe ao menos um arquivo.".into());
        }
        if paths.len() > MAX_FILES {
            return Err(format!(
                "paths tem {} arquivos; importe no máximo {MAX_FILES} por chamada.",
                paths.len()
            ));
        }
        let requests = paths
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let mut fields = match entry {
                    Json::String(path) => serde_json::json!({ "path": path }),
                    Json::Object(map) => {
                        if let Some(key) = map.keys().find(|k| {
                            !matches!(k.as_str(), "path" | "name" | "kind" | "apply_to" | "folder")
                        }) {
                            return Err(file_error(
                                i,
                                format!("campo `{key}` desconhecido; cada arquivo é {ENTRY}"),
                            ));
                        }
                        if !map.contains_key("path") {
                            return Err(file_error(i, format!("informe path ({ENTRY})")));
                        }
                        entry.clone()
                    }
                    _ => {
                        return Err(file_error(
                            i,
                            format!("cada arquivo é um caminho ou {ENTRY}"),
                        ));
                    }
                };
                if let Some(scene) = args.get("scene").filter(|scene| !scene.is_null()) {
                    fields["scene"] = scene.clone();
                }
                // The folder beside paths is each file's that names none.
                if let Some(folder) = args.get("folder").filter(|folder| !folder.is_null())
                    && fields.get("folder").is_none_or(Json::is_null)
                {
                    fields["folder"] = folder.clone();
                }
                ImportRequest::parse(&fields).map_err(|error| file_error(i, error))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            requests,
            listed: true,
        })
    }

    /// Reads and checks every source in full, as [`ImportRequest::read`] does one, before
    /// anything is written.
    pub fn read(&self) -> Result<Vec<ImportSource>, String> {
        let mut total = 0;
        let mut sources = Vec::new();
        for (i, request) in self.requests.iter().enumerate() {
            let source = request.read().map_err(|error| self.error(i, error))?;
            total += source.bytes.len() as u64;
            if total > MAX_BATCH_BYTES {
                return Err(format!(
                    "Os arquivos de paths passam de {} MB juntos; importe-os em mais de uma \
                     chamada.",
                    MAX_BATCH_BYTES / (1024 * 1024)
                ));
            }
            sources.push(source);
        }
        Ok(sources)
    }

    /// The tool's answer: what the import added or, for `paths`, `{imported: [...]}` in their
    /// order.
    pub fn answer(&self, imported: &[Imported]) -> Result<Json, String> {
        match imported {
            [one] if !self.listed => serde_json::to_value(one).map_err(|e| e.to_string()),
            all => Ok(serde_json::json!({ "imported": all })),
        }
    }

    /// `error` of file `i`, naming it when the batch lists files.
    fn error(&self, i: usize, error: String) -> String {
        if self.listed {
            file_error(i, error)
        } else {
            error
        }
    }
}

/// `error` of file `i` of `paths`, counted from 0 and in words.
fn file_error(i: usize, error: String) -> String {
    format!("paths[{i}] (o {}º arquivo): {error}", i + 1)
}

fn too_large_inline() -> String {
    format!(
        "data_base64 passa de {} MB; para arquivos maiores, use path.",
        MAX_INLINE_BYTES / (1024 * 1024)
    )
}

/// A path given in full on a disk of this computer, without the spaces around it: network
/// paths are refused, since one reaches another machine, and so are device paths (`\\.\`,
/// `\\?\`) and relative ones. The import reads files this way and the export writes games;
/// `folder` says which the messages name.
pub fn local_disk_path(path: &str, folder: bool) -> Result<&Path, String> {
    let trimmed = path.trim();
    let (one, the) = if folder {
        ("uma pasta", "da pasta")
    } else {
        ("um arquivo", "do arquivo")
    };
    let not_on_a_disk = || {
        format!(
            "Caminhos de rede ou de dispositivo não são aceitos; use {one} de um disco deste \
             computador."
        )
    };
    // Windows takes '/' and '\' alike: "\/servidor/pasta" and "/\servidor\pasta" are network
    // paths as much as "\\servidor\pasta".
    let separators = trimmed.chars().take_while(|c| matches!(c, '/' | '\\'));
    if separators.count() >= 2 {
        return Err(not_on_a_disk());
    }
    let local = Path::new(trimmed);
    // And what Windows reads the path as decides: only a drive letter is a disk of this computer.
    if let Some(Component::Prefix(prefix)) = local.components().next()
        && !matches!(prefix.kind(), Prefix::Disk(_))
    {
        return Err(not_on_a_disk());
    }
    if !local.is_absolute() {
        return Err(format!(
            "Informe o caminho completo {the} (absoluto), não {trimmed}."
        ));
    }
    Ok(local)
}
/// `path` with `.` and `..` worked out.
pub(crate) fn normalized(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}
/// `path` with the part that exists resolved by the system (links, case, short names) and the
/// rest as given.
pub(crate) fn real(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(found) = std::fs::canonicalize(existing) {
            return rest.iter().rev().fold(found, |path, part| path.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return path.to_owned(),
        }
    }
}
/// Whether one of `a` and `b` is the other or lies in it, without regard to case.
pub(crate) fn overlaps(a: &Path, b: &Path) -> bool {
    let parts = |path: &Path| -> Vec<String> {
        path.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    let (a, b) = (parts(a), parts(b));
    let shared = a.len().min(b.len());
    a[..shared] == b[..shared]
}

/// A path the import may read: absolute, on a disk of this computer (see [`local_disk_path`])
/// and ending in .png or .wav. Answers the kind it gives.
fn check_path(path: &str) -> Result<ImportKind, String> {
    let file = local_disk_path(path, false)?;
    ImportKind::of_extension(file)
        .ok_or_else(|| format!("Só se importam arquivos .png e .wav, não {}.", path.trim()))
}

/// The name shown in the editor: one line, no control characters, at most
/// [`MAX_NAME_CHARS`] characters.
fn clean_name(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect();
    words
        .join(" ")
        .chars()
        .take(MAX_NAME_CHARS)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Shows texture `asset` on `entity`: in its image when it is an interface element, else in its
/// material. A sprite takes the image's proportions (`size`), keeping its height, as the
/// editor's Importar does.
pub fn apply_texture(entity: &mut Entity, asset: &str, size: Option<[u32; 2]>) {
    if let Some(ui) = &mut entity.ui {
        ui.texture = Some(asset.to_owned());
        return;
    }
    entity.material.texture = Some(asset.to_owned());
    if entity.primitive == Some(Primitive::Sprite)
        && let Some([width, height]) = size
        && height > 0
    {
        entity.dimensions[0] = entity.dimensions[1] * width as f32 / height as f32;
    }
}

/// What an import makes in the project, told right before it is made (see
/// [`crate::journal::Creating`]): a folder, or the file of the asset `id` at `path`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Making<'a> {
    Folder(&'a str),
    File { path: &'a str, id: &'a str },
}

/// Why a folder of an import is refused in the old layout.
const LEGACY_FOLDER: &str = "Este projeto ainda está no formato antigo, sem pastas, e os recursos \
    vão para a pasta assets: importe sem folder (a gravação organiza o projeto em pastas, com uma \
    cópia completa antes) e depois leve o arquivo para a pasta com oxy_apply move";

/// The parts of the folder path `text`, each a name Windows keeps and none starting with a dot.
fn folder_parts(text: &str) -> Result<Vec<String>, String> {
    let text = text.trim().replace('\\', "/");
    let text = text.trim_matches('/');
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split('/')
        .map(|part| names::check_name(part).map_err(|why| format!("folder inválido: {why}")))
        .collect()
}

/// Copies `source` into the project in `root` and adds the asset, applied to the request's
/// `apply_to` in its `scene`, by ID or name (`scene` by default). The project is validated
/// before it changes; on failure it is left as it was and no file or folder stays behind.
pub fn import(
    project: &mut Project,
    root: &Path,
    scene: &str,
    request: &ImportRequest,
    source: &ImportSource,
) -> Result<Imported, String> {
    import_with(project, root, scene, request, source, &mut |_| Ok(()))
}

/// [`import`], calling `before` with each folder and file it makes right before it makes it
/// (see [`crate::journal::Creating`]); nothing is made when it fails.
pub fn import_with(
    project: &mut Project,
    root: &Path,
    scene: &str,
    request: &ImportRequest,
    source: &ImportSource,
    before: &mut dyn FnMut(Making) -> Result<(), String>,
) -> Result<Imported, String> {
    let found = project.find_scene(request.scene.as_deref().unwrap_or(scene), "scene")?;
    if let Some(target) = &request.apply_to {
        if source.kind != ImportKind::Texture {
            return Err("apply_to só vale para imagens: um som toca pelo nó action.sound.".into());
        }
        let entity = found
            .entity(target)
            .ok_or_else(|| format!("Objeto {target} não existe na cena {}", found.name))?;
        if entity.ui.is_none() && !entity.has_geometry() {
            return Err(format!(
                "{} não tem forma nem é elemento da interface: a imagem não apareceria nele.",
                entity.name
            ));
        }
    }
    let scene = found.id.clone();
    let tree = project.layout == Layout::Tree;
    if !tree && request.folder.is_some() {
        return Err(LEGACY_FOLDER.into());
    }
    let mut copy = project.clone();
    let size = source.image.as_ref().map(|i| [i.width, i.height]);
    let inside = match (&request.path, tree) {
        (Some(path), true) => inside_tree(root, Path::new(path.trim())),
        _ => None,
    };
    let (id, name, path, made, in_place) = match inside {
        Some(relative) => {
            let id = in_place(&mut copy, root, &relative, request, source.kind)?;
            let asset = copy.asset(&id).expect("registered");
            (
                id.clone(),
                asset.name.clone(),
                asset.path.clone(),
                Vec::new(),
                true,
            )
        }
        None => {
            let (folder, missing) = if tree {
                folder_of(root, request.folder.as_deref().unwrap_or_default())?
            } else {
                (String::new(), Vec::new())
            };
            if tree {
                free_name(&copy, root, &folder, request, source)?;
            }
            let made = make_folders(root, &missing, before)?;
            let stored = persistence::store_asset_in(
                &mut copy,
                root,
                &source.bytes,
                source.name.clone(),
                source.kind.asset(),
                &folder,
                &mut |path, id| before(Making::File { path, id }),
            );
            let id = match stored {
                Ok(id) => id,
                Err(error) => {
                    remove_folders(root, &made);
                    return Err(error.to_string());
                }
            };
            let (name, path) = copy
                .asset(&id)
                .map(|a| (a.name.clone(), a.path.clone()))
                .unwrap_or_default();
            (id, name, path, made, false)
        }
    };
    if let Some(target) = &request.apply_to
        && let Some(entity) = copy.scene_mut(&scene).and_then(|s| s.entity_mut(target))
    {
        apply_texture(entity, &id, size);
    }
    if let Err(error) = validate_project(&copy) {
        if !in_place && let Ok(file) = resolve_asset_path(root, &path) {
            let _ = std::fs::remove_file(file);
        }
        remove_folders(root, &made);
        return Err(error);
    }
    *project = copy;
    Ok(Imported {
        asset: id,
        name,
        kind: source.kind,
        path,
        width: size.map(|s| s[0]),
        height: size.map(|s| s[1]),
        seconds: source.seconds,
        applied_to: request.apply_to.clone(),
        in_place,
        made,
    })
}

/// The path from the project's folder `root` of the file `source`, when it is inside it.
fn inside_tree(root: &Path, source: &Path) -> Option<String> {
    let root = real(&normalized(root));
    let source = real(&normalized(source));
    let inside = source.strip_prefix(&root).ok()?;
    let parts: Option<Vec<&str>> = inside
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect();
    Some(parts?.join("/")).filter(|path| !path.is_empty())
}

/// Registers the file `relative`, already in the project's tree, as it is: the asset that has
/// it, or a new one named after it. Answers its ID. A name or folder other than the file's is
/// refused (renaming and moving are oxy_apply's), and so is a file the tree leaves out.
fn in_place(
    project: &mut Project,
    root: &Path,
    relative: &str,
    request: &ImportRequest,
    kind: ImportKind,
) -> Result<Id, String> {
    if crate::project_tree::presence(root, relative) != crate::project_tree::Presence::Present {
        return Err(format!(
            "O arquivo {relative} está na pasta do projeto, mas fica fora da Biblioteca (numa \
             pasta que começa com ponto, num atalho ou noutro projeto); leve-o para uma pasta do \
             projeto antes"
        ));
    }
    let relative = names::disk_case(root, relative).map_err(|e| e.to_string())?;
    let id = match project
        .assets
        .iter()
        .find(|a| !a.path.is_empty() && names::same_name(&a.path, &relative))
    {
        Some(asset) if asset.kind != kind.asset() => {
            return Err(format!(
                "O arquivo {relative} já é {} do projeto",
                persistence::kind_label(asset.kind)
            ));
        }
        Some(asset) => asset.id.clone(),
        None => {
            let name = persistence::name_of(&relative, kind.asset())
                .ok_or_else(|| format!("O arquivo {relative} não termina em .png nem em .wav"))?;
            let id = new_id();
            project.assets.push(Asset {
                id: id.clone(),
                name,
                path: relative.clone(),
                kind: kind.asset(),
                model: None,
            });
            id
        }
    };
    let asset = project.asset(&id).expect("registered");
    if let Some(name) = &request.name
        && name.trim() != asset.name
    {
        return Err(format!(
            "O arquivo {relative} já está no projeto como {}, e o nome é o do arquivo: para \
             mudá-lo, use oxy_apply rename_asset",
            asset.name
        ));
    }
    if let Some(folder) = &request.folder {
        let parent = relative.rsplit_once('/').map_or("", |(folder, _)| folder);
        if !names::same_name(folder.trim().trim_matches(['/', '\\']), parent) {
            return Err(format!(
                "O arquivo {relative} já está no projeto; para levá-lo a outra pasta, use \
                 oxy_apply move"
            ));
        }
    }
    Ok(id)
}

/// The folder `text` of the project at `root` in the disk's case, with the folders on its way
/// that are missing, each before those inside it; refused when a part names a file, a link or
/// another project, where what is written would be outside the tree.
fn folder_of(root: &Path, text: &str) -> Result<(String, Vec<String>), String> {
    let mut current = String::new();
    let mut missing: Vec<String> = Vec::new();
    for part in folder_parts(text)? {
        let next = crate::journal::join(&current, &part);
        let next = names::disk_case(root, &next).unwrap_or(next);
        names::check_path(&next).map_err(|why| format!("folder inválido: {why}"))?;
        if missing.is_empty() {
            let place = resolve_asset_path(root, &next).map_err(|e| e.to_string())?;
            match std::fs::symlink_metadata(&place) {
                Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
                Ok(_) => return Err(format!("{next} já existe e não é uma pasta do projeto")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing.push(next.clone());
                }
                Err(error) => return Err(format!("Não foi possível ler a pasta {next}: {error}")),
            }
            if let Some(why) = crate::project_tree::outside_reason(root, &format!("{next}/x")) {
                return Err(format!("A pasta {next} fica fora do projeto: {why}"));
            }
        } else {
            missing.push(next.clone());
        }
        current = next;
    }
    Ok((current, missing))
}

/// Makes the folders `missing` of the project at `root`, each before those inside it, `before`
/// told of each first. Answers them; none stays when one fails.
fn make_folders(
    root: &Path,
    missing: &[String],
    before: &mut dyn FnMut(Making) -> Result<(), String>,
) -> Result<Vec<String>, String> {
    let mut made: Vec<String> = Vec::new();
    for folder in missing {
        let done = before(Making::Folder(folder)).and_then(|()| {
            let place = resolve_asset_path(root, folder).map_err(|e| e.to_string())?;
            crate::fs_ops::create_dir_new(&place)
                .map_err(|e| format!("Não foi possível criar a pasta {folder}: {e}"))
        });
        if let Err(error) = done {
            remove_folders(root, &made);
            return Err(error);
        }
        made.push(folder.clone());
    }
    Ok(made)
}

/// Takes away the folders `made` (each before those inside it) when they are still empty.
fn remove_folders(root: &Path, made: &[String]) {
    for folder in made.iter().rev() {
        if let Ok(place) = resolve_asset_path(root, folder) {
            let _ = crate::fs_ops::remove_empty_dir(&place);
        }
    }
}

/// Refuses the name the file of `source` takes in the folder `folder` of the tree layout when an
/// asset of its kind has it or a file has its path, suggesting a free one; a name the request
/// gives that Windows would not keep is refused with why.
fn free_name(
    project: &Project,
    root: &Path,
    folder: &str,
    request: &ImportRequest,
    source: &ImportSource,
) -> Result<(), String> {
    let kind = source.kind.asset();
    let wanted = match &request.name {
        Some(name) => names::check_name(name).map_err(|why| format!("Nome inválido: {why}"))?,
        None => source.name.clone(),
    };
    let (free, path) =
        persistence::new_asset_file_in(project, Some(root), "", &wanted, kind, folder);
    let fallback = if kind == AssetKind::Texture {
        "Imagem"
    } else {
        "Som"
    };
    if free == names::fitted_name(&wanted, fallback) {
        return Ok(());
    }
    let taken = project
        .assets
        .iter()
        .find(|a| a.kind == kind && a.name.trim().to_lowercase() == wanted.to_lowercase())
        .map_or_else(
            || {
                let extension = path.rsplit('.').next().unwrap_or_default();
                format!(
                    "o arquivo {}",
                    crate::journal::join(folder, &format!("{wanted}.{extension}"))
                )
            },
            |other| {
                format!(
                    "{} \"{}\" ({})",
                    persistence::kind_label(kind),
                    other.name,
                    other.path
                )
            },
        );
    Err(format!(
        "Já existe {taken}: o nome é único por tipo no projeto e a OXY não grava por cima; passe \
         outro name ({free} está livre)"
    ))
}

/// Imports every file of `batch`, read as `sources`, as [`import`] does one: all of them, in
/// order, or none, with no copy left behind and the error naming the file.
pub fn import_batch(
    project: &mut Project,
    root: &Path,
    scene: &str,
    batch: &ImportBatch,
    sources: &[ImportSource],
) -> Result<Vec<Imported>, String> {
    import_batch_with(project, root, scene, batch, sources, &mut |_| Ok(()))
}

/// [`import_batch`], calling `before` before each file is written (see [`import_with`]).
pub fn import_batch_with(
    project: &mut Project,
    root: &Path,
    scene: &str,
    batch: &ImportBatch,
    sources: &[ImportSource],
    before: &mut dyn FnMut(Making) -> Result<(), String>,
) -> Result<Vec<Imported>, String> {
    let mut copy = project.clone();
    let mut imported = Vec::new();
    for (i, (request, source)) in batch.requests.iter().zip(sources).enumerate() {
        match import_with(&mut copy, root, scene, request, source, before) {
            Ok(done) => imported.push(done),
            Err(error) => {
                remove_copies(root, &imported);
                return Err(batch.error(i, error));
            }
        }
    }
    *project = copy;
    Ok(imported)
}

/// Takes away the files `imported` copied into the project in `root`, and the folders made for
/// them once empty, when the project that names them is not saved after all; a file registered
/// where it was stays.
pub fn remove_copies(root: &Path, imported: &[Imported]) {
    for imported in imported.iter().rev() {
        if !imported.in_place
            && let Ok(file) = resolve_asset_path(root, &imported.path)
        {
            let _ = std::fs::remove_file(file);
        }
        remove_folders(root, &imported.made);
    }
}
