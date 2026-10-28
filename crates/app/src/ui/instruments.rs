//! Simulation controls, scope, FFT, Bode plots, and CSV export.

use std::collections::BTreeSet;
use std::time::Duration;

use editor_core::SymbolId;
use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, RichText, Ui, Vec2};
use egui_plot::{HoverPosition, Line, Plot};
use sim_core::units::{format_si_precision, Unit};
use worker::protocol::{AnalysisKind, AnalysisResult};

#[cfg(test)]
use crate::analysis::prepare_transient;
use crate::analysis::{
    analysis_name, bode_csv, bode_trace, compute_spectrum, prepare_ac, spectrum_csv, transient_csv,
    transient_traces, transient_traces_from_samples, transient_traces_from_samples_with_document,
    AcSettings, BodePoint, BodeTrace, CompletedRun, SignalCatalog, SignalId, SignalUnit,
    SimulationClient, SimulationError, Spectrum, Trace, TransientSettings, MAX_AC_POINTS,
};
use crate::document::Document;
use crate::export;
use crate::simulation::{LiveSimulation, TickOutcome, LIVE_SPEEDS};
use crate::ui::scopes::InlineScopes;
use crate::ui::{Status, ACCENT, HOVER, SYMBOL};

const ERROR: Color32 = Color32::from_rgb(240, 128, 128);
const CONTROL_WIDTH: f32 = 304.0;
pub(crate) const MAX_PLOT_POINTS: usize = 4096;

const TRACE_COLORS: [Color32; 8] = [
    Color32::from_rgb(255, 190, 92),
    Color32::from_rgb(120, 210, 220),
    Color32::from_rgb(112, 210, 155),
    Color32::from_rgb(240, 138, 180),
    Color32::from_rgb(130, 170, 245),
    Color32::from_rgb(245, 145, 90),
    Color32::from_rgb(190, 160, 245),
    Color32::from_rgb(180, 220, 95),
];

/// Instrument tab selected in the bottom panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstrumentTab {
    Scope,
    Spectrum,
    Bode,
}

impl InstrumentTab {
    const ALL: [Self; 3] = [Self::Scope, Self::Spectrum, Self::Bode];

    const fn label(self) -> &'static str {
        match self {
            Self::Scope => "Portées",
            Self::Spectrum => "Spectre",
            Self::Bode => "Bode",
        }
    }

    const fn filename(self) -> &'static str {
        match self {
            Self::Scope => "circuitsim-portee.csv",
            Self::Spectrum => "circuitsim-spectre.csv",
            Self::Bode => "circuitsim-bode.csv",
        }
    }
}

/// Persistent state for simulation, instruments, and export.
pub struct InstrumentsState {
    tab: InstrumentTab,
    transient: TransientSettings,
    ac: AcSettings,
    selected: BTreeSet<SignalId>,
    spectrum_signal: Option<SignalId>,
    client: Option<SimulationClient>,
    client_error: Option<String>,
    transient_run: Option<CompletedRun>,
    ac_run: Option<CompletedRun>,
    pending: Option<AnalysisKind>,
    error: Option<String>,
    selection_initialized: bool,
    auto_refresh: bool,
    scope_window: f64,
    scope_signals: BTreeSet<SignalId>,
    realtime: Option<LiveSimulation>,
    realtime_speed: f64,
    live_step: String,
    animation_speed: f64,
    power_brightness: f32,
    live_error: Option<String>,
    diagnostic_request: bool,
    bode_cursor: Option<f64>,
    hidden_probes: BTreeSet<SymbolId>,
    export_path: String,
}

impl InstrumentsState {
    /// Creates the panel and its worker client.
    pub fn new() -> Self {
        let (client, client_error) = match SimulationClient::new() {
            Ok(client) => (Some(client), None),
            Err(error) => (None, Some(error.user_message())),
        };
        Self {
            tab: InstrumentTab::Scope,
            transient: TransientSettings::default(),
            ac: AcSettings::default(),
            selected: BTreeSet::new(),
            spectrum_signal: None,
            client,
            client_error,
            transient_run: None,
            ac_run: None,
            pending: None,
            error: None,
            selection_initialized: false,
            auto_refresh: true,
            scope_window: crate::simulation::DEFAULT_SCOPE_WINDOW_SECONDS,
            scope_signals: BTreeSet::new(),
            realtime: None,
            realtime_speed: 1.0,
            live_step: "1m".to_owned(),
            animation_speed: 1.0,
            power_brightness: 0.6,
            live_error: None,
            diagnostic_request: false,
            bode_cursor: None,
            hidden_probes: BTreeSet::new(),
            export_path: default_export_path(),
        }
    }

    /// Clears analyses and cursors without recreating the worker client.
    pub fn clear_results(&mut self) {
        self.transient_run = None;
        self.ac_run = None;
        self.pending = None;
        self.error = None;
        self.realtime = None;
        self.live_error = None;
        self.selection_initialized = false;
        self.bode_cursor = None;
    }

    /// Returns the last transient sample mapped to names for the inspector.
    pub fn live_values(&self) -> crate::analysis::LiveValues {
        if let Some(run) = &self.realtime {
            return run.live_values();
        }
        let Some(run) = &self.transient_run else {
            return crate::analysis::LiveValues::default();
        };
        let AnalysisResult::Transient(result) = &run.result else {
            return crate::analysis::LiveValues::default();
        };
        let Some(sample) = result.samples.last() else {
            return crate::analysis::LiveValues::default();
        };
        let mut node_voltages = std::collections::BTreeMap::new();
        for (name, node) in &run.context.nodes {
            if let Some(voltage) = sample.node_voltage(*node) {
                node_voltages.insert(name.clone(), voltage);
            }
        }
        let mut branch_currents = std::collections::BTreeMap::new();
        for (name, component) in &run.context.components {
            if let Some(current) = sample.branch_current(*component) {
                branch_currents.insert(name.clone(), current);
            }
        }
        crate::analysis::LiveValues {
            document_revision: run.context.document_revision,
            time: Some(sample.time),
            node_voltages,
            branch_currents,
        }
    }

    /// Returns true when a transient analysis can be submitted.
    pub fn can_simulate(&self, document: &Document) -> bool {
        self.can_run(document)
    }

    /// Returns true while an analysis is pending.
    pub const fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Returns the document revision of the last transient result.
    pub fn transient_revision(&self) -> Option<u64> {
        if let Some(run) = &self.realtime {
            return Some(run.sample_revision());
        }
        self.transient_run
            .as_ref()
            .map(|run| run.context.document_revision)
    }

    /// Returns the time of the last transient sample.
    pub fn transient_time(&self) -> Option<f64> {
        if let Some(run) = &self.realtime {
            return run.last_sample().map(|sample| sample.time);
        }
        let run = self.transient_run.as_ref()?;
        let AnalysisResult::Transient(result) = &run.result else {
            return None;
        };
        result.samples.last().map(|sample| sample.time)
    }

    /// Returns the nominal integration step of the active transient.
    pub fn nominal_step(&self) -> Option<f64> {
        if let Some(run) = &self.realtime {
            return run.context().nominal_step;
        }
        self.transient_run
            .as_ref()
            .and_then(|run| run.context.nominal_step)
    }

    /// Returns the effective step of the decimated realtime scope traces.
    pub fn scope_step(&self) -> Option<f64> {
        self.realtime
            .as_ref()
            .and_then(LiveSimulation::scope_step)
            .or_else(|| self.nominal_step())
    }

    /// Returns the simulated time window shown by the scopes.
    pub const fn scope_window(&self) -> f64 {
        self.scope_window
    }

    /// Changes the simulated time window shown by the scopes.
    pub fn set_scope_window(&mut self, seconds: f64) {
        if seconds.is_finite() {
            self.scope_window = seconds.clamp(
                crate::simulation::MIN_SCOPE_WINDOW_SECONDS,
                crate::simulation::MAX_SCOPE_WINDOW_SECONDS,
            );
        }
    }

    /// Returns a cache key for the current transient traces.
    pub fn trace_key(&self) -> (u64, Option<f64>, f64) {
        (
            self.transient_revision().unwrap_or(0),
            self.transient_time(),
            self.scope_window,
        )
    }

    /// Returns whether edits trigger an automatic transient refresh.
    pub const fn auto_refresh(&self) -> bool {
        self.auto_refresh
    }

    /// Brings the merged scope tab to the front.
    pub fn focus_scopes(&mut self) {
        self.tab = InstrumentTab::Scope;
    }

    /// Enables or disables automatic transient refresh.
    pub fn set_auto_refresh(&mut self, enabled: bool) {
        self.auto_refresh = enabled;
    }

