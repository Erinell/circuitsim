//! Interactive schematic canvas with pan, zoom, placement, and dragging.

use std::collections::BTreeSet;

use editor_core::{GridPos, ObjectRef, Orientation, SnapTarget, Symbol, SymbolId, WireId};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Key, Painter, PointerButton, Pos2, Rect, Sense,
    Stroke, StrokeKind, Ui, Vec2,
};

use crate::document::{ActionError, ClipboardItem, Document, SelectionChange, SelectionMode};
use crate::ui::glyph::{paint_symbol, GlyphColors};
use crate::ui::{
    symbol_kind_name, Status, ACCENT, CANVAS_BACKGROUND, HOVER, LABEL, MUTED, PIN, SYMBOL, WIRE,
};
use crate::view::{MinimapTransform, Viewport, DEFAULT_SCALE};

const MINOR_GRID: Color32 = Color32::from_rgb(24, 32, 46);
const MAJOR_GRID: Color32 = Color32::from_rgb(34, 45, 64);
const AXIS: Color32 = Color32::from_rgb(58, 78, 108);
const PANEL_BADGE: Color32 = Color32::from_rgba_premultiplied(24, 34, 48, 220);
const SELECTION_FILL: Color32 = Color32::from_rgba_premultiplied(112, 82, 24, 120);
const MINIMAP_BACKGROUND: Color32 = Color32::from_rgba_premultiplied(12, 18, 28, 230);
const MINIMAP_WIDTH: f32 = 152.0;
const MINIMAP_HEIGHT: f32 = 112.0;
const MINIMAP_MARGIN: f32 = 12.0;
const ZOOM_STEP: f32 = 1.25;

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
    paste_preview: bool,
    fit_requested: bool,
    pending_fit: Option<(GridPos, GridPos)>,
    last_rect: Option<Rect>,
    minimap_visible: bool,
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
            paste_preview: false,
            fit_requested: true,
            pending_fit: None,
            last_rect: None,
            minimap_visible: true,
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

    /// Returns whether the minimap is drawn.
    pub const fn minimap_visible(&self) -> bool {
        self.minimap_visible
    }

    /// Shows or hides the minimap.
    pub fn set_minimap_visible(&mut self, visible: bool) {
        self.minimap_visible = visible;
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
    pub fn show(
        &mut self,
        ui: &mut Ui,
        document: &mut Document,
        tool: &crate::ui::Tool,
        status: &mut Status,
        interactive: bool,
    ) {
        let available = ui.available_size();
        if available.x < 2.0 || available.y < 2.0 {
            return;
        }
        let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
        let rect = response.rect;
        self.last_rect = Some(rect);
        painter.rect_filled(rect, 0.0, CANVAS_BACKGROUND);

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

        if interactive {
            self.handle_viewport(ui, &response, rect);
        }
        let raw_point = ui
            .input(|input| input.pointer.hover_pos())
            .filter(|position| rect.contains(*position))
            .and_then(|position| self.viewport.screen_to_grid(rect, position));
        self.snap_target = if matches!(tool, crate::ui::Tool::Wire | crate::ui::Tool::Place(_)) {
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
            self.handle_input(ui, &response, rect, document, tool, status, focus_point);
        }
        let hovered_point = focus_point.or_else(|| {
            ui.input(|input| input.pointer.hover_pos())
                .filter(|position| rect.contains(*position))
                .and_then(|position| self.viewport.screen_to_grid(rect, position))
        });

        self.paint_grid(&painter, rect);
        self.paint_document(&painter, rect, document);
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
        self.paint_minimap(&painter, rect, document);
        self.set_cursor(ui, &response, tool);
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
                    ui.label(egui::RichText::new("Nets existants").small().color(MUTED));
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
                        .color(MUTED),
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
                if let Some(object) = document.object_at(point) {
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
                        let delta = document.wire(wire).map_or(GridPos::default(), |segment| {
                            if segment.is_horizontal() {
                                GridPos::new(0, end.y.saturating_sub(start.y))
                            } else {
                                GridPos::new(end.x.saturating_sub(start.x), 0)
                            }
                        });
                        if delta != GridPos::default() {
                            match document.move_wire_segment(wire, delta) {
                                Ok(()) => status.success("Segment déplacé."),
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
        let point = self.last_pointer_grid;
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
                    ObjectRef::Symbol(_) => {
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
                    ObjectRef::Wire(_) | ObjectRef::Junction(_) => {
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

    fn handle_click(
        &mut self,
        point: GridPos,
        document: &mut Document,
        tool: &crate::ui::Tool,
        status: &mut Status,
        shift: bool,
    ) {
        match tool {
            crate::ui::Tool::Select => {}
            crate::ui::Tool::Place(kind) => {
                let insertable = document
                    .registry()
                    .get(kind)
                    .is_some_and(|definition| definition.pins.len() == 2);
                if let Some(ObjectRef::Wire(wire)) = document.object_at(point) {
                    if insertable {
                        match document.insert_symbol_on_wire(kind.clone(), wire, point) {
                            Ok(_) => {
                                status.success(format!(
                                    "{} inséré en série à ({}, {}).",
                                    symbol_kind_name(kind),
                                    point.x,
                                    point.y
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
                match document.add_symbol_oriented(
                    kind.clone(),
                    point,
                    self.placement_orientation,
                    self.placement_mirrored,
                ) {
                    Ok(_) => status.success(format!(
                        "{} ajouté à ({}, {}).",
                        symbol_kind_name(kind),
                        point.x,
                        point.y
                    )),
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
        let (min, max) = self.viewport.visible_bounds(rect);
        let step = if self.viewport.scale() >= 14.0 { 1 } else { 5 };
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
                        AXIS
                    } else if major {
                        MAJOR_GRID
                    } else {
                        MINOR_GRID
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
                        AXIS
                    } else if major {
                        MAJOR_GRID
                    } else {
                        MINOR_GRID
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

    fn paint_document(&self, painter: &Painter, rect: Rect, document: &Document) {
        for wire in document.schematic().wires() {
            let selected = document.selection().contains(&ObjectRef::Wire(wire.id));
            let hovered = self.hover == Some(ObjectRef::Wire(wire.id));
            let net_hovered = self.hover_net.contains(&ObjectRef::Wire(wire.id));
            if (hovered || net_hovered) && !selected {
                painter.line_segment(
                    [
                        self.viewport.grid_to_screen(rect, wire.a),
                        self.viewport.grid_to_screen(rect, wire.b),
                    ],
                    Stroke::new(5.0, HOVER.gamma_multiply(0.25)),
                );
            }
            painter.line_segment(
                [
                    self.viewport.grid_to_screen(rect, wire.a),
                    self.viewport.grid_to_screen(rect, wire.b),
                ],
                Stroke::new(
                    if selected {
                        4.0
                    } else if hovered || net_hovered {
                        3.0
                    } else {
                        2.2
                    },
                    if selected {
                        ACCENT
                    } else if hovered || net_hovered {
                        HOVER
                    } else {
                        WIRE
                    },
                ),
            );
        }

        for symbol in document.schematic().symbols() {
            let selected = document.selection().contains(&ObjectRef::Symbol(symbol.id));
            let hovered = self.hover == Some(ObjectRef::Symbol(symbol.id));
            let net_hovered = self.hover_net.contains(&ObjectRef::Symbol(symbol.id));
            let body = if selected {
                ACCENT
            } else if hovered || net_hovered {
                HOVER
            } else {
                SYMBOL
            };
            let colors = GlyphColors {
                body,
                pin: if selected || hovered || net_hovered {
                    body
                } else {
                    PIN
                },
                label: if selected { ACCENT } else { LABEL },
                value: if selected { ACCENT } else { MUTED },
            };
            paint_symbol(
                painter,
                symbol,
                document.registry().get(&symbol.kind),
                self.viewport.grid_to_screen(rect, symbol.at),
                self.viewport.scale(),
                colors,
                true,
            );
            if selected {
                paint_symbol_outline(painter, document, symbol, &self.viewport, rect, ACCENT, 1.7);
            } else if hovered || net_hovered {
                paint_symbol_outline(painter, document, symbol, &self.viewport, rect, HOVER, 1.2);
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
                ACCENT
            } else if hovered || net_hovered {
                HOVER
            } else {
                WIRE
            };
            painter.circle_filled(position, radius, color);
            painter.circle_stroke(position, radius, Stroke::new(1.0, CANVAS_BACKGROUND));
        }

        for label in document.schematic().labels() {
            let selected = document.selection().contains(&ObjectRef::Label(label.id));
            let hovered = self.hover == Some(ObjectRef::Label(label.id));
            let net_hovered = self.hover_net.contains(&ObjectRef::Label(label.id));
            let position = self.viewport.grid_to_screen(rect, label.at);
            let color = if selected {
                ACCENT
            } else if hovered || net_hovered {
                HOVER
            } else {
                LABEL
            };
            let font = FontId::proportional(13.0);
            let galley = painter.layout_no_wrap(label.text.clone(), font, color);
            let text_pos = position + Vec2::new(7.0, -6.0);
            let text_rect = Align2::LEFT_BOTTOM.anchor_size(text_pos, galley.size());
            if selected {
                painter.rect_filled(text_rect.expand(3.0), 3.0, SELECTION_FILL);
                painter.rect_stroke(
                    text_rect.expand(3.0),
                    3.0,
                    Stroke::new(1.0, ACCENT),
                    StrokeKind::Outside,
                );
            }
            painter.circle_filled(position, 2.8, color);
            painter.galley(text_rect.min, galley, color);
        }
    }

    fn paint_hover(&self, painter: &Painter, rect: Rect, document: &Document) {
        let Some(object) = self.hover else {
            return;
        };
        if document.selection().contains(&object) {
            return;
        }
        match object {
            ObjectRef::Label(id) => {
                if let Some(label) = document.label(id) {
                    painter.circle_stroke(
                        self.viewport.grid_to_screen(rect, label.at),
                        8.0,
                        Stroke::new(1.2, HOVER),
                    );
                }
            }
            ObjectRef::Junction(id) => {
                if let Some(junction) = document.junction(id) {
                    painter.circle_stroke(
                        self.viewport.grid_to_screen(rect, junction.at),
                        9.0,
                        Stroke::new(1.2, HOVER),
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
                    let delta = if segment.is_horizontal() {
                        GridPos::new(0, current.y.saturating_sub(start.y))
                    } else {
                        GridPos::new(current.x.saturating_sub(start.x), 0)
                    };
                    if delta != GridPos::default() {
                        if let (Some(a), Some(b)) =
                            (segment.a.checked_add(delta), segment.b.checked_add(delta))
                        {
                            painter.line_segment(
                                [
                                    self.viewport.grid_to_screen(rect, a),
                                    self.viewport.grid_to_screen(rect, b),
                                ],
                                Stroke::new(3.0, ACCENT.gamma_multiply(0.6)),
                            );
                        }
                    }
                }
            }
            Some(DragState::Marquee { start, current, .. }) => {
                let min = GridPos::new(start.x.min(current.x), start.y.min(current.y));
                let max = GridPos::new(start.x.max(current.x), start.y.max(current.y));
                let rectangle = Rect::from_two_pos(
                    self.viewport.grid_to_screen(rect, min),
                    self.viewport.grid_to_screen(rect, max),
                );
                let color = if start.x <= current.x { ACCENT } else { HOVER };
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
                let color = ACCENT.gamma_multiply(0.9);
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
                    let preview = Symbol::from_def(
                        SymbolId(0),
                        definition,
                        point,
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
                        self.viewport.grid_to_screen(rect, point),
                        self.viewport.scale(),
                        GlyphColors {
                            body: SYMBOL,
                            pin: PIN,
                            label: LABEL,
                            value: MUTED,
                        },
                        false,
                    );
                    for pin in &definition.pins {
                        if let Some(position) = preview.pin_position_of(pin) {
                            painter.circle_stroke(
                                self.viewport.grid_to_screen(rect, position),
                                (self.viewport.scale() * 0.4).clamp(4.0, 8.0),
                                Stroke::new(1.0, ACCENT.gamma_multiply(0.7)),
                            );
                        }
                    }
                }
                if matches!(document.object_at(point), Some(ObjectRef::Wire(_)))
                    && document
                        .registry()
                        .get(kind)
                        .is_some_and(|definition| definition.pins.len() == 2)
                {
                    painter.circle_filled(self.viewport.grid_to_screen(rect, point), 4.0, ACCENT);
                }
            }
        } else if let crate::ui::Tool::Junction = tool {
            if let Some(point) = hovered_point {
                painter.circle_stroke(
                    self.viewport.grid_to_screen(rect, point),
                    7.0,
                    Stroke::new(1.3, ACCENT),
                );
            }
        } else if let crate::ui::Tool::Label = tool {
            if let Some(point) = hovered_point {
                let screen = self.viewport.grid_to_screen(rect, point);
                painter.circle_stroke(screen, 7.0, Stroke::new(1.3, ACCENT));
                painter.text(
                    screen + Vec2::new(8.0, -6.0),
                    Align2::LEFT_BOTTOM,
                    "NET",
                    FontId::proportional(12.0),
                    ACCENT.gamma_multiply(0.8),
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
                                body: ACCENT,
                                pin: ACCENT,
                                label: ACCENT,
                                value: ACCENT,
                            },
                            false,
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
                            Stroke::new(3.0, ACCENT.gamma_multiply(0.45)),
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
                            ACCENT.gamma_multiply(0.5),
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
                            ACCENT.gamma_multiply(0.6),
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
                                body: ACCENT,
                                pin: ACCENT,
                                label: ACCENT,
                                value: ACCENT,
                            },
                            false,
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
                            Stroke::new(2.5, ACCENT.gamma_multiply(0.7)),
                        );
                    }
                }
                ClipboardItem::Junction(junction) => {
                    if let Some(position) = junction.at.checked_add(delta) {
                        ghost.circle_filled(
                            self.viewport.grid_to_screen(rect, position),
                            4.0,
                            ACCENT.gamma_multiply(0.7),
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
                            ACCENT,
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
        let color = ACCENT.gamma_multiply(0.22);
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
            Stroke::new(1.4, HOVER),
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

    fn paint_minimap(&self, painter: &Painter, rect: Rect, document: &Document) {
        let Some((area, transform)) = self.minimap_geometry(rect, document) else {
            return;
        };
        painter.rect_filled(area, 4.0, MINIMAP_BACKGROUND);
        painter.rect_stroke(area, 4.0, Stroke::new(1.0, MAJOR_GRID), StrokeKind::Inside);
        let painter = painter.with_clip_rect(area.shrink(2.0));
        for wire in document.schematic().wires() {
            let selected = document.selection().contains(&ObjectRef::Wire(wire.id));
            painter.line_segment(
                [transform.point(wire.a), transform.point(wire.b)],
                Stroke::new(
                    if selected { 1.6 } else { 1.0 },
                    if selected {
                        ACCENT
                    } else {
                        WIRE.gamma_multiply(0.8)
                    },
                ),
            );
        }
        for symbol in document.schematic().symbols() {
            let selected = document.selection().contains(&ObjectRef::Symbol(symbol.id));
            painter.circle_filled(
                transform.point(symbol.at),
                if selected { 2.6 } else { 1.9 },
                if selected { ACCENT } else { SYMBOL },
            );
        }
        for junction in document.schematic().junctions() {
            let selected = document
                .selection()
                .contains(&ObjectRef::Junction(junction.id));
            painter.circle_filled(
                transform.point(junction.at),
                if selected { 2.2 } else { 1.5 },
                if selected { ACCENT } else { WIRE },
            );
        }
        for label in document.schematic().labels() {
            let selected = document.selection().contains(&ObjectRef::Label(label.id));
            painter.circle_filled(
                transform.point(label.at),
                if selected { 2.0 } else { 1.3 },
                if selected { ACCENT } else { LABEL },
            );
        }
        let visible = self.viewport.visible_bounds(rect);
        painter.rect_stroke(
            transform.rect(visible.0, visible.1),
            0.0,
            Stroke::new(1.2, HOVER),
            StrokeKind::Inside,
        );
    }

    fn paint_empty_state(&self, painter: &Painter, rect: Rect, document: &Document) {
        if !document.schematic().is_empty() {
            return;
        }
        painter.text(
            rect.center() - Vec2::new(0.0, 12.0),
            Align2::CENTER_CENTER,
            "Choisissez un symbole dans la palette",
            FontId::proportional(20.0),
            MUTED,
        );
        painter.text(
            rect.center() + Vec2::new(0.0, 18.0),
            Align2::CENTER_CENTER,
            "ou utilisez l'outil Fil pour dessiner une connexion.",
            FontId::proportional(13.0),
            MUTED.gamma_multiply(0.8),
        );
    }

    fn paint_tool_badge(&self, painter: &Painter, rect: Rect, tool: &crate::ui::Tool) {
        let text = match tool {
            crate::ui::Tool::Select => "Sélection",
            crate::ui::Tool::Place(kind) => symbol_kind_name(kind),
            crate::ui::Tool::Wire => "Fil",
            crate::ui::Tool::Junction => "Jonction",
            crate::ui::Tool::Label => "Label",
        };
        let hint = match tool {
            crate::ui::Tool::Select => "Glisser pour déplacer · Maj pour ajouter",
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
        painter.rect_filled(badge, 6.0, PANEL_BADGE);
        painter.text(
            badge.left_top() + Vec2::new(10.0, 8.0),
            Align2::LEFT_TOP,
            text,
            FontId::proportional(13.0),
            ACCENT,
        );
        painter.text(
            badge.left_top() + Vec2::new(10.0, 25.0),
            Align2::LEFT_TOP,
            hint,
            FontId::proportional(10.5),
            MUTED,
        );
        painter.text(
            rect.left_bottom() + Vec2::new(10.0, -10.0),
            Align2::LEFT_BOTTOM,
            "Molette : zoom · bouton milieu : déplacer · mini-carte : cliquer",
            FontId::proportional(10.0),
            MUTED.gamma_multiply(0.7),
        );
    }

    fn set_cursor(&self, ui: &Ui, response: &egui::Response, tool: &crate::ui::Tool) {
        if !response.hovered() {
            return;
        }
        let middle = ui.input(|input| input.pointer.middle_down());
        let cursor = if middle {
            CursorIcon::Grabbing
        } else {
            match tool {
                crate::ui::Tool::Select => {
                    if self.hover.is_some() {
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

fn report(status: &mut Status, result: Result<(), ActionError>, success: &str) {
    match result {
        Ok(()) => status.success(success),
        Err(error) => status.error(error.user_message()),
    }
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
    let margin = viewport.scale() * 1.05;
    if symbol.kind.is_ground() {
        min.x -= viewport.scale() * 1.2;
        max.x += viewport.scale() * 1.2;
        min.y -= viewport.scale() * 0.5;
        max.y += viewport.scale() * 1.8;
    } else {
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
                    canvas.show(ui, document, tool, status, true);
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
            .grid_to_screen(screen(), GridPos::new(-2, -8));
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
            .grid_to_screen(screen(), GridPos::new(-7, -8));
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
}
