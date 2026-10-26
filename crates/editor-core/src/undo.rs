//! Unlimited undo/redo stack with atomic command application.

use crate::command::{Command, EditError};
use crate::schematic::Schematic;

/// Unlimited undo/redo history.
#[derive(Clone, Debug, Default)]
pub struct UndoStack {
    done: Vec<Command>,
    undone: Vec<Command>,
}

impl UndoStack {
    /// Creates an empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies a command atomically and records it.
    pub fn execute(
        &mut self,
        schematic: &mut Schematic,
        command: Command,
    ) -> Result<(), EditError> {
        let mut trial = schematic.clone();
        command.apply(&mut trial)?;
        trial.validate()?;
        *schematic = trial;
        self.done.push(command);
        self.undone.clear();
        Ok(())
    }

    /// Reverts the most recent command. Returns false when history is empty.
    pub fn undo(&mut self, schematic: &mut Schematic) -> Result<bool, EditError> {
        let Some(command) = self.done.last().cloned() else {
            return Ok(false);
        };
        let mut trial = schematic.clone();
        command.undo(&mut trial)?;
        trial.validate()?;
        *schematic = trial;
        self.done.pop();
        self.undone.push(command);
        Ok(true)
    }

    /// Reapplies the most recently undone command. Returns false when empty.
    pub fn redo(&mut self, schematic: &mut Schematic) -> Result<bool, EditError> {
        let Some(command) = self.undone.last().cloned() else {
            return Ok(false);
        };
        let mut trial = schematic.clone();
        command.apply(&mut trial)?;
        trial.validate()?;
        *schematic = trial;
        self.undone.pop();
        self.done.push(command);
        Ok(true)
    }

    /// Returns true when at least one command can be undone.
    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    /// Returns true when at least one command can be redone.
    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// Number of commands available for undo.
    pub fn undo_len(&self) -> usize {
        self.done.len()
    }

    /// Number of commands available for redo.
    pub fn redo_len(&self) -> usize {
        self.undone.len()
    }

    /// Returns true when no command is available in either direction.
    pub fn is_empty(&self) -> bool {
        self.done.is_empty() && self.undone.is_empty()
    }

    /// Clears the entire history.
    pub fn clear(&mut self) {
        self.done.clear();
        self.undone.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{GridPos, Orientation};
    use crate::schematic::{Junction, JunctionId, NetLabel, NetLabelId, Wire, WireId};
    use crate::symbol::{Symbol, SymbolId, SymbolKind, SymbolRegistry};

    fn symbol(schematic: &mut Schematic, label: &str) -> Symbol {
        let id = SymbolId(schematic.allocate_id().unwrap());
        let registry = SymbolRegistry::builtin();
        let def = registry.get(&SymbolKind::Resistor).unwrap();
        Symbol::from_def(id, def, GridPos::new(0, 0), label, Orientation::R0, false)
    }

    #[test]
    fn adds_and_undoes_symbols() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let initial = schematic.clone();
        let symbol = symbol(&mut schematic, "R1");
        history
            .execute(&mut schematic, Command::AddSymbol(symbol.clone()))
            .unwrap();
        assert_eq!(schematic.symbols().count(), 1);
        assert!(history.can_undo());
        assert!(history.undo(&mut schematic).unwrap());
        assert!(schematic.content_eq(&initial));
        assert!(history.can_redo());
        assert!(history.redo(&mut schematic).unwrap());
        assert_eq!(schematic.symbol(symbol.id), Some(&symbol));
    }

