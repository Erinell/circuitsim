//! Simulation controls, scope, FFT, Bode plots, and CSV export.

use std::collections::BTreeSet;
use std::time::Duration;

use editor_core::{SymbolId, SymbolKind};
use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, RichText, Ui, Vec2};
use egui_plot::{Corner, HoverPosition, Legend, Line, Plot};
use sim_core::units::{format_si_precision, Unit};
use sim_core::{TransientSample, TransientSession};
use worker::protocol::{AnalysisKind, AnalysisResult};

#[cfg(test)]
use crate::analysis::prepare_transient;
use crate::analysis::{
    analysis_name, bode_csv, bode_trace, compute_spectrum, measurements, prepare_ac,
    prepare_realtime, probe_readings, spectrum_csv, transient_csv, transient_traces,
    transient_traces_from_samples, AcSettings, BodePoint, BodeTrace, CompletedRun, SignalCatalog,
    SignalId, SignalUnit, SimulationClient, SimulationError, Spectrum, Trace, TransientSettings,
    MAX_AC_POINTS,
};
use crate::document::Document;
use crate::export;
use crate::ui::{Status, ACCENT, HOVER, MUTED, SYMBOL};

const ERROR: Color32 = Color32::from_rgb(240, 128, 128);
const CONTROL_WIDTH: f32 = 304.0;
const MAX_PLOT_POINTS: usize = 4096;
const MAX_REALTIME_SAMPLES: usize = 100_000;
const MAX_REALTIME_STEPS_PER_FRAME: usize = 800;
const REALTIME_SPEEDS: [f64; 6] = [0.25, 0.5, 1.0, 2.0, 5.0, 10.0];

/// Live transient stream driven by wall-clock time.
struct RealtimeRun {
    session: TransientSession,
    context: crate::analysis::RunContext,
    revision: u64,
    samples: Vec<TransientSample>,
    last_wall: f64,
}

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
            Self::Scope => "Portée",
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
    realtime: Option<RealtimeRun>,
    realtime_speed: f64,
    realtime_paused: bool,
    scope_cursor_a: Option<f64>,
    scope_cursor_b: Option<f64>,
    scope_active_cursor: CursorSlot,
    bode_cursor: Option<f64>,
    hidden_probes: BTreeSet<SymbolId>,
    export_path: String,
}

/// Active scope cursor selected by the controls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CursorSlot {
    #[default]
    A,
    B,
}

impl CursorSlot {
    const fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
        }
    }
}

