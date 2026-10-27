//! Selection inspector and symbol parameter editing.

use editor_core::{
    GridPos, Junction, ObjectRef, Orientation, Series, Symbol, SymbolId, SymbolKind, Wire,
};
use eframe::egui::{self, Align2, Color32, Grid, Key, RichText, TextEdit, Ui, Vec2};
use sim_core::netlist::ParamValue;
use sim_core::units::{format_si_precision, Unit};

use crate::analysis::{probe_readings, LiveValues, ProbeReading, SymbolLive};
use crate::document::{
    format_parameter, parameter_error_message, parameter_label, parameter_unit, parse_parameter,
    Document,
};
use crate::ui::{symbol_kind_name, Status, ACCENT};

const ERROR: Color32 = Color32::from_rgb(240, 128, 128);

/// Inspector draft state, synchronized on selection or revision changes.
#[derive(Default)]
pub struct InspectorState {
    snapshot: Option<Snapshot>,
    symbol: Option<SymbolDraft>,
    label: Option<LabelDraft>,
    series: Series,
    error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    revision: u64,
    selection: Vec<ObjectRef>,
}

#[derive(Clone, Debug)]
struct SymbolDraft {
    id: SymbolId,
    kind: SymbolKind,
    label: String,
    orientation: Orientation,
    mirrored: bool,
    params: Vec<ParamDraft>,
}

#[derive(Clone, Debug)]
struct ParamDraft {
    key: String,
    value: ParamDraftValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ParamDraftValue {
    Number(String),
    Bool(bool),
    Text(String),
}

#[derive(Clone, Debug)]
struct LabelDraft {
    text: String,
}

/// Floating editor opened by double-clicking a component.
pub struct SymbolDialog {
    id: SymbolId,
    draft: SymbolDraft,
    series: Series,
    error: Option<String>,
}

impl SymbolDialog {
    /// Captures the current symbol into a dialog draft.
    pub fn new(document: &Document, id: SymbolId) -> Option<Self> {
        let symbol = document.symbol(id)?;
        Some(Self {
            id,
            draft: SymbolDraft::from_symbol(symbol),
            series: Series::default(),
            error: None,
        })
    }

    /// Returns the edited symbol.
    pub const fn id(&self) -> SymbolId {
        self.id
    }

