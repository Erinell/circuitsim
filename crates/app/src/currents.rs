//! Net and wire current reconstruction from sampled live values.

use std::collections::BTreeMap;

use editor_core::{ExtractedNet, Symbol, SymbolKind, WireId};
use sim_core::netlist::ParamValue;

use crate::analysis::LiveValues;
use crate::document::Document;

/// Leakage below this value must not animate a wire.
const MIN_FLOW_CURRENT: f64 = 1e-7;

/// Strongest injection observed on one net.
pub(crate) struct NetFlow {
    pub(crate) source: editor_core::GridPos,
    pub(crate) away: bool,
    pub(crate) strength: f64,
}

/// Returns the strongest current injection for every energized net.
pub(crate) fn net_flows(document: &Document, live: &LiveValues) -> BTreeMap<String, NetFlow> {
    let mut flows: BTreeMap<String, NetFlow> = BTreeMap::new();
    for symbol in document.schematic().symbols() {
        if symbol.kind.is_measurement() {
            continue;
        }
        let Some(definition) = document.registry().get(&symbol.kind) else {
            continue;
        };
        if definition.pins.len() != 2 {
            continue;
        }
        if symbol.kind == SymbolKind::Switch && !switch_closed(symbol) {
            continue;
        }
        let observed = live.symbol(document, symbol);
        let current = observed.current.or_else(|| current_source_dc(symbol));
        let Some(current) = current else {
            continue;
        };
        for (index, injection) in [-current, current].into_iter().enumerate() {
            if !injection.is_finite() {
                continue;
            }
            let strength = injection.abs();
            if strength <= f64::MIN_POSITIVE {
                continue;
            }
            let Some(pin) = observed.pins.get(index) else {
                continue;
            };
            let Some(net) = pin.net.as_deref() else {
                continue;
            };
            let Some(position) = symbol.pin_position_of(&definition.pins[index]) else {
                continue;
            };
            let candidate = NetFlow {
                source: position,
                away: injection > 0.0,
                strength,
            };
            let replace = match flows.get(net) {
                None => true,
                Some(existing) => {
                    strength > existing.strength
                        || (strength == existing.strength && candidate.away && !existing.away)
                }
            };
            if replace {
                flows.insert(net.to_owned(), candidate);
            }
        }
    }
    let strongest = flows
        .values()
        .map(|flow| flow.strength)
        .fold(0.0_f64, f64::max);
    if strongest < MIN_FLOW_CURRENT {
        flows.clear();
        return flows;
    }
    let threshold = (strongest * 1e-4).max(MIN_FLOW_CURRENT * 0.5);
    flows.retain(|_, flow| flow.strength >= threshold);
    flows
}

/// Returns the strongest current observed on one named net.
pub(crate) fn net_current(document: &Document, live: &LiveValues, net: &str) -> Option<f64> {
    if live.is_empty() {
        return None;
    }
    Some(
        net_flows(document, live)
            .get(net)
            .map_or(0.0, |flow| flow.strength),
    )
}

/// Returns the reconstructed current for every wire of the schematic.
///
/// The whole net is solved once so canvas overlays and diagnostics agree on
/// the same deterministic branch currents.
pub(crate) fn wire_currents(document: &Document, live: &LiveValues) -> BTreeMap<WireId, f64> {
    let mut currents = BTreeMap::new();
    if live.is_empty() {
        return currents;
    }
    for net in document.nets() {
        currents.extend(net_wire_currents(document, live, net));
    }
    currents
}

/// Returns the reconstructed current of one wire.
pub(crate) fn wire_current(document: &Document, live: &LiveValues, wire_id: WireId) -> Option<f64> {
    if live.is_empty() {
        return None;
    }
    let net = document
        .nets()
        .iter()
        .find(|net| net.wires.contains(&wire_id))?;
    net_wire_currents(document, live, net)
        .get(&wire_id)
        .copied()
}