/// Values read at the two time cursors for one trace.
#[derive(Clone, Debug, PartialEq)]
struct CursorMeasurement {
    signal: SignalId,
    value_a: Option<f64>,
    value_b: Option<f64>,
    delta: Option<f64>,
    interval_min: Option<f64>,
    interval_max: Option<f64>,
    interval_rms: Option<f64>,
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
            auto_refresh: false,
            realtime: None,
            realtime_speed: 1.0,
            realtime_paused: false,
            scope_cursor_a: None,
            scope_cursor_b: None,
            scope_active_cursor: CursorSlot::A,
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
        self.realtime_paused = false;
        self.selection_initialized = false;
        self.scope_cursor_a = None;
        self.scope_cursor_b = None;
        self.scope_active_cursor = CursorSlot::A;
        self.bode_cursor = None;
    }

    /// Returns the last transient sample mapped to names for the inspector.
    pub fn live_values(&self) -> crate::analysis::LiveValues {
        if let Some(run) = &self.realtime {
            let Some(sample) = run.samples.last() else {
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
            return crate::analysis::LiveValues {
                document_revision: run.revision,
                time: Some(sample.time),
                node_voltages,
                branch_currents,
            };
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
            return Some(run.revision);
        }
        self.transient_run
            .as_ref()
            .map(|run| run.context.document_revision)
    }

    /// Returns the time of the last transient sample.
    pub fn transient_time(&self) -> Option<f64> {
        if let Some(run) = &self.realtime {
            return run.samples.last().map(|sample| sample.time);
        }
        let run = self.transient_run.as_ref()?;
        let AnalysisResult::Transient(result) = &run.result else {
            return None;
        };
        result.samples.last().map(|sample| sample.time)
    }

    /// Returns whether edits trigger an automatic transient refresh.
    pub const fn auto_refresh(&self) -> bool {
        self.auto_refresh
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
        self.start_realtime(document, now, status);
    }

    /// Starts or restarts the open-ended real-time session.
    pub fn start_realtime(&mut self, document: &Document, now: f64, status: &mut Status) {
        if self.pending.is_some() {
            status.error("Une analyse est déjà en cours.");
            return;
        }
        let Some(extraction) = document.extraction() else {
            status.error("Aucune netlist valide n'est disponible.");
            return;
        };
        let setup = match prepare_realtime(extraction, &self.transient, document.revision()) {
            Ok(setup) => setup,
            Err(error) => {
                self.fail(status, error);
                return;
            }
        };
        let mut session = match TransientSession::new(&setup.netlist, &setup.options) {
            Ok(session) => session,
            Err(error) => {
                self.fail(
                    status,
                    SimulationError::Transient(format!("démarrage temps réel : {error}")),
                );
                return;
            }
        };
        let samples = session.drain_samples();
        self.realtime = Some(RealtimeRun {
            session,
            context: setup.context,
            revision: document.revision(),
            samples,
            last_wall: now,
        });
        self.realtime_paused = false;
        self.error = None;
        status.info("Simulation temps réel démarrée.");
    }

    /// Returns whether a real-time transient session exists.
    pub const fn realtime_active(&self) -> bool {
        self.realtime.is_some()
    }

    /// Advances the real-time session using the host clock.
    pub fn tick_realtime(&mut self, now: f64, status: &mut Status) {
        self.advance_realtime(now, status);
    }

    /// Pauses or resumes the current real-time session.
    pub fn set_realtime_paused(&mut self, paused: bool) {
        self.realtime_paused = paused;
    }

    /// Returns whether the real-time session is paused.
    pub const fn realtime_paused(&self) -> bool {
        self.realtime_paused
    }

    /// Returns the playback speed multiplier.
    pub const fn realtime_speed(&self) -> f64 {
        self.realtime_speed
    }

    /// Sets the playback speed multiplier.
    pub fn set_realtime_speed(&mut self, speed: f64) {
        if speed.is_finite() && speed > 0.0 {
            self.realtime_speed = speed;
        }
    }

    fn advance_realtime(&mut self, now: f64, status: &mut Status) {
        let Some(run) = self.realtime.as_mut() else {
            return;
        };
        if self.realtime_paused {
            run.last_wall = now;
            return;
        }
        let elapsed = (now - run.last_wall).max(0.0);
        run.last_wall = now;
        let target = run.session.time() + elapsed * self.realtime_speed;
        let mut steps = 0;
        while run.session.time() < target && steps < MAX_REALTIME_STEPS_PER_FRAME {
            if let Err(error) = run.session.step() {
                let message = format!("simulation temps réel : {error}");
                self.error = Some(message.clone());
                status.error(message);
                self.realtime = None;
                return;
            }
            steps += 1;
        }
        let drained = run.session.drain_samples();
        run.samples.extend(drained);
        if run.samples.len() > MAX_REALTIME_SAMPLES {
            let excess = run.samples.len() - MAX_REALTIME_SAMPLES;
            run.samples.drain(0..excess);
        }
    }

    /// Draws the bottom instrument panel.
    pub fn show(&mut self, ui: &mut Ui, document: &Document, status: &mut Status) {
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
        if self.realtime.is_some() && !self.realtime_paused {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        self.tab_bar(ui, document);
        ui.separator();
        if let Some(error) = &self.error {
            ui.colored_label(ERROR, error);
        }

        let height = ui.available_height().max(120.0);
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
            let plot_size = Vec2::new(ui.available_width(), height);
            ui.allocate_ui_with_layout(plot_size, Layout::top_down(Align::Min), |ui| {
                self.plots(ui);
            });
        });
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
                        .color(MUTED),
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
                    .color(MUTED),
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
            InstrumentTab::Scope => {
                self.scope_controls(ui, document, catalog, now, status);
            }
            InstrumentTab::Spectrum => {
                self.spectrum_controls(ui, document, catalog, now, status);
            }
            InstrumentTab::Bode => {
                self.bode_controls(ui, document, catalog, now, status);
            }
        }
        ui.separator();
        self.export_controls(ui, status);
    }

    fn scope_controls(
        &mut self,
        ui: &mut Ui,
        document: &Document,
        catalog: &SignalCatalog,
        now: f64,
        status: &mut Status,
    ) {
        let traces = self.current_traces();
        if let Ok(Some(available)) = &traces {
            if !available.is_empty() {
                self.scope_cursor_controls(ui, available);
                ui.separator();
            }
        }
        let can_run = self.can_run(document);
        let action = realtime_controls(
            ui,
            &mut self.transient,
            catalog,
            can_run,
            &mut self.realtime_speed,
            &mut self.realtime_paused,
            self.realtime.is_some(),
        );
        if matches!(action, RealtimeAction::Start) {
            self.start_realtime(document, now, status);
        } else if matches!(action, RealtimeAction::TogglePause) {
            self.realtime_paused = !self.realtime_paused;
        }
        ui.separator();
        signal_controls(ui, &mut self.selected, catalog);
        ui.separator();
        self.probe_controls(ui, document, catalog);
        ui.separator();
        ui.heading("Mesures");
        match traces {
            Ok(Some(traces)) if !traces.is_empty() => {
                measurement_table(ui, &traces);
            }
            Ok(_) => {
                ui.label(
                    RichText::new("Lancez le transitoire pour mesurer les signaux.").color(MUTED),
                );
            }
            Err(error) => {
                ui.colored_label(ERROR, error.user_message());
            }
        }
    }

    fn probe_controls(&mut self, ui: &mut Ui, document: &Document, catalog: &SignalCatalog) {
        let readings = probe_readings(document, &self.live_values());
        if readings.is_empty() {
            ui.label(
                RichText::new(
                    "Placez un voltmètre, un ampèremètre ou un ohmmètre depuis la palette.",
                )
                .small()
                .color(MUTED),
            );
            return;
        }
        ui.heading("Sondes");
        for reading in readings {
            ui.horizontal(|ui| {
                let mut visible = self.probe_visible(reading.id);
                if ui
                    .checkbox(&mut visible, &reading.label)
                    .on_hover_text("Afficher ou masquer la mesure sur le canevas")
                    .changed()
                {
                    self.set_probe_visible(reading.id, visible);
                }
                ui.label(
                    RichText::new(symbol_kind_short(&reading.kind))
                        .small()
                        .color(MUTED),
                );
            });
            let value = reading.value.map_or_else(
                || "—".to_owned(),
                |value| {
                    format_si_precision(
                        value,
                        reading.unit.unwrap_or(sim_core::units::Unit::Volt),
                        4,
                    )
                },
            );
            ui.label(
                RichText::new(format!("Valeur : {value}"))
                    .small()
                    .color(MUTED),
            );
            if let Some(signal) = reading.signal() {
                if catalog.contains(&signal) {
                    let mut plotted = self.selected.contains(&signal);
                    if ui
                        .checkbox(&mut plotted, "Graphique")
                        .on_hover_text("Ajouter ou retirer ce signal de la portée")
                        .changed()
                    {
                        if plotted {
                            self.selected.insert(signal);
                        } else {
                            self.selected.remove(&signal);
                        }
                    }
                }
            }
        }
        ui.label(
            RichText::new(
                "Le voltmètre se pose sur un fil ; l’ampèremètre et l’ohmmètre \
                 mesurent le composant sous la sonde.",
            )
            .small()
            .color(MUTED),
        );
    }

    fn scope_cursor_controls(&mut self, ui: &mut Ui, traces: &[Trace]) {
        ui.heading("Curseurs");
        ui.horizontal(|ui| {
            for slot in [CursorSlot::A, CursorSlot::B] {
                ui.selectable_value(&mut self.scope_active_cursor, slot, slot.label());
            }
            if ui
                .small_button("Effacer")
                .on_hover_text("Retire les deux curseurs")
                .clicked()
            {
                self.scope_cursor_a = None;
                self.scope_cursor_b = None;
            }
        });
        ui.label(
            RichText::new("Clic : premier A, deuxième B, puis curseur actif · droit/Maj : B")
                .small()
                .color(MUTED),
        );
        for (slot, value) in [
            (CursorSlot::A, self.scope_cursor_a),
            (CursorSlot::B, self.scope_cursor_b),
        ] {
            if let Some(time) = value {
                ui.label(format!(
                    "{} : t = {}",
                    slot.label(),
                    format_si_precision(time, Unit::Second, 4)
                ));
            }
        }
        if let (Some(a), Some(b)) = (self.scope_cursor_a, self.scope_cursor_b) {
            let delta = (b - a).abs();
            let frequency = if delta > 0.0 { 1.0 / delta } else { 0.0 };
            ui.label(format!(
                "Δt = {} · 1/Δt = {}",
                format_si_precision(delta, Unit::Second, 4),
                format_si_precision(frequency, Unit::Hertz, 4)
            ));
        }
        for measurement in cursor_measurements(traces, self.scope_cursor_a, self.scope_cursor_b) {
            let unit = measurement.signal.unit().unit();
            let value = |value: Option<f64>| {
                value.map_or_else(
                    || "—".to_owned(),
                    |value| format_si_precision(value, unit, 4),
                )
            };
            ui.label(RichText::new(measurement.signal.label()).strong());
            ui.label(
                RichText::new(format!(
                    "A {} · B {} · Δ {}",
                    value(measurement.value_a),
                    value(measurement.value_b),
                    value(measurement.delta)
                ))
                .small()
                .color(MUTED),
            );
            if measurement.interval_min.is_some() {
                ui.label(
                    RichText::new(format!(
                        "intervalle min {} · max {} · RMS {}",
                        value(measurement.interval_min),
                        value(measurement.interval_max),
                        value(measurement.interval_rms)
                    ))
                    .small()
                    .color(MUTED),
                );
            }
        }
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
        let action = realtime_controls(
            ui,
            &mut self.transient,
            catalog,
            can_run,
            &mut self.realtime_speed,
            &mut self.realtime_paused,
            self.realtime.is_some(),
        );
        if matches!(action, RealtimeAction::Start) {
            self.start_realtime(document, now, status);
        } else if matches!(action, RealtimeAction::TogglePause) {
            self.realtime_paused = !self.realtime_paused;
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
                    ui.label(RichText::new("Aucune composante non continue.").color(MUTED));
                }
                ui.label(
                    RichText::new(format!(
                        "{} points · résolution {}",
                        spectrum.sample_count,
                        format_frequency(spectrum.resolution)
                    ))
                    .small()
                    .color(MUTED),
                );
            }
            Ok(None) => {
                ui.label(
                    RichText::new("Lancez le transitoire pour calculer le spectre.").color(MUTED),
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
                        .color(MUTED),
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
                            .color(MUTED),
                    );
                }
            }
            Ok(None) => {
                ui.label(RichText::new("Lancez l'analyse AC pour tracer le Bode.").color(MUTED));
            }
            Err(error) => {
                ui.colored_label(ERROR, error.user_message());
            }
        }
    }

    fn plots(&mut self, ui: &mut Ui) {
        match self.tab {
            InstrumentTab::Scope => self.scope_plot(ui),
            InstrumentTab::Spectrum => self.spectrum_plot(ui),
            InstrumentTab::Bode => self.bode_plot(ui),
        }
    }

    fn scope_plot(&mut self, ui: &mut Ui) {
        let traces = match self.current_traces() {
            Ok(Some(traces)) => traces,
            Ok(None) => {
                empty_plot_message(ui, "Aucun résultat transitoire.");
                return;
            }
            Err(error) => {
                ui.colored_label(ERROR, error.user_message());
                return;
            }
        };
        if traces.is_empty() {
            empty_plot_message(ui, "Sélectionnez au moins un signal.");
            return;
        }
        let unit = common_unit(&traces);
        let revision = self.transient_run.as_ref().map_or(0, |run| run.revision);
        let height = ui.available_height().max(100.0);
        let sample_times = traces
            .first()
            .map(|trace| {
                trace
                    .points
                    .iter()
                    .map(|point| point[0])
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut clicked_time = None;
        let mut second_cursor = false;
        let y_formatter = move |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            format_axis(mark.value, unit)
        };
        let x_formatter = |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            format_si_precision(mark.value, Unit::Second, 3)
        };
        let response = Plot::new(format!("scope_plot_{revision}"))
            .height(height)
            .x_axis_label("Temps")
            .y_axis_label(match unit {
                Some(SignalUnit::Volt) => "Tension",
                Some(SignalUnit::Ampere) => "Courant",
                None => "Valeur",
            })
            .legend(
                Legend::default()
                    .position(Corner::LeftTop)
                    .follow_insertion_order(true),
            )
            .x_axis_formatter(x_formatter)
            .y_axis_formatter(y_formatter)
            .label_formatter(move |position| {
                let position = position_position(position);
                Some(format!(
                    "t = {}\n{}",
                    format_si_precision(position.x, Unit::Second, 4),
                    format_axis(position.y, unit)
                ))
            })
            .show(ui, |plot_ui| {
                for (index, trace) in traces.iter().enumerate() {
                    plot_ui.line(
                        Line::new(trace.label(), decimate(&trace.points, MAX_PLOT_POINTS))
                            .color(trace_color(index))
                            .width(1.8),
                    );
                }
                if plot_ui.response().clicked() || plot_ui.response().secondary_clicked() {
                    if let Some(point) = plot_ui.pointer_coordinate() {
                        clicked_time = nearest_sample_time(&sample_times, point.x);
                        second_cursor = plot_ui.response().secondary_clicked()
                            || plot_ui.ctx().input(|input| input.modifiers.shift);
                    }
                }
            });
        if let Some(time) = clicked_time {
            if second_cursor {
                self.scope_cursor_b = Some(time);
                self.scope_active_cursor = CursorSlot::B;
            } else if self.scope_cursor_a.is_none() {
                self.scope_cursor_a = Some(time);
                self.scope_active_cursor = CursorSlot::A;
            } else if self.scope_cursor_b.is_none() {
                self.scope_cursor_b = Some(time);
                self.scope_active_cursor = CursorSlot::B;
            } else {
                match self.scope_active_cursor {
                    CursorSlot::A => self.scope_cursor_a = Some(time),
                    CursorSlot::B => self.scope_cursor_b = Some(time),
                }
            }
        }
        self.paint_scope_cursors(ui, &response);
    }

    fn paint_scope_cursors(&self, ui: &Ui, response: &egui_plot::PlotResponse<()>) {
        let frame = *response.transform.frame();
        if !frame.is_positive() {
            return;
        }
        let painter = ui.painter().with_clip_rect(frame);
        let cursor_a = self.scope_cursor_a.map(|time| {
            (
                CursorSlot::A.label(),
                response.transform.position_from_point_x(time),
                HOVER,
            )
        });
        let cursor_b = self.scope_cursor_b.map(|time| {
            (
                CursorSlot::B.label(),
                response.transform.position_from_point_x(time),
                ACCENT,
            )
        });
        if let (Some((_, xa, _)), Some((_, xb, _))) = (cursor_a, cursor_b) {
            painter.rect_filled(
                egui::Rect::from_x_y_ranges(xa.min(xb)..=xa.max(xb), frame.y_range()),
                0.0,
                HOVER.gamma_multiply(0.07),
            );
        }
        for (label, x, color) in [cursor_a, cursor_b].into_iter().flatten() {
            if x < frame.left() || x > frame.right() {
                continue;
            }
            painter.line_segment(
                [egui::pos2(x, frame.top()), egui::pos2(x, frame.bottom())],
                egui::Stroke::new(1.3, color),
            );
            painter.text(
                egui::pos2((x + 4.0).min(frame.right() - 10.0), frame.top() + 4.0),
                Align2::LEFT_TOP,
                label,
                FontId::monospace(11.0),
                color,
            );
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

    fn export_controls(&mut self, ui: &mut Ui, status: &mut Status) {
        egui::CollapsingHeader::new("Export CSV")
            .default_open(false)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Toutes les valeurs de l'onglet actif, sans décimation.")
                        .small()
                        .color(MUTED),
                );
                ui.label(
                    RichText::new(format!("Nom proposé : {}", self.tab.filename()))
                        .small()
                        .color(MUTED),
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
                    match self.export_payload() {
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

    fn export_payload(&self) -> Result<Option<(String, String)>, SimulationError> {
        match self.tab {
            InstrumentTab::Scope => {
                if self.selected.is_empty() {
                    return Ok(None);
                }
                let Some(traces) = self.current_traces()? else {
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

    fn current_traces(&self) -> Result<Option<Vec<Trace>>, SimulationError> {
        if let Some(run) = &self.realtime {
            if self.selected.is_empty() {
                return Ok(Some(Vec::new()));
            }
            let signals = self.selected.iter().cloned().collect::<Vec<_>>();
            let samples = if let Some(step) = run.context.nominal_step {
                let latest = run.samples.last().map_or(0.0, |sample| sample.time);
                let start = latest - step * 500.0;
                let first = run.samples.partition_point(|sample| sample.time < start);
                &run.samples[first..]
            } else {
                run.samples.as_slice()
            };
            return transient_traces_from_samples(samples, &run.context, &signals).map(Some);
        }
        let Some(run) = &self.transient_run else {
            return Ok(None);
        };
        let AnalysisResult::Transient(result) = &run.result else {
            return Ok(None);
        };
        if self.selected.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let signals = self.selected.iter().cloned().collect::<Vec<_>>();
        transient_traces(result, &run.context, &signals).map(Some)
    }

    fn spectrum_data(&self) -> Result<Option<(Trace, Spectrum)>, SimulationError> {
        let Some(signal) = &self.spectrum_signal else {
            return Ok(None);
        };
        if let Some(run) = &self.realtime {
            let trace = transient_traces_from_samples(
                &run.samples,
                &run.context,
                std::slice::from_ref(signal),
            )?
            .into_iter()
            .next()
            .ok_or(SimulationError::NoData)?;
            let step = run
                .context
                .nominal_step
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
            .map(|run| run.revision)
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
    paused: &mut bool,
    running: bool,
) -> RealtimeAction {
    ui.heading("Transitoire temps réel");
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
            ui.label(RichText::new("Aucune source indépendante.").color(MUTED));
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
    let mut action = RealtimeAction::None;
    ui.horizontal(|ui| {
        ui.label("Vitesse");
        egui::ComboBox::from_id_salt("realtime_speed")
            .selected_text(format!("{speed:.2}×"))
            .show_ui(ui, |ui| {
                for candidate in REALTIME_SPEEDS {
                    ui.selectable_value(speed, candidate, format!("{candidate:.2}×"));
                }
            });
        if running
            && ui
                .button(if *paused { "Reprendre" } else { "Pause" })
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
        ui.label(RichText::new("Aucune source indépendante.").color(MUTED));
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
                .color(MUTED),
        );
    }
    ui.add_enabled(can_run, egui::Button::new("Lancer l'analyse AC"))
        .clicked()
}

fn signal_controls(ui: &mut Ui, selected: &mut BTreeSet<SignalId>, catalog: &SignalCatalog) {
    egui::CollapsingHeader::new("Signaux affichés")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("Toutes tensions").clicked() {
                    for name in &catalog.voltages {
                        selected.insert(SignalId::Voltage(name.clone()));
                    }
                }
                if ui.small_button("Aucun").clicked() {
                    selected.clear();
                }
            });
            if catalog.voltages.is_empty() && catalog.currents.is_empty() {
                ui.label(RichText::new("Aucun signal disponible.").color(MUTED));
                return;
            }
            if !catalog.voltages.is_empty() {
                ui.label(RichText::new("Tensions").small().color(MUTED));
                for name in &catalog.voltages {
                    let signal = SignalId::Voltage(name.clone());
                    checkbox_signal(ui, selected, signal);
                }
            }
            if !catalog.currents.is_empty() {
                ui.label(RichText::new("Courants de branche").small().color(MUTED));
                for name in &catalog.currents {
                    let signal = SignalId::Current(name.clone());
                    checkbox_signal(ui, selected, signal);
                }
            }
        });
}

fn checkbox_signal(ui: &mut Ui, selected: &mut BTreeSet<SignalId>, signal: SignalId) {
    let mut checked = selected.contains(&signal);
    let label = format!("{} ({})", signal.label(), signal.unit().symbol());
    if ui.checkbox(&mut checked, label).changed() {
        if checked {
            selected.insert(signal);
        } else {
            selected.remove(&signal);
        }
    }
}

fn spectrum_signal_control(ui: &mut Ui, signal: &mut Option<SignalId>, catalog: &SignalCatalog) {
    ui.label("Signal analysé");
    let available = catalog.signals();
    if available.is_empty() {
        ui.label(RichText::new("Aucun signal disponible.").color(MUTED));
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
        ui.label(RichText::new("Aucune tension disponible.").color(MUTED));
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

fn symbol_kind_short(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Voltmeter => "V",
        SymbolKind::Ammeter => "A",
        SymbolKind::Ohmmeter => "Ω",
        _ => "sonde",
    }
}

fn nearest_sample_time(times: &[f64], target: f64) -> Option<f64> {
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

fn value_at(points: &[[f64; 2]], time: f64) -> Option<f64> {
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

fn cursor_measurements(
    traces: &[Trace],
    cursor_a: Option<f64>,
    cursor_b: Option<f64>,
) -> Vec<CursorMeasurement> {
    traces
        .iter()
        .map(|trace| {
            let value_a = cursor_a.and_then(|time| value_at(&trace.points, time));
            let value_b = cursor_b.and_then(|time| value_at(&trace.points, time));
            let delta = value_a.zip(value_b).map(|(a, b)| b - a);
            let interval = match (cursor_a, cursor_b) {
                (Some(a), Some(b)) if a <= b => {
                    let window = trace
                        .points
                        .iter()
                        .filter(|point| point[0] >= a && point[0] <= b)
                        .collect::<Vec<_>>();
                    if window.is_empty() {
                        None
                    } else {
                        let mut minimum = window[0][1];
                        let mut maximum = window[0][1];
                        let mut square_sum = 0.0;
                        for point in &window {
                            minimum = minimum.min(point[1]);
                            maximum = maximum.max(point[1]);
                            square_sum += point[1] * point[1];
                        }
                        let count = window.len() as f64;
                        Some((minimum, maximum, (square_sum / count).sqrt()))
                    }
                }
                _ => None,
            };
            CursorMeasurement {
                signal: trace.signal.clone(),
                value_a,
                value_b,
                delta,
                interval_min: interval.map(|(minimum, _, _)| minimum),
                interval_max: interval.map(|(_, maximum, _)| maximum),
                interval_rms: interval.map(|(_, _, rms)| rms),
            }
        })
        .collect()
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

fn measurement_table(ui: &mut Ui, traces: &[Trace]) {
    for measurement in measurements(traces) {
        let unit = measurement.signal.unit().unit();
        ui.label(RichText::new(measurement.signal.label()).strong());
        ui.label(
            RichText::new(format!(
                "min {} · max {} · crête-à-crête {} · moy {} · RMS {} · {} pts",
                format_si_precision(measurement.minimum, unit, 4),
                format_si_precision(measurement.maximum, unit, 4),
                format_si_precision(measurement.peak_to_peak, unit, 4),
                format_si_precision(measurement.mean, unit, 4),
                format_si_precision(measurement.rms, unit, 4),
                measurement.samples
            ))
            .small()
            .color(MUTED),
        );
    }
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

fn common_unit(traces: &[Trace]) -> Option<SignalUnit> {
    let first = traces.first()?.unit();
    traces
        .iter()
        .all(|trace| trace.unit() == first)
        .then_some(first)
}

fn trace_color(index: usize) -> Color32 {
    TRACE_COLORS[index % TRACE_COLORS.len()]
}

fn format_axis(value: f64, unit: Option<SignalUnit>) -> String {
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

fn format_frequency(value: f64) -> String {
    format_si_precision(value, Unit::Hertz, 3)
}

fn position_position(position: &HoverPosition<'_>) -> egui_plot::PlotPoint {
    match position {
        HoverPosition::NearDataPoint { position, .. } | HoverPosition::Elsewhere { position } => {
            *position
        }
    }
}

fn decimate(points: &[[f64; 2]], limit: usize) -> Vec<[f64; 2]> {
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
    ui.label(RichText::new(message).color(MUTED));
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
                    state.show(ui, document, &mut status);
                });
        })
    }

    #[test]
    fn defaults_select_demo_voltages() {
        let context = egui::Context::default();
        let mut state = InstrumentsState::new();
        let document = Document::demo().unwrap();
        frame(&context, &mut state, &document);
        assert_eq!(
            state.selected,
            BTreeSet::from([
                SignalId::Voltage("OUT".to_owned()),
                SignalId::Voltage("VIN".to_owned()),
            ])
        );
        assert_eq!(
            state.spectrum_signal,
            Some(SignalId::Voltage("OUT".to_owned()))
        );
        assert_eq!(state.ac.output.as_deref(), Some("OUT"));
        assert_eq!(state.transient.final_value, "5 V");
    }

    #[test]
    fn clearing_a_schematic_resets_automatic_selection() {
        let mut state = InstrumentsState::new();
        let catalog = SignalCatalog {
            voltages: vec!["OUT".to_owned()],
            currents: Vec::new(),
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
    fn scope_cursors_snap_and_measure_an_interval() {
        let trace = Trace {
            signal: SignalId::Voltage("OUT".to_owned()),
            points: vec![[0.0, 1.0], [1.0, 3.0], [2.0, 5.0]],
        };
        assert_eq!(nearest_sample_time(&[0.0, 1.0, 2.0], 0.6), Some(1.0));
        assert_eq!(nearest_sample_time(&[0.0, 1.0, 2.0], 9.0), Some(2.0));
        let measurements = cursor_measurements(&[trace], Some(0.0), Some(2.0));
        assert_eq!(measurements.len(), 1);
        let measurement = &measurements[0];
        assert_eq!(measurement.value_a, Some(1.0));
        assert_eq!(measurement.value_b, Some(5.0));
        assert_eq!(measurement.delta, Some(4.0));
        assert_eq!(measurement.interval_min, Some(1.0));
        assert_eq!(measurement.interval_max, Some(5.0));
        let expected_rms = (35.0_f64 / 3.0).sqrt();
        assert!((measurement.interval_rms.unwrap() - expected_rms).abs() < 1e-12);
        assert_eq!(value_at(&[[0.0, 1.0], [2.0, 5.0]], 0.9), Some(1.0));
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
        state.advance_realtime(0.01, &mut status);
        let advanced = state.transient_time().unwrap();
        assert!(advanced > start);
        state.set_realtime_paused(true);
        state.advance_realtime(0.02, &mut status);
        assert_eq!(state.transient_time(), Some(advanced));
        let traces = state.current_traces().unwrap().unwrap();
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
        let (filename, csv) = state.export_payload().unwrap().unwrap();
        assert_eq!(filename, "circuitsim-portee.csv");
        assert!(csv.starts_with("signal,unit,time_s,value\n"));
        assert!(state.spectrum_data().unwrap().is_some());
        state.tab = InstrumentTab::Spectrum;
        let (filename, csv) = state.export_payload().unwrap().unwrap();
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
        let (filename, csv) = state.export_payload().unwrap().unwrap();
        assert_eq!(filename, "circuitsim-bode.csv");
        assert!(csv.starts_with("signal,frequency_hz,magnitude_v,gain_db,phase_deg\n"));
    }
}
