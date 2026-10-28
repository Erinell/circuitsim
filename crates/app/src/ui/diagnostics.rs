//! Clickable overcurrent diagnostics shown in the inspector.

use eframe::egui::{self, RichText, Ui};

use crate::analysis::LiveValues;
use crate::document::Document;
use crate::overcurrent::{self, Severity};
use crate::ui::canvas::CanvasState;
use crate::ui::{muted_text_color, Status, CRITICAL, WARNING};

/// Draws the overcurrent list and selects the clicked object.
pub fn show(
    ui: &mut Ui,
    document: &mut Document,
    live: &LiveValues,
    wire_limit: f64,
    enabled: bool,
    canvas: &mut CanvasState,
    status: &mut Status,
) {
    if !enabled {
        return;
    }
    let items = overcurrent::items(document, live, wire_limit);
    let title = if items.is_empty() {
        "Surintensités".to_owned()
    } else {
        format!("Surintensités ({})", items.len())
    };
    egui::CollapsingHeader::new(title)
        .default_open(!items.is_empty())
        .show(ui, |ui| {
            if live.is_empty() {
                ui.label(
                    RichText::new("Lancez une simulation pour détecter les surintensités.")
                        .small()
                        .color(muted_text_color(ui)),
                );
                return;
            }
            if live.document_revision != document.revision() {
                ui.label(
                    RichText::new("Résultat antérieur : alertes masquées.")
                        .small()
                        .color(muted_text_color(ui)),
                );
                return;
            }
            if items.is_empty() {
                ui.label(
                    RichText::new("Aucune surintensité détectée.")
                        .small()
                        .color(muted_text_color(ui)),
                );
                return;
            }
            for item in items {
                let color = if item.severity == Severity::Critical {
                    CRITICAL
                } else {
                    WARNING
                };
                let label = format!("{} · {}", item.title, item.detail());
                let response = ui
                    .selectable_label(false, RichText::new(label).small().color(color))
                    .on_hover_text("Sélectionner et cadrer cet objet");
                if response.clicked() {
                    document.select_only(Some(item.object));
                    if let Some(bounds) = document.selection_bounds() {
                        canvas.request_fit_bounds(bounds);
                    }
                    status.success(format!("{} sélectionné.", item.title));
                }
            }
        });
}
