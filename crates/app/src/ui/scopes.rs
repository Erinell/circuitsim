//! Inline scopes embedded at the bottom of the canvas.

use std::collections::BTreeSet;
use std::sync::Arc;

use eframe::egui::{self, Color32, RichText, Ui, Vec2};
use egui_plot::{Corner, HLine, Legend, Line, LineStyle, Plot, VLine};

use crate::analysis::{
    capacitor_labels, component_current_labels, compute_spectrum, measurements, probe_readings,
    SignalCatalog, SignalId, SignalUnit, Spectrum, Trace,
};
use crate::document::Document;
use crate::overcurrent::{over_limit_stats, severity, signal_limit, Severity};
use crate::ui::instruments::{
    common_unit, decimate, format_axis, format_frequency, nearest_sample_time, trace_color,
    value_at, InstrumentsState, MAX_PLOT_POINTS,
};
use crate::ui::{Status, ACCENT, CRITICAL, HOVER, MUTED, WARNING};
use sim_core::units::{format_si_precision, Unit};

/// Signal view shown by one inline scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScopeView {
    Time,
    Spectrum,
}

/// Y-axis scaling mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScaleMode {
    Auto,
    Manual,
}

/// One stacked inline scope.
type TraceCache = ((u64, Option<f64>, f64), Arc<Vec<Trace>>);

#[derive(Clone, Debug)]
struct Scope {
    id: usize,
    title: String,
    signals: BTreeSet<SignalId>,
    frozen: bool,
    frozen_traces: Option<Arc<Vec<Trace>>>,
    cache: Option<TraceCache>,
    scale: ScaleMode,
    manual_min: f64,
    manual_max: f64,
    separate: bool,
    view: ScopeView,
    cursor_a: Option<f64>,
    cursor_b: Option<f64>,
    reset_view: bool,
}

impl Scope {
    fn with_signals(id: usize, signals: BTreeSet<SignalId>) -> Self {
        Self {
            id,
            title: format!("Portée {id}"),
            signals,
            frozen: false,
            frozen_traces: None,
            cache: None,
            scale: ScaleMode::Auto,
            manual_min: -1.0,
            manual_max: 1.0,
            separate: false,
            view: ScopeView::Time,
            cursor_a: None,
            cursor_b: None,
            reset_view: false,
        }
    }
}

/// Action requested by a scope header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScopeAction {
    None,
    Remove,
    MoveUp,
    MoveDown,
    MergeNext,
}

/// Persistent collection of inline scopes.
#[derive(Clone, Debug)]
pub struct InlineScopes {
    scopes: Vec<Scope>,
    next_id: usize,
}

impl Default for InlineScopes {
    fn default() -> Self {
        Self {
            scopes: Vec::new(),
            next_id: 1,
        }
    }
}

impl InlineScopes {
    /// Creates the default collection with the current instrument signals.
    pub fn with_signals(signals: BTreeSet<SignalId>) -> Self {
        let mut scopes = Self {
            scopes: Vec::new(),
            next_id: 1,
        };
        scopes.push_signals(signals);
        scopes
    }

