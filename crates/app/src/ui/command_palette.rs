//! Keyboard command palette with deterministic fuzzy filtering.

use editor_core::{GridPos, SymbolKind};
use eframe::egui::{self, Align2, Key};

use crate::document::{Alignment, DistributionAxis, Document};
use crate::ui::palette::BUILTINS;
use crate::ui::Tool;

/// Action selected from the command palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandAction {
    /// Select a placement tool.
    Place(SymbolKind),
    /// Select an editing tool.
    Tool(Tool),
    /// Fit the canvas.
    Fit,
    /// Undo the last edit.
    Undo,
    /// Redo the last edit.
    Redo,
    /// Delete the selection.
    Delete,
    /// Copy the selection.
    Copy,
    /// Arm clipboard paste placement.
    Paste,
    /// Duplicate the selection.
    Duplicate,
    /// Select every object.
    SelectAll,
    /// Rotate the selected symbols.
    Rotate,
    /// Mirror the selected symbols.
    Mirror,
    /// Align the selection.
    Align(Alignment),
    /// Distribute the selection.
    Distribute(DistributionAxis),
    /// Clear the schematic.
    New,
    /// Toggle the instruments panel.
    Instruments,
    /// Open help.
    Help,
}

/// Persistent command palette state.
#[derive(Default)]
pub struct CommandPalette {
    open: bool,
    focus: bool,
    query: String,
    selected: usize,
    anchor: Option<GridPos>,
}

impl CommandPalette {
    /// Returns true when the palette is visible.
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Opens and focuses the palette at the current canvas cursor.
    pub fn open(&mut self, anchor: Option<GridPos>) {
        self.open = true;
        self.focus = true;
        self.query.clear();
        self.selected = 0;
        self.anchor = anchor;
    }

    /// Returns the canvas position captured when the palette opened.
    pub const fn anchor(&self) -> Option<GridPos> {
        self.anchor
    }

    /// Closes the palette.
    pub fn close(&mut self) {
        self.open = false;
        self.focus = false;
    }

    /// Toggles the palette.
    pub fn toggle(&mut self, anchor: Option<GridPos>) {
        if self.open {
            self.close();
        } else {
            self.open(anchor);
        }
    }