    /// Returns whether a probe value is displayed.
    pub fn probe_visible(&self, id: SymbolId) -> bool {
        !self.hidden_probes.contains(&id)
    }

    /// Shows or hides one probe value.
    pub fn set_probe_visible(&mut self, id: SymbolId, visible: bool) {
        if visible {
            self.hidden_probes.remove(&id);
        } else {
            self.hidden_probes.insert(id);
        }
    }

    /// Runs the configured transient immediately.
    pub fn run_transient_now(&mut self, document: &Document, now: f64, status: &mut Status) {
        let paused = self.realtime_paused();
        self.restart_realtime(document, now, paused, status);
    }

    /// Starts or restarts the open-ended real-time session.
    pub fn start_realtime(&mut self, document: &Document, now: f64, status: &mut Status) {
        self.restart_realtime(document, now, false, status);
    }

    /// Resets the session to t = 0 with the current settings.
    pub fn reset_realtime(&mut self, document: &Document, now: f64, status: &mut Status) {
        let paused = self.realtime_paused();
        self.restart_realtime(document, now, paused, status);
    }

    fn restart_realtime(
        &mut self,
        document: &Document,
        now: f64,
        paused: bool,
        status: &mut Status,
    ) {
        if self.pending.is_some() {
            status.error("Une analyse est déjà en cours.");
            return;
        }
        let live_step = match crate::analysis::parse_live_step(&self.live_step) {
            Ok(step) => step,
            Err(error) => {
                let message = error.user_message();
                self.live_error = Some(message.clone());
                self.fail(status, error);
                return;
            }
        };
        if !(crate::simulation::MIN_LIVE_STEP_SECONDS..=crate::simulation::MAX_LIVE_STEP_SECONDS)
            .contains(&live_step)
        {
            self.live_error =
                Some("Le pas temps réel doit être compris entre 1 µs et 10 ms.".to_owned());
            status.error("Le pas temps réel doit être compris entre 1 µs et 10 ms.");
            return;
        }
        match LiveSimulation::start_with_step(document, &self.transient, now, live_step) {
            Ok(mut run) => {
                run.set_speed(self.realtime_speed);
                run.set_paused(paused);
                self.realtime = Some(run);
                self.live_error = None;
                self.error = None;
                status.info(if paused {
                    "Simulation temps réel prête, en pause."
                } else {
                    "Simulation temps réel démarrée."
                });
            }
            Err(error) => {
                let message = error.user_message();
                self.live_error = Some(message.clone());
                self.realtime = None;
                self.fail(status, error);
            }
        }
    }

    /// Returns whether a real-time transient session exists.
    pub const fn realtime_active(&self) -> bool {
        self.realtime.is_some()
    }

    /// Advances the real-time session using the host clock.
    pub fn tick_realtime(&mut self, document: &Document, now: f64, status: &mut Status) {
        let Some(mut run) = self.realtime.take() else {
            return;
        };
        match run.tick(document, now) {
            Ok(TickOutcome::NeedsRestart) => {
                let paused = run.paused();
                let speed = run.speed();
                self.realtime_speed = speed;
                self.restart_realtime(document, now, paused, status);
                if self.realtime.is_some() {
                    status.info("Topologie modifiée : session relancée.");
                }
            }
            Ok(TickOutcome::Advanced | TickOutcome::Paused) => {
                self.realtime = Some(run);
            }
            Err(error) => {
                let message = error.user_message();
                self.live_error = Some(message.clone());
                self.fail(status, error);
            }
        }
    }

    /// Advances exactly one step and keeps the session paused.
    pub fn step_realtime(&mut self, document: &Document, status: &mut Status) {
        let Some(run) = self.realtime.as_mut() else {
            status.info("Démarrez la simulation avant d'avancer d'un pas.");
            return;
        };
        run.set_paused(true);
        match run.step_once(document) {
            Ok(()) => status.success("Un pas simulé."),
            Err(error) => {
                let message = error.user_message();
                self.live_error = Some(message.clone());
                self.realtime = None;
                self.fail(status, error);
            }
        }
    }

    /// Pauses or resumes the current real-time session.
    pub fn set_realtime_paused(&mut self, paused: bool) {
        if let Some(run) = self.realtime.as_mut() {
            run.set_paused(paused);
        }
    }

    /// Returns whether the real-time session is paused.
    pub fn realtime_paused(&self) -> bool {
        self.realtime.as_ref().is_some_and(LiveSimulation::paused)
    }

    /// Returns the playback speed multiplier.
    pub const fn realtime_speed(&self) -> f64 {
        self.realtime_speed
    }

    /// Sets the playback speed multiplier.
    pub fn set_realtime_speed(&mut self, speed: f64) {
        if speed.is_finite() && speed > 0.0 {
            self.realtime_speed = speed;
            if let Some(run) = self.realtime.as_mut() {
                run.set_speed(speed);
            }
        }
    }

    /// Returns the current-animation speed multiplier.
    pub const fn animation_speed(&self) -> f64 {
        self.animation_speed
    }

    /// Sets the current-animation speed multiplier.
    pub fn set_animation_speed(&mut self, speed: f64) {
        if speed.is_finite() && speed > 0.0 {
            self.animation_speed = speed.clamp(0.1, 4.0);
        }
    }

    /// Returns the component-power glow strength.
    pub const fn power_brightness(&self) -> f32 {
        self.power_brightness
    }

    /// Sets the component-power glow strength.
    pub fn set_power_brightness(&mut self, brightness: f32) {
        if brightness.is_finite() {
            self.power_brightness = brightness.clamp(0.0, 1.0);
        }
    }

    /// Returns the ratio between simulated and requested time.
    pub fn live_ratio(&self) -> f64 {
        self.realtime.as_ref().map_or(1.0, LiveSimulation::ratio)
    }

    /// Returns whether the live engine cannot keep up.
    pub fn live_lagging(&self) -> bool {
        self.realtime.as_ref().is_some_and(LiveSimulation::lagging)
    }

    /// Returns the last live error, if any.
    pub fn live_error(&self) -> Option<&str> {
        self.live_error.as_deref()
    }

    /// Takes one request to reveal the diagnostic panel.
    pub fn take_diagnostic_request(&mut self) -> bool {
        std::mem::take(&mut self.diagnostic_request)
    }

