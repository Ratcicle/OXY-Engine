//! Guia: a modal with the list at the left, from top to bottom Scripts, the recipes in script,
//! the reference of the API and the nodes (legacy), and the page chosen at the right; the search
//! looks in all of it. The pages of Scripts, the reference and the search come from oxy_core
//! (`guide_scripts`), the nodes' pages from `guide` and the recipes from `guide_recipes`.
use super::*;
use crate::graph_ui::{GUIDE_FIRST_BEHAVIOR, categories, operation_description};
use crate::icons::{self, Icon};
use crate::theme;
use egui::FontId;
use egui::text::{LayoutJob, TextFormat};
use oxy_core::guide_recipes::{self, ScriptRecipe};
use oxy_core::guide_scripts::{self, Block, Found, Page};
use oxy_core::script::generate::{GuideEntry, GuidePage};

/// The topics of the two lists of the nodes: their recipes and their categories.
const RECIPES: &str = "recipes";
const CATEGORIES: &str = "categories";
/// Width of the list at the left.
const NAV_WIDTH: f32 = 236.;
/// Colour of the paragraphs.
const PARAGRAPH: Color32 = Color32::from_rgb(196, 196, 202);
/// Colour of the text that follows what stands out, such as a row's details after its summary.
const DETAIL: Color32 = Color32::from_rgb(180, 180, 182);
/// Selected colour of the counts and marks of the list.
const SELECTED_NOTE: Color32 = Color32::from_rgb(185, 211, 243);

/// What a page asks to open as a copy.
enum Open {
    Recipe(usize),
    ScriptRecipe(usize),
}

/// The topic of the page of Scripts at `index`; the first is also the Guide's empty topic.
fn script_topic(index: usize) -> String {
    format!("script:{index}")
}

/// The topic of the reference's page of `page`, at its row `name` when given.
fn api_topic(page: &GuidePage, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("api:{}#{name}", page.title),
        None => format!("api:{}", page.title),
    }
}