    /// Adds signals to the active scope, creating one when necessary.
    pub fn push_signal(&mut self, signal: SignalId) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.signals.insert(signal);
            scope.frozen = false;
            scope.frozen_traces = None;
            scope.cache = None;
        } else {
            self.push_signals(BTreeSet::from([signal]));
        }
    }

    /// Adds several signals, creating a scope when necessary.
    pub fn push_signals(&mut self, signals: BTreeSet<SignalId>) {
        if signals.is_empty() {
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.scopes.push(Scope::with_signals(id, signals));
    }

    /// Adds a scope with the catalog's default signal.
    pub fn add_default_scope(&mut self, catalog: &SignalCatalog) -> bool {
        let Some(signal) = default_signal(catalog) else {
            return false;
        };
        self.push_signals(BTreeSet::from([signal]));
        true
    }

    /// Returns true when no scope exists.
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }

    /// Returns every signal currently plotted or frozen in an inline scope.
    pub fn active_signals(&self) -> BTreeSet<SignalId> {
        self.scopes
            .iter()
            .flat_map(|scope| scope.signals.iter().cloned())
            .collect()
    }

    /// Draws the inline scope area.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        instruments: &mut InstrumentsState,
        document: &Document,
        status: &mut Status,
        wire_limit: f64,
    ) {
        let catalog = document
            .extraction()
            .map(SignalCatalog::from_extraction)
            .unwrap_or_default();
        if self.scopes.is_empty() {
            let default = default_signal(&catalog);
            if let Some(signal) = default {
                self.push_signal(signal);
            }
        }
        self.probe_strip(ui, instruments, document, &catalog);
        let available = (ui.available_height() - 4.0).max(120.0);
        let count = self.scopes.len().max(1) as f32;
        let gap = 6.0_f32;
        let scope_height = ((available - gap * (count - 1.0)) / count).max(80.0);
        let scope_count = self.scopes.len();
        egui::ScrollArea::vertical()
            .id_salt("inline_scopes")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let scope_width = ui.available_width();
                let mut actions = Vec::new();
                for (index, scope) in self.scopes.iter_mut().enumerate() {
                    let action = ui
                        .allocate_ui_with_layout(
                            Vec2::new(scope_width, scope_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                scope.show(
                                    ui,
                                    instruments,
                                    document,
                                    &catalog,
                                    index > 0,
                                    index + 1 < scope_count,
                                    wire_limit,
                                )
                            },
                        )
                        .inner;
                    if action != ScopeAction::None {
                        actions.push((scope.id, action));
                    }
                    if index + 1 < scope_count {
                        ui.add_space(gap);
                    }
                }
                actions.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
                for (id, action) in actions {
                    self.apply_action(id, action, status);
                }
            });
    }

    fn probe_strip(
        &mut self,
        ui: &mut Ui,
        instruments: &mut InstrumentsState,
        document: &Document,
        catalog: &SignalCatalog,
    ) {
        let readings = probe_readings(document, &instruments.live_values());
        if readings.is_empty() {
            return;
        }
        egui::CollapsingHeader::new(format!("Sondes ({})", readings.len()))
            .default_open(false)
            .show(ui, |ui| {
                for reading in readings {
                    ui.horizontal_wrapped(|ui| {
                        let unit = reading.unit.unwrap_or(Unit::Volt);
                        let value = reading.value.map_or_else(
                            || "—".to_owned(),
                            |value| format_si_precision(value, unit, 4),
                        );
                        ui.label(
                            RichText::new(format!("{} : {value}", reading.label))
                                .small()
                                .color(MUTED),
                        );
                        if let Some(signal) =
                            reading.signal().filter(|signal| catalog.contains(signal))
                        {
                            let mut plotted = self
                                .scopes
                                .last()
                                .is_some_and(|scope| scope.signals.contains(&signal));
                            if ui.checkbox(&mut plotted, "Graphique").changed() {
                                self.toggle_active_signal(signal, plotted);
                            }
                        }
                        let mut visible = instruments.probe_visible(reading.id);
                        if ui.checkbox(&mut visible, "Afficher").changed() {
                            instruments.set_probe_visible(reading.id, visible);
                        }
                    });
                }
            });
    }

    fn toggle_active_signal(&mut self, signal: SignalId, plotted: bool) {
        if plotted {
            self.push_signal(signal);
        } else if let Some(scope) = self.scopes.last_mut() {
            scope.signals.remove(&signal);
            scope.frozen = false;
            scope.frozen_traces = None;
            scope.cache = None;
        }
    }

    fn apply_action(&mut self, id: usize, action: ScopeAction, status: &mut Status) {
        let Some(index) = self.scopes.iter().position(|scope| scope.id == id) else {
            return;
        };
        match action {
            ScopeAction::None => {}
            ScopeAction::Remove => {
                self.scopes.remove(index);
                status.info("Portée supprimée.");
            }
            ScopeAction::MoveUp => {
                if index > 0 {
                    self.scopes.swap(index, index - 1);
                    status.info("Portée déplacée vers le haut.");
                }
            }
            ScopeAction::MoveDown => {
                if index + 1 < self.scopes.len() {
                    self.scopes.swap(index, index + 1);
                    status.info("Portée déplacée vers le bas.");
                }
            }
            ScopeAction::MergeNext => {
                if index + 1 < self.scopes.len() {
                    let next = self.scopes.remove(index + 1);
                    self.scopes[index].signals.extend(next.signals);
                    self.scopes[index].frozen = false;
                    self.scopes[index].frozen_traces = None;
                    self.scopes[index].cache = None;
                    status.info("Portées fusionnées.");
                }
            }
        }
    }
}

