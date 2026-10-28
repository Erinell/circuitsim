//! Union-find based schematic connectivity extraction and ERC generation.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sim_core::netlist::{
    ComponentKind, NetName, Netlist, NetlistBuilder, NetlistError, NodeId, NodeKind,
};
use thiserror::Error;

use crate::erc::{ErcCode, ErcIssue, ErcReport};
use crate::grid::{segment_relation, GridPos, SegmentRelation};
use crate::naming::NetNameAllocator;
use crate::schematic::{NetLabelId, ObjectRef, Schematic, SchematicError, Wire, WireId};
use crate::symbol::{SymbolId, SymbolKind, SymbolRegistry};

/// Stable reference to one pin of a placed symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PinKey {
    pub symbol: SymbolId,
    pub pin: u16,
}

/// One electrical net with the schematic objects that belong to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtractedNet {
    pub node: NodeId,
    pub name: NetName,
    pub pins: Vec<PinKey>,
    pub wires: Vec<WireId>,
    pub labels: Vec<NetLabelId>,
}

/// Complete extraction result, including ERC findings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extraction {
    pub netlist: Netlist,
    pub erc: ErcReport,
    pub nets: Vec<ExtractedNet>,
    pub net_of_pin: BTreeMap<PinKey, NodeId>,
}

