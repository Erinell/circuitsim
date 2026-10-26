//! Application shell, toolbar, shortcuts, and panel layout.

use editor_core::ObjectRef;
use eframe::egui::{self, Align, Color32, Context, FontId, Key, Layout, RichText, Ui};

use crate::document::Document;
use crate::ui::canvas::CanvasState;
use crate::ui::instruments::InstrumentsState;
use crate::ui::properties::InspectorState;
use crate::ui::{self, Status, Tool};
use crate::view::DEFAULT_SCALE;

/// Root application state.
pub struct CircuitApp {
    document: Document,
    tool: Tool,
    filter: String,
    canvas: CanvasState,
    inspector: InspectorState,
    instruments: InstrumentsState,
    status: Status,
    show_help: bool,
    show_instruments: bool,
}

impl CircuitApp {
    /// Creates the application and configures the egui style.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);
        let (document, status) = match Document::demo() {
            Ok(document) => (
                document,
                Status::new_success("Schéma de démonstration chargé."),
            ),
            Err(error) => (
                Document::new(),
                Status::new_error(format!(
                    "Démonstration indisponible : {}",
                    error.user_message()
                )),
            ),
        };
        Self {
            document,
            tool: Tool::Select,
            filter: String::new(),
            canvas: CanvasState::new(),
            inspector: InspectorState::default(),
            instruments: InstrumentsState::new(),
            status,
            show_help: false,
            show_instruments: true,
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        if self.tool != tool {
            self.canvas.cancel_interaction();
            self.tool = tool;
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("CircuitSim").strong().size(16.0));
            ui.label(RichText::new("Éditeur").small().color(ui::MUTED));
            if self.document.is_dirty() {
                ui.label(RichText::new("● modifié").small().color(ui::ACCENT));
            }
            ui.separator();

            if ui
                .button("Nouveau")
                .on_hover_text("Efface le schéma (annulable)")
                .clicked()
            {
                let was_empty = self.document.schematic().is_empty();
                match self.document.clear() {
                    Ok(()) => {
                        self.canvas.cancel_interaction();
                        self.canvas.request_fit();
                        if was_empty {
                            self.status.info("Le schéma est déjà vide.");
                        } else {
                            self.status
                                .success("Schéma effacé. Ctrl+Z permet de le restaurer.");
                        }
                    }
                    Err(error) => self.status.error(error.user_message()),
                }
            }

            ui.add_enabled_ui(self.document.can_undo(), |ui| {
                if ui.button("Annuler").on_hover_text("Ctrl+Z").clicked() {
                    self.undo();
                }
            });
            ui.add_enabled_ui(self.document.can_redo(), |ui| {
                if ui.button("Rétablir").on_hover_text("Ctrl+Y").clicked() {
                    self.redo();
                }
            });
            ui.separator();

            tool_toggle(
                ui,
                &mut self.tool,
                &mut self.canvas,
                Tool::Select,
                "Sélection",
            );
            tool_toggle(ui, &mut self.tool, &mut self.canvas, Tool::Wire, "Fil");
            tool_toggle(
                ui,
                &mut self.tool,
                &mut self.canvas,
                Tool::Junction,
                "Jonction",
            );
            tool_toggle(ui, &mut self.tool, &mut self.canvas, Tool::Label, "Label");
            ui.separator();

            let has_symbols = selection_has_symbols(&self.document);
            ui.add_enabled_ui(has_symbols, |ui| {
                if ui.button("Pivoter").on_hover_text("R").clicked() {
                    self.rotate();
                }
                if ui.button("Miroir").on_hover_text("M").clicked() {
                    self.mirror();
                }
            });
            ui.add_enabled_ui(!self.document.selection().is_empty(), |ui| {
                if ui.button("Supprimer").on_hover_text("Suppr").clicked() {
                    self.delete();
                }
            });
            if ui.button("Ajuster").on_hover_text("Home").clicked() {
                self.canvas.request_fit();
            }
            ui.separator();
            if ui
                .selectable_label(self.show_instruments, "Instruments")
                .on_hover_text("Afficher ou masquer la simulation, les portées et le Bode")
                .clicked()
            {
                self.show_instruments = !self.show_instruments;
            }
            if ui.button("Aide").clicked() {
                self.show_help = !self.show_help;
            }
        });
        ui.add_space(2.0);
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            self.status.show(ui);
            ui.separator();
            let (nets, components) = self.document.extraction().map_or((0, 0), |extraction| {
                (extraction.nets.len(), extraction.netlist.components.len())
            });
            ui.label(
                RichText::new(format!(
                    "{nets} net(s) · {components} composant(s) · {} sélectionné(s)",
                    self.document.selection().len()
                ))
                .small()
                .color(ui::MUTED),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let zoom = self.canvas.viewport().scale() / DEFAULT_SCALE * 100.0;
                ui.label(
                    RichText::new(format!("Zoom {zoom:.0} % · grille entière"))
                        .small()
                        .color(ui::MUTED),
                );
            });
        });
    }

    fn palette(&mut self, ui: &mut Ui) {
        ui::palette::show(
            ui,
            self.document.registry(),
            &mut self.filter,
            &mut self.tool,
            &mut self.canvas,
        );
    }

    fn inspector(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui::properties::show(
                    ui,
                    &mut self.document,
                    &mut self.inspector,
                    &mut self.status,
                );
                ui::erc::show(ui, &mut self.document, &mut self.status);
            });
    }

    fn canvas(&mut self, ui: &mut Ui) {
        let interactive = !self.canvas.has_dialog() && !self.show_help;
        ui::canvas::CanvasState::show(
            &mut self.canvas,
            ui,
            &mut self.document,
            &self.tool,
            &mut self.status,
            interactive,
        );
    }

    fn undo(&mut self) {
        match self.document.undo() {
            Ok(true) => {
                self.canvas.cancel_interaction();
                self.status.success("Dernière modification annulée.");
            }
            Ok(false) => self.status.info("Aucune modification à annuler."),
            Err(error) => self.status.error(error.user_message()),
        }
    }

    fn redo(&mut self) {
        match self.document.redo() {
            Ok(true) => {
                self.canvas.cancel_interaction();
                self.status.success("Modification rétablie.");
            }
            Ok(false) => self.status.info("Aucune modification à rétablir."),
            Err(error) => self.status.error(error.user_message()),
        }
    }

    fn rotate(&mut self) {
        match self.document.rotate_selection() {
            Ok(()) => self.status.success("Symbole(s) pivoté(s)."),
            Err(error) => self.status.error(error.user_message()),
        }
    }

    fn mirror(&mut self) {
        match self.document.mirror_selection() {
            Ok(()) => self.status.success("Symbole(s) mis en miroir."),
            Err(error) => self.status.error(error.user_message()),
        }
    }

    fn delete(&mut self) {
        match self.document.delete_selection() {
            Ok(()) => self.status.success("Sélection supprimée."),
            Err(error) => self.status.error(error.user_message()),
        }
    }

    fn handle_shortcuts(&mut self, context: &Context) {
        if context.egui_wants_keyboard_input() {
            return;
        }
        if self.canvas.has_dialog() {
            return;
        }
        if self.show_help {
            if context.input(|input| input.key_pressed(Key::Escape)) {
                self.show_help = false;
            }
            return;
        }

        let (command, shift, escape, delete, home) = context.input(|input| {
            (
                input.modifiers.command,
                input.modifiers.shift,
                input.key_pressed(Key::Escape),
                input.key_pressed(Key::Delete) || input.key_pressed(Key::Backspace),
                input.key_pressed(Key::Home),
            )
        });
        if command && context.input(|input| input.key_pressed(Key::Z)) {
            if shift {
                self.redo();
            } else {
                self.undo();
            }
            return;
        }
        if command && context.input(|input| input.key_pressed(Key::Y)) {
            self.redo();
            return;
        }
        if command && context.input(|input| input.key_pressed(Key::A)) {
            self.document.select_all();
            self.status.info("Tous les objets sont sélectionnés.");
            return;
        }
        if escape {
            let had_tool = self.tool != Tool::Select;
            self.canvas.cancel_interaction();
            if had_tool {
                self.tool = Tool::Select;
                self.status.info("Outil Sélection.");
            } else if !self.document.selection().is_empty() {
                self.document.clear_selection();
                self.status.info("Sélection effacée.");
            }
            return;
        }
        if delete {
            if !self.document.selection().is_empty() {
                self.delete();
            }
            return;
        }
        if home {
            self.canvas.request_fit();
            return;
        }

        if context.input(|input| input.key_pressed(Key::V)) {
            self.set_tool(Tool::Select);
        } else if context.input(|input| input.key_pressed(Key::W)) {
            self.set_tool(Tool::Wire);
        } else if context.input(|input| input.key_pressed(Key::J)) {
            self.set_tool(Tool::Junction);
        } else if context.input(|input| input.key_pressed(Key::L)) {
            self.set_tool(Tool::Label);
        } else if context.input(|input| input.key_pressed(Key::R)) {
            if selection_has_symbols(&self.document) {
                self.rotate();
            } else {
                self.status.info("Sélectionnez un symbole à pivoter.");
            }
        } else if context.input(|input| input.key_pressed(Key::M)) {
            if selection_has_symbols(&self.document) {
                self.mirror();
            } else {
                self.status
                    .info("Sélectionnez un symbole à mettre en miroir.");
            }
        }
    }

    fn help_window(&mut self, context: &Context) {
        if !self.show_help {
            return;
        }
        let mut open = self.show_help;
        egui::Window::new("Aide de CircuitSim")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(500.0)
            .show(context, |ui| {
                ui.heading("Éditer un schéma");
                ui.label(
                    "La palette choisit un composant ou un outil. Le canevas se déplace avec \
                     le bouton milieu de la souris et se zoome avec la molette.",
                );
                ui.add_space(6.0);
                ui.label(RichText::new("Souris").strong());
                ui.label("• Clic : sélectionner, placer ou ajouter un point de fil.");
                ui.label("• Glisser : déplacer la sélection courante.");
                ui.label("• Maj + clic : ajouter ou retirer un objet de la sélection.");
                ui.label("• Clic milieu + glisser : déplacer la vue.");
                ui.label("• Molette : zoom autour du curseur.");
                ui.add_space(6.0);
                ui.label(RichText::new("Clavier").strong());
                egui::Grid::new("help_shortcuts")
                    .num_columns(2)
                    .spacing([18.0, 3.0])
                    .show(ui, |ui| {
                        shortcut(ui, "V", "outil Sélection");
                        shortcut(ui, "W", "outil Fil");
                        shortcut(ui, "J", "outil Jonction");
                        shortcut(ui, "L", "outil Label");
                        shortcut(ui, "R / M", "pivoter / miroir");
                        shortcut(ui, "Suppr", "supprimer la sélection");
                        shortcut(ui, "Ctrl+Z", "annuler");
                        shortcut(ui, "Ctrl+Y", "rétablir");
                        shortcut(ui, "Ctrl+A", "tout sélectionner");
                        shortcut(ui, "Échap", "annuler l'outil ou la sélection");
                        shortcut(ui, "Home", "ajuster la vue au schéma");
                    });
                ui.add_space(6.0);
                ui.label(RichText::new("Simulation").strong());
                ui.label(
                    "Le panneau Instruments exécute le transitoire, le spectre FFT et \
                     l'analyse AC par le service worker. L'échelon de source force une source \
                     à une valeur initiale avant d'appliquer sa valeur finale.",
                );
                ui.label(
                    "Les résultats restent affichés après une modification du schéma et sont \
                     signalés comme antérieurs. Les courbes et le Bode peuvent être exportés \
                     en CSV.",
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Les commandes sont atomiques : une modification refusée ne change pas \
                         le schéma, et toute action acceptée peut être annulée.",
                    )
                    .small()
                    .color(ui::MUTED),
                );
            });
        self.show_help = open;
    }
}