impl Scope {
    #[allow(clippy::too_many_arguments)]
    fn show(
        &mut self,
        ui: &mut Ui,
        instruments: &InstrumentsState,
        document: &Document,
        catalog: &SignalCatalog,
        can_move_up: bool,
        can_move_down: bool,
        wire_limit: f64,
    ) -> ScopeAction {
        let mut action = ScopeAction::None;
        let live_traces = self.traces(instruments, document);
        let mut freeze_now = false;
        let reset_view = std::mem::take(&mut self.reset_view);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&self.title).strong());
            ui.menu_button("Signaux", |ui| {
                let mut changed = false;
                ui.label(RichText::new("Tensions").strong());
                for name in &catalog.voltages {
                    changed |=
                        signal_checkbox(ui, &mut self.signals, SignalId::Voltage(name.clone()));
                }
                ui.separator();
                ui.label(RichText::new("Tensions des condensateurs").strong());
                for name in capacitor_labels(document) {
                    changed |=
                        signal_checkbox(ui, &mut self.signals, SignalId::ComponentVoltage(name));
                }
                ui.separator();
                ui.label(RichText::new("Courants des nets").strong());
                for name in &catalog.voltages {
                    changed |=
                        signal_checkbox(ui, &mut self.signals, SignalId::NetCurrent(name.clone()));
                }
                ui.separator();
                ui.label(RichText::new("Courants des composants").strong());
                for name in component_current_labels(document) {
                    changed |= signal_checkbox(ui, &mut self.signals, SignalId::Current(name));
                }
                if changed {
                    self.frozen = false;
                    self.frozen_traces = None;
                    self.cache = None;
                }
            });
            ui.menu_button("Affichage", |ui| {
                ui.label(RichText::new("Signaux").strong());
                ui.radio_value(&mut self.separate, false, "Superposés");
                ui.radio_value(&mut self.separate, true, "Séparés");
                ui.separator();
                ui.label(RichText::new("Échelle Y").strong());
                ui.radio_value(&mut self.scale, ScaleMode::Auto, "Automatique");
                ui.radio_value(&mut self.scale, ScaleMode::Manual, "Manuelle");
                if self.scale == ScaleMode::Manual {
                    ui.add(
                        egui::DragValue::new(&mut self.manual_min)
                            .speed(0.1)
                            .prefix("min "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.manual_max)
                            .speed(0.1)
                            .prefix("max "),
                    );
                }
                ui.separator();
                ui.radio_value(&mut self.view, ScopeView::Time, "Temporel");
                ui.radio_value(&mut self.view, ScopeView::Spectrum, "FFT");
            });
            if ui
                .selectable_label(self.frozen, if self.frozen { "Figée" } else { "Figer" })
                .clicked()
            {
                if self.frozen {
                    self.frozen = false;
                    self.frozen_traces = None;
                } else {
                    freeze_now = true;
                }
            }
            if ui.button("Effacer").clicked() {
                self.frozen = true;
                self.frozen_traces = Some(Arc::new(Vec::new()));
            }
            if ui
                .button("A/B")
                .on_hover_text("Effacer les curseurs A et B")
                .clicked()
            {
                self.cursor_a = None;
                self.cursor_b = None;
            }
            if ui
                .add_enabled(can_move_up, egui::Button::new("↑"))
                .on_hover_text("Monter la portée")
                .clicked()
            {
                action = ScopeAction::MoveUp;
            }
            if ui
                .add_enabled(can_move_down, egui::Button::new("↓"))
                .on_hover_text("Descendre la portée")
                .clicked()
            {
                action = ScopeAction::MoveDown;
            }
            if ui
                .button("Recentrer")
                .on_hover_text("Réinitialise le zoom et le déplacement X/Y")
                .clicked()
            {
                self.reset_view = true;
            }
            if ui
                .button("Fusionner")
                .on_hover_text("Fusionner avec la portée suivante")
                .clicked()
            {
                action = ScopeAction::MergeNext;
            }
            if ui.button("Supprimer").clicked() {
                action = ScopeAction::Remove;
            }
        });
        if freeze_now {
            self.frozen = true;
            self.frozen_traces = Some(live_traces.clone());
        }
        let traces = if self.frozen {
            self.frozen_traces.clone().unwrap_or_default()
        } else {
            live_traces
        };
        if traces.is_empty() {
            ui.label(
                RichText::new("Aucun signal. Ouvrez Signaux ou lancez une simulation.")
                    .small()
                    .color(MUTED),
            );
            return action;
        }
        let plot_height = ui.available_height().max(40.0);
        let unit = common_unit(&traces);
        let side_width = if ui.available_width() >= 560.0 {
            200.0
        } else {
            0.0
        };
        if side_width > 0.0 {
            ui.horizontal_top(|ui| {
                let plot_width = (ui.available_width() - side_width - 8.0).max(180.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(plot_width, plot_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        self.show_plot(
                            ui,
                            &traces,
                            unit,
                            instruments.scope_step(),
                            plot_height,
                            reset_view,
                            document,
                            wire_limit,
                        );
                    },
                );
                ui.separator();
                ui.allocate_ui_with_layout(
                    Vec2::new(side_width, plot_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(format!("scope_values_{}", self.id))
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .max_height(plot_height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                self.show_values(ui, &traces, unit, document, wire_limit);
                            });
                    },
                );
            });
        } else {
            ui.horizontal_wrapped(|ui| self.show_values(ui, &traces, unit, document, wire_limit));
            self.show_plot(
                ui,
                &traces,
                unit,
                instruments.scope_step(),
                plot_height,
                reset_view,
                document,
                wire_limit,
            );
        }
        action
    }

    #[allow(clippy::too_many_arguments)]
    fn show_plot(
        &mut self,
        ui: &mut Ui,
        traces: &[Trace],
        unit: Option<SignalUnit>,
        step: Option<f64>,
        plot_height: f32,
        reset_view: bool,
        document: &Document,
        wire_limit: f64,
    ) {
        match self.view {
            ScopeView::Time => self.show_time(
                ui,
                traces,
                unit,
                step,
                plot_height,
                reset_view,
                document,
                wire_limit,
            ),
            ScopeView::Spectrum => {
                self.show_spectrum(ui, traces, unit, step, plot_height, reset_view)
            }
        }
    }

    fn traces(&mut self, instruments: &InstrumentsState, document: &Document) -> Arc<Vec<Trace>> {
        if self.frozen {
            return self.frozen_traces.clone().unwrap_or_default();
        }
        let key = instruments.trace_key();
        if let Some((cached_key, traces)) = &self.cache {
            if *cached_key == key {
                return traces.clone();
            }
        }
        let traces = Arc::new(
            instruments
                .traces_for(document, &self.signals)
                .unwrap_or_default()
                .unwrap_or_default(),
        );
        self.cache = Some((key, traces.clone()));
        traces
    }

    fn show_values(
        &self,
        ui: &mut Ui,
        traces: &[Trace],
        unit: Option<SignalUnit>,
        document: &Document,
        wire_limit: f64,
    ) {
        for trace in traces {
            let value = trace.points.last().map_or(0.0, |point| point[1]);
            ui.label(
                RichText::new(format!("{} = {}", trace.label(), format_axis(value, unit)))
                    .small()
                    .color(signal_color(&trace.signal)),
            );
            if let Some(limit) = signal_limit(document, &trace.signal, wire_limit) {
                if let Some(stats) = over_limit_stats(&trace.points, limit) {
                    let severity = severity(stats.peak, Some(limit));
                    ui.label(
                        RichText::new(format!(
                            "! {} : {} · {}",
                            trace.label(),
                            crate::overcurrent::format_limit_line(stats.peak, limit, severity),
                            format_si_precision(stats.duration, Unit::Second, 3)
                        ))
                        .small()
                        .color(severity_color(severity)),
                    );
                }
            }
        }
        for measurement in measurements(traces) {
            let measurement_unit = measurement.signal.unit().unit();
            ui.label(
                RichText::new(format!(
                    "{} min {} · max {} · RMS {}",
                    measurement.signal.label(),
                    format_si_precision(measurement.minimum, measurement_unit, 3),
                    format_si_precision(measurement.maximum, measurement_unit, 3),
                    format_si_precision(measurement.rms, measurement_unit, 3)
                ))
                .small()
                .color(MUTED),
            );
        }
        if let (Some(a), Some(b)) = (self.cursor_a, self.cursor_b) {
            let times = traces
                .first()
                .map(|trace| {
                    trace
                        .points
                        .iter()
                        .map(|point| point[0])
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let a = nearest_sample_time(&times, a).unwrap_or(a);
            let b = nearest_sample_time(&times, b).unwrap_or(b);
            ui.label(
                RichText::new(format!(
                    "A/B : tA = {} · tB = {} · Δt = {}",
                    format_si_precision(a, Unit::Second, 4),
                    format_si_precision(b, Unit::Second, 4),
                    format_si_precision((b - a).abs(), Unit::Second, 4)
                ))
                .small()
                .color(MUTED),
            );
            for trace in traces {
                if let (Some(va), Some(vb)) =
                    (value_at(&trace.points, a), value_at(&trace.points, b))
                {
                    ui.label(
                        RichText::new(format!(
                            "{} A={} B={} Δ={}",
                            trace.label(),
                            format_axis(va, unit),
                            format_axis(vb, unit),
                            format_axis(vb - va, unit)
                        ))
                        .small()
                        .color(signal_color(&trace.signal)),
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn show_time(
        &mut self,
        ui: &mut Ui,
        traces: &[Trace],
        unit: Option<SignalUnit>,
        nominal_step: Option<f64>,
        plot_height: f32,
        reset_view: bool,
        document: &Document,
        wire_limit: f64,
    ) {
        let _ = nominal_step;
        let mut clicked_time = None;
        if self.separate {
            let each = ((plot_height - 30.0) / traces.len().max(1) as f32).max(80.0);
            for (index, trace) in traces.iter().enumerate() {
                let plot_id = format!("inline_scope_{}_signal_{index}", self.id);
                if let Some(time) = self.plot_one(
                    ui,
                    plot_id,
                    std::slice::from_ref(trace),
                    unit,
                    each,
                    reset_view,
                    document,
                    wire_limit,
                ) {
                    clicked_time = Some(time);
                }
            }
        } else if let Some(time) = self.plot_one(
            ui,
            format!("inline_scope_{}", self.id),
            traces,
            unit,
            plot_height,
            reset_view,
            document,
            wire_limit,
        ) {
            clicked_time = Some(time);
        }
        if let Some(time) = clicked_time {
            self.update_cursor(time, traces);
        }
    }

    fn show_spectrum(
        &mut self,
        ui: &mut Ui,
        traces: &[Trace],
        unit: Option<SignalUnit>,
        nominal_step: Option<f64>,
        plot_height: f32,
        reset_view: bool,
    ) {
        let Some(step) = nominal_step else {
            ui.label(RichText::new("FFT indisponible sans pas nominal.").color(MUTED));
            return;
        };
        let Some(trace) = traces.first() else {
            return;
        };
        let spectrum = match compute_spectrum(trace, step) {
            Ok(spectrum) => spectrum,
            Err(error) => {
                ui.label(RichText::new(error.user_message()).color(MUTED));
                return;
            }
        };
        self.show_spectrum_values(ui, &spectrum, trace);
        let points = spectrum
            .frequencies
            .iter()
            .zip(&spectrum.magnitudes)
            .map(|(frequency, magnitude)| [*frequency, *magnitude])
            .collect::<Vec<_>>();
        let mut plot = Plot::new(format!("inline_scope_{}_fft", self.id))
            .height(plot_height)
            .allow_drag(true)
            .allow_zoom(true)
            .x_axis_label("Fréquence")
            .y_axis_label(match unit {
                Some(SignalUnit::Volt) => "Tension",
                Some(SignalUnit::Ampere) => "Courant",
                None => "Valeur",
            })
            .x_axis_formatter(|mark, _| format_frequency(mark.value));
        if reset_view {
            plot = plot.reset();
        }
        plot.show(ui, |plot_ui| {
            plot_ui.line(
                Line::new(trace.label(), decimate(&points, MAX_PLOT_POINTS))
                    .color(signal_color(&trace.signal))
                    .width(1.8),
            );
        });
    }

    fn show_spectrum_values(&self, ui: &mut Ui, spectrum: &Spectrum, trace: &Trace) {
        let peak = spectrum.peak.as_ref().map_or_else(
            || "aucun pic".to_owned(),
            |peak| {
                format!(
                    "pic {} à {}",
                    format_axis(peak.magnitude, Some(trace.unit())),
                    format_frequency(peak.frequency)
                )
            },
        );
        ui.label(
            RichText::new(format!(
                "{} · résolution {} · {peak}",
                trace.label(),
                format_frequency(spectrum.resolution)
            ))
            .small()
            .color(signal_color(&trace.signal)),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn plot_one(
        &mut self,
        ui: &mut Ui,
        id: String,
        traces: &[Trace],
        unit: Option<SignalUnit>,
        height: f32,
        reset_view: bool,
        document: &Document,
        wire_limit: f64,
    ) -> Option<f64> {
        let mut clicked = None;
        let y_formatter = move |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            format_axis(mark.value, unit)
        };
        let x_formatter = |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            sim_core::units::format_si_precision(mark.value, sim_core::units::Unit::Second, 3)
        };
        let mut plot = Plot::new(id)
            .height(height)
            .allow_drag(true)
            .allow_zoom(true)
            .allow_scroll(true)
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
                let position = match position {
                    egui_plot::HoverPosition::NearDataPoint { position, .. }
                    | egui_plot::HoverPosition::Elsewhere { position } => position,
                };
                Some(format!(
                    "t = {}\n{}",
                    sim_core::units::format_si_precision(
                        position.x,
                        sim_core::units::Unit::Second,
                        4
                    ),
                    format_axis(position.y, unit)
                ))
            });
        if self.scale == ScaleMode::Manual {
            plot = plot.include_y(self.manual_min).include_y(self.manual_max);
        }
        if reset_view {
            plot = plot.reset();
        }
        plot.show(ui, |plot_ui| {
            for trace in traces {
                plot_ui.line(
                    Line::new(trace.label(), decimate(&trace.points, MAX_PLOT_POINTS))
                        .color(signal_color(&trace.signal))
                        .width(1.8),
                );
                if let Some(limit) = signal_limit(document, &trace.signal, wire_limit) {
                    plot_ui.hline(
                        HLine::new("", limit)
                            .id(egui::Id::new(format!(
                                "imax_pos_{}_{}",
                                self.id,
                                trace.label()
                            )))
                            .color(WARNING)
                            .width(1.0)
                            .style(LineStyle::dashed_dense())
                            .allow_hover(false),
                    );
                    plot_ui.hline(
                        HLine::new("", -limit)
                            .id(egui::Id::new(format!(
                                "imax_neg_{}_{}",
                                self.id,
                                trace.label()
                            )))
                            .color(WARNING)
                            .width(1.0)
                            .style(LineStyle::dashed_dense())
                            .allow_hover(false),
                    );
                    for (index, segment) in
                        crate::overcurrent::over_limit_segments(&trace.points, limit)
                            .into_iter()
                            .enumerate()
                    {
                        plot_ui.line(
                            Line::new("", segment)
                                .color(CRITICAL)
                                .width(2.6)
                                .allow_hover(false)
                                .id(egui::Id::new(format!(
                                    "overcurrent_{}_{}_{}",
                                    self.id,
                                    trace.label(),
                                    index
                                ))),
                        );
                    }
                }
            }
            if let Some(value) = self.cursor_a {
                plot_ui.vline(VLine::new("A", value).color(ACCENT).width(1.0));
            }
            if let Some(value) = self.cursor_b {
                plot_ui.vline(VLine::new("B", value).color(HOVER).width(1.0));
            }
            if plot_ui.response().clicked() {
                if let Some(point) = plot_ui.pointer_coordinate() {
                    clicked = Some(point.x);
                }
            }
        });
        clicked
    }

    fn update_cursor(&mut self, time: f64, traces: &[Trace]) {
        let times = traces
            .first()
            .map(|trace| {
                trace
                    .points
                    .iter()
                    .map(|point| point[0])
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let time = nearest_sample_time(&times, time).unwrap_or(time);
        if self.cursor_a.is_none() {
            self.cursor_a = Some(time);
        } else {
            self.cursor_b = Some(time);
        }
    }
}

fn signal_checkbox(ui: &mut Ui, signals: &mut BTreeSet<SignalId>, signal: SignalId) -> bool {
    let mut selected = signals.contains(&signal);
    if ui.checkbox(&mut selected, signal.label()).changed() {
        if selected {
            signals.insert(signal);
        } else {
            signals.remove(&signal);
        }
        true
    } else {
        false
    }
}

fn default_signal(catalog: &SignalCatalog) -> Option<SignalId> {
    catalog
        .default_voltage()
        .map(SignalId::Voltage)
        .or_else(|| catalog.voltages.first().cloned().map(SignalId::Voltage))
}

/// Deterministic signal color shared by inline scopes and canvas markers.
pub(crate) fn signal_color(signal: &SignalId) -> Color32 {
    let hash = signal
        .label()
        .bytes()
        .fold(0_usize, |acc, byte| acc.wrapping_add(byte as usize));
    trace_color(hash)
}

fn severity_color(severity: Severity) -> Color32 {
    match severity {
        Severity::Critical => CRITICAL,
        Severity::Warning => WARNING,
        Severity::None => MUTED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_create_and_extend_a_scope() {
        let mut scopes = InlineScopes::default();
        assert!(scopes.scopes.is_empty());
        scopes.push_signal(SignalId::Voltage("OUT".to_owned()));
        assert_eq!(scopes.scopes.len(), 1);
        scopes.push_signal(SignalId::Current("V1".to_owned()));
        assert_eq!(scopes.scopes[0].signals.len(), 2);
        scopes.push_signal(SignalId::Voltage("OUT".to_owned()));
        assert_eq!(scopes.scopes[0].signals.len(), 2);
    }

    #[test]
    fn empty_signal_sets_do_not_create_scopes() {
        let mut scopes = InlineScopes::default();
        scopes.push_signals(BTreeSet::new());
        assert!(scopes.scopes.is_empty());
    }

    #[test]
    fn actions_reorder_merge_and_remove_scopes() {
        let mut scopes = InlineScopes::default();
        scopes.push_signals(BTreeSet::from([SignalId::Voltage("A".to_owned())]));
        scopes.push_signals(BTreeSet::from([SignalId::Voltage("B".to_owned())]));
        scopes.apply_action(2, ScopeAction::MoveUp, &mut Status::new_info(""));
        assert!(scopes.scopes[0]
            .signals
            .contains(&SignalId::Voltage("B".to_owned())));
        scopes.apply_action(2, ScopeAction::MergeNext, &mut Status::new_info(""));
        assert_eq!(scopes.scopes.len(), 1);
        assert_eq!(scopes.scopes[0].signals.len(), 2);
        scopes.apply_action(2, ScopeAction::Remove, &mut Status::new_info(""));
        assert!(scopes.scopes.is_empty());
    }

    #[test]
    fn active_signals_are_deduplicated() {
        let mut scopes = InlineScopes::default();
        scopes.push_signals(BTreeSet::from([SignalId::Voltage("OUT".to_owned())]));
        scopes.push_signals(BTreeSet::from([SignalId::Voltage("OUT".to_owned())]));
        assert_eq!(scopes.active_signals().len(), 1);
    }

    #[test]
    fn moving_a_scope_down_uses_its_identity() {
        let mut scopes = InlineScopes::default();
        scopes.push_signals(BTreeSet::from([SignalId::Voltage("A".to_owned())]));
        scopes.push_signals(BTreeSet::from([SignalId::Voltage("B".to_owned())]));
        scopes.apply_action(1, ScopeAction::MoveDown, &mut Status::new_info(""));
        assert!(scopes.scopes[0]
            .signals
            .contains(&SignalId::Voltage("B".to_owned())));
    }

    #[test]
    fn toggling_a_probe_signal_updates_the_active_scope() {
        let signal = SignalId::Voltage("OUT".to_owned());
        let mut scopes = InlineScopes::default();
        scopes.toggle_active_signal(signal.clone(), true);
        assert!(scopes.scopes[0].signals.contains(&signal));
        scopes.toggle_active_signal(signal.clone(), false);
        assert!(!scopes.scopes[0].signals.contains(&signal));
    }

    #[test]
    fn adding_a_scope_from_controls_uses_the_default_signal() {
        let catalog = SignalCatalog {
            voltages: vec!["OUT".to_owned()],
            currents: Vec::new(),
            components: Vec::new(),
            sources: Vec::new(),
        };
        let mut scopes = InlineScopes::default();
        assert!(scopes.add_default_scope(&catalog));
        assert_eq!(scopes.scopes.len(), 1);
        assert_eq!(
            scopes.scopes[0].signals,
            BTreeSet::from([SignalId::Voltage("OUT".to_owned())])
        );
        assert!(!scopes.add_default_scope(&SignalCatalog::default()));
        assert_eq!(scopes.scopes.len(), 1);
    }

    #[test]
    fn merged_panel_starts_with_the_instruments_visible() {
        assert!(crate::preferences::PersistedPreferences::default().show_instruments);
    }
}
