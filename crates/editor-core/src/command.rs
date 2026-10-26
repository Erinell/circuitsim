//! Reversible editing commands.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::grid::GridPos;
use crate::schematic::{Junction, NetLabel, ObjectRef, Schematic, SchematicError, Wire};
use crate::symbol::Symbol;

/// Atomic or composite schematic edit.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    AddSymbol(Symbol),
    RemoveSymbol(Symbol),
    AddWire(Wire),
    RemoveWire(Wire),
    AddJunction(Junction),
    RemoveJunction(Junction),
    AddLabel(NetLabel),
    RemoveLabel(NetLabel),
    MoveObjects {
        objects: Vec<ObjectRef>,
        delta: GridPos,
    },
    ReplaceSymbol {
        old: Symbol,
        new: Symbol,
    },
    ReplaceWire {
        old: Wire,
        new: Wire,
    },
    ReplaceLabel {
        old: NetLabel,
        new: NetLabel,
    },
    Composite(Vec<Command>),
}

/// Error returned when a command cannot be applied or reverted.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum EditError {
    #[error("object already exists: {0:?}")]
    AlreadyExists(ObjectRef),
    #[error("object not found: {0:?}")]
    NotFound(ObjectRef),
    #[error("command does not match the current schematic state")]
    StaleCommand,
    #[error("invalid command: {0}")]
    InvalidCommand(&'static str),
    #[error("position overflow")]
    PositionOverflow,
    #[error("schematic validation failed: {0}")]
    Schematic(#[from] SchematicError),
    #[error("object identifier space is exhausted")]
    IdSpaceExhausted,
}

impl Command {
    pub(crate) fn apply(&self, schematic: &mut Schematic) -> Result<(), EditError> {
        match self {
            Self::AddSymbol(symbol) => insert_symbol(schematic, symbol.clone()),
            Self::RemoveSymbol(symbol) => {
                let current = schematic
                    .symbols
                    .get(&symbol.id)
                    .ok_or(EditError::NotFound(ObjectRef::Symbol(symbol.id)))?;
                if current != symbol {
                    return Err(EditError::StaleCommand);
                }
                schematic.symbols.remove(&symbol.id);
                Ok(())
            }
            Self::AddWire(wire) => insert_wire(schematic, wire.clone()),
            Self::RemoveWire(wire) => {
                let current = schematic
                    .wires
                    .get(&wire.id)
                    .ok_or(EditError::NotFound(ObjectRef::Wire(wire.id)))?;
                if current != wire {
                    return Err(EditError::StaleCommand);
                }
                schematic.wires.remove(&wire.id);
                Ok(())
            }
            Self::AddJunction(junction) => insert_junction(schematic, junction.clone()),
            Self::RemoveJunction(junction) => {
                let current = schematic
                    .junctions
                    .get(&junction.id)
                    .ok_or(EditError::NotFound(ObjectRef::Junction(junction.id)))?;
                if current != junction {
                    return Err(EditError::StaleCommand);
                }
                schematic.junctions.remove(&junction.id);
                Ok(())
            }
            Self::AddLabel(label) => insert_label(schematic, label.clone()),
            Self::RemoveLabel(label) => {
                let current = schematic
                    .labels
                    .get(&label.id)
                    .ok_or(EditError::NotFound(ObjectRef::Label(label.id)))?;
                if current != label {
                    return Err(EditError::StaleCommand);
                }
                schematic.labels.remove(&label.id);
                Ok(())
            }
            Self::MoveObjects { objects, delta } => {
                apply_move(schematic, objects, *delta)?;
                Ok(())
            }
            Self::ReplaceSymbol { old, new } => replace_symbol(schematic, old, new),
            Self::ReplaceWire { old, new } => replace_wire(schematic, old, new),
            Self::ReplaceLabel { old, new } => replace_label(schematic, old, new),
            Self::Composite(commands) => {
                if commands.is_empty() {
                    return Err(EditError::InvalidCommand("composite command is empty"));
                }
                for command in commands {
                    command.apply(schematic)?;
                }
                Ok(())
            }
        }
    }

    pub(crate) fn undo(&self, schematic: &mut Schematic) -> Result<(), EditError> {
        match self {
            Self::AddSymbol(symbol) => {
                remove_object(schematic, ObjectRef::Symbol(symbol.id))?;
                Ok(())
            }
            Self::RemoveSymbol(symbol) => {
                let id = symbol.id;
                if schematic.symbols.insert(id, symbol.clone()).is_some() {
                    return Err(EditError::AlreadyExists(ObjectRef::Symbol(id)));
                }
                bump_next_id(schematic, id.0)?;
                Ok(())
            }
            Self::AddWire(wire) => {
                remove_object(schematic, ObjectRef::Wire(wire.id))?;
                Ok(())
            }
            Self::RemoveWire(wire) => {
                let id = wire.id;
                if schematic.wires.insert(id, wire.clone()).is_some() {
                    return Err(EditError::AlreadyExists(ObjectRef::Wire(id)));
                }
                bump_next_id(schematic, id.0)?;
                Ok(())
            }
            Self::AddJunction(junction) => {
                remove_object(schematic, ObjectRef::Junction(junction.id))?;
                Ok(())
            }
            Self::RemoveJunction(junction) => {
                let id = junction.id;
                if schematic.junctions.insert(id, junction.clone()).is_some() {
                    return Err(EditError::AlreadyExists(ObjectRef::Junction(id)));
                }
                bump_next_id(schematic, id.0)?;
                Ok(())
            }
            Self::AddLabel(label) => {
                remove_object(schematic, ObjectRef::Label(label.id))?;
                Ok(())
            }
            Self::RemoveLabel(label) => {
                let id = label.id;
                if schematic.labels.insert(id, label.clone()).is_some() {
                    return Err(EditError::AlreadyExists(ObjectRef::Label(id)));
                }
                bump_next_id(schematic, id.0)?;
                Ok(())
            }
            Self::MoveObjects { objects, delta } => {
                let inverse = delta.checked_neg().ok_or(EditError::PositionOverflow)?;
                apply_move(schematic, objects, inverse)?;
                Ok(())
            }
            Self::ReplaceSymbol { old, new } => replace_symbol(schematic, new, old),
            Self::ReplaceWire { old, new } => replace_wire(schematic, new, old),
            Self::ReplaceLabel { old, new } => replace_label(schematic, new, old),
            Self::Composite(commands) => {
                for command in commands.iter().rev() {
                    command.undo(schematic)?;
                }
                Ok(())
            }
        }
    }
}

fn insert_symbol(schematic: &mut Schematic, symbol: Symbol) -> Result<(), EditError> {
    let id = symbol.id;
    validate_new_id(id.0)?;
    if schematic.symbols.contains_key(&id) {
        return Err(EditError::AlreadyExists(ObjectRef::Symbol(id)));
    }
    schematic.symbols.insert(id, symbol);
    bump_next_id(schematic, id.0)
}

fn insert_wire(schematic: &mut Schematic, wire: Wire) -> Result<(), EditError> {
    let id = wire.id;
    validate_new_id(id.0)?;
    if schematic.wires.contains_key(&id) {
        return Err(EditError::AlreadyExists(ObjectRef::Wire(id)));
    }
    schematic.wires.insert(id, wire);
    bump_next_id(schematic, id.0)
}

fn insert_junction(schematic: &mut Schematic, junction: Junction) -> Result<(), EditError> {
    let id = junction.id;
    validate_new_id(id.0)?;
    if schematic.junctions.contains_key(&id) {
        return Err(EditError::AlreadyExists(ObjectRef::Junction(id)));
    }
    schematic.junctions.insert(id, junction);
    bump_next_id(schematic, id.0)
}

fn insert_label(schematic: &mut Schematic, label: NetLabel) -> Result<(), EditError> {
    let id = label.id;
    validate_new_id(id.0)?;
    if schematic.labels.contains_key(&id) {
        return Err(EditError::AlreadyExists(ObjectRef::Label(id)));
    }
    schematic.labels.insert(id, label);
    bump_next_id(schematic, id.0)
}

fn remove_object(schematic: &mut Schematic, object: ObjectRef) -> Result<(), EditError> {
    let removed = match object {
        ObjectRef::Symbol(id) => schematic.symbols.remove(&id).is_some(),
        ObjectRef::Wire(id) => schematic.wires.remove(&id).is_some(),
        ObjectRef::Junction(id) => schematic.junctions.remove(&id).is_some(),
        ObjectRef::Label(id) => schematic.labels.remove(&id).is_some(),
    };
    if removed {
        Ok(())
    } else {
        Err(EditError::NotFound(object))
    }
}

fn replace_symbol(schematic: &mut Schematic, old: &Symbol, new: &Symbol) -> Result<(), EditError> {
    if old.id != new.id {
        return Err(EditError::InvalidCommand(
            "replacement symbol must keep the same identifier",
        ));
    }
    if old == new {
        return Err(EditError::InvalidCommand("replacement is a no-op"));
    }
    let current = schematic
        .symbols
        .get(&old.id)
        .ok_or(EditError::NotFound(ObjectRef::Symbol(old.id)))?;
    if current != old {
        return Err(EditError::StaleCommand);
    }
    schematic.symbols.insert(new.id, new.clone());
    Ok(())
}

fn replace_wire(schematic: &mut Schematic, old: &Wire, new: &Wire) -> Result<(), EditError> {
    if old.id != new.id {
        return Err(EditError::InvalidCommand(
            "replacement wire must keep the same identifier",
        ));
    }
    if old == new {
        return Err(EditError::InvalidCommand("replacement is a no-op"));
    }
    let current = schematic
        .wires
        .get(&old.id)
        .ok_or(EditError::NotFound(ObjectRef::Wire(old.id)))?;
    if current != old {
        return Err(EditError::StaleCommand);
    }
    schematic.wires.insert(new.id, new.clone());
    Ok(())
}

fn replace_label(
    schematic: &mut Schematic,
    old: &NetLabel,
    new: &NetLabel,
) -> Result<(), EditError> {
    if old.id != new.id {
        return Err(EditError::InvalidCommand(
            "replacement label must keep the same identifier",
        ));
    }
    if old == new {
        return Err(EditError::InvalidCommand("replacement is a no-op"));
    }
    let current = schematic
        .labels
        .get(&old.id)
        .ok_or(EditError::NotFound(ObjectRef::Label(old.id)))?;
    if current != old {
        return Err(EditError::StaleCommand);
    }
    schematic.labels.insert(new.id, new.clone());
    Ok(())
}

fn apply_move(
    schematic: &mut Schematic,
    objects: &[ObjectRef],
    delta: GridPos,
) -> Result<(), EditError> {
    if objects.is_empty() {
        return Err(EditError::InvalidCommand("move command has no objects"));
    }
    if delta == GridPos::default() {
        return Err(EditError::InvalidCommand("move delta is zero"));
    }
    let unique: BTreeSet<ObjectRef> = objects.iter().copied().collect();
    if unique.len() != objects.len() {
        return Err(EditError::InvalidCommand(
            "move command contains duplicate objects",
        ));
    }
    for object in objects {
        match object {
            ObjectRef::Symbol(id) => {
                let symbol = schematic
                    .symbols
                    .get_mut(id)
                    .ok_or(EditError::NotFound(*object))?;
                symbol.at = symbol
                    .at
                    .checked_add(delta)
                    .ok_or(EditError::PositionOverflow)?;
            }
            ObjectRef::Wire(id) => {
                let wire = schematic
                    .wires
                    .get_mut(id)
                    .ok_or(EditError::NotFound(*object))?;
                wire.a = wire
                    .a
                    .checked_add(delta)
                    .ok_or(EditError::PositionOverflow)?;
                wire.b = wire
                    .b
                    .checked_add(delta)
                    .ok_or(EditError::PositionOverflow)?;
            }
            ObjectRef::Junction(id) => {
                let junction = schematic
                    .junctions
                    .get_mut(id)
                    .ok_or(EditError::NotFound(*object))?;
                junction.at = junction
                    .at
                    .checked_add(delta)
                    .ok_or(EditError::PositionOverflow)?;
            }
            ObjectRef::Label(id) => {
                let label = schematic
                    .labels
                    .get_mut(id)
                    .ok_or(EditError::NotFound(*object))?;
                label.at = label
                    .at
                    .checked_add(delta)
                    .ok_or(EditError::PositionOverflow)?;
            }
        }
    }
    Ok(())
}

fn validate_new_id(id: u32) -> Result<(), EditError> {
    if id == 0 {
        Err(EditError::InvalidCommand("object identifier 0 is reserved"))
    } else {
        Ok(())
    }
}

fn bump_next_id(schematic: &mut Schematic, id: u32) -> Result<(), EditError> {
    let next = id.checked_add(1).ok_or(EditError::IdSpaceExhausted)?;
    schematic.next_id = schematic.next_id.max(next);
    Ok(())
}