impl eframe::App for CircuitApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(ui.ctx());

        egui::Panel::top("top_bar")
            .resizable(false)
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status_bar")
            .resizable(false)
            .show(ui, |ui| self.status_bar(ui));
        if self.show_instruments {
            egui::Panel::bottom("instruments_panel")
                .resizable(true)
                .default_size(324.0)
                .size_range(210.0..=560.0)
                .show(ui, |ui| {
                    self.instruments.show(ui, &self.document, &mut self.status);
                });
        }
        egui::Panel::left("palette_panel")
            .resizable(true)
            .default_size(248.0)
            .size_range(220.0..=330.0)
            .show(ui, |ui| self.palette(ui));
        egui::Panel::right("inspector_panel")
            .resizable(true)
            .default_size(335.0)
            .size_range(280.0..=470.0)
            .show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));

        self.canvas
            .show_label_dialog(ui.ctx(), &mut self.document, &mut self.status);
        self.help_window(ui.ctx());
    }
}

fn configure_style(context: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(18, 24, 35);
    visuals.window_fill = Color32::from_rgb(24, 32, 46);
    visuals.extreme_bg_color = Color32::from_rgb(13, 19, 28);
    visuals.faint_bg_color = Color32::from_rgb(28, 37, 51);
    visuals.selection.bg_fill = Color32::from_rgb(62, 103, 152);
    visuals.hyperlink_color = ui::HOVER;
    visuals.window_stroke = egui::Stroke::new(1.0, Color32::from_rgb(56, 76, 102));
    context.set_visuals(visuals);

    context.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(7.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(19.0));
    });
}

fn tool_toggle(ui: &mut Ui, tool: &mut Tool, canvas: &mut CanvasState, target: Tool, label: &str) {
    let selected = *tool == target;
    if ui.selectable_label(selected, label).clicked() && !selected {
        *tool = target;
        canvas.cancel_interaction();
    }
}

fn selection_has_symbols(document: &Document) -> bool {
    document
        .selection()
        .iter()
        .any(|object| matches!(object, ObjectRef::Symbol(_)))
}

fn shortcut(ui: &mut Ui, keys: &str, description: &str) {
    ui.label(RichText::new(keys).monospace().color(ui::ACCENT));
    ui.label(description);
    ui.end_row();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_helper_detects_selected_symbols() {
        let mut document = Document::demo().unwrap();
        assert!(!selection_has_symbols(&document));
        let id = document.schematic().symbols().next().unwrap().id;
        document.select_only(Some(ObjectRef::Symbol(id)));
        assert!(selection_has_symbols(&document));
    }
}
