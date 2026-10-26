//! Selection inspector and symbol parameter editing.

use editor_core::{Junction, ObjectRef, Orientation, Symbol, SymbolId, SymbolKind, Wire};
use eframe::egui::{self, Color32, Grid, RichText, TextEdit, Ui};
use sim_core::netlist::ParamValue;

use crate::document::{
    format_parameter, parameter_error_message, parameter_label, parameter_unit, parse_parameter,
    Document,
};
use crate::ui::{symbol_kind_name, Status, ACCENT, MUTED};

/// Inspector draft state, synchronized on selection or revision changes.
#[derive(Default)]
pub struct InspectorState {
    snapshot: Option<Snapshot>,
    symbol: Option<SymbolDraft>,
    label: Option<LabelDraft>,
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

#[derive(Clone, Debug)]
enum ParamDraftValue {
    Number(String),
    Bool(bool),
    Text(String),
}

#[derive(Clone, Debug)]
struct LabelDraft {
    text: String,
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
pub fn show(ui: &mut Ui, document: &mut Document, state: &mut InspectorState, status: &mut Status) {
    state.sync(document);
    ui.add_space(4.0);
    ui.heading("Propriétés");
    ui.label(
        RichText::new(format!(
            "{} objet(s) sélectionné(s)",
            document.selection().len()
        ))
        .small()
        .color(MUTED),
    );
    if let Some(error) = &state.error {
        ui.colored_label(Color32::from_rgb(240, 128, 128), error);
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
        let action = state
            .symbol
            .as_mut()
            .map(|draft| show_symbol(ui, draft))
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
        return;
    }

    if let Some(wire) = document.selected_wire() {
        show_wire(ui, wire);
    } else if let Some(junction) = document.selected_junction() {
        show_junction(ui, junction);
    } else {
        ui.label("Objet introuvable.");
    }
}

fn show_empty(ui: &mut Ui) {
    ui.label("Aucun objet sélectionné.");
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            "Cliquez sur un symbole, un fil, une jonction ou un label pour afficher ses propriétés.",
        )
        .color(MUTED),
    );
    ui.add_space(10.0);
    ui.label(RichText::new("Sélection multiple").strong());
    ui.label(
        RichText::new("Maj + clic ajoute ou retire un objet de la sélection.")
            .small()
            .color(MUTED),
    );
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
    ui.add_space(8.0);
    ui.label(
        RichText::new(
            "La suppression et les transformations sont atomiques et annulables avec Ctrl+Z.",
        )
        .small()
        .color(MUTED),
    );
}

fn show_symbol(ui: &mut Ui, draft: &mut SymbolDraft) -> EditorAction {
    ui.label(
        RichText::new(format!(
            "{}  ·  {}",
            draft.label,
            symbol_kind_name(&draft.kind)
        ))
        .strong()
        .color(ACCENT),
    );
    ui.label(
        RichText::new(format!("Identifiant : {}", draft.id.0))
            .small()
            .color(MUTED),
    );
    ui.add_space(6.0);

    Grid::new("symbol_properties")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Référence");
            ui.add(TextEdit::singleline(&mut draft.label).desired_width(170.0));
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
            ui.checkbox(&mut draft.mirrored, "Horizontal");
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.label(RichText::new("Paramètres").strong());
    if draft.params.is_empty() {
        ui.label(RichText::new("Aucun paramètre.").small().color(MUTED));
    } else {
        Grid::new("symbol_parameters")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                for param in &mut draft.params {
                    ui.label(parameter_label(&param.key))
                        .on_hover_text(format!("Clé : {}", param.key));
                    match &mut param.value {
                        ParamDraftValue::Number(text) => {
                            ui.add(
                                TextEdit::singleline(text)
                                    .desired_width(170.0)
                                    .hint_text("valeur SI, ex. 4.7k"),
                            );
                        }
                        ParamDraftValue::Bool(flag) => {
                            ui.checkbox(flag, "");
                        }
                        ParamDraftValue::Text(text) => {
                            ui.add(TextEdit::singleline(text).desired_width(170.0));
                        }
                    }
                    ui.end_row();
                }
            });
        ui.label(
            RichText::new("Notation acceptée : 1k, 4.7k, 2.2u, 10m, 1e-6, avec unité optionnelle.")
                .small()
                .color(MUTED),
        );
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
    action
}

fn show_label(ui: &mut Ui, draft: &mut LabelDraft, document: &Document) -> EditorAction {
    ui.label(RichText::new("Label de net").strong().color(ACCENT));
    if let Some(label) = document.selected_label() {
        ui.label(
            RichText::new(format!(
                "Position : ({}, {}) · id {}",
                label.at.x, label.at.y, label.id.0
            ))
            .small()
            .color(MUTED),
        );
    }
    ui.add_space(6.0);
    let mut action = EditorAction::None;
    Grid::new("label_properties")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Nom");
            ui.add(TextEdit::singleline(&mut draft.text).desired_width(170.0));
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
    ui.label(RichText::new("Fil").strong().color(ACCENT));
    ui.label(
        RichText::new(format!("Identifiant : {}", wire.id.0))
            .small()
            .color(MUTED),
    );
    ui.add_space(6.0);
    Grid::new("wire_properties")
        .num_columns(2)
        .spacing([10.0, 6.0])
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
    ui.add_space(8.0);
    ui.label(
        RichText::new("La géométrie d'un fil se modifie en le déplaçant avec la sélection.")
            .small()
            .color(MUTED),
    );
}

fn show_junction(ui: &mut Ui, junction: &Junction) {
    ui.label(RichText::new("Jonction").strong().color(ACCENT));
    ui.label(
        RichText::new(format!("Identifiant : {}", junction.id.0))
            .small()
            .color(MUTED),
    );
    ui.add_space(6.0);
    Grid::new("junction_properties")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Position");
            ui.monospace(format!("({}, {})", junction.at.x, junction.at.y));
            ui.end_row();
        });
    ui.add_space(8.0);
    ui.label(
        RichText::new("La jonction se déplace avec la sélection.")
            .small()
            .color(MUTED),
    );
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
}
