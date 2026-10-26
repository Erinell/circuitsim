use std::collections::BTreeSet;

use editor_core::{
    extract_netlist, Command, GridPos, Junction, JunctionId, NetLabel, NetLabelId, ObjectRef,
    Orientation, PinKey, Schematic, Symbol, SymbolId, SymbolKind, SymbolRegistry, UndoStack, Wire,
    WireId,
};
use proptest::prelude::*;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    fn next_u64(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn below(&mut self, upper: usize) -> usize {
        if upper == 0 {
            0
        } else {
            (self.next_u64() % upper as u64) as usize
        }
    }

    fn range(&mut self, low: i32, high: i32) -> i32 {
        let span = i64::from(high) - i64::from(low) + 1;
        low + (self.next_u64() % span as u64) as i32
    }

    fn boolean(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

fn random_command(
    rng: &mut Rng,
    schematic: &mut Schematic,
    registry: &SymbolRegistry,
) -> Option<Command> {
    match rng.below(8) {
        0 | 1 => {
            let kinds = [
                SymbolKind::Resistor,
                SymbolKind::Capacitor,
                SymbolKind::Inductor,
                SymbolKind::VoltageSource,
                SymbolKind::CurrentSource,
                SymbolKind::Ground,
                SymbolKind::Switch,
                SymbolKind::Diode,
                SymbolKind::Led,
                SymbolKind::Zener,
                SymbolKind::OpAmp,
            ];
            let kind = kinds[rng.below(kinds.len())].clone();
            let def = registry.get(&kind)?;
            let id = SymbolId(schematic.allocate_id().ok()?);
            let orientation = [
                Orientation::R0,
                Orientation::R90,
                Orientation::R180,
                Orientation::R270,
            ][rng.below(4)];
            let symbol = Symbol::from_def(
                id,
                def,
                GridPos::new(rng.range(-8, 8), rng.range(-8, 8)),
                format!("S{}", id.0),
                orientation,
                rng.boolean(),
            );
            Some(Command::AddSymbol(symbol))
        }
        2 => {
            let id = WireId(schematic.allocate_id().ok()?);
            let horizontal = rng.boolean();
            let start = if horizontal {
                GridPos::new(rng.range(-8, 4), rng.range(-8, 8))
            } else {
                GridPos::new(rng.range(-8, 8), rng.range(-8, 4))
            };
            let length = rng.range(1, 4);
            let direction = if rng.boolean() { 1 } else { -1 };
            let end = if horizontal {
                GridPos::new(start.x + direction * length, start.y)
            } else {
                GridPos::new(start.x, start.y + direction * length)
            };
            let wire = Wire::new(id, start, end).ok()?;
            Some(Command::AddWire(wire))
        }
        3 => {
            for _ in 0..8 {
                let at = GridPos::new(rng.range(-8, 8), rng.range(-8, 8));
                if schematic.junctions().any(|junction| junction.at == at) {
                    continue;
                }
                let id = JunctionId(schematic.allocate_id().ok()?);
                return Some(Command::AddJunction(Junction::new(id, at)));
            }
            None
        }
        4 => {
            let id = NetLabelId(schematic.allocate_id().ok()?);
            let labels = ["A", "B", "OUT", "CLK", "DATA"];
            let at = GridPos::new(rng.range(-8, 8), rng.range(-8, 8));
            let label = NetLabel::new(id, at, labels[rng.below(labels.len())]).ok()?;
            Some(Command::AddLabel(label))
        }
        5 => {
            let refs = schematic.object_refs();
            if refs.is_empty() {
                return None;
            }
            let count = 1 + rng.below(refs.len().min(3));
            let mut chosen = BTreeSet::new();
            for _ in 0..count {
                chosen.insert(refs[rng.below(refs.len())]);
            }
            let objects: Vec<ObjectRef> = chosen.into_iter().collect();
            if objects.is_empty() {
                return None;
            }
            let delta = loop {
                let delta = GridPos::new(rng.range(-2, 2), rng.range(-2, 2));
                if delta != GridPos::default() {
                    break delta;
                }
            };
            Some(Command::MoveObjects { objects, delta })
        }
        6 => {
            let symbols: Vec<Symbol> = schematic.symbols().cloned().collect();
            if symbols.is_empty() {
                return None;
            }
            let old = symbols[rng.below(symbols.len())].clone();
            let mut new = old.clone();
            new.orientation = old.orientation.next();
            new.params.insert(
                "test_value".to_owned(),
                sim_core::netlist::ParamValue::Number(rng.range(1, 100) as f64),
            );
            Some(Command::ReplaceSymbol { old, new })
        }
        _ => {
            let refs = schematic.object_refs();
            if refs.is_empty() {
                return None;
            }
            match refs[rng.below(refs.len())] {
                ObjectRef::Symbol(id) => schematic.symbol(id).cloned().map(Command::RemoveSymbol),
                ObjectRef::Wire(id) => schematic.wire(id).cloned().map(Command::RemoveWire),
                ObjectRef::Junction(id) => {
                    schematic.junction(id).cloned().map(Command::RemoveJunction)
                }
                ObjectRef::Label(id) => schematic.label(id).cloned().map(Command::RemoveLabel),
            }
        }
    }
}

fn random_schematic(seed: u64, registry: &SymbolRegistry) -> Schematic {
    let mut rng = Rng::new(seed);
    let mut schematic = Schematic::new();
    let mut history = UndoStack::new();
    for _ in 0..30 {
        if let Some(command) = random_command(&mut rng, &mut schematic, registry) {
            let _ = history.execute(&mut schematic, command);
        }
    }
    schematic
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn undo_redo_restores_exact_content(seed in any::<u64>()) {
        let registry = SymbolRegistry::builtin();
        let mut rng = Rng::new(seed);
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let initial = schematic.clone();

        for _ in 0..40 {
            if let Some(command) = random_command(&mut rng, &mut schematic, &registry) {
                let _ = history.execute(&mut schematic, command);
            }
        }

        let final_state = schematic.clone();
        let final_undo_len = history.undo_len();
        while history.undo(&mut schematic).unwrap() {}
        prop_assert!(schematic.content_eq(&initial));
        while history.redo(&mut schematic).unwrap() {}
        prop_assert!(schematic.content_eq(&final_state));
        prop_assert_eq!(history.undo_len(), final_undo_len);
    }

    #[test]
    fn extraction_is_deterministic_and_consistent(seed in any::<u64>()) {
        let registry = SymbolRegistry::builtin();
        let schematic = random_schematic(seed, &registry);
        let first = extract_netlist(&schematic, &registry).unwrap();
        let second = extract_netlist(&schematic, &registry).unwrap();
        prop_assert_eq!(&first, &second);
        prop_assert!(first.netlist.validate().is_ok());
        prop_assert_eq!(first.nets.len(), first.netlist.nodes.len());

        let mut pin_count = 0_usize;
        for symbol in schematic.symbols() {
            let def = registry.get(&symbol.kind).unwrap();
            for pin in 0..def.pin_count() {
                pin_count += 1;
                let key = PinKey {
                    symbol: symbol.id,
                    pin: u16::try_from(pin).unwrap(),
                };
                prop_assert!(first.net_of_pin.contains_key(&key));
            }
        }
        prop_assert_eq!(first.net_of_pin.len(), pin_count);

        for (index, net) in first.nets.iter().enumerate() {
            prop_assert_eq!(net.node.0 as usize, index);
            prop_assert!(!net.pins.is_empty());
        }

        let names: BTreeSet<&str> = first
            .netlist
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect();
        prop_assert_eq!(names.len(), first.netlist.nodes.len());
    }
}