    /// Draws the merged instruments and scopes panel.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        scopes: &mut InlineScopes,
        document: &Document,
        status: &mut Status,
        wire_limit: f64,
    ) {
        let now = ui.input(|input| input.time);
        self.poll(now, status);

        let catalog = document
            .extraction()
            .map(SignalCatalog::from_extraction)
            .unwrap_or_default();
        if document.extraction().is_some() {
            self.ensure_catalog(&catalog);
        }
        if self.pending.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(20));
        }
        if self.realtime_active() && !self.realtime_paused() {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        self.tab_bar(ui, document);
        ui.separator();
        if let Some(error) = &self.error {
            ui.colored_label(ERROR, error);
        }

        let height = ui.available_height().max(120.0);
        match self.tab {
            InstrumentTab::Scope => {
                self.scope_tab(ui, scopes, document, &catalog, status, height, wire_limit);
            }
            InstrumentTab::Spectrum | InstrumentTab::Bode => {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(CONTROL_WIDTH, height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("instrument_controls")
                                .max_height(height)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    self.controls(ui, document, &catalog, now, status);
                                });
                        },
                    );
                    ui.separator();
                    let remaining = ui.available_width();
                    let side_width = if remaining > 460.0 { 178.0 } else { 0.0 };
                    let plot_width = (remaining - side_width - 8.0).max(120.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(plot_width, height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            self.plots(ui);
                        },
                    );
                    if side_width > 0.0 {
                        ui.separator();
                        ui.allocate_ui_with_layout(
                            Vec2::new(side_width, height),
                            Layout::top_down(Align::Min),
                            |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt("live_controls")
                                    .max_height(height)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        self.live_controls(ui, document, now, status);
                                    });
                            },
                        );
                    }
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn scope_tab(
        &mut self,
        ui: &mut Ui,
        scopes: &mut InlineScopes,
        document: &Document,
        catalog: &SignalCatalog,
        status: &mut Status,
        height: f32,
        wire_limit: f64,
    ) {
        let now = ui.input(|input| input.time);
        self.scope_signals = scopes.active_signals();
        if scopes.is_empty() && !self.selected.is_empty() {
            scopes.push_signals(self.selected.clone());
        }
        let available = ui.available_width();
        let control_width =
            ((available * 0.4).clamp(220.0, CONTROL_WIDTH)).min((available - 180.0).max(150.0));
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(control_width, height),
                Layout::top_down(Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("instrument_controls")
                        .max_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.live_controls(ui, document, now, status);
                            ui.separator();
                            if ui
                                .button("Ajouter une portée")
                                .on_hover_text("Ajoute une portée avec le signal par défaut")
                                .clicked()
                            {
                                if scopes.add_default_scope(catalog) {
                                    status.info("Portée ajoutée.");
                                } else {
                                    status.info("Aucun signal disponible.");
                                }
                            }
                            ui.label(
                                RichText::new(
                                    "Molette : zoom · glisser : déplacer · clic : curseurs A/B \
                                     · double-clic : auto-échelle",
                                )
                                .small()
                                .color(crate::ui::muted_text_color(ui)),
                            );
                            ui.separator();
                            let mut window = self.scope_window;
                            ui.label(RichText::new("Fenêtre temporelle").strong());
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut window,
                                        crate::simulation::MIN_SCOPE_WINDOW_SECONDS
                                            ..=crate::simulation::MAX_SCOPE_WINDOW_SECONDS,
                                    )
                                    .logarithmic(true)
                                    .suffix(" s"),
                                )
                                .on_hover_text(
                                    "Durée de temps simulé affichée par les portées (10 s par \
                                     défaut). Le tampon garde 10 000 échantillons : sa durée \
                                     vaut 10 000 × le pas live.",
                                )
                                .changed()
                            {
                                self.set_scope_window(window);
                            }
                            ui.label(
                                RichText::new(format!(
                                    "{} affichées",
                                    format_si_precision(self.scope_window, Unit::Second, 3)
                                ))
                                .small()
                                .color(crate::ui::muted_text_color(ui)),
                            );
                            ui.separator();
                            self.export_controls(ui, document, status);
                        });
                },
            );
            ui.separator();
            let plot_width = ui.available_width().max(160.0);
            ui.allocate_ui_with_layout(
                Vec2::new(plot_width, height),
                Layout::top_down(Align::Min),
                |ui| {
                    scopes.show(ui, self, document, status, wire_limit);
                },
            );
        });
        self.scope_signals = scopes.active_signals();
    }

    fn live_controls(&mut self, ui: &mut Ui, document: &Document, now: f64, status: &mut Status) {
        ui.heading("Contrôles");
        let running = self.realtime_active();
        let paused = self.realtime_paused();
        let live_error = self.live_error().map(str::to_owned);
        if let Some(error) = live_error {
            if ui
                .link("Voir le diagnostic")
                .on_hover_text(error.clone())
                .clicked()
            {
                self.diagnostic_request = true;
                status.info(error);
            }
        }
        let mut speed = self.realtime_speed;
        if ui
            .add(
                egui::Slider::new(&mut speed, 0.05..=10.0)
                    .logarithmic(true)
                    .text("Simulation"),
            )
            .on_hover_text("Rapport entre le temps simulé et le temps réel")
            .changed()
        {
            self.set_realtime_speed(speed);
        }
        let effective_step = self
            .realtime
            .as_ref()
            .and_then(LiveSimulation::effective_step);
        let mut apply_step = false;
        ui.horizontal(|ui| {
            ui.label("Pas live");
            ui.add(
                egui::TextEdit::singleline(&mut self.live_step)
                    .desired_width(70.0)
                    .hint_text("1m"),
            )
            .on_hover_text(
                "Pas d'intégration de la session temps réel (1µ à 10 ms). Un pas plus \
                 fin montre le détail mais ralentit le temps simulé.",
            );
            let pending = crate::analysis::parse_live_step(&self.live_step)
                .ok()
                .zip(effective_step)
                .is_some_and(|(requested, current)| (requested - current).abs() > current * 1e-9);
            if running
                && pending
                && ui
                    .small_button("Appliquer")
                    .on_hover_text("Redémarre la session à t = 0 avec ce pas.")
                    .clicked()
            {
                apply_step = true;
            }
        });
        if apply_step {
            self.reset_realtime(document, now, status);
        }
        if let Some(step) = self
            .realtime
            .as_ref()
            .and_then(LiveSimulation::effective_step)
        {
            let span = crate::simulation::MAX_SAMPLES as f64 * step;
            ui.label(
                RichText::new(format!(
                    "Pas temps réel : {} · tampon {}",
                    format_si_precision(step, Unit::Second, 3),
                    format_si_precision(span, Unit::Second, 3)
                ))
                .small()
                .color(crate::ui::muted_text_color(ui)),
            )
            .on_hover_text(
                "Un échantillon est gardé par pas d'intégration : plus le pas est fin, \
                 plus la fenêtre conservée est courte, mais plus le détail est fin. \
                 Les analyses complètes gardent le pas saisi dans l'onglet Spectre.",
            );
        }
        let mut animation = self.animation_speed;
        if ui
            .add(
                egui::Slider::new(&mut animation, 0.1..=4.0)
                    .logarithmic(true)
                    .text("Courant"),
            )
            .on_hover_text("Vitesse des points animés sur les fils")
            .changed()
        {
            self.set_animation_speed(animation);
        }
        let mut power = self.power_brightness;
        if ui
            .add(egui::Slider::new(&mut power, 0.0..=1.0).text("Puissance"))
            .on_hover_text("Intensité du halo de puissance des composants")
            .changed()
        {
            self.set_power_brightness(power);
        }
        ui.separator();
        let can_run = self.can_simulate(document);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    can_run || running,
                    egui::Button::new(if running && !paused {
                        "Pause"
                    } else if running {
                        "Reprendre"
                    } else {
                        "Démarrer"
                    }),
                )
                .on_hover_text("Espace")
                .clicked()
            {
                if running {
                    self.set_realtime_paused(!paused);
                } else {
                    self.start_realtime(document, now, status);
                }
            }
            if ui
                .add_enabled(running, egui::Button::new("Réinitialiser"))
                .on_hover_text("Remet la session à t = 0")
                .clicked()
            {
                self.reset_realtime(document, now, status);
            }
            if ui
                .add_enabled(running && paused, egui::Button::new("Pas suivant"))
                .on_hover_text("Avance d'un pas en pause")
                .clicked()
            {
                self.step_realtime(document, status);
            }
        });
        if let Some(time) = self.transient_time() {
            ui.label(format!(
                "t = {}",
                format_si_precision(time, Unit::Second, 4)
            ));
        }
        if let Some(error) = &self.error {
            if self.live_error.is_none() {
                ui.colored_label(ERROR, error);
            }
        }
    }

    fn tab_bar(&mut self, ui: &mut Ui, document: &Document) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Instruments").strong().size(16.0));
            for tab in InstrumentTab::ALL {
                ui.selectable_value(&mut self.tab, tab, tab.label());
            }
            if let Some(kind) = self.pending {
                ui.separator();
                ui.spinner();
                ui.label(
                    RichText::new(format!("Calcul {} en cours…", analysis_name(kind)))
                        .small()
                        .color(crate::ui::muted_text_color(ui)),
                );
            }
            ui.separator();
            let mut auto = self.auto_refresh;
            if ui
                .checkbox(&mut auto, "Auto")
                .on_hover_text(
                    "Recalcule le transitoire après une modification, avec un court délai.",
                )
                .changed()
            {
                self.auto_refresh = auto;
            }
            if let Some(time) = self.transient_time() {
                ui.label(
                    RichText::new(format!(
                        "t = {}",
                        format_si_precision(time, Unit::Second, 4)
                    ))
                    .small()
                    .color(crate::ui::muted_text_color(ui)),
                );
            }
            if self
                .current_revision()
                .is_some_and(|revision| revision != document.revision())
            {
                ui.separator();
                ui.label(
                    RichText::new("Résultat antérieur aux dernières modifications")
                        .small()
                        .color(ACCENT),
                );
            }
        });
    }

    fn controls(
        &mut self,
        ui: &mut Ui,
        document: &Document,
        catalog: &SignalCatalog,
        now: f64,
        status: &mut Status,
    ) {
        if let Some(error) = &self.client_error {
            ui.colored_label(ERROR, error);
        }
        match self.tab {
            InstrumentTab::Scope => {}
            InstrumentTab::Spectrum => {
                self.spectrum_controls(ui, document, catalog, now, status);
            }
            InstrumentTab::Bode => {
                self.bode_controls(ui, document, catalog, now, status);
            }
        }
        ui.separator();
        self.export_controls(ui, document, status);
    }

    fn spectrum_controls(
        &mut self,
        ui: &mut Ui,
        document: &Document,
        catalog: &SignalCatalog,
        now: f64,
        status: &mut Status,
    ) {
        let can_run = self.can_run(document);
        let mut speed = self.realtime_speed;
        let paused = self.realtime_paused();
        let running = self.realtime_active();
        let ratio = self.live_ratio();
        let lagging = self.live_lagging();
        let effective_step = self
            .realtime
            .as_ref()
            .and_then(LiveSimulation::effective_step);
        let action = realtime_controls(
            ui,
            &mut self.transient,
            catalog,
            can_run,
            &mut speed,
            paused,
            running,
            ratio,
            lagging,
            effective_step,
        );
        if speed != self.realtime_speed {
            self.set_realtime_speed(speed);
        }
        if matches!(action, RealtimeAction::Start) {
            self.start_realtime(document, now, status);
        } else if matches!(action, RealtimeAction::TogglePause) {
            let paused = self.realtime_paused();
            self.set_realtime_paused(!paused);
        }
        ui.separator();
        spectrum_signal_control(ui, &mut self.spectrum_signal, catalog);
        match self.spectrum_data() {
            Ok(Some((trace, spectrum))) => {
                if let Some(peak) = &spectrum.peak {
                    ui.label(format!(
                        "Pic : {} à {}",
                        format_si_precision(peak.magnitude, trace.unit().unit(), 4),
                        format_frequency(peak.frequency)
                    ));
                } else {
                    ui.label(
                        RichText::new("Aucune composante non continue.")
                            .color(crate::ui::muted_text_color(ui)),
                    );
                }
                ui.label(
                    RichText::new(format!(
                        "{} points · résolution {}",
                        spectrum.sample_count,
                        format_frequency(spectrum.resolution)
                    ))
                    .small()
                    .color(crate::ui::muted_text_color(ui)),
                );
            }
            Ok(None) => {
                ui.label(
                    RichText::new("Lancez le transitoire pour calculer le spectre.")
                        .color(crate::ui::muted_text_color(ui)),
                );
            }
            Err(error) => {
                ui.colored_label(ERROR, error.user_message());
            }
        }
    }

    fn bode_controls(
        &mut self,
        ui: &mut Ui,
        document: &Document,
        catalog: &SignalCatalog,
        now: f64,
        status: &mut Status,
    ) {
        let can_run = self.can_run(document);
        if ac_controls(ui, &mut self.ac, catalog, can_run) {
            self.run_ac(document, now, status);
        }
        ui.separator();
        bode_output_control(ui, &mut self.ac.output, catalog);
        match self.bode_data() {
            Ok(Some(trace)) => {
                ui.label(
                    RichText::new(format!("{} points de fréquence", trace.points.len()))
                        .small()
                        .color(crate::ui::muted_text_color(ui)),
                );
                if let Some(frequency) = self.bode_cursor {
                    if let Some(point) = nearest_bode_point(&trace.points, frequency) {
                        ui.label(format!(
                            "Curseur : {} · gain {:.2} dB · phase {:.1}°",
                            format_frequency(point.frequency),
                            point.gain_db,
                            point.phase_degrees
                        ));
                        if ui.small_button("Effacer le curseur").clicked() {
                            self.bode_cursor = None;
                        }
                    }
                } else {
                    ui.label(
                        RichText::new("Clic sur le graphe : marqueur de fréquence.")
                            .small()
                            .color(crate::ui::muted_text_color(ui)),
                    );
                }
            }
            Ok(None) => {
                ui.label(
                    RichText::new("Lancez l'analyse AC pour tracer le Bode.")
                        .color(crate::ui::muted_text_color(ui)),
                );
            }
            Err(error) => {
                ui.colored_label(ERROR, error.user_message());
            }
        }
    }

    fn plots(&mut self, ui: &mut Ui) {
        match self.tab {
            InstrumentTab::Spectrum => self.spectrum_plot(ui),
            InstrumentTab::Bode => self.bode_plot(ui),
            InstrumentTab::Scope => {}
        }
    }

    fn spectrum_plot(&self, ui: &mut Ui) {
        let (trace, spectrum) = match self.spectrum_data() {
            Ok(Some(data)) => data,
            Ok(None) => {
                empty_plot_message(ui, "Aucun spectre disponible.");
                return;
            }
            Err(error) => {
                ui.colored_label(ERROR, error.user_message());
                return;
            }
        };
        let unit = trace.unit();
        let revision = self.transient_run.as_ref().map_or(0, |run| run.revision);
        let points = spectrum
            .frequencies
            .iter()
            .zip(&spectrum.magnitudes)
            .map(|(frequency, magnitude)| [*frequency, *magnitude])
            .collect::<Vec<_>>();
        let height = ui.available_height().max(100.0);
        let x_formatter = |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            format_frequency(mark.value)
        };
        let y_formatter = move |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            format_si_precision(mark.value, unit.unit(), 3)
        };
        Plot::new(format!("spectrum_plot_{revision}"))
            .height(height)
            .x_axis_label("Fréquence")
            .y_axis_label(match unit {
                SignalUnit::Volt => "Amplitude (V)",
                SignalUnit::Ampere => "Amplitude (A)",
            })
            .x_axis_formatter(x_formatter)
            .y_axis_formatter(y_formatter)
            .label_formatter(move |position| {
                let position = position_position(position);
                Some(format!(
                    "{}\n{}",
                    format_frequency(position.x),
                    format_si_precision(position.y, unit.unit(), 4)
                ))
            })
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new(
                        format!("Spectre de {}", trace.label()),
                        decimate(&points, MAX_PLOT_POINTS),
                    )
                    .color(HOVER)
                    .width(1.6),
                );
            });
    }

    fn bode_plot(&mut self, ui: &mut Ui) {
        let trace = match self.bode_data() {
            Ok(Some(trace)) => trace,
            Ok(None) => {
                empty_plot_message(ui, "Aucune réponse AC disponible.");
                return;
            }
            Err(error) => {
                ui.colored_label(ERROR, error.user_message());
                return;
            }
        };
        let gain = trace
            .points
            .iter()
            .map(|point| [point.frequency.log10(), point.gain_db])
            .collect::<Vec<_>>();
        let phase = trace
            .points
            .iter()
            .map(|point| [point.frequency.log10(), point.phase_degrees])
            .collect::<Vec<_>>();
        let revision = self.ac_run.as_ref().map_or(0, |run| run.revision);
        let available = ui.available_height().max(160.0);
        let gain_height = ((available - 24.0) * 0.55).max(70.0);
        let phase_height = (available - gain_height - 24.0).max(70.0);
        let x_formatter = |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            format_frequency(10f64.powf(mark.value))
        };
        let gain_formatter = |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            format!("{:.1} dB", mark.value)
        };
        let phase_formatter = |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            format!("{:.0}°", mark.value)
        };
        let mut clicked_frequency = None;
        let gain_response = Plot::new(format!("bode_gain_{revision}"))
            .height(gain_height)
            .link_axis(egui::Id::new("bode_frequency_axis"), [true, false])
            .x_axis_label("Fréquence")
            .y_axis_label("Gain (dB)")
            .x_axis_formatter(x_formatter)
            .y_axis_formatter(gain_formatter)
            .x_grid_spacer(bode_grid)
            .label_formatter(|position| {
                let position = position_position(position);
                Some(format!(
                    "{}\n{:.2} dB",
                    format_frequency(10f64.powf(position.x)),
                    position.y
                ))
            })
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new("Gain", decimate(&gain, MAX_PLOT_POINTS))
                        .color(SYMBOL)
                        .width(1.8),
                );
                if plot_ui.response().clicked() {
                    if let Some(point) = plot_ui.pointer_coordinate() {
                        clicked_frequency = Some(10f64.powf(point.x));
                    }
                }
            });
        self.apply_bode_click(clicked_frequency.take(), &trace);
        ui.add_space(4.0);
        let phase_response = Plot::new(format!("bode_phase_{revision}"))
            .height(phase_height)
            .link_axis(egui::Id::new("bode_frequency_axis"), [true, false])
            .x_axis_label("Fréquence")
            .y_axis_label("Phase (°)")
            .x_axis_formatter(x_formatter)
            .y_axis_formatter(phase_formatter)
            .x_grid_spacer(bode_grid)
            .include_y(-180.0)
            .include_y(180.0)
            .label_formatter(|position| {
                let position = position_position(position);
                Some(format!(
                    "{}\n{:.1}°",
                    format_frequency(10f64.powf(position.x)),
                    position.y
                ))
            })
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new("Phase", decimate(&phase, MAX_PLOT_POINTS))
                        .color(ACCENT)
                        .width(1.6),
                );
                if plot_ui.response().clicked() {
                    if let Some(point) = plot_ui.pointer_coordinate() {
                        clicked_frequency = Some(10f64.powf(point.x));
                    }
                }
            });
        self.apply_bode_click(clicked_frequency, &trace);
        if let Some(point) = self
            .bode_cursor
            .and_then(|frequency| nearest_bode_point(&trace.points, frequency))
        {
            let x = point.frequency.log10();
            paint_vertical_marker(ui, &gain_response, x, "f", HOVER);
            paint_vertical_marker(ui, &phase_response, x, "f", HOVER);
        }
    }

    fn apply_bode_click(&mut self, frequency: Option<f64>, trace: &BodeTrace) {
        if let Some(frequency) = frequency {
            self.bode_cursor =
                nearest_bode_point(&trace.points, frequency).map(|point| point.frequency);
        }
    }

    fn export_controls(&mut self, ui: &mut Ui, document: &Document, status: &mut Status) {
        egui::CollapsingHeader::new("Export CSV")
            .default_open(false)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Toutes les valeurs de l'onglet actif, sans décimation.")
                        .small()
                        .color(crate::ui::muted_text_color(ui)),
                );
                ui.label(
                    RichText::new(format!("Nom proposé : {}", self.tab.filename()))
                        .small()
                        .color(crate::ui::muted_text_color(ui)),
                );
                let mut copy = false;
                let mut persist = false;
                ui.horizontal(|ui| {
                    copy = ui.button("Copier").clicked();
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        persist = ui.button("Enregistrer").clicked();
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        persist = ui.button("Télécharger").clicked();
                    }
                });
                if copy || persist {
                    match self.export_payload(document) {
                        Ok(Some((filename, contents))) => {
                            if copy {
                                ui.ctx().copy_text(contents.clone());
                                status.success("CSV copié dans le presse-papiers.");
                            }
                            if persist {
                                #[cfg(not(target_arch = "wasm32"))]
                                {
                                    let destination = if self.export_path.trim().is_empty() {
                                        filename.as_str()
                                    } else {
                                        self.export_path.trim()
                                    };
                                    match export::save_csv(destination, &contents) {
                                        Ok(()) => status.success(format!(
                                            "CSV enregistré dans « {destination} »."
                                        )),
                                        Err(error) => status.error(error.to_string()),
                                    }
                                }
                                #[cfg(target_arch = "wasm32")]
                                {
                                    let download_name = if self.export_path.trim().is_empty() {
                                        filename.as_str()
                                    } else {
                                        self.export_path.trim()
                                    };
                                    match export::download_csv(download_name, &contents) {
                                        Ok(()) => status.success(format!(
                                            "Téléchargement de « {download_name} »."
                                        )),
                                        Err(error) => status.error(error.to_string()),
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            status.error("Aucune donnée à exporter dans cet onglet.");
                        }
                        Err(error) => status.error(error.user_message()),
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.label("Destination");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.export_path)
                            .desired_width(f32::INFINITY)
                            .hint_text("nom proposé par l'onglet"),
                    );
                }
                #[cfg(target_arch = "wasm32")]
                {
                    ui.label("Nom du fichier");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.export_path)
                            .desired_width(f32::INFINITY)
                            .hint_text("nom proposé par l'onglet"),
                    );
                }
            });
    }

    fn export_payload(
        &self,
        document: &Document,
    ) -> Result<Option<(String, String)>, SimulationError> {
        match self.tab {
            InstrumentTab::Scope => {
                if self.scope_signals.is_empty() {
                    return Ok(None);
                }
                let Some(traces) = self.traces_for(document, &self.scope_signals)? else {
                    return Ok(None);
                };
                if traces.is_empty() {
                    return Ok(None);
                }
                Ok(Some((
                    InstrumentTab::Scope.filename().to_owned(),
                    transient_csv(&traces),
                )))
            }
            InstrumentTab::Spectrum => {
                let Some((trace, spectrum)) = self.spectrum_data()? else {
                    return Ok(None);
                };
                Ok(Some((
                    InstrumentTab::Spectrum.filename().to_owned(),
                    spectrum_csv(&trace, &spectrum),
                )))
            }
            InstrumentTab::Bode => {
                let Some(trace) = self.bode_data()? else {
                    return Ok(None);
                };
                Ok(Some((
                    InstrumentTab::Bode.filename().to_owned(),
                    bode_csv(&trace),
                )))
            }
        }
    }

    /// Returns the selected scope signals.
    pub fn selected_signals(&self) -> BTreeSet<SignalId> {
        self.selected.clone()
    }

    /// Returns traces for an arbitrary signal set.
    pub fn traces_for(
        &self,
        document: &Document,
        signals: &BTreeSet<SignalId>,
    ) -> Result<Option<Vec<Trace>>, SimulationError> {
        if let Some(run) = &self.realtime {
            if signals.is_empty() {
                return Ok(Some(Vec::new()));
            }
            let signals = signals.iter().cloned().collect::<Vec<_>>();
            let samples = run.samples();
            let latest = samples.last().map_or(0.0, |sample| sample.time);
            let start = latest - self.scope_window;
            let first = samples.partition_point(|sample| sample.time < start);
            return transient_traces_from_samples_with_document(
                &samples[first..],
                run.context(),
                document,
                &signals,
            )
            .map(Some);
        }
        let Some(run) = &self.transient_run else {
            return Ok(None);
        };
        let AnalysisResult::Transient(result) = &run.result else {
            return Ok(None);
        };
        if signals.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let signals = signals.iter().cloned().collect::<Vec<_>>();
        let latest = result.samples.last().map_or(0.0, |sample| sample.time);
        let start = latest - self.scope_window;
        let first = result.samples.partition_point(|sample| sample.time < start);
        transient_traces_from_samples_with_document(
            &result.samples[first..],
            &run.context,
            document,
            &signals,
        )
        .map(Some)
    }

    fn spectrum_data(&self) -> Result<Option<(Trace, Spectrum)>, SimulationError> {
        let Some(signal) = &self.spectrum_signal else {
            return Ok(None);
        };
        if let Some(run) = &self.realtime {
            let trace = transient_traces_from_samples(
                run.samples(),
                run.context(),
                std::slice::from_ref(signal),
            )?
            .into_iter()
            .next()
            .ok_or(SimulationError::NoData)?;
            let step = run
                .effective_step()
                .ok_or(SimulationError::InvalidSpectrumStep)?;
            let spectrum = compute_spectrum(&trace, step)?;
            return Ok(Some((trace, spectrum)));
        }
        let Some(run) = &self.transient_run else {
            return Ok(None);
        };
        let AnalysisResult::Transient(result) = &run.result else {
            return Ok(None);
        };
        let step = run
            .context
            .nominal_step
            .ok_or(SimulationError::InvalidSpectrumStep)?;
        let trace = transient_traces(result, &run.context, std::slice::from_ref(signal))?
            .into_iter()
            .next()
            .ok_or(SimulationError::NoData)?;
        let spectrum = compute_spectrum(&trace, step)?;
        Ok(Some((trace, spectrum)))
    }

    fn bode_data(&self) -> Result<Option<BodeTrace>, SimulationError> {
        let Some(output) = &self.ac.output else {
            return Ok(None);
        };
        let Some(run) = &self.ac_run else {
            return Ok(None);
        };
        let AnalysisResult::Ac(result) = &run.result else {
            return Ok(None);
        };
        bode_trace(result, &run.context, &SignalId::Voltage(output.clone())).map(Some)
    }

    fn active_run(&self) -> Option<&CompletedRun> {
        match self.tab {
            InstrumentTab::Scope | InstrumentTab::Spectrum => self.transient_run.as_ref(),
            InstrumentTab::Bode => self.ac_run.as_ref(),
        }
    }

    fn current_revision(&self) -> Option<u64> {
        self.realtime
            .as_ref()
            .map(LiveSimulation::sample_revision)
            .or_else(|| self.active_run().map(|run| run.context.document_revision))
    }

    fn ensure_catalog(&mut self, catalog: &SignalCatalog) {
        self.transient.ensure_catalog(&catalog.sources);
        self.ac.ensure_catalog(catalog);
        let available = catalog.signals();
        if available.is_empty() {
            self.selection_initialized = false;
        }
        if !self.selection_initialized {
            self.selected = catalog
                .voltages
                .iter()
                .cloned()
                .map(SignalId::Voltage)
                .collect();
            if self.selected.is_empty() {
                self.selected = catalog
                    .currents
                    .iter()
                    .cloned()
                    .map(SignalId::Current)
                    .collect();
            }
            self.selection_initialized = !self.selected.is_empty();
        }
        self.selected.retain(|signal| catalog.contains(signal));
        if !self
            .spectrum_signal
            .as_ref()
            .is_some_and(|signal| catalog.contains(signal))
        {
            self.spectrum_signal = catalog
                .default_voltage()
                .map(SignalId::Voltage)
                .or_else(|| available.first().cloned());
        }
    }

    fn can_run(&self, document: &Document) -> bool {
        self.pending.is_none() && document.extraction().is_some()
    }

    #[cfg(test)]
    fn run_transient(&mut self, document: &Document, now: f64, status: &mut Status) {
        if self.pending.is_some() {
            status.error("Une analyse est déjà en cours.");
            return;
        }
        let Some(extraction) = document.extraction() else {
            status.error("Aucune netlist valide n'est disponible.");
            return;
        };
        let prepared = match prepare_transient(extraction, &self.transient, document.revision()) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.fail(status, error);
                return;
            }
        };
        let Some(client) = self.client.as_mut() else {
            status.error("Le service de calcul n'est pas disponible.");
            return;
        };
        match client.submit(prepared) {
            Ok(_) => {
                self.error = None;
                self.pending = Some(AnalysisKind::Transient);
                status.info("Calcul transitoire en cours…");
                self.poll(now, status);
            }
            Err(error) => self.fail(status, error),
        }
    }

    fn run_ac(&mut self, document: &Document, now: f64, status: &mut Status) {
        if self.pending.is_some() {
            status.error("Une analyse est déjà en cours.");
            return;
        }
        let Some(extraction) = document.extraction() else {
            status.error("Aucune netlist valide n'est disponible.");
            return;
        };
        let prepared = match prepare_ac(extraction, &self.ac, document.revision()) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.fail(status, error);
                return;
            }
        };
        let Some(client) = self.client.as_mut() else {
            status.error("Le service de calcul n'est pas disponible.");
            return;
        };
        match client.submit(prepared) {
            Ok(_) => {
                self.error = None;
                self.pending = Some(AnalysisKind::Ac);
                status.info("Calcul AC en cours…");
                self.poll(now, status);
            }
            Err(error) => self.fail(status, error),
        }
    }

    fn poll(&mut self, now: f64, status: &mut Status) {
        let Some(client) = self.client.as_mut() else {
            return;
        };
        let outcome = client.poll(now);
        match outcome {
            Ok(Some(run)) => {
                let message = completion_message(&run);
                match run.kind {
                    AnalysisKind::Transient => self.transient_run = Some(run),
                    AnalysisKind::Ac => self.ac_run = Some(run),
                    AnalysisKind::Dc => {}
                }
                self.pending = None;
                self.error = None;
                status.success(message);
            }
            Ok(None) => {}
            Err(error) => {
                self.pending = None;
                self.fail(status, error);
            }
        }
    }

    fn fail(&mut self, status: &mut Status, error: SimulationError) {
        let message = error.user_message();
        status.error(message.clone());
        self.error = Some(message);
        self.pending = None;
    }
}