    #[test]
    fn move_is_reversible() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let symbol = symbol(&mut schematic, "R1");
        let id = symbol.id;
        history
            .execute(&mut schematic, Command::AddSymbol(symbol))
            .unwrap();
        let before = schematic.clone();
        history
            .execute(
                &mut schematic,
                Command::MoveObjects {
                    objects: vec![crate::schematic::ObjectRef::Symbol(id)],
                    delta: GridPos::new(3, 4),
                },
            )
            .unwrap();
        assert_eq!(
            schematic.symbol(id).map(|symbol| symbol.at),
            Some(GridPos::new(3, 4))
        );
        history.undo(&mut schematic).unwrap();
        assert!(schematic.content_eq(&before));
    }

    #[test]
    fn composite_commands_are_reversible() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let symbol = symbol(&mut schematic, "R1");
        let wire_id = WireId(schematic.allocate_id().unwrap());
        let wire = Wire::new(wire_id, GridPos::new(4, 0), GridPos::new(8, 0)).unwrap();
        let command = Command::Composite(vec![Command::AddSymbol(symbol), Command::AddWire(wire)]);
        history.execute(&mut schematic, command).unwrap();
        assert_eq!(schematic.symbols().count(), 1);
        assert_eq!(schematic.wires().count(), 1);
        history.undo(&mut schematic).unwrap();
        assert!(schematic.is_empty());
        history.redo(&mut schematic).unwrap();
        assert_eq!(schematic.wires().count(), 1);
    }

    #[test]
    fn rejects_invalid_wire_atomically() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let wire_id = WireId(schematic.allocate_id().unwrap());
        let invalid = Wire {
            id: wire_id,
            a: GridPos::new(0, 0),
            b: GridPos::new(2, 2),
        };
        let result = history.execute(&mut schematic, Command::AddWire(invalid));
        assert!(matches!(result, Err(EditError::Schematic(_))));
        assert!(schematic.is_empty());
        assert!(history.is_empty());
    }

    #[test]
    fn rejects_stale_replacements() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let symbol = symbol(&mut schematic, "R1");
        history
            .execute(&mut schematic, Command::AddSymbol(symbol.clone()))
            .unwrap();
        let mut stale = symbol.clone();
        stale.label = "R2".to_owned();
        let mut replacement = symbol;
        replacement.label = "R3".to_owned();
        let result = history.execute(
            &mut schematic,
            Command::ReplaceSymbol {
                old: stale,
                new: replacement,
            },
        );
        assert_eq!(result, Err(EditError::StaleCommand));
    }

    #[test]
    fn rejects_duplicate_labels_atomically() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let first = symbol(&mut schematic, "R1");
        history
            .execute(&mut schematic, Command::AddSymbol(first))
            .unwrap();
        let before = schematic.clone();
        let second = symbol(&mut schematic, "R1");
        let result = history.execute(&mut schematic, Command::AddSymbol(second));
        assert!(matches!(result, Err(EditError::Schematic(_))));
        assert!(schematic.content_eq(&before));
        assert_eq!(history.undo_len(), 1);
    }

    #[test]
    fn removals_are_reversible() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let symbol = symbol(&mut schematic, "R1");
        let wire_id = WireId(schematic.allocate_id().unwrap());
        let wire = Wire::new(wire_id, GridPos::new(4, 0), GridPos::new(8, 0)).unwrap();
        let junction_id = JunctionId(schematic.allocate_id().unwrap());
        let junction = Junction::new(junction_id, GridPos::new(10, 10));
        let label_id = NetLabelId(schematic.allocate_id().unwrap());
        let label = NetLabel::new(label_id, GridPos::new(12, 12), "OUT").unwrap();

        for command in [
            Command::AddSymbol(symbol.clone()),
            Command::AddWire(wire.clone()),
            Command::AddJunction(junction.clone()),
            Command::AddLabel(label.clone()),
        ] {
            history.execute(&mut schematic, command).unwrap();
        }
        let before = schematic.clone();

        for command in [
            Command::RemoveSymbol(symbol),
            Command::RemoveWire(wire),
            Command::RemoveJunction(junction),
            Command::RemoveLabel(label),
        ] {
            history.execute(&mut schematic, command).unwrap();
        }
        assert!(schematic.is_empty());

        for _ in 0..4 {
            assert!(history.undo(&mut schematic).unwrap());
        }
        assert!(schematic.content_eq(&before));

        for _ in 0..4 {
            assert!(history.redo(&mut schematic).unwrap());
        }
        assert!(schematic.is_empty());
    }

    #[test]
    fn label_edits_are_reversible() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let id = NetLabelId(schematic.allocate_id().unwrap());
        let label = NetLabel::new(id, GridPos::new(1, 1), "OUT").unwrap();
        history
            .execute(&mut schematic, Command::AddLabel(label.clone()))
            .unwrap();
        let mut renamed = label.clone();
        renamed.text = "SIGNAL".to_owned();
        history
            .execute(
                &mut schematic,
                Command::ReplaceLabel {
                    old: label,
                    new: renamed.clone(),
                },
            )
            .unwrap();
        assert_eq!(schematic.label(id), Some(&renamed));
        history.undo(&mut schematic).unwrap();
        assert_eq!(
            schematic.label(id).map(|label| label.text.as_str()),
            Some("OUT")
        );
    }
}
