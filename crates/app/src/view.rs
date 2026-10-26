//! Pan, zoom, and coordinate transforms for the schematic canvas.

use editor_core::grid::{GRID_MAX, GRID_MIN};
use editor_core::GridPos;
use eframe::egui::{Pos2, Rect, Vec2};

/// Smallest canvas zoom in pixels per grid unit.
pub const MIN_SCALE: f32 = 8.0;
/// Largest canvas zoom in pixels per grid unit.
pub const MAX_SCALE: f32 = 80.0;
/// Initial canvas zoom in pixels per grid unit.
pub const DEFAULT_SCALE: f32 = 24.0;

/// Two-dimensional canvas viewport.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    scale: f32,
    pan: Vec2,
}

impl Viewport {
    /// Creates a viewport centered on the grid origin.
    pub fn new() -> Self {
        Self {
            scale: DEFAULT_SCALE,
            pan: Vec2::ZERO,
        }
    }

    /// Returns the zoom in pixels per grid unit.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Returns the pan offset in screen points.
    pub fn pan(&self) -> Vec2 {
        self.pan
    }

    /// Converts a grid position to screen coordinates.
    pub fn grid_to_screen(&self, rect: Rect, position: GridPos) -> Pos2 {
        rect.center() + self.pan + Vec2::new(position.x as f32, position.y as f32) * self.scale
    }

    /// Converts screen coordinates to fractional grid coordinates.
    pub fn screen_to_grid_f(&self, rect: Rect, position: Pos2) -> Vec2 {
        (position - rect.center() - self.pan) / self.scale
    }

    /// Snaps screen coordinates to the nearest in-bounds grid position.
    pub fn screen_to_grid(&self, rect: Rect, position: Pos2) -> Option<GridPos> {
        let world = self.screen_to_grid_f(rect, position);
        if !world.x.is_finite() || !world.y.is_finite() {
            return None;
        }
        Some(GridPos::new(
            world.x.round().clamp(GRID_MIN as f32, GRID_MAX as f32) as i32,
            world.y.round().clamp(GRID_MIN as f32, GRID_MAX as f32) as i32,
        ))
    }

    /// Zooms around a fixed screen anchor.
    pub fn zoom_at(&mut self, rect: Rect, anchor: Pos2, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let world = self.screen_to_grid_f(rect, anchor);
        let scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        if scale == self.scale {
            return;
        }
        self.scale = scale;
        self.pan = anchor - rect.center() - world * self.scale;
    }

    /// Zooms around the canvas centre.
    pub fn zoom_center(&mut self, rect: Rect, factor: f32) {
        self.zoom_at(rect, rect.center(), factor);
    }

    /// Sets a zoom level while keeping the world point under the centre fixed.
    pub fn set_scale(&mut self, rect: Rect, scale: f32) {
        let target = scale.clamp(MIN_SCALE, MAX_SCALE);
        if (target - self.scale).abs() <= f32::EPSILON {
            return;
        }
        self.zoom_at(rect, rect.center(), target / self.scale);
    }

    /// Centers the world origin at the requested grid position.
    pub fn center_on(&mut self, position: GridPos) {
        self.pan = -Vec2::new(position.x as f32, position.y as f32) * self.scale;
    }

    /// Pans by a whole number of grid cells.
    pub fn pan_grid(&mut self, delta: GridPos) {
        self.pan += Vec2::new(delta.x as f32, delta.y as f32) * self.scale;
    }

    /// Returns the current zoom as a percentage of the default scale.
    pub fn zoom_percent(&self) -> f32 {
        self.scale / DEFAULT_SCALE * 100.0
    }

    /// Pans by a screen-space delta.
    pub fn pan_by(&mut self, delta: Vec2) {
        if delta.x.is_finite() && delta.y.is_finite() {
            self.pan += delta;
        }
    }

    /// Resets zoom and pan around the origin.
    pub fn reset(&mut self) {
        self.scale = DEFAULT_SCALE;
        self.pan = Vec2::ZERO;
    }

    /// Fits a grid-space bounding box with a visible margin.
    pub fn fit(&mut self, rect: Rect, bounds: (GridPos, GridPos)) {
        let (min, max) = bounds;
        let width = (max.x.saturating_sub(min.x) + 4).max(4) as f32;
        let height = (max.y.saturating_sub(min.y) + 4).max(4) as f32;
        let available = Vec2::new(
            (rect.width() - 80.0).max(64.0),
            (rect.height() - 80.0).max(64.0),
        );
        let scale = (available.x / width)
            .min(available.y / height)
            .clamp(MIN_SCALE, MAX_SCALE);
        self.scale = scale;
        let center = Vec2::new(
            (min.x as f32 + max.x as f32) * 0.5,
            (min.y as f32 + max.y as f32) * 0.5,
        );
        self.pan = -center * scale;
    }

