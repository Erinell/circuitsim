//! Editable document state shared by the application panels.

use std::collections::BTreeSet;

use editor_core::{
    extract_netlist, Command, EditError, ErcReport, ExtractedNet, Extraction, GridPos, Junction,
    JunctionId, NetLabel, NetLabelId, ObjectRef, Orientation, Schematic, Symbol, SymbolId,
    SymbolKind, SymbolRegistry, UndoStack, Wire, WireGeometryError, WireId,
};
use sim_core::netlist::{NameError, ParamValue};
use sim_core::units::UnitParseError;
use thiserror::Error;

/// Error returned by an application editing action.
#[derive(Debug, Error)]
pub enum ActionError {
    /// A reversible editor command was refused.
    #[error("la commande a été refusée : {0}")]
    Edit(#[from] EditError),
    /// A name failed validation.
    #[error("le nom est invalide : {0}")]
    Name(#[from] NameError),
    /// A wire geometry failed validation.
    #[error("le fil est invalide : {0}")]
    Wire(#[from] WireGeometryError),
    /// The requested symbol kind is not registered.
    #[error("le type de symbole n'est pas enregistré")]
    UnknownSymbol,
    /// The identifier space is exhausted.
    #[error("l'espace d'identifiants est épuisé")]
    IdSpaceExhausted,
    /// The action requires at least one selected object.
    #[error("aucun objet n'est sélectionné")]
    EmptySelection,
    /// The action requires at least one selected symbol.
    #[error("aucun symbole n'est sélectionné")]
    NoSymbolSelected,
    /// The referenced object disappeared from the document.
    #[error("l'objet référencé n'existe plus")]
    MissingObject,
    /// An equivalent object already exists.
    #[error("un objet identique existe déjà")]
    Duplicate,
    /// A zero-length wire was requested.
    #[error("un fil de longueur nulle est refusé")]
    ZeroLengthWire,
    /// A numerical parameter is not finite.
    #[error("les paramètres numériques doivent être finis")]
    NonFiniteParameter,
}

impl ActionError {
    /// Returns a concise French message suitable for the interface.
    pub fn user_message(&self) -> String {
        match self {
            Self::Edit(error) => match error {
                EditError::AlreadyExists(_) => "Un objet identique existe déjà.".to_owned(),
                EditError::NotFound(_) => "L'objet demandé n'existe plus.".to_owned(),
                EditError::StaleCommand => "Le schéma a changé : recommencez l'action.".to_owned(),
                EditError::InvalidCommand(_) => {
                    "La modification n'est pas applicable au schéma.".to_owned()
                }
                EditError::PositionOverflow => "La position sort de la grille.".to_owned(),
                EditError::Schematic(_) => {
                    "La modification produirait un schéma invalide.".to_owned()
                }
                EditError::IdSpaceExhausted => "L'espace d'identifiants est épuisé.".to_owned(),
            },
            Self::Name(error) => match error {
                NameError::Empty => "Le nom ne peut pas être vide.".to_owned(),
                NameError::TooLong { max, actual } => {
                    format!("Le nom est trop long ({actual} caractères, maximum {max}).")
                }
                NameError::ControlCharacter => {
                    "Le nom contient un caractère de contrôle.".to_owned()
                }
            },
            Self::Wire(error) => match error {
                WireGeometryError::ZeroLength => {
                    "Un fil ne peut pas avoir une longueur nulle.".to_owned()
                }
                WireGeometryError::NotOrthogonal => {
                    "Un fil doit être horizontal ou vertical.".to_owned()
                }
                WireGeometryError::OutOfBounds => "Le fil sort de la grille autorisée.".to_owned(),
            },
            Self::UnknownSymbol => "Ce type de symbole n'est pas enregistré.".to_owned(),
            Self::IdSpaceExhausted => "L'espace d'identifiants est épuisé.".to_owned(),
            Self::EmptySelection => "Sélectionnez au moins un objet.".to_owned(),
            Self::NoSymbolSelected => "Sélectionnez au moins un symbole.".to_owned(),
            Self::MissingObject => "L'objet référencé n'existe plus.".to_owned(),
            Self::Duplicate => "Un objet identique existe déjà.".to_owned(),
            Self::ZeroLengthWire => "Un fil ne peut pas avoir une longueur nulle.".to_owned(),
            Self::NonFiniteParameter => "Les paramètres numériques doivent être finis.".to_owned(),
        }
    }
}

/// Editable schematic, history, selection, and cached extraction.
pub struct Document {
    schematic: Schematic,
    history: UndoStack,
    registry: SymbolRegistry,
    extraction: Option<Extraction>,
    extraction_error: Option<String>,
    selected: BTreeSet<ObjectRef>,
    revision: u64,
    dirty: bool,
}

impl Document {
    /// Creates an empty document.
    pub fn new() -> Self {
        let mut document = Self {
            schematic: Schematic::new(),
            history: UndoStack::new(),
            registry: SymbolRegistry::builtin(),
            extraction: None,
            extraction_error: None,
            selected: BTreeSet::new(),
            revision: 0,
            dirty: false,
        };
        document.refresh_extraction();
        document
    }

    /// Creates the editable demonstration circuit.
    pub fn demo() -> Result<Self, ActionError> {
        let mut document = Self::new();

        let source = document.add_symbol(SymbolKind::VoltageSource, GridPos::new(-10, -6))?;
        set_orientation(&mut document, source, Orientation::R90)?;
        document.add_symbol(SymbolKind::Resistor, GridPos::new(-2, -8))?;
        let capacitor = document.add_symbol(SymbolKind::Capacitor, GridPos::new(6, -3))?;
        set_orientation(&mut document, capacitor, Orientation::R90)?;
        document.add_symbol(SymbolKind::Ground, GridPos::new(-10, -2))?;

        document.add_wire(GridPos::new(-10, -8), GridPos::new(-4, -8))?;
        document.add_wire(GridPos::new(0, -8), GridPos::new(6, -8))?;
        document.add_wire(GridPos::new(6, -8), GridPos::new(6, -5))?;
        document.add_wire(GridPos::new(6, -1), GridPos::new(-10, -1))?;
        document.add_wire(GridPos::new(-10, -1), GridPos::new(-10, -2))?;
        document.add_wire(GridPos::new(-10, -4), GridPos::new(-10, -2))?;
        document.add_label(GridPos::new(-6, -8), "VIN")?;
        document.add_label(GridPos::new(6, -8), "OUT")?;

        document.history.clear();
        document.selected.clear();
        document.dirty = false;
        document.revision = 0;
        Ok(document)
    }

    /// Returns the current schematic.
    pub const fn schematic(&self) -> &Schematic {
        &self.schematic
    }

    /// Returns the symbol registry used for extraction.
    pub const fn registry(&self) -> &SymbolRegistry {
        &self.registry
    }

    /// Returns the current selection in deterministic order.
    pub fn selection(&self) -> &BTreeSet<ObjectRef> {
        &self.selected
    }

    /// Returns the current revision. It increases after every accepted edit.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns true after an edit and until the document is replaced.
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Returns the cached extraction when the schematic is structurally valid.
    pub const fn extraction(&self) -> Option<&Extraction> {
        self.extraction.as_ref()
    }

    /// Returns the extraction error when extraction could not complete.
    pub fn extraction_error(&self) -> Option<&str> {
        self.extraction_error.as_deref()
    }

    /// Returns the ERC report when extraction succeeded.
    pub fn erc(&self) -> Option<&ErcReport> {
        self.extraction.as_ref().map(|extraction| &extraction.erc)
    }

    /// Returns the extracted nets when extraction succeeded.
    pub fn nets(&self) -> &[ExtractedNet] {
        self.extraction
            .as_ref()
            .map_or(&[], |extraction| extraction.nets.as_slice())
    }

    /// Returns true when at least one command can be undone.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Returns true when at least one command can be redone.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Returns a placed symbol.
    pub fn symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.schematic.symbol(id)
    }

    /// Returns a wire.
    pub fn wire(&self, id: WireId) -> Option<&Wire> {
        self.schematic.wire(id)
    }

    /// Returns a junction.
    pub fn junction(&self, id: JunctionId) -> Option<&Junction> {
        self.schematic.junction(id)
    }

    /// Returns a net label.
    pub fn label(&self, id: NetLabelId) -> Option<&NetLabel> {
        self.schematic.label(id)
    }

    /// Returns the single selected symbol, if exactly one symbol is selected.
    pub fn selected_symbol(&self) -> Option<&Symbol> {
        if self.selected.len() != 1 {
            return None;
        }
        match self.selected.first() {
            Some(ObjectRef::Symbol(id)) => self.symbol(*id),
            _ => None,
        }
    }

    /// Returns the single selected wire, if exactly one wire is selected.
    pub fn selected_wire(&self) -> Option<&Wire> {
        if self.selected.len() != 1 {
            return None;
        }
        match self.selected.first() {
            Some(ObjectRef::Wire(id)) => self.wire(*id),
            _ => None,
        }
    }

    /// Returns the single selected junction, if exactly one junction is selected.
    pub fn selected_junction(&self) -> Option<&Junction> {
        if self.selected.len() != 1 {
            return None;
        }
        match self.selected.first() {
            Some(ObjectRef::Junction(id)) => self.junction(*id),
            _ => None,
        }
    }

    /// Returns the single selected label, if exactly one label is selected.
    pub fn selected_label(&self) -> Option<&NetLabel> {
        if self.selected.len() != 1 {
            return None;
        }
        match self.selected.first() {
            Some(ObjectRef::Label(id)) => self.label(*id),
            _ => None,
        }
    }

    /// Replaces the selection with one object, or clears it.
    pub fn select_only(&mut self, object: Option<ObjectRef>) {
        self.selected.clear();
        if let Some(object) = object {
            self.selected.insert(object);
        }
    }

    /// Adds or removes one object from the selection.
    pub fn toggle_selection(&mut self, object: ObjectRef) {
        if !self.selected.remove(&object) {
            self.selected.insert(object);
        }
    }

    /// Selects every editable object.
    pub fn select_all(&mut self) {
        self.selected = self.schematic.object_refs().into_iter().collect();
    }

    /// Clears the selection.
    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    /// Removes dangling references after undo, redo, or deletion.
    pub fn prune_selection(&mut self) {
        let schematic = &self.schematic;
        self.selected.retain(|object| match object {
            ObjectRef::Symbol(id) => schematic.symbol(*id).is_some(),
            ObjectRef::Wire(id) => schematic.wire(*id).is_some(),
            ObjectRef::Junction(id) => schematic.junction(*id).is_some(),
            ObjectRef::Label(id) => schematic.label(*id).is_some(),
        });
    }

    /// Returns true when an object reference still exists in the schematic.
    pub fn contains_object(&self, object: ObjectRef) -> bool {
        match object {
            ObjectRef::Symbol(id) => self.schematic.symbol(id).is_some(),
            ObjectRef::Wire(id) => self.schematic.wire(id).is_some(),
            ObjectRef::Junction(id) => self.schematic.junction(id).is_some(),
            ObjectRef::Label(id) => self.schematic.label(id).is_some(),
        }
    }

    /// Finds the topmost object at a snapped grid position.
    pub fn object_at(&self, point: GridPos) -> Option<ObjectRef> {
        if let Some(label) = self
            .schematic
            .labels()
            .filter(|label| label.at == point)
            .max_by_key(|label| label.id)
        {
            return Some(ObjectRef::Label(label.id));
        }
        if let Some(junction) = self
            .schematic
            .junctions()
            .filter(|junction| junction.at == point)
            .max_by_key(|junction| junction.id)
        {
            return Some(ObjectRef::Junction(junction.id));
        }

        let symbol = self
            .schematic
            .symbols()
            .filter(|symbol| self.symbol_contains(symbol, point))
            .max_by_key(|symbol| symbol.id);
        if let Some(symbol) = symbol {
            return Some(ObjectRef::Symbol(symbol.id));
        }

        self.schematic
            .wires()
            .filter(|wire| wire.contains(point))
            .max_by_key(|wire| wire.id)
            .map(|wire| ObjectRef::Wire(wire.id))
    }

    /// Returns the bounding box of all placed objects in grid coordinates.
    pub fn content_bounds(&self) -> Option<(GridPos, GridPos)> {
        let mut min = GridPos::new(i32::MAX, i32::MAX);
        let mut max = GridPos::new(i32::MIN, i32::MIN);
        let mut empty = true;

        let mut include = |point: GridPos| {
            min.x = min.x.min(point.x);
            min.y = min.y.min(point.y);
            max.x = max.x.max(point.x);
            max.y = max.y.max(point.y);
            empty = false;
        };

        for symbol in self.schematic.symbols() {
            include(symbol.at);
            if let Some(definition) = self.registry.get(&symbol.kind) {
                for pin in &definition.pins {
                    if let Some(position) = symbol.pin_position_of(pin) {
                        include(position);
                    }
                }
            }
        }
        for wire in self.schematic.wires() {
            include(wire.a);
            include(wire.b);
        }
        for junction in self.schematic.junctions() {
            include(junction.at);
        }
        for label in self.schematic.labels() {
            include(label.at);
        }

        if empty {
            None
        } else {
            Some((min, max))
        }
    }

    /// Adds a built-in symbol with a generated unique label.
    pub fn add_symbol(&mut self, kind: SymbolKind, at: GridPos) -> Result<SymbolId, ActionError> {
        let definition = self
            .registry
            .get(&kind)
            .ok_or(ActionError::UnknownSymbol)?
            .clone();
        let label = self.next_label(&kind)?;
        let mut allocator = self.schematic.clone();
        let id = SymbolId(
            allocator
                .allocate_id()
                .map_err(|_| ActionError::IdSpaceExhausted)?,
        );
        let symbol = Symbol::from_def(id, &definition, at, label, Orientation::R0, false);
        self.execute(Command::AddSymbol(symbol))?;
        self.select_only(Some(ObjectRef::Symbol(id)));
        Ok(id)
    }

    /// Adds a rectilinear wire, splitting a diagonal request into two segments.
    pub fn add_wire(&mut self, start: GridPos, end: GridPos) -> Result<Vec<WireId>, ActionError> {
        if start == end {
            return Err(ActionError::ZeroLengthWire);
        }
        let segments = if start.x != end.x && start.y != end.y {
            let corner = GridPos::new(end.x, start.y);
            vec![(start, corner), (corner, end)]
        } else {
            vec![(start, end)]
        };
        let existing: BTreeSet<(GridPos, GridPos)> = self
            .schematic
            .wires()
            .map(|wire| (wire.a, wire.b))
            .collect();
        let mut allocator = self.schematic.clone();
        let mut commands = Vec::new();
        let mut ids = Vec::new();

        for (a, b) in segments {
            if a == b {
                continue;
            }
            let mut wire = Wire::new(WireId(0), a, b)?;
            if existing.contains(&(wire.a, wire.b)) {
                continue;
            }
            let id = WireId(
                allocator
                    .allocate_id()
                    .map_err(|_| ActionError::IdSpaceExhausted)?,
            );
            wire.id = id;
            ids.push(id);
            commands.push(Command::AddWire(wire));
        }

        if commands.is_empty() {
            return Err(ActionError::Duplicate);
        }
        self.execute(single_or_composite(commands))?;
        self.clear_selection();
        Ok(ids)
    }

    /// Adds an explicit junction dot.
    pub fn add_junction(&mut self, at: GridPos) -> Result<JunctionId, ActionError> {
        if self.schematic.junctions().any(|junction| junction.at == at) {
            return Err(ActionError::Duplicate);
        }
        let mut allocator = self.schematic.clone();
        let id = JunctionId(
            allocator
                .allocate_id()
                .map_err(|_| ActionError::IdSpaceExhausted)?,
        );
        self.execute(Command::AddJunction(Junction::new(id, at)))?;
        self.select_only(Some(ObjectRef::Junction(id)));
        Ok(id)
    }

    /// Adds a net label with a validated name.
    pub fn add_label(
        &mut self,
        at: GridPos,
        text: impl Into<String>,
    ) -> Result<NetLabelId, ActionError> {
        let temporary = NetLabel::new(NetLabelId(0), at, text)?;
        let mut allocator = self.schematic.clone();
        let id = NetLabelId(
            allocator
                .allocate_id()
                .map_err(|_| ActionError::IdSpaceExhausted)?,
        );
        let label = NetLabel::new(id, at, temporary.text)?;
        self.execute(Command::AddLabel(label))?;
        self.select_only(Some(ObjectRef::Label(id)));
        Ok(id)
    }

    /// Replaces a label with a newly validated name.
    pub fn update_label(
        &mut self,
        id: NetLabelId,
        text: impl Into<String>,
    ) -> Result<(), ActionError> {
        let old = self.label(id).ok_or(ActionError::MissingObject)?.clone();
        let new = NetLabel::new(id, old.at, text)?;
        if new == old {
            return Ok(());
        }
        self.execute(Command::ReplaceLabel { old, new })
    }

    /// Replaces a symbol with an edited version.
    pub fn update_symbol(&mut self, new: Symbol) -> Result<(), ActionError> {
        let old = self
            .symbol(new.id)
            .ok_or(ActionError::MissingObject)?
            .clone();
        if new == old {
            return Ok(());
        }
        self.execute(Command::ReplaceSymbol { old, new })
    }

    /// Rotates every selected symbol clockwise by one quarter turn.
    pub fn rotate_selection(&mut self) -> Result<(), ActionError> {
        let commands = self.symbol_replacements(|symbol| {
            let mut replacement = symbol.clone();
            replacement.orientation = replacement.orientation.next();
            replacement
        })?;
        self.execute(single_or_composite(commands))
    }

    /// Toggles the mirror flag of every selected symbol.
    pub fn mirror_selection(&mut self) -> Result<(), ActionError> {
        let commands = self.symbol_replacements(|symbol| {
            let mut replacement = symbol.clone();
            replacement.mirrored = !replacement.mirrored;
            replacement
        })?;
        self.execute(single_or_composite(commands))
    }

    /// Moves every selected object by a grid delta.
    pub fn move_selection(&mut self, delta: GridPos) -> Result<(), ActionError> {
        if self.selected.is_empty() {
            return Err(ActionError::EmptySelection);
        }
        if delta == GridPos::default() {
            return Ok(());
        }
        self.execute(Command::MoveObjects {
            objects: self.selected.iter().copied().collect(),
            delta,
        })
    }

    /// Deletes every selected object as one atomic command.
    pub fn delete_selection(&mut self) -> Result<(), ActionError> {
        if self.selected.is_empty() {
            return Err(ActionError::EmptySelection);
        }
        let mut commands = Vec::new();
        for object in &self.selected {
            match object {
                ObjectRef::Symbol(id) => {
                    if let Some(symbol) = self.symbol(*id) {
                        commands.push(Command::RemoveSymbol(symbol.clone()));
                    }
                }
                ObjectRef::Wire(id) => {
                    if let Some(wire) = self.wire(*id) {
                        commands.push(Command::RemoveWire(wire.clone()));
                    }
                }
                ObjectRef::Junction(id) => {
                    if let Some(junction) = self.junction(*id) {
                        commands.push(Command::RemoveJunction(junction.clone()));
                    }
                }
                ObjectRef::Label(id) => {
                    if let Some(label) = self.label(*id) {
                        commands.push(Command::RemoveLabel(label.clone()));
                    }
                }
            }
        }
        if commands.is_empty() {
            return Err(ActionError::EmptySelection);
        }
        self.execute(single_or_composite(commands))?;
        self.clear_selection();
        Ok(())
    }

    /// Removes all editable content as one undoable command.
    pub fn clear(&mut self) -> Result<(), ActionError> {
        let mut commands = Vec::new();
        commands.extend(self.schematic.symbols().cloned().map(Command::RemoveSymbol));
        commands.extend(self.schematic.wires().cloned().map(Command::RemoveWire));
        commands.extend(
            self.schematic
                .junctions()
                .cloned()
                .map(Command::RemoveJunction),
        );
        commands.extend(self.schematic.labels().cloned().map(Command::RemoveLabel));
        if commands.is_empty() {
            return Ok(());
        }
        self.execute(single_or_composite(commands))
    }

    /// Undoes the most recent command.
    pub fn undo(&mut self) -> Result<bool, ActionError> {
        let changed = self.history.undo(&mut self.schematic)?;
        if changed {
            self.after_change();
        }
        Ok(changed)
    }

    /// Redoes the most recently undone command.
    pub fn redo(&mut self) -> Result<bool, ActionError> {
        let changed = self.history.redo(&mut self.schematic)?;
        if changed {
            self.after_change();
        }
        Ok(changed)
    }

    fn execute(&mut self, command: Command) -> Result<(), ActionError> {
        self.history.execute(&mut self.schematic, command)?;
        self.after_change();
        Ok(())
    }

    fn after_change(&mut self) {
        self.revision = self.revision.saturating_add(1);
        self.dirty = true;
        self.prune_selection();
        self.refresh_extraction();
    }

    fn refresh_extraction(&mut self) {
        match extract_netlist(&self.schematic, &self.registry) {
            Ok(extraction) => {
                self.extraction = Some(extraction);
                self.extraction_error = None;
            }
            Err(error) => {
                self.extraction = None;
                self.extraction_error = Some(error.to_string());
            }
        }
    }

    fn symbol_replacements(
        &self,
        mut edit: impl FnMut(&Symbol) -> Symbol,
    ) -> Result<Vec<Command>, ActionError> {
        let mut commands = Vec::new();
        for symbol in self.selected.iter().filter_map(|object| match object {
            ObjectRef::Symbol(id) => self.symbol(*id),
            ObjectRef::Wire(_) | ObjectRef::Junction(_) | ObjectRef::Label(_) => None,
        }) {
            let new = edit(symbol);
            if new != *symbol {
                commands.push(Command::ReplaceSymbol {
                    old: symbol.clone(),
                    new,
                });
            }
        }
        if commands.is_empty() {
            Err(ActionError::NoSymbolSelected)
        } else {
            Ok(commands)
        }
    }

    fn next_label(&self, kind: &SymbolKind) -> Result<String, ActionError> {
        let prefix = label_prefix(kind);
        let mut index = 1_u32;
        loop {
            let candidate = format!("{prefix}{index}");
            if !self
                .schematic
                .symbols()
                .any(|symbol| symbol.label == candidate)
            {
                return Ok(candidate);
            }
            index = index.checked_add(1).ok_or(ActionError::IdSpaceExhausted)?;
        }
    }

    fn symbol_contains(&self, symbol: &Symbol, point: GridPos) -> bool {
        if symbol.kind.is_ground() {
            return point.x >= symbol.at.x.saturating_sub(1)
                && point.x <= symbol.at.x.saturating_add(1)
                && point.y >= symbol.at.y.saturating_sub(1)
                && point.y <= symbol.at.y.saturating_add(2);
        }

        let Some(definition) = self.registry.get(&symbol.kind) else {
            return point.x.abs_diff(symbol.at.x) <= 2 && point.y.abs_diff(symbol.at.y) <= 2;
        };
        let mut min = symbol.at;
        let mut max = symbol.at;
        for pin in &definition.pins {
            if let Some(position) = symbol.pin_position_of(pin) {
                min.x = min.x.min(position.x);
                min.y = min.y.min(position.y);
                max.x = max.x.max(position.x);
                max.y = max.y.max(position.y);
            }
        }
        min.x = min.x.saturating_sub(1);
        min.y = min.y.saturating_sub(1);
        max.x = max.x.saturating_add(1);
        max.y = max.y.saturating_add(1);
        point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

fn set_orientation(
    document: &mut Document,
    id: SymbolId,
    orientation: Orientation,
) -> Result<(), ActionError> {
    let mut symbol = document
        .symbol(id)
        .ok_or(ActionError::MissingObject)?
        .clone();
    symbol.orientation = orientation;
    document.update_symbol(symbol)
}

fn single_or_composite(mut commands: Vec<Command>) -> Command {
    if commands.len() == 1 {
        commands.remove(0)
    } else {
        Command::Composite(commands)
    }
}

fn label_prefix(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Resistor => "R",
        SymbolKind::Capacitor => "C",
        SymbolKind::Inductor => "L",
        SymbolKind::VoltageSource => "V",
        SymbolKind::CurrentSource => "I",
        SymbolKind::Ground => "GND",
        SymbolKind::Switch => "SW",
        SymbolKind::Diode => "D",
        SymbolKind::Led => "LED",
        SymbolKind::Zener => "DZ",
        SymbolKind::OpAmp => "U",
        SymbolKind::Custom(_) => "X",
    }
}

/// Returns the display unit for a parameter, when one is known.
pub fn parameter_unit(kind: &SymbolKind, key: &str) -> Option<sim_core::units::Unit> {
    use sim_core::units::Unit;

    match (kind, key) {
        (SymbolKind::Resistor, "resistance") | (SymbolKind::Switch, "ron" | "roff") => {
            Some(Unit::Ohm)
        }
        (SymbolKind::Capacitor, "capacitance") => Some(Unit::Farad),
        (SymbolKind::Inductor, "inductance") => Some(Unit::Henry),
        (SymbolKind::VoltageSource, "dc")
        | (SymbolKind::Zener, "bv")
        | (SymbolKind::OpAmp, "v_sat_pos" | "v_sat_neg") => Some(Unit::Volt),
        (SymbolKind::CurrentSource, "dc")
        | (SymbolKind::Diode | SymbolKind::Led | SymbolKind::Zener, "is" | "ibv") => {
            Some(Unit::Ampere)
        }
        _ => None,
    }
}

/// Formats a numeric parameter for an editable text field.
pub fn format_parameter(value: f64, unit: Option<sim_core::units::Unit>) -> String {
    if let Some(unit) = unit {
        sim_core::units::format_si_precision(value, unit, 4)
    } else {
        let formatted = sim_core::units::format_si_precision(value, sim_core::units::Unit::Volt, 4);
        formatted
            .strip_suffix(" V")
            .unwrap_or(&formatted)
            .trim()
            .to_owned()
    }
}

/// Parses an editable parameter value using engineering notation.
pub fn parse_parameter(text: &str) -> Result<f64, sim_core::units::UnitParseError> {
    let value = sim_core::units::parse_si(text)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(sim_core::units::UnitParseError::NotFinite)
    }
}

/// Returns a concise French message for a parameter parsing error.
pub fn parameter_error_message(error: &UnitParseError) -> String {
    match error {
        UnitParseError::Empty => "La valeur est vide.".to_owned(),
        UnitParseError::InvalidNumber(_) => {
            "Le nombre est invalide. Exemples : 4.7k, 2.2u, 1e-6.".to_owned()
        }
        UnitParseError::MismatchedUnit { expected, found } => format!(
            "L'unité attendue est {}, mais {} a été fournie.",
            expected.symbol(),
            found.symbol()
        ),
        UnitParseError::NotFinite => "La valeur doit être finie.".to_owned(),
        UnitParseError::Overflow => "La valeur dépasse la plage représentable.".to_owned(),
    }
}

/// Returns the localized label of a known parameter.
pub fn parameter_label(key: &str) -> &str {
    match key {
        "resistance" => "Résistance",
        "capacitance" => "Capacité",
        "inductance" => "Inductance",
        "dc" => "Tension ou courant DC",
        "ron" => "Résistance fermée",
        "roff" => "Résistance ouverte",
        "closed" => "Fermé",
        "is" => "Courant de saturation",
        "n" => "Coefficient d'émission",
        "bv" => "Tension de claquage",
        "ibv" => "Courant de claquage",
        "nbv" => "Coefficient de claquage",
        "gain" => "Gain",
        "v_sat_pos" => "Saturation positive",
        "v_sat_neg" => "Saturation négative",
        other => other,
    }
}

/// Returns true when a parameter holds a numeric value.
pub fn is_numeric_parameter(value: &ParamValue) -> bool {
    matches!(value, ParamValue::Number(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_no_state_change(
        document: &mut Document,
        action: impl FnOnce(&mut Document) -> Result<(), ActionError>,
    ) {
        let before = document.schematic().clone();
        let revision = document.revision();
        let next_id = document.schematic().next_id();
        assert!(action(document).is_err());
        assert!(document.schematic().content_eq(&before));
        assert_eq!(document.revision(), revision);
        assert_eq!(document.schematic().next_id(), next_id);
    }

    #[test]
    fn demo_extracts_a_clean_netlist() {
        let document = Document::demo().unwrap();
        let extraction = document.extraction().unwrap();
        assert!(extraction.erc.is_empty());
        assert_eq!(extraction.netlist.nodes.len(), 3);
        assert_eq!(extraction.netlist.components.len(), 4);
        assert!(document.extraction_error().is_none());
    }

    #[test]
    fn generated_symbol_labels_are_unique_and_deterministic() {
        let mut document = Document::new();
        let first = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        let second = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(10, 0))
            .unwrap();
        assert_eq!(document.symbol(first).map(|s| s.label.as_str()), Some("R1"));
        assert_eq!(
            document.symbol(second).map(|s| s.label.as_str()),
            Some("R2")
        );
    }

    #[test]
    fn diagonal_wire_request_creates_two_orthogonal_segments() {
        let mut document = Document::new();
        let ids = document
            .add_wire(GridPos::new(0, 0), GridPos::new(6, 4))
            .unwrap();
        assert_eq!(ids.len(), 2);
        let first = document.wire(ids[0]).unwrap();
        let second = document.wire(ids[1]).unwrap();
        assert!(first.is_horizontal());
        assert!(second.is_vertical());
        assert_eq!(first.b, GridPos::new(6, 0));
        assert_eq!(second.a, GridPos::new(6, 0));
        assert!(document.selection().is_empty());
    }

    #[test]
    fn wire_failure_does_not_consume_identifiers() {
        let mut document = Document::new();
        document
            .add_wire(GridPos::new(0, 0), GridPos::new(6, 0))
            .unwrap();
        assert_no_state_change(&mut document, |document| {
            document
                .add_wire(GridPos::new(6, 0), GridPos::new(0, 0))
                .map(|_| ())
        });
    }

    #[test]
    fn duplicate_junction_is_refused_atomically() {
        let mut document = Document::new();
        document.add_junction(GridPos::new(2, 3)).unwrap();
        assert_no_state_change(&mut document, |document| {
            document.add_junction(GridPos::new(2, 3)).map(|_| ())
        });
    }

    #[test]
    fn rotation_mirror_and_undo_restore_the_symbol() {
        let mut document = Document::new();
        let id = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        let original = document.symbol(id).unwrap().clone();
        document.select_only(Some(ObjectRef::Symbol(id)));
        document.rotate_selection().unwrap();
        assert_eq!(
            document.symbol(id).map(|symbol| symbol.orientation),
            Some(Orientation::R90)
        );
        document.mirror_selection().unwrap();
        assert!(document.symbol(id).is_some_and(|symbol| symbol.mirrored));
        document.undo().unwrap();
        assert!(!document.symbol(id).is_some_and(|symbol| symbol.mirrored));
        document.undo().unwrap();
        assert_eq!(document.symbol(id), Some(&original));
    }

    #[test]
    fn invalid_label_update_keeps_the_previous_name() {
        let mut document = Document::new();
        let id = document.add_label(GridPos::new(0, 0), "OUT").unwrap();
        assert!(document.update_label(id, "  ").is_err());
        assert_eq!(
            document.label(id).map(|label| label.text.as_str()),
            Some("OUT")
        );
    }

    #[test]
    fn selection_is_pruned_after_undo() {
        let mut document = Document::new();
        let id = document
            .add_symbol(SymbolKind::Diode, GridPos::new(0, 0))
            .unwrap();
        assert!(document.selection().contains(&ObjectRef::Symbol(id)));
        document.undo().unwrap();
        assert!(document.selection().is_empty());
    }

    #[test]
    fn clear_and_undo_restore_every_object() {
        let mut document = Document::demo().unwrap();
        let before = document.schematic().clone();
        document.clear().unwrap();
        assert!(document.schematic().is_empty());
        document.undo().unwrap();
        assert!(document.schematic().content_eq(&before));
    }

    #[test]
    fn hit_testing_prefers_labels_and_symbols() {
        let document = Document::demo().unwrap();
        assert!(matches!(
            document.object_at(GridPos::new(6, -8)),
            Some(ObjectRef::Label(_))
        ));
        assert!(matches!(
            document.object_at(GridPos::new(-2, -8)),
            Some(ObjectRef::Symbol(_))
        ));
        assert!(matches!(
            document.object_at(GridPos::new(-7, -8)),
            Some(ObjectRef::Wire(_))
        ));
    }

    #[test]
    fn parameter_parsing_and_formatting_round_trip_engineering_values() {
        let value = parse_parameter("4k7").unwrap();
        assert!((value - 4_700.0).abs() < 1e-9);
        assert_eq!(
            format_parameter(4_700.0, Some(sim_core::units::Unit::Ohm)),
            "4.7 kΩ"
        );
        assert_eq!(format_parameter(1.8, None), "1.8");
    }

    #[test]
    fn formatted_parameters_can_be_reparsed_without_changes() {
        let document = Document::demo().unwrap();
        for symbol in document.schematic().symbols() {
            for (key, value) in &symbol.params {
                let ParamValue::Number(number) = value else {
                    continue;
                };
                let text = format_parameter(*number, parameter_unit(&symbol.kind, key));
                let parsed = parse_parameter(&text).unwrap();
                assert!(
                    (parsed - number).abs() <= 1e-9 * number.abs().max(1.0),
                    "{text} did not round-trip for {key}"
                );
            }
        }
    }

    #[test]
    fn user_messages_are_french_and_actionable() {
        assert_eq!(
            ActionError::Name(NameError::Empty).user_message(),
            "Le nom ne peut pas être vide."
        );
        assert_eq!(
            ActionError::Wire(WireGeometryError::NotOrthogonal).user_message(),
            "Un fil doit être horizontal ou vertical."
        );
        assert!(parameter_error_message(&UnitParseError::NotFinite).contains("finie"));
    }
}