impl Default for InstrumentsState {
    fn default() -> Self {
        Self::new()
    }
}

fn default_export_path() -> String {
    String::new()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RealtimeAction {
    None,
    Start,
    TogglePause,
}

#[allow(clippy::too_many_arguments)]
fn realtime_controls(
    ui: &mut Ui,
    settings: &mut TransientSettings,
    catalog: &SignalCatalog,
    can_run: bool,
    speed: &mut f64,
    paused: bool,
    running: bool,
    ratio: f64,
    lagging: bool,
    effective_step: Option<f64>,
) -> RealtimeAction {
    let mut action = RealtimeAction::None;
    ui.heading("Transitoire temps réel");
    ui.horizontal(|ui| {
        let state = if running {
            if paused {
                "Pause"
            } else {
                "En marche"
            }
        } else if can_run {
            "Arrêtée"
        } else {
            "Circuit invalide"
        };
        ui.label(RichText::new(state).strong());
        let fine = effective_step.is_some_and(|step| step < 1e-3);
        if lagging && fine {
            let factor = if ratio > 0.005 { 1.0 / ratio } else { 200.0 };
            ui.label(
                RichText::new(if factor >= 200.0 {
                    "ralenti ×200+".to_owned()
                } else {
                    format!("ralenti ×{factor:.0}")
                })
                .small()
                .color(crate::ui::muted_text_color(ui)),
            )
            .on_hover_text(
                "Pas fin : le temps simulé avance moins vite que l'horloge, c'est le \
                 prix du détail. Baissez la vitesse pour revenir à 100 % du temps réel.",
            );
        } else if lagging {
            ui.label(
                RichText::new(format!("trop lent (×{:.1})", ratio))
                    .small()
                    .color(ERROR),
            )
            .on_hover_text(
                "Chaque image est limitée à 5 ms de calcul. Réduisez la vitesse, \
                 augmentez le pas ou simplifiez le circuit.",
            );
        } else if running {
            ui.label(
                RichText::new(format!("{:.0} % du temps réel", ratio * 100.0))
                    .small()
                    .color(crate::ui::muted_text_color(ui)),
            );
        }
    });
    egui::Grid::new("realtime_time_fields")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Pas");
            ui.add(
                egui::TextEdit::singleline(&mut settings.step)
                    .desired_width(86.0)
                    .hint_text("10u"),
            );
            ui.end_row();
        });
    ui.checkbox(&mut settings.step_enabled, "Échelon de source")
        .on_hover_text(
            "Force la source à sa valeur initiale, puis applique la valeur finale \
             à l'instant indiqué.",
        );
    if settings.step_enabled {
        if catalog.sources.is_empty() {
            ui.label(
                RichText::new("Aucune source indépendante.").color(crate::ui::muted_text_color(ui)),
            );
        } else {
            let previous_source = settings.source.clone();
            egui::ComboBox::from_id_salt("transient_step_source")
                .selected_text(settings.source.as_deref().unwrap_or("—"))
                .show_ui(ui, |ui| {
                    for source in &catalog.sources {
                        ui.selectable_value(
                            &mut settings.source,
                            Some(source.label.clone()),
                            format!("{} · {}", source.label, source.formatted_dc()),
                        );
                    }
                });
            if settings.source != previous_source {
                if let Some(source) = settings
                    .source
                    .as_ref()
                    .and_then(|label| catalog.sources.iter().find(|source| &source.label == label))
                {
                    settings.final_value = source.formatted_dc();
                }
            }
            let unit = settings
                .source
                .as_ref()
                .and_then(|label| catalog.sources.iter().find(|source| &source.label == label))
                .map_or(Unit::Volt, |source| source.unit);
            egui::Grid::new("transient_step_fields")
                .num_columns(4)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Initiale");
                    ui.add(
                        egui::TextEdit::singleline(&mut settings.initial_value)
                            .desired_width(70.0)
                            .hint_text(format!("0 {}", unit.symbol())),
                    );
                    ui.label("Instant");
                    ui.add(
                        egui::TextEdit::singleline(&mut settings.event_time)
                            .desired_width(70.0)
                            .hint_text("0"),
                    );
                    ui.end_row();
                    ui.label("Finale");
                    ui.add(
                        egui::TextEdit::singleline(&mut settings.final_value)
                            .desired_width(70.0)
                            .hint_text(format!("5 {}", unit.symbol())),
                    );
                    ui.label(" ");
                    ui.end_row();
                });
        }
    }
    ui.horizontal(|ui| {
        ui.label("Vitesse");
        egui::ComboBox::from_id_salt("realtime_speed")
            .selected_text(format!("{speed:.2}×"))
            .show_ui(ui, |ui| {
                for candidate in LIVE_SPEEDS {
                    ui.selectable_value(speed, candidate, format!("{candidate:.2}×"));
                }
            });
        if running
            && ui
                .button(if paused { "Reprendre" } else { "Pause" })
                .clicked()
        {
            action = RealtimeAction::TogglePause;
        }
    });
    if ui
        .add_enabled(
            can_run,
            egui::Button::new(if running {
                "Redémarrer le temps réel"
            } else {
                "Démarrer le temps réel"
            }),
        )
        .on_hover_text("Aucune plage d’arrêt : le tampon avance avec l’horloge")
        .clicked()
    {
        action = RealtimeAction::Start;
    }
    action
}