    /// Returns the visible grid bounds, inclusive and clamped.
    pub fn visible_bounds(&self, rect: Rect) -> (GridPos, GridPos) {
        let first = self.screen_to_grid_f(rect, rect.left_top());
        let second = self.screen_to_grid_f(rect, rect.right_bottom());
        let min_x = first
            .x
            .min(second.x)
            .floor()
            .clamp(GRID_MIN as f32, GRID_MAX as f32) as i32;
        let max_x = first
            .x
            .max(second.x)
            .ceil()
            .clamp(GRID_MIN as f32, GRID_MAX as f32) as i32;
        let min_y = first
            .y
            .min(second.y)
            .floor()
            .clamp(GRID_MIN as f32, GRID_MAX as f32) as i32;
        let max_y = first
            .y
            .max(second.y)
            .ceil()
            .clamp(GRID_MIN as f32, GRID_MAX as f32) as i32;
        (GridPos::new(min_x, min_y), GridPos::new(max_x, max_y))
    }
}

/// Uniform projection from schematic grid coordinates to a minimap area.
#[derive(Clone, Copy, Debug)]
pub struct MinimapTransform {
    origin: Pos2,
    scale: f32,
    min: GridPos,
}

impl MinimapTransform {
    /// Builds a centered, aspect-preserving projection with a small margin.
    pub fn new(area: Rect, bounds: (GridPos, GridPos)) -> Option<Self> {
        if area.width() <= 0.0 || area.height() <= 0.0 {
            return None;
        }
        let (min, max) = bounds;
        let width = max.x.saturating_sub(min.x).max(1) as f32;
        let height = max.y.saturating_sub(min.y).max(1) as f32;
        let available = area.shrink(6.0);
        if available.width() <= 0.0 || available.height() <= 0.0 {
            return None;
        }
        let scale = (available.width() / width).min(available.height() / height);
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let used = Vec2::new(width * scale, height * scale);
        let origin = available.center() - used * 0.5;
        Some(Self { origin, scale, min })
    }

    /// Projects a grid position into the minimap.
    pub fn point(&self, position: GridPos) -> Pos2 {
        self.origin
            + Vec2::new(
                (position.x - self.min.x) as f32,
                (position.y - self.min.y) as f32,
            ) * self.scale
    }

    /// Projects a grid rectangle into the minimap.
    pub fn rect(&self, min: GridPos, max: GridPos) -> Rect {
        Rect::from_two_pos(self.point(min), self.point(max))
    }

    /// Converts a minimap point back to the nearest grid position.
    pub fn grid_at(&self, position: Pos2) -> GridPos {
        let x = ((position.x - self.origin.x) / self.scale).round() + self.min.x as f32;
        let y = ((position.y - self.origin.y) / self.scale).round() + self.min.y as f32;
        GridPos::new(
            x.clamp(GRID_MIN as f32, GRID_MAX as f32) as i32,
            y.clamp(GRID_MIN as f32, GRID_MAX as f32) as i32,
        )
    }