    /// Draws the dialog. Returns false when it should close.
    pub fn show(
        &mut self,
        context: &egui::Context,
        document: &mut Document,
        status: &mut Status,
        live: &LiveValues,
    ) -> bool {
        let mut open = true;
        let mut close = false;
        let live_info = document
            .symbol(self.id)
            .filter(|symbol| !symbol.kind.is_measurement())
            .map(|symbol| live.symbol(document, symbol));
        let probe_info = probe_readings(document, live)
            .into_iter()
            .find(|probe| probe.id == self.id);
        egui::Window::new("Propriétés du composant")
            .open(&mut open)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_width(390.0)
            .show(context, |ui| {
                if let Some(error) = &self.error {
                    ui.colored_label(ERROR, error);
                }
                let action = show_symbol(
                    ui,
                    &mut self.draft,
                    live_info.as_ref(),
                    probe_info.as_ref(),
                    live,
                    document.revision(),
                    &mut self.series,
                );
                match action {
                    EditorAction::None => {}
                    EditorAction::Reset => {
                        if let Some(symbol) = document.symbol(self.id) {
                            self.draft = SymbolDraft::from_symbol(symbol);
                            self.error = None;
                            status.info("Modifications annulées.");
                        }
                    }
                    EditorAction::Apply => {
                        let Some(current) = document.symbol(self.id) else {
                            self.error = Some("Le symbole n'existe plus.".to_owned());
                            return;
                        };
                        let new = match self.draft.to_symbol(current) {
                            Ok(new) => new,
                            Err(error) => {
                                self.error = Some(error);
                                return;
                            }
                        };
                        if new == *current {
                            status.info("Aucune modification à appliquer.");
                            return;
                        }
                        match document.update_symbol(new) {
                            Ok(()) => {
                                self.error = None;
                                status.success("Symbole mis à jour.");
                                close = true;
                            }
                            Err(error) => {
                                let message = error.user_message();
                                self.error = Some(message.clone());
                                status.error(message);
                            }
                        }
                    }
                }
            });
        if close || context.input(|input| input.key_pressed(Key::Escape)) {
            open = false;
        }
        open
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditorAction {
    None,
    Apply,
    Reset,
}

impl SymbolDraft {
    fn from_symbol(symbol: &Symbol) -> Self {
        Self {
            id: symbol.id,
            kind: symbol.kind.clone(),
            label: symbol.label.clone(),
            orientation: symbol.orientation,
            mirrored: symbol.mirrored,
            params: symbol
                .params
                .iter()
                .map(|(key, value)| ParamDraft {
                    key: key.clone(),
                    value: match value {
                        ParamValue::Number(number) => ParamDraftValue::Number(format_parameter(
                            *number,
                            parameter_unit(&symbol.kind, key),
                        )),
                        ParamValue::Bool(flag) => ParamDraftValue::Bool(*flag),
                        ParamValue::Text(text) => ParamDraftValue::Text(text.clone()),
                    },
                })
                .collect(),
        }
    }

    fn to_symbol(&self, current: &Symbol) -> Result<Symbol, String> {
        let mut symbol = current.clone();
        symbol.label = self.label.trim().to_owned();
        symbol.orientation = self.orientation;
        symbol.mirrored = self.mirrored;
        symbol.params.clear();
        for param in &self.params {
            let value = match &param.value {
                ParamDraftValue::Number(text) => {
                    let value = parse_parameter(text).map_err(|error| {
                        format!(
                            "{} : {}",
                            parameter_label(&param.key),
                            parameter_error_message(&error)
                        )
                    })?;
                    ParamValue::Number(value)
                }
                ParamDraftValue::Bool(flag) => ParamValue::Bool(*flag),
                ParamDraftValue::Text(text) => ParamValue::Text(text.clone()),
            };
            symbol.params.insert(param.key.clone(), value);
        }
        Ok(symbol)
    }
}

impl InspectorState {
    /// Clears any draft when the document or selection changed externally.
    fn sync(&mut self, document: &Document) {
        let snapshot = Snapshot {
            revision: document.revision(),
            selection: document.selection().iter().copied().collect(),
        };
        if self.snapshot.as_ref() != Some(&snapshot) {
            self.reload(document);
        }
    }

    fn reload(&mut self, document: &Document) {
        self.snapshot = Some(Snapshot {
            revision: document.revision(),
            selection: document.selection().iter().copied().collect(),
        });
        self.error = None;
        self.symbol = document.selected_symbol().map(SymbolDraft::from_symbol);
        self.label = document.selected_label().map(|label| LabelDraft {
            text: label.text.clone(),
        });
    }

    fn report(
        &mut self,
        document: &Document,
        status: &mut Status,
        result: Result<(), crate::document::ActionError>,
        success_message: &str,
    ) {
        match result {
            Ok(()) => {
                self.reload(document);
                status.success(success_message);
            }
            Err(error) => {
                let message = error.user_message();
                self.error = Some(message.clone());
                status.error(message);
            }
        }
    }
}

/// Draws the properties panel and applies validated edits.
pub fn show(
    ui: &mut Ui,
    document: &mut Document,
    state: &mut InspectorState,
    status: &mut Status,
    live: &LiveValues,
) {
    state.sync(document);
    ui.add_space(4.0);
    ui.heading("Propriétés");
    if let Some(error) = &state.error {
        ui.colored_label(ERROR, error);
    }
    ui.separator();

    if document.selection().is_empty() {
        show_empty(ui);
        return;
    }
    if document.selection().len() > 1 {
        show_multiple(ui, document, status);
        return;
    }

    if state.symbol.is_some() {
        let live_info = state
            .symbol
            .as_ref()
            .and_then(|draft| document.symbol(draft.id))
            .filter(|symbol| !symbol.kind.is_measurement())
            .map(|symbol| live.symbol(document, symbol));
        let probe_info = state.symbol.as_ref().and_then(|draft| {
            probe_readings(document, live)
                .into_iter()
                .find(|probe| probe.id == draft.id)
        });
        let series = &mut state.series;
        let action = state
            .symbol
            .as_mut()
            .map(|draft| {
                show_symbol(
                    ui,
                    draft,
                    live_info.as_ref(),
                    probe_info.as_ref(),
                    live,
                    document.revision(),
                    series,
                )
            })
            .unwrap_or(EditorAction::None);
        match action {
            EditorAction::None => {}
            EditorAction::Reset => {
                state.reload(document);
                status.info("Modifications annulées.");
            }
            EditorAction::Apply => {
                let Some(draft) = state.symbol.as_ref() else {
                    return;
                };
                let Some(current) = document.symbol(draft.id) else {
                    state.error = Some("Le symbole n'existe plus.".to_owned());
                    return;
                };
                let new = match draft.to_symbol(current) {
                    Ok(new) => new,
                    Err(error) => {
                        state.error = Some(error.clone());
                        status.error(error);
                        return;
                    }
                };
                if new == *current {
                    status.info("Aucune modification à appliquer.");
                    return;
                }
                let result = document.update_symbol(new);
                state.report(document, status, result, "Symbole mis à jour.");
            }
        }
        return;
    }

    if state.label.is_some() {
        let action = state
            .label
            .as_mut()
            .map(|draft| show_label(ui, draft, document))
            .unwrap_or(EditorAction::None);
        match action {
            EditorAction::None => {}
            EditorAction::Reset => {
                state.reload(document);
                status.info("Modification annulée.");
            }
            EditorAction::Apply => {
                let Some(id) = document.selected_label().map(|label| label.id) else {
                    state.error = Some("Le label n'existe plus.".to_owned());
                    return;
                };
                let Some(text) = state.label.as_ref().map(|draft| draft.text.clone()) else {
                    return;
                };
                let result = document.update_label(id, text);
                state.report(document, status, result, "Label mis à jour.");
            }
        }
        if let Some(label) = document.selected_label() {
            show_net_live(ui, document, live, label.at);
        }
        return;
    }

    if let Some(wire) = document.selected_wire() {
        show_wire(ui, wire);
        show_net_live(ui, document, live, wire.a);
    } else if let Some(junction) = document.selected_junction() {
        show_junction(ui, junction);
        show_net_live(ui, document, live, junction.at);
    } else {
        ui.label("Objet introuvable.");
    }
}

fn show_empty(ui: &mut Ui) {
    ui.label(RichText::new("Aucun objet sélectionné.").strong());
    ui.label("Cliquez un objet ou utilisez Ctrl+A.");
}

fn show_multiple(ui: &mut Ui, document: &mut Document, status: &mut Status) {
    let mut symbols = 0_usize;
    let mut wires = 0_usize;
    let mut junctions = 0_usize;
    let mut labels = 0_usize;
    for object in document.selection() {
        match object {
            ObjectRef::Symbol(_) => symbols += 1,
            ObjectRef::Wire(_) => wires += 1,
            ObjectRef::Junction(_) => junctions += 1,
            ObjectRef::Label(_) => labels += 1,
        }
    }
    ui.label(RichText::new("Sélection multiple").strong());
    ui.label(format!(
        "{symbols} symbole(s), {wires} fil(s), {junctions} jonction(s), {labels} label(s)"
    ));
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if symbols > 0 {
            if ui.button("Pivoter").clicked() {
                match document.rotate_selection() {
                    Ok(()) => status.success("Symboles pivotés."),
                    Err(error) => status.error(error.user_message()),
                }
            }
            if ui.button("Miroir").clicked() {
                match document.mirror_selection() {
                    Ok(()) => status.success("Symboles mis en miroir."),
                    Err(error) => status.error(error.user_message()),
                }
            }
        }
        if ui.button("Supprimer").clicked() {
            match document.delete_selection() {
                Ok(()) => status.success("Objets supprimés."),
                Err(error) => status.error(error.user_message()),
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn show_symbol(
    ui: &mut Ui,
    draft: &mut SymbolDraft,
    live_info: Option<&SymbolLive>,
    probe_info: Option<&ProbeReading>,
    live: &LiveValues,
    revision: u64,
    series: &mut Series,
) -> EditorAction {
    ui.label(
        RichText::new(format!(
            "{}  ·  {}",
            draft.label,
            symbol_kind_name(&draft.kind)
        ))
        .strong()
        .color(ACCENT),
    )
    .on_hover_text(format!("Identifiant : {}", draft.id.0));
    ui.add_space(6.0);

    Grid::new("symbol_properties")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Référence");
            ui.add(TextEdit::singleline(&mut draft.label).desired_width(180.0))
                .on_hover_text("Référence unique du composant");
            ui.end_row();

            ui.label("Orientation");
            egui::ComboBox::from_id_salt("symbol_orientation")
                .selected_text(orientation_name(draft.orientation))
                .show_ui(ui, |ui| {
                    for orientation in [
                        Orientation::R0,
                        Orientation::R90,
                        Orientation::R180,
                        Orientation::R270,
                    ] {
                        ui.selectable_value(
                            &mut draft.orientation,
                            orientation,
                            orientation_name(orientation),
                        );
                    }
                });
            ui.end_row();

            ui.label("Miroir");
            ui.checkbox(&mut draft.mirrored, "Horizontal")
                .on_hover_text("Miroir horizontal du symbole");
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.label(RichText::new("Paramètres").strong());
    let mut commit = false;
    if draft.params.is_empty() {
        ui.label("Aucun paramètre.");
    } else {
        ui.horizontal(|ui| {
            ui.label("Série");
            egui::ComboBox::from_id_salt("parameter_series")
                .selected_text(series.label())
                .show_ui(ui, |ui| {
                    for candidate in Series::ALL {
                        ui.selectable_value(series, candidate, candidate.label());
                    }
                })
                .response
                .on_hover_text("Série E utilisée par la molette et les boutons −/+");
        });
        let kind = draft.kind.clone();
        let mut diode_type = None;
        Grid::new("symbol_parameters")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                for param in &mut draft.params {
                    let unit = parameter_unit(&kind, &param.key);
                    ui.label(parameter_label(&param.key))
                        .on_hover_text(format!("Clé : {}", param.key));
                    match &mut param.value {
                        ParamDraftValue::Number(text) => {
                            ui.horizontal(|ui| {
                                let response = ui.add(
                                    TextEdit::singleline(text)
                                        .desired_width(145.0)
                                        .hint_text("4k7, 2.2u, 1e-6"),
                                );
                                let normalized = parse_parameter(text)
                                    .ok()
                                    .map(|value| format_parameter(value, unit));
                                let response = response.on_hover_text(match &normalized {
                                    Some(value) => format!(
                                        "Valeur normalisée : {value}\nMolette ou Alt+↑/↓ : série E"
                                    ),
                                    None => "Saisie : 4k7, 100n, 1e-6".to_owned(),
                                });
                                if ui
                                    .small_button("−")
                                    .on_hover_text("Valeur E série précédente")
                                    .clicked()
                                {
                                    cycle_number(text, *series, unit, -1);
                                }
                                if ui
                                    .small_button("+")
                                    .on_hover_text("Valeur E série suivante")
                                    .clicked()
                                {
                                    cycle_number(text, *series, unit, 1);
                                }
                                if response.hovered() {
                                    let scroll = ui.input(|input| input.smooth_scroll_delta.y);
                                    if scroll.abs() > 0.1 {
                                        cycle_number(
                                            text,
                                            *series,
                                            unit,
                                            if scroll > 0.0 { 1 } else { -1 },
                                        );
                                    }
                                }
                                if response.has_focus() {
                                    let (up, down) = ui.input(|input| {
                                        (
                                            input.modifiers.alt && input.key_pressed(Key::ArrowUp),
                                            input.modifiers.alt
                                                && input.key_pressed(Key::ArrowDown),
                                        )
                                    });
                                    if up {
                                        cycle_number(text, *series, unit, 1);
                                    } else if down {
                                        cycle_number(text, *series, unit, -1);
                                    }
                                }
                                if response.lost_focus()
                                    && ui.input(|input| input.key_pressed(Key::Enter))
                                {
                                    commit = true;
                                }
                            });
                            if let Err(error) = parse_parameter(text) {
                                ui.colored_label(ERROR, parameter_error_message(&error));
                            }
                        }
                        ParamDraftValue::Bool(flag) => {
                            ui.checkbox(flag, "");
                        }
                        ParamDraftValue::Text(text) => {
                            if kind == SymbolKind::Diode && param.key == "type" {
                                let mut schottky = text.eq_ignore_ascii_case("schottky");
                                let mut changed = false;
                                egui::ComboBox::from_id_salt("diode_type")
                                    .selected_text(if schottky {
                                        "Diode Schottky"
                                    } else {
                                        "Diode"
                                    })
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_value(&mut schottky, false, "Diode")
                                            .clicked()
                                        {
                                            changed = true;
                                        }
                                        if ui
                                            .selectable_value(&mut schottky, true, "Diode Schottky")
                                            .clicked()
                                        {
                                            changed = true;
                                        }
                                    });
                                if changed {
                                    *text =
                                        if schottky { "schottky" } else { "standard" }.to_owned();
                                    diode_type = Some(schottky);
                                }
                            } else {
                                ui.add(
                                    TextEdit::singleline(text)
                                        .desired_width(180.0)
                                        .hint_text("texte"),
                                )
                                .on_hover_text("Paramètre texte");
                            }
                        }
                    }
                    ui.end_row();
                }
            });
        if let Some(schottky) = diode_type {
            apply_diode_preset(&mut draft.params, schottky);
            commit = true;
        }
    }

    if let Some(probe) = probe_info {
        ui.add_space(10.0);
        ui.label(RichText::new("Sonde").strong());
        ui.label(format!(
            "{} · {}",
            probe.label,
            symbol_kind_name(&probe.kind)
        ));
        let value = probe.value.map_or_else(
            || "—".to_owned(),
            |value| format_si_precision(value, probe.unit.unwrap_or(Unit::Volt), 4),
        );
        ui.label(format!("Valeur : {value}"));
        if let Some(net) = &probe.net {
            ui.label(format!("Net : {net}"));
        }
        if let Some(target) = &probe.target {
            ui.label(format!("Composant : {target}"));
        }
    }

    if let Some(info) = live_info {
        show_symbol_live(ui, info, live, revision);
    }

    ui.add_space(10.0);
    let mut action = EditorAction::None;
    ui.horizontal(|ui| {
        if ui.button("Appliquer").clicked() {
            action = EditorAction::Apply;
        }
        if ui.button("Réinitialiser").clicked() {
            action = EditorAction::Reset;
        }
    });
    if commit {
        action = EditorAction::Apply;
    }
    action
}

fn cycle_number(text: &mut String, series: Series, unit: Option<Unit>, steps: i32) {
    let Ok(value) = parse_parameter(text) else {
        return;
    };
    let Some(next) = series.cycle(value, steps) else {
        return;
    };
    *text = format_parameter(next, unit);
}

fn apply_diode_preset(params: &mut [ParamDraft], schottky: bool) {
    let (forward_voltage, emission, maximum_current) = if schottky {
        (0.35, 1.05, 0.2)
    } else {
        (0.7, 1.0, 0.2)
    };
    for param in params {
        let text = match param.key.as_str() {
            "vf" => Some(format_parameter(forward_voltage, Some(Unit::Volt))),
            "n" => Some(format_parameter(emission, None)),
            "imax" => Some(format_parameter(maximum_current, Some(Unit::Ampere))),
            _ => None,
        };
        if let (Some(text), ParamDraftValue::Number(field)) = (text, &mut param.value) {
            *field = text;
        }
    }
}

fn show_symbol_live(ui: &mut Ui, info: &SymbolLive, live: &LiveValues, revision: u64) {
    ui.add_space(10.0);
    ui.label(RichText::new("Grandeurs simulées").strong())
        .on_hover_text("Lancez une simulation transitoire pour actualiser V, I et P.");
    if live.is_empty() {
        return;
    }
    if live.document_revision != revision {
        ui.label(RichText::new("Résultat antérieur aux dernières modifications.").color(ACCENT));
    }
    if let Some(time) = live.time {
        ui.label(format!(
            "t = {}",
            format_si_precision(time, Unit::Second, 4)
        ));
    }
    for pin in &info.pins {
        let net = pin.net.as_deref().unwrap_or("—");
        let voltage = pin.voltage.map_or_else(
            || "—".to_owned(),
            |value| format_si_precision(value, Unit::Volt, 4),
        );
        ui.label(format!("{} · {net} : {voltage}", pin.name));
    }
    if let (Some(first), Some(second)) = (info.pins.first(), info.pins.get(1)) {
        if let (Some(a), Some(b)) = (first.voltage, second.voltage) {
            ui.label(format!(
                "Tension : {}",
                format_si_precision(a - b, Unit::Volt, 4)
            ));
        }
    }
    if let Some(current) = info.current {
        ui.label(format!(
            "Courant : {}",
            format_si_precision(current, Unit::Ampere, 4)
        ));
    }
    if let Some(power) = info.power {
        ui.label(format!(
            "Puissance : {}",
            format_si_precision(power, Unit::Watt, 4)
        ));
    }
}

fn show_net_live(ui: &mut Ui, document: &Document, live: &LiveValues, at: GridPos) {
    ui.add_space(10.0);
    ui.label(RichText::new("Grandeurs simulées").strong())
        .on_hover_text("Lancez une simulation transitoire pour actualiser la tension.");
    if live.is_empty() {
        return;
    }
    if live.document_revision != document.revision() {
        ui.label(RichText::new("Résultat antérieur aux dernières modifications.").color(ACCENT));
    }
    let Some(name) = document.net_name_at(at) else {
        ui.label("Aucun net à cette position.");
        return;
    };
    let voltage = live.voltage(&name).map_or_else(
        || "—".to_owned(),
        |value| format_si_precision(value, Unit::Volt, 4),
    );
    ui.label(format!("Net {name} : {voltage}"));
}

fn show_label(ui: &mut Ui, draft: &mut LabelDraft, document: &Document) -> EditorAction {
    let heading = ui.label(RichText::new("Label de net").strong().color(ACCENT));
    if let Some(label) = document.selected_label() {
        heading.on_hover_text(format!(
            "Position : ({}, {}) · id {}",
            label.at.x, label.at.y, label.id.0
        ));
    }
    ui.add_space(6.0);
    let mut action = EditorAction::None;
    Grid::new("label_properties")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Nom");
            ui.add(TextEdit::singleline(&mut draft.text).desired_width(180.0))
                .on_hover_text("Nom du net ; la masse doit s'appeler 0");
            ui.end_row();
        });
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui.button("Appliquer").clicked() {
            action = EditorAction::Apply;
        }
        if ui.button("Réinitialiser").clicked() {
            action = EditorAction::Reset;
        }
    });
    action
}