fn net_wire_currents(
    document: &Document,
    live: &LiveValues,
    net: &ExtractedNet,
) -> BTreeMap<WireId, f64> {
    let mut currents = BTreeMap::new();
    if net.wires.is_empty() {
        return currents;
    }
    let mut node_index = BTreeMap::new();
    let mut node_points = Vec::new();
    for id in &net.wires {
        let Some(wire) = document.wire(*id) else {
            return currents;
        };
        for point in [wire.a, wire.b] {
            let next = node_points.len();
            node_index.entry(point).or_insert_with(|| {
                node_points.push(point);
                next
            });
        }
    }
    let node_count = node_points.len();
    if node_count == 0 {
        return currents;
    }
    let mut adjacency: Vec<Vec<(usize, WireId, bool)>> = vec![Vec::new(); node_count];
    for id in &net.wires {
        let Some(wire) = document.wire(*id) else {
            return currents;
        };
        let Some(&a) = node_index.get(&wire.a) else {
            return currents;
        };
        let Some(&b) = node_index.get(&wire.b) else {
            return currents;
        };
        adjacency[a].push((b, *id, true));
        adjacency[b].push((a, *id, false));
    }
    for neighbours in &mut adjacency {
        neighbours.sort_by_key(|(_, id, _)| *id);
    }

    let mut injections = vec![0.0_f64; node_count];
    let mut unknown = vec![0_usize; node_count];
    for symbol in document.schematic().symbols() {
        if symbol.kind.is_measurement() {
            continue;
        }
        let Some(definition) = document.registry().get(&symbol.kind) else {
            continue;
        };
        if definition.pins.len() != 2 {
            continue;
        }
        let observed = live.symbol(document, symbol);
        let current = observed.current.or_else(|| current_source_dc(symbol));
        for (index, pin) in definition.pins.iter().enumerate() {
            let Some(position) = symbol.pin_position_of(pin) else {
                continue;
            };
            let Some(&node) = node_index.get(&position) else {
                continue;
            };
            let Some(pin_net) = observed.pins.get(index).and_then(|pin| pin.net.as_deref()) else {
                continue;
            };
            if pin_net != net.name.as_str() {
                continue;
            }
            match current {
                Some(current) if current.is_finite() => {
                    injections[node] += if index == 0 { -current } else { current };
                }
                _ => unknown[node] += 1,
            }
        }
    }
    let residual: f64 = injections.iter().sum();
    let unknowns: usize = unknown.iter().sum();
    if unknowns > 0 && residual.is_finite() && residual.abs() > f64::MIN_POSITIVE {
        let correction = residual / unknowns as f64;
        for (node, count) in unknown.iter().enumerate() {
            if *count > 0 {
                injections[node] -= correction * *count as f64;
            }
        }
    }

    let root = injections
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            left.abs()
                .partial_cmp(&right.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map_or(0, |(index, _)| index);
    let mut parent = vec![usize::MAX; node_count];
    let mut parent_edge: Vec<Option<(WireId, bool)>> = vec![None; node_count];
    let mut order = Vec::with_capacity(node_count);
    let mut stack = vec![root];
    parent[root] = root;
    while let Some(node) = stack.pop() {
        order.push(node);
        for (next, id, forward) in &adjacency[node] {
            if parent[*next] == usize::MAX {
                parent[*next] = node;
                parent_edge[*next] = Some((*id, *forward));
                stack.push(*next);
            }
        }
    }

    let mut subtree = injections;
    for node in order.into_iter().rev() {
        if node == root {
            continue;
        }
        let Some((edge, child_is_b)) = parent_edge[node] else {
            continue;
        };
        let flow = subtree[node];
        currents.insert(edge, if child_is_b { -flow } else { flow });
        subtree[parent[node]] += flow;
    }
    currents
}

pub(crate) fn current_source_dc(symbol: &Symbol) -> Option<f64> {
    (symbol.kind == SymbolKind::CurrentSource)
        .then(|| symbol.params.get("dc").and_then(ParamValue::as_number))
        .flatten()
}

fn switch_closed(symbol: &Symbol) -> bool {
    symbol
        .params
        .get("closed")
        .and_then(ParamValue::as_bool)
        .unwrap_or(false)
}
