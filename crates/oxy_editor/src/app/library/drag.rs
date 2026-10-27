//! Dragging in the Library: the items and folders picked, carried as a [`LibraryDrag`] (which
//! the Hierarchy, carrying objects, never takes), dropped on a folder of the PASTAS column, a
//! subfolder of the list or a part of the breadcrumb, the project's root among them. The target
//! turns green with what dropping does, or red with why not, as the tree's commands plan it;
//! dropping moves the files as one undo step.
use super::*;

/// What is dragged in the Library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LibraryDrag {
    pub(crate) items: Vec<Key>,
}

/// What dropping the dragged items on a folder does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Landing {
    /// They move there: what the target says ("Mover para Sons").
    Move(String),
    /// Nothing: they are all in that folder already.
    Here,
    /// Refused: why, as a sentence.
    Refused(String),
}

/// A target that takes the drop: its fill, its border and the color of its text.
const TAKES: (Color32, Color32, Color32) = (
    Color32::from_rgb(35, 54, 42),
    Color32::from_rgb(108, 196, 140),
    Color32::from_rgb(189, 232, 200),
);
/// A target that refuses it.
const REFUSES: (Color32, Color32, Color32) = (
    Color32::from_rgb(58, 34, 36),
    Color32::from_rgb(224, 108, 108),
    Color32::from_rgb(246, 201, 201),
);

impl Landing {
    /// The fill, border and text color of a target with this landing, when it shows one.
    pub(super) fn colors(&self) -> Option<(Color32, Color32, Color32)> {
        match self {
            Self::Move(_) => Some(TAKES),
            Self::Refused(_) => Some(REFUSES),
            Self::Here => None,
        }
    }
}

/// "Sons já tem um som chamado Bipe. Renomeie um dos dois antes.": why `name` cannot go to the
/// folder `place` names, where `what` ("um som chamado") has it.
pub(super) fn taken_line(place: &str, what: &str, name: &str) -> String {
    format!("{place} já tem {what} {name}. Renomeie um dos dois antes.")
}

/// What the label at the pointer says of `items` while they are dragged: "Bipe", "3 itens".
pub(super) fn dragged_line(items: &[Key], lib: &Library) -> (Option<Kind>, String) {
    match items {
        [Key::Folder(folder)] => (None, model::file_name(folder).to_owned()),
        [key] => lib.item(key).map_or((None, String::new()), |item| {
            (Some(item.kind), item.name.clone())
        }),
        _ => (None, model::count(items.len(), "item", "itens")),
    }
}

impl Editor {
    /// What dragging from the row of `key` carries: the selection when it has the row (the
    /// project's root left out), else the row alone, which becomes the selection.
    pub(super) fn begin_drag(&mut self, response: &egui::Response, key: &Key) {
        if !response.drag_started() {
            return;
        }
        if !self.library.selection.contains(key) {
            self.library.selection = vec![key.clone()];
            self.library.anchor = Some(key.clone());
        }
        let items: Vec<Key> = self
            .library
            .selection
            .iter()
            .filter(|key| !matches!(key, Key::Folder(folder) if folder.is_empty()))
            .cloned()
            .collect();
        self.library.rename = None;
        response.dnd_set_drag_payload(LibraryDrag { items });
    }

    /// The landing of what is dragged over `response`, the row of the folder `folder`, if
    /// anything of the Library is; told to the label at the pointer.
    pub(super) fn hovered_landing(
        &mut self,
        response: &egui::Response,
        folder: &str,
    ) -> Option<Landing> {
        let drag = response.dnd_hover_payload::<LibraryDrag>()?;
        let landing = self.landing_on(&drag.items, folder);
        self.library.landing = Some(landing.clone());
        Some(landing)
    }

    /// Moves what is dropped on `response`, the row of the folder `folder`, when it takes it;
    /// says why not otherwise.
    pub(super) fn take_drop(&mut self, response: &egui::Response, folder: &str) {
        let Some(drag) = response.dnd_release_payload::<LibraryDrag>() else {
            return;
        };
        self.library.landings = None;
        match self.landing_on(&drag.items, folder) {
            Landing::Move(_) => {
                let lib = self.library_model();
                if let Err(error) = self.move_keys(&drag.items, folder, &lib) {
                    self.warn(error);
                }
            }
            Landing::Refused(why) => self.warn(why),
            Landing::Here => {}
        }
    }

