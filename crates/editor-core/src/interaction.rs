//! Pure geometry helpers shared by canvas interactions and tests.

use std::cmp::Ordering;

use crate::extract::PinKey;
use crate::grid::GridPos;
use crate::schematic::{Wire, WireId};

/// Default magnetic radius in grid cells.
pub const DEFAULT_SNAP_RADIUS: i32 = 2;

/// Electrical geometry targeted by a magnetic snap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    /// A symbol pin.
    Pin(PinKey),
    /// An existing wire.
    Wire(WireId),
}

/// Snapped position and its source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapTarget {
    /// Snapped grid position.
    pub at: GridPos,
    /// Geometry that produced the snap.
    pub kind: SnapKind,
}

/// Returns the nearest pin or wire within `radius` grid cells.
pub fn nearest_snap(
    point: GridPos,
    pins: &[(PinKey, GridPos)],
    wires: &[Wire],
    radius: i32,
) -> Option<SnapTarget> {
    if radius <= 0 {
        return None;
    }
    let max_distance = i64::from(radius) * i64::from(radius);
    let mut best: Option<(i64, u8, SnapTarget)> = None;

    for (key, position) in pins {
        let distance = squared_distance(point, *position);
        if distance > max_distance {
            continue;
        }
        let candidate = SnapTarget {
            at: *position,
            kind: SnapKind::Pin(*key),
        };
        if better(distance, 0, candidate, best) {
            best = Some((distance, 0, candidate));
        }
    }

    for wire in wires {
        let candidate_position = closest_point_on_wire(point, wire);
        let distance = squared_distance(point, candidate_position);
        if distance > max_distance {
            continue;
        }
        let candidate = SnapTarget {
            at: candidate_position,
            kind: SnapKind::Wire(wire.id),
        };
        if better(distance, 1, candidate, best) {
            best = Some((distance, 1, candidate));
        }
    }

    best.map(|(_, _, target)| target)
}

fn better(
    distance: i64,
    priority: u8,
    candidate: SnapTarget,
    best: Option<(i64, u8, SnapTarget)>,
) -> bool {
    let Some((best_distance, best_priority, best_target)) = best else {
        return true;
    };
    match distance.cmp(&best_distance) {
        Ordering::Less => true,
        Ordering::Greater => false,
        Ordering::Equal => match priority.cmp(&best_priority) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => snap_key(candidate) < snap_key(best_target),
        },
    }
}

fn snap_key(target: SnapTarget) -> (i32, i32, u8, u32, u16) {
    match target.kind {
        SnapKind::Pin(key) => (target.at.x, target.at.y, 0, key.symbol.0, key.pin),
        SnapKind::Wire(id) => (target.at.x, target.at.y, 1, id.0, 0),
    }
}

fn squared_distance(a: GridPos, b: GridPos) -> i64 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dy = i64::from(a.y) - i64::from(b.y);
    dx * dx + dy * dy
}

fn closest_point_on_wire(point: GridPos, wire: &Wire) -> GridPos {
    if wire.is_horizontal() {
        GridPos::new(point.x.clamp(wire.a.x, wire.b.x), wire.a.y)
    } else {
        GridPos::new(wire.a.x, point.y.clamp(wire.a.y, wire.b.y))
    }
}

/// Axis-aligned obstacle rectangle in grid coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridRect {
    /// Minimum corner.
    pub min: GridPos,
    /// Maximum corner.
    pub max: GridPos,
}

