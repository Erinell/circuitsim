//! Interactive schematic canvas with pan, zoom, placement, and dragging.

use std::collections::{BTreeMap, BTreeSet};

use editor_core::{
    GridPos, ObjectRef, Orientation, SnapKind, SnapTarget, Symbol, SymbolId, WireId,
};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Key, Painter, PointerButton, Pos2, Rect, Sense,
    Stroke, StrokeKind, Ui, Vec2,
};
use sim_core::units::{format_si_precision, Unit};

use crate::analysis::{LiveValues, ProbeReading, SignalId};
use crate::currents::{net_current, net_flows, wire_current, wire_currents};
use crate::document::{
    format_parameter, parameter_unit, ActionError, ClipboardItem, Document, SelectionChange,
    SelectionMode, WireEnd,
};
use crate::overcurrent::{self, Severity};
use crate::ui::controls::{control_fraction, control_value, ParameterControls};
use crate::ui::glyph::{paint_symbol, GlyphColors, SymbolText};
use crate::ui::theme::{
    CanvasPalette, CanvasSettings, CanvasTheme, CurrentDirection, VoltageScale,
};
use crate::ui::{muted_text_color, symbol_kind_name, Status};
use crate::view::{MinimapTransform, Viewport, DEFAULT_SCALE};

const MINIMAP_WIDTH: f32 = 152.0;
const MINIMAP_HEIGHT: f32 = 112.0;
const MINIMAP_MARGIN: f32 = 12.0;
const ZOOM_STEP: f32 = 1.25;
const ENDPOINT_RADIUS: f32 = 11.0;

/// Canvas interaction state that survives between frames.
pub struct CanvasState {
    viewport: Viewport,
    drag: Option<DragState>,
    wire_start: Option<GridPos>,
    wire_prefer_horizontal: bool,
    placement_orientation: Orientation,
    placement_mirrored: bool,
    hover: Option<ObjectRef>,
    hover_net: BTreeSet<ObjectRef>,
    snap_target: Option<SnapTarget>,
    last_pointer_grid: Option<GridPos>,
    context_menu_point: Option<GridPos>,
    paste_preview: bool,
    fit_requested: bool,
    pending_fit: Option<(GridPos, GridPos)>,
    last_rect: Option<Rect>,
    minimap_visible: bool,
    animation_speed: f64,
    power_brightness: f32,
    settings: CanvasSettings,
    controls: ParameterControls,
    scope_signals: BTreeSet<SignalId>,
    property_request: Option<SymbolId>,
    scope_request: Option<SignalId>,
    label_dialog: Option<LabelDialog>,
}

#[derive(Clone, Debug)]
enum DragState {
    Objects {
        start: GridPos,
        current: GridPos,
    },
    Segment {
        wire: WireId,
        start: GridPos,
        current: GridPos,
    },
    WireEndpoint {
        wire: WireId,
        end: WireEnd,
        start: GridPos,
        current: GridPos,
    },
    WireFromPin {
        start: GridPos,
        current: GridPos,
        prefer_horizontal: bool,
    },
    Marquee {
        start: GridPos,
        current: GridPos,
        change: SelectionChange,
    },
}

/// Pending label name dialog.
struct LabelDialog {
    at: GridPos,
    text: String,
    error: Option<String>,
    focus: bool,
}

impl CanvasState {
    /// Creates a canvas that fits its content on the first frame.
    pub fn new() -> Self {
        Self {
            viewport: Viewport::new(),
            drag: None,
            wire_start: None,
            wire_prefer_horizontal: true,
            placement_orientation: Orientation::R0,
            placement_mirrored: false,
            hover: None,
            hover_net: BTreeSet::new(),
            snap_target: None,
            last_pointer_grid: None,
            context_menu_point: None,
            paste_preview: false,
            fit_requested: true,
            pending_fit: None,
            last_rect: None,
            minimap_visible: true,
            animation_speed: 1.0,
            power_brightness: 0.6,
            settings: CanvasSettings::default(),
            controls: ParameterControls::default(),
            scope_signals: BTreeSet::new(),
            property_request: None,
            scope_request: None,
            label_dialog: None,
        }
    }

    /// Returns the current viewport.
    pub const fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// Requests that the next frame fits all content.
    pub fn request_fit(&mut self) {
        self.fit_requested = true;
    }

    /// Cancels an in-progress drag or wire without touching the document.
    pub fn cancel_interaction(&mut self) {
        self.drag = None;
        self.wire_start = None;
        self.wire_prefer_horizontal = true;
        self.placement_orientation = Orientation::R0;
        self.placement_mirrored = false;
        self.paste_preview = false;
    }

    /// Arms a clipboard paste preview at the pointer.
    pub fn begin_paste(&mut self) {
        self.paste_preview = true;
        self.drag = None;
        self.wire_start = None;
    }

    /// Returns true while a paste preview follows the pointer.
    pub const fn is_pasting(&self) -> bool {
        self.paste_preview
    }

    /// Returns the last snapped pointer position, if any.
    pub const fn last_pointer_grid(&self) -> Option<GridPos> {
        self.last_pointer_grid
    }

    fn remember_context_point(&mut self, opened: bool, current: Option<GridPos>) {
        if opened {
            self.context_menu_point = current;
        }
    }

    fn placement_origin(
        &self,
        document: &Document,
        kind: &editor_core::SymbolKind,
        target: GridPos,
    ) -> GridPos {
        let Some(definition) = document.registry().get(kind) else {
            return target;
        };
        let Some(snap) = self.snap_target else {
            return target;
        };
        let pin_index = match snap.kind {
            SnapKind::Pin(_) => 0,
            SnapKind::Wire(id) => {
                let Some(wire) = document.wire(id) else {
                    return target;
                };
                if snap.at == wire.b {
                    0
                } else if snap.at == wire.a {
                    1
                } else {
                    return target;
                }
            }
        };
        let Some(pin) = definition.pins.get(pin_index) else {
            return target;
        };
        let preview = Symbol::from_def(
            SymbolId(0),
            definition,
            GridPos::default(),
            "",
            self.placement_orientation,
            self.placement_mirrored,
        );
        let Some(offset) = preview.pin_position_of(pin) else {
            return target;
        };
        target.checked_sub(offset).unwrap_or(target)
    }

    /// Rotates the placement ghost.
    pub fn rotate_placement(&mut self) {
        self.placement_orientation = self.placement_orientation.next();
    }

    /// Mirrors the placement ghost.
    pub fn mirror_placement(&mut self) {
        self.placement_mirrored = !self.placement_mirrored;
    }

    /// Returns the current placement orientation.
    pub const fn placement_orientation(&self) -> Orientation {
        self.placement_orientation
    }

    /// Returns the current placement mirror state.
    pub const fn placement_mirrored(&self) -> bool {
        self.placement_mirrored
    }

    /// Zooms in around the canvas centre.
    pub fn zoom_in(&mut self) {
        if let Some(rect) = self.last_rect {
            self.viewport.zoom_center(rect, ZOOM_STEP);
        }
    }

    /// Zooms out around the canvas centre.
    pub fn zoom_out(&mut self) {
        if let Some(rect) = self.last_rect {
            self.viewport.zoom_center(rect, 1.0 / ZOOM_STEP);
        }
    }

    /// Restores the default zoom without moving the world centre.
    pub fn reset_zoom(&mut self) {
        if let Some(rect) = self.last_rect {
            self.viewport.set_scale(rect, DEFAULT_SCALE);
        }
    }

    /// Restores the default zoom and centers the grid origin.
    pub fn reset_view(&mut self) {
        self.viewport.reset();
    }

    /// Pans by a whole number of grid cells.
    pub fn pan_by_grid(&mut self, delta: GridPos) {
        self.viewport.pan_grid(delta);
    }

    /// Requests a fit around explicit grid bounds on the next frame.
    pub fn request_fit_bounds(&mut self, bounds: (GridPos, GridPos)) {
        self.pending_fit = Some(bounds);
    }

    #[cfg(test)]
    pub(crate) const fn pending_fit_bounds_for_test(&self) -> Option<(GridPos, GridPos)> {
        self.pending_fit
    }

    /// Returns whether the minimap is drawn.
    pub const fn minimap_visible(&self) -> bool {
        self.minimap_visible
    }

    /// Shows or hides the minimap.
    pub fn set_minimap_visible(&mut self, visible: bool) {
        self.minimap_visible = visible;
    }

    /// Returns whether current flow is animated on wires.
    pub const fn current_flow_visible(&self) -> bool {
        self.settings.layers.current
    }

    /// Shows or hides the animated current flow.
    pub fn set_current_flow_visible(&mut self, visible: bool) {
        self.settings.layers.current = visible;
    }

    /// Sets the current-animation speed multiplier.
    pub fn set_animation_speed(&mut self, speed: f64) {
        if speed.is_finite() && speed > 0.0 {
            self.animation_speed = speed.clamp(0.1, 4.0);
        }
    }

    /// Sets the component-power glow strength.
    pub fn set_power_brightness(&mut self, brightness: f32) {
        if brightness.is_finite() {
            self.power_brightness = brightness.clamp(0.0, 1.0);
        }
    }

    /// Returns the canvas display settings.
    pub const fn settings(&self) -> &CanvasSettings {
        &self.settings
    }

    /// Returns mutable canvas display settings.
    pub fn settings_mut(&mut self) -> &mut CanvasSettings {
        &mut self.settings
    }

    /// Applies the canvas color theme.
    pub fn set_theme(&mut self, theme: CanvasTheme) {
        self.settings.theme = theme;
    }

    /// Sets the signals currently plotted in inline scopes.
    pub fn set_scope_signals(&mut self, signals: BTreeSet<SignalId>) {
        self.scope_signals = signals;
    }

    /// Takes one pending double-click property request.
    pub fn take_property_request(&mut self) -> Option<SymbolId> {
        self.property_request.take()
    }

    /// Takes one pending signal request for an inline scope.
    pub fn take_scope_request(&mut self) -> Option<SignalId> {
        self.scope_request.take()
    }

    /// Returns true while the label name dialog is open.
    pub const fn has_dialog(&self) -> bool {
        self.label_dialog.is_some()
    }

    /// Closes the label dialog without creating a label.
    pub fn cancel_dialog(&mut self) {
        self.label_dialog = None;
    }