    /// Draws the palette and returns the chosen action.
    pub fn show(&mut self, context: &egui::Context, document: &Document) -> Option<CommandAction> {
        if !self.open {
            return None;
        }
        let mut action = None;
        let mut close = false;
        egui::Window::new("Palette de commandes")
            .id(egui::Id::new("command_palette"))
            .anchor(Align2::CENTER_TOP, [0.0, 64.0])
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .min_width(560.0)
            .max_width(560.0)
            .show(context, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .desired_width(f32::INFINITY)
                        .hint_text("Rechercher un composant ou une action…"),
                );
                if self.focus {
                    response.request_focus();
                    self.focus = false;
                }
                if response.changed() {
                    self.selected = 0;
                }
                ui.label(
                    egui::RichText::new("Entrée pour exécuter · Échap pour fermer")
                        .small()
                        .color(crate::ui::MUTED),
                );
                ui.separator();

                let matches = self.matches(document);
                if matches.is_empty() {
                    ui.label(egui::RichText::new("Aucun résultat.").color(crate::ui::MUTED));
                } else {
                    self.selected = self.selected.min(matches.len() - 1);
                    egui::ScrollArea::vertical()
                        .max_height(330.0)
                        .show(ui, |ui| {
                            for (index, (_, label, detail, command)) in matches.iter().enumerate() {
                                let selected = index == self.selected;
                                let response =
                                    ui.selectable_label(selected, format!("{label}   {detail}"));
                                if response.clicked() {
                                    action = Some(command.clone());
                                    close = true;
                                }
                            }
                        });
                }

                let down = ui.input(|input| input.key_pressed(Key::ArrowDown));
                let up = ui.input(|input| input.key_pressed(Key::ArrowUp));
                if down && !matches.is_empty() {
                    self.selected = (self.selected + 1) % matches.len();
                }
                if up && !matches.is_empty() {
                    self.selected = (self.selected + matches.len() - 1) % matches.len();
                }
                if ui.input(|input| input.key_pressed(Key::Enter)) && !matches.is_empty() {
                    action = Some(matches[self.selected].3.clone());
                    close = true;
                }
                if ui.input(|input| input.key_pressed(Key::Escape)) {
                    close = true;
                }
            });
        if close {
            self.close();
        }
        action
    }

    fn matches(&self, document: &Document) -> Vec<(i64, String, String, CommandAction)> {
        let query = self.query.trim();
        let mut entries = Vec::new();
        for (kind, name, reference) in &BUILTINS {
            let label = format!("Placer {name}");
            let detail = format!("[{reference}] {}", kind.key());
            push_match(
                &mut entries,
                query,
                &label,
                &detail,
                CommandAction::Place(kind.clone()),
            );
        }
        for (key, definition) in document.registry().iter() {
            if matches!(definition.kind, SymbolKind::Custom(_)) {
                push_match(
                    &mut entries,
                    query,
                    &format!("Placer {key}"),
                    "[custom]",
                    CommandAction::Place(definition.kind.clone()),
                );
            }
        }

        push_match(
            &mut entries,
            query,
            "Outil Sélection",
            "V",
            CommandAction::Tool(Tool::Select),
        );
        push_match(
            &mut entries,
            query,
            "Outil Fil",
            "W",
            CommandAction::Tool(Tool::Wire),
        );
        push_match(
            &mut entries,
            query,
            "Outil Jonction",
            "J",
            CommandAction::Tool(Tool::Junction),
        );
        push_match(
            &mut entries,
            query,
            "Outil Label",
            "L",
            CommandAction::Tool(Tool::Label),
        );
        push_match(
            &mut entries,
            query,
            "Ajuster la vue",
            "Home",
            CommandAction::Fit,
        );
        if document.can_undo() {
            push_match(
                &mut entries,
                query,
                "Annuler",
                "Ctrl+Z",
                CommandAction::Undo,
            );
        }
        if document.can_redo() {
            push_match(
                &mut entries,
                query,
                "Rétablir",
                "Ctrl+Y",
                CommandAction::Redo,
            );
        }
        if !document.selection().is_empty() {
            push_match(&mut entries, query, "Copier", "Ctrl+C", CommandAction::Copy);
            push_match(
                &mut entries,
                query,
                "Dupliquer la sélection",
                "Ctrl+D",
                CommandAction::Duplicate,
            );
            push_match(
                &mut entries,
                query,
                "Supprimer la sélection",
                "Suppr",
                CommandAction::Delete,
            );
            push_match(&mut entries, query, "Pivoter", "R", CommandAction::Rotate);
            push_match(&mut entries, query, "Miroir", "M", CommandAction::Mirror);
        }
        if document.clipboard().is_some() {
            push_match(
                &mut entries,
                query,
                "Coller",
                "Ctrl+V",
                CommandAction::Paste,
            );
        }
        push_match(
            &mut entries,
            query,
            "Tout sélectionner",
            "Ctrl+A",
            CommandAction::SelectAll,
        );
        if document.selection().len() >= 2 {
            for (label, alignment) in [
                ("Aligner à gauche", Alignment::Left),
                ("Aligner au centre", Alignment::CenterX),
                ("Aligner à droite", Alignment::Right),
                ("Aligner en haut", Alignment::Top),
                ("Aligner au milieu", Alignment::CenterY),
                ("Aligner en bas", Alignment::Bottom),
            ] {
                push_match(
                    &mut entries,
                    query,
                    label,
                    "alignement",
                    CommandAction::Align(alignment),
                );
            }
        }
        if document.selection().len() >= 3 {
            push_match(
                &mut entries,
                query,
                "Distribuer horizontalement",
                "distribution",
                CommandAction::Distribute(DistributionAxis::Horizontal),
            );
            push_match(
                &mut entries,
                query,
                "Distribuer verticalement",
                "distribution",
                CommandAction::Distribute(DistributionAxis::Vertical),
            );
        }
        push_match(
            &mut entries,
            query,
            "Nouveau schéma",
            "efface",
            CommandAction::New,
        );
        push_match(
            &mut entries,
            query,
            "Afficher les instruments",
            "panneau",
            CommandAction::Instruments,
        );
        push_match(
            &mut entries,
            query,
            "Aide et raccourcis",
            "?",
            CommandAction::Help,
        );

        entries.sort_by(|a, b| {
            (a.0, a.1.to_lowercase(), a.2.to_lowercase()).cmp(&(
                b.0,
                b.1.to_lowercase(),
                b.2.to_lowercase(),
            ))
        });
        entries.truncate(14);
        entries
    }
}

fn push_match(
    entries: &mut Vec<(i64, String, String, CommandAction)>,
    query: &str,
    label: &str,
    detail: &str,
    action: CommandAction,
) {
    if let Some(score) = fuzzy_score(query, &format!("{label} {detail}")) {
        entries.push((score, label.to_owned(), detail.to_owned(), action));
    }
}

/// Returns a deterministic subsequence score; lower is better.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i64> {
    let query = normalize_search(query);
    let text = normalize_search(text);
    if query.is_empty() {
        return Some(0);
    }
    let mut positions = Vec::with_capacity(query.len());
    let mut start = 0;
    for character in query.chars() {
        let offset = text[start..].find(character)?;
        positions.push(start + offset);
        start += offset + character.len_utf8();
    }
    let gaps = positions
        .windows(2)
        .map(|window| window[1].saturating_sub(window[0] + 1))
        .sum::<usize>();
    let start_penalty = positions.first().copied().unwrap_or(0);
    Some(i64::try_from(gaps * 10 + start_penalty).unwrap_or(i64::MAX))
}

fn normalize_search(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| match character {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ÿ' => 'y',
            'ñ' => 'n',
            _ => character,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_score_prefers_dense_prefix_matches() {
        let dense = fuzzy_score("res", "Résistance").unwrap();
        let scattered = fuzzy_score("res", "Source de tension").unwrap_or(i64::MAX);
        assert!(dense < scattered);
        assert!(fuzzy_score("xyz", "Résistance").is_none());
    }

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(fuzzy_score("", "Ajuster"), Some(0));
    }
}
