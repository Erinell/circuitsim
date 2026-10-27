//! Inline scopes embedded at the bottom of the canvas.

use std::collections::BTreeSet;
use std::sync::Arc;

use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{Corner, Legend, Line, Plot, VLine};

use crate::analysis::{compute_spectrum, SignalCatalog, SignalId, SignalUnit, Spectrum, Trace};
use crate::document::Document;
use crate::ui::instruments::{
    common_unit, decimate, format_axis, format_frequency, nearest_sample_time, trace_color,
    value_at, InstrumentsState, MAX_PLOT_POINTS,
};
use crate::ui::{Status, ACCENT, HOVER, MUTED};
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
type TraceCache = ((u64, Option<f64>), Arc<Vec<Trace>>);

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
#[derive(Clone, Debug, Default)]
pub struct InlineScopes {
    scopes: Vec<Scope>,
    next_id: usize,
    visible: bool,
}

impl InlineScopes {
    /// Creates the default collection with the current instrument signals.
    pub fn with_signals(signals: BTreeSet<SignalId>) -> Self {
        let mut scopes = Self {
            scopes: Vec::new(),
            next_id: 1,
            visible: true,
        };
        scopes.push_signals(signals);
        scopes
    }

    /// Shows or hides the inline area.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Returns whether the inline area is visible.
    pub const fn visible(&self) -> bool {
        self.visible
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
        instruments: &InstrumentsState,
        document: &Document,
        status: &mut Status,
    ) {
        if !self.visible {
            return;
        }
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
        ui.horizontal(|ui| {
            ui.label(RichText::new("Portées").strong());
            if ui.button("Ajouter une portée").clicked() {
                if let Some(signal) = default_signal(&catalog) {
                    self.push_signals(BTreeSet::from([signal]));
                    status.info("Portée ajoutée.");
                } else {
                    status.info("Aucun signal disponible.");
                }
            }
            if ui.button("Fermer").clicked() {
                self.visible = false;
            }
            ui.label(
                RichText::new(
                    "Molette : zoom · glisser : déplacer · clic : curseurs A/B · double-clic : auto-échelle",
                )
                .small()
                .color(MUTED),
            );
        });
        ui.separator();
        let available = ui.available_height();
        let count = self.scopes.len().max(1) as f32;
        let plot_height = ((available - 24.0) / count - 78.0).clamp(96.0, 640.0);
        egui::ScrollArea::vertical()
            .id_salt("inline_scopes")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut actions = Vec::new();
                for (index, scope) in self.scopes.iter_mut().enumerate() {
                    let action = scope.show(ui, instruments, &catalog, plot_height);
                    if action != ScopeAction::None {
                        actions.push((index, action));
                    }
                    ui.separator();
                }
                actions.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
                for (index, action) in actions {
                    self.apply_action(index, action, status);
                }
            });
    }

    fn apply_action(&mut self, index: usize, action: ScopeAction, status: &mut Status) {
        match action {
            ScopeAction::None => {}
            ScopeAction::Remove => {
                if index < self.scopes.len() {
                    self.scopes.remove(index);
                    status.info("Portée supprimée.");
                }
            }
            ScopeAction::MoveUp => {
                if index > 0 {
                    self.scopes.swap(index, index - 1);
                }
            }
            ScopeAction::MoveDown => {
                if index + 1 < self.scopes.len() {
                    self.scopes.swap(index, index + 1);
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
    fn show(
        &mut self,
        ui: &mut Ui,
        instruments: &InstrumentsState,
        catalog: &SignalCatalog,
        plot_height: f32,
    ) -> ScopeAction {
        let mut action = ScopeAction::None;
        let live_traces = self.traces(instruments);
        let mut freeze_now = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&self.title).strong());
            ui.menu_button("Signaux", |ui| {
                let mut changed = false;
                for name in &catalog.voltages {
                    changed |=
                        signal_checkbox(ui, &mut self.signals, SignalId::Voltage(name.clone()));
                }
                for name in &catalog.currents {
                    changed |=
                        signal_checkbox(ui, &mut self.signals, SignalId::Current(name.clone()));
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
            if ui.button("↑").on_hover_text("Monter").clicked() {
                action = ScopeAction::MoveUp;
            }
            if ui.button("↓").on_hover_text("Descendre").clicked() {
                action = ScopeAction::MoveDown;
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
        let unit = common_unit(&traces);
        self.show_values(ui, &traces, unit);
        match self.view {
            ScopeView::Time => {
                self.show_time(ui, &traces, unit, instruments.nominal_step(), plot_height)
            }
            ScopeView::Spectrum => {
                self.show_spectrum(ui, &traces, unit, instruments.nominal_step(), plot_height)
            }
        }
        action
    }

    fn traces(&mut self, instruments: &InstrumentsState) -> Arc<Vec<Trace>> {
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
                .traces_for(&self.signals)
                .unwrap_or_default()
                .unwrap_or_default(),
        );
        self.cache = Some((key, traces.clone()));
        traces
    }

    fn show_values(&self, ui: &mut Ui, traces: &[Trace], unit: Option<SignalUnit>) {
        ui.horizontal_wrapped(|ui| {
            for trace in traces {
                let value = trace.points.last().map_or(0.0, |point| point[1]);
                ui.label(
                    RichText::new(format!("{} = {}", trace.label(), format_axis(value, unit)))
                        .small()
                        .color(signal_color(&trace.signal)),
                );
            }
        });
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
            ui.horizontal_wrapped(|ui| {
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
            });
        }
    }

    fn show_time(
        &mut self,
        ui: &mut Ui,
        traces: &[Trace],
        unit: Option<SignalUnit>,
        nominal_step: Option<f64>,
        plot_height: f32,
    ) {
        let _ = nominal_step;
        let mut clicked_time = None;
        if self.separate {
            let each = ((plot_height - 30.0) / traces.len().max(1) as f32).max(80.0);
            for (index, trace) in traces.iter().enumerate() {
                let plot_id = format!("inline_scope_{}_signal_{index}", self.id);
                if let Some(time) =
                    self.plot_one(ui, plot_id, std::slice::from_ref(trace), unit, each)
                {
                    clicked_time = Some(time);
                }
            }
        } else if let Some(time) = self.plot_one(
            ui,
            format!("inline_scope_{}", self.id),
            traces,
            unit,
            plot_height,
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
        Plot::new(format!("inline_scope_{}_fft", self.id))
            .height(plot_height)
            .allow_drag(true)
            .allow_zoom(true)
            .x_axis_label("Fréquence")
            .y_axis_label(match unit {
                Some(SignalUnit::Volt) => "Tension",
                Some(SignalUnit::Ampere) => "Courant",
                None => "Valeur",
            })
            .x_axis_formatter(|mark, _| format_frequency(mark.value))
            .show(ui, |plot_ui| {
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

    fn plot_one(
        &mut self,
        ui: &mut Ui,
        id: String,
        traces: &[Trace],
        unit: Option<SignalUnit>,
        height: f32,
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
        plot.show(ui, |plot_ui| {
            for trace in traces {
                plot_ui.line(
                    Line::new(trace.label(), decimate(&trace.points, MAX_PLOT_POINTS))
                        .color(signal_color(&trace.signal))
                        .width(1.8),
                );
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
        scopes.set_visible(false);
        assert!(!scopes.visible());
    }

    #[test]
    fn actions_reorder_merge_and_remove_scopes() {
        let mut scopes = InlineScopes::default();
        scopes.push_signals(BTreeSet::from([SignalId::Voltage("A".to_owned())]));
        scopes.push_signals(BTreeSet::from([SignalId::Voltage("B".to_owned())]));
        scopes.apply_action(1, ScopeAction::MoveUp, &mut Status::new_info(""));
        assert!(scopes.scopes[0]
            .signals
            .contains(&SignalId::Voltage("B".to_owned())));
        scopes.apply_action(0, ScopeAction::MergeNext, &mut Status::new_info(""));
        assert_eq!(scopes.scopes.len(), 1);
        assert_eq!(scopes.scopes[0].signals.len(), 2);
        scopes.apply_action(0, ScopeAction::Remove, &mut Status::new_info(""));
        assert!(scopes.scopes.is_empty());
    }

    #[test]
    fn active_signals_are_deduplicated() {
        let mut scopes = InlineScopes::default();
        scopes.push_signals(BTreeSet::from([SignalId::Voltage("OUT".to_owned())]));
        scopes.push_signals(BTreeSet::from([SignalId::Voltage("OUT".to_owned())]));
        assert_eq!(scopes.active_signals().len(), 1);
    }
}
