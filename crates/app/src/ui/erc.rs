//! ERC findings, extraction summary, and deterministic net list.

use editor_core::{ErcCode, ErcSeverity, ObjectRef};
use eframe::egui::{self, Color32, RichText, Ui};

use crate::document::Document;
use crate::ui::canvas::CanvasState;
use crate::ui::{muted_text_color, Status};

/// Draws the ERC summary and lets the user select and frame a finding's subject.
pub fn show(ui: &mut Ui, document: &mut Document, canvas: &mut CanvasState, status: &mut Status) {
    ui.separator();
    ui.heading("Contrôle ERC");
    if let Some(error) = document.extraction_error() {
        ui.colored_label(
            Color32::from_rgb(240, 128, 128),
            format!("Extraction impossible : {error}"),
        );
        return;
    }
    let Some(extraction) = document.extraction() else {
        ui.label(RichText::new("Aucune extraction disponible.").color(muted_text_color(ui)));
        return;
    };

    ui.label(format!(
        "{} net(s) · {} composant(s) · {} nœud(s)",
        extraction.nets.len(),
        extraction.netlist.components.len(),
        extraction.netlist.nodes.len()
    ));
    ui.horizontal_wrapped(|ui| {
        severity_label(ui, ErcSeverity::Error, extraction.erc.error_count());
        severity_label(ui, ErcSeverity::Warning, extraction.erc.warning_count());
        severity_label(ui, ErcSeverity::Info, extraction.erc.info_count());
    });

    if extraction.erc.is_empty() {
        ui.colored_label(Color32::from_rgb(112, 210, 155), "Aucun problème détecté.");
    } else {
        let issues = extraction.erc.issues().to_vec();
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for issue in &issues {
                    let color = severity_color(issue.severity);
                    let selected = issue
                        .subject
                        .is_some_and(|subject| document.selection().contains(&subject));
                    let location = issue
                        .at
                        .map(|at| format!("({}, {})", at.x, at.y))
                        .unwrap_or_default();
                    let response = ui
                        .horizontal_wrapped(|ui| {
                            ui.label(RichText::new("●").color(color).small());
                            let response = ui.selectable_label(
                                selected,
                                RichText::new(issue_label(issue.code)).color(color),
                            );
                            if !location.is_empty() {
                                ui.label(
                                    RichText::new(location).small().color(muted_text_color(ui)),
                                );
                            }
                            if let Some(subject) = issue.subject {
                                ui.label(
                                    RichText::new(subject_name(subject))
                                        .small()
                                        .color(muted_text_color(ui)),
                                );
                            }
                            response
                        })
                        .inner
                        .on_hover_text(format!("{}\nCode : {:?}", issue.message, issue.code));
                    if response.clicked() {
                        let Some(subject) = issue.subject else {
                            status.info("Ce diagnostic ne cible aucun objet.");
                            continue;
                        };
                        select_issue_subject(document, canvas, status, subject);
                    }
                }
            });
    }

    ui.add_space(4.0);
    egui::CollapsingHeader::new("Nets extraits")
        .default_open(false)
        .show(ui, |ui| {
            if document.nets().is_empty() {
                ui.label(
                    RichText::new("Aucun net simulable.")
                        .small()
                        .color(muted_text_color(ui)),
                );
                return;
            }
            for net in document.nets() {
                ui.horizontal(|ui| {
                    ui.monospace(net.name.as_str());
                    ui.label(
                        RichText::new(format!("{} broche(s)", net.pins.len()))
                            .small()
                            .color(muted_text_color(ui)),
                    );
                });
            }
        });
}

fn severity_label(ui: &mut Ui, severity: ErcSeverity, count: usize) {
    ui.label(
        RichText::new(format!("{} : {count}", severity_name(severity)))
            .small()
            .color(severity_color(severity)),
    );
}

fn severity_name(severity: ErcSeverity) -> &'static str {
    match severity {
        ErcSeverity::Error => "Erreurs",
        ErcSeverity::Warning => "Avertissements",
        ErcSeverity::Info => "Infos",
    }
}