impl Editor {
    pub(super) fn guide_ui(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut open = None;
        let content = ctx.content_rect();
        let width = (content.width() - 48.).clamp(160., 968.);
        let body_height = (content.height() - 190.).clamp(80., 620.);
        let response = egui::Modal::new("logic_guide".into()).show(ctx, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Guia")
                        .font(theme::medium(16.))
                        .color(theme::TEXT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::small(ui, Icon::Close, "Fechar guia", 24.).clicked() {
                        close = true;
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.logic_ui.search)
                            .hint_text("Buscar assunto, função ou nó")
                            .desired_width((ui.available_width() - 12.).clamp(80., 300.)),
                    );
                });
            });
            ui.add_space(6.);
            if width >= 560. {
                let nav_width = (width * 0.28).clamp(170., NAV_WIDTH);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(nav_width, body_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("guide_nav")
                                .max_height(body_height)
                                .auto_shrink([false, false])
                                .show(ui, |ui| self.guide_nav(ui));
                        },
                    );
                    let (line, _) =
                        ui.allocate_exact_size(Vec2::new(1., body_height), Sense::hover());
                    ui.painter().rect_filled(line, 0., theme::BORDER);
                    ui.add_space(8.);
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), body_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            // Each page opens at its top.
                            egui::ScrollArea::vertical()
                                .id_salt(("guide_article", self.logic_ui.topic.clone()))
                                .max_height(body_height)
                                .auto_shrink([false, false])
                                .show(ui, |ui| open = self.guide_article(ui));
                        },
                    );
                });
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("guide_all")
                    .max_height(body_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.guide_nav(ui);
                        ui.separator();
                        open = self.guide_article(ui);
                    });
            }
            ui.add_space(8.);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Fechar guia").clicked() {
                        close = true;
                    }
                });
            });
        });
        if close || response.should_close() {
            self.logic_ui.guide = false;
        }
        match open {
            Some(Open::Recipe(index)) => {
                let result = (|| {
                    let project = guide_recipes::load(index)?;
                    let path = std::env::temp_dir()
                        .join(format!("oxy-guide-{}", new_id()))
                        .join(persistence::PROJECT_FILE);
                    persistence::save_project(&path, &project)?;
                    Ok::<_, String>(path)
                })();
                match result {
                    Ok(path) => {
                        self.logic_ui.guide = false;
                        self.transition(Transition::Recipe(path));
                    }
                    Err(e) => self.warn(e),
                }
            }
            Some(Open::ScriptRecipe(index)) => {
                // The project and its scripts, each in its folder, in a folder of their own.
                let folder = std::env::temp_dir().join(format!("oxy-guide-{}", new_id()));
                match guide_recipes::write_script_recipe(index, &folder) {
                    Ok(path) => {
                        self.logic_ui.guide = false;
                        self.transition(Transition::ScriptRecipe(path));
                    }
                    Err(e) => self.warn(e),
                }
            }
            None => {}
        }
    }

    /// Shows the page of `topic`; a row of the reference (`api:Área#nome`) scrolls into view.
    fn guide_go(&mut self, topic: String) {
        self.logic_ui.jump = topic.contains('#');
        self.logic_ui.topic = topic;
    }

    /// The list: Scripts, the recipes in script, the reference by area and the nodes (legacy),
    /// or what the search found.
    fn guide_nav(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 1.;
        let topic = self.logic_ui.topic.clone();
        let mut go = None;
        if !self.logic_ui.search.trim().is_empty() {
            nav_section(ui, "Resultados", None, theme::TEXT_MUTED, false);
            let found = guide_scripts::search(&self.logic_ui.search);
            if found.is_empty() {
                crate::widgets::hint(ui, "Nada encontrado. Tente outro termo.");
            }
            for found in found {
                let (mark, label, note, key) = found_item(found);
                if nav_item(ui, mark, &label, Some(note), topic == key).clicked() {
                    go = Some(key);
                }
            }
        } else {
            nav_section(ui, "Scripts", None, theme::TEXT_MUTED, true);
            for (index, page) in guide_scripts::pages().iter().enumerate() {
                let key = script_topic(index);
                let selected = topic == key || (index == 0 && topic.is_empty());
                if nav_item(ui, None, page.title, None, selected).clicked() {
                    go = Some(key);
                }
            }
            nav_section(ui, "Receitas em script", None, theme::TEXT_MUTED, false);
            for (index, recipe) in guide_recipes::script_recipes().iter().enumerate() {
                let key = format!("script-recipe:{index}");
                if nav_item(ui, Some("{ }"), recipe.title, None, topic == key).clicked() {
                    go = Some(key);
                }
            }
            let total = guide_scripts::functions().to_string();
            nav_section(
                ui,
                "Referência da API",
                Some(total.as_str()),
                theme::TEXT_MUTED,
                false,
            );
            for page in guide_scripts::reference() {
                let key = api_topic(page, None);
                let selected = topic == key || topic.starts_with(&format!("{key}#"));
                let count = page.entries.len().to_string();
                if nav_item(ui, None, page.title, Some(count.as_str()), selected).clicked() {
                    go = Some(key);
                }
            }
            // The yellow of the seal "Nós (legado)" of the Lógica.
            nav_section(ui, "Nós (legado)", None, theme::WARNING, false);
            let first = topic == GUIDE_FIRST_BEHAVIOR;
            if nav_item(ui, None, "Primeiro comportamento", None, first).clicked() {
                go = Some(GUIDE_FIRST_BEHAVIOR.to_owned());
            }
            let recipes = guide_recipes::recipes().len().to_string();
            let selected = topic == RECIPES || topic.starts_with("recipe:");
            if nav_item(
                ui,
                None,
                "Receitas em nós",
                Some(recipes.as_str()),
                selected,
            )
            .clicked()
            {
                go = Some(RECIPES.to_owned());
            }
            let count = categories().len().to_string();
            let selected = topic == CATEGORIES
                || topic.starts_with("category:")
                || oxy_core::guide::topics()
                    .iter()
                    .any(|t| t.operation.id == topic);
            if nav_item(
                ui,
                None,
                "Nós por categoria",
                Some(count.as_str()),
                selected,
            )
            .clicked()
            {
                go = Some(CATEGORIES.to_owned());
            }
        }
        if let Some(key) = go {
            self.guide_go(key);
        }
    }

    /// The page chosen in the guide. Returns the recipe to open, if requested.
    fn guide_article(&mut self, ui: &mut egui::Ui) -> Option<Open> {
        let topic = self.logic_ui.topic.clone();
        ui.spacing_mut().item_spacing.y = 6.;
        let index = |prefix: &str| {
            topic
                .strip_prefix(prefix)
                .and_then(|index| index.parse::<usize>().ok())
        };
        if topic.is_empty() {
            script_page(ui, &guide_scripts::pages()[0]);
            return None;
        }
        if let Some(page) = index("script:").and_then(|i| guide_scripts::pages().get(i)) {
            script_page(ui, page);
            return None;
        }
        if let Some(index) = index("script-recipe:")
            && let Some(recipe) = guide_recipes::script_recipes().get(index)
        {
            return script_recipe_page(ui, index, recipe).then_some(Open::ScriptRecipe(index));
        }
        if let Some(api) = topic.strip_prefix("api:") {
            let (title, name) = match api.split_once('#') {
                Some((title, name)) => (title, Some(name)),
                None => (api, None),
            };
            if let Some(page) = guide_scripts::reference()
                .iter()
                .find(|page| page.title == title)
            {
                let mut jump = self.logic_ui.jump.then_some(name).flatten();
                api_page(ui, page, &mut jump);
                if self.logic_ui.jump && jump.is_none() {
                    self.logic_ui.jump = false;
                }
                return None;
            }
        }
        if let Some(index) = index("recipe:")
            && let Some(recipe) = guide_recipes::recipes().get(index)
        {
            kicker(ui, "Receita");
            heading(ui, recipe.title);
            // The recipe opens from the top of its page, before the details.
            let open = ui
                .button(format!(
                    "Abrir cópia da receita: {}",
                    recipe.title.to_lowercase()
                ))
                .clicked();
            ui.add_space(2.);
            section(ui, "Preparação", recipe.setup);
            section(ui, "Conexões e valores", recipe.flow);
            section(ui, "Resultado esperado", recipe.expected);
            return open.then_some(Open::Recipe(index));
        }
        if topic == RECIPES {
            kicker(ui, "Nós (legado)");
            heading(ui, "Receitas em nós");
            paragraph(
                ui,
                "Receitas montadas com nós, o jeito das versões anteriores: continuam funcionando e abrem como cópia editável. A lógica nova se escreve em scripts, como nas Receitas em script.",
            );
            ui.spacing_mut().item_spacing.y = 1.;
            for (index, recipe) in guide_recipes::recipes().iter().enumerate() {
                if nav_item(ui, None, recipe.title, None, false).clicked() {
                    self.guide_go(format!("recipe:{index}"));
                }
            }
            return None;
        }
        if topic == CATEGORIES {
            kicker(ui, "Nós (legado)");
            heading(ui, "Nós por categoria");
            paragraph(
                ui,
                "Escolha uma categoria para ver os nós dela. A busca, acima, também acha um nó pelo nome.",
            );
            ui.spacing_mut().item_spacing.y = 1.;
            for (category, ops) in categories() {
                let count = ops.len().to_string();
                if nav_item(ui, None, category, Some(count.as_str()), false).clicked() {
                    self.guide_go(format!("category:{category}"));
                }
            }
            return None;
        }
        if let Some((category, ops)) = topic
            .strip_prefix("category:")
            .and_then(|name| categories().iter().find(|(category, _)| *category == name))
        {
            kicker(ui, "Nós");
            heading(ui, category);
            paragraph(
                ui,
                "Escolha um nó para ver propósito, entradas e saídas, parâmetros, exemplo e cuidados. O mesmo texto abre por Ajuda deste nó.",
            );
            ui.spacing_mut().item_spacing.y = 1.;
            for op in ops {
                if nav_item(ui, None, op.label, None, false)
                    .on_hover_text(operation_description(op.id))
                    .clicked()
                {
                    self.guide_go(op.id.into());
                }
            }
            return None;
        }
        if let Some(topic) = oxy_core::guide::topics()
            .iter()
            .find(|t| t.operation.id == topic)
        {
            let op = topic.operation;
            kicker(ui, op.category);
            heading(ui, op.label);
            paragraph(ui, topic.purpose);
            section(ui, "Preparação e contexto", topic.requirements());
            let ports: Vec<String> = op
                .inputs
                .iter()
                .map(|port| format!("Entrada · {} ({})", port.label, port.kind.label()))
                .chain(
                    op.outputs
                        .iter()
                        .map(|port| format!("Saída · {} ({})", port.label, port.kind.label())),
                )
                .collect();
            if !ports.is_empty() {
                section(ui, "Entradas e saídas", &ports.join("\n"));
            }
            if !op.params.is_empty() {
                let params: Vec<_> = op.params.iter().map(|param| param.label).collect();
                section(ui, "Parâmetros", &params.join("\n"));
            }
            section(ui, "Exemplo", topic.example);
            section(ui, "Cuidados", topic.caution);
            return None;
        }
        // The first behavior, also what a topic the Guide no longer has falls back to.
        heading(ui, "Seu primeiro comportamento");
        for line in oxy_core::guide::FIRST_BEHAVIOR.lines() {
            match line
                .split_once(". ")
                .filter(|(number, _)| number.chars().all(|c| c.is_ascii_digit()))
            {
                Some((number, text)) => step(ui, number, text, PARAGRAPH),
                None => paragraph(ui, line),
            }
        }
        let open = ui
            .button("Abrir cópia da receita: primeira mensagem")
            .clicked();
        ui.add_space(4.);
        ui.separator();
        ui.label(
            egui::RichText::new("Execução e dados")
                .font(theme::medium(14.))
                .color(theme::TEXT),
        );
        for line in oxy_core::guide::FOUNDATIONS.lines() {
            paragraph(ui, line);
        }
        open.then_some(Open::Recipe(0))
    }
}