fn ac_controls(
    ui: &mut Ui,
    settings: &mut AcSettings,
    catalog: &SignalCatalog,
    can_run: bool,
) -> bool {
    ui.heading("Analyse AC");
    egui::Grid::new("ac_sweep_fields")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Fréquence début");
            ui.add(
                egui::TextEdit::singleline(&mut settings.start)
                    .desired_width(86.0)
                    .hint_text("1"),
            );
            ui.end_row();
            ui.label("Fréquence fin");
            ui.add(
                egui::TextEdit::singleline(&mut settings.stop)
                    .desired_width(86.0)
                    .hint_text("1M"),
            );
            ui.end_row();
            ui.label("Points");
            ui.add(
                egui::DragValue::new(&mut settings.points)
                    .range(2..=MAX_AC_POINTS)
                    .speed(1.0),
            );
            ui.end_row();
        });
    ui.label("Source excitée");
    if catalog.sources.is_empty() {
        ui.label(
            RichText::new("Aucune source indépendante.").color(crate::ui::muted_text_color(ui)),
        );
    } else {
        egui::ComboBox::from_id_salt("ac_source")
            .selected_text(settings.source.as_deref().unwrap_or("—"))
            .show_ui(ui, |ui| {
                for source in &catalog.sources {
                    ui.selectable_value(
                        &mut settings.source,
                        Some(source.label.clone()),
                        format!("{} · {}", source.label, source.formatted_dc()),
                    );
                }
            });
        let unit = settings
            .source
            .as_ref()
            .and_then(|label| catalog.sources.iter().find(|source| &source.label == label))
            .map_or(Unit::Volt, |source| source.unit);
        ui.horizontal(|ui| {
            ui.label(format!("Amplitude ({})", unit.symbol()));
            ui.add(
                egui::TextEdit::singleline(&mut settings.amplitude)
                    .desired_width(70.0)
                    .hint_text("1"),
            );
        });
        ui.label(
            RichText::new("Le gain est normalisé par l'amplitude d'excitation.")
                .small()
                .color(crate::ui::muted_text_color(ui)),
        );
    }
    ui.add_enabled(can_run, egui::Button::new("Lancer l'analyse AC"))
        .clicked()
}

