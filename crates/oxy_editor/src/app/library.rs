//! The dock's Biblioteca: the project's real folders (the PASTAS column, a breadcrumb in a narrow
//! dock), "Todos os recursos" as one flat list, the chips that filter by kind inside the open
//! folder, a search over the whole project, and the compact list of images, sounds, models and
//! scripts with their state, how they are used and the action of their kind. Renaming, moving,
//! deleting and making folders go through the tree's commands, each its own undo step with the
//! files it moves; organizing waits for the test to stop, while viewing, listening and searching
//! do not. When the last click was in the Library, Delete, F2 and Ctrl+A act on its items.
mod actions;
mod dialogs;
mod model;

pub(crate) use model::{Key, ScriptFacts};

use super::*;
use crate::icons::{self, Icon};
use crate::theme;
use egui::{Align, RichText, UiBuilder};
use model::{Item, Kind, Library, Sources, State};
use oxy_core::{
    command::{self, Command},
    edit_history::FileOp,
};
use std::collections::{BTreeMap, BTreeSet};

/// Height of a row of the list.
const ROW_HEIGHT: f32 = 24.;
/// Height of a row of the PASTAS column.
const FOLDER_ROW: f32 = 22.;
/// Width of the PASTAS column.
const FOLDER_COLUMN: f32 = 200.;
/// Narrower than this, the dock shows the folders as a breadcrumb.
const NARROW: f32 = 560.;
/// A row whose file is missing, unreadable or at an invalid path: its fill and texts.
const WARN_FILL: Color32 = Color32::from_rgb(58, 51, 34);
const WARN_NAME: Color32 = Color32::from_rgb(240, 224, 184);
const WARN_TYPE: Color32 = Color32::from_rgb(201, 185, 143);
/// The count of the open folder.
const OPEN_COUNT: Color32 = Color32::from_rgb(185, 211, 243);
/// Why organizing waits: the test, a project never saved, the old layout.
pub(crate) const PLAYING: &str = "Pare o teste para organizar os arquivos do projeto.";
const UNSAVED: &str = "Salve o projeto para organizar pastas.";
const OLD_LAYOUT: &str =
    "Salve o projeto para organizar em pastas; a OXY renomeia os arquivos e guarda um backup.";
/// Why what edits the scene waits.
const EDIT_DURING_TEST: &str = "Pare o teste para editar a cena.";
/// Why the project's root is neither renamed, moved nor deleted.
const ROOT: &str = "A raiz do projeto não muda de nome nem sai do lugar.";
/// Why an item whose file is missing is not renamed.
const LOCATE_FIRST: &str = "O arquivo não está na pasta do projeto; localize-o antes.";
/// What "Localizar arquivo…" says of a missing model, which no file picked becomes.
const MODEL_BY_ID: &str = "Um modelo é achado pelo ID que tem dentro do arquivo: com o arquivo de \
                           volta na pasta do projeto, em qualquer pasta, a OXY o acha sozinha ao \
                           abrir o projeto ou com F5.";

/// Where the Library looks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// Every item of the project, flat.
    All,
    /// A folder of the tree ("" is the project's root).
    Folder(String),
}

impl Default for Place {
    fn default() -> Self {
        Self::Folder(String::new())
    }
}

/// What a name typed in the Library renames or makes.
#[derive(Clone, Debug, PartialEq)]
enum Target {
    /// An item, with its name as it was.
    Item(Key, Kind, String),
    Folder(String),
    /// A new folder in that one.
    NewFolder(String),
}

/// A name being typed in the Library.
#[derive(Clone, Debug)]
struct Rename {
    target: Target,
    text: String,
    focus: bool,
    /// Why the name is refused, said under it.
    error: Option<String>,
    /// Typed in the list rather than in the PASTAS column.
    in_list: bool,
}

/// The Library's state in the editor session.
#[derive(Default)]
pub(crate) struct LibraryUi {
    pub(crate) place: Place,
    /// The chip on: items of one kind only.
    pub(crate) filter: Option<Kind>,
    pub(crate) search: String,
    pub(crate) selection: Vec<Key>,
    anchor: Option<Key>,
    /// The last click was in the Library: Delete, F2 and Ctrl+A act on it.
    pub(crate) focus: bool,
    /// The folders of the column shown open.
    expanded: BTreeSet<String>,
    rename: Option<Rename>,
    move_to: Option<dialogs::MoveDialog>,
    delete_folder: Option<dialogs::DeleteFolder>,
    delete_items: Option<Vec<Key>>,
    /// What opening told of each asset with a problem.
    pub(crate) report: BTreeMap<Id, persistence::AssetState>,
    /// The new files a walk found that may be each missing asset.
    pub(crate) near: BTreeMap<Id, Vec<String>>,
    /// What was found new in the folder since the project opened.
    pub(crate) new: BTreeSet<Key>,
    /// Where the Library's body was drawn: a click there gives it the focus.
    rect: Option<Rect>,
    /// The row to scroll to.
    reveal: Option<Key>,
    /// The PASTAS column shows (a dock wide enough, a project in folders).
    column: bool,
    /// The folder selected last was picked in the list rather than in the column.
    picked_in_list: bool,
}

impl LibraryUi {
    /// A dialog of the Library is open.
    pub(crate) fn dialog_open(&self) -> bool {
        self.move_to.is_some() || self.delete_folder.is_some() || self.delete_items.is_some()
    }
}

/// A batch's message for a person: without the batch's place ("Comando 0 (move, o 1º do
/// lote): ") nor "Nada foi alterado.".
fn plain(message: &str) -> String {
    let message = message
        .strip_prefix("Comando ")
        .and_then(|rest| rest.split_once("): "))
        .map_or(message, |(_, rest)| rest);
    message
        .trim_end()
        .trim_end_matches("Nada foi alterado.")
        .trim_end()
        .trim_end_matches('.')
        .to_owned()
}

/// How the menus and the dialogs name an item: "Bipe (som)".
fn labelled(item: &Item) -> String {
    format!("{} ({})", item.name, item.kind.label().to_lowercase())
}

/// The item's icon, or "{ }" for a script.
fn paint_icon(ui: &egui::Ui, rect: Rect, kind: Kind, color: Color32) {
    let icon = match kind {
        Kind::Image => Icon::Sprite,
        Kind::Sound => Icon::Sound,
        Kind::Model => Icon::Cube,
        Kind::Script => {
            paint_braces(ui, rect, color);
            return;
        }
    };
    icons::paint(
        ui,
        Rect::from_center_size(rect.center(), Vec2::splat(14.)),
        icon,
        color,
    );
}

/// The "{ }" of a script, drawn as lines: two braces around the center of `rect`.
fn paint_braces(ui: &egui::Ui, rect: Rect, color: Color32) {
    let c = rect.center();
    let stroke = egui::Stroke::new(1.3, color);
    for side in [-1., 1.] {
        let column = c.x + side * 3.6;
        let end = column - side * 1.8;
        let tip = column + side * 1.6;
        let points = vec![
            egui::pos2(end, c.y - 5.5),
            egui::pos2(column, c.y - 4.6),
            egui::pos2(column, c.y - 1.2),
            egui::pos2(tip, c.y),
            egui::pos2(column, c.y + 1.2),
            egui::pos2(column, c.y + 4.6),
            egui::pos2(end, c.y + 5.5),
        ];
        ui.painter().add(egui::Shape::line(points, stroke));
    }
}