impl GridRect {
    /// Builds a normalized rectangle.
    pub fn new(a: GridPos, b: GridPos) -> Self {
        Self {
            min: GridPos::new(a.x.min(b.x), a.y.min(b.y)),
            max: GridPos::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    /// Returns true when a point lies strictly inside the rectangle.
    pub fn strictly_contains(self, point: GridPos) -> bool {
        point.x > self.min.x && point.x < self.max.x && point.y > self.min.y && point.y < self.max.y
    }
}

/// Builds an orthogonal route, preferring one elbow direction and avoiding obstacles.
pub fn route_wire(
    start: GridPos,
    end: GridPos,
    prefer_horizontal: bool,
    obstacles: &[GridRect],
) -> Vec<GridPos> {
    if start == end {
        return vec![start];
    }

    let mut candidates = Vec::new();
    candidates.push(vec![start, GridPos::new(end.x, start.y), end]);
    candidates.push(vec![start, GridPos::new(start.x, end.y), end]);
    let mid_x = GridPos::new(start.x + (end.x - start.x) / 2, start.y);
    let mid_y = GridPos::new(start.x, start.y + (end.y - start.y) / 2);
    candidates.push(vec![start, mid_x, GridPos::new(mid_x.x, end.y), end]);
    candidates.push(vec![start, mid_y, GridPos::new(end.x, mid_y.y), end]);

    for rect in obstacles {
        for y in [rect.min.y.saturating_sub(1), rect.max.y.saturating_add(1)] {
            candidates.push(vec![
                start,
                GridPos::new(start.x, y),
                GridPos::new(end.x, y),
                end,
            ]);
        }
        for x in [rect.min.x.saturating_sub(1), rect.max.x.saturating_add(1)] {
            candidates.push(vec![
                start,
                GridPos::new(x, start.y),
                GridPos::new(x, end.y),
                end,
            ]);
        }
    }

    let mut best: Option<(i64, u8, bool, Vec<GridPos>)> = None;
    for candidate in candidates {
        let path = simplify(candidate);
        if path
            .iter()
            .any(|point| obstacles.iter().any(|rect| rect.strictly_contains(*point)))
        {
            continue;
        }
        if path
            .windows(2)
            .any(|segment| segment_intersects_obstacle(segment[0], segment[1], obstacles))
        {
            continue;
        }
        let length = path
            .windows(2)
            .map(|segment| {
                i64::from((segment[1].x - segment[0].x).abs())
                    + i64::from((segment[1].y - segment[0].y).abs())
            })
            .sum();
        let bends = path.len().saturating_sub(2) as u8;
        let first_horizontal = path
            .get(1)
            .is_some_and(|next| next.y == start.y && next.x != start.x);
        let preference_penalty = first_horizontal != prefer_horizontal;
        let key = (length, bends, preference_penalty, path.clone());
        let replace = best
            .as_ref()
            .is_none_or(|(best_length, best_bends, best_penalty, _)| {
                (length, bends, preference_penalty) < (*best_length, *best_bends, *best_penalty)
            });
        if replace {
            best = Some(key);
        }
    }

    best.map_or_else(
        || vec![start, GridPos::new(end.x, start.y), end],
        |(_, _, _, path)| path,
    )
}

fn simplify(points: Vec<GridPos>) -> Vec<GridPos> {
    let mut result = Vec::with_capacity(points.len());
    for point in points {
        if result.last() != Some(&point) {
            result.push(point);
        }
    }
    result
}

fn segment_intersects_obstacle(start: GridPos, end: GridPos, obstacles: &[GridRect]) -> bool {
    if start.x == end.x {
        let low = start.y.min(end.y);
        let high = start.y.max(end.y);
        obstacles.iter().any(|rect| {
            start.x > rect.min.x && start.x < rect.max.x && low < rect.max.y && high > rect.min.y
        })
    } else {
        let low = start.x.min(end.x);
        let high = start.x.max(end.x);
        obstacles.iter().any(|rect| {
            start.y > rect.min.y && start.y < rect.max.y && low < rect.max.x && high > rect.min.x
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schematic::Wire;
    use crate::symbol::SymbolId;

    fn pin(symbol: u32, pin: u16, x: i32, y: i32) -> (PinKey, GridPos) {
        (
            PinKey {
                symbol: SymbolId(symbol),
                pin,
            },
            GridPos::new(x, y),
        )
    }

    #[test]
    fn magnetic_snap_prefers_the_nearest_pin() {
        let pins = [pin(2, 0, 10, 0), pin(1, 0, 7, 0)];
        let wires = [Wire::new(WireId(3), GridPos::new(0, 0), GridPos::new(20, 0)).unwrap()];
        let target = nearest_snap(GridPos::new(7, 1), &pins, &wires, 2).unwrap();
        assert_eq!(target.at, GridPos::new(7, 0));
        assert!(matches!(target.kind, SnapKind::Pin(_)));
    }

    #[test]
    fn magnetic_snap_projects_onto_wire() {
        let wires = [Wire::new(WireId(1), GridPos::new(0, 0), GridPos::new(10, 0)).unwrap()];
        let target = nearest_snap(GridPos::new(5, 2), &[], &wires, 2).unwrap();
        assert_eq!(target.at, GridPos::new(5, 0));
        assert_eq!(target.kind, SnapKind::Wire(WireId(1)));
    }

    #[test]
    fn routing_avoids_a_symbol_obstacle() {
        let obstacle = GridRect::new(GridPos::new(2, -2), GridPos::new(6, 2));
        let path = route_wire(GridPos::new(0, 0), GridPos::new(8, 0), true, &[obstacle]);
        assert!(path.len() > 2);
        assert!(path.windows(2).all(|segment| !segment_intersects_obstacle(
            segment[0],
            segment[1],
            &[obstacle]
        )));
    }

    #[test]
    fn preferred_elbow_is_observable() {
        let horizontal = route_wire(GridPos::new(0, 0), GridPos::new(4, 4), true, &[]);
        let vertical = route_wire(GridPos::new(0, 0), GridPos::new(4, 4), false, &[]);
        assert_eq!(horizontal[1], GridPos::new(4, 0));
        assert_eq!(vertical[1], GridPos::new(0, 4));
    }
}
