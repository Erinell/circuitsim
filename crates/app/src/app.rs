//! Application shell, toolbar, shortcuts, and panel layout.

use editor_core::{GridPos, ObjectRef};
use eframe::egui::{self, Align, Color32, Context, FontId, Key, Layout, RichText, Ui};
use std::time::Duration;

use crate::document::{Alignment, DistributionAxis, Document};
use crate::ui::canvas::CanvasState;
use crate::ui::command_palette::{CommandAction, CommandPalette};
use crate::ui::history;
use crate::ui::instruments::InstrumentsState;
use crate::ui::palette::PaletteState;
use crate::ui::properties::InspectorState;
use crate::ui::{self, Status, Tool};

/// Root application state.
pub struct CircuitApp {
    document: Document,
    tool: Tool,
    filter: String,
    palette_state: PaletteState,
    canvas: CanvasState,
    inspector: InspectorState,
    instruments: InstrumentsState,
    palette: CommandPalette,
    status: Status,
    auto: AutoRefresh,
    preferences: DisplayPreferences,
    preferences_dirty: bool,
    show_help: bool,
    show_onboarding: bool,
    show_preferences: bool,
    show_palette: bool,
    show_inspector: bool,
    show_instruments: bool,
}

/// Reliable panel arrangement offered by the View menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayoutPreset {
    /// Every panel visible.
    All,
    /// Palette and inspector, without instruments.
    Editing,
    /// Instruments and inspector, without the component palette.
    Simulation,
    /// Canvas only.
    Canvas,
}

/// Visual theme available in the accessibility preferences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ThemeMode {
    /// Default dark palette.
    #[default]
    Dark,
    /// Maximum contrast for low-vision use.
    HighContrast,
}

impl ThemeMode {
    const ALL: [Self; 2] = [Self::Dark, Self::HighContrast];

    const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Sombre",
            Self::HighContrast => "Contraste élevé",
        }
    }
}

/// Session-local display preferences.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DisplayPreferences {
    theme: ThemeMode,
    scale: f32,
    reduce_motion: bool,
}

impl Default for DisplayPreferences {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            scale: 1.0,
            reduce_motion: false,
        }
    }
}

/// Supported interface scales.
const UI_SCALES: [f32; 4] = [0.85, 1.0, 1.15, 1.3];

impl LayoutPreset {
    const ALL: [Self; 4] = [Self::All, Self::Editing, Self::Simulation, Self::Canvas];

    const fn label(self) -> &'static str {
        match self {
            Self::All => "Tout",
            Self::Editing => "Édition",
            Self::Simulation => "Simulation",
            Self::Canvas => "Canevas",
        }
    }

    const fn visibility(self) -> (bool, bool, bool) {
        match self {
            Self::All => (true, true, true),
            Self::Editing => (true, true, false),
            Self::Simulation => (false, true, true),
            Self::Canvas => (false, false, false),
        }
    }
}

/// Debounce state for automatic transient refreshes.
#[derive(Clone, Debug, Default)]
struct AutoRefresh {
    due: Option<f64>,
    attempted: Option<u64>,
}

const AUTO_REFRESH_DELAY: f64 = 0.35;

impl AutoRefresh {
    fn should_run(
        &mut self,
        enabled: bool,
        busy: bool,
        document_revision: u64,
        result_revision: Option<u64>,
        now: f64,
    ) -> bool {
        if !enabled {
            self.due = None;
            self.attempted = None;
            return false;
        }
        if busy {
            return false;
        }
        if result_revision == Some(document_revision) || self.attempted == Some(document_revision) {
            self.due = None;
            return false;
        }
        let due = *self.due.get_or_insert(now + AUTO_REFRESH_DELAY);
        if now < due {
            return false;
        }
        self.due = None;
        self.attempted = Some(document_revision);
        true
    }

    fn note_manual(&mut self, document_revision: u64) {
        self.attempted = Some(document_revision);
        self.due = None;
    }

    const fn pending(&self) -> bool {
        self.due.is_some()
    }
}