#[cfg(test)]
impl Editor {
    /// The Guide's topic while it is open.
    pub(crate) fn qa_guide_topic(&self) -> Option<&str> {
        self.logic_ui.guide.then_some(self.logic_ui.topic.as_str())
    }
}

/// A result of the search as the list shows it: its mark, its label, the note at its right and
/// its topic.
fn found_item(found: Found) -> (Option<&'static str>, String, &'static str, String) {
    match found {
        Found::Page(index) => (
            None,
            guide_scripts::pages()[index].title.into(),
            "Scripts",
            script_topic(index),
        ),
        Found::ScriptRecipe(index) => (
            Some("{ }"),
            guide_recipes::script_recipes()[index].title.into(),
            "Receita",
            format!("script-recipe:{index}"),
        ),
        Found::Function { name, .. } => {
            let (page, entry) = guide_scripts::entry(name).expect("A busca acha linhas da API");
            (
                None,
                guide_scripts::call_name(entry).into(),
                page.title,
                api_topic(page, Some(name)),
            )
        }
        Found::Recipe(index) => (
            None,
            guide_recipes::recipes()[index].title.into(),
            "Receita em nós",
            format!("recipe:{index}"),
        ),
        Found::Node(id) => {
            let op = oxy_core::graph::registry()
                .iter()
                .find(|op| op.id == id)
                .expect("A busca acha nós do registro");
            (None, op.label.into(), op.category, id.into())
        }
    }
}

