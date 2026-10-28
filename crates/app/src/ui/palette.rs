//! Symbol palette and editing-tool selector.

use std::collections::BTreeSet;

use editor_core::{GridPos, Orientation, Symbol, SymbolId, SymbolKind, SymbolRegistry};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Sense, Ui, Vec2};

use crate::ui::canvas::CanvasState;
use crate::ui::glyph::{paint_symbol, GlyphColors, SymbolText};
use crate::ui::theme::ResistorStyle;
use crate::ui::{Tool, ACCENT, HOVER, LABEL, MUTED, PIN, SYMBOL};

pub(crate) const BUILTINS: [(SymbolKind, &str, &str); 13] = [
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
    (
        SymbolKind::ShuntReference,
        "Référence shunt programmable",
        "U",
    ),
    (SymbolKind::OpAmp, "Ampli. opérationnel", "U"),
    (SymbolKind::Ohmmeter, "Ohmmètre", "OH"),
];

/// Favorites and recently used components.
#[derive(Default)]
pub struct PaletteState {
    favorites: BTreeSet<SymbolKind>,
    recent: Vec<SymbolKind>,
}

impl PaletteState {
    pub(crate) fn is_favorite(&self, kind: &SymbolKind) -> bool {
        self.favorites.contains(kind)
    }

    /// Returns favorite kinds in deterministic order.
    pub(crate) fn favorites(&self) -> impl Iterator<Item = SymbolKind> + '_ {
        self.favorites.iter().cloned()
    }

    /// Replaces the favorite set, keeping deterministic order.
    pub(crate) fn set_favorites(&mut self, favorites: impl IntoIterator<Item = SymbolKind>) {
        self.favorites = favorites.into_iter().collect();
    }

    fn toggle_favorite(&mut self, kind: &SymbolKind) {
        if !self.favorites.remove(kind) {
            self.favorites.insert(kind.clone());
        }
    }

    fn touch(&mut self, kind: &SymbolKind) {
        self.recent.retain(|candidate| candidate != kind);
        self.recent.insert(0, kind.clone());
        self.recent.truncate(5);
    }
}