fn spectrum_signal_control(ui: &mut Ui, signal: &mut Option<SignalId>, catalog: &SignalCatalog) {
    ui.label("Signal analysé");
    let available = catalog.signals();
    if available.is_empty() {
        ui.label(RichText::new("Aucun signal disponible.").color(crate::ui::muted_text_color(ui)));
        *signal = None;
        return;
    }
    let selected_text = signal
        .as_ref()
        .map_or_else(|| "—".to_owned(), SignalId::label);
    egui::ComboBox::from_id_salt("spectrum_signal")
        .selected_text(selected_text)
        .show_ui(ui, |ui| {
            for option in available {
                ui.selectable_value(signal, Some(option.clone()), option.label());
            }
        });
}

fn bode_output_control(ui: &mut Ui, output: &mut Option<String>, catalog: &SignalCatalog) {
    ui.label("Tension de sortie");
    if catalog.voltages.is_empty() {
        ui.label(
            RichText::new("Aucune tension disponible.").color(crate::ui::muted_text_color(ui)),
        );
        *output = None;
        return;
    }
    egui::ComboBox::from_id_salt("bode_output")
        .selected_text(output.as_deref().unwrap_or("—"))
        .show_ui(ui, |ui| {
            for name in &catalog.voltages {
                ui.selectable_value(output, Some(name.clone()), name.as_str());
            }
        });
}

