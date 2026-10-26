//! Integer grid geometry and orthogonal segment relations.

use std::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

/// Minimum coordinate accepted by the editor.
pub const GRID_MIN: i32 = -1_000_000;

/// Maximum coordinate accepted by the editor.
pub const GRID_MAX: i32 = 1_000_000;

/// Strict integer position on the schematic grid.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct GridPos {
    pub x: i32,
    pub y: i32,
}

impl GridPos {
    /// Creates a grid position.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Returns true when both coordinates stay inside the supported grid.
    pub const fn is_within_bounds(self) -> bool {
        self.x >= GRID_MIN && self.x <= GRID_MAX && self.y >= GRID_MIN && self.y <= GRID_MAX
    }

    /// Adds two positions with overflow checking.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            x: self.x.checked_add(other.x)?,
            y: self.y.checked_add(other.y)?,
        })
    }

    /// Subtracts two positions with overflow checking.
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        Some(Self {
            x: self.x.checked_sub(other.x)?,
            y: self.y.checked_sub(other.y)?,
        })
    }

    /// Negates both coordinates with overflow checking.
    pub fn checked_neg(self) -> Option<Self> {
        Some(Self {
            x: self.x.checked_neg()?,
            y: self.y.checked_neg()?,
        })
    }

    /// Manhattan distance between two positions.
    pub fn manhattan_distance(self, other: Self) -> u32 {
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y)
    }
}

impl Add for GridPos {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for GridPos {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

/// Quarter-turn orientation in the screen coordinate system.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Orientation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl Orientation {
    /// Rotates a local position around the symbol origin.
    pub const fn rotate(self, position: GridPos) -> GridPos {
        match self {
            Self::R0 => position,
            Self::R90 => GridPos::new(-position.y, position.x),
            Self::R180 => GridPos::new(-position.x, -position.y),
            Self::R270 => GridPos::new(position.y, -position.x),
        }
    }

    /// Next orientation in clockwise order.
    pub const fn next(self) -> Self {
        match self {
            Self::R0 => Self::R90,
            Self::R90 => Self::R180,
            Self::R180 => Self::R270,
            Self::R270 => Self::R0,
        }
    }
}

/// Geometric relation between two orthogonal segments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentRelation {
    Disjoint,
    CollinearTouch(GridPos),
    CollinearOverlap {
        start: GridPos,
        end: GridPos,
    },
    Perpendicular {
        at: GridPos,
        endpoint_a: bool,
        endpoint_b: bool,
    },
}

/// Returns true when a point lies on a closed orthogonal segment.
pub fn point_on_segment(point: GridPos, start: GridPos, end: GridPos) -> bool {
    if start.x == end.x {
        point.x == start.x && between(point.y, start.y, end.y)
    } else if start.y == end.y {
        point.y == start.y && between(point.x, start.x, end.x)
    } else {
        false
    }
}

/// Computes the relation between two orthogonal segments.
pub fn segment_relation(
    a_start: GridPos,
    a_end: GridPos,
    b_start: GridPos,
    b_end: GridPos,
) -> SegmentRelation {
    let a_horizontal = a_start.y == a_end.y && a_start.x != a_end.x;
    let b_horizontal = b_start.y == b_end.y && b_start.x != b_end.x;
    let a_vertical = a_start.x == a_end.x && a_start.y != a_end.y;
    let b_vertical = b_start.x == b_end.x && b_start.y != b_end.y;

    match (a_horizontal, a_vertical, b_horizontal, b_vertical) {
        (true, false, true, false) => {
            if a_start.y != b_start.y {
                return SegmentRelation::Disjoint;
            }
            let a_low = a_start.x.min(a_end.x);
            let a_high = a_start.x.max(a_end.x);
            let b_low = b_start.x.min(b_end.x);
            let b_high = b_start.x.max(b_end.x);
            collinear_interval(a_low, a_high, b_low, b_high, true, a_start.y)
        }
        (false, true, false, true) => {
            if a_start.x != b_start.x {
                return SegmentRelation::Disjoint;
            }
            let a_low = a_start.y.min(a_end.y);
            let a_high = a_start.y.max(a_end.y);
            let b_low = b_start.y.min(b_end.y);
            let b_high = b_start.y.max(b_end.y);
            collinear_interval(a_low, a_high, b_low, b_high, false, a_start.x)
        }
        (true, false, false, true) => perpendicular_relation(a_start, a_end, b_start, b_end, true),
        (false, true, true, false) => perpendicular_relation(b_start, b_end, a_start, a_end, false),
        _ => SegmentRelation::Disjoint,
    }
}