/// A page of Scripts: its title, then its texts, numbered steps, scripts and parts.
fn script_page(ui: &mut egui::Ui, page: &Page) {
    heading(ui, page.heading);
    let mut number = 0;
    let mut after_step = false;
    for block in page.blocks {
        match *block {
            Block::Text(text) => rich_paragraph(ui, text, PARAGRAPH),
            Block::Step(text) => {
                number += 1;
                step(ui, &number.to_string(), text, theme::TEXT);
            }
            // A script right after a step belongs to it, under its text; another one says its
            // file.
            Block::Code { file, code } if after_step => {
                ui.horizontal_top(|ui| {
                    ui.add_space(30.);
                    ui.vertical(|ui| crate::app::code_view::snippet(ui, file, code, true));
                });
            }
            Block::Code { file, code } => {
                script_title(ui, file, code);
                crate::app::code_view::snippet(ui, file, code, true);
            }
            Block::Heading(title) => {
                ui.add_space(2.);
                ui.separator();
                ui.label(
                    egui::RichText::new(title)
                        .font(theme::medium(14.))
                        .color(theme::TEXT),
                );
            }
        }
        after_step = matches!(block, Block::Step(_));
    }
}

/// A recipe in script: Abrir cópia, the preparation, each script whole and the expected
/// result. Answers whether Abrir cópia was clicked.
fn script_recipe_page(ui: &mut egui::Ui, index: usize, recipe: &ScriptRecipe) -> bool {
    kicker(ui, "Receita em script");
    heading(ui, recipe.title);
    let open = ui
        .button(format!(
            "Abrir cópia da receita: {}",
            recipe.title.to_lowercase()
        ))
        .clicked();
    ui.add_space(2.);
    section(ui, "Preparação", recipe.setup);
    for (path, text) in recipe.scripts {
        script_title(ui, path, text);
        crate::app::code_view::snippet(ui, ("guide_recipe", index, *path), text, true);
    }
    section(ui, "Resultado esperado", recipe.expected);
    open
}