impl CircuitApp {
    /// Creates the application and configures the egui style.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let preferences = DisplayPreferences::default();
        apply_style(&cc.egui_ctx, preferences);
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
            palette_state: PaletteState::default(),
            canvas: CanvasState::new(),
            inspector: InspectorState::default(),
            instruments: InstrumentsState::new(),
            palette: CommandPalette::default(),
            status,
            auto: AutoRefresh::default(),
            preferences,
            preferences_dirty: false,
            show_help: false,
            show_onboarding: true,
            show_preferences: false,
            show_palette: true,
            show_inspector: true,
            show_instruments: true,
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        if self.tool != tool {
            self.canvas.cancel_interaction();
            self.tool = tool;
        }
    }

    fn apply_layout(&mut self, layout: LayoutPreset) {
        let (palette, inspector, instruments) = layout.visibility();
        self.show_palette = palette;
        self.show_inspector = inspector;
        self.show_instruments = instruments;
    }

    fn fit_selection(&mut self) {
        let Some(bounds) = self.document.selection_bounds() else {
            self.status.info("Sélectionnez au moins un objet à cadrer.");
            return;
        };
        self.canvas.request_fit_bounds(bounds);
        self.status.success("Vue cadrée sur la sélection.");
    }

    fn simulate(&mut self, now: f64) {
        if !self.instruments.can_simulate(&self.document) {
            if self.instruments.is_busy() {
                self.status.info("Une analyse est déjà en cours.");
            } else {
                self.status
                    .error("Aucune netlist valide n'est disponible pour la simulation.");
            }
            return;
        }
        self.auto.note_manual(self.document.revision());
        self.instruments
            .run_transient_now(&self.document, now, &mut self.status);
    }

    fn tick_auto_simulation(&mut self, now: f64) {
        let enabled = self.instruments.auto_refresh();
        if self.auto.should_run(
            enabled,
            self.instruments.is_busy(),
            self.document.revision(),
            self.instruments.transient_revision(),
            now,
        ) {
            self.instruments
                .run_transient_now(&self.document, now, &mut self.status);
        }
    }

    fn load_demo(&mut self) {
        match Document::demo() {
            Ok(document) => {
                self.document = document;
                self.instruments.clear_results();
                self.canvas.cancel_interaction();
                self.canvas.request_fit();
                self.status.success("Exemple RC chargé.");
            }
            Err(error) => self.status.error(error.user_message()),
        }
    }

    fn apply_preferences(&mut self, context: &Context) {
        if !self.preferences_dirty {
            return;
        }
        apply_style(context, self.preferences);
        self.preferences_dirty = false;
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
            ui.add_enabled_ui(!self.document.selection().is_empty(), |ui| {
                if ui
                    .button("Copier")
                    .on_hover_text("Ctrl+C : copie interne")
                    .clicked()
                {
                    self.copy();
                }
                if ui
                    .button("Dupliquer")
                    .on_hover_text("Ctrl+D : copie et colle avec décalage")
                    .clicked()
                {
                    self.duplicate();
                }
            });
            ui.add_enabled_ui(self.document.clipboard().is_some(), |ui| {
                if ui
                    .button("Coller")
                    .on_hover_text("Ctrl+V : aperçu fantôme puis clic")
                    .clicked()
                {
                    self.paste();
                }
            });
            ui.add_enabled_ui(self.document.selection().len() >= 2, |ui| {
                ui.menu_button("Aligner", |ui| {
                    if ui.button("Gauche").clicked() {
                        self.align(Alignment::Left);
                        ui.close();
                    }
                    if ui.button("Centre horizontal").clicked() {
                        self.align(Alignment::CenterX);
                        ui.close();
                    }
                    if ui.button("Droite").clicked() {
                        self.align(Alignment::Right);
                        ui.close();
                    }
                    if ui.button("Haut").clicked() {
                        self.align(Alignment::Top);
                        ui.close();
                    }
                    if ui.button("Milieu vertical").clicked() {
                        self.align(Alignment::CenterY);
                        ui.close();
                    }
                    if ui.button("Bas").clicked() {
                        self.align(Alignment::Bottom);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.document.selection().len() >= 3,
                            egui::Button::new("Distribuer horizontalement"),
                        )
                        .clicked()
                    {
                        self.distribute(DistributionAxis::Horizontal);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.document.selection().len() >= 3,
                            egui::Button::new("Distribuer verticalement"),
                        )
                        .clicked()
                    {
                        self.distribute(DistributionAxis::Vertical);
                        ui.close();
                    }
                });
            });
            if ui
                .button("Ajuster")
                .on_hover_text("Home : cadrer tout le schéma")
                .clicked()
            {
                self.canvas.request_fit();
            }
            ui.add_enabled_ui(!self.document.selection().is_empty(), |ui| {
                if ui
                    .button("Cadrer sél.")
                    .on_hover_text("Ctrl+Home : cadrer la sélection")
                    .clicked()
                {
                    self.fit_selection();
                }
            });
            ui.separator();
            if ui
                .add_enabled(
                    self.instruments.can_simulate(&self.document),
                    egui::Button::new("Simuler"),
                )
                .on_hover_text("Ctrl+Entrée / F5 : relancer le transitoire")
                .clicked()
            {
                let now = ui.input(|input| input.time);
                self.simulate(now);
            }
            if self.instruments.is_busy() {
                ui.spinner();
            }
            ui.menu_button("Vue", |ui| {
                ui.checkbox(&mut self.show_palette, "Palette");
                ui.checkbox(&mut self.show_inspector, "Inspecteur");
                ui.checkbox(&mut self.show_instruments, "Instruments");
                let mut minimap = self.canvas.minimap_visible();
                if ui.checkbox(&mut minimap, "Mini-carte").changed() {
                    self.canvas.set_minimap_visible(minimap);
                }
                let mut voltages = self.canvas.net_voltages_visible();
                if ui
                    .checkbox(&mut voltages, "Tensions des nets")
                    .on_hover_text("Affiche les tensions du dernier transitoire sur le canevas")
                    .changed()
                {
                    self.canvas.set_net_voltages_visible(voltages);
                }
                ui.separator();
                ui.label(RichText::new("Disposition").small().color(ui::MUTED));
                for layout in LayoutPreset::ALL {
                    if ui.button(layout.label()).clicked() {
                        self.apply_layout(layout);
                        ui.close();
                    }
                }
                ui.separator();
                if ui
                    .button("Préférences d’affichage…")
                    .on_hover_text("Contraste, taille de l’interface et animations (Ctrl+Maj+A)")
                    .clicked()
                {
                    self.show_preferences = true;
                    ui.close();
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .small_button("−")
                        .on_hover_text("Zoom arrière (-)")
                        .clicked()
                    {
                        self.canvas.zoom_out();
                    }
                    ui.label(format!("{:.0} %", self.canvas.viewport().zoom_percent()));
                    if ui
                        .small_button("+")
                        .on_hover_text("Zoom avant (+)")
                        .clicked()
                    {
                        self.canvas.zoom_in();
                    }
                });
                if ui
                    .button("Zoom 100 %")
                    .on_hover_text("0 : zoom par défaut")
                    .clicked()
                {
                    self.canvas.reset_zoom();
                    ui.close();
                }
                if ui
                    .button("Origine")
                    .on_hover_text("Centrer la grille sur l'origine")
                    .clicked()
                {
                    self.canvas.reset_view();
                    ui.close();
                }
            });
            ui.separator();
            if ui
                .button("Démarrage")
                .on_hover_text("Guide de démarrage rapide")
                .clicked()
            {
                self.show_onboarding = true;
            }
            if ui
                .button("Accessibilité")
                .on_hover_text("Contraste, taille de l’interface et animations (Ctrl+Maj+A)")
                .clicked()
            {
                self.show_preferences = true;
            }
            if ui
                .button("Commandes")
                .on_hover_text("Palette de commandes (Ctrl+K)")
                .clicked()
            {
                self.palette.open(self.canvas.last_pointer_grid());
            }
            if ui
                .button("Aide")
                .on_hover_text("Raccourcis et fonctionnement (?)")
                .clicked()
            {
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
                    "{nets} net(s) · {components} composant(s) · {} sélectionné(s) · curseur {}",
                    self.document.selection().len(),
                    self.canvas.last_pointer_grid().map_or_else(
                        || "—".to_owned(),
                        |point| format!("({}, {})", point.x, point.y)
                    )
                ))
                .small()
                .color(ui::MUTED),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let zoom = self.canvas.viewport().zoom_percent();
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
            &mut self.palette_state,
            &mut self.tool,
            &mut self.canvas,
        );
    }

    fn inspector(&mut self, ui: &mut Ui) {
        let live = self.instruments.live_values();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui::properties::show(
                    ui,
                    &mut self.document,
                    &mut self.inspector,
                    &mut self.status,
                    &live,
                );
                history::show(ui, &mut self.document, &mut self.status);
                ui::erc::show(ui, &mut self.document, &mut self.status);
            });
    }

    fn canvas(&mut self, ui: &mut Ui) {
        let interactive = !self.canvas.has_dialog()
            && !self.show_help
            && !self.show_onboarding
            && !self.show_preferences
            && !self.palette.is_open();
        let live = self.instruments.live_values();
        ui::canvas::CanvasState::show(
            &mut self.canvas,
            ui,
            &mut self.document,
            &self.tool,
            &mut self.status,
            interactive,
            &live,
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

    fn copy(&mut self) {
        if self.document.selection().is_empty() {
            self.status.info("Sélectionnez au moins un objet à copier.");
            return;
        }
        match self.document.copy_selection() {
            Ok(count) => self.status.success(format!("{count} objet(s) copié(s).")),
            Err(error) => self.status.error(error.user_message()),
        }
    }

    fn paste(&mut self) {
        if self.document.clipboard().is_none() {
            self.status.info("Le presse-papiers est vide.");
            return;
        }
        self.canvas.begin_paste();
        self.status
            .info("Cliquez sur le canevas pour coller. Échap annule.");
    }

    fn duplicate(&mut self) {
        if self.document.selection().is_empty() {
            self.status
                .info("Sélectionnez au moins un objet à dupliquer.");
            return;
        }
        match self.document.duplicate_selection() {
            Ok(count) => self
                .status
                .success(format!("{count} objet(s) dupliqué(s).")),
            Err(error) => self.status.error(error.user_message()),
        }
    }

    fn align(&mut self, alignment: Alignment) {
        match self.document.align_selection(alignment) {
            Ok(()) => self.status.success("Sélection alignée."),
            Err(error) => self.status.error(error.user_message()),
        }
    }

    fn distribute(&mut self, axis: DistributionAxis) {
        match self.document.distribute_selection(axis) {
            Ok(()) => self.status.success("Sélection distribuée."),
            Err(error) => self.status.error(error.user_message()),
        }
    }

    fn apply_command(&mut self, command: CommandAction) {
        match command {
            CommandAction::Place(kind) => {
                if let Some(point) = self.palette.anchor() {
                    if let Some(editor_core::ObjectRef::Wire(wire)) = self.document.object_at(point)
                    {
                        if self
                            .document
                            .registry()
                            .get(&kind)
                            .is_some_and(|definition| definition.pins.len() == 2)
                        {
                            match self.document.insert_symbol_on_wire(kind, wire, point) {
                                Ok(_) => self.status.success("Composant inséré en série."),
                                Err(error) => self.status.error(error.user_message()),
                            }
                            return;
                        }
                    }
                    match self.document.add_symbol_oriented(
                        kind,
                        point,
                        self.canvas.placement_orientation(),
                        self.canvas.placement_mirrored(),
                    ) {
                        Ok(_) => self.status.success("Composant placé au curseur."),
                        Err(error) => self.status.error(error.user_message()),
                    }
                } else {
                    self.set_tool(Tool::Place(kind));
                    self.status.info("Placez le composant sur le canevas.");
                }
            }
            CommandAction::Tool(tool) => self.set_tool(tool),
            CommandAction::Fit => self.canvas.request_fit(),
            CommandAction::Undo => self.undo(),
            CommandAction::Redo => self.redo(),
            CommandAction::Delete => self.delete(),
            CommandAction::Copy => self.copy(),
            CommandAction::Paste => self.paste(),
            CommandAction::Duplicate => self.duplicate(),
            CommandAction::SelectAll => {
                self.document.select_all();
                self.status.info("Tous les objets sont sélectionnés.");
            }
            CommandAction::Rotate => self.rotate(),
            CommandAction::Mirror => self.mirror(),
            CommandAction::Align(alignment) => self.align(alignment),
            CommandAction::Distribute(axis) => self.distribute(axis),
            CommandAction::New => match self.document.clear() {
                Ok(()) => {
                    self.canvas.cancel_interaction();
                    self.canvas.request_fit();
                    self.status
                        .success("Schéma effacé. Ctrl+Z permet de le restaurer.");
                }
                Err(error) => self.status.error(error.user_message()),
            },
            CommandAction::Instruments => self.show_instruments = !self.show_instruments,
            CommandAction::Onboarding => self.show_onboarding = true,
            CommandAction::LoadDemo => self.load_demo(),
            CommandAction::Preferences => self.show_preferences = true,
            CommandAction::Help => self.show_help = true,
        }
    }

    fn handle_shortcuts(&mut self, context: &Context) {
        if context.input(|input| input.modifiers.command && input.key_pressed(Key::K)) {
            self.palette.toggle(self.canvas.last_pointer_grid());
            self.canvas.cancel_interaction();
            return;
        }
        if self.palette.is_open() {
            return;
        }
        if (self.show_onboarding || self.show_preferences)
            && context.input(|input| input.key_pressed(Key::Escape))
        {
            self.show_onboarding = false;
            self.show_preferences = false;
            return;
        }
        if context.egui_wants_keyboard_input() {
            return;
        }
        if self.canvas.has_dialog() {
            return;
        }
        if self.show_help {
            if context.input(|input| {
                input.key_pressed(Key::Escape)
                    || input.key_pressed(Key::F1)
                    || input.key_pressed(Key::Questionmark)
                    || (input.modifiers.shift && input.key_pressed(Key::Slash))
            }) {
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
        if command && shift && context.input(|input| input.key_pressed(Key::A)) {
            self.show_preferences = true;
            return;
        }
        if command && context.input(|input| input.key_pressed(Key::A)) {
            self.document.select_all();
            self.status.info("Tous les objets sont sélectionnés.");
            return;
        }
        if command && context.input(|input| input.key_pressed(Key::C)) {
            self.copy();
            return;
        }
        if command && context.input(|input| input.key_pressed(Key::V)) {
            self.paste();
            return;
        }
        if command && context.input(|input| input.key_pressed(Key::D)) {
            self.duplicate();
            return;
        }
        for (key, layout) in [
            (Key::Num1, LayoutPreset::All),
            (Key::Num2, LayoutPreset::Editing),
            (Key::Num3, LayoutPreset::Simulation),
            (Key::Num4, LayoutPreset::Canvas),
        ] {
            if command && context.input(|input| input.key_pressed(key)) {
                self.apply_layout(layout);
                self.status
                    .info(format!("Disposition : {}.", layout.label()));
                return;
            }
        }
        if command && context.input(|input| input.key_pressed(Key::Home)) {
            self.fit_selection();
            return;
        }
        if command && context.input(|input| input.key_pressed(Key::Num0)) {
            self.canvas.reset_view();
            self.status.info("Vue ramenée à l'origine.");
            return;
        }
        if !command
            && context.input(|input| input.key_pressed(Key::Plus) || input.key_pressed(Key::Equals))
        {
            self.canvas.zoom_in();
            return;
        }
        if !command && context.input(|input| input.key_pressed(Key::Minus)) {
            self.canvas.zoom_out();
            return;
        }
        if !command && context.input(|input| input.key_pressed(Key::Num0)) {
            self.canvas.reset_zoom();
            return;
        }
        if context.input(|input| {
            input.key_pressed(Key::F5) || (input.modifiers.command && input.key_pressed(Key::Enter))
        }) {
            let now = context.input(|input| input.time);
            self.simulate(now);
            return;
        }
        let (pan_left, pan_right, pan_up, pan_down, alt) = context.input(|input| {
            (
                input.key_pressed(Key::ArrowLeft),
                input.key_pressed(Key::ArrowRight),
                input.key_pressed(Key::ArrowUp),
                input.key_pressed(Key::ArrowDown),
                input.modifiers.alt,
            )
        });
        if (pan_left || pan_right || pan_up || pan_down) && !command && !alt {
            let step = if shift { 8 } else { 2 };
            let delta = GridPos::new(
                if pan_left {
                    step
                } else if pan_right {
                    -step
                } else {
                    0
                },
                if pan_up {
                    step
                } else if pan_down {
                    -step
                } else {
                    0
                },
            );
            self.canvas.pan_by_grid(delta);
            return;
        }
        if context.input(|input| {
            input.key_pressed(Key::Questionmark)
                || input.key_pressed(Key::F1)
                || (input.modifiers.shift && input.key_pressed(Key::Slash))
        }) {
            self.show_help = true;
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
        } else if context.input(|input| input.key_pressed(Key::R))
            && !matches!(self.tool, Tool::Place(_))
        {
            if selection_has_symbols(&self.document) {
                self.rotate();
            } else {
                self.status.info("Sélectionnez un symbole à pivoter.");
            }
        } else if context.input(|input| input.key_pressed(Key::M))
            && !matches!(self.tool, Tool::Place(_))
        {
            if selection_has_symbols(&self.document) {
                self.mirror();
            } else {
                self.status
                    .info("Sélectionnez un symbole à mettre en miroir.");
            }
        }
    }

    fn onboarding_window(&mut self, context: &Context) {
        if !self.show_onboarding {
            return;
        }
        let mut open = self.show_onboarding;
        let mut load_demo = false;
        let mut empty = false;
        let mut open_help = false;
        let mut close = false;
        egui::Window::new("Démarrage rapide")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .show(context, |ui| {
                ui.heading("Bienvenue dans CircuitSim");
                ui.label(
                    "Quatre gestes suffisent pour placer, câbler, simuler et explorer un \
                     circuit analogique.",
                );
                ui.add_space(8.0);
                for (index, title, detail) in [
                    (
                        1,
                        "Placer",
                        "Choisissez un composant dans la palette ou appuyez sur Ctrl+K. \
                         R et F orientent l’aperçu.",
                    ),
                    (
                        2,
                        "Câbler",
                        "Appuyez sur W, partez d’une broche ou d’un fil, puis cliquez \
                         l’extrémité. Maj change le coude.",
                    ),
                    (
                        3,
                        "Simuler",
                        "Ctrl+Entrée lance le transitoire. Les tensions des nets \
                         apparaissent sur le canevas.",
                    ),
                    (
                        4,
                        "Explorer",
                        "Sélectionnez un objet pour l’inspecteur, utilisez deux curseurs \
                         dans la portée et ? pour l’aide.",
                    ),
                ] {
                    ui.label(
                        RichText::new(format!("{index}. {title}"))
                            .strong()
                            .color(ui::ACCENT),
                    );
                    ui.label(RichText::new(detail).small().color(ui::MUTED));
                    ui.add_space(3.0);
                }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button("Charger l’exemple RC")
                        .on_hover_text("Remplace le schéma par le circuit RC de démonstration")
                        .clicked()
                    {
                        load_demo = true;
                    }
                    if ui
                        .button("Schéma vide")
                        .on_hover_text("Efface le schéma, annulable avec Ctrl+Z")
                        .clicked()
                    {
                        empty = true;
                    }
                    if ui.button("Aide complète").clicked() {
                        open_help = true;
                    }
                    if ui.button("Fermer").clicked() {
                        close = true;
                    }
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Le guide reste accessible par le bouton Démarrage et la palette \
                         de commandes.",
                    )
                    .small()
                    .color(ui::MUTED),
                );
            });
        if load_demo {
            self.load_demo();
            open = false;
        }
        if close {
            open = false;
        }
        if empty {
            match self.document.clear() {
                Ok(()) => {
                    self.instruments.clear_results();
                    self.canvas.cancel_interaction();
                    self.canvas.request_fit();
                    self.status
                        .success("Schéma vide. Utilisez la palette pour commencer.");
                }
                Err(error) => self.status.error(error.user_message()),
            }
            open = false;
        }
        if open_help {
            self.show_help = true;
            open = false;
        }
        self.show_onboarding = open;
    }

    fn preferences_window(&mut self, context: &Context) {
        if !self.show_preferences {
            return;
        }
        let mut open = self.show_preferences;
        let mut changed = false;
        egui::Window::new("Préférences d’affichage")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .show(context, |ui| {
                ui.heading("Accessibilité");
                ui.label(
                    RichText::new("Réglages appliqués à cette session.")
                        .small()
                        .color(ui::MUTED),
                );
                ui.separator();
                ui.label(RichText::new("Thème").strong());
                ui.horizontal(|ui| {
                    for theme in ThemeMode::ALL {
                        if ui
                            .radio_value(&mut self.preferences.theme, theme, theme.label())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Taille de l’interface").strong());
                egui::ComboBox::from_id_salt("ui_scale")
                    .selected_text(format!("{:.0} %", self.preferences.scale * 100.0))
                    .show_ui(ui, |ui| {
                        for scale in UI_SCALES {
                            if ui
                                .selectable_value(
                                    &mut self.preferences.scale,
                                    scale,
                                    format!("{:.0} %", scale * 100.0),
                                )
                                .changed()
                            {
                                changed = true;
                            }
                        }
                    });
                ui.add_space(6.0);
                if ui
                    .checkbox(
                        &mut self.preferences.reduce_motion,
                        "Réduire les animations",
                    )
                    .on_hover_text("Désactive les transitions et animations de l’interface")
                    .changed()
                {
                    changed = true;
                }
                ui.add_space(6.0);
                ui.separator();
                ui.label(
                    RichText::new(
                        "Clavier : Tab et Maj+Tab parcourent les contrôles, Entrée active, \
                         Échap ferme les fenêtres. Ctrl+K ouvre toutes les commandes.",
                    )
                    .small()
                    .color(ui::MUTED),
                );
                ui.add_space(6.0);
                if ui.button("Réinitialiser").clicked() {
                    self.preferences = DisplayPreferences::default();
                    changed = true;
                }
            });
        if changed {
            self.preferences_dirty = true;
        }
        self.show_preferences = open;
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
                ui.label(
                    RichText::new(
                        "Le bouton Démarrage rouvre le guide de quatre étapes. \
                         Ctrl+Maj+A ouvre les préférences d’affichage.",
                    )
                    .small()
                    .color(ui::MUTED),
                );
                ui.add_space(6.0);
                ui.label(RichText::new("Souris").strong());
                ui.label("• Clic : sélectionner, placer ou ajouter un point de fil.");
                ui.label("• Glisser : déplacer la sélection courante.");
                ui.label("• Maj + clic : ajouter ou retirer un objet de la sélection.");
                ui.label("• Ctrl + clic : inverser un objet dans la sélection.");
                ui.label("• Rectangle gauche→droite : objets entièrement inclus.");
                ui.label("• Rectangle droite→gauche : objets touchés.");
                ui.label("• Clic milieu + glisser : déplacer la vue.");
                ui.label("• Molette : zoom autour du curseur.");
                ui.label("• Mini-carte en bas à droite : cliquer pour recentrer la vue.");
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
                        shortcut(ui, "R / F en pose", "tourner / miroir de l’aperçu");
                        shortcut(ui, "Ctrl+K", "palette de commandes");
                        shortcut(ui, "Alt+clic", "sélectionner tout le net");
                        shortcut(ui, "Ctrl+C / Ctrl+V", "copier / coller avec aperçu");
                        shortcut(ui, "Ctrl+D", "dupliquer la sélection");
                        shortcut(ui, "Ctrl+Entrée / F5", "relancer le transitoire");
                        shortcut(ui, "Suppr", "supprimer la sélection");
                        shortcut(ui, "Ctrl+Z", "annuler");
                        shortcut(ui, "Ctrl+Y", "rétablir");
                        shortcut(ui, "Ctrl+A", "tout sélectionner");
                        shortcut(ui, "Échap", "annuler l'outil, le collage ou la sélection");
                        shortcut(ui, "? / F1", "ouvrir ou fermer cette aide");
                        shortcut(ui, "Ctrl+Maj+A", "préférences d’affichage et accessibilité");
                        shortcut(ui, "Home", "ajuster la vue au schéma");
                        shortcut(ui, "Ctrl+Home", "cadrer la sélection");
                        shortcut(ui, "+ / -", "zoom avant / arrière");
                        shortcut(ui, "0", "zoom 100 %");
                        shortcut(ui, "Flèches", "déplacer la vue (Maj : pas rapide)");
                        shortcut(ui, "Ctrl+1…4", "Tout, Édition, Simulation, Canevas");
                    });
                ui.add_space(6.0);
                ui.label(RichText::new("Simulation").strong());
                ui.label(
                    "Le panneau Instruments exécute le transitoire, le spectre FFT et \
                     l'analyse AC par le service worker. L'échelon de source force une source \
                     à une valeur initiale avant d'appliquer sa valeur finale.",
                );
                ui.label(
                    "Simuler (Ctrl+Entrée ou F5) relance le transitoire ; Auto recalcule après \
                     une modification et superpose les tensions des nets sur le canevas.",
                );
                ui.label(
                    "Dans la portée, deux clics placent les curseurs A et B ; les écarts et \
                     mesures d’intervalle s’affichent à gauche. Un clic sur le Bode pose un \
                     marqueur de fréquence.",
                );
                ui.label(
                    "Sur le canevas, Ctrl+K ouvre la palette de commandes. R et F orientent \
                     l’aperçu de placement ; Maj change le coude d’un fil. Alt+clic sélectionne \
                     le net complet et un composant déposé sur un fil s’insère en série.",
                );
                ui.label(
                    "L’inspecteur affiche les valeurs simulées du dernier échantillon \
                     transitoire. Les paramètres acceptent 4k7 ou 100n ; la molette et les \
                     boutons −/+ parcourent la série E12, E24 ou E96 choisie.",
                );
                ui.label(
                    "Le panneau Historique, dans l’inspecteur, ramène le schéma à un état \
                     nommé. Un clic droit sur le canevas propose les actions de l’objet.",
                );
                ui.label(
                    "Le menu Vue masque les panneaux, choisit une disposition et règle le \
                     zoom. La mini-carte et les flèches déplacent la vue sans toucher au schéma.",
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
        self.apply_preferences(ui.ctx());
        self.handle_shortcuts(ui.ctx());
        let now = ui.input(|input| input.time);
        self.tick_auto_simulation(now);
        if self.instruments.auto_refresh() && (self.auto.pending() || self.instruments.is_busy()) {
            ui.ctx().request_repaint_after(Duration::from_millis(40));
        }

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
        if self.show_palette {
            egui::Panel::left("palette_panel")
                .resizable(true)
                .default_size(248.0)
                .size_range(220.0..=330.0)
                .show(ui, |ui| self.palette(ui));
        }
        if self.show_inspector {
            egui::Panel::right("inspector_panel")
                .resizable(true)
                .default_size(335.0)
                .size_range(280.0..=470.0)
                .show(ui, |ui| self.inspector(ui));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));

        self.canvas
            .show_label_dialog(ui.ctx(), &mut self.document, &mut self.status);
        if let Some(command) = self.palette.show(ui.ctx(), &self.document) {
            self.apply_command(command);
        }
        self.help_window(ui.ctx());
        self.onboarding_window(ui.ctx());
        self.preferences_window(ui.ctx());
    }
}

fn apply_style(context: &Context, preferences: DisplayPreferences) {
    let visuals = match preferences.theme {
        ThemeMode::Dark => dark_visuals(),
        ThemeMode::HighContrast => high_contrast_visuals(),
    };
    context.set_visuals(visuals);
    context.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(7.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.animation_time = if preferences.reduce_motion { 0.0 } else { 0.2 };
        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(19.0));
    });
    context.set_zoom_factor(preferences.scale);
}

