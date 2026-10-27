//! Application shell, toolbar, shortcuts, and panel layout.

use editor_core::{GridPos, ObjectRef};
use eframe::egui::{
    self, Align, Align2, Color32, Context, FontId, Key, Layout, RichText, Ui, Vec2,
};
use std::collections::BTreeSet;
use std::time::Duration;

use crate::analysis::probe_readings;

use crate::document::{Alignment, DistributionAxis, Document};
use crate::preferences::PersistedPreferences;
use crate::ui::canvas::CanvasState;
use crate::ui::command_palette::{CommandAction, CommandPalette};
use crate::ui::history;
use crate::ui::instruments::InstrumentsState;
use crate::ui::palette::PaletteState;
use crate::ui::project::ProjectDialog;
use crate::ui::properties::{InspectorState, SymbolDialog};
use crate::ui::scopes::InlineScopes;
use crate::ui::shortcuts::SHORTCUTS;
use crate::ui::theme::CanvasTheme;
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
    scopes: InlineScopes,
    palette: CommandPalette,
    symbol_dialog: Option<SymbolDialog>,
    project_dialog: ProjectDialog,
    status: Status,
    auto: AutoRefresh,
    preferences: DisplayPreferences,
    preferences_dirty: bool,
    last_persisted: Option<PersistedPreferences>,
    last_persist_time: f64,
    persist_error: bool,
    show_help: bool,
    show_onboarding: bool,
    show_preferences: bool,
    show_about: bool,
    show_palette: bool,
    show_inspector: bool,
    show_instruments: bool,
    live_autostart: bool,
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
    /// Light background for daylight use.
    Light,
}

impl ThemeMode {
    const ALL: [Self; 3] = [Self::Dark, Self::HighContrast, Self::Light];