/// "SCRIPT · area_da_porta.luau", with its count of lines at the right.
fn script_title(ui: &mut egui::Ui, path: &str, text: &str) {
    ui.add_space(2.);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.), Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        format!("SCRIPT · {path}"),
        theme::medium(10.5),
        theme::TEXT_MUTED,
    );
    let lines = text.trim_end().lines().count();
    ui.painter().text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        if lines == 1 {
            "1 linha".to_owned()
        } else {
            format!("{lines} linhas")
        },
        FontId::proportional(11.5),
        theme::TEXT_MUTED,
    );
}

/// A page of the reference: its area, how many rows it has, what it is about and each row.
/// `jump` is the row to scroll into view, taken when it is drawn.
fn api_page(ui: &mut egui::Ui, page: &GuidePage, jump: &mut Option<&str>) {
    kicker(ui, "Referência da API");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.;
        heading(ui, page.title);
        let count = match page.entries.len() {
            1 => "1 função".to_owned(),
            n => format!("{n} funções"),
        };
        ui.label(
            egui::RichText::new(count)
                .size(12.5)
                .color(theme::TEXT_MUTED),
        );
    });
    rich_paragraph(ui, page.intro, DETAIL);
    for entry in &page.entries {
        ui.add_space(6.);
        let strip = signature(ui, entry);
        if *jump == Some(entry.name) {
            strip.scroll_to_me(Some(egui::Align::TOP));
            *jump = None;
        }
        api_entry(ui, entry);
    }
}