    /// Returns the minimap scale in pixels per grid cell.
    pub const fn scale(&self) -> f32 {
        self.scale
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(20.0, 30.0), Vec2::new(900.0, 600.0))
    }

    #[test]
    fn grid_and_screen_coordinates_round_trip() {
        let viewport = Viewport::new();
        for position in [
            GridPos::new(0, 0),
            GridPos::new(-12, 7),
            GridPos::new(1_000, -1_000),
        ] {
            let screen = viewport.grid_to_screen(rect(), position);
            assert_eq!(viewport.screen_to_grid(rect(), screen), Some(position));
        }
    }

    #[test]
    fn zoom_keeps_the_anchor_world_position_fixed() {
        let mut viewport = Viewport::new();
        let anchor = Pos2::new(310.0, 220.0);
        let before = viewport.screen_to_grid_f(rect(), anchor);
        viewport.zoom_at(rect(), anchor, 2.5);
        let after = viewport.screen_to_grid_f(rect(), anchor);
        assert!((before.x - after.x).abs() < 1e-4);
        assert!((before.y - after.y).abs() < 1e-4);
    }

    #[test]
    fn zoom_is_clamped_to_supported_range() {
        let mut viewport = Viewport::new();
        viewport.zoom_at(rect(), rect().center(), 1e6);
        assert_eq!(viewport.scale(), MAX_SCALE);
        viewport.zoom_at(rect(), rect().center(), 1e-9);
        assert_eq!(viewport.scale(), MIN_SCALE);
    }

    #[test]
    fn fit_centers_the_bounds_and_uses_the_available_space() {
        let mut viewport = Viewport::new();
        let bounds = (GridPos::new(-10, -8), GridPos::new(6, 0));
        viewport.fit(rect(), bounds);
        let center = Pos2::new(
            (bounds.0.x as f32 + bounds.1.x as f32) * 0.5,
            (bounds.0.y as f32 + bounds.1.y as f32) * 0.5,
        );
        let screen = viewport.grid_to_screen(
            rect(),
            GridPos::new(center.x.round() as i32, center.y.round() as i32),
        );
        assert!((screen - rect().center()).length() < viewport.scale());
        assert!((MIN_SCALE..=MAX_SCALE).contains(&viewport.scale()));
    }

    #[test]
    fn snapping_respects_grid_bounds() {
        let viewport = Viewport::new();
        let point = rect().center() + Vec2::new(1.0e9, -1.0e9);
        let snapped = viewport.screen_to_grid(rect(), point).unwrap();
        assert_eq!(snapped.x, GRID_MAX);
        assert_eq!(snapped.y, GRID_MIN);
    }

    #[test]
    fn visible_bounds_cover_the_canvas() {
        let viewport = Viewport::new();
        let (min, max) = viewport.visible_bounds(rect());
        assert!(min.x < 0 && min.y < 0);
        assert!(max.x > 0 && max.y > 0);
        assert!(min.x >= GRID_MIN && max.x <= GRID_MAX);
        assert!(min.y >= GRID_MIN && max.y <= GRID_MAX);
    }

    #[test]
    fn navigation_centers_and_pans_by_grid_cells() {
        let mut viewport = Viewport::new();
        viewport.center_on(GridPos::new(12, -8));
        let center = viewport.grid_to_screen(rect(), GridPos::new(12, -8));
        assert!((center - rect().center()).length() < 1e-4);
        viewport.pan_grid(GridPos::new(-3, 2));
        let moved = viewport.grid_to_screen(rect(), GridPos::new(12, -8));
        let expected = rect().center() + Vec2::new(-3.0, 2.0) * viewport.scale();
        assert!((moved - expected).length() < 1e-4);
    }

    #[test]
    fn zoom_presets_preserve_the_world_centre() {
        let mut viewport = Viewport::new();
        viewport.center_on(GridPos::new(4, 6));
        viewport.zoom_center(rect(), 2.0);
        assert!((viewport.zoom_percent() - 200.0).abs() < 1e-3);
        let center = viewport.grid_to_screen(rect(), GridPos::new(4, 6));
        assert!((center - rect().center()).length() < 1e-3);
        viewport.set_scale(rect(), DEFAULT_SCALE);
        assert!((viewport.zoom_percent() - 100.0).abs() < 1e-3);
    }

    #[test]
    fn minimap_projects_bounds_and_round_trips_points() {
        let area = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(160.0, 110.0));
        let bounds = (GridPos::new(-20, -10), GridPos::new(40, 30));
        let transform = MinimapTransform::new(area, bounds).unwrap();
        assert!(transform.scale() > 0.0);
        for point in [bounds.0, bounds.1, GridPos::new(0, 0), GridPos::new(15, 12)] {
            let projected = transform.point(point);
            assert!(area.contains(projected));
            assert_eq!(transform.grid_at(projected), point);
        }
        let painted = transform.rect(bounds.0, bounds.1);
        assert!(painted.width() <= area.width());
        assert!(painted.height() <= area.height());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn minimap_round_trips_integer_points(
            x in -500_i32..500,
            y in -500_i32..500,
            width in 1_i32..400,
            height in 1_i32..400,
        ) {
            let min = GridPos::new(x, y);
            let max = GridPos::new(x + width, y + height);
            let area = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(180.0, 120.0));
            let transform = MinimapTransform::new(area, (min, max)).unwrap();
            let point = GridPos::new(x + width / 2, y + height / 2);
            prop_assert_eq!(transform.grid_at(transform.point(point)), point);
        }
    }
}