/// Draws the palette and updates the active tool.
pub fn show(
    ui: &mut Ui,
    registry: &SymbolRegistry,
    filter: &mut String,
    state: &mut PaletteState,
    tool: &mut Tool,
    canvas: &mut CanvasState,
) {
    ui.add_space(4.0);
    ui.heading("Palette");
    ui.label(
        egui::RichText::new("Choisissez un outil ou un composant.")
            .small()
            .color(crate::ui::muted_text_color(ui)),
    );
    ui.add_space(6.0);

    ui.horizontal_wrapped(|ui| {
        tool_button(ui, tool, canvas, Tool::Select, "Sélection");
        tool_button(ui, tool, canvas, Tool::Wire, "Fil  (W)");
        tool_button(ui, tool, canvas, Tool::Junction, "Jonction  (J)");
        tool_button(ui, tool, canvas, Tool::Label, "Nom de net  (N)");
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
            if needle.is_empty() && !state.favorites.is_empty() {
                ui.label(
                    egui::RichText::new("Favoris")
                        .small()
                        .color(crate::ui::muted_text_color(ui)),
                );
                for kind in state.favorites.clone() {
                    if show_builtin(ui, registry, state, tool, canvas, &kind) {
                        shown += 1;
                    }
                }
            }
            if needle.is_empty() && !state.recent.is_empty() {
                ui.label(
                    egui::RichText::new("Récents")
                        .small()
                        .color(crate::ui::muted_text_color(ui)),
                );
                for kind in state.recent.clone() {
                    if show_builtin(ui, registry, state, tool, canvas, &kind) {
                        shown += 1;
                    }
                }
            }
            if needle.is_empty() {
                let passive = [SymbolKind::Resistor, SymbolKind::Capacitor, SymbolKind::Inductor];
                let sources = [
                    SymbolKind::VoltageSource,
                    SymbolKind::CurrentSource,
                    SymbolKind::Ground,
                    SymbolKind::Switch,
                ];
                let semi = [
                    SymbolKind::Diode,
                    SymbolKind::Led,
                    SymbolKind::Zener,
                    SymbolKind::ShuntReference,
                    SymbolKind::OpAmp,
                ];
                let probes = [SymbolKind::Ohmmeter];
                for (title, kinds) in [
                    ("Passifs", passive.as_slice()),
                    ("Sources et connexions", sources.as_slice()),
                    ("Semi-conducteurs", semi.as_slice()),
                    ("Mesures", probes.as_slice()),
                ] {
                    egui::CollapsingHeader::new(title)
                        .default_open(true)
                        .show(ui, |ui| {
                            for kind in kinds {
                                if show_builtin(ui, registry, state, tool, canvas, kind) {
                                    shown += 1;
                                }
                            }
                        });
                }
                let customs = registry
                    .iter()
                    .filter(|(_, definition)| matches!(definition.kind, SymbolKind::Custom(_)))
                    .map(|(key, definition)| (key.to_owned(), definition.kind.clone()))
                    .collect::<Vec<_>>();
                if !customs.is_empty() {
                    egui::CollapsingHeader::new("Personnalisés")
                        .default_open(true)
                        .show(ui, |ui| {
                            for (key, kind) in &customs {
                                if symbol_button(ui, registry, state, tool, kind, key, key) {
                                    state.touch(kind);
                                    canvas.cancel_interaction();
                                    shown += 1;
                                }
                            }
                        });
                }
            } else {
                for (kind, name, reference) in &BUILTINS {
                    let haystack = format!("{name} {reference} {}", kind.key()).to_lowercase();
                    if !haystack.contains(&needle) {
                        continue;
                    }
                    shown += 1;
                    if symbol_button(ui, registry, state, tool, kind, name, reference) {
                        state.touch(kind);
                        canvas.cancel_interaction();
                    }
                }
                for (key, definition) in registry.iter() {
                    if !matches!(definition.kind, SymbolKind::Custom(_)) {
                        continue;
                    }
                    if !format!("{key} personnalisé").to_lowercase().contains(&needle) {
                        continue;
                    }
                    shown += 1;
                    if symbol_button(ui, registry, state, tool, &definition.kind, key, key) {
                        state.touch(&definition.kind);
                        canvas.cancel_interaction();
                    }
                }
            }
            if shown == 0 {
                ui.label(
                    egui::RichText::new("Aucun composant ne correspond.")
                        .italics()
                        .color(crate::ui::muted_text_color(ui)),
                );
            }
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "Clic droit : favori · V sélection, W fil, J jonction, L label, R pivoter, M miroir.",
                )
                .small()
                .color(crate::ui::muted_text_color(ui)),
            );
        });
}

fn show_builtin(
    ui: &mut Ui,
    registry: &SymbolRegistry,
    state: &mut PaletteState,
    tool: &mut Tool,
    canvas: &mut CanvasState,
    kind: &SymbolKind,
) -> bool {
    let Some((kind, name, reference)) = BUILTINS.iter().find(|(candidate, _, _)| candidate == kind)
    else {
        return false;
    };
    if symbol_button(ui, registry, state, tool, kind, name, reference) {
        state.touch(kind);
        canvas.cancel_interaction();
    }
    true
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
    state: &mut PaletteState,
    tool: &mut Tool,
    kind: &SymbolKind,
    name: &str,
    reference: &str,
) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 48.0), Sense::click());
    let response = response.on_hover_text(format!("{name}  [{reference}] · clic droit : favori"));
    let _ = response.context_menu(|ui| {
        let label = if state.is_favorite(kind) {
            "Retirer des favoris"
        } else {
            "Ajouter aux favoris"
        };
        if ui.button(label).clicked() {
            state.toggle_favorite(kind);
            ui.close();
        }
    });
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
                resistor: ResistorStyle::default(),
            },
            SymbolText::NONE,
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
    if state.is_favorite(kind) {
        painter.text(
            Pos2::new(rect.right() - 12.0, rect.center().y),
            Align2::CENTER_CENTER,
            "★",
            FontId::proportional(13.0),
            ACCENT,
        );
    }

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
        assert_eq!(BUILTINS.len(), 13);
        assert_eq!(BUILTINS[0].2, "R");
        assert_eq!(BUILTINS[12].2, "OH");
        assert!(!BUILTINS
            .iter()
            .any(|(kind, _, _)| matches!(kind, SymbolKind::Voltmeter | SymbolKind::Ammeter)));
    }
}