/// A row of the reference under its signature: its summary standing out before the details, its
/// parameters, its nodes, where it runs in 0.8 and its example.
fn api_entry(ui: &mut egui::Ui, entry: &GuideEntry) {
    let mut job = LayoutJob::default();
    rich(&mut job, entry.summary, theme::TEXT, true);
    let mut details = entry.doc.to_owned();
    if entry.writable {
        details = format!("Leitura e escrita. {details}");
    }
    if !details.trim().is_empty() {
        rich(&mut job, &format!(" {}", details.trim_end()), DETAIL, false);
    }
    ui.add(egui::Label::new(job).wrap());
    for param in &entry.params {
        meta_row(ui, param.name, |ui| {
            rich_paragraph(ui, param.doc, theme::TEXT)
        });
    }
    let nodes = guide_scripts::node_labels(entry);
    let chip = format!("0.8: {}", entry.domain);
    if nodes.is_empty() {
        domain_chip(ui, &chip);
    } else {
        meta_row(ui, "Em nós", |ui| {
            ui.horizontal_top(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    domain_chip(ui, &chip);
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(nodes.join(", "))
                                    .size(12.5)
                                    .color(theme::TEXT),
                            )
                            .wrap(),
                        );
                    });
                });
            });
        });
    }
    crate::app::code_view::snippet(ui, ("guide_example", entry.name), entry.example, false);
}

/// The signature of a row in a dark strip, its name in blue.
fn signature(ui: &mut egui::Ui, entry: &GuideEntry) -> egui::Response {
    let name = guide_scripts::call_name(entry);
    let rest = &entry.signature[name.len()..];
    let font = FontId::monospace(12.5);
    let mut job = LayoutJob::default();
    for (text, color) in [(name, theme::ACCENT_BRIGHT), (rest, theme::TEXT)] {
        job.append(
            text,
            0.,
            TextFormat {
                font_id: font.clone(),
                color,
                ..Default::default()
            },
        );
    }
    egui::Frame::new()
        .fill(Color32::from_rgb(30, 30, 32))
        .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(job).wrap());
        })
        .response
}

/// A line of a row's details: a short muted name at the left, `add` at its right.
fn meta_row(ui: &mut egui::Ui, tag: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(84., 18.), Sense::hover());
        ui.painter().with_clip_rect(rect).text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            tag,
            FontId::proportional(12.),
            theme::TEXT_MUTED,
        );
        ui.vertical(add);
    });
}

/// Where a row runs in 0.8, as a small chip.
fn domain_chip(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(theme::BG_HEADER)
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(11.5)
                    .color(Color32::from_rgb(180, 180, 182)),
            );
        });
}

/// Appends `text` to `job`: a name in backticks as code (a `file.luau:line` in the blue of the
/// Console's links), `**text**` standing out, the rest in `color`; all of it standing out when
/// `strong`.
fn rich(job: &mut LayoutJob, text: &str, color: Color32, strong: bool) {
    let mut append = |text: &str, font_id: FontId, color: Color32| {
        if !text.is_empty() {
            job.append(
                text,
                0.,
                TextFormat {
                    font_id,
                    color,
                    ..Default::default()
                },
            );
        }
    };
    for (index, part) in text.split('`').enumerate() {
        if index % 2 == 1 {
            let link = part.contains(".luau:");
            let color = if link {
                theme::ACCENT_BRIGHT
            } else {
                theme::TEXT
            };
            append(part, FontId::monospace(12.), color);
            continue;
        }
        for (index, piece) in part.split("**").enumerate() {
            if strong || index % 2 == 1 {
                append(piece, theme::medium(12.5), theme::TEXT);
            } else {
                append(piece, FontId::proportional(12.5), color);
            }
        }
    }
}

fn rich_paragraph(ui: &mut egui::Ui, text: &str, color: Color32) {
    let mut job = LayoutJob::default();
    rich(&mut job, text, color, false);
    ui.add(egui::Label::new(job).wrap());
}