    /// Draws and handles the canvas for one frame.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        document: &mut Document,
        tool: &crate::ui::Tool,
        status: &mut Status,
        interactive: bool,
        live: &LiveValues,
        probes: &[ProbeReading],
        visible_probes: &BTreeSet<SymbolId>,
        flow_time: f64,
    ) {
        let available = ui.available_size();
        if available.x < 2.0 || available.y < 2.0 {
            return;
        }
        let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
        let rect = response.rect;
        self.last_rect = Some(rect);
        painter.rect_filled(rect, 0.0, self.settings.palette().background);

        if let Some(bounds) = self.pending_fit.take() {
            self.viewport.fit(rect, bounds);
            self.fit_requested = false;
        } else if self.fit_requested {
            if let Some(bounds) = document.content_bounds() {
                self.viewport.fit(rect, bounds);
            } else {
                self.viewport.reset();
            }
            self.fit_requested = false;
        }

        self.handle_viewport(ui, &response, rect);
        if !interactive && response.dragged_by(PointerButton::Primary) {
            self.viewport.pan_by(response.drag_delta());
        }
        let raw_point = ui
            .input(|input| input.pointer.hover_pos())
            .filter(|position| rect.contains(*position))
            .and_then(|position| self.viewport.screen_to_grid(rect, position));
        let pin_hover = raw_point.is_some_and(|point| document.pin_at(point).is_some());
        let snapping = matches!(tool, crate::ui::Tool::Wire | crate::ui::Tool::Place(_))
            || matches!(self.drag, Some(DragState::WireFromPin { .. }))
            || (matches!(tool, crate::ui::Tool::Select) && pin_hover);
        self.snap_target = if snapping {
            raw_point.and_then(|point| document.nearest_snap(point))
        } else {
            None
        };
        let focus_point = self.snap_target.map_or(raw_point, |target| Some(target.at));
        self.last_pointer_grid = focus_point;
        self.hover = raw_point.and_then(|point| document.object_at(point));
        self.hover_net = raw_point
            .filter(|point| {
                document.pin_at(*point).is_some()
                    || matches!(
                        document.object_at(*point),
                        Some(ObjectRef::Wire(_) | ObjectRef::Label(_) | ObjectRef::Junction(_))
                    )
            })
            .and_then(|point| document.net_objects_at(point))
            .unwrap_or_default();

        if interactive {
            self.handle_placement_keys(ui, tool);
            let controls_handled =
                self.handle_parameter_controls(ui, &response, rect, document, status);
            if !controls_handled {
                self.handle_input(ui, &response, rect, document, tool, status, focus_point);
            }
        }
        let hovered_point = focus_point.or_else(|| {
            ui.input(|input| input.pointer.hover_pos())
                .filter(|position| rect.contains(*position))
                .and_then(|position| self.viewport.screen_to_grid(rect, position))
        });

        self.paint_grid(&painter, rect);
        self.paint_document(&painter, rect, document, live);
        self.paint_scope_markers(&painter, rect, document);
        self.paint_current_flow(&painter, rect, document, live, flow_time);
        self.paint_voltage_legend(&painter, rect, document, live);
        self.paint_parameter_controls(&painter, rect, document);
        self.paint_wire_handles(&painter, rect, document);
        self.paint_hover(&painter, rect, document);
        let shift = ui.input(|input| input.modifiers.shift);
        self.paint_previews(&painter, rect, document, tool, hovered_point, shift);
        if self.paste_preview {
            if let Some(point) = hovered_point {
                self.paint_clipboard_ghost(&painter, rect, document, point);
            }
        }
        self.paint_snap(&painter, rect);
        self.paint_empty_state(&painter, rect, document);
        self.paint_tool_badge(&painter, rect, tool);
        self.paint_simulation_badge(&painter, rect, document, live);
        self.paint_minimap(&painter, rect, document);
        self.paint_probe_values(&painter, rect, document, probes, visible_probes);
        if interactive && self.drag.is_none() && !self.paste_preview {
            if let Some(position) = ui.input(|input| input.pointer.hover_pos()) {
                self.paint_hover_readout(&painter, rect, document, live, position);
            }
        }
        self.set_cursor(ui, &response, tool, document);
        if interactive {
            self.context_menu(&response, document, status);
        }
    }

    /// Shows the label naming dialog and creates the label on confirmation.
    pub fn show_label_dialog(
        &mut self,
        context: &egui::Context,
        document: &mut Document,
        status: &mut Status,
    ) {
        let Some(dialog) = self.label_dialog.as_mut() else {
            return;
        };
        let names = document
            .existing_net_names()
            .into_iter()
            .filter(|name| name != "0")
            .take(8)
            .collect::<Vec<_>>();
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new("Nommer le net")
            .id(egui::Id::new("label_name_dialog"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(context, |ui| {
                ui.label(format!("Position : ({}, {})", dialog.at.x, dialog.at.y));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut dialog.text)
                        .desired_width(240.0)
                        .hint_text("ex. VOUT"),
                );
                if dialog.focus {
                    response.request_focus();
                    dialog.focus = false;
                }
                if let Some(error) = &dialog.error {
                    ui.colored_label(Color32::from_rgb(240, 128, 128), error);
                }
                if !names.is_empty() {
                    ui.label(
                        egui::RichText::new("Nets existants")
                            .small()
                            .color(muted_text_color(ui)),
                    );
                    ui.horizontal_wrapped(|ui| {
                        for name in &names {
                            if ui.small_button(name).clicked() {
                                dialog.text.clone_from(name);
                                dialog.focus = true;
                            }
                        }
                    });
                }
                ui.label(
                    egui::RichText::new("Le nom est coupé autour des espaces extérieurs.")
                        .small()
                        .color(muted_text_color(ui)),
                );
                ui.horizontal(|ui| {
                    if ui.button("Créer").clicked() {
                        submit = true;
                    }
                    if ui.button("Annuler").clicked() {
                        cancel = true;
                    }
                    if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                        cancel = true;
                    }
                });
            });

        if submit {
            let at = dialog.at;
            let text = dialog.text.clone();
            match document.add_label(at, text) {
                Ok(_) => {
                    status.success("Label ajouté.");
                    self.label_dialog = None;
                }
                Err(error) => {
                    let message = error.user_message();
                    dialog.error = Some(message.clone());
                    status.error(message);
                }
            }
        } else if cancel {
            self.label_dialog = None;
            status.info("Création du label annulée.");
        }
    }

    fn handle_viewport(&mut self, ui: &Ui, response: &egui::Response, rect: Rect) {
        let (middle_down, pointer_delta, hover_pos, scroll, zoom) = ui.input(|input| {
            (
                input.pointer.middle_down(),
                input.pointer.delta(),
                input.pointer.hover_pos(),
                input.smooth_scroll_delta,
                input.zoom_delta(),
            )
        });
        if middle_down && (response.hovered() || response.dragged_by(PointerButton::Middle)) {
            self.viewport.pan_by(pointer_delta);
        }
        if response.hovered() {
            if let Some(anchor) = hover_pos.filter(|position| rect.contains(*position)) {
                if (zoom - 1.0).abs() > 1e-4 {
                    self.viewport.zoom_at(rect, anchor, zoom);
                } else if scroll.y.abs() > 0.01 {
                    self.viewport
                        .zoom_at(rect, anchor, (scroll.y * 0.0015).exp());
                }
            }
        }
    }

    fn handle_placement_keys(&mut self, ui: &Ui, tool: &crate::ui::Tool) {
        if !matches!(tool, crate::ui::Tool::Place(_)) {
            return;
        }
        if ui.input(|input| input.key_pressed(Key::R)) {
            self.rotate_placement();
        }
        if ui.input(|input| input.key_pressed(Key::F)) {
            self.mirror_placement();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_input(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        rect: Rect,
        document: &mut Document,
        tool: &crate::ui::Tool,
        status: &mut Status,
        hovered_point: Option<GridPos>,
    ) {
        let (primary_pressed, primary_released, primary_down, shift, ctrl, alt, latest_pos) = ui
            .input(|input| {
                (
                    input.pointer.primary_pressed(),
                    input.pointer.primary_released(),
                    input.pointer.primary_down(),
                    input.modifiers.shift,
                    input.modifiers.command || input.modifiers.ctrl,
                    input.modifiers.alt,
                    input.pointer.hover_pos(),
                )
            });
        let latest_point = latest_pos
            .and_then(|position| self.viewport.screen_to_grid(rect, position))
            .or(hovered_point);

        if response.double_clicked_by(PointerButton::Primary) {
            if let Some(point) = hovered_point {
                if let Some(ObjectRef::Symbol(id)) = document.object_at(point) {
                    self.property_request = Some(id);
                    self.drag = None;
                    return;
                }
            }
        }

        if let Some((map_rect, transform)) = self.minimap_geometry(rect, document) {
            if let Some(position) = latest_pos.filter(|position| map_rect.contains(*position)) {
                if primary_pressed {
                    self.viewport.center_on(transform.grid_at(position));
                }
                self.drag = None;
                return;
            }
        }

        if self.paste_preview {
            if primary_pressed {
                if let Some(point) = hovered_point {
                    match document.paste_clipboard(point) {
                        Ok(count) => status.success(format!("{count} objet(s) collé(s).")),
                        Err(error) => status.error(error.user_message()),
                    }
                }
                self.drag = None;
            }
            return;
        }

        if response.hovered() && primary_pressed && matches!(tool, crate::ui::Tool::Select) {
            if let Some(point) = hovered_point {
                if alt {
                    match document.select_net_at(point) {
                        Ok(count) => {
                            status.success(format!("{count} objet(s) du net sélectionné(s)."))
                        }
                        Err(error) => status.error(error.user_message()),
                    }
                    self.drag = None;
                    return;
                }
                let change = if ctrl {
                    SelectionChange::Toggle
                } else if shift {
                    SelectionChange::Add
                } else {
                    SelectionChange::Replace
                };
                if !ctrl && !shift && document.pin_at(point).is_some() {
                    self.drag = Some(DragState::WireFromPin {
                        start: point,
                        current: point,
                        prefer_horizontal: true,
                    });
                } else if let Some((wire, end, start)) =
                    self.free_endpoint_near(rect, latest_pos, document)
                {
                    document.select_only(Some(ObjectRef::Wire(wire)));
                    self.drag = Some(DragState::WireEndpoint {
                        wire,
                        end,
                        start,
                        current: start,
                    });
                } else if let Some(object) = document.object_at(point) {
                    match change {
                        SelectionChange::Replace => {
                            if !document.selection().contains(&object) {
                                document.select_only(Some(object));
                            }
                        }
                        SelectionChange::Add | SelectionChange::Toggle => {
                            document.toggle_selection(object);
                        }
                    }
                    self.drag = if document.selection().contains(&object) {
                        Some(match object {
                            ObjectRef::Wire(wire) => DragState::Segment {
                                wire,
                                start: point,
                                current: point,
                            },
                            _ => DragState::Objects {
                                start: point,
                                current: point,
                            },
                        })
                    } else {
                        None
                    };
                } else {
                    self.drag = Some(DragState::Marquee {
                        start: point,
                        current: point,
                        change,
                    });
                }
            }
        }

        if primary_down {
            if let Some(drag) = self.drag.as_mut() {
                if let Some(point) = latest_point {
                    match drag {
                        DragState::Objects { current, .. }
                        | DragState::Segment { current, .. }
                        | DragState::WireEndpoint { current, .. }
                        | DragState::WireFromPin { current, .. }
                        | DragState::Marquee { current, .. } => {
                            *current = point;
                        }
                    }
                }
            }
        }

        if primary_released {
            if let Some(drag) = self.drag.take() {
                match drag {
                    DragState::Objects { start, current } => {
                        let end = latest_point.unwrap_or(current);
                        let delta = GridPos::new(
                            end.x.saturating_sub(start.x),
                            end.y.saturating_sub(start.y),
                        );
                        if delta != GridPos::default() {
                            match document.move_selection(delta) {
                                Ok(()) => status.success("Sélection déplacée."),
                                Err(error) => status.error(error.user_message()),
                            }
                        }
                    }
                    DragState::Segment {
                        wire,
                        start,
                        current,
                    } => {
                        let end = latest_point.unwrap_or(current);
                        let delta = if document.wire(wire).is_some() {
                            drag_axis_delta(start, end)
                        } else {
                            GridPos::default()
                        };
                        if delta != GridPos::default() {
                            match document.move_wire_segment(wire, delta) {
                                Ok(()) => status.success("Segment déplacé."),
                                Err(error) => status.error(error.user_message()),
                            }
                        }
                    }
                    DragState::WireEndpoint {
                        wire,
                        end,
                        start,
                        current,
                    } => {
                        let target = latest_point.unwrap_or(current);
                        if target != start {
                            match document.move_wire_endpoint(wire, end, target) {
                                Ok(()) => status.success("Extrémité du fil déplacée."),
                                Err(error) => status.error(error.user_message()),
                            }
                        }
                    }
                    DragState::WireFromPin {
                        start,
                        current,
                        prefer_horizontal,
                    } => {
                        let target = self
                            .snap_target
                            .map_or(latest_point.unwrap_or(current), |snap| snap.at);
                        let prefer_horizontal = if shift {
                            !prefer_horizontal
                        } else {
                            prefer_horizontal
                        };
                        if target != start {
                            let path = document.route_path(start, target, prefer_horizontal);
                            match document.add_wire_path(&path) {
                                Ok(wires) => {
                                    let count = wires.len();
                                    status.success(format!("{count} segment(s) de fil créé(s)."));
                                }
                                Err(error) => status.error(error.user_message()),
                            }
                        }
                    }
                    DragState::Marquee {
                        start,
                        current,
                        change,
                    } => {
                        let mode = if start.x <= current.x {
                            SelectionMode::Enclosing
                        } else {
                            SelectionMode::Touching
                        };
                        let count = document.select_in_rect(start, current, mode, change);
                        if count > 0 {
                            let rule = match mode {
                                SelectionMode::Enclosing => "inclus",
                                SelectionMode::Touching => "touchés",
                            };
                            status.success(format!("{count} objet(s) {rule} sélectionné(s)."));
                        }
                    }
                }
            }
        }

        if response.clicked_by(PointerButton::Primary) {
            if let Some(point) = hovered_point {
                self.handle_click(point, document, tool, status, shift);
            }
        }
    }

    fn context_menu(
        &mut self,
        response: &egui::Response,
        document: &mut Document,
        status: &mut Status,
    ) {
        self.remember_context_point(response.secondary_clicked(), self.last_pointer_grid);
        let point = self.context_menu_point;
        response.context_menu(|ui| {
            let Some(point) = point else {
                ui.label("Aucun emplacement sous le curseur.");
                return;
            };
            let object = document.object_at(point);
            if let Some(object) = object {
                if !document.selection().contains(&object) {
                    document.select_only(Some(object));
                }
                match object {
                    ObjectRef::Symbol(id) => {
                        if ui.button("Propriétés…").clicked() {
                            self.property_request = Some(id);
                            ui.close();
                        }
                        if ui.button("Pivoter").clicked() {
                            report(status, document.rotate_selection(), "Symbole(s) pivoté(s).");
                            ui.close();
                        }
                        if ui.button("Miroir").clicked() {
                            report(
                                status,
                                document.mirror_selection(),
                                "Symbole(s) mis en miroir.",
                            );
                            ui.close();
                        }
                        if ui
                            .button("Mesurer la résistance")
                            .on_hover_text("Ajoute un ohmmètre sur ce composant")
                            .clicked()
                        {
                            let at = document.symbol(id).map_or(point, |symbol| symbol.at);
                            match document.add_symbol(editor_core::SymbolKind::Ohmmeter, at) {
                                Ok(probe) => {
                                    document.select_only(Some(ObjectRef::Symbol(probe)));
                                    status.success("Ohmmètre ajouté.");
                                }
                                Err(error) => status.error(error.user_message()),
                            }
                            ui.close();
                        }
                        ui.menu_button("Ajouter un curseur", |ui| {
                            let Some(symbol) = document.symbol(id) else {
                                return;
                            };
                            let numeric = symbol
                                .params
                                .iter()
                                .filter_map(|(key, value)| {
                                    value.as_number().map(|value| (key.clone(), value))
                                })
                                .collect::<Vec<_>>();
                            if numeric.is_empty() {
                                ui.label("Aucun paramètre numérique.");
                            }
                            for (key, value) in numeric {
                                if ui.button(format!("{} = {value}", key)).clicked() {
                                    self.controls.add(id, key, value);
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("Tracer dans une portée", |ui| {
                            let Some(symbol) = document.symbol(id) else {
                                return;
                            };
                            let mut signals = Vec::new();
                            for (key, position) in document.pin_positions() {
                                if key.symbol != symbol.id {
                                    continue;
                                }
                                let Some(net) = document.net_name_at(position) else {
                                    continue;
                                };
                                if net == "0" {
                                    continue;
                                }
                                for signal in
                                    [SignalId::Voltage(net.clone()), SignalId::NetCurrent(net)]
                                {
                                    if !signals.contains(&signal) {
                                        signals.push(signal);
                                    }
                                }
                            }
                            if signals.is_empty() {
                                ui.label("Aucun signal disponible.");
                            }
                            for signal in signals {
                                if ui.button(signal.label()).clicked() {
                                    self.scope_request = Some(signal);
                                    ui.close();
                                }
                            }
                        });
                    }
                    ObjectRef::Label(id) => {
                        if ui.button("Renommer").clicked() {
                            if let Some(label) = document.label(id) {
                                self.label_dialog = Some(LabelDialog {
                                    at: label.at,
                                    text: label.text.clone(),
                                    error: None,
                                    focus: true,
                                });
                            }
                            ui.close();
                        }
                        if ui.button("Sélectionner le net").clicked() {
                            report(
                                status,
                                document.select_net_at(point).map(|_| ()),
                                "Net sélectionné.",
                            );
                            ui.close();
                        }
                    }
                    ObjectRef::Wire(id) => {
                        if ui.button("Couper ici").clicked() {
                            report(status, document.split_wire(id, point), "Fil coupé.");
                            ui.close();
                        }
                        if ui.button("Ajouter une jonction").clicked() {
                            report(
                                status,
                                document.add_junction(point).map(|_| ()),
                                "Jonction ajoutée.",
                            );
                            ui.close();
                        }
                        if ui.button("Ajouter un label").clicked() {
                            self.label_dialog = Some(LabelDialog {
                                at: point,
                                text: document
                                    .net_name_at(point)
                                    .unwrap_or_else(|| "NET".to_owned()),
                                error: None,
                                focus: true,
                            });
                            ui.close();
                        }
                        if ui.button("Sélectionner le net").clicked() {
                            report(
                                status,
                                document.select_net_at(point).map(|_| ()),
                                "Net sélectionné.",
                            );
                            ui.close();
                        }
                        if let Some(net) = document.net_name_at(point) {
                            if net != "0" {
                                ui.menu_button("Tracer dans une portée", |ui| {
                                    if ui.button("Tension").clicked() {
                                        self.scope_request = Some(SignalId::Voltage(net.clone()));
                                        ui.close();
                                    }
                                    if ui.button("Courant").clicked() {
                                        self.scope_request =
                                            Some(SignalId::NetCurrent(net.clone()));
                                        ui.close();
                                    }
                                });
                            }
                        }
                    }
                    ObjectRef::Junction(_) => {
                        if ui.button("Sélectionner le net").clicked() {
                            report(
                                status,
                                document.select_net_at(point).map(|_| ()),
                                "Net sélectionné.",
                            );
                            ui.close();
                        }
                    }
                }
                ui.separator();
                if ui.button("Copier").clicked() {
                    report(status, document.copy_selection().map(|_| ()), "Copié.");
                    ui.close();
                }
                if ui.button("Dupliquer").clicked() {
                    report(
                        status,
                        document.duplicate_selection().map(|_| ()),
                        "Sélection dupliquée.",
                    );
                    ui.close();
                }
                if ui.button("Supprimer").clicked() {
                    report(status, document.delete_selection(), "Sélection supprimée.");
                    ui.close();
                }
            } else {
                ui.menu_button("Ajouter", |ui| {
                    for (category, items) in [
                        (
                            "Sources",
                            vec![
                                (editor_core::SymbolKind::VoltageSource, "Source de tension"),
                                (editor_core::SymbolKind::CurrentSource, "Source de courant"),
                            ],
                        ),
                        (
                            "Passifs",
                            vec![
                                (editor_core::SymbolKind::Resistor, "Résistance"),
                                (editor_core::SymbolKind::Capacitor, "Condensateur"),
                                (editor_core::SymbolKind::Inductor, "Bobine"),
                            ],
                        ),
                        (
                            "Semi-conducteurs",
                            vec![
                                (editor_core::SymbolKind::Diode, "Diode"),
                                (editor_core::SymbolKind::Led, "LED"),
                                (editor_core::SymbolKind::Zener, "Zener"),
                                (
                                    editor_core::SymbolKind::ShuntReference,
                                    "Référence shunt programmable",
                                ),
                                (
                                    editor_core::SymbolKind::Comparator,
                                    "Comparateur collecteur ouvert",
                                ),
                                (editor_core::SymbolKind::OpAmp, "Amplificateur"),
                            ],
                        ),
                        (
                            "Mesures",
                            vec![(editor_core::SymbolKind::Ohmmeter, "Ohmmètre")],
                        ),
                        (
                            "Divers",
                            vec![
                                (editor_core::SymbolKind::Ground, "Masse"),
                                (editor_core::SymbolKind::Switch, "Interrupteur"),
                            ],
                        ),
                    ] {
                        ui.menu_button(category, |ui| {
                            for (kind, label) in items {
                                if ui.button(label).clicked() {
                                    match document.add_symbol(kind, point) {
                                        Ok(id) => {
                                            document.select_only(Some(ObjectRef::Symbol(id)));
                                            status.success(format!("{label} ajouté."));
                                        }
                                        Err(error) => status.error(error.user_message()),
                                    }
                                    ui.close();
                                }
                            }
                        });
                    }
                });
                if document.clipboard().is_some() {
                    if ui.button("Coller ici").clicked() {
                        report(
                            status,
                            document.paste_clipboard(point).map(|_| ()),
                            "Collé.",
                        );
                        ui.close();
                    }
                    ui.separator();
                }
                if ui.button("Tout sélectionner").clicked() {
                    document.select_all();
                    status.info("Tous les objets sont sélectionnés.");
                    ui.close();
                }
                if ui.button("Ajuster la vue").clicked() {
                    self.request_fit();
                    ui.close();
                }
                ui.separator();
                if ui.button("Annuler").clicked() {
                    match document.undo() {
                        Ok(true) => status.success("Dernière modification annulée."),
                        Ok(false) => status.info("Aucune modification à annuler."),
                        Err(error) => status.error(error.user_message()),
                    }
                    ui.close();
                }
                if ui.button("Rétablir").clicked() {
                    match document.redo() {
                        Ok(true) => status.success("Modification rétablie."),
                        Ok(false) => status.info("Aucune modification à rétablir."),
                        Err(error) => status.error(error.user_message()),
                    }
                    ui.close();
                }
            }
        });
    }

    fn free_endpoint_near(
        &self,
        rect: Rect,
        pointer: Option<Pos2>,
        document: &Document,
    ) -> Option<(WireId, WireEnd, GridPos)> {
        let pointer = pointer?;
        let mut best: Option<(f32, WireId, WireEnd, GridPos)> = None;
        for (wire, end, point) in document.free_wire_endpoints() {
            let screen = self.viewport.grid_to_screen(rect, point);
            let distance = (screen - pointer).length();
            if distance <= ENDPOINT_RADIUS
                && best.is_none_or(|(best_distance, _, _, _)| distance < best_distance)
            {
                best = Some((distance, wire, end, point));
            }
        }
        best.map(|(_, wire, end, point)| (wire, end, point))
    }

    fn handle_click(
        &mut self,
        point: GridPos,
        document: &mut Document,
        tool: &crate::ui::Tool,
        status: &mut Status,
        shift: bool,
    ) {
        match tool {
            crate::ui::Tool::Select => {
                if let Some(ObjectRef::Symbol(id)) = document.object_at(point) {
                    if document
                        .symbol(id)
                        .is_some_and(|symbol| symbol.kind == editor_core::SymbolKind::Switch)
                    {
                        match document.toggle_switch(id) {
                            Ok(closed) => status.success(if closed {
                                "Interrupteur fermé."
                            } else {
                                "Interrupteur ouvert."
                            }),
                            Err(error) => status.error(error.user_message()),
                        }
                    }
                }
            }
            crate::ui::Tool::Place(kind) => {
                let insertable = document
                    .registry()
                    .get(kind)
                    .is_some_and(|definition| definition.pins.len() == 2);
                let wire_interior = match self.snap_target {
                    Some(SnapTarget {
                        at,
                        kind: SnapKind::Wire(id),
                    }) => document
                        .wire(id)
                        .filter(|wire| at != wire.a && at != wire.b)
                        .map(|_| (id, at)),
                    _ => None,
                };
                if let Some((wire, at)) = wire_interior {
                    if insertable {
                        match document.insert_symbol_on_wire(kind.clone(), wire, at) {
                            Ok(_) => {
                                status.success(format!(
                                    "{} inséré en série à ({}, {}).",
                                    symbol_kind_name(kind),
                                    at.x,
                                    at.y
                                ));
                                return;
                            }
                            Err(error) => {
                                status.error(error.user_message());
                                return;
                            }
                        }
                    }
                }
                let origin = self.placement_origin(document, kind, point);
                match document.add_symbol_oriented(
                    kind.clone(),
                    origin,
                    self.placement_orientation,
                    self.placement_mirrored,
                ) {
                    Ok(_) => {
                        if origin == point {
                            status.success(format!(
                                "{} ajouté à ({}, {}).",
                                symbol_kind_name(kind),
                                origin.x,
                                origin.y
                            ));
                        } else {
                            status.success(format!(
                                "{} ajouté, broche alignée sur ({}, {}).",
                                symbol_kind_name(kind),
                                point.x,
                                point.y
                            ));
                        }
                    }
                    Err(error) => status.error(error.user_message()),
                }
            }
            crate::ui::Tool::Wire => {
                if let Some(start) = self.wire_start {
                    if start == point {
                        self.wire_start = None;
                        status.info("Fil annulé : extrémités identiques.");
                    } else {
                        let prefer_horizontal = if shift {
                            !self.wire_prefer_horizontal
                        } else {
                            self.wire_prefer_horizontal
                        };
                        let path = document.route_path(start, point, prefer_horizontal);
                        match document.add_wire_path(&path) {
                            Ok(_) => {
                                self.wire_start = None;
                                status.success("Fil ajouté.");
                            }
                            Err(error) => status.error(error.user_message()),
                        }
                    }
                } else {
                    self.wire_start = Some(point);
                    status.info("Choisissez l'extrémité du fil. Échap annule.");
                }
            }
            crate::ui::Tool::Junction => match document.add_junction(point) {
                Ok(_) => status.success("Jonction ajoutée."),
                Err(error) => status.error(error.user_message()),
            },
            crate::ui::Tool::Label => {
                self.label_dialog = Some(LabelDialog {
                    at: point,
                    text: document
                        .net_name_at(point)
                        .unwrap_or_else(|| "NET".to_owned()),
                    error: None,
                    focus: true,
                });
            }
        }
    }

    fn paint_grid(&self, painter: &Painter, rect: Rect) {
        let palette = self.settings.palette();
        let (min, max) = self.viewport.visible_bounds(rect);
        let step = if self.viewport.scale() >= 14.0 || self.settings.small_grid {
            1
        } else {
            5
        };
        let mut count = 0_usize;

        let mut x = min.x.div_euclid(step) * step;
        while x <= max.x && count < 1000 {
            let screen_x = self.viewport.grid_to_screen(rect, GridPos::new(x, 0)).x;
            let major = x % 5 == 0;
            painter.line_segment(
                [
                    Pos2::new(screen_x, rect.top()),
                    Pos2::new(screen_x, rect.bottom()),
                ],
                Stroke::new(
                    if major { 1.0 } else { 0.6 },
                    if x == 0 {
                        palette.axis
                    } else if major {
                        palette.major_grid
                    } else {
                        palette.minor_grid
                    },
                ),
            );
            let Some(next) = x.checked_add(step) else {
                break;
            };
            x = next;
            count += 1;
        }

        count = 0;
        let mut y = min.y.div_euclid(step) * step;
        while y <= max.y && count < 1000 {
            let screen_y = self.viewport.grid_to_screen(rect, GridPos::new(0, y)).y;
            let major = y % 5 == 0;
            painter.line_segment(
                [
                    Pos2::new(rect.left(), screen_y),
                    Pos2::new(rect.right(), screen_y),
                ],
                Stroke::new(
                    if major { 1.0 } else { 0.6 },
                    if y == 0 {
                        palette.axis
                    } else if major {
                        palette.major_grid
                    } else {
                        palette.minor_grid
                    },
                ),
            );
            let Some(next) = y.checked_add(step) else {
                break;
            };
            y = next;
            count += 1;
        }
    }

    fn paint_document(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        live: &LiveValues,
    ) {
        let palette = self.settings.palette();
        let problem_wires = document.problem_wires();
        let (wire_voltages, voltage_scale) = voltage_data(document, live);
        let max_power = max_component_power(document, live);
        let overcurrent_enabled = self.settings.layers.overcurrent
            && !live.is_empty()
            && live.document_revision == document.revision();
        let wire_limit = self.settings.wire_current_limit;
        let wire_current_map = if overcurrent_enabled {
            wire_currents(document, live)
        } else {
            BTreeMap::new()
        };
        let net_flow_map = if overcurrent_enabled {
            net_flows(document, live)
        } else {
            BTreeMap::new()
        };
        let mut net_badges: BTreeMap<WireId, (f64, f64, Severity)> = BTreeMap::new();
        if overcurrent_enabled {
            for net in document.nets() {
                let mut best: Option<(WireId, f64)> = None;
                for id in &net.wires {
                    let current = wire_current_map.get(id).copied().unwrap_or(0.0).abs();
                    let replace = match best {
                        None => true,
                        Some((best_id, best_current)) => {
                            current > best_current || (current == best_current && id.0 < best_id.0)
                        }
                    };
                    if replace {
                        best = Some((*id, current));
                    }
                }
                let current = best.map(|(_, current)| current).unwrap_or(0.0).max(
                    net_flow_map
                        .get(net.name.as_str())
                        .map_or(0.0, |flow| flow.strength),
                );
                let severity = overcurrent::severity(current, Some(wire_limit));
                if severity.is_alert() {
                    if let Some((id, _)) = best {
                        net_badges.insert(id, (current, wire_limit, severity));
                    }
                }
            }
        }
        for wire in document.schematic().wires() {
            let selected = document.selection().contains(&ObjectRef::Wire(wire.id));
            let hovered = self.hover == Some(ObjectRef::Wire(wire.id));
            let net_hovered = self.hover_net.contains(&ObjectRef::Wire(wire.id));
            let problem = problem_wires.contains(&wire.id);
            let voltage = self
                .settings
                .layers
                .voltage
                .then(|| voltage_scale.zip(wire_voltages.get(&wire.id).copied()))
                .flatten()
                .map_or(palette.wire, |(scale, value)| {
                    scale.color(value, self.settings.voltage_palette)
                });
            let severity = if overcurrent_enabled {
                overcurrent::severity(
                    wire_current_map.get(&wire.id).copied().unwrap_or(0.0),
                    Some(wire_limit),
                )
            } else {
                Severity::None
            };
            let width = if selected {
                5.2
            } else if hovered || net_hovered {
                4.2
            } else {
                3.2
            };
            if problem {
                painter.line_segment(
                    [
                        self.viewport.grid_to_screen(rect, wire.a),
                        self.viewport.grid_to_screen(rect, wire.b),
                    ],
                    Stroke::new(9.0, palette.wire_error.gamma_multiply(0.18)),
                );
            }
            if (hovered || net_hovered) && !selected {
                painter.line_segment(
                    [
                        self.viewport.grid_to_screen(rect, wire.a),
                        self.viewport.grid_to_screen(rect, wire.b),
                    ],
                    Stroke::new(7.0, palette.hover.gamma_multiply(0.25)),
                );
            }
            if severity.is_alert() {
                painter.line_segment(
                    [
                        self.viewport.grid_to_screen(rect, wire.a),
                        self.viewport.grid_to_screen(rect, wire.b),
                    ],
                    Stroke::new(
                        width + 4.2,
                        overcurrent_color(palette, severity).gamma_multiply(0.34),
                    ),
                );
            }
            painter.line_segment(
                [
                    self.viewport.grid_to_screen(rect, wire.a),
                    self.viewport.grid_to_screen(rect, wire.b),
                ],
                Stroke::new(
                    width,
                    if problem {
                        palette.wire_error
                    } else if selected {
                        palette.accent
                    } else if hovered || net_hovered {
                        palette.hover
                    } else {
                        voltage
                    },
                ),
            );
            if severity.is_alert() {
                if let Some((current, limit, badge_severity)) = net_badges.get(&wire.id) {
                    if *badge_severity == Severity::Critical || self.viewport.scale() >= 6.0 {
                        self.paint_wire_badge(
                            painter,
                            rect,
                            wire,
                            *current,
                            *limit,
                            *badge_severity,
                        );
                    }
                }
            }
        }

        for symbol in document.schematic().symbols() {
            let selected = document.selection().contains(&ObjectRef::Symbol(symbol.id));
            let hovered = self.hover == Some(ObjectRef::Symbol(symbol.id));
            let net_hovered = self.hover_net.contains(&ObjectRef::Symbol(symbol.id));
            let body = if selected {
                palette.accent
            } else if hovered || net_hovered {
                palette.hover
            } else {
                palette.symbol
            };
            let colors = GlyphColors {
                body,
                pin: if selected || hovered || net_hovered {
                    body
                } else {
                    palette.pin
                },
                label: if selected {
                    palette.accent
                } else {
                    palette.label
                },
                value: if selected {
                    palette.accent
                } else {
                    palette.muted
                },
                resistor: self.settings.resistor_style,
            };
            if self.settings.layers.power
                && symbol.kind != editor_core::SymbolKind::Led
                && supports_power_glow(&symbol.kind)
            {
                self.paint_power_glow(painter, rect, document, symbol, live, max_power);
            }
            if symbol.kind == editor_core::SymbolKind::Led {
                self.paint_led_glow(painter, rect, document, symbol, live);
            }
            paint_symbol(
                painter,
                symbol,
                document.registry().get(&symbol.kind),
                self.viewport.grid_to_screen(rect, symbol.at),
                self.viewport.scale(),
                colors,
                SymbolText {
                    labels: self.settings.layers.references,
                    values: self.settings.layers.values,
                    scale: self.settings.value_scale,
                },
            );
            if selected {
                paint_symbol_outline(
                    painter,
                    document,
                    symbol,
                    &self.viewport,
                    rect,
                    palette.accent,
                    1.7,
                );
            } else if hovered || net_hovered {
                paint_symbol_outline(
                    painter,
                    document,
                    symbol,
                    &self.viewport,
                    rect,
                    palette.hover,
                    1.2,
                );
            }
            if overcurrent_enabled {
                if let Some(limit) = overcurrent::component_limit(symbol) {
                    if let Some(current) = live.symbol(document, symbol).current {
                        let severity = overcurrent::severity(current, Some(limit));
                        if severity.is_alert() {
                            let color = overcurrent_color(palette, severity);
                            paint_symbol_outline(
                                painter,
                                document,
                                symbol,
                                &self.viewport,
                                rect,
                                color,
                                2.4,
                            );
                            let center = self.viewport.grid_to_screen(rect, symbol.at)
                                + Vec2::new(
                                    self.viewport.scale() * 1.35,
                                    -self.viewport.scale() * 1.35,
                                );
                            let radius = (self.viewport.scale() * 0.3).clamp(5.0, 8.5);
                            paint_alert_triangle(
                                painter,
                                center,
                                radius,
                                color,
                                palette.background,
                            );
                        }
                    }
                }
            }
        }

        for junction in document.schematic().junctions() {
            let selected = document
                .selection()
                .contains(&ObjectRef::Junction(junction.id));
            let hovered = self.hover == Some(ObjectRef::Junction(junction.id));
            let net_hovered = self.hover_net.contains(&ObjectRef::Junction(junction.id));
            let radius = (self.viewport.scale() * 0.28).clamp(3.0, 7.0)
                * if selected || hovered || net_hovered {
                    1.25
                } else {
                    1.0
                };
            let position = self.viewport.grid_to_screen(rect, junction.at);
            let color = if selected {
                palette.accent
            } else if hovered || net_hovered {
                palette.hover
            } else {
                palette.wire
            };
            painter.circle_filled(position, radius, color);
            painter.circle_stroke(position, radius, Stroke::new(1.0, palette.background));
        }

        if !self.settings.layers.net_labels {
            return;
        }
        for label in document.schematic().labels() {
            let selected = document.selection().contains(&ObjectRef::Label(label.id));
            let hovered = self.hover == Some(ObjectRef::Label(label.id));
            let net_hovered = self.hover_net.contains(&ObjectRef::Label(label.id));
            let position = self.viewport.grid_to_screen(rect, label.at);
            let color = if selected {
                palette.accent
            } else if hovered || net_hovered {
                palette.hover
            } else {
                palette.label
            };
            let font = FontId::proportional(13.0);
            let galley = painter.layout_no_wrap(label.text.clone(), font, color);
            let text_pos = position + Vec2::new(7.0, -6.0);
            let text_rect = Align2::LEFT_BOTTOM.anchor_size(text_pos, galley.size());
            if selected {
                painter.rect_filled(text_rect.expand(3.0), 3.0, palette.selection_fill);
                painter.rect_stroke(
                    text_rect.expand(3.0),
                    3.0,
                    Stroke::new(1.0, palette.accent),
                    StrokeKind::Outside,
                );
            }
            painter.circle_filled(position, 2.8, color);
            painter.galley(text_rect.min, galley, color);
        }
    }

    fn paint_wire_badge(
        &self,
        painter: &Painter,
        rect: Rect,
        wire: &editor_core::Wire,
        current: f64,
        limit: f64,
        severity: Severity,
    ) {
        let palette = self.settings.palette();
        let color = overcurrent_color(palette, severity);
        let start = self.viewport.grid_to_screen(rect, wire.a);
        let end = self.viewport.grid_to_screen(rect, wire.b);
        let anchor = start + (end - start) * 0.5 + Vec2::new(0.0, -8.0);
        let text = format!(
            "{} / {}",
            format_si_precision(current, Unit::Ampere, 3),
            format_si_precision(limit, Unit::Ampere, 3)
        );
        let galley = painter.layout_no_wrap(text, FontId::proportional(10.0), color);
        let size = Vec2::new(galley.size().x + 22.0, galley.size().y + 6.0);
        let badge = Align2::CENTER_BOTTOM.anchor_size(anchor, size);
        painter.rect_filled(badge, 4.0, palette.panel_badge);
        painter.rect_stroke(
            badge,
            4.0,
            Stroke::new(1.0, color.gamma_multiply(0.85)),
            StrokeKind::Inside,
        );
        paint_alert_triangle(
            painter,
            Pos2::new(badge.left() + 9.0, badge.center().y),
            4.5,
            color,
            palette.background,
        );
        painter.galley(
            Pos2::new(badge.left() + 17.0, badge.top() + 3.0),
            galley,
            color,
        );
    }

    fn paint_simulation_badge(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        live: &LiveValues,
    ) {
        if live.is_empty() {
            return;
        }
        let stale = live.document_revision != document.revision();
        let time = live.time.map_or_else(
            || "échantillon".to_owned(),
            |value| format_si_precision(value, Unit::Second, 4),
        );
        let text = format!(
            "t = {time} · {}",
            if stale { "antérieur" } else { "à jour" }
        );
        let palette = self.settings.palette();
        let color = if stale { palette.accent } else { palette.hover };
        let galley = painter.layout_no_wrap(text, FontId::proportional(11.0), color);
        let anchor = rect.right_top() + Vec2::new(-12.0, 12.0);
        let badge = Align2::RIGHT_TOP
            .anchor_size(anchor, galley.size())
            .expand2(Vec2::new(8.0, 4.0));
        painter.rect_filled(badge, 5.0, palette.live_badge);
        painter.rect_stroke(
            badge,
            5.0,
            Stroke::new(1.0, color.gamma_multiply(0.6)),
            StrokeKind::Inside,
        );
        painter.galley(badge.min + Vec2::new(8.0, 4.0), galley, color);
    }

    fn paint_power_glow(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        symbol: &Symbol,
        live: &LiveValues,
        max_power: f64,
    ) {
        let observed = live.symbol(document, symbol);
        let Some(power) = observed.power.filter(|power| *power > 0.0) else {
            return;
        };
        if max_power <= f64::MIN_POSITIVE || self.power_brightness <= 0.0 {
            return;
        }
        let intensity = (power / max_power).sqrt().clamp(0.0, 1.0) as f32;
        let center = self.viewport.grid_to_screen(rect, symbol.at);
        let radius = (self.viewport.scale() * 1.2).clamp(4.0, 22.0);
        painter.circle_filled(
            center,
            radius,
            Color32::from_rgba_premultiplied(
                255,
                120,
                48,
                (45.0 * intensity * self.power_brightness) as u8,
            ),
        );
        painter.circle_filled(
            center,
            radius * 0.55,
            Color32::from_rgba_premultiplied(
                255,
                190,
                90,
                (65.0 * intensity * self.power_brightness) as u8,
            ),
        );
    }

    fn paint_scope_markers(&self, painter: &Painter, rect: Rect, document: &Document) {
        if self.scope_signals.is_empty() {
            return;
        }
        for signal in &self.scope_signals {
            let color = crate::ui::scopes::signal_color(signal);
            match signal {
                SignalId::Voltage(name) | SignalId::NetCurrent(name) => {
                    let Some(net) = document
                        .nets()
                        .iter()
                        .find(|net| net.name.as_str() == name.as_str())
                    else {
                        continue;
                    };
                    let Some(wire) = net.wires.first().and_then(|id| document.wire(*id)) else {
                        continue;
                    };
                    let position = self.viewport.grid_to_screen(
                        rect,
                        GridPos::new(
                            (wire.a.x + wire.b.x).div_euclid(2),
                            (wire.a.y + wire.b.y).div_euclid(2),
                        ),
                    );
                    painter.circle_filled(position, 3.0, color);
                    painter.circle_stroke(position, 6.0, Stroke::new(1.2, color));
                }
                SignalId::ComponentVoltage(label) | SignalId::Current(label) => {
                    let Some(symbol) = document
                        .schematic()
                        .symbols()
                        .find(|symbol| symbol.label == *label)
                    else {
                        continue;
                    };
                    let position = self.viewport.grid_to_screen(rect, symbol.at);
                    painter.circle_stroke(
                        position,
                        (self.viewport.scale() * 1.4).clamp(9.0, 22.0),
                        Stroke::new(1.2, color),
                    );
                }
            }
        }
    }

    fn paint_voltage_legend(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        live: &LiveValues,
    ) {
        if !self.settings.layers.voltage || live.is_empty() || rect.width() < 520.0 {
            return;
        }
        let (_, scale) = voltage_data(document, live);
        let Some(scale) = scale.filter(VoltageScale::is_visible) else {
            return;
        };
        let palette = self.settings.palette();
        let origin = rect.left_top() + Vec2::new(12.0, 66.0);
        let bar = Rect::from_min_size(origin + Vec2::new(8.0, 18.0), Vec2::new(180.0, 10.0));
        let panel = Rect::from_min_size(origin, Vec2::new(196.0, 46.0));
        painter.rect_filled(panel, 5.0, palette.panel_badge);
        painter.rect_stroke(
            panel,
            5.0,
            Stroke::new(1.0, palette.major_grid),
            StrokeKind::Inside,
        );
        let steps = 60;
        for index in 0..steps {
            let t = index as f64 / (steps - 1) as f64 * 2.0 - 1.0;
            let color = crate::ui::theme::voltage_color(t, self.settings.voltage_palette);
            let x = bar.left() + bar.width() * index as f32 / steps as f32;
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(x, bar.top()),
                    Vec2::new(bar.width() / steps as f32 + 0.5, bar.height()),
                ),
                0.0,
                color,
            );
        }
        painter.text(
            panel.left_top() + Vec2::new(8.0, 4.0),
            Align2::LEFT_TOP,
            "Tension",
            FontId::proportional(11.0),
            palette.muted,
        );
        painter.text(
            bar.left_bottom() + Vec2::new(0.0, 2.0),
            Align2::LEFT_TOP,
            format_si_precision(-scale.max_abs(), Unit::Volt, 2),
            FontId::proportional(10.0),
            palette.muted,
        );
        painter.text(
            bar.center_bottom() + Vec2::new(0.0, 2.0),
            Align2::CENTER_TOP,
            "0 V",
            FontId::proportional(10.0),
            palette.muted,
        );
        painter.text(
            bar.right_bottom() + Vec2::new(0.0, 2.0),
            Align2::RIGHT_TOP,
            format_si_precision(scale.max_abs(), Unit::Volt, 2),
            FontId::proportional(10.0),
            palette.muted,
        );
    }

    fn paint_hover_readout(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        live: &LiveValues,
        pointer: Pos2,
    ) {
        let Some(point) = self.last_pointer_grid else {
            return;
        };
        let mut title = String::new();
        let mut lines = Vec::new();
        let palette = self.settings.palette();
        if let Some(key) = document.pin_at(point) {
            if let Some(symbol) = document.symbol(key.symbol) {
                if let Some(definition) = document.registry().get(&symbol.kind) {
                    let pin_name = definition
                        .pins
                        .get(usize::from(key.pin))
                        .map_or("—", |pin| pin.name.as_str());
                    title = format!("Broche {pin_name} · {}", symbol.label);
                    let observed = live.symbol(document, symbol);
                    if let Some(pin) = observed.pins.get(usize::from(key.pin)) {
                        if let Some(net) = &pin.net {
                            lines.push(format!("Net : {net}"));
                        }
                        if let Some(voltage) = pin.voltage {
                            lines.push(format!(
                                "V : {}",
                                format_si_precision(voltage, Unit::Volt, 4)
                            ));
                        }
                        if let Some(net) = &pin.net {
                            if let Some(current) = net_current(document, live, net) {
                                lines.push(current_line(
                                    current,
                                    Some(self.settings.wire_current_limit),
                                ));
                            }
                        }
                    }
                    if live.document_revision != document.revision() {
                        lines.push("Résultat antérieur".to_owned());
                    }
                }
            }
        }
        if title.is_empty() {
            if let Some(object) = document.object_at(point) {
                match object {
                    ObjectRef::Symbol(id) => {
                        if let Some(symbol) = document.symbol(id) {
                            title =
                                format!("{} · {}", symbol_kind_name(&symbol.kind), symbol.label);
                            for (key, value) in symbol.params.iter().take(4) {
                                lines.push(format!(
                                    "{key} = {}",
                                    format_hover_parameter(&symbol.kind, key, value)
                                ));
                            }
                            let observed = live.symbol(document, symbol);
                            for pin in &observed.pins {
                                let Some(net) = &pin.net else {
                                    continue;
                                };
                                let voltage = pin.voltage.map_or_else(
                                    || "—".to_owned(),
                                    |value| format_si_precision(value, Unit::Volt, 3),
                                );
                                lines.push(format!("{} · {net} : {voltage}", pin.name));
                            }
                            if let (Some(first), Some(second)) =
                                (observed.pins.first(), observed.pins.get(1))
                            {
                                if let (Some(a), Some(b)) = (first.voltage, second.voltage) {
                                    lines.push(format!(
                                        "U : {}",
                                        format_si_precision(a - b, Unit::Volt, 3)
                                    ));
                                }
                            }
                            if let Some(current) = observed.current {
                                lines.push(current_line(
                                    current,
                                    overcurrent::component_limit(symbol),
                                ));
                            }
                            if let Some(power) = observed.power {
                                lines.push(format!(
                                    "P : {}",
                                    format_si_precision(power, Unit::Watt, 3)
                                ));
                            }
                            if live.document_revision != document.revision() {
                                lines.push("Résultat antérieur".to_owned());
                            }
                        }
                    }
                    ObjectRef::Wire(id) => {
                        if let Some(net) = document.net_name_at(point) {
                            title = format!("Net {net}");
                            if let Some(voltage) = live.voltage(&net) {
                                lines.push(format!(
                                    "V : {}",
                                    format_si_precision(voltage, Unit::Volt, 4)
                                ));
                            }
                            if let Some(current) = wire_current(document, live, id)
                                .or_else(|| net_current(document, live, &net))
                            {
                                lines.push(current_line(
                                    current,
                                    Some(self.settings.wire_current_limit),
                                ));
                            }
                            if live.document_revision != document.revision() {
                                lines.push("Résultat antérieur".to_owned());
                            }
                        }
                    }
                    ObjectRef::Junction(_) | ObjectRef::Label(_) => {
                        if let Some(net) = document.net_name_at(point) {
                            title = format!("Net {net}");
                            if let Some(voltage) = live.voltage(&net) {
                                lines.push(format!(
                                    "V : {}",
                                    format_si_precision(voltage, Unit::Volt, 4)
                                ));
                            }
                            if let Some(current) = net_current(document, live, &net) {
                                lines.push(current_line(
                                    current,
                                    Some(self.settings.wire_current_limit),
                                ));
                            }
                            if live.document_revision != document.revision() {
                                lines.push("Résultat antérieur".to_owned());
                            }
                        }
                    }
                }
            }
        }
        if title.is_empty() {
            return;
        }
        let width = 250.0_f32.min(rect.width() - 16.0).max(120.0);
        let height = 20.0 + lines.len() as f32 * 14.0;
        let mut origin = pointer + Vec2::new(16.0, 18.0);
        if origin.x + width > rect.right() {
            origin.x = pointer.x - width - 12.0;
        }
        if origin.y + height > rect.bottom() {
            origin.y = pointer.y - height - 12.0;
        }
        origin.x = origin.x.max(rect.left() + 4.0);
        origin.y = origin.y.max(rect.top() + 4.0);
        let panel = Rect::from_min_size(origin, Vec2::new(width, height));
        painter.rect_filled(panel, 5.0, palette.panel_badge);
        painter.rect_stroke(
            panel,
            5.0,
            Stroke::new(1.0, palette.major_grid),
            StrokeKind::Inside,
        );
        painter.text(
            panel.left_top() + Vec2::new(8.0, 5.0),
            Align2::LEFT_TOP,
            title,
            FontId::proportional(11.5),
            palette.accent,
        );
        for (index, line) in lines.iter().enumerate() {
            painter.text(
                panel.left_top() + Vec2::new(8.0, 24.0 + index as f32 * 14.0),
                Align2::LEFT_TOP,
                line,
                FontId::proportional(11.0),
                palette.muted,
            );
        }
    }

    fn paint_parameter_controls(&self, painter: &Painter, rect: Rect, document: &Document) {
        if self.controls.is_empty() {
            return;
        }
        let palette = self.settings.palette();
        for control in self.controls.items() {
            let Some(symbol) = document.symbol(control.symbol) else {
                continue;
            };
            let track = self.control_rect(rect, symbol);
            let range = control.range();
            let fraction = control_fraction(control.draft, range);
            let unit = parameter_unit(&symbol.kind, &control.key);
            let label = format!(
                "{} : {}",
                control.key,
                format_parameter(control.draft, unit)
            );
            painter.rect_filled(track.expand(4.0), 4.0, palette.live_badge);
            painter.rect_stroke(
                track.expand(4.0),
                4.0,
                Stroke::new(1.0, palette.major_grid),
                StrokeKind::Inside,
            );
            painter.text(
                track.left_top() + Vec2::new(0.0, -12.0),
                Align2::LEFT_BOTTOM,
                label,
                FontId::proportional(10.0),
                palette.muted,
            );
            painter.line_segment(
                [
                    Pos2::new(track.left() + 7.0, track.center().y),
                    Pos2::new(track.right() - 7.0, track.center().y),
                ],
                Stroke::new(2.0, palette.major_grid),
            );
            let knob_x = track.left() + 7.0 + (track.width() - 14.0) * fraction as f32;
            painter.circle_filled(Pos2::new(knob_x, track.center().y), 4.5, palette.accent);
        }
    }

    fn handle_parameter_controls(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        rect: Rect,
        document: &mut Document,
        status: &mut Status,
    ) -> bool {
        if self.controls.is_empty() || !response.hovered() {
            return false;
        }
        let Some(pointer) = ui.input(|input| input.pointer.hover_pos()) else {
            return false;
        };
        let (pressed, released, down, scroll) = ui.input(|input| {
            (
                input.pointer.primary_pressed(),
                input.pointer.primary_released(),
                input.pointer.primary_down(),
                input.smooth_scroll_delta.y,
            )
        });
        let tracks = self
            .controls
            .items()
            .iter()
            .map(|control| {
                document
                    .symbol(control.symbol)
                    .map(|symbol| self.control_rect(rect, symbol))
            })
            .collect::<Vec<_>>();
        let mut handled = false;
        for (control, track) in self.controls.items_mut().iter_mut().zip(tracks) {
            let Some(track) = track else {
                continue;
            };
            let hit = track.expand(5.0).contains(pointer);
            if pressed && hit {
                control.dragging = true;
            }
            if control.dragging {
                handled = true;
                let fraction =
                    ((pointer.x - (track.left() + 7.0)) / (track.width() - 14.0)).clamp(0.0, 1.0);
                let range = control.range();
                control.draft = control_value(f64::from(fraction), range);
                if released || !down {
                    let Some(symbol) = document.symbol(control.symbol).cloned() else {
                        control.dragging = false;
                        continue;
                    };
                    let mut changed = symbol;
                    changed.params.insert(
                        control.key.clone(),
                        sim_core::netlist::ParamValue::Number(control.draft),
                    );
                    match document.update_symbol(changed) {
                        Ok(()) => {
                            control.value = control.draft;
                            status.success(format!("{} mis à jour.", control.key));
                        }
                        Err(error) => status.error(error.user_message()),
                    }
                    control.dragging = false;
                }
            } else if hit && scroll.abs() > 0.1 {
                handled = true;
                let range = control.range();
                let step = (range.1 - range.0) * 0.02;
                control.draft =
                    (control.draft + step * f64::from(scroll.signum())).clamp(range.0, range.1);
                if let Some(symbol) = document.symbol(control.symbol).cloned() {
                    let mut changed = symbol;
                    changed.params.insert(
                        control.key.clone(),
                        sim_core::netlist::ParamValue::Number(control.draft),
                    );
                    match document.update_symbol(changed) {
                        Ok(()) => control.value = control.draft,
                        Err(error) => status.error(error.user_message()),
                    }
                }
            }
            if hit {
                handled = true;
            }
        }
        handled
    }

    fn control_rect(&self, rect: Rect, symbol: &Symbol) -> Rect {
        let center = self.viewport.grid_to_screen(rect, symbol.at);
        Rect::from_min_size(
            center + Vec2::new(-70.0, self.viewport.scale() * 3.5),
            Vec2::new(140.0, 10.0),
        )
    }

    fn paint_led_glow(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        symbol: &Symbol,
        live: &LiveValues,
    ) {
        let observed = live.symbol(document, symbol);
        let voltage = match (observed.pins.first(), observed.pins.get(1)) {
            (Some(first), Some(second)) => first.voltage.zip(second.voltage),
            _ => None,
        }
        .map(|(a, b)| a - b);
        let Some(voltage) = voltage else {
            return;
        };
        let brightness = ((voltage - 0.8) / 1.2).clamp(0.0, 1.0) as f32;
        if brightness <= 0.0 {
            return;
        }
        let center = self.viewport.grid_to_screen(rect, symbol.at);
        let zoom_fade = (self.viewport.scale() / 20.0).clamp(0.3, 1.0);
        let (red, green, blue) = led_glow_color(symbol);
        painter.circle_filled(
            center,
            (self.viewport.scale() * 1.5).clamp(3.0, 26.0),
            Color32::from_rgba_premultiplied(
                red,
                green,
                blue,
                (70.0 * brightness * self.power_brightness * zoom_fade) as u8,
            ),
        );
        painter.circle_filled(
            center,
            (self.viewport.scale() * 0.85).clamp(2.0, 15.0),
            Color32::from_rgba_premultiplied(
                red.saturating_add(50),
                green.saturating_add(50),
                blue.saturating_add(50),
                (120.0 * brightness * self.power_brightness * zoom_fade) as u8,
            ),
        );
    }

    fn paint_current_flow(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        live: &LiveValues,
        now: f64,
    ) {
        if live.is_empty() || !self.settings.layers.current || self.viewport.scale() < 8.0 {
            return;
        }
        let Some(extraction) = document.extraction() else {
            return;
        };
        let flows = net_flows(document, live);
        if flows.is_empty() {
            return;
        }
        let palette = self.settings.palette();
        for net in &extraction.nets {
            let Some(flow) = flows.get(net.name.as_str()) else {
                continue;
            };
            let strength = (flow.strength * 1_000.0).ln_1p().clamp(0.0, 6.0);
            let spacing = ((1.8 - strength * 0.18).clamp(0.55, 1.8)) as f32;
            let speed = (2.0 + strength * 0.6) * self.animation_speed;
            let phase = (now * speed).rem_euclid(f64::from(spacing)) as f32;
            let directions = wire_directions(document, net, flow.source);
            let reverse = self.settings.current_direction == CurrentDirection::Electrons;
            for id in &net.wires {
                let Some(wire) = document.wire(*id) else {
                    continue;
                };
                let forward = (directions.get(id).copied().unwrap_or(true) == flow.away) != reverse;
                let (start, end) = if forward {
                    (wire.a, wire.b)
                } else {
                    (wire.b, wire.a)
                };
                let length = start.manhattan_distance(end) as f32;
                if length <= f32::EPSILON {
                    continue;
                }
                let start_screen = self.viewport.grid_to_screen(rect, start);
                let end_screen = self.viewport.grid_to_screen(rect, end);
                let mut traveled = phase;
                while traveled < length {
                    let ratio = traveled / length;
                    let position = start_screen + (end_screen - start_screen) * ratio;
                    painter.circle_filled(position, 3.8, palette.flow_halo);
                    painter.circle_filled(position, 2.4, palette.flow_color);
                    traveled += spacing;
                }
            }
        }
    }

    fn paint_wire_handles(&self, painter: &Painter, rect: Rect, document: &Document) {
        let palette = self.settings.palette();
        for (wire, _, point) in document.free_wire_endpoints() {
            let relevant = document.selection().contains(&ObjectRef::Wire(wire))
                || self.hover == Some(ObjectRef::Wire(wire));
            if !relevant {
                continue;
            }
            let screen = self.viewport.grid_to_screen(rect, point);
            painter.circle_filled(screen, 4.5, palette.background);
            painter.circle_stroke(screen, 4.5, Stroke::new(1.8, palette.hover));
        }
    }

    fn paint_hover(&self, painter: &Painter, rect: Rect, document: &Document) {
        let Some(object) = self.hover else {
            return;
        };
        if document.selection().contains(&object) {
            return;
        }
        let palette = self.settings.palette();
        match object {
            ObjectRef::Label(id) => {
                if let Some(label) = document.label(id) {
                    painter.circle_stroke(
                        self.viewport.grid_to_screen(rect, label.at),
                        8.0,
                        Stroke::new(1.2, palette.hover),
                    );
                }
            }
            ObjectRef::Junction(id) => {
                if let Some(junction) = document.junction(id) {
                    painter.circle_stroke(
                        self.viewport.grid_to_screen(rect, junction.at),
                        9.0,
                        Stroke::new(1.2, palette.hover),
                    );
                }
            }
            ObjectRef::Wire(_) | ObjectRef::Symbol(_) => {}
        }
    }

    fn paint_previews(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        tool: &crate::ui::Tool,
        hovered_point: Option<GridPos>,
        shift: bool,
    ) {
        let palette = self.settings.palette();
        match &self.drag {
            Some(DragState::Objects { start, current }) => {
                let delta = GridPos::new(
                    current.x.saturating_sub(start.x),
                    current.y.saturating_sub(start.y),
                );
                if delta != GridPos::default() {
                    let mut ghost_painter = painter.clone();
                    ghost_painter.set_opacity(0.45);
                    for object in document.selection() {
                        self.paint_ghost_object(&ghost_painter, rect, document, *object, delta);
                    }
                    self.paint_alignment_guides(painter, rect, document, delta);
                }
            }
            Some(DragState::Segment {
                wire,
                start,
                current,
            }) => {
                if let Some(segment) = document.wire(*wire) {
                    let delta = drag_axis_delta(*start, *current);
                    if delta != GridPos::default() {
                        if let (Some(a), Some(b)) =
                            (segment.a.checked_add(delta), segment.b.checked_add(delta))
                        {
                            painter.line_segment(
                                [
                                    self.viewport.grid_to_screen(rect, a),
                                    self.viewport.grid_to_screen(rect, b),
                                ],
                                Stroke::new(3.0, palette.accent.gamma_multiply(0.6)),
                            );
                        }
                    }
                }
            }
            Some(DragState::WireEndpoint {
                wire, end, current, ..
            }) => {
                if let Some(segment) = document.wire(*wire) {
                    let path = document.wire_endpoint_extension(segment, *end, *current);
                    if path.len() >= 2 {
                        let points = path
                            .iter()
                            .map(|point| self.viewport.grid_to_screen(rect, *point))
                            .collect::<Vec<_>>();
                        painter.line(
                            points,
                            Stroke::new(3.2, palette.accent.gamma_multiply(0.55)),
                        );
                    }
                    painter.circle_stroke(
                        self.viewport.grid_to_screen(rect, *current),
                        6.0,
                        Stroke::new(1.6, palette.accent),
                    );
                }
            }
            Some(DragState::WireFromPin {
                start,
                current,
                prefer_horizontal,
            }) => {
                let target = self.snap_target.map_or(*current, |snap| snap.at);
                if target != *start {
                    let path = document.route_path(*start, target, *prefer_horizontal);
                    for pair in path.windows(2) {
                        painter.line_segment(
                            [
                                self.viewport.grid_to_screen(rect, pair[0]),
                                self.viewport.grid_to_screen(rect, pair[1]),
                            ],
                            Stroke::new(3.0, palette.accent.gamma_multiply(0.85)),
                        );
                    }
                    painter.circle_stroke(
                        self.viewport.grid_to_screen(rect, target),
                        7.0,
                        Stroke::new(1.5, palette.hover),
                    );
                }
            }
            Some(DragState::Marquee { start, current, .. }) => {
                let min = GridPos::new(start.x.min(current.x), start.y.min(current.y));
                let max = GridPos::new(start.x.max(current.x), start.y.max(current.y));
                let rectangle = Rect::from_two_pos(
                    self.viewport.grid_to_screen(rect, min),
                    self.viewport.grid_to_screen(rect, max),
                );
                let color = if start.x <= current.x {
                    palette.accent
                } else {
                    palette.hover
                };
                painter.rect_filled(rectangle, 0.0, color.gamma_multiply(0.08));
                painter.rect_stroke(rectangle, 0.0, Stroke::new(1.2, color), StrokeKind::Inside);
            }
            None => {}
        }

        if let Some(start) = self.wire_start {
            if let Some(end) = hovered_point {
                let prefer_horizontal = if shift {
                    !self.wire_prefer_horizontal
                } else {
                    self.wire_prefer_horizontal
                };
                let path = document.route_path(start, end, prefer_horizontal);
                let color = palette.accent.gamma_multiply(0.9);
                for pair in path.windows(2) {
                    painter.line_segment(
                        [
                            self.viewport.grid_to_screen(rect, pair[0]),
                            self.viewport.grid_to_screen(rect, pair[1]),
                        ],
                        Stroke::new(2.0, color),
                    );
                }
                painter.circle_filled(self.viewport.grid_to_screen(rect, start), 4.0, color);
                painter.circle_stroke(
                    self.viewport.grid_to_screen(rect, end),
                    5.0,
                    Stroke::new(1.5, color),
                );
            }
        }

        if let crate::ui::Tool::Place(kind) = tool {
            if let Some(point) = hovered_point {
                if let Some(definition) = document.registry().get(kind) {
                    let origin = self.placement_origin(document, kind, point);
                    let preview = Symbol::from_def(
                        SymbolId(0),
                        definition,
                        origin,
                        "",
                        self.placement_orientation,
                        self.placement_mirrored,
                    );
                    let mut ghost_painter = painter.clone();
                    ghost_painter.set_opacity(0.55);
                    paint_symbol(
                        &ghost_painter,
                        &preview,
                        Some(definition),
                        self.viewport.grid_to_screen(rect, origin),
                        self.viewport.scale(),
                        GlyphColors {
                            body: palette.symbol,
                            pin: palette.pin,
                            label: palette.label,
                            value: palette.muted,
                            resistor: self.settings.resistor_style,
                        },
                        SymbolText::NONE,
                    );
                    for pin in &definition.pins {
                        if let Some(position) = preview.pin_position_of(pin) {
                            painter.circle_stroke(
                                self.viewport.grid_to_screen(rect, position),
                                (self.viewport.scale() * 0.4).clamp(4.0, 8.0),
                                Stroke::new(1.0, palette.accent.gamma_multiply(0.7)),
                            );
                        }
                    }
                }
                let insert_preview = match self.snap_target {
                    Some(SnapTarget {
                        at,
                        kind: SnapKind::Wire(id),
                    }) => document
                        .wire(id)
                        .is_some_and(|wire| at != wire.a && at != wire.b),
                    _ => false,
                };
                if insert_preview
                    && document
                        .registry()
                        .get(kind)
                        .is_some_and(|definition| definition.pins.len() == 2)
                {
                    painter.circle_filled(
                        self.viewport.grid_to_screen(rect, point),
                        4.0,
                        palette.accent,
                    );
                }
            }
        } else if let crate::ui::Tool::Junction = tool {
            if let Some(point) = hovered_point {
                painter.circle_stroke(
                    self.viewport.grid_to_screen(rect, point),
                    7.0,
                    Stroke::new(1.3, palette.accent),
                );
            }
        } else if let crate::ui::Tool::Label = tool {
            if let Some(point) = hovered_point {
                let screen = self.viewport.grid_to_screen(rect, point);
                painter.circle_stroke(screen, 7.0, Stroke::new(1.3, palette.accent));
                painter.text(
                    screen + Vec2::new(8.0, -6.0),
                    Align2::LEFT_BOTTOM,
                    "NET",
                    FontId::proportional(12.0),
                    palette.accent.gamma_multiply(0.8),
                );
            }
        }
    }

    fn paint_ghost_object(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        object: ObjectRef,
        delta: GridPos,
    ) {
        let palette = self.settings.palette();
        match object {
            ObjectRef::Symbol(id) => {
                if let Some(symbol) = document.symbol(id) {
                    if let Some(at) = symbol.at.checked_add(delta) {
                        paint_symbol(
                            painter,
                            symbol,
                            document.registry().get(&symbol.kind),
                            self.viewport.grid_to_screen(rect, at),
                            self.viewport.scale(),
                            GlyphColors {
                                body: palette.accent,
                                pin: palette.accent,
                                label: palette.accent,
                                value: palette.accent,
                                resistor: self.settings.resistor_style,
                            },
                            SymbolText::NONE,
                        );
                    }
                }
            }
            ObjectRef::Wire(id) => {
                if let Some(wire) = document.wire(id) {
                    if let (Some(a), Some(b)) =
                        (wire.a.checked_add(delta), wire.b.checked_add(delta))
                    {
                        painter.line_segment(
                            [
                                self.viewport.grid_to_screen(rect, a),
                                self.viewport.grid_to_screen(rect, b),
                            ],
                            Stroke::new(3.0, palette.accent.gamma_multiply(0.45)),
                        );
                    }
                }
            }
            ObjectRef::Junction(id) => {
                if let Some(junction) = document.junction(id) {
                    if let Some(at) = junction.at.checked_add(delta) {
                        painter.circle_filled(
                            self.viewport.grid_to_screen(rect, at),
                            4.5,
                            palette.accent.gamma_multiply(0.5),
                        );
                    }
                }
            }
            ObjectRef::Label(id) => {
                if let Some(label) = document.label(id) {
                    if let Some(at) = label.at.checked_add(delta) {
                        painter.text(
                            self.viewport.grid_to_screen(rect, at),
                            Align2::LEFT_BOTTOM,
                            &label.text,
                            FontId::proportional(13.0),
                            palette.accent.gamma_multiply(0.6),
                        );
                    }
                }
            }
        }
    }

    fn paint_clipboard_ghost(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        at: GridPos,
    ) {
        let palette = self.settings.palette();
        let Some(clipboard) = document.clipboard() else {
            return;
        };
        let Some(delta) = at.checked_sub(clipboard.anchor()) else {
            return;
        };
        let mut ghost = painter.clone();
        ghost.set_opacity(0.5);
        for item in clipboard.items() {
            match item {
                ClipboardItem::Symbol(symbol) => {
                    if let Some(position) = symbol.at.checked_add(delta) {
                        let mut preview = symbol.clone();
                        preview.at = position;
                        paint_symbol(
                            &ghost,
                            &preview,
                            document.registry().get(&preview.kind),
                            self.viewport.grid_to_screen(rect, position),
                            self.viewport.scale(),
                            GlyphColors {
                                body: palette.accent,
                                pin: palette.accent,
                                label: palette.accent,
                                value: palette.accent,
                                resistor: self.settings.resistor_style,
                            },
                            SymbolText::NONE,
                        );
                    }
                }
                ClipboardItem::Wire(wire) => {
                    if let (Some(a), Some(b)) =
                        (wire.a.checked_add(delta), wire.b.checked_add(delta))
                    {
                        ghost.line_segment(
                            [
                                self.viewport.grid_to_screen(rect, a),
                                self.viewport.grid_to_screen(rect, b),
                            ],
                            Stroke::new(2.5, palette.accent.gamma_multiply(0.7)),
                        );
                    }
                }
                ClipboardItem::Junction(junction) => {
                    if let Some(position) = junction.at.checked_add(delta) {
                        ghost.circle_filled(
                            self.viewport.grid_to_screen(rect, position),
                            4.0,
                            palette.accent.gamma_multiply(0.7),
                        );
                    }
                }
                ClipboardItem::Label(label) => {
                    if let Some(position) = label.at.checked_add(delta) {
                        ghost.text(
                            self.viewport.grid_to_screen(rect, position),
                            Align2::LEFT_BOTTOM,
                            &label.text,
                            FontId::proportional(13.0),
                            palette.accent,
                        );
                    }
                }
            }
        }
    }

    fn paint_alignment_guides(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        delta: GridPos,
    ) {
        let moving = document
            .selection()
            .iter()
            .filter_map(|object| match object {
                ObjectRef::Symbol(id) => document.symbol(*id),
                _ => None,
            })
            .filter_map(|symbol| symbol.at.checked_add(delta))
            .collect::<Vec<_>>();
        let fixed = document
            .schematic()
            .symbols()
            .filter(|symbol| !document.selection().contains(&ObjectRef::Symbol(symbol.id)))
            .map(|symbol| symbol.at)
            .collect::<Vec<_>>();
        let mut vertical = BTreeSet::new();
        let mut horizontal = BTreeSet::new();
        for a in &moving {
            for b in &fixed {
                if a.x == b.x {
                    vertical.insert(a.x);
                }
                if a.y == b.y {
                    horizontal.insert(a.y);
                }
            }
        }
        let color = self.settings.palette().accent.gamma_multiply(0.22);
        for x in vertical.into_iter().take(8) {
            let screen = self.viewport.grid_to_screen(rect, GridPos::new(x, 0));
            painter.line_segment(
                [
                    Pos2::new(screen.x, rect.top()),
                    Pos2::new(screen.x, rect.bottom()),
                ],
                Stroke::new(1.0, color),
            );
        }
        for y in horizontal.into_iter().take(8) {
            let screen = self.viewport.grid_to_screen(rect, GridPos::new(0, y));
            painter.line_segment(
                [
                    Pos2::new(rect.left(), screen.y),
                    Pos2::new(rect.right(), screen.y),
                ],
                Stroke::new(1.0, color),
            );
        }
    }

    fn paint_snap(&self, painter: &Painter, rect: Rect) {
        let Some(target) = self.snap_target else {
            return;
        };
        let position = self.viewport.grid_to_screen(rect, target.at);
        painter.circle_stroke(
            position,
            (self.viewport.scale() * 0.45).clamp(5.0, 9.0),
            Stroke::new(1.4, self.settings.palette().hover),
        );
    }

    fn minimap_geometry(
        &self,
        rect: Rect,
        document: &Document,
    ) -> Option<(Rect, MinimapTransform)> {
        if !self.minimap_visible || rect.width() < 300.0 || rect.height() < 220.0 {
            return None;
        }
        let content = document.content_bounds()?;
        let area = Rect::from_min_size(
            Pos2::new(
                rect.right() - MINIMAP_WIDTH - MINIMAP_MARGIN,
                rect.bottom() - MINIMAP_HEIGHT - MINIMAP_MARGIN,
            ),
            Vec2::new(MINIMAP_WIDTH, MINIMAP_HEIGHT),
        );
        let transform = MinimapTransform::new(area, content)?;
        Some((area, transform))
    }

    fn paint_probe_values(
        &self,
        painter: &Painter,
        rect: Rect,
        document: &Document,
        probes: &[ProbeReading],
        visible_probes: &BTreeSet<SymbolId>,
    ) {
        for probe in probes {
            if !visible_probes.contains(&probe.id) {
                continue;
            }
            let Some(symbol) = document.symbol(probe.id) else {
                continue;
            };
            let palette = self.settings.palette();
            let text = probe.value.map_or_else(
                || "—".to_owned(),
                |value| format_si_precision(value, probe.unit.unwrap_or(Unit::Volt), 3),
            );
            let color = match probe.kind {
                editor_core::SymbolKind::Voltmeter => palette.accent,
                editor_core::SymbolKind::Ammeter => palette.hover,
                editor_core::SymbolKind::Ohmmeter => Color32::from_rgb(112, 210, 155),
                _ => palette.muted,
            };
            let galley = painter.layout_no_wrap(text, FontId::monospace(10.5), color);
            let anchor = self.viewport.grid_to_screen(rect, symbol.at) + Vec2::new(0.0, 16.0);
            let text_rect = Align2::CENTER_TOP.anchor_size(anchor, galley.size());
            painter.rect_filled(text_rect.expand(3.0), 3.0, palette.live_badge);
            painter.galley(text_rect.min, galley, color);
        }
    }

    fn paint_minimap(&self, painter: &Painter, rect: Rect, document: &Document) {
        let Some((area, transform)) = self.minimap_geometry(rect, document) else {
            return;
        };
        let palette = self.settings.palette();
        painter.rect_filled(area, 4.0, palette.minimap_background);
        painter.rect_stroke(
            area,
            4.0,
            Stroke::new(1.0, palette.major_grid),
            StrokeKind::Inside,
        );
        let painter = painter.with_clip_rect(area.shrink(2.0));
        for wire in document.schematic().wires() {
            let selected = document.selection().contains(&ObjectRef::Wire(wire.id));
            painter.line_segment(
                [transform.point(wire.a), transform.point(wire.b)],
                Stroke::new(
                    if selected { 1.6 } else { 1.0 },
                    if selected {
                        palette.accent
                    } else {
                        palette.wire.gamma_multiply(0.8)
                    },
                ),
            );
        }
        for symbol in document.schematic().symbols() {
            let selected = document.selection().contains(&ObjectRef::Symbol(symbol.id));
            painter.circle_filled(
                transform.point(symbol.at),
                if selected { 2.6 } else { 1.9 },
                if selected {
                    palette.accent
                } else {
                    palette.symbol
                },
            );
        }
        for junction in document.schematic().junctions() {
            let selected = document
                .selection()
                .contains(&ObjectRef::Junction(junction.id));
            painter.circle_filled(
                transform.point(junction.at),
                if selected { 2.2 } else { 1.5 },
                if selected {
                    palette.accent
                } else {
                    palette.wire
                },
            );
        }
        for label in document.schematic().labels() {
            let selected = document.selection().contains(&ObjectRef::Label(label.id));
            painter.circle_filled(
                transform.point(label.at),
                if selected { 2.0 } else { 1.3 },
                if selected {
                    palette.accent
                } else {
                    palette.label
                },
            );
        }
        let visible = self.viewport.visible_bounds(rect);
        painter.rect_stroke(
            transform.rect(visible.0, visible.1),
            0.0,
            Stroke::new(1.2, palette.hover),
            StrokeKind::Inside,
        );
    }

    fn paint_empty_state(&self, painter: &Painter, rect: Rect, document: &Document) {
        if !document.schematic().is_empty() {
            return;
        }
        let palette = self.settings.palette();
        painter.text(
            rect.center() - Vec2::new(0.0, 12.0),
            Align2::CENTER_CENTER,
            "Choisissez un symbole dans la palette",
            FontId::proportional(20.0),
            palette.muted,
        );
        painter.text(
            rect.center() + Vec2::new(0.0, 18.0),
            Align2::CENTER_CENTER,
            "Câblez avec W, puis lancez avec Espace.",
            FontId::proportional(13.0),
            palette.muted.gamma_multiply(0.8),
        );
        painter.text(
            rect.center() + Vec2::new(0.0, 40.0),
            Align2::CENTER_CENTER,
            "Le bouton Démarrage propose un guide en quatre étapes.",
            FontId::proportional(12.0),
            palette.muted.gamma_multiply(0.65),
        );
    }

    fn paint_tool_badge(&self, painter: &Painter, rect: Rect, tool: &crate::ui::Tool) {
        let palette = self.settings.palette();
        let text = match tool {
            crate::ui::Tool::Select => "Sélection",
            crate::ui::Tool::Place(kind) => symbol_kind_name(kind),
            crate::ui::Tool::Wire => "Fil",
            crate::ui::Tool::Junction => "Jonction",
            crate::ui::Tool::Label => "Label",
        };
        let hint = match tool {
            crate::ui::Tool::Select => {
                "Glisser une broche : câbler · glisser : déplacer · Maj : ajouter"
            }
            crate::ui::Tool::Wire => {
                if self.wire_start.is_some() {
                    "Second clic : extrémité · Maj : coude · Échap : annuler"
                } else {
                    "Départ broche/fil · Échap : outil sélection"
                }
            }
            crate::ui::Tool::Place(_) => "Clic : placer (répétitif) · R/F : tourner/miroir · Échap",
            crate::ui::Tool::Junction => "Clic : jonction · Échap : outil sélection",
            crate::ui::Tool::Label => "Clic : nommer le net · Échap : outil sélection",
        };
        let badge = Rect::from_min_size(
            rect.left_top() + Vec2::new(12.0, 12.0),
            Vec2::new(300.0, 44.0),
        );
        painter.rect_filled(badge, 6.0, palette.panel_badge);
        painter.text(
            badge.left_top() + Vec2::new(10.0, 8.0),
            Align2::LEFT_TOP,
            text,
            FontId::proportional(13.0),
            palette.accent,
        );
        painter.text(
            badge.left_top() + Vec2::new(10.0, 25.0),
            Align2::LEFT_TOP,
            hint,
            FontId::proportional(11.0),
            palette.muted,
        );
        painter.text(
            rect.left_bottom() + Vec2::new(10.0, -10.0),
            Align2::LEFT_BOTTOM,
            "Molette : zoom · bouton milieu : déplacer · mini-carte : cliquer",
            FontId::proportional(10.5),
            palette.muted.gamma_multiply(0.7),
        );
    }

    fn set_cursor(
        &self,
        ui: &Ui,
        response: &egui::Response,
        tool: &crate::ui::Tool,
        document: &Document,
    ) {
        if !response.hovered() {
            return;
        }
        let middle = ui.input(|input| input.pointer.middle_down());
        let cursor = if middle {
            CursorIcon::Grabbing
        } else {
            match tool {
                crate::ui::Tool::Select => {
                    if self
                        .last_pointer_grid
                        .is_some_and(|point| document.pin_at(point).is_some())
                    {
                        CursorIcon::Crosshair
                    } else if self.hover.is_some() {
                        CursorIcon::PointingHand
                    } else {
                        CursorIcon::Default
                    }
                }
                crate::ui::Tool::Place(_)
                | crate::ui::Tool::Wire
                | crate::ui::Tool::Junction
                | crate::ui::Tool::Label => CursorIcon::Crosshair,
            }
        };
        ui.ctx().set_cursor_icon(cursor);
    }
}

impl Default for CanvasState {
    fn default() -> Self {
        Self::new()
    }
}

fn drag_axis_delta(start: GridPos, end: GridPos) -> GridPos {
    let dx = end.x.saturating_sub(start.x);
    let dy = end.y.saturating_sub(start.y);
    if dx.abs() >= dy.abs() {
        GridPos::new(dx, 0)
    } else {
        GridPos::new(0, dy)
    }
}

fn wire_directions(
    document: &Document,
    net: &editor_core::ExtractedNet,
    source: GridPos,
) -> BTreeMap<WireId, bool> {
    let mut adjacency: BTreeMap<GridPos, Vec<(WireId, bool)>> = BTreeMap::new();
    for id in &net.wires {
        let Some(wire) = document.wire(*id) else {
            continue;
        };
        adjacency.entry(wire.a).or_default().push((*id, true));
        adjacency.entry(wire.b).or_default().push((*id, false));
    }
    let Some(root) = adjacency
        .keys()
        .copied()
        .min_by_key(|point| point.manhattan_distance(source))
    else {
        return BTreeMap::new();
    };
    let mut directions = BTreeMap::new();
    let mut visited = BTreeSet::new();
    let mut queue = std::collections::VecDeque::from([root]);
    visited.insert(root);
    while let Some(point) = queue.pop_front() {
        for (id, forward) in adjacency.get(&point).into_iter().flatten() {
            let Some(wire) = document.wire(*id) else {
                continue;
            };
            let next = if *forward { wire.b } else { wire.a };
            directions.entry(*id).or_insert(*forward);
            if visited.insert(next) {
                queue.push_back(next);
            }
        }
    }
    directions
}

fn voltage_data(
    document: &Document,
    live: &LiveValues,
) -> (BTreeMap<WireId, f64>, Option<VoltageScale>) {
    let mut wires = BTreeMap::new();
    let mut values = Vec::new();
    if live.is_empty() {
        return (wires, None);
    }
    for net in document.nets() {
        let Some(voltage) = live.voltage(net.name.as_str()) else {
            continue;
        };
        values.push(voltage);
        for id in &net.wires {
            wires.insert(*id, voltage);
        }
    }
    (wires, VoltageScale::from_values(values))
}

fn max_component_power(document: &Document, live: &LiveValues) -> f64 {
    document
        .schematic()
        .symbols()
        .filter(|symbol| !symbol.kind.is_measurement())
        .filter_map(|symbol| live.symbol(document, symbol).power)
        .filter(|power| power.is_finite() && *power > 0.0)
        .fold(0.0, f64::max)
}

fn format_hover_parameter(
    kind: &editor_core::SymbolKind,
    key: &str,
    value: &sim_core::netlist::ParamValue,
) -> String {
    match value {
        sim_core::netlist::ParamValue::Number(value) => {
            format_parameter(*value, parameter_unit(kind, key))
        }
        sim_core::netlist::ParamValue::Bool(value) => {
            if *value { "fermé" } else { "ouvert" }.to_owned()
        }
        sim_core::netlist::ParamValue::Text(value) => value.clone(),
    }
}

fn led_glow_color(symbol: &Symbol) -> (u8, u8, u8) {
    let wavelength = symbol
        .params
        .get("wavelength")
        .and_then(sim_core::netlist::ParamValue::as_number)
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(620.0);
    match wavelength {
        value if value < 450.0 => (120, 70, 255),
        value if value < 495.0 => (60, 120, 255),
        value if value < 570.0 => (70, 220, 110),
        value if value < 590.0 => (255, 220, 60),
        value if value < 620.0 => (255, 160, 50),
        value if value < 750.0 => (255, 70, 50),
        _ => (200, 30, 30),
    }
}

fn supports_power_glow(kind: &editor_core::SymbolKind) -> bool {
    !matches!(
        kind,
        editor_core::SymbolKind::Resistor
            | editor_core::SymbolKind::Switch
            | editor_core::SymbolKind::Led
            | editor_core::SymbolKind::Ground
            | editor_core::SymbolKind::Voltmeter
            | editor_core::SymbolKind::Ammeter
            | editor_core::SymbolKind::Ohmmeter
    )
}

fn report(status: &mut Status, result: Result<(), ActionError>, success: &str) {
    match result {
        Ok(()) => status.success(success),
        Err(error) => status.error(error.user_message()),
    }
}

fn overcurrent_color(palette: CanvasPalette, severity: Severity) -> Color32 {
    match severity {
        Severity::Critical => palette.critical,
        Severity::Warning => palette.warning,
        Severity::None => palette.wire,
    }
}

fn current_line(current: f64, limit: Option<f64>) -> String {
    match limit.and_then(overcurrent::valid_limit) {
        Some(limit) => format!(
            "I : {}",
            overcurrent::format_limit_line(
                current,
                limit,
                overcurrent::severity(current, Some(limit))
            )
        ),
        None => format!(
            "I : {}",
            format_si_precision(current.abs(), Unit::Ampere, 3)
        ),
    }
}

fn paint_alert_triangle(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    color: Color32,
    text_color: Color32,
) {
    let points = vec![
        Pos2::new(center.x, center.y - radius),
        Pos2::new(center.x + radius * 0.95, center.y + radius * 0.8),
        Pos2::new(center.x - radius * 0.95, center.y + radius * 0.8),
    ];
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        Stroke::new(1.0, text_color.gamma_multiply(0.7)),
    ));
    painter.text(
        center + Vec2::new(0.0, radius * 0.18),
        Align2::CENTER_CENTER,
        "!",
        FontId::proportional((radius * 1.4).max(7.0)),
        text_color,
    );
}

fn paint_symbol_outline(
    painter: &Painter,
    document: &Document,
    symbol: &Symbol,
    viewport: &Viewport,
    rect: Rect,
    color: Color32,
    width: f32,
) {
    let center = viewport.grid_to_screen(rect, symbol.at);
    let mut min = center;
    let mut max = center;
    if let Some(definition) = document.registry().get(&symbol.kind) {
        for pin in &definition.pins {
            if let Some(position) = symbol.pin_position_of(pin) {
                let screen = viewport.grid_to_screen(rect, position);
                min.x = min.x.min(screen.x);
                min.y = min.y.min(screen.y);
                max.x = max.x.max(screen.x);
                max.y = max.y.max(screen.y);
            }
        }
    }
    let margin = viewport.scale() * 0.25;
    if symbol.kind.is_ground() {
        min.x -= viewport.scale() * 1.2;
        max.x += viewport.scale() * 1.2;
        min.y -= viewport.scale() * 0.5;
        max.y += viewport.scale() * 1.8;
    } else {
        let half = crate::ui::glyph::body_half_extent(&symbol.kind) * viewport.scale();
        match symbol.orientation {
            editor_core::Orientation::R0 | editor_core::Orientation::R180 => {
                min.y = min.y.min(center.y - half);
                max.y = max.y.max(center.y + half);
            }
            editor_core::Orientation::R90 | editor_core::Orientation::R270 => {
                min.x = min.x.min(center.x - half);
                max.x = max.x.max(center.x + half);
            }
        }
        min -= Vec2::splat(margin);
        max += Vec2::splat(margin);
    }
    painter.rect_stroke(
        Rect::from_min_max(min, max),
        4.0,
        Stroke::new(width, color),
        StrokeKind::Outside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Frame, Modifiers, RawInput};

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 768.0))
    }

    fn frame(
        context: &egui::Context,
        canvas: &mut CanvasState,
        document: &mut Document,
        status: &mut Status,
        tool: &crate::ui::Tool,
        events: Vec<Event>,
    ) {
        let modifiers = events
            .iter()
            .rev()
            .find_map(|event| match event {
                Event::PointerButton { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let input = RawInput {
            screen_rect: Some(screen()),
            events,
            modifiers,
            focused: true,
            ..Default::default()
        };
        let _ = context.run_ui(input, |ui| {
            egui::CentralPanel::default()
                .frame(Frame::NONE)
                .show(ui, |ui| {
                    canvas.show(
                        ui,
                        document,
                        tool,
                        status,
                        true,
                        &LiveValues::default(),
                        &[],
                        &BTreeSet::new(),
                        0.0,
                    );
                });
        });
    }

    fn click(
        context: &egui::Context,
        canvas: &mut CanvasState,
        document: &mut Document,
        status: &mut Status,
        tool: &crate::ui::Tool,
        at: Pos2,
    ) {
        frame(
            context,
            canvas,
            document,
            status,
            tool,
            vec![
                Event::PointerMoved(at),
                Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::default(),
                },
            ],
        );
        frame(
            context,
            canvas,
            document,
            status,
            tool,
            vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            }],
        );
    }

    #[test]
    fn canvas_click_selects_a_symbol() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::demo().unwrap();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Select;

        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        let at = canvas
            .viewport()
            .grid_to_screen(screen(), GridPos::new(-1, -10));
        click(&context, &mut canvas, &mut document, &mut status, &tool, at);

        let resistor = document
            .schematic()
            .symbols()
            .find(|symbol| matches!(symbol.kind, editor_core::SymbolKind::Resistor))
            .unwrap();
        assert_eq!(
            document.selection().iter().copied().collect::<Vec<_>>(),
            vec![ObjectRef::Symbol(resistor.id)]
        );
    }

    #[test]
    fn canvas_marquee_selects_in_both_directions() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::new();
        document
            .add_symbol(editor_core::SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_symbol(editor_core::SymbolKind::Resistor, GridPos::new(4, 0))
            .unwrap();
        document.clear_selection();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Select;
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        let start = canvas
            .viewport()
            .grid_to_screen(screen(), GridPos::new(-3, -3));
        let end = canvas
            .viewport()
            .grid_to_screen(screen(), GridPos::new(3, 3));
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![
                Event::PointerMoved(start),
                Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::default(),
                },
            ],
        );
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![Event::PointerMoved(end)],
        );
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![Event::PointerButton {
                pos: end,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            }],
        );
        assert_eq!(document.selection().len(), 1);
        assert!(document
            .selection()
            .iter()
            .all(|object| matches!(object, ObjectRef::Symbol(_))));

        let start = canvas
            .viewport()
            .grid_to_screen(screen(), GridPos::new(7, 3));
        let end = canvas
            .viewport()
            .grid_to_screen(screen(), GridPos::new(4, -3));
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![
                Event::PointerMoved(start),
                Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::default(),
                },
            ],
        );
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![Event::PointerMoved(end)],
        );
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![Event::PointerButton {
                pos: end,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            }],
        );
        assert_eq!(document.selection().len(), 1);
        assert!(document
            .selection()
            .iter()
            .any(|object| matches!(object, ObjectRef::Symbol(_))));
    }

    #[test]
    fn canvas_drag_from_a_pin_creates_an_undoable_wire() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::demo().unwrap();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Select;
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        let resistor = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == editor_core::SymbolKind::Resistor)
            .unwrap();
        let definition = document.registry().get(&resistor.kind).unwrap();
        let start_grid = resistor.pin_position_of(&definition.pins[0]).unwrap();
        let end_grid = start_grid.checked_add(GridPos::new(0, 6)).unwrap();
        let start = canvas.viewport().grid_to_screen(screen(), start_grid);
        let end = canvas.viewport().grid_to_screen(screen(), end_grid);
        let before = document.schematic().wires().count();
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![
                Event::PointerMoved(start),
                Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::default(),
                },
            ],
        );
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![Event::PointerMoved(end)],
        );
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![Event::PointerButton {
                pos: end,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            }],
        );
        assert!(document.schematic().wires().count() > before);
        document.undo().unwrap();
        assert_eq!(document.schematic().wires().count(), before);
    }

    #[test]
    fn canvas_paste_preview_places_the_clipboard() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::new();
        let symbol = document
            .add_symbol(editor_core::SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        document.copy_selection().unwrap();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Select;
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        canvas.begin_paste();
        let at = canvas
            .viewport()
            .grid_to_screen(screen(), GridPos::new(2, 2));
        click(&context, &mut canvas, &mut document, &mut status, &tool, at);
        assert_eq!(document.schematic().symbols().count(), 2);
        assert!(canvas.is_pasting());
        canvas.cancel_interaction();
        assert!(!canvas.is_pasting());
    }

    #[test]
    fn context_menu_target_freezes_when_the_pointer_moves() {
        let mut canvas = CanvasState::new();
        canvas.remember_context_point(true, Some(GridPos::new(1, 2)));
        canvas.remember_context_point(false, Some(GridPos::new(9, 9)));
        assert_eq!(canvas.context_menu_point, Some(GridPos::new(1, 2)));
    }

    #[test]
    fn current_flow_uses_simulated_currents() {
        let document = Document::demo().unwrap();
        let resistor = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == editor_core::SymbolKind::Resistor)
            .unwrap();
        let left_net = document
            .net_name_at(GridPos::new(-3, -10))
            .expect("R1 left net");
        let right_net = document
            .net_name_at(GridPos::new(1, -10))
            .expect("R1 right net");
        let live = LiveValues {
            document_revision: document.revision(),
            time: Some(1e-3),
            node_voltages: [
                (left_net.clone(), 10.0),
                (right_net.clone(), 9.0),
                ("0".to_owned(), 0.0),
            ]
            .into_iter()
            .collect(),
            branch_currents: BTreeMap::new(),
        };
        let flows = net_flows(&document, &live);
        assert!(flows.contains_key(&left_net));
        assert!(flows.contains_key(&right_net));
        assert!(flows.get(&left_net).is_some_and(|flow| flow.strength > 0.0));
        assert!(document
            .symbol(resistor.id)
            .is_some_and(|symbol| symbol.kind == editor_core::SymbolKind::Resistor));
    }

    #[test]
    fn an_open_switch_stops_the_animated_flow() {
        let mut document = Document::switch_example().unwrap();
        let leakage = document
            .schematic()
            .symbols()
            .filter(|symbol| !symbol.kind.is_measurement())
            .map(|symbol| (symbol.label.clone(), 5e-9))
            .collect::<BTreeMap<_, _>>();
        let live = LiveValues {
            document_revision: document.revision(),
            time: Some(1e-3),
            node_voltages: BTreeMap::new(),
            branch_currents: leakage,
        };
        assert!(net_flows(&document, &live).is_empty());

        let switch = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == editor_core::SymbolKind::Switch)
            .unwrap()
            .clone();
        let mut closed = switch.clone();
        closed.params.insert(
            "closed".to_owned(),
            sim_core::netlist::ParamValue::Bool(true),
        );
        document.update_symbol(closed).unwrap();
        let flowing = document
            .schematic()
            .symbols()
            .filter(|symbol| !symbol.kind.is_measurement())
            .map(|symbol| (symbol.label.clone(), 5e-3))
            .collect::<BTreeMap<_, _>>();
        let live = LiveValues {
            document_revision: document.revision(),
            time: Some(1e-3),
            node_voltages: BTreeMap::new(),
            branch_currents: flowing,
        };
        assert!(!net_flows(&document, &live).is_empty());
    }

    #[test]
    fn hover_current_comes_from_the_hovered_net() {
        let document = Document::demo().unwrap();
        let left_net = document
            .net_name_at(GridPos::new(-3, -10))
            .expect("R1 left net");
        let right_net = document
            .net_name_at(GridPos::new(1, -10))
            .expect("R1 right net");
        let live = LiveValues {
            document_revision: document.revision(),
            time: Some(1e-3),
            node_voltages: [
                (left_net.clone(), 10.0),
                (right_net, 9.0),
                ("0".to_owned(), 0.0),
            ]
            .into_iter()
            .collect(),
            branch_currents: BTreeMap::new(),
        };
        let current = net_current(&document, &live, &left_net).unwrap();
        assert!((current - 1e-3).abs() < 1e-12, "current = {current}");
        assert_eq!(
            net_current(&document, &LiveValues::default(), &left_net),
            None
        );
    }

    #[test]
    fn hover_wire_current_follows_the_branch_to_ground() {
        let document = Document::divider_example().unwrap();
        let extraction = document.extraction().unwrap();
        let result = sim_core::solve_dc(&extraction.netlist, sim_core::DcOptions::default())
            .expect("divider has a DC operating point");
        let node_voltages = extraction
            .nets
            .iter()
            .filter_map(|net| {
                result
                    .node_voltage(net.node)
                    .map(|voltage| (net.name.as_str().to_owned(), voltage))
            })
            .collect();
        let live = LiveValues {
            document_revision: document.revision(),
            time: Some(1e-3),
            node_voltages,
            branch_currents: BTreeMap::new(),
        };
        let wire = document
            .schematic()
            .wires()
            .find(|wire| wire.a == GridPos::new(5, -5) && wire.b == GridPos::new(8, -5))
            .expect("source-to-divider wire exists");
        let current = wire_current(&document, &live, wire.id).unwrap();
        assert!(
            (current.abs() - 2.5e-3).abs() < 1e-12,
            "current = {current}"
        );
    }

    #[test]
    fn flow_directions_follow_the_wire_chain_from_the_source() {
        let mut document = Document::new();
        let first = document
            .add_wire(GridPos::new(0, 0), GridPos::new(4, 0))
            .unwrap()[0];
        let second = document
            .add_wire(GridPos::new(4, 0), GridPos::new(4, 4))
            .unwrap()[0];
        document
            .add_symbol(editor_core::SymbolKind::Ground, GridPos::new(4, 0))
            .unwrap();
        let net = document
            .nets()
            .iter()
            .find(|net| net.wires.contains(&first))
            .expect("grounded wires form a net");
        assert!(net.wires.contains(&second));
        let directions = wire_directions(&document, net, GridPos::new(0, 0));
        assert_eq!(directions.get(&first), Some(&true));
        assert_eq!(directions.get(&second), Some(&true));
    }

    #[test]
    fn canvas_minimap_click_recenters_the_view() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::demo().unwrap();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Select;
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        let (area, transform) = canvas
            .minimap_geometry(screen(), &document)
            .expect("demo content has a minimap");
        let target = transform.grid_at(area.center());
        click(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            area.center(),
        );
        let center = canvas
            .viewport()
            .screen_to_grid(screen(), screen().center())
            .unwrap();
        assert_eq!(center, target);
        canvas.set_minimap_visible(false);
        assert!(canvas.minimap_geometry(screen(), &document).is_none());
    }

    #[test]
    fn canvas_placement_click_adds_a_symbol() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::new();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Place(editor_core::SymbolKind::Resistor);

        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        let at = screen().center();
        click(&context, &mut canvas, &mut document, &mut status, &tool, at);

        let symbols: Vec<_> = document.schematic().symbols().collect();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].kind, editor_core::SymbolKind::Resistor);
        assert_eq!(
            symbols[0].at,
            canvas
                .viewport()
                .screen_to_grid(screen(), at)
                .expect("center is on the canvas")
        );
    }

    #[test]
    fn canvas_wire_tool_creates_orthogonal_segments() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::new();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Wire;

        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        let start = canvas
            .viewport()
            .screen_to_grid(screen(), screen().center())
            .unwrap();
        let end = GridPos::new(start.x + 3, start.y + 2);
        let start_screen = canvas.viewport().grid_to_screen(screen(), start);
        let end_screen = canvas.viewport().grid_to_screen(screen(), end);
        click(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            start_screen,
        );
        click(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            end_screen,
        );

        let wires: Vec<_> = document.schematic().wires().collect();
        assert_eq!(wires.len(), 2);
        assert_eq!(
            wires
                .iter()
                .map(|wire| (wire.a, wire.b))
                .collect::<Vec<_>>(),
            vec![
                (start, GridPos::new(end.x, start.y)),
                (GridPos::new(end.x, start.y), end),
            ]
        );
    }

    #[test]
    fn canvas_drag_moves_the_selected_symbol() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::demo().unwrap();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Select;

        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        let resistor = document
            .schematic()
            .symbols()
            .find(|symbol| matches!(symbol.kind, editor_core::SymbolKind::Resistor))
            .unwrap();
        let id = resistor.id;
        let start = resistor.at;
        let target = GridPos::new(start.x + 2, start.y + 1);
        let start_screen = canvas.viewport().grid_to_screen(screen(), start);
        let target_screen = canvas.viewport().grid_to_screen(screen(), target);

        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![
                Event::PointerMoved(start_screen),
                Event::PointerButton {
                    pos: start_screen,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::default(),
                },
            ],
        );
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![Event::PointerMoved(target_screen)],
        );
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![Event::PointerButton {
                pos: target_screen,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            }],
        );

        assert_eq!(document.symbol(id).unwrap().at, target);
        assert!(document.selection().contains(&ObjectRef::Symbol(id)));
    }

    #[test]
    fn canvas_alt_click_selects_a_whole_net() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::demo().unwrap();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Select;
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        let at = canvas
            .viewport()
            .grid_to_screen(screen(), GridPos::new(-9, -10));
        let modifiers = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![
                Event::PointerMoved(at),
                Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers,
                },
            ],
        );
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: false,
                modifiers,
            }],
        );
        assert!(document.selection().len() > 2);
        assert!(document
            .selection()
            .iter()
            .any(|object| matches!(object, ObjectRef::Wire(_))));
    }

    #[test]
    fn canvas_placement_on_wire_inserts_in_series() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::new();
        document
            .add_wire(GridPos::new(0, 0), GridPos::new(8, 0))
            .unwrap();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Place(editor_core::SymbolKind::Resistor);
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        let at = canvas
            .viewport()
            .grid_to_screen(screen(), GridPos::new(4, 0));
        click(&context, &mut canvas, &mut document, &mut status, &tool, at);
        assert_eq!(document.schematic().symbols().count(), 1);
        assert_eq!(document.schematic().wires().count(), 2);
    }

    #[test]
    fn canvas_placement_at_a_wire_end_snaps_a_pin() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::new();
        document
            .add_wire(GridPos::new(0, 0), GridPos::new(8, 0))
            .unwrap();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Place(editor_core::SymbolKind::Resistor);
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        let at = canvas
            .viewport()
            .grid_to_screen(screen(), GridPos::new(8, 0));
        click(&context, &mut canvas, &mut document, &mut status, &tool, at);

        let symbol = document.schematic().symbols().next().unwrap();
        assert_eq!(symbol.at, GridPos::new(10, 0));
        assert_eq!(document.schematic().wires().count(), 1);
        let definition = document.registry().get(&symbol.kind).unwrap();
        assert_eq!(
            symbol.pin_position_of(&definition.pins[0]),
            Some(GridPos::new(8, 0))
        );
    }

    #[test]
    fn canvas_rotated_placement_keeps_the_preview_orientation() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::new();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Place(editor_core::SymbolKind::Resistor);
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        canvas.rotate_placement();
        let at = screen().center();
        click(&context, &mut canvas, &mut document, &mut status, &tool, at);
        assert_eq!(
            document
                .schematic()
                .symbols()
                .next()
                .map(|symbol| symbol.orientation),
            Some(Orientation::R90)
        );
    }

    #[test]
    fn canvas_label_tool_opens_the_name_dialog() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::new();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Label;

        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        click(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            screen().center(),
        );

        assert!(canvas.has_dialog());
        assert!(document.schematic().labels().next().is_none());
    }

    #[test]
    fn canvas_drag_moves_a_horizontal_wire_vertically() {
        let context = egui::Context::default();
        let mut canvas = CanvasState::new();
        let mut document = Document::new();
        let mut status = Status::new_info("");
        let tool = crate::ui::Tool::Select;
        let wire = document
            .add_wire(GridPos::new(-4, 0), GridPos::new(4, 0))
            .unwrap()[0];
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            Vec::new(),
        );
        let start = GridPos::new(0, 0);
        let target = GridPos::new(0, 3);
        let start_screen = canvas.viewport().grid_to_screen(screen(), start);
        let target_screen = canvas.viewport().grid_to_screen(screen(), target);
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![
                Event::PointerMoved(start_screen),
                Event::PointerButton {
                    pos: start_screen,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::default(),
                },
            ],
        );
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![Event::PointerMoved(target_screen)],
        );
        frame(
            &context,
            &mut canvas,
            &mut document,
            &mut status,
            &tool,
            vec![Event::PointerButton {
                pos: target_screen,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            }],
        );

        let moved = document.wire(wire).unwrap();
        assert_eq!(moved.a, GridPos::new(-4, 3));
        assert_eq!(moved.b, GridPos::new(4, 3));
    }

    #[test]
    fn drag_axis_delta_follows_the_dominant_axis() {
        assert_eq!(
            drag_axis_delta(GridPos::new(0, 0), GridPos::new(6, 2)),
            GridPos::new(6, 0)
        );
        assert_eq!(
            drag_axis_delta(GridPos::new(0, 0), GridPos::new(1, 5)),
            GridPos::new(0, 5)
        );
        assert_eq!(
            drag_axis_delta(GridPos::new(2, 2), GridPos::new(2, 2)),
            GridPos::default()
        );
    }
}
