//! Simulation controls, scope, FFT, Bode plots, and CSV export.

use std::collections::BTreeSet;
use std::time::Duration;

use eframe::egui::{self, Align, Color32, Layout, RichText, Ui, Vec2};
use egui_plot::{Corner, HoverPosition, Legend, Line, Plot};
use sim_core::units::{format_si_precision, Unit};
use worker::protocol::{AnalysisKind, AnalysisResult};

use crate::analysis::{
    analysis_name, bode_csv, bode_trace, compute_spectrum, measurements, prepare_ac,
    prepare_transient, spectrum_csv, transient_csv, transient_traces, AcSettings, BodeTrace,
    CompletedRun, SignalCatalog, SignalId, SignalUnit, SimulationClient, SimulationError, Spectrum,
    Trace, TransientSettings, MAX_AC_POINTS,
};
use crate::document::Document;
use crate::export;
use crate::ui::{Status, ACCENT, HOVER, MUTED, SYMBOL};

const ERROR: Color32 = Color32::from_rgb(240, 128, 128);
const CONTROL_WIDTH: f32 = 304.0;
const MAX_PLOT_POINTS: usize = 4096;

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
            export_path: default_export_path(),
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
            if self
                .active_run()
                .is_some_and(|run| run.context.document_revision != document.revision())
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
        let can_run = self.can_run(document);
        if transient_controls(ui, &mut self.transient, catalog, can_run) {
            self.run_transient(document, now, status);
        }
        ui.separator();
        signal_controls(ui, &mut self.selected, catalog);
        ui.separator();
        ui.heading("Mesures");
        match self.current_traces() {
            Ok(Some(traces)) if !traces.is_empty() => measurement_table(ui, &traces),
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

    fn spectrum_controls(
        &mut self,
        ui: &mut Ui,
        document: &Document,
        catalog: &SignalCatalog,
        now: f64,
        status: &mut Status,
    ) {
        let can_run = self.can_run(document);
        if transient_controls(ui, &mut self.transient, catalog, can_run) {
            self.run_transient(document, now, status);
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
            }
            Ok(None) => {
                ui.label(RichText::new("Lancez l'analyse AC pour tracer le Bode.").color(MUTED));
            }
            Err(error) => {
                ui.colored_label(ERROR, error.user_message());
            }
        }
    }

    fn plots(&self, ui: &mut Ui) {
        match self.tab {
            InstrumentTab::Scope => self.scope_plot(ui),
            InstrumentTab::Spectrum => self.spectrum_plot(ui),
            InstrumentTab::Bode => self.bode_plot(ui),
        }
    }

    fn scope_plot(&self, ui: &mut Ui) {
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
        let y_formatter = move |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            format_axis(mark.value, unit)
        };
        let x_formatter = |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            format_si_precision(mark.value, Unit::Second, 3)
        };
        Plot::new(format!("scope_plot_{revision}"))
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
            });
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

    fn bode_plot(&self, ui: &mut Ui) {
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
        Plot::new(format!("bode_gain_{revision}"))
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
            });
        ui.add_space(4.0);
        Plot::new(format!("bode_phase_{revision}"))
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
            });
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
        self.client.is_some() && self.pending.is_none() && document.extraction().is_some()
    }

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

fn transient_controls(
    ui: &mut Ui,
    settings: &mut TransientSettings,
    catalog: &SignalCatalog,
    can_run: bool,
) -> bool {
    ui.heading("Transitoire");
    egui::Grid::new("transient_time_fields")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Début");
            ui.add(
                egui::TextEdit::singleline(&mut settings.start_time)
                    .desired_width(86.0)
                    .hint_text("0"),
            );
            ui.end_row();
            ui.label("Arrêt");
            ui.add(
                egui::TextEdit::singleline(&mut settings.stop_time)
                    .desired_width(86.0)
                    .hint_text("5m"),
            );
            ui.end_row();
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
    ui.add_enabled(can_run, egui::Button::new("Lancer le transitoire"))
        .clicked()
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
