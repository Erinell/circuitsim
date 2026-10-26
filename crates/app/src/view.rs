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

impl Default for Viewport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