fn show_wire(ui: &mut Ui, wire: &Wire) {
    ui.label(RichText::new("Fil").strong().color(ACCENT))
        .on_hover_text(format!("Identifiant : {}", wire.id.0));
    ui.add_space(6.0);
    Grid::new("wire_properties")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Départ");
            ui.monospace(format!("({}, {})", wire.a.x, wire.a.y));
            ui.end_row();
            ui.label("Arrivée");
            ui.monospace(format!("({}, {})", wire.b.x, wire.b.y));
            ui.end_row();
            ui.label("Longueur");
            ui.monospace(format!(
                "{} cases",
                wire.a.x.abs_diff(wire.b.x) + wire.a.y.abs_diff(wire.b.y)
            ));
            ui.end_row();
            ui.label("Orientation");
            ui.monospace(if wire.is_horizontal() {
                "horizontale"
            } else {
                "verticale"
            });
            ui.end_row();
        });
}

fn show_junction(ui: &mut Ui, junction: &Junction) {
    ui.label(RichText::new("Jonction").strong().color(ACCENT))
        .on_hover_text(format!("Identifiant : {}", junction.id.0));
    ui.add_space(6.0);
    Grid::new("junction_properties")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Position");
            ui.monospace(format!("({}, {})", junction.at.x, junction.at.y));
            ui.end_row();
        });
}