    /// A target row painted as its landing says: green or red with its border, over `rect`.
    pub(super) fn paint_landing(ui: &egui::Ui, rect: Rect, landing: Option<&Landing>) {
        if let Some((fill, stroke, _)) = landing.and_then(Landing::colors) {
            ui.painter().rect(
                rect,
                4.,
                fill,
                egui::Stroke::new(1., stroke),
                egui::StrokeKind::Inside,
            );
        }
    }

    /// What dropping `items` on `folder` does, planned once per folder while the drag lasts.
    fn landing_on(&mut self, items: &[Key], folder: &str) -> Landing {
        if let Some((keys, known)) = &self.library.landings
            && keys == items
            && let Some(landing) = known.get(folder)
        {
            return landing.clone();
        }
        let landing = self.plan_landing(items, folder);
        let landings = self
            .library
            .landings
            .get_or_insert_with(|| (items.to_vec(), BTreeMap::new()));
        if landings.0 != items {
            *landings = (items.to_vec(), BTreeMap::new());
        }
        landings.1.insert(folder.to_owned(), landing.clone());
        landing
    }

    /// [`Self::landing_on`] planned by the tree's commands on the folder as it is now.
    fn plan_landing(&self, items: &[Key], folder: &str) -> Landing {
        // A folder over its own row, as its drag starts, lands nowhere.
        if items.is_empty()
            || items
                .iter()
                .any(|key| matches!(key, Key::Folder(path) if path == folder))
        {
            return Landing::Here;
        }
        if let Some(why) = self.organize_refusal() {
            return Landing::Refused(why.to_owned());
        }
        let lib = self.library_model();
        let here = items.iter().all(|key| match key {
            Key::Folder(path) => model::parent(path) == folder,
            key => lib.item(key).is_some_and(|item| item.folder == folder),
        });
        if here {
            return Landing::Here;
        }
        let Some(root) = self.project_root() else {
            return Landing::Refused(UNSAVED.to_owned());
        };
        let commands = [move_command(items, folder)];
        let planned = command::TreeView::read(&root)
            .map_err(|error| error.to_string())
            .and_then(|mut view| {
                command::plan_with_files(&self.state.project, &self.scene_id, &commands, &mut view)
                    .map(drop)
                    .map_err(|error| plain(&error.to_string()))
            });
        match planned {
            Ok(()) => Landing::Move(match items {
                [_] => format!("Mover para {}", self.folder_label(folder)),
                _ => format!(
                    "Mover {} para {}",
                    model::count(items.len(), "item", "itens"),
                    self.folder_label(folder)
                ),
            }),
            Err(why) => Landing::Refused(
                self.taken_reason(&lib, items, folder)
                    .unwrap_or_else(|| format!("{why}.")),
            ),
        }
    }

    /// Why `items` cannot go to `folder` in the Library's words, when something there already
    /// has the name of one of them.
    fn taken_reason(&self, lib: &Library, items: &[Key], folder: &str) -> Option<String> {
        let place = self.folder_label(folder);
        for key in items {
            let name = match key {
                Key::Folder(path) => model::file_name(path).to_owned(),
                key => lib.item(key)?.file_name().to_owned(),
            };
            let same = |other: &str| oxy_core::names::same_name(other, &name);
            if let Some(other) = lib.items.iter().find(|other| {
                other.folder == folder && other.key != *key && same(other.file_name())
            }) {
                return Some(taken_line(&place, other.kind.called(), &other.name));
            }
            if lib.subfolders(folder).iter().any(|other| same(&other.name)) {
                return Some(taken_line(&place, "uma pasta chamada", &name));
            }
            if lib
                .others
                .get(folder)
                .is_some_and(|others| others.iter().any(|o| same(o)))
            {
                return Some(taken_line(&place, "um arquivo chamado", &name));
            }
        }
        None
    }

