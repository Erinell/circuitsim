//! Inline scopes embedded at the bottom of the canvas.

use std::collections::BTreeSet;

use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{Corner, Legend, Line, Plot};

use crate::analysis::{SignalCatalog, SignalId, SignalUnit, Trace};
use crate::document::Document;
use crate::ui::instruments::{
    common_unit, decimate, format_axis, trace_color, InstrumentsState, MAX_PLOT_POINTS,
};
use crate::ui::{Status, MUTED};

/// One stacked inline scope.
#[derive(Clone, Debug)]
struct Scope {
    id: usize,
    title: String,
    signals: BTreeSet<SignalId>,
    frozen: bool,
    frozen_traces: Option<Vec<Trace>>,
}

impl Scope {
    fn with_signals(id: usize, signals: BTreeSet<SignalId>) -> Self {
        Self {
            id,
            title: format!("Portée {id}"),
            signals,
            frozen: false,
            frozen_traces: None,
        }
    }
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
            let default = catalog
                .default_voltage()
                .map(SignalId::Voltage)
                .or_else(|| catalog.voltages.first().cloned().map(SignalId::Voltage));
            if let Some(signal) = default {
                self.push_signal(signal);
            }
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Portées").strong());
            if ui.button("Ajouter une portée").clicked() {
                let default = catalog
                    .default_voltage()
                    .map(SignalId::Voltage)
                    .or_else(|| catalog.voltages.first().cloned().map(SignalId::Voltage));
                if let Some(signal) = default {
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
                RichText::new("Molette : zoom · glisser : déplacer · légende : signaux")
                    .small()
                    .color(MUTED),
            );
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("inline_scopes")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut remove = None;
                for (index, scope) in self.scopes.iter_mut().enumerate() {
                    if scope.show(ui, instruments, &catalog) {
                        remove = Some(index);
                    }
                    ui.separator();
                }
                if let Some(index) = remove {
                    self.scopes.remove(index);
                    status.info("Portée supprimée.");
                }
            });
    }
}

impl Scope {
    fn show(
        &mut self,
        ui: &mut Ui,
        instruments: &InstrumentsState,
        catalog: &SignalCatalog,
    ) -> bool {
        let mut remove = false;
        let live_traces = instruments
            .traces_for(&self.signals)
            .unwrap_or_default()
            .unwrap_or_default();
        let mut freeze_now = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&self.title).strong());
            ui.menu_button("Signaux", |ui| {
                let mut changed = false;
                for name in &catalog.voltages {
                    let signal = SignalId::Voltage(name.clone());
                    let mut selected = self.signals.contains(&signal);
                    if ui.checkbox(&mut selected, signal.label()).changed() {
                        if selected {
                            self.signals.insert(signal);
                        } else {
                            self.signals.remove(&signal);
                        }
                        changed = true;
                    }
                }
                for name in &catalog.currents {
                    let signal = SignalId::Current(name.clone());
                    let mut selected = self.signals.contains(&signal);
                    if ui.checkbox(&mut selected, signal.label()).changed() {
                        if selected {
                            self.signals.insert(signal);
                        } else {
                            self.signals.remove(&signal);
                        }
                        changed = true;
                    }
                }
                if changed {
                    self.frozen = false;
                    self.frozen_traces = None;
                }
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
                self.frozen_traces = Some(Vec::new());
            }
            if ui.button("Supprimer").clicked() {
                remove = true;
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
            return remove;
        }
        let unit = common_unit(&traces);
        ui.horizontal_wrapped(|ui| {
            for trace in &traces {
                let value = trace.points.last().map_or(0.0, |point| point[1]);
                ui.label(
                    RichText::new(format!("{} = {}", trace.label(), format_axis(value, unit)))
                        .small()
                        .color(signal_color(&trace.signal)),
                );
            }
        });
        let y_formatter = move |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            format_axis(mark.value, unit)
        };
        let x_formatter = |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
            sim_core::units::format_si_precision(mark.value, sim_core::units::Unit::Second, 3)
        };
        Plot::new(format!("inline_scope_{}", self.id))
            .height(150.0)
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
            })
            .show(ui, |plot_ui| {
                for trace in &traces {
                    plot_ui.line(
                        Line::new(trace.label(), decimate(&trace.points, MAX_PLOT_POINTS))
                            .color(signal_color(&trace.signal))
                            .width(1.8),
                    );
                }
            });
        remove
    }
}

fn signal_color(signal: &SignalId) -> Color32 {
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
}