fn orientation_name(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::R0 => "0°",
        Orientation::R90 => "90°",
        Orientation::R180 => "180°",
        Orientation::R270 => "270°",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::GridPos;
    use editor_core::SymbolRegistry;

    fn resistor() -> Symbol {
        let registry = SymbolRegistry::builtin();
        Symbol::from_def(
            SymbolId(1),
            registry.get(&SymbolKind::Resistor).unwrap(),
            GridPos::new(0, 0),
            "R1",
            Orientation::R0,
            false,
        )
    }

    #[test]
    fn draft_round_trip_preserves_symbol() {
        let symbol = resistor();
        let draft = SymbolDraft::from_symbol(&symbol);
        let rebuilt = draft.to_symbol(&symbol).unwrap();
        assert_eq!(rebuilt, symbol);
    }

    #[test]
    fn invalid_parameter_text_keeps_the_draft_editable() {
        let symbol = resistor();
        let mut draft = SymbolDraft::from_symbol(&symbol);
        if let Some(param) = draft.params.first_mut() {
            param.value = ParamDraftValue::Number("not-a-number".to_owned());
        }
        assert!(draft.to_symbol(&symbol).is_err());
    }

    #[test]
    fn schottky_preset_updates_forward_parameters() {
        let mut params = vec![
            ParamDraft {
                key: "vf".to_owned(),
                value: ParamDraftValue::Number("700m".to_owned()),
            },
            ParamDraft {
                key: "n".to_owned(),
                value: ParamDraftValue::Number("1".to_owned()),
            },
            ParamDraft {
                key: "imax".to_owned(),
                value: ParamDraftValue::Number("200m".to_owned()),
            },
        ];
        apply_diode_preset(&mut params, true);
        assert_eq!(
            params[0].value,
            ParamDraftValue::Number("350 mV".to_owned())
        );
        assert_eq!(params[1].value, ParamDraftValue::Number("1.05".to_owned()));
        assert_eq!(
            params[2].value,
            ParamDraftValue::Number("200 mA".to_owned())
        );
    }

    #[test]
    fn orientation_labels_are_french_degrees() {
        assert_eq!(orientation_name(Orientation::R90), "90°");
        assert_eq!(orientation_name(Orientation::R270), "270°");
    }

    #[test]
    fn label_positions_do_not_change_during_editing() {
        let label =
            editor_core::NetLabel::new(editor_core::NetLabelId(1), GridPos::new(3, 4), "OUT")
                .unwrap();
        assert_eq!((label.at.x, label.at.y), (3, 4));
    }

    #[test]
    fn series_cycling_updates_numeric_text_with_units() {
        let mut text = "4.7 kΩ".to_owned();
        cycle_number(&mut text, Series::E12, Some(Unit::Ohm), 1);
        assert!((parse_parameter(&text).unwrap() - 5_600.0).abs() < 1e-9);
        cycle_number(&mut text, Series::E12, Some(Unit::Ohm), -1);
        assert!((parse_parameter(&text).unwrap() - 4_700.0).abs() < 1e-9);
        let mut invalid = "not-a-number".to_owned();
        cycle_number(&mut invalid, Series::E96, None, 1);
        assert_eq!(invalid, "not-a-number");
    }
}