fn between(value: i32, first: i32, second: i32) -> bool {
    let low = first.min(second);
    let high = first.max(second);
    value >= low && value <= high
}

fn collinear_interval(
    a_low: i32,
    a_high: i32,
    b_low: i32,
    b_high: i32,
    horizontal: bool,
    fixed: i32,
) -> SegmentRelation {
    let start = a_low.max(b_low);
    let end = a_high.min(b_high);
    if start > end {
        SegmentRelation::Disjoint
    } else if start == end {
        SegmentRelation::CollinearTouch(if horizontal {
            GridPos::new(start, fixed)
        } else {
            GridPos::new(fixed, start)
        })
    } else {
        let start = if horizontal {
            GridPos::new(start, fixed)
        } else {
            GridPos::new(fixed, start)
        };
        let end = if horizontal {
            GridPos::new(end, fixed)
        } else {
            GridPos::new(fixed, end)
        };
        SegmentRelation::CollinearOverlap { start, end }
    }
}

fn perpendicular_relation(
    horizontal_start: GridPos,
    horizontal_end: GridPos,
    vertical_start: GridPos,
    vertical_end: GridPos,
    horizontal_is_a: bool,
) -> SegmentRelation {
    let at = GridPos::new(vertical_start.x, horizontal_start.y);
    if !point_on_segment(at, horizontal_start, horizontal_end)
        || !point_on_segment(at, vertical_start, vertical_end)
    {
        return SegmentRelation::Disjoint;
    }
    let horizontal_endpoint = at == horizontal_start || at == horizontal_end;
    let vertical_endpoint = at == vertical_start || at == vertical_end;
    if horizontal_is_a {
        SegmentRelation::Perpendicular {
            at,
            endpoint_a: horizontal_endpoint,
            endpoint_b: vertical_endpoint,
        }
    } else {
        SegmentRelation::Perpendicular {
            at,
            endpoint_a: vertical_endpoint,
            endpoint_b: horizontal_endpoint,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: i32, y: i32) -> GridPos {
        GridPos::new(x, y)
    }

    #[test]
    fn detects_collinear_touch_and_overlap() {
        assert_eq!(
            segment_relation(pos(0, 0), pos(10, 0), pos(10, 0), pos(20, 0)),
            SegmentRelation::CollinearTouch(pos(10, 0))
        );
        assert_eq!(
            segment_relation(pos(0, 0), pos(10, 0), pos(5, 0), pos(20, 0)),
            SegmentRelation::CollinearOverlap {
                start: pos(5, 0),
                end: pos(10, 0)
            }
        );
        assert_eq!(
            segment_relation(pos(0, 0), pos(10, 0), pos(11, 0), pos(20, 0)),
            SegmentRelation::Disjoint
        );
    }

    #[test]
    fn detects_perpendicular_endpoints_and_crossings() {
        assert_eq!(
            segment_relation(pos(0, 0), pos(10, 0), pos(5, 0), pos(5, 5)),
            SegmentRelation::Perpendicular {
                at: pos(5, 0),
                endpoint_a: false,
                endpoint_b: true
            }
        );
        assert_eq!(
            segment_relation(pos(0, 0), pos(10, 0), pos(5, -5), pos(5, 5)),
            SegmentRelation::Perpendicular {
                at: pos(5, 0),
                endpoint_a: false,
                endpoint_b: false
            }
        );
        assert_eq!(
            segment_relation(pos(0, 0), pos(10, 0), pos(20, -5), pos(20, 5)),
            SegmentRelation::Disjoint
        );
    }

    #[test]
    fn rotates_around_origin() {
        assert_eq!(Orientation::R90.rotate(pos(1, 0)), pos(0, 1));
        assert_eq!(Orientation::R180.rotate(pos(1, 2)), pos(-1, -2));
        assert_eq!(Orientation::R270.rotate(pos(1, 0)), pos(0, -1));
    }

    #[test]
    fn checks_grid_bounds_and_arithmetic() {
        assert!(pos(GRID_MIN, GRID_MAX).is_within_bounds());
        assert!(!pos(GRID_MIN - 1, 0).is_within_bounds());
        assert_eq!(pos(1, 2).checked_add(pos(3, 4)), Some(pos(4, 6)));
        assert_eq!(pos(i32::MAX, 0).checked_add(pos(1, 0)), None);
        assert_eq!(pos(1, 2).manhattan_distance(pos(4, 6)), 7);
    }
}
