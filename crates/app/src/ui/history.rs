//! Named history panel with direct state jumps.

use eframe::egui::{self, RichText, Ui};

use crate::document::Document;
use crate::ui::{muted_text_color, Status};

/// Draws the named history and jumps to the selected state.
pub fn show(ui: &mut Ui, document: &mut Document, status: &mut Status) {
    let items = document.history_items();
    let position = document.history_position();
    egui::CollapsingHeader::new(format!("Historique ({})", items.len()))
        .default_open(false)
        .show(ui, |ui| {
            if items.is_empty() {
                ui.label(
                    RichText::new("Aucune modification enregistrée.")
                        .small()
                        .color(muted_text_color(ui)),
                );
                return;
            }
            let mut target = None;
            if ui.selectable_label(position == 0, "État initial").clicked() {
                target = Some(0);
            }
            for (index, item) in items.iter().enumerate() {
                let state = index + 1;
                let mut text = RichText::new(format!("{state} · {}", item.label));
                if !item.applied {
                    text = text.color(muted_text_color(ui));
                }
                let response = ui.selectable_label(position == state, text);
                if response.clicked() {
                    target = Some(state);
                }
                if !item.applied {
                    response.on_hover_text("Rétablir jusqu'à cet état");
                }
            }
            ui.label(
                RichText::new(
                    "Cliquez un état pour y revenir ; Ctrl+Z et Ctrl+Y restent disponibles.",
                )
                .small()
                .color(muted_text_color(ui)),
            );
            if let Some(target) = target {
                match document.undo_to(target) {
                    Ok(true) => status.success(format!("Historique ramené à l'état {target}.")),
                    Ok(false) => status.info("Le schéma est déjà à cet état."),
                    Err(error) => status.error(error.user_message()),
                }
            }
        });
}