/// The title of a part of the list, at the left of an optional count.
fn nav_section(ui: &mut egui::Ui, title: &str, count: Option<&str>, color: Color32, first: bool) {
    ui.add_space(if first { 2. } else { 8. });
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.), Sense::hover());
    ui.painter().text(
        rect.left_center() + Vec2::new(8., 0.),
        egui::Align2::LEFT_CENTER,
        title.to_uppercase(),
        theme::medium(10.5),
        color,
    );
    if let Some(count) = count {
        ui.painter().text(
            rect.right_center() - Vec2::new(8., 0.),
            egui::Align2::RIGHT_CENTER,
            count,
            FontId::proportional(11.),
            theme::TEXT_MUTED,
        );
    }
    ui.add_space(2.);
}

/// Full-width row of the guide navigation: an optional mark before its label (the `{ }` of a
/// recipe in script) and an optional muted note at the right.
fn nav_item(
    ui: &mut egui::Ui,
    mark: Option<&str>,
    label: &str,
    trailing: Option<&str>,
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    let fill = if selected {
        theme::ACCENT_SOFT
    } else if response.hovered() {
        theme::BG_RAISED
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 5., fill);
    let note = if selected {
        SELECTED_NOTE
    } else {
        theme::TEXT_MUTED
    };
    let mut right = rect.right() - 8.;
    if let Some(trailing) = trailing {
        let galley =
            ui.painter()
                .layout_no_wrap(trailing.to_owned(), FontId::proportional(11.), note);
        right -= galley.size().x;
        ui.painter().galley(
            egui::pos2(right, rect.center().y - galley.size().y / 2.),
            galley,
            note,
        );
        right -= 8.;
    }
    let mut left = rect.left() + 8.;
    if let Some(mark) = mark {
        let mark_color = if selected {
            SELECTED_NOTE
        } else {
            theme::ACCENT_BRIGHT
        };
        let drawn = ui.painter().text(
            egui::pos2(left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            mark,
            FontId::monospace(10.5),
            mark_color,
        );
        left = drawn.right() + 6.;
    }
    let color = if selected {
        Color32::WHITE
    } else {
        theme::TEXT
    };
    ui.painter().with_clip_rect(rect.with_max_x(right)).text(
        egui::pos2(left, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.5),
        color,
    );
    response
}

fn kicker(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .font(theme::medium(10.5))
            .color(theme::ACCENT_BRIGHT),
    );
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(theme::medium(17.))
            .color(theme::TEXT),
    );
}

fn paragraph(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(12.5).color(PARAGRAPH)).wrap());
}

fn section(ui: &mut egui::Ui, title: &str, text: &str) {
    ui.add_space(2.);
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .font(theme::medium(10.5))
            .color(theme::TEXT_MUTED),
    );
    paragraph(ui, text);
}

