//! Symbol palette and editing-tool selector.

use editor_core::{GridPos, Orientation, Symbol, SymbolId, SymbolKind, SymbolRegistry};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Sense, Ui, Vec2};

use crate::ui::canvas::CanvasState;
use crate::ui::glyph::{paint_symbol, GlyphColors};
use crate::ui::{Tool, ACCENT, HOVER, LABEL, MUTED, PIN, SYMBOL};

const BUILTINS: [(SymbolKind, &str, &str); 11] = [
    (SymbolKind::Resistor, "Résistance", "R"),
    (SymbolKind::Capacitor, "Condensateur", "C"),
    (SymbolKind::Inductor, "Bobine", "L"),
    (SymbolKind::VoltageSource, "Source de tension", "V"),
    (SymbolKind::CurrentSource, "Source de courant", "I"),
    (SymbolKind::Ground, "Masse", "GND"),
    (SymbolKind::Switch, "Interrupteur", "SW"),
    (SymbolKind::Diode, "Diode", "D"),
    (SymbolKind::Led, "LED", "LED"),
    (SymbolKind::Zener, "Diode Zener", "DZ"),
    (SymbolKind::OpAmp, "Ampli. opérationnel", "U"),
];

/// Draws the palette and updates the active tool.
pub fn show(
    ui: &mut Ui,
    registry: &SymbolRegistry,
    filter: &mut String,
    tool: &mut Tool,
    canvas: &mut CanvasState,
) {
    ui.add_space(4.0);
    ui.heading("Palette");
    ui.label(
        egui::RichText::new("Choisissez un outil ou un composant.")
            .small()
            .color(MUTED),
    );
    ui.add_space(6.0);

    ui.horizontal_wrapped(|ui| {
        tool_button(ui, tool, canvas, Tool::Select, "Sélection");
        tool_button(ui, tool, canvas, Tool::Wire, "Fil  (W)");
        tool_button(ui, tool, canvas, Tool::Junction, "Jonction  (J)");
        tool_button(ui, tool, canvas, Tool::Label, "Label  (L)");
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Composants").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::TextEdit::singleline(filter)
                    .desired_width(118.0)
                    .hint_text("Filtrer"),
            );
        });
    });
    ui.add_space(3.0);

    let needle = filter.trim().to_lowercase();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut shown = 0_u16;
            for (kind, name, reference) in &BUILTINS {
                let haystack = format!("{name} {reference} {}", kind.key()).to_lowercase();
                if !needle.is_empty() && !haystack.contains(&needle) {
                    continue;
                }
                shown += 1;
                if symbol_button(ui, registry, tool, kind, name, reference) {
                    canvas.cancel_interaction();
                }
            }
            for (key, definition) in registry.iter() {
                if !matches!(definition.kind, SymbolKind::Custom(_)) {
                    continue;
                }
                let haystack = format!("{key} personnalisé").to_lowercase();
                if !needle.is_empty() && !haystack.contains(&needle) {
                    continue;
                }
                shown += 1;
                if symbol_button(ui, registry, tool, &definition.kind, key, key) {
                    canvas.cancel_interaction();
                }
            }
            if shown == 0 {
                ui.label(
                    egui::RichText::new("Aucun composant ne correspond.")
                        .italics()
                        .color(MUTED),
                );
            }
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "Raccourcis : V sélection, W fil, J jonction, L label, R pivoter, M miroir.",
                )
                .small()
                .color(MUTED),
            );
        });
}

fn tool_button(ui: &mut Ui, tool: &mut Tool, canvas: &mut CanvasState, target: Tool, label: &str) {
    let selected = *tool == target;
    if ui.selectable_label(selected, label).clicked() && !selected {
        *tool = target;
        canvas.cancel_interaction();
    }
}

fn symbol_button(
    ui: &mut Ui,
    registry: &SymbolRegistry,
    tool: &mut Tool,
    kind: &SymbolKind,
    name: &str,
    reference: &str,
) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 48.0), Sense::click());
    let response = response.on_hover_text(format!("{name}  [{reference}]"));
    let selected = tool == &Tool::Place(kind.clone());
    let background = if selected {
        Color32::from_rgb(63, 55, 34)
    } else if response.hovered() {
        Color32::from_rgb(35, 47, 63)
    } else {
        Color32::from_rgb(25, 34, 47)
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect.shrink(1.0), 5.0, background);
    painter.rect_stroke(
        rect.shrink(1.0),
        5.0,
        egui::Stroke::new(
            if selected { 1.4 } else { 0.8 },
            if selected {
                ACCENT
            } else if response.hovered() {
                HOVER
            } else {
                Color32::from_rgb(45, 59, 78)
            },
        ),
        egui::StrokeKind::Inside,
    );

    if let Some(definition) = registry.get(kind) {
        let preview = Symbol::from_def(
            SymbolId(0),
            definition,
            GridPos::new(0, 0),
            "",
            Orientation::R0,
            false,
        );
        paint_symbol(
            &painter,
            &preview,
            Some(definition),
            Pos2::new(rect.left() + 24.0, rect.center().y),
            8.0,
            GlyphColors {
                body: if selected { ACCENT } else { SYMBOL },
                pin: PIN,
                label: LABEL,
                value: MUTED,
            },
            false,
        );
    }
    painter.text(
        Pos2::new(rect.left() + 45.0, rect.center().y - 7.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(12.5),
        if selected {
            ACCENT
        } else {
            Color32::from_rgb(210, 221, 236)
        },
    );
    painter.text(
        Pos2::new(rect.left() + 45.0, rect.center().y + 8.0),
        Align2::LEFT_CENTER,
        reference,
        FontId::monospace(9.5),
        MUTED,
    );

    if response.clicked() {
        *tool = Tool::Place(kind.clone());
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_palette_is_complete_and_deterministic() {
        assert_eq!(BUILTINS.len(), 11);
        assert_eq!(BUILTINS[0].2, "R");
        assert_eq!(BUILTINS[10].2, "U");
    }
}