fn severity_color(severity: ErcSeverity) -> Color32 {
    match severity {
        ErcSeverity::Error => Color32::from_rgb(240, 128, 128),
        ErcSeverity::Warning => Color32::from_rgb(238, 190, 92),
        ErcSeverity::Info => Color32::from_rgb(128, 182, 236),
    }
}

fn issue_label(code: ErcCode) -> &'static str {
    match code {
        ErcCode::NoGround => "Aucune masse",
        ErcCode::MultipleGround => "Plusieurs masses",
        ErcCode::FloatingPin => "Broche flottante",
        ErcCode::StackedPins => "Broches superposées",
        ErcCode::DanglingWire => "Fil pendant",
        ErcCode::UnconnectedWireEnd => "Extrémité de fil libre",
        ErcCode::WireOverlap => "Chevauchement de fils",
        ErcCode::JunctionOffWire => "Jonction hors fil",
        ErcCode::RedundantJunction => "Jonction redondante",
        ErcCode::LabelOffWire => "Label hors fil",
        ErcCode::AmbiguousLabel => "Label ambigu",
        ErcCode::MultipleLabelsOnNet => "Plusieurs labels sur le net",
        ErcCode::DuplicateNetName => "Nom de net dupliqué",
        ErcCode::ReservedNetName => "Nom de net réservé",
        ErcCode::GroundLabelIgnored => "Label de masse ignoré",
    }
}

fn subject_name(subject: ObjectRef) -> String {
    match subject {
        ObjectRef::Symbol(id) => format!("symbole #{}", id.0),
        ObjectRef::Wire(id) => format!("fil #{}", id.0),
        ObjectRef::Junction(id) => format!("jonction #{}", id.0),
        ObjectRef::Label(id) => format!("label #{}", id.0),
    }
}

fn select_issue_subject(
    document: &mut Document,
    canvas: &mut CanvasState,
    status: &mut Status,
    subject: ObjectRef,
) {
    if document.contains_object(subject) {
        document.select_only(Some(subject));
        if let Some(bounds) = document.selection_bounds() {
            canvas.request_fit_bounds(bounds);
        }
        status.info(format!("{} sélectionné.", subject_name(subject)));
    } else {
        status.error("L'objet ciblé n'existe plus.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_erc_code_has_a_french_label() {
        let codes = [
            ErcCode::NoGround,
            ErcCode::MultipleGround,
            ErcCode::FloatingPin,
            ErcCode::StackedPins,
            ErcCode::DanglingWire,
            ErcCode::UnconnectedWireEnd,
            ErcCode::WireOverlap,
            ErcCode::JunctionOffWire,
            ErcCode::RedundantJunction,
            ErcCode::LabelOffWire,
            ErcCode::AmbiguousLabel,
            ErcCode::MultipleLabelsOnNet,
            ErcCode::DuplicateNetName,
            ErcCode::ReservedNetName,
            ErcCode::GroundLabelIgnored,
        ];
        for code in codes {
            assert!(!issue_label(code).is_empty());
        }
    }

    #[test]
    fn issue_subjects_are_stable() {
        assert_eq!(
            subject_name(ObjectRef::Wire(editor_core::WireId(7))),
            "fil #7"
        );
    }

    #[test]
    fn selecting_an_erc_subject_requests_a_canvas_fit() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(
                editor_core::SymbolKind::Resistor,
                editor_core::GridPos::new(12, -4),
            )
            .unwrap();
        let subject = ObjectRef::Symbol(symbol);
        let expected_bounds = document.selection_bounds();
        document.clear_selection();
        let mut canvas = CanvasState::new();
        let mut status = Status::default();

        select_issue_subject(&mut document, &mut canvas, &mut status, subject);

        assert!(document.selection().contains(&subject));
        assert_eq!(canvas.pending_fit_bounds_for_test(), expected_bounds);
    }
}