fn dark_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(18, 24, 35);
    visuals.window_fill = Color32::from_rgb(24, 32, 46);
    visuals.extreme_bg_color = Color32::from_rgb(13, 19, 28);
    visuals.faint_bg_color = Color32::from_rgb(28, 37, 51);
    visuals.selection.bg_fill = Color32::from_rgb(62, 103, 152);
    visuals.hyperlink_color = ui::HOVER;
    visuals.window_stroke = egui::Stroke::new(1.0, Color32::from_rgb(56, 76, 102));
    visuals
}

fn high_contrast_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(Color32::WHITE);
    visuals.panel_fill = Color32::BLACK;
    visuals.window_fill = Color32::from_rgb(8, 8, 8);
    visuals.extreme_bg_color = Color32::BLACK;
    visuals.faint_bg_color = Color32::from_rgb(26, 26, 26);
    visuals.selection.bg_fill = Color32::from_rgb(0, 95, 215);
    visuals.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
    visuals.hyperlink_color = Color32::from_rgb(120, 220, 255);
    visuals.window_stroke = egui::Stroke::new(1.5, Color32::WHITE);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.2, Color32::WHITE);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.2, Color32::WHITE);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.5, Color32::WHITE);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, Color32::WHITE);
    visuals
}

fn tool_toggle(ui: &mut Ui, tool: &mut Tool, canvas: &mut CanvasState, target: Tool, label: &str) {
    let selected = *tool == target;
    let shortcut = match label {
        "Sélection" => "V",
        "Fil" => "W",
        "Jonction" => "J",
        "Label" => "L",
        _ => "",
    };
    let response = ui
        .selectable_label(selected, label)
        .on_hover_text(format!("{label} ({shortcut})"));
    if response.clicked() && !selected {
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

    #[test]
    fn layout_presets_cover_every_panel_combination() {
        assert_eq!(LayoutPreset::All.visibility(), (true, true, true));
        assert_eq!(LayoutPreset::Editing.visibility(), (true, true, false));
        assert_eq!(LayoutPreset::Simulation.visibility(), (false, true, true));
        assert_eq!(LayoutPreset::Canvas.visibility(), (false, false, false));
        assert_eq!(LayoutPreset::ALL.len(), 4);
    }

    #[test]
    fn display_preferences_have_safe_defaults_and_scales() {
        let preferences = DisplayPreferences::default();
        assert_eq!(preferences.theme, ThemeMode::Dark);
        assert_eq!(preferences.scale, 1.0);
        assert!(!preferences.reduce_motion);
        assert!(UI_SCALES.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(ThemeMode::HighContrast.label(), "Contraste élevé");
    }

    #[test]
    fn auto_refresh_debounces_and_tracks_the_document_revision() {
        let mut auto = AutoRefresh::default();
        assert!(!auto.should_run(false, false, 3, None, 0.0));
        assert!(!auto.should_run(true, false, 3, None, 0.0));
        assert!(auto.pending());
        assert!(!auto.should_run(true, false, 3, None, 0.2));
        assert!(auto.should_run(true, false, 3, None, 0.4));
        assert!(!auto.should_run(true, false, 3, None, 1.0));
        assert!(!auto.should_run(true, true, 4, None, 1.0));
        assert!(!auto.should_run(true, false, 4, None, 1.0));
        assert!(auto.should_run(true, false, 4, None, 1.4));
        auto.note_manual(5);
        assert!(!auto.should_run(true, false, 5, None, 2.0));
        assert!(!auto.should_run(true, false, 5, Some(5), 3.0));
        assert!(!auto.should_run(true, false, 6, Some(6), 4.0));
    }
}