    /// While something of the Library is dragged: what it is and, over a folder, what dropping
    /// there does, at the pointer.
    pub(super) fn drag_label(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(drag) = egui::DragAndDrop::payload::<LibraryDrag>(ctx) else {
            self.library.landings = None;
            return;
        };
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        let Some(pointer) = ctx.pointer_latest_pos() else {
            return;
        };
        let (kind, name) = dragged_line(&drag.items, lib);
        let landing = self.library.landing.clone();
        egui::Area::new(egui::Id::new("library_drag"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pointer + Vec2::new(14., 10.))
            .interactable(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 4.;
                egui::Frame::new()
                    .fill(theme::BG_WINDOW)
                    .stroke(egui::Stroke::new(1., theme::ACCENT))
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .shadow(egui::Shadow {
                        offset: [0, 6],
                        blur: 18,
                        spread: 0,
                        color: Color32::from_black_alpha(128),
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (icon, _) =
                                ui.allocate_exact_size(Vec2::new(16., 18.), Sense::hover());
                            match kind {
                                Some(kind) => {
                                    paint_icon(ui, icon, kind, Color32::from_rgb(180, 180, 182))
                                }
                                None => icons::paint(
                                    ui,
                                    Rect::from_center_size(icon.center(), Vec2::splat(14.)),
                                    Icon::Folder,
                                    Color32::from_rgb(180, 180, 182),
                                ),
                            }
                            ui.label(RichText::new(name).color(theme::TEXT));
                        });
                    });
                let Some(landing) = landing else {
                    return;
                };
                let (Landing::Move(text) | Landing::Refused(text)) = &landing else {
                    return;
                };
                let (fill, _, color) = landing.colors().expect("a landing that shows");
                egui::Frame::new()
                    .fill(fill)
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.set_max_width(320.);
                        ui.add(egui::Label::new(RichText::new(text).size(12.).color(color)).wrap());
                    });
            });
    }
}

/// The command that moves `keys` into the folder `to`.
pub(super) fn move_command(keys: &[Key], to: &str) -> Command {
    Command::Move {
        items: keys
            .iter()
            .map(|key| match key {
                Key::Asset(id) => id.clone(),
                Key::Script(path) | Key::Folder(path) => path.clone(),
            })
            .collect(),
        to: to.to_owned(),
        kind: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_taken_reads_as_the_prototype_says_it() {
        assert_eq!(
            taken_line("Sons", Kind::Sound.called(), "Bipe"),
            "Sons já tem um som chamado Bipe. Renomeie um dos dois antes."
        );
        assert_eq!(
            taken_line("parkour", Kind::Image.called(), "Pedra"),
            "parkour já tem uma imagem chamada Pedra. Renomeie um dos dois antes."
        );
    }

    #[test]
    fn the_label_at_the_pointer_names_one_item_or_counts_them() {
        let lib = Library {
            items: vec![Item {
                key: Key::Asset("a".into()),
                kind: Kind::Sound,
                name: "Bipe".into(),
                path: "Pista/Bipe.wav".into(),
                folder: "Pista".into(),
                state: State::Ok,
                new: false,
                usage: String::new(),
                error_line: None,
            }],
            ..Library::default()
        };
        assert_eq!(
            dragged_line(&[Key::Asset("a".into())], &lib),
            (Some(Kind::Sound), "Bipe".to_owned())
        );
        assert_eq!(
            dragged_line(&[Key::Folder("Pista/Checkpoints".into())], &lib),
            (None, "Checkpoints".to_owned())
        );
        assert_eq!(
            dragged_line(&[Key::Asset("a".into()), Key::Folder("Menu".into())], &lib),
            (None, "2 itens".to_owned())
        );
        assert_eq!(Landing::Here.colors(), None);
        assert_eq!(Landing::Move(String::new()).colors(), Some(TAKES));
    }

    #[test]
    fn a_move_names_its_items_and_where_they_go() {
        assert_eq!(
            move_command(
                &[Key::Asset("id".into()), Key::Script("Pista/a.luau".into())],
                ""
            ),
            Command::Move {
                items: vec!["id".into(), "Pista/a.luau".into()],
                to: String::new(),
                kind: None,
            }
        );
    }
}