impl Editor {
    /// The Library of the open project, as the last walk of its folder and the scripts' check
    /// found it.
    pub(crate) fn library_model(&self) -> Library {
        let facts = self.script_status.library_facts();
        let images = &self.state.images;
        let status = &self.script_status;
        let required = |path: &str| -> Vec<String> {
            status
                .required_by(path)
                .into_iter()
                .map(str::to_owned)
                .collect()
        };
        Library::new(&Sources {
            project: &self.state.project,
            tree: self.tree_scan.as_ref(),
            scripts: &facts,
            missing_scripts: self.script_status.missing_paths(),
            required_by: &required,
            report: &self.library.report,
            new: &self.library.new,
            in_memory: &|id| images.is_dirty(id),
        })
    }

    /// How many items the Library has, for its tab.
    pub(crate) fn library_count(&self) -> usize {
        self.state.project.assets.len() + self.script_status.library_len()
    }

    /// Why the project's files cannot be organized now (folders made, renamed, moved or
    /// deleted; files moved), if so.
    fn organize_refusal(&self) -> Option<&'static str> {
        if self.playing() {
            Some(PLAYING)
        } else if self.path.is_none() {
            Some(UNSAVED)
        } else if self.state.project.layout == Layout::Legacy {
            Some(OLD_LAYOUT)
        } else {
            None
        }
    }

    /// Why an item of `kind` cannot be renamed or deleted now, if so: an asset's name changes
    /// in the old layout too, a script is a file.
    fn item_refusal(&self, kind: Kind) -> Option<&'static str> {
        if self.playing() {
            Some(PLAYING)
        } else if kind == Kind::Script {
            self.organize_refusal()
        } else {
            None
        }
    }

    /// Why `item` cannot be renamed now, if so: as [`Self::item_refusal`] says, and in a project
    /// in folders while its file is missing.
    fn rename_refusal(&self, item: &Item) -> Option<&'static str> {
        self.item_refusal(item.kind).or_else(|| {
            (self.state.project.layout == Layout::Tree && item.state == State::Missing)
                .then_some(LOCATE_FIRST)
        })
    }

    /// Whether Delete, F2 and Ctrl+A go to the Library: the last click was in it and it shows.
    pub(crate) fn library_focused(&self) -> bool {
        self.library.focus
            && self.dock_open()
            && self.dock.tab == DockTab::Library
            && !self.animating()
    }

    /// Follows where the pointer was pressed: in the Library, it takes the keys; elsewhere (a
    /// floating window too), they go back to the scene. A click in a menu, a popup or a dialog
    /// changes nothing; one that closes a menu by landing on a panel counts where it lands.
    pub(crate) fn follow_library_focus(&mut self, ctx: &egui::Context) {
        let press = ctx.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos, pressed: true, ..
                } => Some(*pos),
                _ => None,
            })
        });
        // Drawn again this frame when the Library shows.
        let rect = self.library.rect.take();
        let Some(pos) = press else {
            return;
        };
        // Panels have no layer of their own; a dialog open answers for the whole window.
        self.library.focus = match ctx.layer_id_at(pos).map(|layer| layer.order) {
            None | Some(egui::Order::Background) => rect.is_some_and(|rect| rect.contains(pos)),
            Some(egui::Order::Middle) => false,
            Some(_) => return,
        };
    }

    /// The folder new things go in: the open one, the root when the Library shows everything,
    /// in the old layout and when the last walk no longer found the open folder.
    pub(crate) fn open_folder(&self) -> String {
        let walked = |folder: &str| {
            self.tree_scan
                .as_ref()
                .is_some_and(|tree| tree.folders.iter().any(|f| f == folder))
        };
        match &self.library.place {
            Place::Folder(folder)
                if self.state.project.layout == Layout::Tree && walked(folder) =>
            {
                folder.clone()
            }
            _ => String::new(),
        }
    }

    /// The folder `folder` as buttons and the status bar name it: its last name, or the
    /// project's for the root.
    fn folder_label(&self, folder: &str) -> String {
        if folder.is_empty() {
            self.state.project.name.clone()
        } else {
            model::file_name(folder).to_owned()
        }
    }

    /// What the status bar says the Library shows.
    pub(crate) fn library_status(&self) -> Option<String> {
        if self.path.is_none() || self.state.project.layout != Layout::Tree {
            return None;
        }
        Some(match &self.library.place {
            Place::All => "Biblioteca: Todos os recursos".to_owned(),
            Place::Folder(folder) => format!("Biblioteca: {}", self.folder_label(folder)),
        })
    }

    /// Opens `place` in the list, the column's way to it opened; a name being typed goes.
    fn open_place(&mut self, place: Place) {
        if let Place::Folder(folder) = &place {
            self.expand_to(folder);
        }
        if self.library.place != place {
            self.library.rename = None;
        }
        self.library.place = place;
    }

    /// Opens the folder `folder` in the PASTAS column, and every folder on its way.
    fn expand_to(&mut self, folder: &str) {
        let mut at = folder;
        while !at.is_empty() {
            self.library.expanded.insert(at.to_owned());
            at = model::parent(at);
        }
    }

    /// The items the list shows now, in its order.
    fn visible<'a>(&self, lib: &'a Library) -> Vec<&'a Item> {
        let folder = match &self.library.place {
            Place::Folder(folder) if !lib.folders.is_empty() => Some(folder.as_str()),
            _ => None,
        };
        lib.shown(folder, &self.library.search, self.library.filter)
    }

    /// The header of the Biblioteca tab, right of the tabs: Importar para <pasta>… and the
    /// search over the whole project.
    pub(super) fn library_header(&mut self, ui: &mut egui::Ui) {
        let folder = self.open_folder();
        let label = if self.state.project.layout == Layout::Tree && self.path.is_some() {
            format!("Importar para {}…", self.folder_label(&folder))
        } else {
            "Importar…".to_owned()
        };
        let import = ui
            .add_enabled(!self.playing(), egui::Button::new(label))
            .on_hover_text("Importa imagens PNG e sons WAV para a pasta aberta.")
            .on_disabled_hover_text(PLAYING);
        if import.clicked() {
            self.import_into(&folder);
        }
        // Narrow docks drop the search instead of overlapping the tabs.
        let width = (ui.available_width() - 12.).min(200.);
        if width >= 80. {
            ui.add(
                egui::TextEdit::singleline(&mut self.library.search)
                    .hint_text("Buscar no projeto…")
                    .desired_width(width),
            );
        }
    }

    /// The Biblioteca tab's body.
    pub(super) fn library_body(&mut self, ui: &mut egui::Ui) {
        let lib = self.library_model();
        let area = ui.available_rect_before_wrap();
        self.library.rect = Some(area);
        self.settle_library(&lib);
        let blank = ui.interact(area, ui.id().with("library_blank"), Sense::click());
        if blank.clicked() {
            self.library.selection.clear();
        }
        let mut top = area.top();
        if self.state.project.layout == Layout::Legacy && self.path.is_some() {
            let mut banner = ui.new_child(
                UiBuilder::new()
                    .max_rect(area)
                    .layout(egui::Layout::top_down(Align::Min)),
            );
            crate::widgets::banner(&mut banner, OLD_LAYOUT, |_| {});
            top = banner.min_rect().bottom() + 6.;
        }
        let area = Rect::from_min_max(egui::pos2(area.left(), top), area.max);
        let tree = !lib.folders.is_empty() || self.path.is_none();
        let column = tree && area.width() >= NARROW;
        self.library.column = column;
        let list_area = if column {
            let rect = Rect::from_min_size(area.min, Vec2::new(FOLDER_COLUMN, area.height()));
            let mut column_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::top_down(Align::Min)),
            );
            self.folder_column(&mut column_ui, &lib);
            ui.painter().vline(
                rect.right() + 4.,
                rect.y_range(),
                egui::Stroke::new(1., theme::CARD_BORDER),
            );
            Rect::from_min_max(egui::pos2(rect.right() + 16., area.top()), area.max)
        } else {
            area
        };
        let mut list_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(list_area)
                .layout(egui::Layout::top_down(Align::Min)),
        );
        self.library_list(&mut list_ui, &lib, column);
        ui.advance_cursor_after_rect(area);
    }

    /// Keeps the Library's state in step with what it lists: a located asset opened and
    /// selected, and the selection and the open folder without what left.
    fn settle_library(&mut self, lib: &Library) {
        if let Some(id) = self.asset_ui.locate.take() {
            let key = Key::Asset(id);
            if let Some(item) = lib.item(&key) {
                let folder = item.folder.clone();
                self.open_place(Place::Folder(folder));
                self.library.filter = None;
                self.library.search.clear();
                self.library.selection = vec![key.clone()];
                self.library.anchor = Some(key.clone());
                self.library.reveal = Some(key);
            }
        }
        if !lib.folders.is_empty()
            && let Place::Folder(folder) = &self.library.place
            && lib.folder(folder).is_none()
        {
            self.library.place = Place::default();
        }
        let tree = !lib.folders.is_empty();
        self.library.selection.retain(|key| match key {
            Key::Folder(folder) => tree && lib.folder(folder).is_some(),
            _ => lib.item(key).is_some(),
        });
    }

    /// The PASTAS column: the project's root, "Todos os recursos" and the folders, open as the
    /// person left them.
    fn folder_column(&mut self, ui: &mut egui::Ui, lib: &Library) {
        let refusal = self.organize_refusal();
        ui.horizontal(|ui| {
            ui.set_height(22.);
            ui.add_space(6.);
            ui.label(
                RichText::new("PASTAS")
                    .font(theme::medium(11.))
                    .color(theme::TEXT_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let add = ui
                    .add_enabled_ui(refusal.is_none(), |ui| {
                        icons::small(ui, Icon::Plus, "Nova pasta", 20.)
                    })
                    .inner;
                let add = match refusal {
                    Some(why) => add.on_disabled_hover_text(why),
                    None => add,
                };
                if add.clicked() {
                    let parent = self.open_folder();
                    self.begin_new_folder(&parent);
                }
            });
        });
        ui.add_space(2.);
        egui::ScrollArea::vertical()
            .id_salt("library_folders")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.;
                if self.path.is_none() {
                    let name = RichText::new(&self.state.project.name).color(theme::TEXT);
                    self.folder_line(ui, 0, Some(true), name, None, true, "");
                    ui.add_space(6.);
                    crate::widgets::hint(ui, UNSAVED);
                    return;
                }
                let total = lib.items.len();
                let all = RichText::new("Todos os recursos")
                    .italics()
                    .color(theme::TEXT_MUTED);
                let selected = self.library.place == Place::All;
                let (row, _) =
                    self.folder_line(ui, 0, None, all, Some(total), selected, "Todos os recursos");
                if row.clicked() {
                    self.library.place = Place::All;
                    self.library.selection.clear();
                }
                for folder in &lib.folders {
                    let shown = {
                        let mut at = model::parent(&folder.path);
                        let mut open = true;
                        while !at.is_empty() {
                            open &= self.library.expanded.contains(at);
                            at = model::parent(at);
                        }
                        open
                    };
                    if !folder.path.is_empty() && !shown {
                        continue;
                    }
                    self.folder_row(ui, lib, folder);
                    if let Some(Rename {
                        target: Target::NewFolder(parent),
                        ..
                    }) = &self.library.rename
                        && *parent == folder.path
                    {
                        self.new_folder_row(ui, folder.depth + 1);
                    }
                }
            });
    }

    /// One line of the PASTAS column: its chevron (none for a line without one), its name and
    /// its count at the right; the line's response and whether the chevron was clicked.
    #[allow(clippy::too_many_arguments)]
    fn folder_line(
        &mut self,
        ui: &mut egui::Ui,
        depth: usize,
        chevron: Option<bool>,
        name: RichText,
        count: Option<usize>,
        selected: bool,
        qa: &str,
    ) -> (egui::Response, bool) {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), FOLDER_ROW), Sense::click());
        if !qa.is_empty() {
            icons::register_qa(ui, qa, rect);
        }
        let fill = if selected {
            theme::ACCENT_SOFT
        } else if response.hovered() {
            theme::BG_WINDOW
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 4., fill);
        let left = rect.left() + 6. + depth as f32 * 14.;
        let mut toggled = false;
        if let Some(open) = chevron {
            let at =
                Rect::from_center_size(egui::pos2(left + 7., rect.center().y), Vec2::splat(14.));
            let arrow = ui.interact(at, response.id.with("chevron"), Sense::click());
            icons::paint(
                ui,
                at.shrink(2.),
                if open {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                },
                theme::TEXT_MUTED,
            );
            toggled = arrow.clicked();
        }
        let count_width = if let Some(count) = count {
            let galley = ui.painter().layout_no_wrap(
                count.to_string(),
                egui::FontId::proportional(12.),
                if selected {
                    OPEN_COUNT
                } else {
                    theme::TEXT_MUTED
                },
            );
            let width = galley.size().x;
            ui.painter().galley(
                egui::pos2(
                    rect.right() - 6. - width,
                    rect.center().y - galley.size().y / 2.,
                ),
                galley,
                Color32::PLACEHOLDER,
            );
            width + 10.
        } else {
            0.
        };
        let name_rect = Rect::from_min_max(
            egui::pos2(left + 18., rect.top()),
            egui::pos2(rect.right() - 6. - count_width, rect.bottom()),
        );
        let mut name_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(name_rect)
                .layout(egui::Layout::left_to_right(Align::Center)),
        );
        name_ui.add(egui::Label::new(name).truncate().selectable(false));
        (response, toggled)
    }

    /// A folder of the PASTAS column: clicking opens it, its chevron shows what it holds, the
    /// right button opens its menu, F2 renames it in place.
    fn folder_row(&mut self, ui: &mut egui::Ui, lib: &Library, folder: &model::Folder) {
        let renaming = matches!(&self.library.rename, Some(Rename { target: Target::Folder(path), in_list: false, .. }) if *path == folder.path);
        if renaming {
            ui.horizontal(|ui| {
                ui.add_space(6. + folder.depth as f32 * 14. + 18.);
                let width = ui.available_width() - 6.;
                self.rename_field(ui, width);
            });
            self.rename_error(ui, 6. + folder.depth as f32 * 14. + 18.);
            return;
        }
        let open = self.library.place == Place::Folder(folder.path.clone());
        let expanded = folder.path.is_empty() || self.library.expanded.contains(&folder.path);
        let color = if open || folder.path.is_empty() {
            theme::TEXT
        } else {
            Color32::from_rgb(180, 180, 182)
        };
        let (row, toggled) = self.folder_line(
            ui,
            folder.depth,
            (folder.children && !folder.path.is_empty())
                .then_some(expanded)
                .or(folder.path.is_empty().then_some(true)),
            RichText::new(&folder.name).color(color),
            Some(folder.direct),
            open,
            &format!("Pasta: {}", folder.name),
        );
        let row = row.on_hover_text(format!(
            "{} nesta pasta · {} com as subpastas",
            model::count(folder.direct, "item", "itens"),
            folder.total
        ));
        if toggled && !folder.path.is_empty() {
            if expanded {
                self.library.expanded.remove(&folder.path);
            } else {
                self.library.expanded.insert(folder.path.clone());
            }
        } else if row.clicked() {
            self.open_place(Place::Folder(folder.path.clone()));
            self.library.selection = if folder.path.is_empty() {
                Vec::new()
            } else {
                vec![Key::Folder(folder.path.clone())]
            };
            self.library.picked_in_list = false;
        }
        row.context_menu(|ui| self.folder_menu(ui, lib, &folder.path, false));
    }

    /// The line where the name of a new folder is typed, at `depth`.
    fn new_folder_row(&mut self, ui: &mut egui::Ui, depth: usize) {
        let indent = 6. + depth as f32 * 14. + 18.;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            let width = ui.available_width() - 6.;
            self.rename_field(ui, width);
        });
        self.rename_error(ui, indent);
    }

    /// The menu of the folder `folder` ("" is the root).
    fn folder_menu(&mut self, ui: &mut egui::Ui, lib: &Library, folder: &str, in_list: bool) {
        ui.set_min_width(210.);
        let refusal = self.organize_refusal();
        let fixed = if folder.is_empty() {
            Some(ROOT)
        } else {
            refusal
        };
        let item = |ui: &mut egui::Ui, label: &str, shortcut: &str, why: Option<&str>| {
            let response = ui.add_enabled(
                why.is_none(),
                egui::Button::new(label).shortcut_text(shortcut),
            );
            match why {
                Some(why) => response.on_disabled_hover_text(why).clicked(),
                None => response.clicked(),
            }
        };
        if item(ui, "Nova subpasta", "", refusal) {
            self.begin_new_folder(folder);
            ui.close();
        }
        if item(ui, "Renomear", "F2", fixed) {
            self.begin_folder_rename(folder, in_list);
            ui.close();
        }
        if item(ui, "Mover para…", "", fixed) {
            self.open_move(vec![Key::Folder(folder.to_owned())], lib);
            ui.close();
        }
        let import = format!("Importar para {}…", self.folder_label(folder));
        if item(ui, &import, "", self.playing().then_some(PLAYING)) {
            ui.close();
            self.import_into(folder);
        }
        if item(
            ui,
            "Mostrar no Explorer",
            "",
            self.path.is_none().then_some(UNSAVED),
        ) {
            self.show_in_explorer(folder, true);
            ui.close();
        }
        ui.separator();
        let delete = ui
            .add_enabled(
                fixed.is_none(),
                egui::Button::new(RichText::new("Excluir pasta…").color(theme::ERROR)),
            )
            .on_disabled_hover_text(fixed.unwrap_or_default());
        if delete.clicked() {
            self.library_delete(vec![Key::Folder(folder.to_owned())]);
            ui.close();
        }
    }

    /// The list side: where it is (the breadcrumb, or a menu of the folders in a narrow dock),
    /// the chips, then the rows.
    fn library_list(&mut self, ui: &mut egui::Ui, lib: &Library, column: bool) {
        let shown = self.visible(lib);
        let searching = !self.library.search.trim().is_empty();
        let tree = !lib.folders.is_empty();
        let folder = match &self.library.place {
            Place::Folder(folder) if tree && !searching => Some(folder.clone()),
            _ => None,
        };
        // The chips count what the place shows, whatever kind is on.
        let counted = match &folder {
            Some(folder) => lib.shown(Some(folder), "", None),
            None => lib.shown(None, &self.library.search, None),
        };
        ui.horizontal_wrapped(|ui| {
            ui.set_min_height(24.);
            ui.spacing_mut().item_spacing.x = 4.;
            if tree && !column {
                self.folders_menu(ui, lib);
            }
            if searching {
                ui.label(
                    RichText::new(format!(
                        "Busca no projeto: “{}”",
                        self.library.search.trim()
                    ))
                    .size(12.5)
                    .color(theme::TEXT_MUTED),
                );
            } else if self.library.place == Place::All || !tree {
                ui.label(
                    RichText::new("Todos os recursos")
                        .size(12.5)
                        .color(theme::TEXT_MUTED),
                );
            } else if let Some(folder) = &folder {
                self.breadcrumb(ui, folder);
            }
            ui.add_space(8.);
            for (kind, count) in Library::chips(&counted) {
                let label = format!("{} {count}", kind.map_or("Tudo", Kind::chip));
                if chip(ui, &label, self.library.filter == kind).clicked() {
                    self.library.filter = kind;
                }
            }
        });
        ui.add_space(4.);
        // Without the PASTAS column, the list says why there are no folders.
        if self.path.is_none() && !column {
            crate::widgets::hint(ui, UNSAVED);
            ui.add_space(4.);
        }
        let flat = folder.is_none() && tree;
        let width = ui.available_width();
        let widths = [
            (width * 0.26).clamp(100., 220.),
            if flat {
                (width * 0.2).clamp(80., 200.)
            } else {
                0.
            },
        ];
        let order: Vec<Key> = shown.iter().map(|item| item.key.clone()).collect();
        let subfolders = match &folder {
            Some(folder) if self.library.filter.is_none() => lib.subfolders(folder),
            _ => Vec::new(),
        };
        egui::ScrollArea::vertical()
            .id_salt("library_items")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.;
                if !column
                    && let Some(Rename {
                        target: Target::NewFolder(parent),
                        ..
                    }) = &self.library.rename
                    && folder.as_deref() == Some(parent.as_str())
                {
                    self.new_folder_row(ui, 0);
                }
                for sub in &subfolders {
                    self.subfolder_row(ui, lib, sub, widths[0]);
                }
                for item in &shown {
                    self.item_row(ui, lib, item, &order, flat, widths);
                }
                if shown.is_empty() && subfolders.is_empty() {
                    ui.add_space(10.);
                    let empty = if searching {
                        "Nada no projeto com esse nome."
                    } else if self.library.filter.is_some() && folder.is_some() {
                        "Nenhum item deste tipo nesta pasta."
                    } else if folder.is_some() {
                        "Pasta vazia"
                    } else if self.state.project.assets.is_empty() {
                        "Nenhum recurso. Importe PNG ou WAV, ou salve uma hierarquia como modelo."
                    } else {
                        "Nenhum item deste tipo."
                    };
                    ui.horizontal(|ui| {
                        ui.add_space(8.);
                        crate::widgets::hint(ui, empty);
                    });
                }
                if let Some(folder) = &folder
                    && let Some(others) = lib.others.get(folder)
                {
                    self.others_row(ui, folder, others);
                }
            });
    }

    /// The breadcrumb of `folder`, each part opening its folder.
    fn breadcrumb(&mut self, ui: &mut egui::Ui, folder: &str) {
        let mut parts = vec![(String::new(), self.state.project.name.clone())];
        let mut at = String::new();
        for name in folder.split('/').filter(|n| !n.is_empty()) {
            at = model::join(&at, name);
            parts.push((at.clone(), name.to_owned()));
        }
        let last = parts.len() - 1;
        for (index, (path, name)) in parts.into_iter().enumerate() {
            if index > 0 {
                ui.label(RichText::new("›").size(12.5).color(theme::TEXT_MUTED));
            }
            let color = if index == last {
                theme::TEXT
            } else {
                theme::TEXT_MUTED
            };
            let part = ui.add(
                egui::Label::new(RichText::new(name).size(12.5).color(color))
                    .sense(Sense::click())
                    .selectable(false),
            );
            if part.clicked() && index != last {
                self.open_place(Place::Folder(path));
                self.library.selection.clear();
            }
        }
    }

    /// The folders as a menu, for a dock too narrow for the PASTAS column.
    fn folders_menu(&mut self, ui: &mut egui::Ui, lib: &Library) {
        icons::menu_button(
            ui,
            Icon::Folder,
            "Pastas",
            "Pastas do projeto",
            false,
            |ui| {
                if ui
                    .selectable_label(self.library.place == Place::All, "Todos os recursos")
                    .clicked()
                {
                    self.library.place = Place::All;
                    ui.close();
                }
                for folder in &lib.folders {
                    let text = format!("{}{}", "   ".repeat(folder.depth), folder.name);
                    let open = self.library.place == Place::Folder(folder.path.clone());
                    if ui.selectable_label(open, text).clicked() {
                        self.open_place(Place::Folder(folder.path.clone()));
                        ui.close();
                    }
                }
                if self.organize_refusal().is_none() {
                    ui.separator();
                    if ui.button("Nova pasta").clicked() {
                        let parent = self.open_folder();
                        self.begin_new_folder(&parent);
                        ui.close();
                    }
                }
            },
        );
    }

    /// A subfolder in the list: double click or its arrow opens it, the right button its menu.
    fn subfolder_row(
        &mut self,
        ui: &mut egui::Ui,
        lib: &Library,
        folder: &model::Folder,
        name_width: f32,
    ) {
        let key = Key::Folder(folder.path.clone());
        let selected = self.library.selection.contains(&key);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
        icons::register_qa(ui, &format!("Subpasta: {}", folder.name), rect);
        let fill = if selected {
            theme::ACCENT_SOFT
        } else if response.hovered() {
            theme::BG_WINDOW
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 4., fill);
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(8., 0.)))
                .layout(egui::Layout::left_to_right(Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 10.;
        let (arrow, _) = row.allocate_exact_size(Vec2::new(20., ROW_HEIGHT), Sense::hover());
        let opener = row.interact(arrow, response.id.with("open"), Sense::click());
        icons::paint(
            &row,
            Rect::from_center_size(arrow.center(), Vec2::splat(12.)),
            Icon::ChevronRight,
            theme::TEXT_MUTED,
        );
        let renaming = matches!(&self.library.rename, Some(Rename { target: Target::Folder(path), in_list, .. }) if *path == folder.path && (*in_list || !self.library.column));
        if renaming {
            self.rename_field(&mut row, name_width);
        } else {
            row.allocate_ui(Vec2::new(name_width, ROW_HEIGHT), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&folder.name).color(Color32::from_rgb(180, 180, 182)),
                    )
                    .truncate()
                    .selectable(false),
                );
            });
            row.label(
                RichText::new(format!(
                    "pasta · {}",
                    model::count(folder.total, "item", "itens")
                ))
                .size(12.)
                .color(theme::TEXT_MUTED),
            );
        }
        if renaming {
            self.rename_error(ui, 38.);
        }
        if opener.clicked() || response.double_clicked() {
            self.open_place(Place::Folder(folder.path.clone()));
            self.library.selection.clear();
        } else if response.clicked() || response.secondary_clicked() {
            self.library.selection = vec![key.clone()];
            self.library.anchor = None;
            self.library.picked_in_list = true;
        }
        response.context_menu(|ui| self.folder_menu(ui, lib, &folder.path, true));
    }

    /// An item of the list: its icon, name (its path in the tooltip), [folder,] type, use or
    /// state, and the action of its kind; clicks select, the right button opens its menu.
    fn item_row(
        &mut self,
        ui: &mut egui::Ui,
        lib: &Library,
        item: &Item,
        order: &[Key],
        flat: bool,
        widths: [f32; 2],
    ) {
        let selected = self.library.selection.contains(&item.key);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
        icons::register_qa(ui, &format!("Recurso: {}", item.name), rect);
        let warns = item.state.warns();
        let fill = if selected {
            theme::ACCENT_SOFT
        } else if warns {
            WARN_FILL
        } else if response.hovered() {
            theme::BG_WINDOW
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 4., fill);
        if self.library.reveal.as_ref() == Some(&item.key) {
            response.scroll_to_me(Some(Align::Center));
            self.library.reveal = None;
        }
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(8., 0.)))
                .layout(egui::Layout::left_to_right(Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 10.;
        let failing = matches!(item.state, State::Error(_));
        let icon_color = if warns {
            theme::WARNING
        } else if failing {
            theme::ERROR
        } else if item.kind == Kind::Script {
            theme::ACCENT_BRIGHT
        } else {
            Color32::from_rgb(180, 180, 182)
        };
        let (icon, _) = row.allocate_exact_size(Vec2::new(20., ROW_HEIGHT), Sense::hover());
        paint_icon(&row, icon, item.kind, icon_color);
        let renaming = matches!(&self.library.rename, Some(Rename { target: Target::Item(key, ..), .. }) if *key == item.key);
        if renaming {
            self.rename_field(&mut row, widths[0]);
        } else {
            row.allocate_ui(Vec2::new(widths[0], ROW_HEIGHT), |ui| {
                let mut name =
                    RichText::new(&item.name).color(if warns { WARN_NAME } else { theme::TEXT });
                if item.kind == Kind::Script {
                    name = name.monospace();
                }
                ui.add(egui::Label::new(name).truncate().selectable(false))
                    .on_hover_text(if item.path.is_empty() {
                        "Guardado no arquivo do projeto"
                    } else {
                        &item.path
                    });
            });
        }
        if flat {
            row.allocate_ui(Vec2::new(widths[1], ROW_HEIGHT), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(model::trail(&self.state.project.name, &item.folder))
                            .size(12.)
                            .color(theme::TEXT_MUTED),
                    )
                    .truncate()
                    .selectable(false),
                );
            });
        }
        row.allocate_ui(Vec2::new(56., ROW_HEIGHT), |ui| {
            ui.label(RichText::new(item.kind.label()).size(12.).color(if warns {
                WARN_TYPE
            } else {
                theme::TEXT_MUTED
            }));
        });
        row.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 12.;
            self.item_actions(ui, lib, item);
            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                Vec2::new(width, ROW_HEIGHT),
                egui::Layout::left_to_right(Align::Center),
                |ui| self.item_usage(ui, item),
            );
        });
        if renaming {
            self.rename_error(ui, 38.);
        }
        let modifiers = ui.input(|i| i.modifiers);
        if response.clicked() {
            let (selection, anchor) = model::click(
                &self.library.selection,
                self.library.anchor.as_ref(),
                order,
                &item.key,
                modifiers.ctrl || modifiers.command,
                modifiers.shift,
            );
            self.library.selection = selection;
            self.library.anchor = Some(anchor);
        }
        if response.double_clicked() {
            match (&item.key, item.kind) {
                (Key::Script(path), _) if item.state != State::Missing => {
                    self.open_code(path, item.error_line.filter(|l| *l > 0).unwrap_or(1));
                }
                (Key::Asset(id), Kind::Sound) => self.hear_asset(id),
                _ => {}
            }
        }
        if response.secondary_clicked() && !selected {
            self.library.selection = vec![item.key.clone()];
            self.library.anchor = Some(item.key.clone());
        }
        response.context_menu(|ui| self.item_menu(ui, lib, item));
    }

    /// What the row of `item` says of it: how it is used, or its state when it is not fine;
    /// "novo" for a file found new.
    fn item_usage(&mut self, ui: &mut egui::Ui, item: &Item) {
        ui.spacing_mut().item_spacing.x = 0.;
        if item.new {
            let text = ui.painter().layout_no_wrap(
                "novo".into(),
                egui::FontId::proportional(10.5),
                theme::ACCENT_BRIGHT,
            );
            let (pill, _) =
                ui.allocate_exact_size(text.size() + Vec2::new(10., 4.), Sense::hover());
            ui.painter().rect_filled(pill, 7., theme::ACCENT_SOFT);
            ui.painter()
                .galley(pill.center() - text.size() / 2., text, Color32::PLACEHOLDER);
            ui.add_space(8.);
        }
        let muted = |text: &str| RichText::new(text).size(12.).color(theme::TEXT_MUTED);
        match (&item.state, model::state_line(item)) {
            (State::Error(why), _) if item.kind == Kind::Script => {
                let base = item.usage.strip_suffix(" · 1 erro").unwrap_or(&item.usage);
                ui.label(muted(&format!("{base} · ")));
                let line = item.error_line.unwrap_or(0);
                let place = if line > 0 {
                    format!("Linha {line}: {why}")
                } else {
                    why.clone()
                };
                ui.add(
                    egui::Label::new(RichText::new("1 erro").size(12.).color(theme::ERROR))
                        .truncate(),
                )
                .on_hover_text(place);
            }
            (State::Unsaved, Some(line)) => {
                ui.add(egui::Label::new(muted(&line).italics()).truncate());
            }
            (State::Error(_), Some(line)) => {
                ui.add(
                    egui::Label::new(RichText::new(&line).size(12.).color(theme::ERROR)).truncate(),
                )
                .on_hover_text(&line);
            }
            (_, Some(line)) => {
                ui.add(
                    egui::Label::new(RichText::new(&line).size(12.).color(theme::WARNING))
                        .truncate(),
                )
                .on_hover_text(&line);
            }
            (_, None) => {
                ui.add(egui::Label::new(muted(&item.usage)).truncate());
            }
        }
    }

    /// The action of the item's kind at the right of its row (added from the right): Ver
    /// código and Anexar for a script, Colocar na cena for a model, Aplicar for an image, Ouvir
    /// for a sound, and Localizar arquivo… for one whose file is missing.
    fn item_actions(&mut self, ui: &mut egui::Ui, lib: &Library, item: &Item) {
        let action = |ui: &mut egui::Ui, text: &str| {
            ui.add(egui::Button::new(RichText::new(text).size(12.).color(theme::TEXT)).frame(false))
        };
        // What edits the scene waits for the test, as the Hierarchy and the Properties do.
        let playing = self.playing();
        let edit = |ui: &mut egui::Ui, text: &str| {
            ui.add_enabled(
                !playing,
                egui::Button::new(RichText::new(text).size(12.).color(theme::TEXT)).frame(false),
            )
            .on_disabled_hover_text(EDIT_DURING_TEST)
        };
        if item.state == State::Missing {
            let locate = ui.add(
                egui::Button::new(
                    RichText::new("Localizar arquivo…")
                        .size(12.)
                        .underline()
                        .color(WARN_NAME),
                )
                .frame(false),
            );
            egui::Popup::from_toggle_button_response(&locate)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| self.locate_popover(ui, lib, item));
            return;
        }
        match (&item.key, item.kind) {
            (Key::Script(path), _) => {
                if edit(ui, "Anexar")
                    .on_hover_text("Anexa o script aos objetos selecionados.")
                    .clicked()
                {
                    self.attach_from_library(path);
                }
                if action(ui, "Ver código").clicked() {
                    self.open_code(path, item.error_line.filter(|l| *l > 0).unwrap_or(1));
                }
            }
            (Key::Asset(id), Kind::Model) => {
                if edit(ui, "Colocar na cena").clicked() {
                    self.place_model(id);
                }
            }
            (Key::Asset(id), Kind::Image) => {
                if edit(ui, "Aplicar")
                    .on_hover_text("Aplica na peça selecionada. Sem seleção, cria um sprite.")
                    .clicked()
                    && let Some(asset) = self.state.project.asset(id).cloned()
                {
                    self.apply_texture_asset(&asset);
                    self.library.focus = false;
                }
            }
            (Key::Asset(id), Kind::Sound) => {
                if action(ui, "Ouvir").clicked() {
                    self.hear_asset(id);
                }
            }
            _ => {}
        }
    }

    /// The menu of an item: its action, Renomear, Mover para…, Mostrar no Explorer and
    /// Excluir…; on the whole selection when the item is in it.
    fn item_menu(&mut self, ui: &mut egui::Ui, lib: &Library, item: &Item) {
        ui.set_min_width(210.);
        let selection: Vec<Key> = if self.library.selection.contains(&item.key) {
            self.library
                .selection
                .iter()
                .filter(|key| !matches!(key, Key::Folder(_)))
                .cloned()
                .collect()
        } else {
            vec![item.key.clone()]
        };
        let many = selection.len() > 1;
        let enabled = |ui: &mut egui::Ui, label: &str, shortcut: &str, why: Option<&str>| {
            let response = ui.add_enabled(
                why.is_none(),
                egui::Button::new(label).shortcut_text(shortcut),
            );
            match why {
                Some(why) => response.on_disabled_hover_text(why).clicked(),
                None => response.clicked(),
            }
        };
        if !many {
            match (&item.key, item.kind, &item.state) {
                // A model is found by the ID inside its file, never picked.
                (_, Kind::Model, State::Missing) => {}
                (_, _, State::Missing) => {
                    if enabled(
                        ui,
                        "Escolher o arquivo…",
                        "",
                        self.playing().then_some(PLAYING),
                    ) {
                        ui.close();
                        self.pick_relink(&item.key, item.kind);
                    }
                }
                (Key::Script(path), ..) => {
                    if ui.button("Ver código").clicked() {
                        self.open_code(path, item.error_line.filter(|l| *l > 0).unwrap_or(1));
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.playing(),
                            egui::Button::new("Anexar aos selecionados"),
                        )
                        .on_disabled_hover_text(EDIT_DURING_TEST)
                        .clicked()
                    {
                        self.attach_from_library(path);
                        ui.close();
                    }
                }
                (Key::Asset(id), Kind::Model, _) => {
                    if ui
                        .add_enabled(!self.playing(), egui::Button::new("Colocar na cena"))
                        .on_disabled_hover_text(EDIT_DURING_TEST)
                        .clicked()
                    {
                        self.place_model(id);
                        ui.close();
                    }
                }
                (Key::Asset(id), Kind::Image, _) => {
                    if ui
                        .add_enabled(!self.playing(), egui::Button::new("Aplicar"))
                        .on_disabled_hover_text(EDIT_DURING_TEST)
                        .clicked()
                    {
                        if let Some(asset) = self.state.project.asset(id).cloned() {
                            self.apply_texture_asset(&asset);
                            self.library.focus = false;
                        }
                        ui.close();
                    }
                }
                (Key::Asset(id), Kind::Sound, _) => {
                    if ui.button("Ouvir").clicked() {
                        self.hear_asset(id);
                        ui.close();
                    }
                }
                _ => {}
            }
            ui.separator();
            if enabled(ui, "Renomear", "F2", self.rename_refusal(item)) {
                self.begin_item_rename(item);
                ui.close();
            }
        } else {
            ui.label(
                RichText::new(format!("{} selecionados", selection.len()))
                    .size(12.)
                    .color(theme::TEXT_MUTED),
            );
            ui.separator();
        }
        let label = if many {
            format!("Mover {} itens para…", selection.len())
        } else {
            "Mover para…".to_owned()
        };
        if enabled(ui, &label, "", self.organize_refusal()) {
            self.open_move(selection.clone(), lib);
            ui.close();
        }
        let file = !matches!(item.state, State::Missing | State::Unsaved) && !item.path.is_empty();
        let why = if self.path.is_none() {
            Some(UNSAVED)
        } else if !file {
            Some("O arquivo ainda não está na pasta do projeto.")
        } else {
            None
        };
        if !many && enabled(ui, "Mostrar no Explorer", "", why) {
            self.show_in_explorer(&item.path, false);
            ui.close();
        }
        ui.separator();
        let label = if many {
            format!("Excluir {} itens…", selection.len())
        } else {
            "Excluir…".to_owned()
        };
        let why = if selection.iter().any(|key| matches!(key, Key::Script(_))) {
            self.item_refusal(Kind::Script)
        } else {
            self.playing().then_some(PLAYING)
        };
        let delete = ui
            .add_enabled(
                why.is_none(),
                egui::Button::new(RichText::new(label).color(theme::ERROR)),
            )
            .on_disabled_hover_text(why.unwrap_or_default());
        if delete.clicked() {
            self.library_delete(selection);
            ui.close();
        }
    }

    /// The files that may be the missing item's, each with Religar, and Escolher outro
    /// arquivo….
    fn locate_popover(&mut self, ui: &mut egui::Ui, lib: &Library, item: &Item) {
        ui.set_width(300.);
        ui.label(
            RichText::new(format!("Localizar arquivo de “{}”", item.name))
                .font(theme::medium(13.))
                .color(theme::TEXT),
        );
        ui.add_space(4.);
        if item.kind == Kind::Model {
            crate::widgets::hint(ui, MODEL_BY_ID);
            return;
        }
        let near: Vec<String> = match &item.key {
            Key::Asset(id) => self.library.near.get(id).cloned().unwrap_or_default(),
            _ => Vec::new(),
        };
        let project = &self.state.project;
        let used = |key: &Key| match key {
            Key::Asset(id) => !oxy_core::editing::asset_uses(project, id).is_empty(),
            _ => false,
        };
        let suggestions = lib.suggestions(item, &near, self.tree_scan.as_ref(), &used);
        let refusal = self.playing().then_some(PLAYING);
        if suggestions.is_empty() {
            crate::widgets::hint(
                ui,
                &format!(
                    "A OXY não achou na pasta do projeto um arquivo que pareça ser {}.",
                    item.kind.article()
                ),
            );
        } else {
            crate::widgets::hint(
                ui,
                if suggestions.len() == 1 {
                    "Achei na pasta do projeto um arquivo do mesmo tipo que a OXY ainda não \
                     conhece:"
                } else {
                    "Achei na pasta do projeto arquivos do mesmo tipo que a OXY ainda não \
                     conhece:"
                },
            );
            for path in &suggestions {
                egui::Frame::new()
                    .fill(theme::BG_PANEL)
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(8, 5))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let relink = ui
                                .with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                    let button = ui
                                        .add_enabled(
                                            refusal.is_none(),
                                            egui::Button::new(
                                                RichText::new("Religar").color(Color32::WHITE),
                                            )
                                            .fill(theme::ACCENT),
                                        )
                                        .on_disabled_hover_text(PLAYING);
                                    ui.add(
                                        egui::Label::new(RichText::new(path).monospace().size(12.))
                                            .truncate(),
                                    );
                                    button
                                })
                                .inner;
                            if relink.clicked() {
                                self.relink(&item.key, path);
                                ui.close();
                            }
                        });
                    });
            }
        }
        ui.add_space(4.);
        let other = ui
            .add_enabled(
                refusal.is_none(),
                egui::Button::new(
                    RichText::new("Escolher outro arquivo…")
                        .size(12.)
                        .color(theme::ACCENT_BRIGHT),
                )
                .frame(false),
            )
            .on_disabled_hover_text(PLAYING);
        if other.clicked() {
            ui.close();
            self.pick_relink(&item.key, item.kind);
        }
    }

    /// "N outros arquivos (…) · Mostrar no Explorer" at the end of a folder's list.
    fn others_row(&mut self, ui: &mut egui::Ui, folder: &str, others: &[String]) {
        ui.add_space(4.);
        ui.horizontal(|ui| {
            ui.add_space(38.);
            ui.spacing_mut().item_spacing.x = 0.;
            ui.label(
                RichText::new(format!("{} · ", model::others_line(others)))
                    .size(12.)
                    .color(Color32::from_rgb(120, 120, 125)),
            );
            let show = ui.add(
                egui::Label::new(
                    RichText::new("Mostrar no Explorer")
                        .size(12.)
                        .color(theme::ACCENT_BRIGHT),
                )
                .sense(Sense::click())
                .selectable(false),
            );
            if show.clicked() {
                self.show_in_explorer(folder, true);
            }
        });
    }

    /// The name being typed, in place of a name of the Library: Enter keeps it, Esc lets it go,
    /// a click elsewhere keeps it when it changed.
    fn rename_field(&mut self, ui: &mut egui::Ui, width: f32) {
        let Some(rename) = self.library.rename.as_mut() else {
            return;
        };
        let response = ui.add(
            egui::TextEdit::singleline(&mut rename.text)
                .id_salt("library_rename")
                .desired_width(width.max(60.)),
        );
        if rename.focus {
            response.request_focus();
            rename.focus = false;
            // The name comes selected, ready to be typed over.
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                let end = egui::text::CCursor::new(rename.text.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        end,
                    )));
                state.store(ui.ctx(), response.id);
            }
        }
        if response.changed() {
            rename.error = oxy_core::names::check_name(&rename.text).err();
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.library.rename = None;
            response.surrender_focus();
            return;
        }
        if response.lost_focus() {
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            self.commit_rename(enter);
        }
    }

    /// Why the name being typed is refused, under it.
    fn rename_error(&mut self, ui: &mut egui::Ui, indent: f32) {
        let Some(error) = self.library.rename.as_ref().and_then(|r| r.error.clone()) else {
            return;
        };
        ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.add(egui::Label::new(RichText::new(error).size(11.5).color(theme::ERROR)).wrap());
        });
    }

    fn begin_new_folder(&mut self, parent: &str) {
        if let Some(why) = self.organize_refusal() {
            self.warn(why);
            return;
        }
        self.expand_to(parent);
        self.library.rename = Some(Rename {
            target: Target::NewFolder(parent.to_owned()),
            text: "Nova pasta".into(),
            focus: true,
            error: None,
            in_list: !self.library.column,
        });
    }

    fn begin_folder_rename(&mut self, folder: &str, in_list: bool) {
        if folder.is_empty() {
            return;
        }
        if let Some(why) = self.organize_refusal() {
            self.warn(why);
            return;
        }
        self.expand_to(model::parent(folder));
        self.library.rename = Some(Rename {
            target: Target::Folder(folder.to_owned()),
            text: model::file_name(folder).to_owned(),
            focus: true,
            error: None,
            in_list,
        });
    }

    fn begin_item_rename(&mut self, item: &Item) {
        if let Some(why) = self.rename_refusal(item) {
            self.warn(why);
            return;
        }
        self.library.rename = Some(Rename {
            target: Target::Item(item.key.clone(), item.kind, item.name.clone()),
            text: item.name.clone(),
            focus: true,
            error: None,
            in_list: true,
        });
    }

    /// Renames, or makes the folder, as the name typed says; on Enter a name refused stays in
    /// the field with why, on a click elsewhere it is told in the Console.
    fn commit_rename(&mut self, enter: bool) {
        let Some(mut rename) = self.library.rename.take() else {
            return;
        };
        let name = rename.text.trim().to_owned();
        let (label, command, made) = match &rename.target {
            Target::Item(key, kind, current) => {
                if name == *current {
                    return;
                }
                let asset = match key {
                    Key::Asset(id) => id.clone(),
                    Key::Script(path) | Key::Folder(path) => path.clone(),
                };
                (
                    format!("Renomear {current}"),
                    Command::RenameAsset {
                        asset,
                        name: name.clone(),
                        kind: Some(kind.command()),
                    },
                    None,
                )
            }
            Target::Folder(folder) => {
                if name == model::file_name(folder) {
                    return;
                }
                (
                    format!("Renomear pasta {}", model::file_name(folder)),
                    Command::RenameFolder {
                        folder: folder.clone(),
                        name: name.clone(),
                    },
                    None,
                )
            }
            Target::NewFolder(parent) => {
                if name.is_empty() && !enter {
                    return;
                }
                let path = model::join(parent, &name);
                (
                    format!("Criar pasta {name}"),
                    Command::CreateFolder { path: path.clone() },
                    Some(path),
                )
            }
        };
        match self.library_commands(&label, vec![command]) {
            Ok(_) => {
                if let Some(path) = made {
                    self.open_place(Place::Folder(model::parent(&path).to_owned()));
                    self.library.selection = vec![Key::Folder(path)];
                }
            }
            Err(error) if enter => {
                rename.error = Some(error);
                rename.focus = true;
                self.library.rename = Some(rename);
            }
            Err(error) => self.warn(error),
        }
    }

    /// Delete on the Library's selection: a folder asks before its items go to the root; an
    /// image, sound or model asks as before; scripts and several items ask in their own dialog.
    pub(crate) fn library_delete(&mut self, keys: Vec<Key>) {
        if self.playing() && !keys.is_empty() {
            self.warn(PLAYING);
            return;
        }
        match keys.as_slice() {
            [] => {}
            [Key::Folder(folder)] => {
                if folder.is_empty() {
                    return;
                }
                if let Some(why) = self.organize_refusal() {
                    self.warn(why);
                    return;
                }
                self.open_delete_folder(folder);
            }
            [Key::Asset(id)] => self.asset_ui.delete = Some(id.clone()),
            _ => {
                let items: Vec<Key> = keys
                    .into_iter()
                    .filter(|key| !matches!(key, Key::Folder(_)))
                    .collect();
                if items.iter().any(|key| matches!(key, Key::Script(_)))
                    && let Some(why) = self.item_refusal(Kind::Script)
                {
                    self.warn(why);
                    return;
                }
                if !items.is_empty() {
                    self.library.delete_items = Some(items);
                }
            }
        }
    }

    /// Delete, F2 and Ctrl+A with the Library's focus; Ctrl+D does nothing there.
    pub(crate) fn library_keys(&mut self, ctx: &egui::Context) {
        ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::D));
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
            let lib = self.library_model();
            self.library.selection = self.visible(&lib).iter().map(|i| i.key.clone()).collect();
            self.library.anchor = self.library.selection.first().cloned();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
            let keys = self.library.selection.clone();
            self.library_delete(keys);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
            match self.library.selection.clone().as_slice() {
                [Key::Folder(folder)] => {
                    let in_list = self.library.picked_in_list;
                    self.begin_folder_rename(folder, in_list);
                }
                [key] => {
                    let lib = self.library_model();
                    if let Some(item) = lib.item(key) {
                        self.begin_item_rename(item);
                    }
                }
                _ => {}
            }
        }
    }

    /// Bottom dock: Biblioteca, Console and Avisos tabs, and the code tab while a script is
    /// open in it. Collapses to its tab row.
    pub(super) fn dock(&mut self, ctx: &egui::Context) {
        if self.asset_ui.locate.is_some() {
            self.show_dock(DockTab::Library);
        }
        let open = self.dock_open();
        let panel = egui::TopBottomPanel::bottom("dock").frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(8, 0)),
        );
        let panel = if open {
            panel
                .resizable(true)
                .default_height(176.)
                .height_range(110.0..=(ctx.content_rect().height() * 0.45).max(120.))
        } else {
            panel
                .resizable(false)
                .exact_height(theme::PANEL_HEADER_HEIGHT)
        };
        if !open || self.dock.tab != DockTab::Library {
            self.library.rect = None;
        }
        panel.show(ctx, |ui| {
            if self.mesh_operation_active() {
                ui.disable();
            }
            ui.horizontal(|ui| {
                ui.set_height(theme::PANEL_HEADER_HEIGHT);
                let items = [
                    (DockTab::Library, "Biblioteca", Some(self.library_count())),
                    (DockTab::Console, "Console", None),
                    (DockTab::Notices, "Avisos", Some(self.notices.unread())),
                ];
                let mut clicked = crate::widgets::tabs(ui, open.then_some(self.dock.tab), &items);
                // The script the code tab shows, by its file name, with its close ×.
                if let Some(view) = &self.code_view {
                    let name = super::script_status::file_name(&view.path).to_owned();
                    let selected = open && self.dock.tab == DockTab::Code;
                    let (tab, close) = super::code_view::code_tab(ui, &name, selected);
                    if tab {
                        clicked = Some(DockTab::Code);
                    }
                    if close {
                        clicked = None;
                        self.close_code();
                    }
                }
                if let Some(tab) = clicked {
                    if open && self.dock.tab == tab {
                        self.set_dock_open(false);
                    } else {
                        self.show_dock(tab);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (icon, label) = if open {
                        (Icon::ChevronDown, "Recolher painel")
                    } else {
                        (Icon::ChevronUp, "Expandir painel")
                    };
                    if icons::small(ui, icon, label, 26.).clicked() {
                        self.set_dock_open(!open);
                    }
                    if !open {
                        return;
                    }
                    match self.dock.tab {
                        DockTab::Library => self.library_header(ui),
                        DockTab::Console => {
                            if ui.button("Limpar").clicked() {
                                self.clear_console();
                            }
                            self.console_show_menu(ui);
                        }
                        DockTab::Notices | DockTab::Code => {}
                    }
                });
            });
            if open {
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.min_rect().bottom(),
                    egui::Stroke::new(1., theme::BORDER),
                );
                ui.add_space(8.);
                match self.dock.tab {
                    DockTab::Library => self.library_body(ui),
                    DockTab::Console => self.console_body(ui),
                    DockTab::Notices => self.notices_body(ui),
                    DockTab::Code => self.code_body(ui),
                }
            }
        });
    }
}