    const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Sombre",
            Self::HighContrast => "Contraste élevé",
            Self::Light => "Clair",
        }
    }

    const fn canvas_theme(self) -> CanvasTheme {
        match self {
            Self::Dark => CanvasTheme::Dark,
            Self::HighContrast => CanvasTheme::HighContrast,
            Self::Light => CanvasTheme::Light,
        }
    }

    const fn from_canvas(theme: CanvasTheme) -> Self {
        match theme {
            CanvasTheme::Dark => Self::Dark,
            CanvasTheme::HighContrast => Self::HighContrast,
            CanvasTheme::Light => Self::Light,
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
        let stored = crate::preferences::load();
        let mut preferences = DisplayPreferences::default();
        if let Some(stored) = &stored {
            preferences.theme = ThemeMode::from_canvas(stored.canvas.theme);
            if UI_SCALES.contains(&stored.scale) {
                preferences.scale = stored.scale;
            }
            preferences.reduce_motion = stored.reduce_motion;
        }
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
        let mut canvas = CanvasState::new();
        let mut palette_state = PaletteState::default();
        let mut instruments = InstrumentsState::new();
        let mut scopes = InlineScopes::with_signals(instruments.selected_signals());
        let mut show_palette = true;
        let mut show_inspector = true;
        let mut show_instruments = false;
        if let Some(stored) = &stored {
            *canvas.settings_mut() = stored.canvas;
            canvas.set_theme(stored.canvas.theme);
            palette_state.set_favorites(stored.favorites.iter().cloned());
            instruments.set_realtime_speed(stored.realtime_speed);
            instruments.set_animation_speed(stored.animation_speed);
            instruments.set_power_brightness(stored.power_brightness);
            scopes.set_visible(stored.show_inline_scopes);
            show_palette = stored.show_palette;
            show_inspector = stored.show_inspector;
            show_instruments = stored.show_instruments;
        }
        Self {
            document,
            tool: Tool::Select,
            filter: String::new(),
            palette_state,
            canvas,
            inspector: InspectorState::default(),
            instruments,
            scopes,
            palette: CommandPalette::default(),
            symbol_dialog: None,
            project_dialog: ProjectDialog::default(),
            status,
            auto: AutoRefresh::default(),
            preferences,
            preferences_dirty: false,
            last_persisted: stored,
            last_persist_time: 0.0,
            persist_error: false,
            show_help: false,
            show_onboarding: true,
            show_preferences: false,
            show_about: false,
            show_palette,
            show_inspector,
            show_instruments,
            live_autostart: true,
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

    fn toggle_live(&mut self, now: f64) {
        if self.instruments.realtime_active() {
            let paused = self.instruments.realtime_paused();
            self.instruments.set_realtime_paused(!paused);
            self.status.info(if paused {
                "Simulation reprise."
            } else {
                "Simulation en pause."
            });
            return;
        }
        if !self.instruments.can_simulate(&self.document) {
            self.status
                .error("Aucune netlist valide n'est disponible pour la simulation.");
            return;
        }
        self.auto.note_manual(self.document.revision());
        self.instruments
            .start_realtime(&self.document, now, &mut self.status);
    }

    fn reset_live(&mut self, now: f64) {
        if !self.instruments.realtime_active() {
            if !self.instruments.can_simulate(&self.document) {
                self.status
                    .error("Aucune netlist valide n'est disponible pour la simulation.");
                return;
            }
            self.instruments
                .start_realtime(&self.document, now, &mut self.status);
            return;
        }
        self.auto.note_manual(self.document.revision());
        self.instruments
            .reset_realtime(&self.document, now, &mut self.status);
    }

    fn step_live(&mut self) {
        self.instruments
            .step_realtime(&self.document, &mut self.status);
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

    fn stored_preferences(&self) -> PersistedPreferences {
        PersistedPreferences {
            version: crate::preferences::PREFERENCES_VERSION,
            scale: self.preferences.scale,
            reduce_motion: self.preferences.reduce_motion,
            canvas: *self.canvas.settings(),
            show_palette: self.show_palette,
            show_inspector: self.show_inspector,
            show_instruments: self.show_instruments,
            show_inline_scopes: self.scopes.visible(),
            realtime_speed: self.instruments.realtime_speed(),
            animation_speed: self.instruments.animation_speed(),
            power_brightness: self.instruments.power_brightness(),
            favorites: self.palette_state.favorites().collect(),
        }
    }

    fn persist_if_due(&mut self, now: f64) {
        let current = self.stored_preferences();
        if self.last_persisted.as_ref() == Some(&current) {
            return;
        }
        if self.last_persisted.is_some() && now - self.last_persist_time < 0.6 {
            return;
        }
        match crate::preferences::save(&current) {
            Ok(()) => {
                self.last_persisted = Some(current);
                self.last_persist_time = now;
                self.persist_error = false;
            }
            Err(error) => {
                if !self.persist_error {
                    self.status
                        .error(format!("Préférences non enregistrées : {error}"));
                    self.persist_error = true;
                }
            }
        }
    }

    fn load_demo(&mut self) {
        match Document::demo() {
            Ok(document) => self.load_example(document, "RC"),
            Err(error) => self.status.error(error.user_message()),
        }
    }

    fn load_example(&mut self, document: Document, label: &str) {
        self.document = document;
        self.instruments.clear_results();
        self.scopes = InlineScopes::with_signals(BTreeSet::new());
        self.canvas.cancel_interaction();
        self.canvas.request_fit();
        self.live_autostart = true;
        self.status.success(format!("Exemple {label} chargé."));
    }

    fn apply_preferences(&mut self, context: &Context) {
        if !self.preferences_dirty {
            return;
        }
        apply_style(context, self.preferences);
        self.preferences_dirty = false;
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        ui.menu_button("Fichier", |ui| {
            if ui.button("Nouveau").clicked() {
                match self.document.clear() {
                    Ok(()) => {
                        self.canvas.request_fit();
                        self.status.success("Schéma effacé.");
                    }
                    Err(error) => self.status.error(error.user_message()),
                }
                ui.close();
            }
            if ui.button("Projet…").clicked() {
                self.project_dialog.open_with(&self.document);
                ui.close();
            }
            ui.separator();
            ui.label(RichText::new("Exemples").small().color(ui::MUTED));
            for (label, builder) in [
                (
                    "RC",
                    Document::demo as fn() -> Result<Document, crate::document::ActionError>,
                ),
                ("Diviseur", Document::divider_example),
                ("LED", Document::led_example),
                ("Interrupteur", Document::switch_example),
                ("RLC", Document::rlc_example),
            ] {
                if ui.button(label).clicked() {
                    match builder() {
                        Ok(document) => self.load_example(document, label),
                        Err(error) => self.status.error(error.user_message()),
                    }
                    ui.close();
                }
            }
        });
        ui.menu_button("Édition", |ui| {
            if ui.button("Annuler").clicked() {
                self.undo();
                ui.close();
            }
            if ui.button("Rétablir").clicked() {
                self.redo();
                ui.close();
            }
            ui.separator();
            if ui.button("Copier").clicked() {
                self.copy();
                ui.close();
            }
            if ui.button("Coller").clicked() {
                self.paste();
                ui.close();
            }
            if ui.button("Dupliquer").clicked() {
                self.duplicate();
                ui.close();
            }
            if ui.button("Supprimer").clicked() {
                self.delete();
                ui.close();
            }
        });
        ui.menu_button("Dessin", |ui| {
            for (label, tool) in [
                ("Sélection", Tool::Select),
                ("Fil", Tool::Wire),
                ("Jonction", Tool::Junction),
                ("Label", Tool::Label),
            ] {
                if ui.selectable_label(self.tool == tool, label).clicked() {
                    self.set_tool(tool);
                    ui.close();
                }
            }
            ui.separator();
            for kind in self.compact_symbols() {
                let label = ui::symbol_kind_name(&kind).to_owned();
                if ui.button(label).clicked() {
                    self.set_tool(Tool::Place(kind));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Pivoter").clicked() {
                self.rotate();
                ui.close();
            }
            if ui.button("Miroir").clicked() {
                self.mirror();
                ui.close();
            }
        });
        ui.menu_button("Afficher", |ui| {
            ui.checkbox(&mut self.show_palette, "Palette");
            ui.checkbox(&mut self.show_inspector, "Inspecteur");
            ui.checkbox(&mut self.show_instruments, "Panneau d'instruments");
            let mut scopes = self.scopes.visible();
            if ui.checkbox(&mut scopes, "Portées incrustées").changed() {
                self.scopes.set_visible(scopes);
            }
            let mut minimap = self.canvas.minimap_visible();
            if ui.checkbox(&mut minimap, "Mini-carte").changed() {
                self.canvas.set_minimap_visible(minimap);
            }
            let mut flow = self.canvas.current_flow_visible();
            if ui.checkbox(&mut flow, "Courant animé").changed() {
                self.canvas.set_current_flow_visible(flow);
            }
            ui.separator();
            let settings = self.canvas.settings_mut();
            ui.checkbox(&mut settings.layers.voltage, "Tension des fils");
            ui.checkbox(&mut settings.layers.current, "Courant");
            ui.checkbox(&mut settings.layers.power, "Puissance");
            ui.checkbox(&mut settings.layers.values, "Valeurs");
            ui.checkbox(&mut settings.layers.references, "Références");
            ui.checkbox(&mut settings.layers.net_labels, "Labels de nets");
            ui.checkbox(&mut settings.small_grid, "Petite grille");
            ui.separator();
            for layout in LayoutPreset::ALL {
                if ui.button(layout.label()).clicked() {
                    self.apply_layout(layout);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Ajuster la vue").clicked() {
                self.canvas.request_fit();
                ui.close();
            }
            if ui.button("Cadrer la sélection").clicked() {
                self.fit_selection();
                ui.close();
            }
            ui.horizontal(|ui| {
                if ui.small_button("−").clicked() {
                    self.canvas.zoom_out();
                }
                ui.label(format!("{:.0} %", self.canvas.viewport().zoom_percent()));
                if ui.small_button("+").clicked() {
                    self.canvas.zoom_in();
                }
            });
            if ui.button("Zoom 100 %").clicked() {
                self.canvas.reset_zoom();
                ui.close();
            }
            if ui.button("Origine").clicked() {
                self.canvas.reset_view();
                ui.close();
            }
        });
        ui.menu_button("Options", |ui| {
            if ui.button("Préférences d'affichage…").clicked() {
                self.show_preferences = true;
                ui.close();
            }
        });
        ui.menu_button("Aide", |ui| {
            if ui.button("Aide et raccourcis").clicked() {
                self.show_help = true;
                ui.close();
            }
            if ui.button("Démarrage").clicked() {
                self.show_onboarding = true;
                ui.close();
            }
            if ui.button("À propos").clicked() {
                self.show_about = true;
                ui.close();
            }
        });
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
            self.menu_bar(ui);
            ui.separator();

            let now = ui.input(|input| input.time);
            let running = self.instruments.realtime_active();
            let paused = self.instruments.realtime_paused();
            if ui
                .add_enabled(
                    running || self.instruments.can_simulate(&self.document),
                    egui::Button::new(if running && !paused {
                        "Pause"
                    } else {
                        "Marche"
                    }),
                )
                .on_hover_text("Espace : marche/arrêt de la simulation continue")
                .clicked()
            {
                self.toggle_live(now);
            }
            if ui
                .add_enabled(running, egui::Button::new("Reset"))
                .on_hover_text("F5 : remet la session à t = 0")
                .clicked()
            {
                self.reset_live(now);
            }
            if ui
                .add_enabled(running && paused, egui::Button::new("Pas"))
                .on_hover_text("Avance d'un pas lorsque la simulation est en pause")
                .clicked()
            {
                self.step_live();
            }
            if self.instruments.live_lagging() {
                ui.label(
                    RichText::new(format!("trop lent (×{:.1})", self.instruments.live_ratio()))
                        .small()
                        .color(ui::ACCENT),
                )
                .on_hover_text("Le moteur ne suit pas la vitesse demandée");
            }
            if let Some(error) = self.instruments.live_error() {
                ui.label(RichText::new("Circuit invalide").color(Color32::from_rgb(240, 128, 128)))
                    .on_hover_text(error.to_owned());
            }
            if self.instruments.is_busy() {
                ui.spinner();
            }
        });
        ui.add_space(2.0);
    }

    fn compact_symbols(&self) -> Vec<editor_core::SymbolKind> {
        const DEFAULTS: [editor_core::SymbolKind; 9] = [
            editor_core::SymbolKind::Resistor,
            editor_core::SymbolKind::Capacitor,
            editor_core::SymbolKind::Inductor,
            editor_core::SymbolKind::Diode,
            editor_core::SymbolKind::VoltageSource,
            editor_core::SymbolKind::Ground,
            editor_core::SymbolKind::Switch,
            editor_core::SymbolKind::Led,
            editor_core::SymbolKind::OpAmp,
        ];
        let mut symbols = self.palette_state.favorites().take(9).collect::<Vec<_>>();
        for kind in DEFAULTS {
            if symbols.len() >= 9 {
                break;
            }
            if !symbols.contains(&kind) {
                symbols.push(kind);
            }
        }
        symbols
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            self.status.show(ui);
            ui.separator();
            let tool = match &self.tool {
                Tool::Select => "Sélection".to_owned(),
                Tool::Place(kind) => format!("Placement : {}", ui::symbol_kind_name(kind)),
                Tool::Wire => "Fil".to_owned(),
                Tool::Junction => "Jonction".to_owned(),
                Tool::Label => "Label".to_owned(),
            };
            ui.label(RichText::new(format!("Outil : {tool}")).small());
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

    fn central(&mut self, ui: &mut Ui) {
        if self.scopes.visible() {
            let CircuitApp {
                scopes,
                instruments,
                document,
                status,
                ..
            } = self;
            egui::Panel::bottom("inline_scopes_panel")
                .resizable(true)
                .default_size(320.0)
                .size_range(140.0..=640.0)
                .show(ui, |ui| {
                    scopes.show(ui, instruments, document, status);
                });
        }
        self.canvas(ui);
    }

    fn canvas(&mut self, ui: &mut Ui) {
        let interactive = !self.canvas.has_dialog()
            && !self.show_help
            && !self.show_onboarding
            && !self.show_preferences
            && self.symbol_dialog.is_none()
            && !self.palette.is_open();
        let live = self.instruments.live_values();
        let readings = probe_readings(&self.document, &live);
        let visible = self
            .document
            .schematic()
            .symbols()
            .filter(|symbol| {
                symbol.kind.is_measurement() && self.instruments.probe_visible(symbol.id)
            })
            .map(|symbol| symbol.id)
            .collect::<BTreeSet<_>>();
        let flow_time = if self.instruments.realtime_paused() {
            self.instruments.transient_time().unwrap_or(0.0)
        } else {
            ui.input(|input| input.time)
        };
        self.canvas
            .set_animation_speed(self.instruments.animation_speed());
        self.canvas
            .set_power_brightness(self.instruments.power_brightness());
        self.canvas.set_scope_signals(self.scopes.active_signals());
        self.canvas.set_theme(self.preferences.theme.canvas_theme());
        ui::canvas::CanvasState::show(
            &mut self.canvas,
            ui,
            &mut self.document,
            &self.tool,
            &mut self.status,
            interactive,
            &live,
            &readings,
            &visible,
            flow_time,
        );
        if let Some(id) = self.canvas.take_property_request() {
            self.symbol_dialog = SymbolDialog::new(&self.document, id);
        }
        if let Some(signal) = self.canvas.take_scope_request() {
            self.scopes.push_signal(signal);
            self.status.success("Signal ajouté à la portée.");
        }
    }

    fn symbol_dialog_window(&mut self, context: &Context) {
        let Some(dialog) = self.symbol_dialog.as_mut() else {
            return;
        };
        let live = self.instruments.live_values();
        if !dialog.show(context, &mut self.document, &mut self.status, &live) {
            self.symbol_dialog = None;
        }
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
            CommandAction::Project => self.project_dialog.open_with(&self.document),
            CommandAction::About => self.show_about = true,
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

        let (command, shift, alt, escape, delete, home) = context.input(|input| {
            (
                input.modifiers.command,
                input.modifiers.shift,
                input.modifiers.alt,
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
        if !command && context.input(|input| input.key_pressed(Key::Space)) {
            let now = context.input(|input| input.time);
            self.toggle_live(now);
            return;
        }
        if context.input(|input| {
            input.key_pressed(Key::F5) || (input.modifiers.command && input.key_pressed(Key::Enter))
        }) {
            let now = context.input(|input| input.time);
            self.reset_live(now);
            return;
        }
        let (pan_left, pan_right, pan_up, pan_down) = context.input(|input| {
            (
                input.key_pressed(Key::ArrowLeft),
                input.key_pressed(Key::ArrowRight),
                input.key_pressed(Key::ArrowUp),
                input.key_pressed(Key::ArrowDown),
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
        if context.input(|input| input.key_pressed(Key::W)) {
            self.set_tool(Tool::Wire);
        } else if context.input(|input| input.key_pressed(Key::J)) {
            self.set_tool(Tool::Junction);
        } else if context.input(|input| input.key_pressed(Key::N)) {
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
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
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
                        "Glissez depuis une broche, ou utilisez W puis cliquez l’extrémité. \
                         Maj change le coude.",
                    ),
                    (
                        3,
                        "Simuler",
                        "Le circuit tourne dès l’ouverture. Les fils sont colorés par tension \
                         et Espace met la session en pause.",
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

    fn project_window(&mut self, context: &Context) {
        self.project_dialog
            .show(context, &self.document, &mut self.status);
        if let Some(document) = self.project_dialog.take_loaded() {
            self.document = document;
            self.instruments.clear_results();
            self.scopes = InlineScopes::with_signals(BTreeSet::new());
            self.canvas.cancel_interaction();
            self.canvas.request_fit();
            self.live_autostart = true;
            self.status.success("Projet chargé.");
        }
    }

    fn about_window(&mut self, context: &Context) {
        if !self.show_about {
            return;
        }
        let mut open = true;
        egui::Window::new("À propos")
            .open(&mut open)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(context, |ui| {
                ui.heading(format!("CircuitSim {}", env!("CARGO_PKG_VERSION")));
                ui.label("Simulateur analogique natif et WASM.");
                ui.label("Licence : MIT OR Apache-2.0 · Cédric Debois, 2026.");
                ui.separator();
                ui.label(
                    "Les préférences sont enregistrées dans le stockage local du \
                     navigateur ou dans le fichier de configuration natif.",
                );
                egui::CollapsingHeader::new("Licences tierces")
                    .default_open(false)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .id_salt("third_party_licenses")
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(include_str!(
                                            "../../../THIRD_PARTY_LICENSES.md"
                                        ))
                                        .monospace()
                                        .small(),
                                    )
                                    .wrap(),
                                );
                            });
                    });
                if ui.button("Fermer").clicked() {
                    self.show_about = false;
                }
            });
        if !open {
            self.show_about = false;
        }
    }

    fn preferences_window(&mut self, context: &Context) {
        if !self.show_preferences {
            return;
        }
        let mut open = self.show_preferences;
        let mut changed = false;
        egui::Window::new("Préférences d’affichage")
            .open(&mut open)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
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
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
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
                        for entry in SHORTCUTS {
                            shortcut(ui, entry.keys, entry.action);
                        }
                    });
                ui.add_space(6.0);
                ui.label(RichText::new("Simulation").strong());
                ui.label(
                    "Le panneau Instruments exécute le transitoire, le spectre FFT et \
                     l'analyse AC par le service worker. L'échelon de source force une source \
                     à une valeur initiale avant d'appliquer sa valeur finale.",
                );
                ui.label(
                    "La simulation continue démarre à l'ouverture. Espace l'arrête ou la reprend ; \
                     Reset et F5 reviennent à t = 0. Les paramètres sont appliqués à chaud tant que \
                     la topologie reste compatible. Les fils sont colorés par tension et les sondes \
                     V/A/Ω se placent depuis la palette.",
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
        if self.live_autostart {
            self.live_autostart = false;
            if !self.instruments.realtime_active() && self.instruments.can_simulate(&self.document)
            {
                self.instruments
                    .start_realtime(&self.document, now, &mut self.status);
            }
        }
        self.tick_auto_simulation(now);
        self.instruments
            .tick_realtime(&self.document, now, &mut self.status);
        if self.instruments.take_diagnostic_request() {
            self.show_inspector = true;
        }
        if self.instruments.auto_refresh() && (self.auto.pending() || self.instruments.is_busy()) {
            ui.ctx().request_repaint_after(Duration::from_millis(40));
        }
        if self.instruments.realtime_active() && !self.instruments.realtime_paused() {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
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
                .show(ui, |ui| {
                    if ui
                        .small_button("«")
                        .on_hover_text("Replier la palette")
                        .clicked()
                    {
                        self.show_palette = false;
                    }
                    self.palette(ui);
                });
        }
        if self.show_inspector {
            egui::Panel::right("inspector_panel")
                .resizable(true)
                .default_size(335.0)
                .size_range(280.0..=470.0)
                .show(ui, |ui| {
                    if ui
                        .small_button("»")
                        .on_hover_text("Replier l'inspecteur")
                        .clicked()
                    {
                        self.show_inspector = false;
                    }
                    self.inspector(ui);
                });
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.central(ui));

        self.canvas
            .show_label_dialog(ui.ctx(), &mut self.document, &mut self.status);
        if let Some(command) = self.palette.show(ui.ctx(), &self.document) {
            self.apply_command(command);
        }
        self.help_window(ui.ctx());
        self.onboarding_window(ui.ctx());
        self.preferences_window(ui.ctx());
        self.about_window(ui.ctx());
        self.project_window(ui.ctx());
        self.symbol_dialog_window(ui.ctx());
        self.persist_if_due(now);
    }
}

fn apply_style(context: &Context, preferences: DisplayPreferences) {
    let visuals = match preferences.theme {
        ThemeMode::Dark => dark_visuals(),
        ThemeMode::HighContrast => high_contrast_visuals(),
        ThemeMode::Light => light_visuals(),
    };
    context.set_visuals(visuals);
    context.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(7.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.animation_time = if preferences.reduce_motion { 0.0 } else { 0.15 };
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

fn light_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = Color32::from_rgb(248, 249, 251);
    visuals.window_fill = Color32::WHITE;
    visuals.extreme_bg_color = Color32::from_rgb(238, 241, 246);
    visuals.faint_bg_color = Color32::from_rgb(232, 236, 242);
    visuals.selection.bg_fill = Color32::from_rgb(188, 214, 244);
    visuals.selection.stroke = egui::Stroke::new(1.0, Color32::from_rgb(28, 86, 156));
    visuals.hyperlink_color = Color32::from_rgb(0, 110, 130);
    visuals.window_stroke = egui::Stroke::new(1.0, Color32::from_rgb(196, 204, 216));
    visuals
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
        assert_eq!(ThemeMode::Light.label(), "Clair");
        assert_eq!(ThemeMode::ALL.len(), 3);
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