pub(crate) fn nearest_sample_time(times: &[f64], target: f64) -> Option<f64> {
    let index = times.partition_point(|time| *time < target);
    let left = index.checked_sub(1).and_then(|index| times.get(index));
    let right = times.get(index);
    match (left, right) {
        (Some(left), Some(right)) => {
            if (target - *left).abs() <= (*right - target).abs() {
                Some(*left)
            } else {
                Some(*right)
            }
        }
        (Some(left), None) => Some(*left),
        (None, Some(right)) => Some(*right),
        (None, None) => None,
    }
}

pub(crate) fn value_at(points: &[[f64; 2]], time: f64) -> Option<f64> {
    let index = points.partition_point(|point| point[0] < time);
    let left = index.checked_sub(1).and_then(|index| points.get(index));
    let right = points.get(index);
    match (left, right) {
        (Some(left), Some(right)) => {
            if (time - left[0]).abs() <= (right[0] - time).abs() {
                Some(left[1])
            } else {
                Some(right[1])
            }
        }
        (Some(left), None) => Some(left[1]),
        (None, Some(right)) => Some(right[1]),
        (None, None) => None,
    }
}

fn nearest_bode_point(points: &[BodePoint], frequency: f64) -> Option<&BodePoint> {
    let index = points.partition_point(|point| point.frequency < frequency);
    let left = index.checked_sub(1).and_then(|index| points.get(index));
    let right = points.get(index);
    match (left, right) {
        (Some(left), Some(right)) => {
            if (frequency - left.frequency).abs() <= (right.frequency - frequency).abs() {
                Some(left)
            } else {
                Some(right)
            }
        }
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn paint_vertical_marker(
    ui: &Ui,
    response: &egui_plot::PlotResponse<()>,
    x: f64,
    label: &str,
    color: Color32,
) {
    let frame = *response.transform.frame();
    if !frame.is_positive() {
        return;
    }
    let screen_x = response.transform.position_from_point_x(x);
    if screen_x < frame.left() || screen_x > frame.right() {
        return;
    }
    let painter = ui.painter().with_clip_rect(frame);
    painter.line_segment(
        [
            egui::pos2(screen_x, frame.top()),
            egui::pos2(screen_x, frame.bottom()),
        ],
        egui::Stroke::new(1.3, color),
    );
    painter.text(
        egui::pos2(
            (screen_x + 4.0).min(frame.right() - 10.0),
            frame.top() + 4.0,
        ),
        Align2::LEFT_TOP,
        label,
        FontId::monospace(11.0),
        color,
    );
}

fn completion_message(run: &CompletedRun) -> String {
    match &run.result {
        AnalysisResult::Transient(result) => format!(
            "Transitoire terminé : {} échantillons, {} pas, {} itérations.",
            result.samples.len(),
            result.steps,
            result.iterations
        ),
        AnalysisResult::Ac(result) => {
            format!("Analyse AC terminée : {} fréquences.", result.samples.len())
        }
        AnalysisResult::Dc(result) => format!(
            "Point de fonctionnement calculé : {} nœuds.",
            result.node_voltages.len()
        ),
    }
}

pub(crate) fn common_unit(traces: &[Trace]) -> Option<SignalUnit> {
    let first = traces.first()?.unit();
    traces
        .iter()
        .all(|trace| trace.unit() == first)
        .then_some(first)
}

pub(crate) fn trace_color(index: usize) -> Color32 {
    TRACE_COLORS[index % TRACE_COLORS.len()]
}

pub(crate) fn format_axis(value: f64, unit: Option<SignalUnit>) -> String {
    unit.map_or_else(
        || format_plain(value),
        |unit| format_si_precision(value, unit.unit(), 3),
    )
}

fn format_plain(value: f64) -> String {
    if value != 0.0 && (value.abs() >= 1.0e4 || value.abs() < 1.0e-3) {
        format!("{value:.3e}")
    } else {
        format!("{value:.4}")
    }
}

pub(crate) fn format_frequency(value: f64) -> String {
    format_si_precision(value, Unit::Hertz, 3)
}

fn position_position(position: &HoverPosition<'_>) -> egui_plot::PlotPoint {
    match position {
        HoverPosition::NearDataPoint { position, .. } | HoverPosition::Elsewhere { position } => {
            *position
        }
    }
}

pub(crate) fn decimate(points: &[[f64; 2]], limit: usize) -> Vec<[f64; 2]> {
    if points.len() <= limit {
        return points.to_vec();
    }
    let stride = points.len().div_ceil(limit);
    let mut sampled = points.iter().step_by(stride).copied().collect::<Vec<_>>();
    if let Some(last) = points.last() {
        if sampled.last() != Some(last) {
            sampled.push(*last);
        }
    }
    sampled
}

fn bode_grid(input: egui_plot::GridInput) -> Vec<egui_plot::GridMark> {
    let span = (input.bounds.1 - input.bounds.0).abs();
    let major = if span >= 4.0 { 1.0 } else { 0.5 };
    let mut marks = Vec::new();
    let start = (input.bounds.0.min(input.bounds.1) / major).ceil() as i64;
    let end = (input.bounds.0.max(input.bounds.1) / major).floor() as i64;
    for step in start..=end {
        let value = step as f64 * major;
        marks.push(egui_plot::GridMark {
            value,
            step_size: major,
        });
    }
    marks
}

fn empty_plot_message(ui: &mut Ui, message: &str) {
    ui.add_space(16.0);
    ui.label(RichText::new(message).color(crate::ui::muted_text_color(ui)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Frame, Pos2, RawInput, Rect};

    fn frame(
        context: &egui::Context,
        state: &mut InstrumentsState,
        document: &Document,
    ) -> egui::FullOutput {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 600.0))),
            focused: true,
            ..Default::default()
        };
        let mut status = Status::new_info("");
        context.run_ui(input, |ui| {
            egui::CentralPanel::default()
                .frame(Frame::NONE)
                .show(ui, |ui| {
                    let mut scopes = InlineScopes::with_signals(state.selected_signals());
                    state.show(
                        ui,
                        &mut scopes,
                        document,
                        &mut status,
                        crate::overcurrent::DEFAULT_WIRE_CURRENT_LIMIT,
                    );
                });
        })
    }

    #[test]
    fn automatic_refresh_is_enabled_by_default() {
        assert!(InstrumentsState::new().auto_refresh());
    }

    #[test]
    fn scope_window_defaults_to_ten_seconds_and_is_clamped() {
        let mut state = InstrumentsState::new();
        assert!((state.scope_window() - 10.0).abs() < f64::EPSILON);
        state.set_scope_window(2.5);
        assert!((state.scope_window() - 2.5).abs() < f64::EPSILON);
        state.set_scope_window(1e-6);
        assert!((state.scope_window() - 1e-3).abs() < f64::EPSILON);
        state.set_scope_window(100.0);
        assert!((state.scope_window() - 10.0).abs() < f64::EPSILON);
        state.set_scope_window(f64::NAN);
        assert!((state.scope_window() - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn live_step_field_accepts_five_microseconds() {
        assert!((crate::analysis::parse_live_step("5u").unwrap() - 5e-6).abs() < 1e-12);
        assert!((crate::analysis::parse_live_step("1m").unwrap() - 1e-3).abs() < 1e-12);
        assert!(crate::analysis::parse_live_step("0").is_err());
        assert!(crate::analysis::parse_live_step("abc").is_err());
    }

    #[test]
    fn defaults_select_demo_voltages() {
        let context = egui::Context::default();
        let mut state = InstrumentsState::new();
        let document = Document::demo().unwrap();
        let catalog = SignalCatalog::from_extraction(document.extraction().unwrap());
        frame(&context, &mut state, &document);
        let expected = catalog
            .voltages
            .iter()
            .cloned()
            .map(SignalId::Voltage)
            .collect::<BTreeSet<_>>();
        assert_eq!(state.selected, expected);
        assert_eq!(
            state.spectrum_signal,
            catalog.default_voltage().map(SignalId::Voltage)
        );
        assert_eq!(state.ac.output.as_deref(), Some("Vc"));
        assert_eq!(state.transient.final_value, "10 V");
    }

    #[test]
    fn clearing_a_schematic_resets_automatic_selection() {
        let mut state = InstrumentsState::new();
        let catalog = SignalCatalog {
            voltages: vec!["OUT".to_owned()],
            currents: Vec::new(),
            components: Vec::new(),
            sources: Vec::new(),
        };
        state.ensure_catalog(&catalog);
        assert_eq!(state.selected.len(), 1);
        state.selected.clear();
        state.ensure_catalog(&catalog);
        assert!(state.selected.is_empty());
        state.ensure_catalog(&SignalCatalog::default());
        assert!(!state.selection_initialized);
    }

    #[test]
    fn bode_marker_snaps_to_the_nearest_frequency() {
        let points = vec![
            BodePoint {
                frequency: 10.0,
                magnitude: 1.0,
                gain_db: 0.0,
                phase_degrees: 0.0,
            },
            BodePoint {
                frequency: 100.0,
                magnitude: 0.5,
                gain_db: -6.0,
                phase_degrees: -45.0,
            },
        ];
        assert_eq!(
            nearest_bode_point(&points, 74.0).map(|point| point.frequency),
            Some(100.0)
        );
        assert_eq!(
            nearest_bode_point(&points, 12.0).map(|point| point.frequency),
            Some(10.0)
        );
    }

    #[test]
    fn realtime_session_advances_and_pauses() {
        let mut state = InstrumentsState::new();
        let document = Document::demo().unwrap();
        let catalog = SignalCatalog::from_extraction(document.extraction().unwrap());
        state.ensure_catalog(&catalog);
        let mut status = Status::new_info("");
        state.start_realtime(&document, 0.0, &mut status);
        assert!(state.realtime.is_some());
        let start = state.transient_time().unwrap();
        state.tick_realtime(&document, 0.01, &mut status);
        let advanced = state.transient_time().unwrap();
        assert!(advanced > start);
        state.set_realtime_paused(true);
        state.tick_realtime(&document, 0.02, &mut status);
        assert_eq!(state.transient_time(), Some(advanced));
        state.step_realtime(&document, &mut status);
        assert!(state.transient_time().unwrap() > advanced);
        let traces = state
            .traces_for(&document, &state.selected)
            .unwrap()
            .unwrap();
        assert!(!traces.is_empty());
        assert!(state.live_values().time.is_some());
    }

    #[test]
    fn decimation_preserves_both_ends() {
        let points = (0..10_000)
            .map(|index| [index as f64, index as f64 * 2.0])
            .collect::<Vec<_>>();
        let sampled = decimate(&points, 1024);
        assert!(sampled.len() <= 1025);
        assert_eq!(sampled.first(), Some(&[0.0, 0.0]));
        assert_eq!(sampled.last(), Some(&[9999.0, 19998.0]));
    }

    #[test]
    fn instrument_tabs_have_export_names() {
        assert_eq!(InstrumentTab::Scope.filename(), "circuitsim-portee.csv");
        assert_eq!(InstrumentTab::Bode.label(), "Bode");
    }

    #[test]
    fn instrument_runs_and_exports_transient_data() {
        let mut state = InstrumentsState::new();
        let document = Document::demo().unwrap();
        let catalog = SignalCatalog::from_extraction(document.extraction().unwrap());
        state.ensure_catalog(&catalog);
        let mut status = Status::new_info("");
        state.run_transient(&document, 1.0, &mut status);
        assert!(state.pending.is_none());
        assert!(state.error.is_none());
        assert!(state.transient_run.is_some());
        assert_eq!(state.transient_revision(), Some(document.revision()));
        assert!(state.transient_time().is_some_and(|time| time > 0.0));
        assert!(!state.is_busy());
        assert!(state.can_simulate(&document));
        state.scope_signals = state.selected.clone();
        let (filename, csv) = state.export_payload(&document).unwrap().unwrap();
        assert_eq!(filename, "circuitsim-portee.csv");
        assert!(csv.starts_with("signal,unit,time_s,value\n"));
        assert!(state.spectrum_data().unwrap().is_some());
        state.tab = InstrumentTab::Spectrum;
        let (filename, csv) = state.export_payload(&document).unwrap().unwrap();
        assert_eq!(filename, "circuitsim-spectre.csv");
        assert!(csv.starts_with("signal,unit,frequency_hz,magnitude\n"));
    }

    #[test]
    fn bode_tab_stacks_gain_above_phase() {
        let context = egui::Context::default();
        let mut state = InstrumentsState::new();
        let document = Document::demo().unwrap();
        let catalog = SignalCatalog::from_extraction(document.extraction().unwrap());
        state.ensure_catalog(&catalog);
        state.tab = InstrumentTab::Bode;
        let mut status = Status::new_info("");
        state.run_ac(&document, 1.0, &mut status);
        assert!(state.ac_run.is_some());

        let output = frame(&context, &mut state, &document);
        let label_y = |needle: &str| {
            output.shapes.iter().find_map(|clipped| {
                let egui::Shape::Text(text) = &clipped.shape else {
                    return None;
                };
                text.galley.text().contains(needle).then_some(text.pos.y)
            })
        };
        let gain_y = label_y("Gain (dB)").expect("gain axis label rendered");
        let phase_y = label_y("Phase (°)").expect("phase axis label rendered");
        assert!(
            phase_y > gain_y + 20.0,
            "phase plot must sit below gain (gain y {gain_y}, phase y {phase_y})"
        );
    }

    #[test]
    fn instrument_runs_and_exports_ac_data() {
        let mut state = InstrumentsState::new();
        let document = Document::demo().unwrap();
        let catalog = SignalCatalog::from_extraction(document.extraction().unwrap());
        state.ensure_catalog(&catalog);
        state.tab = InstrumentTab::Bode;
        let mut status = Status::new_info("");
        state.run_ac(&document, 1.0, &mut status);
        assert!(state.pending.is_none());
        assert!(state.error.is_none());
        assert!(state.ac_run.is_some());
        let trace = state.bode_data().unwrap().unwrap();
        assert_eq!(trace.points.len(), 101);
        let (filename, csv) = state.export_payload(&document).unwrap().unwrap();
        assert_eq!(filename, "circuitsim-bode.csv");
        assert!(csv.starts_with("signal,frequency_hz,magnitude_v,gain_db,phase_deg\n"));
    }
}