/// A numbered step: its number in a circle, then its text in `color`.
fn step(ui: &mut egui::Ui, number: &str, text: &str, color: Color32) {
    ui.horizontal_top(|ui| {
        let (badge, _) = ui.allocate_exact_size(Vec2::splat(20.), Sense::hover());
        ui.painter()
            .circle_filled(badge.center(), 9., theme::ACCENT_SOFT);
        ui.painter().text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            number,
            theme::medium(11.),
            theme::ACCENT_BRIGHT,
        );
        rich_paragraph(ui, text, color);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Rect, Shape};
    use oxy_core::script::registry::Area;

    /// The texts `draw` paints on a page 700 points wide, tall enough for all of it, on the
    /// second frame (the first loads the theme's fonts).
    fn texts(draw: impl Fn(&mut egui::Ui)) -> Vec<String> {
        fn collect(shape: &Shape, found: &mut Vec<String>) {
            match shape {
                Shape::Text(text) => found.push(text.galley.text().to_owned()),
                Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, found)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut found = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(760., 9000.))),
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(700.);
                    draw(ui);
                });
            });
            found.clear();
            for shape in &output.shapes {
                collect(&shape.shape, &mut found);
            }
        }
        found
    }

    #[test]
    fn the_first_page_shows_its_steps_and_girar_whole_with_its_line_numbers() {
        let page = &guide_scripts::pages()[0];
        let drawn = texts(|ui| script_page(ui, page));
        let has = |text: &str| drawn.iter().any(|t| t == text);
        assert!(has("Seu primeiro script") && has("Como um script funciona"));
        let Block::Code { code, .. } = page.blocks[2] else {
            panic!("girar.luau vem depois do primeiro passo");
        };
        for (index, line) in code.lines().enumerate() {
            assert!(line.is_empty() || has(line), "{line}");
            assert!(has(&(index + 1).to_string()), "linha {}", index + 1);
        }
        // Code in backticks loses them; the step's emphasis keeps its text.
        assert!(
            drawn
                .iter()
                .any(|t| t.starts_with("Crie girar.luau em qualquer pasta"))
        );
        assert!(
            drawn
                .iter()
                .any(|t| t.contains("use Scripts › + Anexar script."))
        );
    }

    #[test]
    fn a_page_of_the_reference_shows_each_row_whole() {
        for (area, count) in [
            (Area::Timers, "5 funções"),
            (Area::Memory, "2 funções"),
            (Area::Modules, "1 função"),
        ] {
            let page = guide_scripts::reference_page(area).unwrap();
            let drawn = texts(|ui| api_page(ui, page, &mut None));
            let has = |text: &str| drawn.iter().any(|t| t == text);
            assert!(has("REFERÊNCIA DA API") && has(page.title) && has(count));
            for entry in &page.entries {
                assert!(has(&entry.signature), "{}", entry.name);
                let summary = entry.summary.replace('`', "");
                assert!(
                    drawn.iter().any(|t| t.starts_with(&summary)),
                    "{}",
                    entry.name
                );
                assert!(has(&format!("0.8: {}", entry.domain)), "{}", entry.name);
                let nodes = guide_scripts::node_labels(entry);
                assert!(nodes.is_empty() || has(&nodes.join(", ")), "{}", entry.name);
                for param in &entry.params {
                    assert!(has(param.name), "{}: {}", entry.name, param.name);
                }
                for line in entry.example.lines() {
                    assert!(line.is_empty() || has(line), "{}: {line}", entry.name);
                }
            }
        }
    }

    #[test]
    fn a_recipe_in_script_shows_each_script_whole_and_not_the_nodes() {
        for (index, recipe) in guide_recipes::script_recipes().iter().enumerate() {
            let drawn = texts(|ui| {
                script_recipe_page(ui, index, recipe);
            });
            let has = |text: &str| drawn.iter().any(|t| t == text);
            let open = format!("Abrir cópia da receita: {}", recipe.title.to_lowercase());
            assert!(has(&open) && has("PREPARAÇÃO") && has("RESULTADO ESPERADO"));
            for (path, text) in recipe.scripts {
                assert!(has(&format!("SCRIPT · {path}")), "{path}");
                for line in text.lines() {
                    assert!(line.is_empty() || has(line), "{path}: {line}");
                }
            }
            assert!(!has("CONEXÕES E VALORES"), "{}", recipe.title);
        }
    }

    #[test]
    fn a_result_of_the_search_opens_its_page() {
        let (mark, label, note, topic) = found_item(Found::Function {
            area: Area::Timers,
            name: "Object:after",
        });
        assert_eq!(
            (mark, label.as_str(), note, topic.as_str()),
            (
                None,
                "obj:after",
                "Temporizadores",
                "api:Temporizadores#Object:after"
            )
        );
        let (mark, label, _, topic) = found_item(Found::ScriptRecipe(0));
        assert_eq!(
            (mark, label.as_str(), topic.as_str()),
            (Some("{ }"), "Porta com chave", "script-recipe:0")
        );
        assert_eq!(found_item(Found::Recipe(1)).3, "recipe:1");
        assert_eq!(found_item(Found::Page(2)).3, "script:2");
        assert_eq!(found_item(Found::Node("control.wait")).3, "control.wait");
    }
}