#[cfg(test)]
impl Editor {
    /// The folder the Library shows (none for "Todos os recursos").
    pub(crate) fn qa_library_place(&self) -> Option<&str> {
        match &self.library.place {
            Place::All => None,
            Place::Folder(folder) => Some(folder),
        }
    }
    /// The Library's selection: each image, sound or model by its name, each script and folder by
    /// its path.
    pub(crate) fn qa_library_selection(&self) -> Vec<String> {
        self.library
            .selection
            .iter()
            .map(|key| match key {
                Key::Asset(id) => self
                    .state
                    .project
                    .asset(id)
                    .map_or_else(|| id.clone(), |a| a.name.clone()),
                Key::Script(path) | Key::Folder(path) => path.clone(),
            })
            .collect()
    }
    /// Whether a dialog of the Library, the deletion of an asset's included, is open.
    pub(crate) fn qa_library_dialog(&self) -> bool {
        self.library.dialog_open() || self.asset_ui.delete.is_some()
    }
    /// Whether Delete, F2 and Ctrl+A go to the Library now.
    pub(crate) fn qa_library_focus(&self) -> bool {
        self.library_focused()
    }
}

/// A chip of the Library's kinds: "Tudo 5", "Sons 2".
fn chip(ui: &mut egui::Ui, label: &str, on: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(12.),
        if on { theme::TEXT } else { theme::TEXT_MUTED },
    );
    let size = galley.size() + Vec2::new(18., 4.);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            on,
            label,
        )
    });
    let (fill, stroke) = if on {
        (theme::ACCENT_SOFT, theme::ACCENT)
    } else if response.hovered() {
        (theme::BG_RAISED, theme::BORDER)
    } else {
        (theme::BG_WINDOW, theme::CARD_BORDER)
    };
    ui.painter().rect(
        rect,
        10.,
        fill,
        egui::Stroke::new(1., stroke),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.,
        galley,
        Color32::PLACEHOLDER,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::plain;

    #[test]
    fn a_batch_message_reads_without_its_place() {
        assert_eq!(
            plain(
                "Comando 0 (rename_asset, o 1º do lote): Nome inválido: \"a:b\" tem :, que o \
                 Windows não aceita em nomes. Nada foi alterado."
            ),
            "Nome inválido: \"a:b\" tem :, que o Windows não aceita em nomes"
        );
        assert_eq!(
            plain("Pista/Bip.wav está aberto em outro programa. Nada foi alterado."),
            "Pista/Bip.wav está aberto em outro programa"
        );
        assert_eq!(plain("Já existe Sons em Pista"), "Já existe Sons em Pista");
    }
}