/// Error preventing a schematic from producing a netlist.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum ExtractError {
    #[error("schematic is invalid: {0}")]
    Schematic(#[from] SchematicError),
    #[error("unknown symbol kind '{kind}' on symbol {symbol:?}")]
    UnknownSymbolKind { symbol: SymbolId, kind: String },
    #[error("symbol {symbol:?} has too many pins")]
    TooManyPins { symbol: SymbolId },
    #[error("pin position overflow on symbol {symbol:?}")]
    PinPositionOverflow { symbol: SymbolId },
    #[error("pin position on symbol {symbol:?} lies outside the supported grid")]
    PinOutOfBounds { symbol: SymbolId },
    #[error("pin {pin} of symbol {symbol:?} has no net mapping")]
    MissingPinMapping { symbol: SymbolId, pin: u16 },
    #[error("netlist error: {0}")]
    Netlist(#[from] NetlistError),
    #[error("invalid net name: {0}")]
    Name(#[from] sim_core::netlist::NameError),
}

#[derive(Clone, Debug)]
struct PinRecord {
    key: PinKey,
    at: GridPos,
    pin_name: String,
    is_ground: bool,
}

#[derive(Clone, Debug)]
struct WireRecord {
    id: WireId,
    wire: Wire,
}

#[derive(Default)]
struct Group {
    pins: Vec<usize>,
    wires: Vec<usize>,
    labels: Vec<NetLabelId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct NetRootKey {
    ground_rank: u8,
    pin: Option<PinKey>,
    wire: Option<WireId>,
    label: Option<NetLabelId>,
    root: usize,
}

/// Extracts a validated netlist and ERC report from a schematic.
pub fn extract_netlist(
    schematic: &Schematic,
    registry: &SymbolRegistry,
) -> Result<Extraction, ExtractError> {
    schematic.validate()?;

    let mut pins = Vec::new();
    for symbol in schematic.symbols() {
        if symbol.kind.is_measurement() {
            continue;
        }
        let def = registry
            .get(&symbol.kind)
            .ok_or_else(|| ExtractError::UnknownSymbolKind {
                symbol: symbol.id,
                kind: symbol.kind.key().to_owned(),
            })?;
        if def.pin_count() > usize::from(u16::MAX) {
            return Err(ExtractError::TooManyPins { symbol: symbol.id });
        }
        for (pin_index, pin) in def.pins.iter().enumerate() {
            let pin_index_u16 = u16::try_from(pin_index)
                .map_err(|_| ExtractError::TooManyPins { symbol: symbol.id })?;
            let at = symbol
                .pin_position_of(pin)
                .ok_or(ExtractError::PinPositionOverflow { symbol: symbol.id })?;
            if !at.is_within_bounds() {
                return Err(ExtractError::PinOutOfBounds { symbol: symbol.id });
            }
            pins.push(PinRecord {
                key: PinKey {
                    symbol: symbol.id,
                    pin: pin_index_u16,
                },
                at,
                pin_name: pin.name.clone(),
                is_ground: symbol.kind.is_ground(),
            });
        }
    }

    let wires: Vec<WireRecord> = schematic
        .wires()
        .map(|wire| WireRecord {
            id: wire.id,
            wire: wire.clone(),
        })
        .collect();

    let mut pin_points: BTreeMap<GridPos, Vec<usize>> = BTreeMap::new();
    for (index, pin) in pins.iter().enumerate() {
        pin_points.entry(pin.at).or_default().push(index);
    }

    let mut union_find = UnionFind::new(pins.len() + wires.len());
    for indices in pin_points.values() {
        if let Some((&first, rest)) = indices.split_first() {
            for &other in rest {
                union_find.union(first, other);
            }
        }
    }

    let mut pin_wire_count = vec![0_usize; pins.len()];
    for (pin_index, pin) in pins.iter().enumerate() {
        for (wire_index, wire) in wires.iter().enumerate() {
            if wire.wire.contains(pin.at) {
                pin_wire_count[pin_index] += 1;
                union_find.union(pin_index, pins.len() + wire_index);
            }
        }
    }

    let mut erc = ErcReport::new();
    for left in 0..wires.len() {
        for right in (left + 1)..wires.len() {
            let relation = segment_relation(
                wires[left].wire.a,
                wires[left].wire.b,
                wires[right].wire.a,
                wires[right].wire.b,
            );
            match relation {
                SegmentRelation::Disjoint => {}
                SegmentRelation::CollinearTouch(_) => {
                    union_find.union(pins.len() + left, pins.len() + right);
                }
                SegmentRelation::CollinearOverlap { start, end } => {
                    union_find.union(pins.len() + left, pins.len() + right);
                    erc.push(
                        ErcIssue::new(
                            ErcCode::WireOverlap,
                            format!(
                                "wires {:?} and {:?} overlap from {start:?} to {end:?}",
                                wires[left].id, wires[right].id
                            ),
                        )
                        .at(start)
                        .subject(ObjectRef::Wire(wires[left].id)),
                    );
                }
                SegmentRelation::Perpendicular {
                    at,
                    endpoint_a,
                    endpoint_b,
                } => {
                    if endpoint_a || endpoint_b || junction_exists(schematic, at) {
                        union_find.union(pins.len() + left, pins.len() + right);
                    }
                }
            }
        }
    }

    let mut junction_positions = BTreeSet::new();
    for junction in schematic.junctions() {
        junction_positions.insert(junction.at);
        let containing: Vec<usize> = wires
            .iter()
            .enumerate()
            .filter(|(_, wire)| wire.wire.contains(junction.at))
            .map(|(index, _)| index)
            .collect();
        if containing.is_empty() {
            if pin_points.contains_key(&junction.at) {
                erc.push(
                    ErcIssue::new(
                        ErcCode::RedundantJunction,
                        "junction is placed directly on a pin without a wire",
                    )
                    .at(junction.at)
                    .subject(ObjectRef::Junction(junction.id)),
                );
            } else {
                erc.push(
                    ErcIssue::new(ErcCode::JunctionOffWire, "junction is not on any wire")
                        .at(junction.at)
                        .subject(ObjectRef::Junction(junction.id)),
                );
            }
        } else {
            if containing.len() == 1 && !pin_points.contains_key(&junction.at) {
                erc.push(
                    ErcIssue::new(
                        ErcCode::RedundantJunction,
                        "junction connects only one wire and no pin",
                    )
                    .at(junction.at)
                    .subject(ObjectRef::Junction(junction.id)),
                );
            }
            let first = containing[0];
            for &other in &containing[1..] {
                union_find.union(pins.len() + first, pins.len() + other);
            }
        }
    }

    let mut label_roots: BTreeMap<usize, Vec<NetLabelId>> = BTreeMap::new();
    for label in schematic.labels() {
        let mut roots = BTreeSet::new();
        for (wire_index, wire) in wires.iter().enumerate() {
            if wire.wire.contains(label.at) {
                roots.insert(union_find.find(pins.len() + wire_index));
            }
        }
        if let Some(indices) = pin_points.get(&label.at) {
            for &pin_index in indices {
                roots.insert(union_find.find(pin_index));
            }
        }
        match roots.len() {
            0 => erc.push(
                ErcIssue::new(ErcCode::LabelOffWire, "net label is not on any conductor")
                    .at(label.at)
                    .subject(ObjectRef::Label(label.id)),
            ),
            1 => {
                let root = roots.into_iter().next().unwrap_or(usize::MAX);
                label_roots.entry(root).or_default().push(label.id);
            }
            _ => erc.push(
                ErcIssue::new(
                    ErcCode::AmbiguousLabel,
                    "net label touches several unconnected nets",
                )
                .at(label.at)
                .subject(ObjectRef::Label(label.id)),
            ),
        }
    }

    let mut groups: BTreeMap<usize, Group> = BTreeMap::new();
    for (pin_index, _) in pins.iter().enumerate() {
        let root = union_find.find(pin_index);
        groups.entry(root).or_default().pins.push(pin_index);
    }
    for (wire_index, _) in wires.iter().enumerate() {
        let root = union_find.find(pins.len() + wire_index);
        groups.entry(root).or_default().wires.push(wire_index);
    }
    for (root, labels) in label_roots {
        groups.entry(root).or_default().labels.extend(labels);
    }

    let ground_root_list: Vec<usize> = groups
        .iter()
        .filter(|(_, group)| group.pins.iter().any(|&index| pins[index].is_ground))
        .map(|(&root, _)| root)
        .collect();
    if let Some((&primary, others)) = ground_root_list.split_first() {
        if !others.is_empty() {
            for &root in others {
                if let Some(group) = groups.remove(&root) {
                    if let Some(target) = groups.get_mut(&primary) {
                        target.pins.extend(group.pins);
                        target.wires.extend(group.wires);
                        target.labels.extend(group.labels);
                    }
                }
            }
            if let Some(group) = groups.get_mut(&primary) {
                group.pins.sort_by_key(|&index| pins[index].key);
                group.wires.sort_by_key(|&index| wires[index].id);
                group.labels.sort();
            }
            let count = others.len() + 1;
            let mut issue = ErcIssue::new(
                ErcCode::MultipleGround,
                format!("{count} symboles de masse sont reliés à la masse commune '0'"),
            );
            if let Some(group) = groups.get(&primary) {
                if let Some(&index) = group.pins.iter().find(|&&index| pins[index].is_ground) {
                    issue = issue
                        .at(pins[index].at)
                        .subject(ObjectRef::Symbol(pins[index].key.symbol));
                }
            }
            erc.push(issue);
        }
    }

    for group in groups.values() {
        if group.pins.is_empty() {
            for &wire_index in &group.wires {
                let wire = &wires[wire_index];
                erc.push(
                    ErcIssue::new(
                        ErcCode::DanglingWire,
                        format!("wire {:?} is not connected to any pin", wire.id),
                    )
                    .at(wire.wire.a)
                    .subject(ObjectRef::Wire(wire.id)),
                );
            }
            continue;
        }
        if group.labels.len() > 1 {
            if let Some(label) = group.labels.first().and_then(|id| schematic.label(*id)) {
                erc.push(
                    ErcIssue::new(
                        ErcCode::MultipleLabelsOnNet,
                        format!(
                            "net has {} labels; the lexicographically smallest name is used",
                            group.labels.len()
                        ),
                    )
                    .at(label.at)
                    .subject(ObjectRef::Label(label.id)),
                );
            }
        }
    }

    let label_positions: BTreeSet<GridPos> = schematic.labels().map(|label| label.at).collect();
    for (point, indices) in &pin_points {
        if indices.len() > 1 {
            let first = &pins[indices[0]];
            erc.push(
                ErcIssue::new(
                    ErcCode::StackedPins,
                    format!("{} pins are stacked at one position", indices.len()),
                )
                .at(*point)
                .subject(ObjectRef::Symbol(first.key.symbol)),
            );
        }
        if indices.len() == 1 {
            let pin_index = indices[0];
            if pin_wire_count[pin_index] == 0 && !label_positions.contains(point) {
                let pin = &pins[pin_index];
                let label = schematic
                    .symbol(pin.key.symbol)
                    .map(|symbol| symbol.label.as_str())
                    .unwrap_or("?");
                erc.push(
                    ErcIssue::new(
                        ErcCode::FloatingPin,
                        format!("pin '{}' of {label} is floating", pin.pin_name),
                    )
                    .at(pin.at)
                    .subject(ObjectRef::Symbol(pin.key.symbol)),
                );
            }
        }
    }

    for (wire_index, record) in wires.iter().enumerate() {
        for endpoint in [record.wire.a, record.wire.b] {
            let has_pin = pin_points.contains_key(&endpoint);
            let has_junction = junction_positions.contains(&endpoint);
            let has_other_wire = wires.iter().enumerate().any(|(other_index, other)| {
                other_index != wire_index && other.wire.contains(endpoint)
            });
            if !has_pin && !has_junction && !has_other_wire {
                erc.push(
                    ErcIssue::new(
                        ErcCode::UnconnectedWireEnd,
                        format!("wire {:?} has an unconnected end", record.id),
                    )
                    .at(endpoint)
                    .subject(ObjectRef::Wire(record.id)),
                );
            }
        }
    }

    let ground_roots: BTreeSet<usize> = groups
        .iter()
        .filter(|(_, group)| group.pins.iter().any(|&index| pins[index].is_ground))
        .map(|(&root, _)| root)
        .collect();
    if ground_roots.is_empty() {
        erc.push(ErcIssue::new(
            ErcCode::NoGround,
            "netlist has no ground symbol",
        ));
    }

    let mut net_roots: Vec<NetRootKey> = groups
        .iter()
        .filter(|(_, group)| !group.pins.is_empty())
        .map(|(&root, group)| {
            let pin_key = group.pins.iter().map(|&index| pins[index].key).min();
            let wire_key = group.wires.iter().map(|&index| wires[index].id).min();
            let label_key = group.labels.iter().copied().min();
            let ground_rank = if ground_roots.contains(&root) { 0 } else { 1 };
            NetRootKey {
                ground_rank,
                pin: pin_key,
                wire: wire_key,
                label: label_key,
                root,
            }
        })
        .collect();
    net_roots.sort();

    let mut builder = NetlistBuilder::new();
    let mut nets = Vec::with_capacity(net_roots.len());
    let mut net_of_pin = BTreeMap::new();
    let mut allocator = NetNameAllocator::new();
    let mut ground_name_assigned = false;

    for key in net_roots {
        let root = key.root;
        let Some(group) = groups.get(&root) else {
            continue;
        };
        let is_ground = ground_roots.contains(&root);
        let preferred = group
            .labels
            .iter()
            .filter_map(|id| schematic.label(*id))
            .map(|label| label.text.as_str())
            .min();
        let allocated = if is_ground && !ground_name_assigned {
            ground_name_assigned = true;
            for label_id in &group.labels {
                if let Some(label) = schematic.label(*label_id) {
                    erc.push(
                        ErcIssue::new(
                            ErcCode::GroundLabelIgnored,
                            format!(
                                "ground net is always named '0'; label '{}' is ignored",
                                label.text
                            ),
                        )
                        .at(label.at)
                        .subject(ObjectRef::Label(label.id)),
                    );
                }
            }
            crate::naming::AllocatedName {
                name: "0".to_owned(),
                renamed: false,
            }
        } else {
            allocator.allocate(preferred)
        };

        if allocated.renamed {
            let subject = preferred
                .and_then(|text| {
                    group.labels.iter().find(|id| {
                        schematic
                            .label(**id)
                            .is_some_and(|label| label.text == text)
                    })
                })
                .copied();
            let issue = if preferred == Some("0") {
                ErcIssue::new(
                    ErcCode::ReservedNetName,
                    "net name '0' is reserved for ground and was renamed",
                )
            } else {
                ErcIssue::new(
                    ErcCode::DuplicateNetName,
                    format!(
                        "net name '{}' is already used and was renamed to '{}'",
                        preferred.unwrap_or("?"),
                        allocated.name
                    ),
                )
            };
            let issue = if let Some(subject) = subject {
                let at = schematic.label(subject).map(|label| label.at);
                let issue = issue.subject(ObjectRef::Label(subject));
                if let Some(at) = at {
                    issue.at(at)
                } else {
                    issue
                }
            } else {
                issue
            };
            erc.push(issue);
        }

        let name = NetName::new(allocated.name)?;
        let kind = if is_ground {
            NodeKind::Ground
        } else {
            NodeKind::Electrical
        };
        let node = builder.add_node(name.as_str(), kind)?;
        let extracted = ExtractedNet {
            node,
            name: name.clone(),
            pins: group.pins.iter().map(|&index| pins[index].key).collect(),
            wires: group.wires.iter().map(|&index| wires[index].id).collect(),
            labels: group.labels.clone(),
        };
        for &pin_index in &group.pins {
            net_of_pin.insert(pins[pin_index].key, node);
        }
        nets.push(extracted);
    }

    for symbol in schematic.symbols() {
        if symbol.kind.is_measurement() {
            continue;
        }
        let def = registry
            .get(&symbol.kind)
            .ok_or_else(|| ExtractError::UnknownSymbolKind {
                symbol: symbol.id,
                kind: symbol.kind.key().to_owned(),
            })?;
        let mut pins_for_component = Vec::with_capacity(def.pin_count());
        for (pin_index, _) in def.pins.iter().enumerate() {
            let pin_index_u16 = u16::try_from(pin_index)
                .map_err(|_| ExtractError::TooManyPins { symbol: symbol.id })?;
            let key = PinKey {
                symbol: symbol.id,
                pin: pin_index_u16,
            };
            let node = net_of_pin
                .get(&key)
                .copied()
                .ok_or(ExtractError::MissingPinMapping {
                    symbol: symbol.id,
                    pin: pin_index_u16,
                })?;
            pins_for_component.push(node);
        }
        builder.add_component(
            component_kind(&symbol.kind),
            symbol.label.clone(),
            symbol.params.clone(),
            pins_for_component,
        )?;
    }

    let netlist = builder.build()?;
    erc.sort();
    Ok(Extraction {
        netlist,
        erc,
        nets,
        net_of_pin,
    })
}

fn junction_exists(schematic: &Schematic, at: GridPos) -> bool {
    schematic.junctions().any(|junction| junction.at == at)
}

fn component_kind(kind: &SymbolKind) -> ComponentKind {
    match kind {
        SymbolKind::Resistor => ComponentKind::Resistor,
        SymbolKind::Capacitor => ComponentKind::Capacitor,
        SymbolKind::Inductor => ComponentKind::Inductor,
        SymbolKind::VoltageSource => ComponentKind::VoltageSource,
        SymbolKind::CurrentSource => ComponentKind::CurrentSource,
        SymbolKind::Ground => ComponentKind::Ground,
        SymbolKind::Switch => ComponentKind::Switch,
        SymbolKind::Diode => ComponentKind::Diode,
        SymbolKind::Led => ComponentKind::Led,
        SymbolKind::Zener => ComponentKind::Zener,
        SymbolKind::ShuntReference => ComponentKind::ShuntReference,
        SymbolKind::Comparator => ComponentKind::Comparator,
        SymbolKind::Mosfet => ComponentKind::Mosfet,
        SymbolKind::OpAmp => ComponentKind::OpAmp,
        SymbolKind::Voltmeter | SymbolKind::Ammeter | SymbolKind::Ohmmeter => {
            ComponentKind::Custom(kind.key().to_owned())
        }
        SymbolKind::Custom(name) => ComponentKind::Custom(name.clone()),
    }
}

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    fn find(&mut self, mut index: usize) -> usize {
        let mut root = index;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        while self.parent[index] != index {
            let next = self.parent[index];
            self.parent[index] = root;
            index = next;
        }
        root
    }

    fn union(&mut self, left: usize, right: usize) {
        let left_root = self.find(left);
        let right_root = self.find(right);
        if left_root == right_root {
            return;
        }
        match self.rank[left_root].cmp(&self.rank[right_root]) {
            std::cmp::Ordering::Less => self.parent[left_root] = right_root,
            std::cmp::Ordering::Greater => self.parent[right_root] = left_root,
            std::cmp::Ordering::Equal => {
                self.parent[right_root] = left_root;
                self.rank[left_root] = self.rank[left_root].saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::grid::{Orientation, GRID_MAX};
    use crate::schematic::{Junction, JunctionId, NetLabel};
    use crate::symbol::Symbol;
    use crate::undo::UndoStack;
    use sim_core::netlist::ParamValue;

    fn registry() -> SymbolRegistry {
        SymbolRegistry::builtin()
    }

    #[test]
    fn shunt_reference_extraction_preserves_ref_k_a_pin_order() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let ground = add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Ground,
            GridPos::new(-10, -10),
            "GND1",
        );
        let reference = add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::ShuntReference,
            GridPos::new(0, 0),
            "U1",
        );

        let extraction = extract_netlist(&schematic, &registry()).unwrap();
        let component = extraction.netlist.component_by_label("U1").unwrap();

        assert_eq!(component.kind, ComponentKind::ShuntReference);
        assert_eq!(component.pins.len(), 3);
        assert_eq!(
            component.pins,
            [0_u16, 1, 2]
                .map(|pin| {
                    extraction.net_of_pin[&PinKey {
                        symbol: reference.id,
                        pin,
                    }]
                })
                .to_vec()
        );
        assert_ne!(component.pins[0], component.pins[1]);
        assert_ne!(component.pins[1], component.pins[2]);
        assert!(extraction.net_of_pin.contains_key(&PinKey {
            symbol: ground.id,
            pin: 0,
        }));
    }

    #[test]
    fn comparator_extraction_preserves_input_output_and_supply_pin_order() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Ground,
            GridPos::new(-10, -10),
            "GND1",
        );
        let comparator = add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Comparator,
            GridPos::new(0, 0),
            "U1",
        );

        let extraction = extract_netlist(&schematic, &registry()).unwrap();
        let component = extraction.netlist.component_by_label("U1").unwrap();

        assert_eq!(component.kind, ComponentKind::Comparator);
        assert_eq!(component.pins.len(), 5);
        assert_eq!(
            component.pins,
            (0_u16..5)
                .map(|pin| {
                    extraction.net_of_pin[&PinKey {
                        symbol: comparator.id,
                        pin,
                    }]
                })
                .collect::<Vec<_>>()
        );
        assert!(component.pins.windows(2).all(|pins| pins[0] != pins[1]));
    }

    #[test]
    fn mosfet_extraction_preserves_drain_gate_source_order_and_solves() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let mosfet = add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Mosfet,
            GridPos::new(0, 0),
            "M1",
        );
        add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Ground,
            GridPos::new(2, 6),
            "GND1",
        );
        add_symbol_oriented(
            &mut schematic,
            &mut history,
            SymbolKind::VoltageSource,
            GridPos::new(0, -6),
            "VDD",
            Orientation::R180,
        );
        add_symbol_oriented(
            &mut schematic,
            &mut history,
            SymbolKind::VoltageSource,
            GridPos::new(-8, 4),
            "VG",
            Orientation::R180,
        );
        // Supply: - to the ground bus, + to the drain.
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(-4, -6),
            GridPos::new(-4, 6),
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(-2, -6),
            GridPos::new(-4, -6),
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(2, -6),
            GridPos::new(2, -2),
        );
        // Ground bus: source, supply return, and gate return all reach it.
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(-10, 6),
            GridPos::new(2, 6),
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(2, 2),
            GridPos::new(2, 6),
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(-10, 4),
            GridPos::new(-10, 6),
        );
        // Gate drive.
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(-6, 4),
            GridPos::new(-6, 0),
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(-6, 0),
            GridPos::new(-2, 0),
        );

        let extraction = extract_netlist(&schematic, &registry()).unwrap();
        assert!(
            extraction.erc.issues().is_empty(),
            "unexpected ERC issues: {:?}",
            extraction.erc.issues()
        );
        let component = extraction.netlist.component_by_label("M1").unwrap();
        assert_eq!(component.kind, ComponentKind::Mosfet);
        assert_eq!(component.pins.len(), 3);
        assert_eq!(
            component.pins,
            (0_u16..3)
                .map(|pin| {
                    extraction.net_of_pin[&PinKey {
                        symbol: mosfet.id,
                        pin,
                    }]
                })
                .collect::<Vec<_>>()
        );
        assert!(component.pins.windows(2).all(|pins| pins[0] != pins[1]));
        assert_eq!(
            component
                .params
                .get("channel")
                .and_then(ParamValue::as_text),
            Some("n")
        );
        // The extracted netlist is directly solvable by the simulator.
        let solved = sim_core::solve_dc(&extraction.netlist, sim_core::DcOptions::default())
            .expect("MOSFET netlist must solve");
        let drain_net = extraction.net_of_pin[&PinKey {
            symbol: mosfet.id,
            pin: 0,
        }];
        let gate_net = extraction.net_of_pin[&PinKey {
            symbol: mosfet.id,
            pin: 1,
        }];
        assert!(
            (solved.node_voltage(drain_net).unwrap() - 5.0).abs() < 1e-9,
            "drain voltage {:?}",
            solved.node_voltage(drain_net)
        );
        assert!((solved.node_voltage(gate_net).unwrap() - 5.0).abs() < 1e-9);
    }

    fn add_symbol(
        schematic: &mut Schematic,
        history: &mut UndoStack,
        kind: SymbolKind,
        at: GridPos,
        label: &str,
    ) -> Symbol {
        add_symbol_oriented(schematic, history, kind, at, label, Orientation::R0)
    }

    fn add_symbol_oriented(
        schematic: &mut Schematic,
        history: &mut UndoStack,
        kind: SymbolKind,
        at: GridPos,
        label: &str,
        orientation: Orientation,
    ) -> Symbol {
        let id = SymbolId(schematic.allocate_id().unwrap());
        let def = registry().get(&kind).unwrap().clone();
        let symbol = Symbol::from_def(id, &def, at, label, orientation, false);
        history
            .execute(schematic, Command::AddSymbol(symbol.clone()))
            .unwrap();
        symbol
    }

    fn add_wire(
        schematic: &mut Schematic,
        history: &mut UndoStack,
        a: GridPos,
        b: GridPos,
    ) -> Wire {
        let id = WireId(schematic.allocate_id().unwrap());
        let wire = Wire::new(id, a, b).unwrap();
        history
            .execute(schematic, Command::AddWire(wire.clone()))
            .unwrap();
        wire
    }

    fn add_junction(schematic: &mut Schematic, history: &mut UndoStack, at: GridPos) -> Junction {
        let id = JunctionId(schematic.allocate_id().unwrap());
        let junction = Junction::new(id, at);
        history
            .execute(schematic, Command::AddJunction(junction.clone()))
            .unwrap();
        junction
    }

    fn build_fixture() -> Schematic {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Ground,
            GridPos::new(-2, 0),
            "GND1",
        );
        add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::VoltageSource,
            GridPos::new(0, -4),
            "V1",
        );
        add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(6, -4),
            "R1",
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(-2, -4),
            GridPos::new(-2, 0),
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(2, -4),
            GridPos::new(4, -4),
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(8, -4),
            GridPos::new(8, 0),
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(8, 0),
            GridPos::new(-2, 0),
        );
        schematic
    }

    #[test]
    fn separate_ground_symbols_share_the_same_reference_node() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let mut add_loop = |x: i32, suffix: &str| {
            let source = add_symbol(
                &mut schematic,
                &mut history,
                SymbolKind::VoltageSource,
                GridPos::new(x, 0),
                &format!("V{suffix}"),
            );
            add_symbol(
                &mut schematic,
                &mut history,
                SymbolKind::Resistor,
                GridPos::new(x + 6, 0),
                &format!("R{suffix}"),
            );
            let ground = add_symbol(
                &mut schematic,
                &mut history,
                SymbolKind::Ground,
                GridPos::new(x - 2, 4),
                &format!("GND{suffix}"),
            );
            add_wire(
                &mut schematic,
                &mut history,
                GridPos::new(x - 2, 0),
                GridPos::new(x - 2, 4),
            );
            add_wire(
                &mut schematic,
                &mut history,
                GridPos::new(x + 2, 0),
                GridPos::new(x + 4, 0),
            );
            add_wire(
                &mut schematic,
                &mut history,
                GridPos::new(x + 8, 0),
                GridPos::new(x + 8, 4),
            );
            add_wire(
                &mut schematic,
                &mut history,
                GridPos::new(x + 8, 4),
                GridPos::new(x - 2, 4),
            );
            (source, ground)
        };
        let (_, gnd1) = add_loop(0, "1");
        let (_, gnd2) = add_loop(20, "2");

        let extraction = extract_netlist(&schematic, &registry()).unwrap();
        let grounds = extraction
            .netlist
            .nodes
            .iter()
            .filter(|node| node.kind.is_ground())
            .collect::<Vec<_>>();
        assert_eq!(grounds.len(), 1);
        assert_eq!(grounds[0].name.as_str(), "0");
        let ground_node = grounds[0].id;
        assert_eq!(
            extraction
                .net_of_pin
                .get(&PinKey {
                    symbol: gnd1.id,
                    pin: 0,
                })
                .copied(),
            Some(ground_node)
        );
        assert_eq!(
            extraction
                .net_of_pin
                .get(&PinKey {
                    symbol: gnd2.id,
                    pin: 0,
                })
                .copied(),
            Some(ground_node)
        );
        assert!(extraction.netlist.validate_for_simulation().is_ok());
        assert!(!extraction.erc.has_errors(), "{:?}", extraction.erc);
        assert!(extraction
            .erc
            .issues()
            .iter()
            .any(|issue| issue.code == ErcCode::MultipleGround
                && issue.severity == crate::erc::ErcSeverity::Info));
    }

    #[test]
    fn extracts_a_simple_closed_circuit() {
        let schematic = build_fixture();
        let extraction = extract_netlist(&schematic, &registry()).unwrap();
        assert_eq!(extraction.netlist.nodes.len(), 2);
        assert_eq!(extraction.netlist.components.len(), 3);
        assert_eq!(extraction.netlist.ground_node_id(), Some(NodeId(0)));
        assert!(extraction.net_of_pin.len() >= 5);
        assert!(!extraction.erc.has_errors(), "{:?}", extraction.erc);
    }

    #[test]
    fn crossing_wires_do_not_connect_without_junction() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let r1 = add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(-4, 0),
            "R1",
        );
        let r2 = add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(4, 0),
            "R2",
        );
        let r3 = add_symbol_oriented(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(0, -4),
            "R3",
            Orientation::R90,
        );
        let r4 = add_symbol_oriented(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(0, 4),
            "R4",
            Orientation::R90,
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(-2, 0),
            GridPos::new(2, 0),
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(0, -2),
            GridPos::new(0, 2),
        );
        let horizontal = PinKey {
            symbol: r1.id,
            pin: 1,
        };
        let vertical = PinKey {
            symbol: r3.id,
            pin: 1,
        };
        let extraction = extract_netlist(&schematic, &registry()).unwrap();
        let horizontal_net = extraction.net_of_pin.get(&horizontal).copied().unwrap();
        let vertical_net = extraction.net_of_pin.get(&vertical).copied().unwrap();
        assert_ne!(horizontal_net, vertical_net);
        assert_eq!(
            extraction.net_of_pin.get(&PinKey {
                symbol: r2.id,
                pin: 0
            }),
            Some(&horizontal_net)
        );
        assert_eq!(
            extraction.net_of_pin.get(&PinKey {
                symbol: r4.id,
                pin: 0
            }),
            Some(&vertical_net)
        );

        add_junction(&mut schematic, &mut history, GridPos::new(0, 0));
        let extraction = extract_netlist(&schematic, &registry()).unwrap();
        assert_eq!(
            extraction.net_of_pin.get(&horizontal),
            extraction.net_of_pin.get(&vertical)
        );
    }

    #[test]
    fn t_junction_connects_without_explicit_dot() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        let r1 = add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(-4, 0),
            "R1",
        );
        let r2 = add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(4, 0),
            "R2",
        );
        let r3 = add_symbol_oriented(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(0, 4),
            "R3",
            Orientation::R90,
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(-2, 0),
            GridPos::new(2, 0),
        );
        add_wire(
            &mut schematic,
            &mut history,
            GridPos::new(0, 0),
            GridPos::new(0, 2),
        );
        let extraction = extract_netlist(&schematic, &registry()).unwrap();
        let net = extraction
            .net_of_pin
            .get(&PinKey {
                symbol: r1.id,
                pin: 1,
            })
            .copied()
            .unwrap();
        assert_eq!(
            extraction.net_of_pin.get(&PinKey {
                symbol: r2.id,
                pin: 0
            }),
            Some(&net)
        );
        assert_eq!(
            extraction.net_of_pin.get(&PinKey {
                symbol: r3.id,
                pin: 0
            }),
            Some(&net)
        );
    }

    #[test]
    fn labels_name_nets_and_duplicates_are_renamed() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(-4, 0),
            "R1",
        );
        add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(4, 0),
            "R2",
        );
        add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(-4, 8),
            "R3",
        );
        add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(4, 8),
            "R4",
        );
        for (a, b) in [
            (GridPos::new(-2, 0), GridPos::new(2, 0)),
            (GridPos::new(-2, 8), GridPos::new(2, 8)),
        ] {
            add_wire(&mut schematic, &mut history, a, b);
        }
        for at in [GridPos::new(0, 0), GridPos::new(0, 8)] {
            let id = NetLabelId(schematic.allocate_id().unwrap());
            let label = NetLabel::new(id, at, "OUT").unwrap();
            history
                .execute(&mut schematic, Command::AddLabel(label))
                .unwrap();
        }
        let extraction = extract_netlist(&schematic, &registry()).unwrap();
        let names: Vec<&str> = extraction
            .netlist
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect();
        assert!(names.contains(&"OUT"));
        assert!(names.contains(&"OUT#2"));
        assert!(extraction
            .erc
            .issues()
            .iter()
            .any(|issue| issue.code == ErcCode::DuplicateNetName));
    }

    #[test]
    fn label_directly_on_pin_names_the_net() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(-4, 0),
            "R1",
        );
        let id = NetLabelId(schematic.allocate_id().unwrap());
        let label = NetLabel::new(id, GridPos::new(-2, 0), "OUT").unwrap();
        history
            .execute(&mut schematic, Command::AddLabel(label))
            .unwrap();

        let extraction = extract_netlist(&schematic, &registry()).unwrap();
        assert!(extraction
            .netlist
            .nodes
            .iter()
            .any(|node| node.name.as_str() == "OUT"));
        assert!(!extraction.erc.issues().iter().any(|issue| {
            issue.code == ErcCode::FloatingPin && issue.at == Some(GridPos::new(-2, 0))
        }));
    }

    #[test]
    fn rejects_symbol_pins_outside_the_grid() {
        let mut schematic = Schematic::new();
        let mut history = UndoStack::new();
        add_symbol(
            &mut schematic,
            &mut history,
            SymbolKind::Resistor,
            GridPos::new(GRID_MAX, 0),
            "R1",
        );
        assert!(matches!(
            extract_netlist(&schematic, &registry()),
            Err(ExtractError::PinOutOfBounds { .. })
        ));
    }
}
