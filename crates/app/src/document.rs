//! Editable document state shared by the application panels.

use std::collections::{BTreeMap, BTreeSet};

use editor_core::grid::segment_relation;
use editor_core::{
    extract_netlist, nearest_snap, route_wire, Command, EditError, ErcReport, ExtractedNet,
    Extraction, GridPos, GridRect, Junction, JunctionId, NetLabel, NetLabelId, ObjectRef,
    Orientation, PinKey, Schematic, SegmentRelation, SnapKind, SnapTarget, Symbol, SymbolId,
    SymbolKind, SymbolRegistry, UndoStack, Wire, WireGeometryError, WireId, DEFAULT_SNAP_RADIUS,
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
    /// The target wire cannot receive an in-series component.
    #[error("le composant ne peut pas être inséré sur ce fil")]
    CannotInsertOnWire,
    /// The wire move is diagonal or would duplicate another segment.
    #[error("ce segment ne peut pas être déplacé à cette position")]
    CannotMoveWire,
    /// The requested alignment needs at least two objects.
    #[error("l'alignement exige au moins deux objets")]
    NotEnoughObjects,
    /// The requested distribution needs at least three objects.
    #[error("la distribution exige au moins trois objets")]
    NotEnoughForDistribution,
    /// No electrical net was found at the requested position.
    #[error("aucun net électrique à cette position")]
    NoNet,
    /// The endpoint is connected and cannot be resized independently.
    #[error("cette extrémité est connectée")]
    CannotResizeWire,
    /// The selected symbol is not a switch.
    #[error("le symbole n'est pas un interrupteur")]
    NotASwitch,
    /// The cut point is not strictly inside the wire.
    #[error("le point de coupe doit être à l'intérieur du fil")]
    CannotSplitWire,
    /// The project JSON is invalid.
    #[error("le projet est invalide : {0}")]
    Project(#[from] editor_core::DocumentError),
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
            Self::CannotInsertOnWire => {
                "Ce fil est trop court ou contient une connexion incompatible.".to_owned()
            }
            Self::CannotMoveWire => {
                "Ce segment ne peut pas être déplacé dans cette topologie.".to_owned()
            }
            Self::NotEnoughObjects => "Sélectionnez au moins deux objets.".to_owned(),
            Self::NotEnoughForDistribution => {
                "Sélectionnez au moins trois objets pour distribuer.".to_owned()
            }
            Self::NoNet => "Aucun net électrique à cette position.".to_owned(),
            Self::CannotResizeWire => {
                "Cette extrémité est connectée ; déplacez le segment ou retirez la connexion."
                    .to_owned()
            }
            Self::NotASwitch => "Ce composant n'est pas un interrupteur.".to_owned(),
            Self::CannotSplitWire => {
                "Le point de coupe doit être strictement à l'intérieur du fil.".to_owned()
            }
            Self::Project(error) => format!("Le projet est invalide : {error}"),
        }
    }
}

/// Alignment applied to a selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    /// Align left edges.
    Left,
    /// Align horizontal centres.
    CenterX,
    /// Align right edges.
    Right,
    /// Align top edges.
    Top,
    /// Align vertical centres.
    CenterY,
    /// Align bottom edges.
    Bottom,
}

/// Axis used by distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistributionAxis {
    /// Distribute horizontally.
    Horizontal,
    /// Distribute vertically.
    Vertical,
}

/// Endpoint of a wire segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireEnd {
    /// First endpoint.
    A,
    /// Second endpoint.
    B,
}

/// Magnetic connection chosen while moving a selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionSnap {
    /// Delta after aligning a moved pin with the target.
    pub delta: GridPos,
    /// Geometry that attracted the pin.
    pub target: SnapTarget,
}

struct SeriesInsertion {
    wire: Wire,
    low: GridPos,
    high: GridPos,
}

/// Rectangle selection rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    /// Objects must lie entirely inside the rectangle.
    Enclosing,
    /// Objects may intersect the rectangle.
    Touching,
}

/// How a rectangle updates the current selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionChange {
    /// Replace the selection.
    Replace,
    /// Add matching objects to the selection.
    Add,
    /// Toggle matching objects.
    Toggle,
}

/// One object stored in the internal clipboard.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipboardItem {
    /// A placed symbol.
    Symbol(Symbol),
    /// A wire segment.
    Wire(Wire),
    /// An explicit junction.
    Junction(Junction),
    /// A net label.
    Label(NetLabel),
}

/// Internal clipboard with a normalized paste anchor.
#[derive(Clone, Debug, PartialEq)]
pub struct Clipboard {
    items: Vec<ClipboardItem>,
    anchor: GridPos,
}

impl Clipboard {
    /// Returns the copied objects in deterministic order.
    pub fn items(&self) -> &[ClipboardItem] {
        &self.items
    }

    /// Returns the top-left grid position of the copied content.
    pub const fn anchor(&self) -> GridPos {
        self.anchor
    }

    /// Returns true when nothing was copied.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
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
    clipboard: Option<Clipboard>,
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
            clipboard: None,
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
        set_number(&mut document, source, "dc", 10.0)?;
        document.add_symbol(SymbolKind::Ground, GridPos::new(-10, -1))?;
        let resistor = document.add_symbol(SymbolKind::Resistor, GridPos::new(-1, -10))?;
        set_number(&mut document, resistor, "resistance", 1_000.0)?;
        let capacitor = document.add_symbol(SymbolKind::Capacitor, GridPos::new(2, -6))?;
        set_orientation(&mut document, capacitor, Orientation::R90)?;
        set_number(&mut document, capacitor, "capacitance", 100e-6)?;
        let switch = document.add_symbol(SymbolKind::Switch, GridPos::new(-6, -10))?;
        set_bool(&mut document, switch, "closed", true)?;

        document.add_wire(GridPos::new(-10, -2), GridPos::new(-10, -1))?;
        document.add_wire(GridPos::new(-10, -4), GridPos::new(-10, -2))?;
        document.add_wire(GridPos::new(1, -10), GridPos::new(2, -10))?;
        document.add_wire(GridPos::new(2, -10), GridPos::new(2, -8))?;
        document.add_wire(GridPos::new(-10, -10), GridPos::new(-8, -10))?;
        document.add_wire(GridPos::new(-10, -10), GridPos::new(-10, -8))?;
        document.add_wire(GridPos::new(-10, -1), GridPos::new(2, -1))?;
        document.add_wire(GridPos::new(2, -4), GridPos::new(2, -1))?;
        document.add_wire(GridPos::new(-4, -10), GridPos::new(-3, -10))?;
        document.add_label(GridPos::new(2, -10), "Vc")?;
        finish_example(document)
    }

    /// Creates a resistive divider example.
    pub fn divider_example() -> Result<Self, ActionError> {
        let mut document = Self::new();
        let source = document.add_symbol(SymbolKind::VoltageSource, GridPos::new(3, -5))?;
        set_orientation(&mut document, source, Orientation::R180)?;
        document.add_symbol(SymbolKind::Ground, GridPos::new(-2, 7))?;
        let upper = document.add_symbol(SymbolKind::Resistor, GridPos::new(8, -2))?;
        set_orientation(&mut document, upper, Orientation::R90)?;
        set_number(&mut document, upper, "resistance", 1_000.0)?;
        let lower = document.add_symbol(SymbolKind::Resistor, GridPos::new(8, 4))?;
        set_orientation(&mut document, lower, Orientation::R90)?;
        set_number(&mut document, lower, "resistance", 1_000.0)?;

        document.add_wire(GridPos::new(-2, -5), GridPos::new(-2, 7))?;
        document.add_wire(GridPos::new(5, -5), GridPos::new(8, -5))?;
        document.add_wire(GridPos::new(8, 0), GridPos::new(8, 2))?;
        document.add_wire(GridPos::new(8, 6), GridPos::new(8, 7))?;
        document.add_wire(GridPos::new(-2, 7), GridPos::new(8, 7))?;
        document.add_wire(GridPos::new(8, -5), GridPos::new(8, -4))?;
        document.add_wire(GridPos::new(-2, -5), GridPos::new(1, -5))?;
        document.add_label(GridPos::new(8, 0), "OUT")?;
        finish_example(document)
    }

    /// Creates a LED and series resistor example.
    pub fn led_example() -> Result<Self, ActionError> {
        let mut document = Self::new();
        let source = document.add_symbol(SymbolKind::VoltageSource, GridPos::new(0, 0))?;
        set_orientation(&mut document, source, Orientation::R180)?;
        document.add_symbol(SymbolKind::Ground, GridPos::new(2, 4))?;
        document.add_symbol(SymbolKind::Resistor, GridPos::new(6, 0))?;
        document.add_symbol(SymbolKind::Led, GridPos::new(12, 0))?;
        document.add_wire(GridPos::new(-2, 0), GridPos::new(-2, 4))?;
        document.add_wire(GridPos::new(-2, 4), GridPos::new(2, 4))?;
        document.add_wire(GridPos::new(2, 0), GridPos::new(4, 0))?;
        document.add_wire(GridPos::new(8, 0), GridPos::new(10, 0))?;
        document.add_wire(GridPos::new(14, 0), GridPos::new(14, 4))?;
        document.add_wire(GridPos::new(14, 4), GridPos::new(2, 4))?;
        document.add_label(GridPos::new(8, 0), "OUT")?;
        finish_example(document)
    }

    /// Creates a switch and load example.
    pub fn switch_example() -> Result<Self, ActionError> {
        let mut document = Self::new();
        let source = document.add_symbol(SymbolKind::VoltageSource, GridPos::new(0, 0))?;
        set_orientation(&mut document, source, Orientation::R180)?;
        document.add_symbol(SymbolKind::Ground, GridPos::new(2, 4))?;
        document.add_symbol(SymbolKind::Resistor, GridPos::new(6, 0))?;
        document.add_symbol(SymbolKind::Switch, GridPos::new(12, 0))?;
        document.add_wire(GridPos::new(-2, 0), GridPos::new(-2, 4))?;
        document.add_wire(GridPos::new(-2, 4), GridPos::new(2, 4))?;
        document.add_wire(GridPos::new(2, 0), GridPos::new(4, 0))?;
        document.add_wire(GridPos::new(8, 0), GridPos::new(10, 0))?;
        document.add_wire(GridPos::new(14, 0), GridPos::new(14, 4))?;
        document.add_wire(GridPos::new(14, 4), GridPos::new(2, 4))?;
        document.add_label(GridPos::new(8, 0), "OUT")?;
        finish_example(document)
    }

    /// Creates a low-side N-MOSFET switch example with a gate switch.
    pub fn mosfet_example() -> Result<Self, ActionError> {
        let mut document = Self::new();
        let rail = document.add_symbol(SymbolKind::VoltageSource, GridPos::new(4, -2))?;
        set_orientation(&mut document, rail, Orientation::R90)?;
        set_number(&mut document, rail, "dc", 10.0)?;
        let drive = document.add_symbol(SymbolKind::VoltageSource, GridPos::new(2, 4))?;
        set_orientation(&mut document, drive, Orientation::R180)?;
        set_number(&mut document, drive, "dc", 5.0)?;
        let switch = document.add_symbol(SymbolKind::Switch, GridPos::new(7, 4))?;
        set_bool(&mut document, switch, "closed", true)?;
        let pull_down = document.add_symbol(SymbolKind::Resistor, GridPos::new(11, 6))?;
        set_orientation(&mut document, pull_down, Orientation::R90)?;
        set_number(&mut document, pull_down, "resistance", 100_000.0)?;
        let load = document.add_symbol(SymbolKind::Resistor, GridPos::new(16, -2))?;
        set_orientation(&mut document, load, Orientation::R90)?;
        set_number(&mut document, load, "resistance", 1_000.0)?;
        document.add_symbol(SymbolKind::Mosfet, GridPos::new(14, 4))?;
        document.add_symbol(SymbolKind::Ground, GridPos::new(0, 8))?;

        document.add_wire(GridPos::new(4, -4), GridPos::new(4, -6))?;
        document.add_wire(GridPos::new(4, -6), GridPos::new(16, -6))?;
        document.add_wire(GridPos::new(16, -6), GridPos::new(16, -4))?;
        document.add_wire(GridPos::new(16, 0), GridPos::new(16, 2))?;
        document.add_wire(GridPos::new(16, 6), GridPos::new(16, 8))?;
        document.add_wire(GridPos::new(0, 8), GridPos::new(11, 8))?;
        document.add_wire(GridPos::new(11, 8), GridPos::new(16, 8))?;
        document.add_wire(GridPos::new(4, 0), GridPos::new(0, 0))?;
        document.add_wire(GridPos::new(0, 0), GridPos::new(0, 4))?;
        document.add_wire(GridPos::new(0, 4), GridPos::new(0, 8))?;
        document.add_wire(GridPos::new(4, 4), GridPos::new(5, 4))?;
        document.add_wire(GridPos::new(9, 4), GridPos::new(11, 4))?;
        document.add_wire(GridPos::new(11, 4), GridPos::new(12, 4))?;
        document.add_label(GridPos::new(16, 0), "OUT")?;
        document.add_label(GridPos::new(12, 4), "GATE")?;
        finish_example(document)
    }

    /// Creates a series RLC example.
    pub fn rlc_example() -> Result<Self, ActionError> {
        let mut document = Self::new();
        let source = document.add_symbol(SymbolKind::VoltageSource, GridPos::new(0, 0))?;
        set_orientation(&mut document, source, Orientation::R180)?;
        document.add_symbol(SymbolKind::Ground, GridPos::new(-2, 8))?;
        let resistor = document.add_symbol(SymbolKind::Resistor, GridPos::new(6, 0))?;
        set_number(&mut document, resistor, "resistance", 1_000.0)?;
        let inductor = document.add_symbol(SymbolKind::Inductor, GridPos::new(12, 0))?;
        set_number(&mut document, inductor, "inductance", 1e-3)?;
        let capacitor = document.add_symbol(SymbolKind::Capacitor, GridPos::new(14, 4))?;
        set_orientation(&mut document, capacitor, Orientation::R90)?;
        set_number(&mut document, capacitor, "capacitance", 1e-6)?;

        document.add_wire(GridPos::new(-2, 0), GridPos::new(-2, 8))?;
        document.add_wire(GridPos::new(2, 0), GridPos::new(4, 0))?;
        document.add_wire(GridPos::new(8, 0), GridPos::new(10, 0))?;
        document.add_wire(GridPos::new(14, 0), GridPos::new(14, 2))?;
        document.add_wire(GridPos::new(14, 6), GridPos::new(14, 8))?;
        document.add_wire(GridPos::new(-2, 8), GridPos::new(14, 8))?;
        document.add_label(GridPos::new(14, 0), "OUT")?;
        finish_example(document)
    }

    /// Creates a switched parallel RLC example.
    pub fn parallel_rlc_example() -> Result<Self, ActionError> {
        let mut document = Self::new();
        document.add_symbol(SymbolKind::VoltageSource, GridPos::new(5, -7))?;
        let resistor = document.add_symbol(SymbolKind::Resistor, GridPos::new(-2, 0))?;
        set_orientation(&mut document, resistor, Orientation::R90)?;
        set_number(&mut document, resistor, "resistance", 10.0)?;
        let switch = document.add_symbol(SymbolKind::Switch, GridPos::new(-2, -5))?;
        set_orientation(&mut document, switch, Orientation::R90)?;
        let inductor = document.add_symbol(SymbolKind::Inductor, GridPos::new(5, -3))?;
        set_number(&mut document, inductor, "inductance", 1.0)?;
        let capacitor = document.add_symbol(SymbolKind::Capacitor, GridPos::new(14, 0))?;
        set_orientation(&mut document, capacitor, Orientation::R90)?;
        set_number(&mut document, capacitor, "capacitance", 15e-6)?;
        let load = document.add_symbol(SymbolKind::Resistor, GridPos::new(14, -5))?;
        set_orientation(&mut document, load, Orientation::R90)?;
        set_number(&mut document, load, "resistance", 100.0)?;
        document.add_symbol(SymbolKind::Ground, GridPos::new(10, -7))?;

        document.add_wire(GridPos::new(-2, 2), GridPos::new(-2, 4))?;
        document.add_wire(GridPos::new(-2, 4), GridPos::new(2, 4))?;
        document.add_wire(GridPos::new(2, 4), GridPos::new(14, 4))?;
        document.add_wire(GridPos::new(-2, -7), GridPos::new(3, -7))?;
        document.add_wire(GridPos::new(7, -7), GridPos::new(14, -7))?;
        document.add_wire(GridPos::new(-2, -3), GridPos::new(3, -3))?;
        document.add_wire(GridPos::new(7, -3), GridPos::new(14, -3))?;
        document.add_wire(GridPos::new(14, 2), GridPos::new(14, 4))?;
        document.add_wire(GridPos::new(14, -3), GridPos::new(14, -2))?;
        document.add_wire(GridPos::new(-2, -3), GridPos::new(-2, -2))?;
        finish_example(document)
    }

    /// Returns the current schematic.
    pub const fn schematic(&self) -> &Schematic {
        &self.schematic
    }

    /// Serializes the project to versioned JSON.
    pub fn to_project_json(&self) -> Result<String, ActionError> {
        Ok(editor_core::to_json(&self.schematic)?)
    }

    /// Loads a project from versioned JSON and revalidates it.
    pub fn from_project_json(json: &str) -> Result<Self, ActionError> {
        let schematic = editor_core::from_json(json)?;
        let mut document = Self::new();
        document.schematic = schematic;
        document.refresh_extraction();
        document.history.clear();
        document.selected.clear();
        document.dirty = false;
        document.revision = 0;
        Ok(document)
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

    /// Returns the named history entries, applied first.
    pub fn history_items(&self) -> Vec<editor_core::HistoryItem> {
        self.history.history()
    }

    /// Returns the number of currently applied commands.
    pub fn history_position(&self) -> usize {
        self.history.undo_len()
    }

    /// Jumps to a named history state.
    pub fn undo_to(&mut self, applied_commands: usize) -> Result<bool, ActionError> {
        let changed = self
            .history
            .undo_to(&mut self.schematic, applied_commands)?;
        if changed {
            self.after_change();
        }
        Ok(changed)
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

    /// Returns the internal clipboard, if any.
    pub const fn clipboard(&self) -> Option<&Clipboard> {
        self.clipboard.as_ref()
    }

    /// Selects objects inside a grid rectangle.
    pub fn select_in_rect(
        &mut self,
        a: GridPos,
        b: GridPos,
        mode: SelectionMode,
        change: SelectionChange,
    ) -> usize {
        let min = GridPos::new(a.x.min(b.x), a.y.min(b.y));
        let max = GridPos::new(a.x.max(b.x), a.y.max(b.y));
        let mut hits = BTreeSet::new();
        for object in self.schematic.object_refs() {
            let Some((object_min, object_max)) = self.object_selection_bounds(object) else {
                continue;
            };
            let matched = match mode {
                SelectionMode::Enclosing => {
                    object_min.x >= min.x
                        && object_min.y >= min.y
                        && object_max.x <= max.x
                        && object_max.y <= max.y
                }
                SelectionMode::Touching => {
                    object_min.x <= max.x
                        && object_max.x >= min.x
                        && object_min.y <= max.y
                        && object_max.y >= min.y
                }
            };
            if matched {
                hits.insert(object);
            }
        }
        match change {
            SelectionChange::Replace => self.selected = hits.clone(),
            SelectionChange::Add => self.selected.extend(hits.iter().copied()),
            SelectionChange::Toggle => {
                for object in &hits {
                    self.toggle_selection(*object);
                }
            }
        }
        hits.len()
    }

    /// Copies the selection into the internal clipboard.
    pub fn copy_selection(&mut self) -> Result<usize, ActionError> {
        if self.selected.is_empty() {
            return Err(ActionError::EmptySelection);
        }
        let mut items = Vec::with_capacity(self.selected.len());
        let mut anchor: Option<GridPos> = None;
        for object in &self.selected {
            let item = self
                .clipboard_item(*object)
                .ok_or(ActionError::MissingObject)?;
            let position = self
                .clipboard_anchor(*object)
                .ok_or(ActionError::MissingObject)?;
            anchor = Some(anchor.map_or(position, |current| {
                GridPos::new(current.x.min(position.x), current.y.min(position.y))
            }));
            items.push(item);
        }
        let Some(anchor) = anchor else {
            return Err(ActionError::EmptySelection);
        };
        let count = items.len();
        self.clipboard = Some(Clipboard { items, anchor });
        Ok(count)
    }

    /// Pastes the clipboard with its anchor at the requested grid position.
    pub fn paste_clipboard(&mut self, at: GridPos) -> Result<usize, ActionError> {
        let clipboard = self.clipboard.clone().ok_or(ActionError::EmptySelection)?;
        let delta = at
            .checked_sub(clipboard.anchor)
            .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
        let mut allocator = self.schematic.clone();
        let mut used_labels = self
            .schematic
            .symbols()
            .map(|symbol| symbol.label.clone())
            .collect::<BTreeSet<_>>();
        let mut commands = Vec::new();
        let mut pasted = BTreeSet::new();
        for item in clipboard.items {
            match item {
                ClipboardItem::Symbol(symbol) => {
                    let mut new = symbol.clone();
                    new.id = SymbolId(
                        allocator
                            .allocate_id()
                            .map_err(|_| ActionError::IdSpaceExhausted)?,
                    );
                    new.at = symbol
                        .at
                        .checked_add(delta)
                        .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
                    new.label = next_label_with(&new.kind, &mut used_labels)?;
                    pasted.insert(ObjectRef::Symbol(new.id));
                    commands.push(Command::AddSymbol(new));
                }
                ClipboardItem::Wire(wire) => {
                    let id = WireId(
                        allocator
                            .allocate_id()
                            .map_err(|_| ActionError::IdSpaceExhausted)?,
                    );
                    let a = wire
                        .a
                        .checked_add(delta)
                        .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
                    let b = wire
                        .b
                        .checked_add(delta)
                        .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
                    let new = Wire::new(id, a, b)?;
                    pasted.insert(ObjectRef::Wire(id));
                    commands.push(Command::AddWire(new));
                }
                ClipboardItem::Junction(junction) => {
                    let id = JunctionId(
                        allocator
                            .allocate_id()
                            .map_err(|_| ActionError::IdSpaceExhausted)?,
                    );
                    let at = junction
                        .at
                        .checked_add(delta)
                        .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
                    if self.schematic.junctions().any(|existing| existing.at == at) {
                        return Err(ActionError::Duplicate);
                    }
                    pasted.insert(ObjectRef::Junction(id));
                    commands.push(Command::AddJunction(Junction::new(id, at)));
                }
                ClipboardItem::Label(label) => {
                    let id = NetLabelId(
                        allocator
                            .allocate_id()
                            .map_err(|_| ActionError::IdSpaceExhausted)?,
                    );
                    let at = label
                        .at
                        .checked_add(delta)
                        .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
                    let new = NetLabel::new(id, at, label.text)?;
                    pasted.insert(ObjectRef::Label(id));
                    commands.push(Command::AddLabel(new));
                }
            }
        }
        if commands.is_empty() {
            return Err(ActionError::EmptySelection);
        }
        let count = commands.len();
        self.execute(single_or_composite(commands))?;
        self.selected = pasted;
        Ok(count)
    }

    /// Copies and immediately pastes the selection with a small offset.
    pub fn duplicate_selection(&mut self) -> Result<usize, ActionError> {
        self.copy_selection()?;
        let anchor = self
            .clipboard
            .as_ref()
            .map(Clipboard::anchor)
            .ok_or(ActionError::EmptySelection)?;
        let at = anchor
            .checked_add(GridPos::new(2, 2))
            .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
        self.paste_clipboard(at)
    }

    fn clipboard_item(&self, object: ObjectRef) -> Option<ClipboardItem> {
        match object {
            ObjectRef::Symbol(id) => self.symbol(id).cloned().map(ClipboardItem::Symbol),
            ObjectRef::Wire(id) => self.wire(id).cloned().map(ClipboardItem::Wire),
            ObjectRef::Junction(id) => self.junction(id).cloned().map(ClipboardItem::Junction),
            ObjectRef::Label(id) => self.label(id).cloned().map(ClipboardItem::Label),
        }
    }

    fn clipboard_anchor(&self, object: ObjectRef) -> Option<GridPos> {
        match object {
            ObjectRef::Symbol(id) => self.symbol(id).map(|symbol| symbol.at),
            ObjectRef::Wire(id) => self.wire(id).map(|wire| wire.a),
            ObjectRef::Junction(id) => self.junction(id).map(|junction| junction.at),
            ObjectRef::Label(id) => self.label(id).map(|label| label.at),
        }
    }

    fn object_selection_bounds(&self, object: ObjectRef) -> Option<(GridPos, GridPos)> {
        self.object_bounds(object)
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
        self.add_symbol_oriented(kind, at, Orientation::R0, false)
    }

    /// Adds a symbol with an explicit orientation and mirror state.
    pub fn add_symbol_oriented(
        &mut self,
        kind: SymbolKind,
        at: GridPos,
        orientation: Orientation,
        mirrored: bool,
    ) -> Result<SymbolId, ActionError> {
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
        let symbol = Symbol::from_def(id, &definition, at, label, orientation, mirrored);
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

    /// Toggles a switch and returns its new closed state.
    pub fn toggle_switch(&mut self, id: SymbolId) -> Result<bool, ActionError> {
        let old = self.symbol(id).ok_or(ActionError::MissingObject)?.clone();
        if old.kind != SymbolKind::Switch {
            return Err(ActionError::NotASwitch);
        }
        let closed = old
            .params
            .get("closed")
            .and_then(ParamValue::as_bool)
            .unwrap_or(false);
        let mut new = old.clone();
        new.params
            .insert("closed".to_owned(), ParamValue::Bool(!closed));
        self.update_symbol(new)?;
        Ok(!closed)
    }

    /// Rotates every selected object clockwise around the selection centre.
    pub fn rotate_selection(&mut self) -> Result<(), ActionError> {
        let commands = self.group_transform(GroupTransform::Rotate)?;
        if commands.is_empty() {
            return Ok(());
        }
        self.execute(single_or_composite(commands))
    }

    /// Mirrors every selected object across the selection vertical centre.
    pub fn mirror_selection(&mut self) -> Result<(), ActionError> {
        let commands = self.group_transform(GroupTransform::Mirror)?;
        if commands.is_empty() {
            return Ok(());
        }
        self.execute(single_or_composite(commands))
    }

    fn group_transform(&self, transform: GroupTransform) -> Result<Vec<Command>, ActionError> {
        if self.selected.is_empty() {
            return Err(ActionError::EmptySelection);
        }
        let (min, max) = self.selection_bounds().ok_or(ActionError::EmptySelection)?;
        let center = GridPos::new((min.x + max.x).div_euclid(2), (min.y + max.y).div_euclid(2));
        let mut commands = Vec::new();
        for object in &self.selected {
            match object {
                ObjectRef::Symbol(id) => {
                    let symbol = self.symbol(*id).ok_or(ActionError::MissingObject)?;
                    let mut new = symbol.clone();
                    new.at = transform
                        .point(symbol.at, center)
                        .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
                    match transform {
                        GroupTransform::Rotate => new.orientation = new.orientation.next(),
                        GroupTransform::Mirror => new.mirrored = !new.mirrored,
                    }
                    if new != *symbol {
                        commands.push(Command::ReplaceSymbol {
                            old: symbol.clone(),
                            new,
                        });
                    }
                }
                ObjectRef::Wire(id) => {
                    let wire = self.wire(*id).ok_or(ActionError::MissingObject)?;
                    let a = transform
                        .point(wire.a, center)
                        .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
                    let b = transform
                        .point(wire.b, center)
                        .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
                    let new = Wire::new(wire.id, a, b)?;
                    if new != *wire {
                        commands.push(Command::ReplaceWire {
                            old: wire.clone(),
                            new,
                        });
                    }
                }
                ObjectRef::Junction(id) => {
                    let junction = self.junction(*id).ok_or(ActionError::MissingObject)?;
                    let at = transform
                        .point(junction.at, center)
                        .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
                    let new = Junction::new(junction.id, at);
                    if new != *junction {
                        commands.push(Command::ReplaceJunction {
                            old: junction.clone(),
                            new,
                        });
                    }
                }
                ObjectRef::Label(id) => {
                    let label = self.label(*id).ok_or(ActionError::MissingObject)?;
                    let at = transform
                        .point(label.at, center)
                        .ok_or(ActionError::Edit(EditError::PositionOverflow))?;
                    let new = NetLabel::new(label.id, at, label.text.clone())?;
                    if new != *label {
                        commands.push(Command::ReplaceLabel {
                            old: label.clone(),
                            new,
                        });
                    }
                }
            }
        }
        Ok(commands)
    }

    /// Returns wires carrying at least one non-information ERC finding.
    pub fn problem_wires(&self) -> BTreeSet<WireId> {
        let mut wires = BTreeSet::new();
        let Some(report) = self.erc() else {
            return wires;
        };
        for issue in report.issues() {
            if issue.severity == editor_core::ErcSeverity::Info {
                continue;
            }
            if let Some(ObjectRef::Wire(id)) = issue.subject {
                wires.insert(id);
            } else if let Some(at) = issue.at {
                for wire in self.schematic.wires() {
                    if wire.contains(at) {
                        wires.insert(wire.id);
                    }
                }
            }
        }
        wires
    }

    /// Returns the bounds of the current selection.
    pub fn selection_bounds(&self) -> Option<(GridPos, GridPos)> {
        let mut min: Option<GridPos> = None;
        let mut max: Option<GridPos> = None;
        for object in &self.selected {
            let Some((object_min, object_max)) = self.object_bounds(*object) else {
                continue;
            };
            min = Some(min.map_or(object_min, |current| {
                GridPos::new(current.x.min(object_min.x), current.y.min(object_min.y))
            }));
            max = Some(max.map_or(object_max, |current| {
                GridPos::new(current.x.max(object_max.x), current.y.max(object_max.y))
            }));
        }
        Some((min?, max?))
    }

    /// Moves every selected object by a grid delta.
    ///
    /// Wires attached to moved symbols follow their pins and receive an
    /// orthogonal bend when the pin leaves the original axis.
    pub fn move_selection(&mut self, delta: GridPos) -> Result<(), ActionError> {
        if self.selected.is_empty() {
            return Err(ActionError::EmptySelection);
        }
        if delta == GridPos::default() {
            return Ok(());
        }
        let moved_pins = self.moved_pin_map(delta);
        let mut commands = vec![Command::MoveObjects {
            objects: self.selected.iter().copied().collect(),
            delta,
        }];
        if !moved_pins.is_empty() {
            commands.extend(self.followed_wire_commands(&moved_pins)?);
        }
        self.execute(single_or_composite(commands))
    }

    fn moved_pin_map(&self, delta: GridPos) -> BTreeMap<GridPos, GridPos> {
        let mut moved = BTreeMap::new();
        for object in &self.selected {
            let ObjectRef::Symbol(id) = object else {
                continue;
            };
            let Some(symbol) = self.symbol(*id) else {
                continue;
            };
            let Some(definition) = self.registry.get(&symbol.kind) else {
                continue;
            };
            for pin in &definition.pins {
                let Some(old) = symbol.pin_position_of(pin) else {
                    continue;
                };
                if let Some(new) = old.checked_add(delta) {
                    moved.insert(old, new);
                }
            }
        }
        moved
    }

    /// Returns the magnetic connection a selection move would create.
    ///
    /// Only pins of selected symbols attract the selection. Wires and pins on
    /// the original nets are ignored so the move can leave them behind.
    pub fn selection_snap(&self, delta: GridPos) -> Option<SelectionSnap> {
        if delta == GridPos::default() {
            return None;
        }
        let moved_pins = self.moved_pin_map(delta);
        if moved_pins.is_empty() {
            return None;
        }
        let selected_symbols = self
            .selected
            .iter()
            .filter_map(|object| match object {
                ObjectRef::Symbol(id) => Some(*id),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let selected_wires = self
            .selected
            .iter()
            .filter_map(|object| match object {
                ObjectRef::Wire(id) => Some(*id),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let attached_wires = self
            .schematic
            .wires()
            .filter(|wire| moved_pins.keys().any(|point| wire.contains(*point)))
            .map(|wire| wire.id)
            .collect::<BTreeSet<_>>();
        let all_pins = self.pin_positions();
        let old_net_nodes = self
            .extraction
            .as_ref()
            .map_or_else(BTreeSet::new, |extraction| {
                all_pins
                    .iter()
                    .filter(|(key, position)| {
                        selected_symbols.contains(&key.symbol) && moved_pins.contains_key(position)
                    })
                    .filter_map(|(key, _)| extraction.net_of_pin.get(key).copied())
                    .collect()
            });
        let old_net_wires = self.original_net_wires(&moved_pins);
        let pins = all_pins
            .into_iter()
            .filter(|(key, _)| !selected_symbols.contains(&key.symbol))
            .filter(|(key, _)| {
                self.extraction.as_ref().is_none_or(|extraction| {
                    extraction
                        .net_of_pin
                        .get(key)
                        .is_none_or(|node| !old_net_nodes.contains(node))
                })
            })
            .collect::<Vec<_>>();
        let wires = self
            .schematic
            .wires()
            .filter(|wire| {
                !selected_wires.contains(&wire.id)
                    && !attached_wires.contains(&wire.id)
                    && !old_net_wires.contains(&wire.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut best: Option<(i64, GridPos, SnapTarget, GridPos)> = None;
        for (old, position) in &moved_pins {
            let Some(target) = nearest_snap(*position, &pins, &wires, DEFAULT_SNAP_RADIUS) else {
                continue;
            };
            let distance = squared_grid_distance(*position, target.at);
            let replace = best.is_none_or(|(best_distance, best_old, _, _)| {
                (distance, *old) < (best_distance, best_old)
            });
            if replace {
                best = Some((distance, *old, target, *position));
            }
        }
        let (_, _, target, position) = best?;
        let adjustment = target.at.checked_sub(position)?;
        Some(SelectionSnap {
            delta: delta.checked_add(adjustment)?,
            target,
        })
    }

    fn original_net_wires(&self, moved_pins: &BTreeMap<GridPos, GridPos>) -> BTreeSet<WireId> {
        let Some(extraction) = self.extraction.as_ref() else {
            return BTreeSet::new();
        };
        let selected_symbols = self
            .selected
            .iter()
            .filter_map(|object| match object {
                ObjectRef::Symbol(id) => Some(*id),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let nodes = self
            .pin_positions()
            .into_iter()
            .filter(|(key, position)| {
                selected_symbols.contains(&key.symbol) && moved_pins.contains_key(position)
            })
            .filter_map(|(key, _)| extraction.net_of_pin.get(&key).copied())
            .collect::<BTreeSet<_>>();
        extraction
            .nets
            .iter()
            .filter(|net| nodes.contains(&net.node))
            .flat_map(|net| net.wires.iter().copied())
            .collect()
    }

    fn series_insertion_after_move(
        &self,
        moved_pins: &BTreeMap<GridPos, GridPos>,
    ) -> Option<SeriesInsertion> {
        if self.selected.len() != 1 {
            return None;
        }
        let ObjectRef::Symbol(id) = *self.selected.iter().next()? else {
            return None;
        };
        let symbol = self.symbol(id)?;
        let definition = self.registry.get(&symbol.kind)?;
        if definition.pins.len() != 2 {
            return None;
        }
        let mut positions = Vec::with_capacity(2);
        for pin in &definition.pins {
            let old = symbol.pin_position_of(pin)?;
            positions.push(*moved_pins.get(&old)?);
        }
        positions.sort_by_key(|position| (position.x, position.y));
        let (low, high) = (positions[0], positions[1]);
        if low == high {
            return None;
        }
        let old_net_wires = self.original_net_wires(moved_pins);
        let mut candidates = self
            .schematic
            .wires()
            .filter(|wire| !old_net_wires.contains(&wire.id))
            .filter(|wire| !moved_pins.keys().any(|point| wire.contains(*point)))
            .filter(|wire| wire.contains(low) && wire.contains(high))
            .filter(|_| {
                !self
                    .schematic
                    .labels()
                    .any(|label| strictly_between(label.at, low, high))
                    && !self
                        .schematic
                        .junctions()
                        .any(|junction| strictly_between(junction.at, low, high))
            })
            .cloned()
            .collect::<Vec<_>>();
        candidates.sort_by_key(|wire| wire.id);
        candidates
            .into_iter()
            .next()
            .map(|wire| SeriesInsertion { wire, low, high })
    }

    /// Moves the selection, snapping a pin onto nearby wire or pin geometry.
    ///
    /// When a moved pin attaches outside the selection, the wires that used to
    /// follow the moved symbols stay in place and become disconnected. A
    /// two-terminal symbol covering a wire is inserted in series like a new
    /// placement.
    pub fn move_selection_snapped(
        &mut self,
        delta: GridPos,
    ) -> Result<Option<SnapTarget>, ActionError> {
        if self.selected.is_empty() {
            return Err(ActionError::EmptySelection);
        }
        let snap = self.selection_snap(delta);
        let applied = snap.map_or(delta, |snap| snap.delta);
        if applied == GridPos::default() {
            return Ok(None);
        }
        let moved_pins = self.moved_pin_map(applied);
        let series = self.series_insertion_after_move(&moved_pins);
        let series_target = series.as_ref().map(|series| SnapTarget {
            at: series.low,
            kind: SnapKind::Wire(series.wire.id),
        });
        let mut commands = vec![Command::MoveObjects {
            objects: self.selected.iter().copied().collect(),
            delta: applied,
        }];
        if let Some(series) = &series {
            commands.push(Command::RemoveWire(series.wire.clone()));
            let mut allocator = self.schematic.clone();
            for (start, end) in [(series.wire.a, series.low), (series.high, series.wire.b)] {
                if start == end {
                    continue;
                }
                let id = WireId(
                    allocator
                        .allocate_id()
                        .map_err(|_| ActionError::IdSpaceExhausted)?,
                );
                commands.push(Command::AddWire(Wire::new(id, start, end)?));
            }
        } else if snap.is_none() && !moved_pins.is_empty() {
            commands.extend(self.followed_wire_commands(&moved_pins)?);
        }
        self.execute(single_or_composite(commands))?;
        Ok(series_target.or_else(|| snap.map(|snap| snap.target)))
    }

    fn followed_wire_commands(
        &self,
        moved_pins: &BTreeMap<GridPos, GridPos>,
    ) -> Result<Vec<Command>, ActionError> {
        let selected_wires = self
            .selected
            .iter()
            .filter_map(|object| match object {
                ObjectRef::Wire(id) => Some(*id),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let mut claimed = BTreeSet::new();
        let mut plans = Vec::new();
        for wire in self.schematic.wires() {
            if selected_wires.contains(&wire.id) || claimed.contains(&wire.id) {
                continue;
            }
            if !moved_pins.keys().any(|point| wire.contains(*point)) {
                continue;
            }
            let Some(chain) = self.trace_wire_chain(wire, &selected_wires, &claimed) else {
                continue;
            };
            claimed.extend(chain.wires.iter().copied());
            let required = self.chain_required_points(&chain);
            if let Some(path) = wire_chain_path(&chain, moved_pins, &required) {
                plans.push((chain, path));
            }
        }
        if plans.is_empty() {
            return Ok(Vec::new());
        }

        let mut working = self
            .schematic
            .wires()
            .map(|wire| (wire.id, wire.clone()))
            .collect::<BTreeMap<_, _>>();
        for (chain, _) in &plans {
            for id in &chain.wires {
                working.remove(id);
            }
        }
        let mut allocator = self.schematic.clone();
        for (chain, path) in &plans {
            let preferred = chain.wires.iter().next().copied();
            for (index, pair) in path.windows(2).enumerate() {
                if pair[0] == pair[1] {
                    continue;
                }
                let preferred = (index == 0).then_some(preferred).flatten();
                insert_wire_segment(&mut working, &mut allocator, pair[0], pair[1], preferred)?;
            }
        }
        Ok(wire_diff_commands(&self.schematic, &working))
    }

    fn trace_wire_chain(
        &self,
        seed: &Wire,
        selected_wires: &BTreeSet<WireId>,
        claimed: &BTreeSet<WireId>,
    ) -> Option<WireChain> {
        let mut blocked = selected_wires.clone();
        blocked.extend(claimed.iter().copied());

        let mut points = self.chain_wire_points(seed, seed.a, seed.b)?;
        let mut horizontal = vec![seed.is_horizontal(); points.len().saturating_sub(1)];
        let mut wires = BTreeSet::from([seed.id]);

        let mut left_wire = seed.clone();
        let mut left_point = *points.first()?;
        while let Some(next) = self.next_chain_wire(&left_wire, left_point, &blocked, &wires) {
            let next_point = if next.a == left_point { next.b } else { next.a };
            let Some(mut segment) = self.chain_wire_points(&next, left_point, next_point) else {
                break;
            };
            let added = segment.len().saturating_sub(1);
            segment.reverse();
            segment.pop();
            segment.extend(points);
            points = segment;
            let mut grown = vec![next.is_horizontal(); added];
            grown.extend(horizontal);
            horizontal = grown;
            wires.insert(next.id);
            left_wire = next;
            left_point = next_point;
        }

        let mut right_wire = seed.clone();
        let mut right_point = *points.last()?;
        while let Some(next) = self.next_chain_wire(&right_wire, right_point, &blocked, &wires) {
            let next_point = if next.a == right_point {
                next.b
            } else {
                next.a
            };
            let Some(segment) = self.chain_wire_points(&next, right_point, next_point) else {
                break;
            };
            let added = segment.len().saturating_sub(1);
            points.extend(segment.into_iter().skip(1));
            horizontal.extend(vec![next.is_horizontal(); added]);
            wires.insert(next.id);
            right_wire = next;
            right_point = next_point;
        }

        Some(WireChain {
            wires,
            points,
            horizontal,
        })
    }

    fn next_chain_wire(
        &self,
        current: &Wire,
        at: GridPos,
        blocked: &BTreeSet<WireId>,
        visited: &BTreeSet<WireId>,
    ) -> Option<Wire> {
        if at != current.a && at != current.b {
            return None;
        }
        let mut endpoints = Vec::new();
        let mut interior_contact = false;
        for other in self.schematic.wires() {
            if other.id == current.id {
                continue;
            }
            if blocked.contains(&other.id) || visited.contains(&other.id) {
                if other.contains(at) {
                    return None;
                }
                continue;
            }
            match chain_connection(current, other, at) {
                Some(ChainConnection::EndpointEnd { collinear }) => {
                    endpoints.push((other.clone(), collinear));
                }
                Some(ChainConnection::EndpointInterior) => interior_contact = true,
                None => {}
            }
        }
        let collinear = endpoints
            .iter()
            .filter(|(_, collinear)| *collinear)
            .collect::<Vec<_>>();
        if collinear.len() == 1 {
            return Some(collinear[0].0.clone());
        }
        if endpoints.len() == 1 && !interior_contact && collinear.is_empty() {
            return Some(endpoints[0].0.clone());
        }
        None
    }

    fn chain_wire_points(&self, wire: &Wire, from: GridPos, to: GridPos) -> Option<Vec<GridPos>> {
        let forward = from == wire.a && to == wire.b;
        let backward = from == wire.b && to == wire.a;
        if !forward && !backward {
            return None;
        }
        for other in self.schematic.wires() {
            if other.id == wire.id {
                continue;
            }
            if matches!(
                segment_relation(wire.a, wire.b, other.a, other.b),
                SegmentRelation::CollinearOverlap { .. }
            ) {
                return None;
            }
        }

        let mut anchors = BTreeSet::new();
        for (_, position) in self.pin_positions() {
            if strictly_between(position, wire.a, wire.b) {
                anchors.insert(position);
            }
        }
        for label in self.schematic.labels() {
            if strictly_between(label.at, wire.a, wire.b) {
                anchors.insert(label.at);
            }
        }
        for junction in self.schematic.junctions() {
            if strictly_between(junction.at, wire.a, wire.b) {
                anchors.insert(junction.at);
            }
        }
        for other in self.schematic.wires() {
            if other.id == wire.id {
                continue;
            }
            for point in [other.a, other.b] {
                if strictly_between(point, wire.a, wire.b) {
                    anchors.insert(point);
                }
            }
        }

        let mut anchors = anchors.into_iter().collect::<Vec<_>>();
        if wire.is_horizontal() {
            anchors.sort_by_key(|point| point.x);
        } else {
            anchors.sort_by_key(|point| point.y);
        }
        if backward {
            anchors.reverse();
        }
        let mut points = Vec::with_capacity(anchors.len() + 2);
        points.push(from);
        points.extend(anchors);
        points.push(to);
        points.dedup();
        Some(points)
    }

    fn chain_required_points(&self, chain: &WireChain) -> BTreeSet<GridPos> {
        let mut required = BTreeSet::new();
        if let Some(first) = chain.points.first() {
            required.insert(*first);
        }
        if let Some(last) = chain.points.last() {
            required.insert(*last);
        }
        let contains_point = |point: GridPos| {
            chain
                .wires
                .iter()
                .filter_map(|id| self.wire(*id))
                .any(|wire| wire.contains(point))
        };
        for (_, position) in self.pin_positions() {
            if contains_point(position) {
                required.insert(position);
            }
        }
        for label in self.schematic.labels() {
            if contains_point(label.at) {
                required.insert(label.at);
            }
        }
        for junction in self.schematic.junctions() {
            if contains_point(junction.at) {
                required.insert(junction.at);
            }
        }
        for other in self.schematic.wires() {
            if chain.wires.contains(&other.id) {
                continue;
            }
            for point in [other.a, other.b] {
                if contains_point(point) {
                    required.insert(point);
                }
            }
        }
        required
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
        let label = history_label(&command);
        self.history
            .execute_named(&mut self.schematic, command, label)?;
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
                && point.y >= symbol.at.y
                && point.y <= symbol.at.y.saturating_add(1);
        }

        let Some(definition) = self.registry.get(&symbol.kind) else {
            return point.x.abs_diff(symbol.at.x) <= 1 && point.y.abs_diff(symbol.at.y) <= 1;
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
        point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y
    }

    /// Returns the nearest pin or wire around a grid position with the default radius.
    pub fn nearest_snap(&self, point: GridPos) -> Option<SnapTarget> {
        self.snap_point(point, DEFAULT_SNAP_RADIUS)
    }

    /// Returns the nearest pin or wire around a grid position.
    pub fn snap_point(&self, point: GridPos, radius: i32) -> Option<SnapTarget> {
        let pins = self.pin_positions();
        let wires = self.schematic.wires().cloned().collect::<Vec<_>>();
        nearest_snap(point, &pins, &wires, radius)
    }

    /// Returns all transformed symbol pins in deterministic order.
    pub fn pin_positions(&self) -> Vec<(PinKey, GridPos)> {
        let mut pins = Vec::new();
        for symbol in self.schematic.symbols() {
            let Some(definition) = self.registry.get(&symbol.kind) else {
                continue;
            };
            for (index, pin) in definition.pins.iter().enumerate() {
                if let Some(position) = symbol.pin_position_of(pin) {
                    pins.push((
                        PinKey {
                            symbol: symbol.id,
                            pin: u16::try_from(index).unwrap_or(u16::MAX),
                        },
                        position,
                    ));
                }
            }
        }
        pins
    }

    /// Routes an orthogonal wire around symbol bodies.
    pub fn route_path(
        &self,
        start: GridPos,
        end: GridPos,
        prefer_horizontal: bool,
    ) -> Vec<GridPos> {
        let obstacles = self
            .schematic
            .symbols()
            .filter_map(|symbol| self.symbol_bounds(symbol))
            .map(|(min, max)| GridRect::new(min, max))
            .collect::<Vec<_>>();
        route_wire(start, end, prefer_horizontal, &obstacles)
    }

    /// Adds an orthogonal path as one atomic undoable command.
    ///
    /// Collinear segments that meet at an endpoint are merged so drawing in
    /// several gestures does not leave a chain of tiny wires.
    pub fn add_wire_path(&mut self, points: &[GridPos]) -> Result<Vec<WireId>, ActionError> {
        if points.len() < 2 {
            return Err(ActionError::ZeroLengthWire);
        }
        let mut working = self
            .schematic
            .wires()
            .map(|wire| (wire.id, wire.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut allocator = self.schematic.clone();
        let mut ids = Vec::new();
        for pair in points.windows(2) {
            if pair[0] == pair[1] {
                continue;
            }
            let id = insert_wire_segment(&mut working, &mut allocator, pair[0], pair[1], None)?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let commands = wire_diff_commands(&self.schematic, &working);
        if commands.is_empty() {
            return Err(ActionError::Duplicate);
        }
        ids.retain(|id| working.contains_key(id));
        self.execute(single_or_composite(commands))?;
        self.clear_selection();
        Ok(ids)
    }

    /// Inserts a two-terminal component in series on an existing wire.
    pub fn insert_symbol_on_wire(
        &mut self,
        kind: SymbolKind,
        wire_id: WireId,
        at: GridPos,
    ) -> Result<SymbolId, ActionError> {
        let old = self
            .wire(wire_id)
            .ok_or(ActionError::MissingObject)?
            .clone();
        let horizontal = old.is_horizontal();
        let span = if horizontal {
            old.b.x - old.a.x
        } else {
            old.b.y - old.a.y
        };
        if span < 4 {
            return Err(ActionError::CannotInsertOnWire);
        }
        let definition = self
            .registry
            .get(&kind)
            .ok_or(ActionError::UnknownSymbol)?
            .clone();
        if definition.pins.len() != 2 {
            return Err(ActionError::CannotInsertOnWire);
        }
        let center = if horizontal {
            GridPos::new(at.x.clamp(old.a.x + 2, old.b.x - 2), old.a.y)
        } else {
            GridPos::new(old.a.x, at.y.clamp(old.a.y + 2, old.b.y - 2))
        };
        let orientation = if horizontal {
            Orientation::R0
        } else {
            Orientation::R90
        };
        let label = self.next_label(&kind)?;
        let mut allocator = self.schematic.clone();
        let symbol_id = SymbolId(
            allocator
                .allocate_id()
                .map_err(|_| ActionError::IdSpaceExhausted)?,
        );
        let symbol = Symbol::from_def(symbol_id, &definition, center, label, orientation, false);
        let mut pin_positions = Vec::with_capacity(2);
        for pin in &definition.pins {
            let position = symbol
                .pin_position_of(pin)
                .ok_or(ActionError::CannotInsertOnWire)?;
            let on_wire = if horizontal {
                position.y == old.a.y && position.x >= old.a.x && position.x <= old.b.x
            } else {
                position.x == old.a.x && position.y >= old.a.y && position.y <= old.b.y
            };
            if !on_wire {
                return Err(ActionError::CannotInsertOnWire);
            }
            pin_positions.push(position);
        }
        pin_positions.sort_by_key(|position| (position.x, position.y));
        let (low, high) = (pin_positions[0], pin_positions[1]);
        let gap_hits = self
            .schematic
            .labels()
            .any(|label| strictly_between(label.at, low, high))
            || self
                .schematic
                .junctions()
                .any(|junction| strictly_between(junction.at, low, high));
        if gap_hits {
            return Err(ActionError::CannotInsertOnWire);
        }

        let mut commands = vec![
            Command::RemoveWire(old.clone()),
            Command::AddSymbol(symbol.clone()),
        ];
        for (start, end) in [(old.a, low), (high, old.b)] {
            if start == end {
                continue;
            }
            let id = WireId(
                allocator
                    .allocate_id()
                    .map_err(|_| ActionError::IdSpaceExhausted)?,
            );
            commands.push(Command::AddWire(Wire::new(id, start, end)?));
        }
        self.execute(single_or_composite(commands))?;
        self.select_only(Some(ObjectRef::Symbol(symbol_id)));
        Ok(symbol_id)
    }

    /// Moves one wire segment along one grid axis.
    ///
    /// A segment can move perpendicularly or translate along its own axis.
    pub fn move_wire_segment(
        &mut self,
        wire_id: WireId,
        delta: GridPos,
    ) -> Result<(), ActionError> {
        let old = self
            .wire(wire_id)
            .ok_or(ActionError::MissingObject)?
            .clone();
        if (delta.x == 0) == (delta.y == 0) {
            return Err(ActionError::CannotMoveWire);
        }
        let new_a = old
            .a
            .checked_add(delta)
            .ok_or(ActionError::CannotMoveWire)?;
        let new_b = old
            .b
            .checked_add(delta)
            .ok_or(ActionError::CannotMoveWire)?;
        let moved = Wire::new(old.id, new_a, new_b).map_err(|_| ActionError::CannotMoveWire)?;
        let duplicate = self
            .schematic
            .wires()
            .any(|wire| wire.id != old.id && wire.a == moved.a && wire.b == moved.b);
        if duplicate {
            return Err(ActionError::CannotMoveWire);
        }
        self.execute(Command::ReplaceWire { old, new: moved })
    }

    /// Splits a wire into two segments at an interior point.
    pub fn split_wire(&mut self, wire_id: WireId, at: GridPos) -> Result<(), ActionError> {
        let old = self
            .wire(wire_id)
            .ok_or(ActionError::MissingObject)?
            .clone();
        if !old.contains(at) || at == old.a || at == old.b {
            return Err(ActionError::CannotSplitWire);
        }
        let mut allocator = self.schematic.clone();
        let first_id = WireId(
            allocator
                .allocate_id()
                .map_err(|_| ActionError::IdSpaceExhausted)?,
        );
        let second_id = WireId(
            allocator
                .allocate_id()
                .map_err(|_| ActionError::IdSpaceExhausted)?,
        );
        let first = Wire::new(first_id, old.a, at)?;
        let second = Wire::new(second_id, at, old.b)?;
        self.execute(single_or_composite(vec![
            Command::RemoveWire(old),
            Command::AddWire(first),
            Command::AddWire(second),
        ]))
    }

    /// Returns free wire endpoints that can be extended or shortened.
    pub fn free_wire_endpoints(&self) -> Vec<(WireId, WireEnd, GridPos)> {
        let mut endpoints = Vec::new();
        for wire in self.schematic.wires() {
            for (end, point) in [(WireEnd::A, wire.a), (WireEnd::B, wire.b)] {
                if self.wire_endpoint_is_free(wire, point) {
                    endpoints.push((wire.id, end, point));
                }
            }
        }
        endpoints
    }

    /// Moves one free wire endpoint.
    ///
    /// Along the original axis the wire is resized. A perpendicular target
    /// keeps the existing wire and appends an orthogonal extension.
    pub fn move_wire_endpoint(
        &mut self,
        wire_id: WireId,
        end: WireEnd,
        at: GridPos,
    ) -> Result<(), ActionError> {
        let old = self
            .wire(wire_id)
            .ok_or(ActionError::MissingObject)?
            .clone();
        let point = match end {
            WireEnd::A => old.a,
            WireEnd::B => old.b,
        };
        if !self.wire_endpoint_is_free(&old, point) {
            return Err(ActionError::CannotResizeWire);
        }
        if at == point {
            return Ok(());
        }
        let on_axis = if old.is_horizontal() {
            at.y == old.a.y
        } else {
            at.x == old.a.x
        };
        if !on_axis {
            let path = self.wire_endpoint_extension(&old, end, at);
            match self.add_wire_path(&path) {
                Ok(ids) => {
                    if let Some(id) = ids.last() {
                        self.select_only(Some(ObjectRef::Wire(*id)));
                    }
                    return Ok(());
                }
                Err(ActionError::Duplicate) => return Ok(()),
                Err(error) => return Err(error),
            }
        }
        let (a, b) = match end {
            WireEnd::A => (at, old.b),
            WireEnd::B => (old.a, at),
        };
        if a == b {
            return Err(ActionError::ZeroLengthWire);
        }
        let moved = Wire::new(old.id, a, b)?;
        if moved == old {
            return Ok(());
        }
        let duplicate = self
            .schematic
            .wires()
            .any(|wire| wire.id != old.id && wire.a == moved.a && wire.b == moved.b);
        if duplicate {
            return Err(ActionError::Duplicate);
        }
        self.execute(Command::ReplaceWire { old, new: moved })
    }

    /// Returns the orthogonal path appended when a free endpoint is dragged to
    /// a perpendicular position.
    pub fn wire_endpoint_extension(&self, wire: &Wire, end: WireEnd, at: GridPos) -> Vec<GridPos> {
        let moving = match end {
            WireEnd::A => wire.a,
            WireEnd::B => wire.b,
        };
        self.route_path(moving, at, !wire.is_horizontal())
    }

    fn wire_endpoint_is_free(&self, wire: &Wire, point: GridPos) -> bool {
        if self.pin_at(point).is_some()
            || self.schematic.labels().any(|label| label.at == point)
            || self
                .schematic
                .junctions()
                .any(|junction| junction.at == point)
        {
            return false;
        }
        self.schematic
            .wires()
            .filter(|other| other.id != wire.id)
            .all(|other| {
                if other.a == point || other.b == point {
                    return false;
                }
                if other.contains(point) && other.is_horizontal() == wire.is_horizontal() {
                    return false;
                }
                true
            })
    }

    /// Aligns selected objects against a common edge or centre.
    pub fn align_selection(&mut self, alignment: Alignment) -> Result<(), ActionError> {
        let mut bounds = Vec::new();
        for object in &self.selected {
            if let Some((min, max)) = self.object_bounds(*object) {
                bounds.push((*object, min, max));
            }
        }
        if bounds.len() < 2 {
            return Err(ActionError::NotEnoughObjects);
        }
        let min_x = bounds.iter().map(|(_, min, _)| min.x).min().unwrap_or(0);
        let max_x = bounds.iter().map(|(_, _, max)| max.x).max().unwrap_or(0);
        let min_y = bounds.iter().map(|(_, min, _)| min.y).min().unwrap_or(0);
        let max_y = bounds.iter().map(|(_, _, max)| max.y).max().unwrap_or(0);
        let mut commands = Vec::new();
        for (object, min, max) in bounds {
            let delta = match alignment {
                Alignment::Left => GridPos::new(min_x - min.x, 0),
                Alignment::CenterX => GridPos::new((min_x + max_x) / 2 - (min.x + max.x) / 2, 0),
                Alignment::Right => GridPos::new(max_x - max.x, 0),
                Alignment::Top => GridPos::new(0, min_y - min.y),
                Alignment::CenterY => GridPos::new(0, (min_y + max_y) / 2 - (min.y + max.y) / 2),
                Alignment::Bottom => GridPos::new(0, max_y - max.y),
            };
            if delta != GridPos::default() {
                commands.push(Command::MoveObjects {
                    objects: vec![object],
                    delta,
                });
            }
        }
        if commands.is_empty() {
            return Ok(());
        }
        self.execute(single_or_composite(commands))
    }

    /// Distributes selected objects evenly along one axis.
    pub fn distribute_selection(&mut self, axis: DistributionAxis) -> Result<(), ActionError> {
        let mut entries = Vec::new();
        for object in &self.selected {
            if let Some((min, max)) = self.object_bounds(*object) {
                let center = match axis {
                    DistributionAxis::Horizontal => (min.x + max.x) / 2,
                    DistributionAxis::Vertical => (min.y + max.y) / 2,
                };
                entries.push((center, *object));
            }
        }
        if entries.len() < 3 {
            return Err(ActionError::NotEnoughForDistribution);
        }
        entries.sort_by_key(|(center, object)| (*center, *object));
        let first = entries[0].0;
        let last = entries[entries.len() - 1].0;
        let denominator = i64::try_from(entries.len() - 1).unwrap_or(1);
        let step = (i64::from(last) - i64::from(first)) / denominator;
        let mut commands = Vec::new();
        for (index, (center, object)) in entries.iter().enumerate().skip(1).take(entries.len() - 2)
        {
            let target =
                first + i32::try_from(step * i64::try_from(index).unwrap_or(0)).unwrap_or(0);
            let delta = match axis {
                DistributionAxis::Horizontal => GridPos::new(target - center, 0),
                DistributionAxis::Vertical => GridPos::new(0, target - center),
            };
            if delta != GridPos::default() {
                commands.push(Command::MoveObjects {
                    objects: vec![*object],
                    delta,
                });
            }
        }
        if commands.is_empty() {
            return Ok(());
        }
        self.execute(single_or_composite(commands))
    }

    /// Returns the bounds of an editable object.
    pub fn object_bounds(&self, object: ObjectRef) -> Option<(GridPos, GridPos)> {
        match object {
            ObjectRef::Symbol(id) => self
                .symbol(id)
                .and_then(|symbol| self.symbol_bounds(symbol)),
            ObjectRef::Wire(id) => self.wire(id).map(|wire| (wire.a, wire.b)),
            ObjectRef::Junction(id) => self.junction(id).map(|junction| (junction.at, junction.at)),
            ObjectRef::Label(id) => self.label(id).map(|label| (label.at, label.at)),
        }
    }

    /// Returns all objects belonging to the net at a pin, wire, or label.
    pub fn net_objects_at(&self, point: GridPos) -> Option<BTreeSet<ObjectRef>> {
        let extraction = self.extraction.as_ref()?;
        let net = self.net_at_point(extraction, point)?;
        let mut objects = BTreeSet::new();
        objects.extend(net.wires.iter().copied().map(ObjectRef::Wire));
        objects.extend(net.labels.iter().copied().map(ObjectRef::Label));
        objects.extend(net.pins.iter().map(|pin| ObjectRef::Symbol(pin.symbol)));
        for junction in self.schematic.junctions() {
            if net
                .wires
                .iter()
                .filter_map(|id| self.wire(*id))
                .any(|wire| wire_contains(wire, junction.at))
            {
                objects.insert(ObjectRef::Junction(junction.id));
            }
        }
        Some(objects)
    }

    /// Selects every object of the net at a position.
    pub fn select_net_at(&mut self, point: GridPos) -> Result<usize, ActionError> {
        let objects = self.net_objects_at(point).ok_or(ActionError::NoNet)?;
        self.selected = objects;
        Ok(self.selected.len())
    }

    /// Returns the name of the net at a position.
    pub fn net_name_at(&self, point: GridPos) -> Option<String> {
        let extraction = self.extraction.as_ref()?;
        self.net_at_point(extraction, point)
            .map(|net| net.name.as_str().to_owned())
    }

    /// Returns all existing net names in deterministic order.
    pub fn existing_net_names(&self) -> Vec<String> {
        let mut names = self
            .extraction
            .as_ref()
            .map(|extraction| {
                extraction
                    .nets
                    .iter()
                    .map(|net| net.name.as_str().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        names.sort();
        names.dedup();
        names
    }

    fn net_at_point<'a>(
        &'a self,
        extraction: &'a Extraction,
        point: GridPos,
    ) -> Option<&'a ExtractedNet> {
        if let Some(key) = self.pin_at(point) {
            let node = extraction.net_of_pin.get(&key)?;
            return extraction.nets.iter().find(|net| net.node == *node);
        }
        match self.object_at(point) {
            Some(ObjectRef::Wire(id)) => extraction.nets.iter().find(|net| net.wires.contains(&id)),
            Some(ObjectRef::Label(id)) => {
                extraction.nets.iter().find(|net| net.labels.contains(&id))
            }
            Some(ObjectRef::Junction(id)) => self.junction(id).and_then(|junction| {
                extraction.nets.iter().find(|net| {
                    net.wires
                        .iter()
                        .filter_map(|wire| self.wire(*wire))
                        .any(|wire| wire_contains(wire, junction.at))
                })
            }),
            Some(ObjectRef::Symbol(id)) => extraction
                .nets
                .iter()
                .find(|net| net.pins.iter().any(|pin| pin.symbol == id)),
            None => None,
        }
    }

    /// Returns the pin exactly at a position.
    pub fn pin_at(&self, point: GridPos) -> Option<PinKey> {
        self.pin_positions()
            .into_iter()
            .find_map(|(key, position)| (position == point).then_some(key))
    }

    /// Returns the non-measurement component whose body contains a point.
    pub fn component_at(&self, point: GridPos) -> Option<&Symbol> {
        self.schematic
            .symbols()
            .filter(|symbol| !symbol.kind.is_measurement() && self.symbol_contains(symbol, point))
            .max_by_key(|symbol| symbol.id)
    }

    /// Returns the electrical net at a position, ignoring measurement probes.
    pub fn electrical_net_name_at(&self, point: GridPos) -> Option<String> {
        let extraction = self.extraction.as_ref()?;
        if let Some(key) = self
            .pin_positions()
            .into_iter()
            .filter(|(key, _)| {
                self.symbol(key.symbol)
                    .is_some_and(|symbol| !symbol.kind.is_measurement())
            })
            .find_map(|(key, position)| (position == point).then_some(key))
        {
            let node = extraction.net_of_pin.get(&key)?;
            return extraction
                .nets
                .iter()
                .find(|net| net.node == *node)
                .map(|net| net.name.as_str().to_owned());
        }
        if let Some(wire) = self
            .schematic
            .wires()
            .filter(|wire| wire.contains(point))
            .max_by_key(|wire| wire.id)
        {
            return extraction
                .nets
                .iter()
                .find(|net| net.wires.contains(&wire.id))
                .map(|net| net.name.as_str().to_owned());
        }
        if let Some(label) = self
            .schematic
            .labels()
            .filter(|label| label.at == point)
            .max_by_key(|label| label.id)
        {
            return extraction
                .nets
                .iter()
                .find(|net| net.labels.contains(&label.id))
                .map(|net| net.name.as_str().to_owned());
        }
        if self
            .schematic
            .junctions()
            .any(|junction| junction.at == point)
        {
            return extraction
                .nets
                .iter()
                .find(|net| {
                    net.wires
                        .iter()
                        .filter_map(|id| self.wire(*id))
                        .any(|wire| wire.contains(point))
                })
                .map(|net| net.name.as_str().to_owned());
        }
        None
    }

    fn symbol_bounds(&self, symbol: &Symbol) -> Option<(GridPos, GridPos)> {
        let mut min = symbol.at;
        let mut max = symbol.at;
        if let Some(definition) = self.registry.get(&symbol.kind) {
            for pin in &definition.pins {
                if let Some(position) = symbol.pin_position_of(pin) {
                    min.x = min.x.min(position.x);
                    min.y = min.y.min(position.y);
                    max.x = max.x.max(position.x);
                    max.y = max.y.max(position.y);
                }
            }
        }
        Some((min, max))
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

fn finish_example(mut document: Document) -> Result<Document, ActionError> {
    document.history.clear();
    document.selected.clear();
    document.dirty = false;
    document.revision = 0;
    document.refresh_extraction();
    Ok(document)
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

fn set_number(
    document: &mut Document,
    id: SymbolId,
    key: &str,
    value: f64,
) -> Result<(), ActionError> {
    let mut symbol = document
        .symbol(id)
        .ok_or(ActionError::MissingObject)?
        .clone();
    symbol
        .params
        .insert(key.to_owned(), sim_core::netlist::ParamValue::Number(value));
    document.update_symbol(symbol)
}

fn set_bool(
    document: &mut Document,
    id: SymbolId,
    key: &str,
    value: bool,
) -> Result<(), ActionError> {
    let mut symbol = document
        .symbol(id)
        .ok_or(ActionError::MissingObject)?
        .clone();
    symbol
        .params
        .insert(key.to_owned(), sim_core::netlist::ParamValue::Bool(value));
    document.update_symbol(symbol)
}

fn single_or_composite(mut commands: Vec<Command>) -> Command {
    if commands.len() == 1 {
        commands.remove(0)
    } else {
        Command::Composite(commands)
    }
}

fn wire_contains(wire: &Wire, point: GridPos) -> bool {
    wire.contains(point)
}

#[derive(Clone, Debug)]
struct WireChain {
    wires: BTreeSet<WireId>,
    /// Ordered vertices from one chain terminal to the other.
    points: Vec<GridPos>,
    /// Original orientation of each segment between consecutive points.
    horizontal: Vec<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChainConnection {
    EndpointEnd { collinear: bool },
    EndpointInterior,
}

fn chain_connection(current: &Wire, other: &Wire, at: GridPos) -> Option<ChainConnection> {
    match segment_relation(current.a, current.b, other.a, other.b) {
        SegmentRelation::CollinearTouch(point) if point == at => {
            Some(ChainConnection::EndpointEnd { collinear: true })
        }
        SegmentRelation::Perpendicular {
            at: point,
            endpoint_a,
            endpoint_b,
        } if point == at && endpoint_a => {
            if endpoint_b {
                Some(ChainConnection::EndpointEnd { collinear: false })
            } else {
                Some(ChainConnection::EndpointInterior)
            }
        }
        _ => None,
    }
}

fn wire_chain_path(
    chain: &WireChain,
    moved_pins: &BTreeMap<GridPos, GridPos>,
    required: &BTreeSet<GridPos>,
) -> Option<Vec<GridPos>> {
    let Some(&first) = chain.points.first() else {
        return Some(Vec::new());
    };
    let targets = chain
        .points
        .iter()
        .map(|point| moved_pins.get(point).copied().unwrap_or(*point))
        .collect::<Vec<_>>();
    let mut required = required.clone();
    for (old, target) in chain.points.iter().zip(&targets) {
        if moved_pins.contains_key(old) {
            required.insert(*target);
        }
    }
    let mut path = vec![targets.first().copied().unwrap_or(first)];
    for (index, &target) in targets.iter().enumerate().skip(1) {
        let from = *path.last()?;
        if from == target {
            continue;
        }
        if from.x != target.x && from.y != target.y {
            let horizontal_corner = GridPos::new(target.x, from.y);
            let vertical_corner = GridPos::new(from.x, target.y);
            let previous = (path.len() >= 2).then(|| path[path.len() - 2]);
            let corner = choose_chain_corner(
                from,
                horizontal_corner,
                vertical_corner,
                chain.horizontal.get(index - 1).copied().unwrap_or(true),
                previous,
                moved_pins,
            );
            if corner != from && corner != target {
                path.push(corner);
            }
        }
        path.push(target);
    }
    let path = simplify_wire_path(path, &required);
    (!wire_path_has_self_overlap(&path) && path.len() >= 2).then_some(path)
}

fn choose_chain_corner(
    from: GridPos,
    horizontal_corner: GridPos,
    vertical_corner: GridPos,
    prefer_horizontal: bool,
    previous: Option<GridPos>,
    moved_pins: &BTreeMap<GridPos, GridPos>,
) -> GridPos {
    let horizontal_blocked = moved_pins.contains_key(&horizontal_corner);
    let vertical_blocked = moved_pins.contains_key(&vertical_corner);
    if horizontal_blocked != vertical_blocked {
        return if horizontal_blocked {
            vertical_corner
        } else {
            horizontal_corner
        };
    }
    if let Some(previous) = previous {
        if previous.y == from.y && previous.x != from.x {
            return horizontal_corner;
        }
        if previous.x == from.x && previous.y != from.y {
            return vertical_corner;
        }
    }
    if prefer_horizontal {
        horizontal_corner
    } else {
        vertical_corner
    }
}

fn simplify_wire_path(mut path: Vec<GridPos>, required: &BTreeSet<GridPos>) -> Vec<GridPos> {
    path.dedup();
    let mut simplified: Vec<GridPos> = Vec::with_capacity(path.len());
    for point in path {
        while simplified.len() >= 2 {
            let before = simplified[simplified.len() - 2];
            let middle = simplified[simplified.len() - 1];
            if collinear_between(before, middle, point) {
                simplified.pop();
                continue;
            }
            if middle == before || middle == point {
                simplified.pop();
                continue;
            }
            if collinear_on_line(before, middle, point) && !required.contains(&middle) {
                simplified.pop();
                continue;
            }
            break;
        }
        simplified.push(point);
    }
    simplified.dedup();
    simplified
}

fn collinear_on_line(before: GridPos, middle: GridPos, after: GridPos) -> bool {
    (before.y == middle.y && middle.y == after.y) || (before.x == middle.x && middle.x == after.x)
}

fn collinear_between(before: GridPos, middle: GridPos, after: GridPos) -> bool {
    if before.y == middle.y && middle.y == after.y {
        let low = before.x.min(after.x);
        let high = before.x.max(after.x);
        middle.x > low && middle.x < high
    } else if before.x == middle.x && middle.x == after.x {
        let low = before.y.min(after.y);
        let high = before.y.max(after.y);
        middle.y > low && middle.y < high
    } else {
        false
    }
}

fn wire_path_has_self_overlap(path: &[GridPos]) -> bool {
    if path.len() < 3 {
        return false;
    }
    for first in 0..path.len() - 1 {
        for second in first + 1..path.len() - 1 {
            let adjacent = second == first + 1;
            match segment_relation(path[first], path[first + 1], path[second], path[second + 1]) {
                SegmentRelation::Disjoint => {}
                SegmentRelation::Perpendicular {
                    endpoint_a,
                    endpoint_b,
                    ..
                } => {
                    if adjacent {
                        if !endpoint_a || !endpoint_b {
                            return true;
                        }
                    } else if endpoint_a || endpoint_b {
                        return true;
                    }
                }
                SegmentRelation::CollinearTouch(_) => {
                    if !adjacent {
                        return true;
                    }
                }
                SegmentRelation::CollinearOverlap { .. } => return true,
            }
        }
    }
    false
}

fn insert_wire_segment(
    working: &mut BTreeMap<WireId, Wire>,
    allocator: &mut Schematic,
    start: GridPos,
    end: GridPos,
    preferred: Option<WireId>,
) -> Result<WireId, ActionError> {
    if start == end {
        return Err(ActionError::ZeroLengthWire);
    }
    let segment = Wire::new(WireId(0), start, end)?;
    let mut current = segment;
    let mut merged = BTreeSet::new();
    loop {
        let candidate = working
            .iter()
            .find(|(id, wire)| {
                !merged.contains(*id)
                    && wire.is_horizontal() == current.is_horizontal()
                    && (wire.a == current.a
                        || wire.a == current.b
                        || wire.b == current.a
                        || wire.b == current.b)
            })
            .map(|(id, wire)| (*id, wire.clone()));
        let Some((id, other)) = candidate else {
            break;
        };
        working.remove(&id);
        merged.insert(id);
        current = if current.is_horizontal() {
            Wire::new(
                current.id,
                GridPos::new(current.a.x.min(other.a.x), current.a.y),
                GridPos::new(current.b.x.max(other.b.x), current.b.y),
            )?
        } else {
            Wire::new(
                current.id,
                GridPos::new(current.a.x, current.a.y.min(other.a.y)),
                GridPos::new(current.b.x, current.b.y.max(other.b.y)),
            )?
        };
    }

    let survivor = if let Some(id) = preferred {
        id
    } else if let Some(id) = merged.iter().next().copied() {
        id
    } else {
        WireId(
            allocator
                .allocate_id()
                .map_err(|_| ActionError::IdSpaceExhausted)?,
        )
    };
    current.id = survivor;
    working.insert(survivor, current);
    Ok(survivor)
}

fn wire_diff_commands(original: &Schematic, working: &BTreeMap<WireId, Wire>) -> Vec<Command> {
    let mut commands = Vec::new();
    for old in original.wires() {
        match working.get(&old.id) {
            None => commands.push(Command::RemoveWire(old.clone())),
            Some(new) if new != old => commands.push(Command::ReplaceWire {
                old: old.clone(),
                new: new.clone(),
            }),
            Some(_) => {}
        }
    }
    for (id, new) in working {
        if original.wire(*id).is_none() {
            commands.push(Command::AddWire(new.clone()));
        }
    }
    commands
}

fn strictly_between(point: GridPos, low: GridPos, high: GridPos) -> bool {
    if low.y == high.y {
        point.y == low.y && point.x > low.x && point.x < high.x
    } else {
        point.x == low.x && point.y > low.y && point.y < high.y
    }
}

fn squared_grid_distance(a: GridPos, b: GridPos) -> i64 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dy = i64::from(a.y) - i64::from(b.y);
    dx * dx + dy * dy
}

/// Geometry transform applied to a whole selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupTransform {
    /// Clockwise quarter turn around the selection centre.
    Rotate,
    /// Mirror across the selection vertical centre.
    Mirror,
}

impl GroupTransform {
    fn point(self, point: GridPos, center: GridPos) -> Option<GridPos> {
        let x = match self {
            Self::Rotate => i64::from(center.x) + i64::from(center.y) - i64::from(point.y),
            Self::Mirror => 2 * i64::from(center.x) - i64::from(point.x),
        };
        let y = match self {
            Self::Rotate => i64::from(center.y) - i64::from(center.x) + i64::from(point.x),
            Self::Mirror => i64::from(point.y),
        };
        Some(GridPos::new(i32::try_from(x).ok()?, i32::try_from(y).ok()?))
    }
}

fn next_label_with(kind: &SymbolKind, used: &mut BTreeSet<String>) -> Result<String, ActionError> {
    let prefix = label_prefix(kind);
    let mut index = 1_u32;
    loop {
        let candidate = format!("{prefix}{index}");
        if used.insert(candidate.clone()) {
            return Ok(candidate);
        }
        index = index.checked_add(1).ok_or(ActionError::IdSpaceExhausted)?;
    }
}

fn history_label(command: &Command) -> String {
    match command {
        Command::AddSymbol(symbol) => format!("Ajouter {}", symbol.label),
        Command::RemoveSymbol(symbol) => format!("Supprimer {}", symbol.label),
        Command::AddWire(_) => "Ajouter un fil".to_owned(),
        Command::RemoveWire(_) => "Supprimer un fil".to_owned(),
        Command::AddJunction(_) => "Ajouter une jonction".to_owned(),
        Command::RemoveJunction(_) => "Supprimer une jonction".to_owned(),
        Command::AddLabel(label) => format!("Ajouter le label {}", label.text),
        Command::RemoveLabel(label) => format!("Supprimer le label {}", label.text),
        Command::MoveObjects { objects, .. } => {
            format!("Déplacer {} objet(s)", objects.len())
        }
        Command::ReplaceSymbol { new, .. } => format!("Modifier {}", new.label),
        Command::ReplaceWire { .. } => "Déplacer un fil".to_owned(),
        Command::ReplaceJunction { .. } => "Déplacer une jonction".to_owned(),
        Command::ReplaceLabel { new, .. } => format!("Renommer le label {}", new.text),
        Command::Composite(commands) => composite_history_label(commands),
    }
}

fn composite_history_label(commands: &[Command]) -> String {
    let has_symbol = commands
        .iter()
        .any(|command| matches!(command, Command::AddSymbol(_)));
    let removes_wire = commands
        .iter()
        .any(|command| matches!(command, Command::RemoveWire(_)));
    if has_symbol && removes_wire {
        return "Insérer un composant".to_owned();
    }
    if commands
        .iter()
        .all(|command| matches!(command, Command::AddWire(_)))
    {
        return "Ajouter un chemin".to_owned();
    }
    if commands.iter().all(|command| {
        matches!(
            command,
            Command::RemoveSymbol(_)
                | Command::RemoveWire(_)
                | Command::RemoveJunction(_)
                | Command::RemoveLabel(_)
        )
    }) {
        return "Supprimer une sélection".to_owned();
    }
    if commands.iter().all(|command| {
        matches!(
            command,
            Command::MoveObjects { .. }
                | Command::ReplaceSymbol { .. }
                | Command::ReplaceWire { .. }
                | Command::ReplaceJunction { .. }
                | Command::ReplaceLabel { .. }
        )
    }) {
        return "Transformer une sélection".to_owned();
    }
    format!("Modifier {} objets", commands.len())
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
        SymbolKind::ShuntReference => "U",
        SymbolKind::Comparator => "U",
        SymbolKind::OpAmp => "U",
        SymbolKind::Mosfet => "M",
        SymbolKind::Voltmeter => "VM",
        SymbolKind::Ammeter => "AM",
        SymbolKind::Ohmmeter => "OH",
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
        (SymbolKind::Inductor, "resistance") => Some(Unit::Ohm),
        (SymbolKind::ShuntReference, "z_ka") => Some(Unit::Ohm),
        (SymbolKind::Comparator, "r_sink") => Some(Unit::Ohm),
        (SymbolKind::Mosfet, "rd" | "rs") => Some(Unit::Ohm),
        (SymbolKind::Mosfet, "cgs" | "cgd" | "cds") => Some(Unit::Farad),
        (SymbolKind::Mosfet, "vth") => Some(Unit::Volt),
        (SymbolKind::Mosfet, "is") => Some(Unit::Ampere),
        (SymbolKind::Led, "wavelength") => None,
        (SymbolKind::Led | SymbolKind::Diode, "vf") => Some(Unit::Volt),
        (SymbolKind::Led | SymbolKind::Diode, "if") => Some(Unit::Ampere),
        (_, "imax") => Some(Unit::Ampere),
        (SymbolKind::VoltageSource, "dc")
        | (SymbolKind::ShuntReference, "vref")
        | (SymbolKind::Comparator, "v_offset")
        | (SymbolKind::Zener, "bv")
        | (SymbolKind::OpAmp, "v_sat_pos" | "v_sat_neg") => Some(Unit::Volt),
        (SymbolKind::CurrentSource, "dc")
        | (SymbolKind::ShuntReference, "iref" | "ik_min")
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
        "vf" => "Tension directe Vf",
        "if" => "Courant nominal If",
        "imax" => "Courant maximal",
        "wavelength" => "Longueur d'onde (nm)",
        "type" => "Type",
        "dc" => "Tension ou courant DC",
        "ron" => "Résistance fermée",
        "roff" => "Résistance ouverte",
        "closed" => "Fermé",
        "is" => "Courant de saturation",
        "n" => "Coefficient d'émission",
        "bv" => "Tension de claquage",
        "ibv" => "Courant de claquage",
        "nbv" => "Coefficient de claquage",
        "vref" => "Consigne de référence VREF",
        "iref" => "Courant de la broche REF",
        "ik_min" => "Courant cathode minimal",
        "z_ka" => "Impédance dynamique ZKA",
        "v_offset" => "Offset d'entrée",
        "r_sink" => "Résistance de sink",
        "gain" => "Gain",
        "v_sat_pos" => "Saturation positive",
        "v_sat_neg" => "Saturation négative",
        "channel" => "Canal",
        "vth" => "Tension de seuil Vth",
        "k" => "Transconductance k",
        "lambda" => "Modulation λ",
        "rd" => "Résistance de drain",
        "rs" => "Résistance de source",
        "cgs" => "Capacité grille-source",
        "cgd" => "Capacité grille-drain",
        "cds" => "Capacité drain-source",
        "swap_ds" => "Inverser drain/source",
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
    use proptest::prelude::*;

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
    fn examples_extract_clean_netlists() {
        for (name, document) in [
            ("divider", Document::divider_example().unwrap()),
            ("led", Document::led_example().unwrap()),
            ("switch", Document::switch_example().unwrap()),
            ("mosfet", Document::mosfet_example().unwrap()),
            ("rlc", Document::rlc_example().unwrap()),
            ("parallel-rlc", Document::parallel_rlc_example().unwrap()),
        ] {
            assert!(
                document.extraction().is_some(),
                "{name} must extract a netlist"
            );
            assert!(
                document.erc().is_some_and(|erc| erc.issues().is_empty()),
                "{name} must have a clean ERC"
            );
            assert!(!document.nets().is_empty(), "{name} must have nets");
        }
    }

    #[test]
    fn example_sources_drive_a_positive_output() {
        for (name, document) in [
            ("divider", Document::divider_example().unwrap()),
            ("led", Document::led_example().unwrap()),
            ("switch", Document::switch_example().unwrap()),
            ("mosfet", Document::mosfet_example().unwrap()),
            ("rlc", Document::rlc_example().unwrap()),
        ] {
            let extraction = document.extraction().expect("example extracts");
            let result = sim_core::solve_dc(&extraction.netlist, sim_core::DcOptions::default())
                .expect("example has a DC operating point");
            let out = extraction
                .netlist
                .nodes
                .iter()
                .find(|node| node.name.as_str() == "OUT")
                .expect("example defines OUT");
            let voltage = result.node_voltage(out.id).expect("OUT is solved");
            assert!(
                voltage > 0.0,
                "{name} must drive OUT positive, got {voltage}"
            );
        }
    }

    #[test]
    fn mosfet_example_gate_switch_controls_the_load() {
        let mut document = Document::mosfet_example().unwrap();
        let mosfet = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == SymbolKind::Mosfet)
            .expect("example places the N-MOSFET");
        assert_eq!(mosfet.at, GridPos::new(14, 4));
        assert_eq!(
            mosfet.params.get("channel").and_then(ParamValue::as_text),
            Some("n")
        );
        let switch = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == SymbolKind::Switch)
            .expect("example places the gate switch");
        assert_eq!(switch.at, GridPos::new(7, 4));
        assert_eq!(
            switch.params.get("closed").and_then(ParamValue::as_bool),
            Some(true)
        );

        let closed = dc_out_voltage(&document);
        assert!(
            closed < 0.2,
            "a closed gate switch must pull the drain near ground, got {closed} V"
        );
        let current_ma = (10.0 - closed) / 1_000.0 * 1e3;
        assert!(
            (9.8..10.0).contains(&current_ma),
            "the 1 kOhm load must carry about 10 mA, got {current_ma} mA"
        );

        let switch_id = switch.id;
        set_bool(&mut document, switch_id, "closed", false).unwrap();
        let open = dc_out_voltage(&document);
        assert!(
            open > 9.9,
            "the 100 kOhm pull-down must turn the MOSFET off, got {open} V"
        );
    }

    fn dc_out_voltage(document: &Document) -> f64 {
        let extraction = document.extraction().expect("example extracts");
        let result = sim_core::solve_dc(&extraction.netlist, sim_core::DcOptions::default())
            .expect("example has a DC operating point");
        let out = extraction
            .netlist
            .nodes
            .iter()
            .find(|node| node.name.as_str() == "OUT")
            .expect("example defines OUT");
        result.node_voltage(out.id).expect("OUT is solved")
    }

    #[test]
    fn divider_example_matches_the_requested_layout() {
        let document = Document::divider_example().unwrap();
        let source = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == SymbolKind::VoltageSource)
            .unwrap();
        assert_eq!(source.at, GridPos::new(3, -5));
        assert_eq!(source.orientation, Orientation::R180);
        for (label, at) in [("R1", GridPos::new(8, -2)), ("R2", GridPos::new(8, 4))] {
            let symbol = document
                .schematic()
                .symbols()
                .find(|symbol| symbol.label == label)
                .unwrap();
            assert_eq!(symbol.at, at);
            assert_eq!(symbol.orientation, Orientation::R90);
            assert_eq!(
                symbol
                    .params
                    .get("resistance")
                    .and_then(sim_core::netlist::ParamValue::as_number),
                Some(1_000.0)
            );
        }
        assert!(document
            .schematic()
            .labels()
            .any(|label| label.text == "OUT" && label.at == GridPos::new(8, 0)));
        let extraction = document.extraction().unwrap();
        let result = sim_core::solve_dc(&extraction.netlist, sim_core::DcOptions::default())
            .expect("divider has a DC operating point");
        let out = extraction
            .netlist
            .nodes
            .iter()
            .find(|node| node.name.as_str() == "OUT")
            .unwrap();
        let voltage = result.node_voltage(out.id).unwrap();
        assert!((voltage - 2.5).abs() < 1e-9, "OUT = {voltage}");
    }

    #[test]
    fn rlc_example_matches_the_requested_layout() {
        let document = Document::rlc_example().unwrap();
        let source = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == SymbolKind::VoltageSource)
            .unwrap();
        assert_eq!(source.at, GridPos::new(0, 0));
        assert_eq!(source.orientation, Orientation::R180);
        for (label, at, orientation) in [
            ("R1", GridPos::new(6, 0), Orientation::R0),
            ("L1", GridPos::new(12, 0), Orientation::R0),
            ("C1", GridPos::new(14, 4), Orientation::R90),
        ] {
            let symbol = document
                .schematic()
                .symbols()
                .find(|symbol| symbol.label == label)
                .unwrap();
            assert_eq!(symbol.at, at);
            assert_eq!(symbol.orientation, orientation);
        }
        assert!(document
            .schematic()
            .labels()
            .any(|label| label.text == "OUT" && label.at == GridPos::new(14, 0)));
        assert!(document
            .schematic()
            .symbols()
            .any(|symbol| symbol.kind == SymbolKind::Ground && symbol.at == GridPos::new(-2, 8)));
        let extraction = document.extraction().unwrap();
        let result = sim_core::solve_dc(&extraction.netlist, sim_core::DcOptions::default())
            .expect("RLC has a DC operating point");
        let out = extraction
            .netlist
            .nodes
            .iter()
            .find(|node| node.name.as_str() == "OUT")
            .unwrap();
        let voltage = result.node_voltage(out.id).unwrap();
        assert!((voltage - 5.0).abs() < 1e-9, "OUT = {voltage}");
    }

    #[test]
    fn parallel_rlc_example_keeps_the_requested_values() {
        let document = Document::parallel_rlc_example().unwrap();
        let expected = [
            ("V1", "dc", 5.0),
            ("R1", "resistance", 10.0),
            ("L1", "inductance", 1.0),
            ("C1", "capacitance", 15e-6),
            ("R2", "resistance", 100.0),
        ];
        for (label, key, value) in expected {
            let symbol = document
                .schematic()
                .symbols()
                .find(|symbol| symbol.label == label)
                .unwrap_or_else(|| panic!("{label} must exist"));
            let actual = symbol
                .params
                .get(key)
                .and_then(sim_core::netlist::ParamValue::as_number)
                .unwrap_or_else(|| panic!("{label}.{key} must be numeric"));
            assert!(
                (actual - value).abs() <= value.abs().max(1.0) * 1e-12,
                "{label}.{key} is {actual}, expected {value}"
            );
        }
        let switch = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == SymbolKind::Switch)
            .unwrap();
        assert!(!switch
            .params
            .get("closed")
            .and_then(sim_core::netlist::ParamValue::as_bool)
            .unwrap_or(true));
    }

    #[test]
    fn project_json_round_trips_the_schematic() {
        let document = Document::demo().unwrap();
        let json = document.to_project_json().unwrap();
        let restored = Document::from_project_json(&json).unwrap();
        assert_eq!(restored.schematic(), document.schematic());
        assert_eq!(restored.revision(), 0);
        assert!(restored.extraction().is_some());
        assert!(Document::from_project_json("{").is_err());
    }

    #[test]
    fn demo_matches_the_requested_rc_layout() {
        let document = Document::demo().unwrap();
        let source = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == SymbolKind::VoltageSource)
            .unwrap();
        assert_eq!(source.at, GridPos::new(-10, -6));
        assert_eq!(source.orientation, Orientation::R90);
        assert_eq!(
            source
                .params
                .get("dc")
                .and_then(sim_core::netlist::ParamValue::as_number),
            Some(10.0)
        );
        let switch = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == SymbolKind::Switch)
            .unwrap();
        assert_eq!(switch.at, GridPos::new(-6, -10));
        assert_eq!(
            switch
                .params
                .get("closed")
                .and_then(sim_core::netlist::ParamValue::as_bool),
            Some(true)
        );
        for (label, at, orientation, key, value) in [
            (
                "R1",
                GridPos::new(-1, -10),
                Orientation::R0,
                "resistance",
                1_000.0,
            ),
            (
                "C1",
                GridPos::new(2, -6),
                Orientation::R90,
                "capacitance",
                100e-6,
            ),
        ] {
            let symbol = document
                .schematic()
                .symbols()
                .find(|symbol| symbol.label == label)
                .unwrap();
            assert_eq!(symbol.at, at);
            assert_eq!(symbol.orientation, orientation);
            assert_eq!(
                symbol
                    .params
                    .get(key)
                    .and_then(sim_core::netlist::ParamValue::as_number),
                Some(value)
            );
        }
        assert!(document
            .schematic()
            .labels()
            .any(|label| label.text == "Vc" && label.at == GridPos::new(2, -10)));
        assert!(document
            .schematic()
            .symbols()
            .any(|symbol| symbol.kind == SymbolKind::Ground && symbol.at == GridPos::new(-10, -1)));
        let extraction = document.extraction().unwrap();
        let result = sim_core::solve_dc(&extraction.netlist, sim_core::DcOptions::default())
            .expect("demo has a DC operating point");
        let out = extraction
            .netlist
            .nodes
            .iter()
            .find(|node| node.name.as_str() == "Vc")
            .unwrap();
        let voltage = result.node_voltage(out.id).unwrap();
        assert!((voltage - 10.0).abs() < 1e-9, "Vc = {voltage}");
    }

    #[test]
    fn demo_extracts_a_clean_netlist() {
        let document = Document::demo().unwrap();
        let extraction = document.extraction().unwrap();
        assert!(extraction.erc.is_empty());
        assert_eq!(extraction.netlist.nodes.len(), 4);
        assert_eq!(extraction.netlist.components.len(), 5);
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
    fn symbol_hit_area_hugs_the_body() {
        let document = Document::demo().unwrap();
        let resistor = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == SymbolKind::Resistor)
            .unwrap();
        let center = resistor.at;
        assert!(matches!(
            document.object_at(center),
            Some(ObjectRef::Symbol(_))
        ));
        assert!(document
            .object_at(GridPos::new(center.x, center.y - 1))
            .is_none());
    }

    #[test]
    fn hit_testing_prefers_labels_and_symbols() {
        let document = Document::demo().unwrap();
        assert!(matches!(
            document.object_at(GridPos::new(2, -10)),
            Some(ObjectRef::Label(_))
        ));
        assert!(matches!(
            document.object_at(GridPos::new(-1, -10)),
            Some(ObjectRef::Symbol(_))
        ));
        assert!(matches!(
            document.object_at(GridPos::new(-9, -10)),
            Some(ObjectRef::Wire(_))
        ));
    }

    #[test]
    fn parameter_parsing_and_formatting_round_trip_engineering_values() {
        let value = parse_parameter("4k7").unwrap();
        assert!((value - 4_700.0).abs() < 1e-9);
        let capacitance = parse_parameter("100n").unwrap();
        assert!((capacitance - 100e-9).abs() < 1e-18);
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
    fn inserts_a_component_in_series_and_undoes_atomically() {
        let mut document = Document::new();
        let wire = document
            .add_wire(GridPos::new(0, 0), GridPos::new(8, 0))
            .unwrap()[0];
        let symbol = document
            .insert_symbol_on_wire(SymbolKind::Resistor, wire, GridPos::new(4, 0))
            .unwrap();
        assert_eq!(document.schematic().wires().count(), 2);
        assert_eq!(document.symbol(symbol).unwrap().at, GridPos::new(4, 0));
        document.undo().unwrap();
        assert_eq!(document.schematic().wires().count(), 1);
        assert!(document.symbol(symbol).is_none());
    }

    #[test]
    fn inserts_a_vertical_component_in_series() {
        let mut document = Document::new();
        let wire = document
            .add_wire(GridPos::new(0, 0), GridPos::new(0, 8))
            .unwrap()[0];
        let symbol = document
            .insert_symbol_on_wire(SymbolKind::Capacitor, wire, GridPos::new(0, 4))
            .unwrap();
        assert_eq!(
            document.symbol(symbol).unwrap().orientation,
            Orientation::R90
        );
        assert_eq!(document.schematic().wires().count(), 2);
    }

    #[test]
    fn toggles_a_switch_atomically() {
        let mut document = Document::new();
        let id = document
            .add_symbol(SymbolKind::Switch, GridPos::new(0, 0))
            .unwrap();
        assert!(document.toggle_switch(id).unwrap());
        assert_eq!(
            document
                .symbol(id)
                .and_then(|symbol| symbol.params.get("closed"))
                .and_then(ParamValue::as_bool),
            Some(true)
        );
        document.undo().unwrap();
        assert_eq!(
            document
                .symbol(id)
                .and_then(|symbol| symbol.params.get("closed"))
                .and_then(ParamValue::as_bool),
            Some(false)
        );
    }

    #[test]
    fn wires_with_erc_problems_are_reported() {
        let mut document = Document::new();
        let wire = document
            .add_wire(GridPos::new(0, 0), GridPos::new(6, 0))
            .unwrap()[0];
        let problems = document.problem_wires();
        assert!(problems.contains(&wire));
    }

    #[test]
    fn splits_a_wire_with_one_undo() {
        let mut document = Document::new();
        let wire = document
            .add_wire(GridPos::new(0, 0), GridPos::new(8, 0))
            .unwrap()[0];
        document.split_wire(wire, GridPos::new(4, 0)).unwrap();
        assert_eq!(document.schematic().wires().count(), 2);
        assert!(document.wire(wire).is_none());
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(0, 0) && wire.b == GridPos::new(4, 0)));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(4, 0) && wire.b == GridPos::new(8, 0)));
        document.undo().unwrap();
        assert_eq!(document.schematic().wires().count(), 1);
        assert!(document.wire(wire).is_some());
    }

    #[test]
    fn rejects_a_wire_split_outside_its_interior() {
        let mut document = Document::new();
        let wire = document
            .add_wire(GridPos::new(0, 0), GridPos::new(8, 0))
            .unwrap()[0];
        assert!(matches!(
            document.split_wire(wire, GridPos::new(0, 0)),
            Err(ActionError::CannotSplitWire)
        ));
        assert!(matches!(
            document.split_wire(wire, GridPos::new(4, 2)),
            Err(ActionError::CannotSplitWire)
        ));
        assert_eq!(document.schematic().wires().count(), 1);
    }

    #[test]
    fn moves_a_segment_without_reconnecting_it() {
        let mut document = Document::new();
        let horizontal = document
            .add_wire(GridPos::new(0, 0), GridPos::new(4, 0))
            .unwrap()[0];
        document
            .add_wire(GridPos::new(4, 0), GridPos::new(4, 4))
            .unwrap();
        document
            .move_wire_segment(horizontal, GridPos::new(0, 2))
            .unwrap();
        let moved = document.wire(horizontal).unwrap();
        assert_eq!(moved.a, GridPos::new(0, 2));
        assert_eq!(moved.b, GridPos::new(4, 2));
        let verticals = document
            .schematic()
            .wires()
            .filter(|wire| !wire.is_horizontal())
            .collect::<Vec<_>>();
        assert_eq!(verticals.len(), 1);
        assert_eq!(verticals[0].a, GridPos::new(4, 0));
        assert_eq!(verticals[0].b, GridPos::new(4, 4));
        assert_eq!(document.schematic().wires().count(), 2);
        document.undo().unwrap();
        assert_eq!(document.wire(horizontal).unwrap().a, GridPos::new(0, 0));
    }

    #[test]
    fn moving_a_segment_leaves_labels_in_place() {
        let mut document = Document::new();
        let horizontal = document
            .add_wire(GridPos::new(0, 0), GridPos::new(8, 0))
            .unwrap()[0];
        document.add_label(GridPos::new(4, 0), "MID").unwrap();
        document
            .move_wire_segment(horizontal, GridPos::new(0, 2))
            .unwrap();
        assert_eq!(document.wire(horizontal).unwrap().a.y, 2);
        assert!(document
            .schematic()
            .labels()
            .any(|label| label.text == "MID" && label.at == GridPos::new(4, 0)));
        assert_eq!(document.schematic().wires().count(), 1);
    }

    #[test]
    fn moves_a_segment_along_its_axis() {
        let mut document = Document::new();
        let wire = document
            .add_wire(GridPos::new(0, 0), GridPos::new(4, 0))
            .unwrap()[0];
        document
            .move_wire_segment(wire, GridPos::new(2, 0))
            .unwrap();
        let moved = document.wire(wire).unwrap();
        assert_eq!(moved.a, GridPos::new(2, 0));
        assert_eq!(moved.b, GridPos::new(6, 0));
        assert!(matches!(
            document.move_wire_segment(wire, GridPos::new(1, 1)),
            Err(ActionError::CannotMoveWire)
        ));
    }

    #[test]
    fn appends_and_merges_collinear_wire_segments() {
        let mut document = Document::new();
        let first = document
            .add_wire_path(&[GridPos::new(0, 0), GridPos::new(2, 0)])
            .unwrap()[0];
        let ids = document
            .add_wire_path(&[GridPos::new(2, 0), GridPos::new(4, 0), GridPos::new(6, 0)])
            .unwrap();
        assert_eq!(ids, vec![first]);
        assert_eq!(document.schematic().wires().count(), 1);
        let wire = document.wire(first).unwrap();
        assert_eq!(wire.a, GridPos::new(0, 0));
        assert_eq!(wire.b, GridPos::new(6, 0));
        document.undo().unwrap();
        assert_eq!(document.schematic().wires().count(), 1);
        assert_eq!(document.wire(first).unwrap().b, GridPos::new(2, 0));
    }

    #[test]
    fn extends_a_horizontal_wire_perpendicularly() {
        let mut document = Document::new();
        let wire = document
            .add_wire(GridPos::new(0, 0), GridPos::new(4, 0))
            .unwrap()[0];
        document
            .move_wire_endpoint(wire, WireEnd::B, GridPos::new(8, 2))
            .unwrap();
        let original = document.wire(wire).unwrap();
        assert_eq!(original.a, GridPos::new(0, 0));
        assert_eq!(original.b, GridPos::new(4, 0));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(4, 0) && wire.b == GridPos::new(4, 2)));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(4, 2) && wire.b == GridPos::new(8, 2)));
        assert_eq!(document.schematic().wires().count(), 3);
        document.undo().unwrap();
        assert_eq!(document.schematic().wires().count(), 1);
        assert_eq!(document.wire(wire).unwrap().b, GridPos::new(4, 0));
    }

    #[test]
    fn extends_a_vertical_wire_perpendicularly() {
        let mut document = Document::new();
        let wire = document
            .add_wire(GridPos::new(0, 0), GridPos::new(0, 4))
            .unwrap()[0];
        document
            .move_wire_endpoint(wire, WireEnd::B, GridPos::new(2, 8))
            .unwrap();
        let original = document.wire(wire).unwrap();
        assert_eq!(original.a, GridPos::new(0, 0));
        assert_eq!(original.b, GridPos::new(0, 4));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(0, 4) && wire.b == GridPos::new(2, 4)));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(2, 4) && wire.b == GridPos::new(2, 8)));
        assert_eq!(document.schematic().wires().count(), 3);
    }

    #[test]
    fn moving_a_symbol_keeps_its_attached_wire_connected() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        let wire = document
            .add_wire(GridPos::new(2, 0), GridPos::new(6, 0))
            .unwrap()[0];
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        document.move_selection(GridPos::new(0, 2)).unwrap();
        assert_eq!(document.symbol(symbol).unwrap().at, GridPos::new(0, 2));
        assert_eq!(
            document.pin_at(GridPos::new(2, 2)).map(|pin| pin.symbol),
            Some(symbol)
        );
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(2, 2) || wire.b == GridPos::new(2, 2)));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(6, 0) || wire.b == GridPos::new(6, 0)));
        assert!(document.wire(wire).is_some());
        assert_eq!(document.schematic().wires().count(), 2);
    }

    #[test]
    fn moving_a_symbol_rebuilds_a_wire_crossing_its_pin() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        let wire = document
            .add_wire(GridPos::new(-2, 0), GridPos::new(6, 0))
            .unwrap()[0];
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        document.move_selection(GridPos::new(2, 0)).unwrap();
        let moved = document.wire(wire).unwrap();
        assert_eq!(moved.a, GridPos::new(0, 0));
        assert_eq!(moved.b, GridPos::new(6, 0));
        assert!(document.pin_at(GridPos::new(0, 0)).is_some());
        assert!(document.pin_at(GridPos::new(4, 0)).is_some());
    }

    #[test]
    fn repeated_symbol_moves_do_not_accumulate_wires() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(2, 0), GridPos::new(6, 0))
            .unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        for _ in 0..4 {
            for delta in [
                GridPos::new(0, 2),
                GridPos::new(2, 0),
                GridPos::new(0, -2),
                GridPos::new(-2, 0),
            ] {
                document.move_selection(delta).unwrap();
                let at = document.symbol(symbol).unwrap().at;
                let pin = GridPos::new(at.x + 2, at.y);
                assert!(
                    document.schematic().wires().any(|wire| wire.contains(pin)),
                    "pin {pin:?} is disconnected after {delta:?}"
                );
                assert!(document
                    .schematic()
                    .wires()
                    .any(|wire| wire.contains(GridPos::new(6, 0))));
                assert!(
                    document.schematic().wires().count() <= 2,
                    "wire count grew to {} after {delta:?}",
                    document.schematic().wires().count()
                );
                let mut geometry = BTreeSet::new();
                for wire in document.schematic().wires() {
                    assert!(geometry.insert((wire.a, wire.b)), "duplicate wire {wire:?}");
                }
            }
        }
        assert_eq!(document.symbol(symbol).unwrap().at, GridPos::new(0, 0));
        assert_eq!(document.schematic().wires().count(), 1);
    }

    #[test]
    fn moving_a_symbol_onto_a_wire_detaches_its_previous_wires() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(2, 0), GridPos::new(6, 0))
            .unwrap();
        let target = document
            .add_wire(GridPos::new(6, 4), GridPos::new(6, 12))
            .unwrap()[0];
        document.select_only(Some(ObjectRef::Symbol(symbol)));

        let snap = document.move_selection_snapped(GridPos::new(4, 6)).unwrap();

        assert_eq!(document.symbol(symbol).unwrap().at, GridPos::new(4, 6));
        assert_eq!(
            snap.map(|snap| (snap.at, snap.kind)),
            Some((GridPos::new(6, 6), SnapKind::Wire(target)))
        );
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(2, 0) && wire.b == GridPos::new(6, 0)));
        assert!(document.schematic().wires().any(|wire| {
            wire.id == target && wire.a == GridPos::new(6, 4) && wire.b == GridPos::new(6, 12)
        }));
        assert_eq!(document.schematic().wires().count(), 2);
    }

    #[test]
    fn moving_a_two_terminal_symbol_onto_a_wire_inserts_it_in_series() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(2, 0), GridPos::new(6, 0))
            .unwrap();
        let target = document
            .add_wire(GridPos::new(0, 8), GridPos::new(10, 8))
            .unwrap()[0];
        document.select_only(Some(ObjectRef::Symbol(symbol)));

        let snap = document.move_selection_snapped(GridPos::new(4, 8)).unwrap();

        assert_eq!(document.symbol(symbol).unwrap().at, GridPos::new(4, 8));
        assert_eq!(
            snap.map(|snap| (snap.at, snap.kind)),
            Some((GridPos::new(2, 8), SnapKind::Wire(target)))
        );
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(2, 0) && wire.b == GridPos::new(6, 0)));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(0, 8) && wire.b == GridPos::new(2, 8)));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(6, 8) && wire.b == GridPos::new(10, 8)));
        assert!(!document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(0, 8) && wire.b == GridPos::new(10, 8)));
        assert_eq!(document.schematic().wires().count(), 3);
    }

    #[test]
    fn moving_a_symbol_snaps_a_pin_to_a_nearby_wire() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        let target = document
            .add_wire(GridPos::new(6, 4), GridPos::new(6, 12))
            .unwrap()[0];
        document.select_only(Some(ObjectRef::Symbol(symbol)));

        let snap = document.move_selection_snapped(GridPos::new(3, 5)).unwrap();

        assert_eq!(document.symbol(symbol).unwrap().at, GridPos::new(4, 5));
        assert_eq!(
            snap.map(|snap| (snap.at, snap.kind)),
            Some((GridPos::new(6, 5), SnapKind::Wire(target)))
        );
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.id == target && wire.contains(GridPos::new(6, 5))));
    }

    #[test]
    fn snapped_moves_still_follow_wires_attached_to_the_selection() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(2, 0), GridPos::new(6, 0))
            .unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));

        let snap = document.move_selection_snapped(GridPos::new(0, 2)).unwrap();

        assert!(snap.is_none());
        assert_eq!(document.symbol(symbol).unwrap().at, GridPos::new(0, 2));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.contains(GridPos::new(2, 2))));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.contains(GridPos::new(6, 0))));
    }

    #[test]
    fn snapped_moves_ignore_wires_on_the_original_net() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(2, 0), GridPos::new(6, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(4, 0), GridPos::new(4, 2))
            .unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));

        let snap = document.move_selection_snapped(GridPos::new(0, 1)).unwrap();

        assert!(snap.is_none());
        assert_eq!(document.symbol(symbol).unwrap().at, GridPos::new(0, 1));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.contains(GridPos::new(2, 1))));
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.contains(GridPos::new(4, 2))));
    }

    #[test]
    fn moving_a_symbol_keeps_the_far_pin_of_a_bridged_wire_connected() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(2, 0), GridPos::new(8, 0))
            .unwrap();
        let other = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(10, 0))
            .unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        for _ in 0..3 {
            document.move_selection(GridPos::new(0, 2)).unwrap();
            let at = document.symbol(symbol).unwrap().at;
            assert!(document
                .schematic()
                .wires()
                .any(|wire| wire.contains(GridPos::new(at.x + 2, at.y))));
            assert!(document
                .schematic()
                .wires()
                .any(|wire| wire.contains(GridPos::new(8, 0))));
            assert!(document.schematic().wires().count() <= 2);
        }
        assert_eq!(document.symbol(other).unwrap().at, GridPos::new(10, 0));
        assert_eq!(document.schematic().wires().count(), 2);
    }

    #[test]
    fn moving_a_symbol_follows_wires_on_both_pins_without_growth() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(-10, 0), GridPos::new(-2, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(2, 0), GridPos::new(10, 0))
            .unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        for _ in 0..3 {
            document.move_selection(GridPos::new(0, 2)).unwrap();
            let at = document.symbol(symbol).unwrap().at;
            for pin in [GridPos::new(at.x - 2, at.y), GridPos::new(at.x + 2, at.y)] {
                assert!(document.schematic().wires().any(|wire| wire.contains(pin)));
            }
            for end in [GridPos::new(-10, 0), GridPos::new(10, 0)] {
                assert!(document.schematic().wires().any(|wire| wire.contains(end)));
            }
            assert!(document.schematic().wires().count() <= 4);
            let mut geometry = BTreeSet::new();
            for wire in document.schematic().wires() {
                assert!(geometry.insert((wire.a, wire.b)), "duplicate wire {wire:?}");
            }
        }
        assert_eq!(document.schematic().wires().count(), 4);
    }

    #[test]
    fn moving_a_symbol_preserves_a_branch_on_the_wire_chain() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(2, 0), GridPos::new(6, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(6, 0), GridPos::new(10, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(6, 0), GridPos::new(6, 4))
            .unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        for _ in 0..3 {
            document.move_selection(GridPos::new(0, 2)).unwrap();
            let at = document.symbol(symbol).unwrap().at;
            let pin = GridPos::new(at.x + 2, at.y);
            assert!(document.schematic().wires().any(|wire| wire.contains(pin)));
            assert!(document
                .schematic()
                .wires()
                .any(|wire| wire.contains(GridPos::new(10, 0))));
            assert!(document
                .schematic()
                .wires()
                .any(|wire| wire.contains(GridPos::new(6, 4))));
            assert_eq!(
                document
                    .schematic()
                    .wires()
                    .filter(|wire| wire.contains(GridPos::new(6, 0)))
                    .count(),
                2
            );
            assert!(document.schematic().wires().count() <= 4);
        }
    }

    #[test]
    fn moving_a_symbol_keeps_an_interior_label_on_the_wire_chain() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(2, 0), GridPos::new(10, 0))
            .unwrap();
        let label = document.add_label(GridPos::new(6, 0), "N1").unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        for _ in 0..3 {
            document.move_selection(GridPos::new(0, 2)).unwrap();
            let at = document.symbol(symbol).unwrap().at;
            let pin = GridPos::new(at.x + 2, at.y);
            assert!(document.schematic().wires().any(|wire| wire.contains(pin)));
            let label = document.label(label).unwrap();
            assert_eq!(label.at, GridPos::new(6, 0));
            assert!(document
                .schematic()
                .wires()
                .any(|wire| wire.contains(label.at)));
            assert!(document
                .schematic()
                .wires()
                .any(|wire| wire.contains(GridPos::new(10, 0))));
            assert!(document.schematic().wires().count() <= 3);
        }
    }

    #[test]
    fn moving_a_symbol_with_pins_inside_a_wire_rebuilds_the_chain_once() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(-6, 0), GridPos::new(6, 0))
            .unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        for _ in 0..3 {
            document.move_selection(GridPos::new(0, 2)).unwrap();
            let at = document.symbol(symbol).unwrap().at;
            for pin in [GridPos::new(at.x - 2, at.y), GridPos::new(at.x + 2, at.y)] {
                assert!(document.schematic().wires().any(|wire| wire.contains(pin)));
            }
            for end in [GridPos::new(-6, 0), GridPos::new(6, 0)] {
                assert!(document.schematic().wires().any(|wire| wire.contains(end)));
            }
            assert!(document.schematic().wires().count() <= 3);
        }
        assert_eq!(document.schematic().wires().count(), 3);
    }

    #[test]
    fn undoing_symbol_moves_restores_the_wire_chain() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(2, 0), GridPos::new(6, 0))
            .unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        document.move_selection(GridPos::new(0, 2)).unwrap();
        let after_first = document.schematic().wires().count();
        document.move_selection(GridPos::new(2, 0)).unwrap();
        document.undo().unwrap();
        assert_eq!(document.schematic().wires().count(), after_first);
        document.undo().unwrap();
        assert_eq!(document.schematic().wires().count(), 1);
        assert!(document
            .schematic()
            .wires()
            .any(|wire| wire.a == GridPos::new(2, 0) && wire.b == GridPos::new(6, 0)));
    }

    #[test]
    fn resizes_only_free_wire_endpoints() {
        let mut document = Document::new();
        let wire = document
            .add_wire(GridPos::new(0, 0), GridPos::new(4, 0))
            .unwrap()[0];
        assert!(document
            .free_wire_endpoints()
            .iter()
            .any(|(id, end, point)| *id == wire
                && *end == WireEnd::B
                && *point == GridPos::new(4, 0)));
        document
            .move_wire_endpoint(wire, WireEnd::B, GridPos::new(2, 0))
            .unwrap();
        assert_eq!(document.wire(wire).unwrap().b, GridPos::new(2, 0));

        document
            .add_wire(GridPos::new(0, 0), GridPos::new(0, 4))
            .unwrap();
        assert!(matches!(
            document.move_wire_endpoint(wire, WireEnd::A, GridPos::new(-2, 0)),
            Err(ActionError::CannotResizeWire)
        ));
    }

    #[test]
    fn aligns_and_distributes_selected_symbols() {
        let mut document = Document::new();
        for at in [GridPos::new(0, 0), GridPos::new(10, 3), GridPos::new(20, 1)] {
            document.add_symbol(SymbolKind::Resistor, at).unwrap();
        }
        document.select_all();
        document.align_selection(Alignment::Top).unwrap();
        assert!(document
            .schematic()
            .symbols()
            .all(|symbol| symbol.at.y == 0));
        document
            .distribute_selection(DistributionAxis::Horizontal)
            .unwrap();
        let xs = document
            .schematic()
            .symbols()
            .map(|symbol| symbol.at.x)
            .collect::<Vec<_>>();
        assert_eq!(xs, vec![0, 10, 20]);
    }

    #[test]
    fn selects_a_whole_net_and_lists_names() {
        let mut document = Document::demo().unwrap();
        let at = GridPos::new(2, -10);
        assert_eq!(document.net_name_at(at).as_deref(), Some("Vc"));
        let count = document.select_net_at(at).unwrap();
        assert!(count > 2);
        assert!(document
            .selection()
            .iter()
            .any(|object| matches!(object, ObjectRef::Wire(_))));
        assert!(document.existing_net_names().contains(&"Vc".to_owned()));
    }

    #[test]
    fn rectangle_selection_distinguishes_enclosing_and_touching() {
        let mut document = Document::new();
        document
            .add_wire(GridPos::new(0, 0), GridPos::new(10, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(20, 0), GridPos::new(30, 0))
            .unwrap();
        let enclosing = document.select_in_rect(
            GridPos::new(5, 0),
            GridPos::new(15, 0),
            SelectionMode::Enclosing,
            SelectionChange::Replace,
        );
        assert_eq!(enclosing, 0);
        let touching = document.select_in_rect(
            GridPos::new(5, 0),
            GridPos::new(15, 0),
            SelectionMode::Touching,
            SelectionChange::Replace,
        );
        assert_eq!(touching, 1);
        document.select_in_rect(
            GridPos::new(20, 0),
            GridPos::new(30, 0),
            SelectionMode::Enclosing,
            SelectionChange::Add,
        );
        assert_eq!(document.selection().len(), 2);
        document.select_in_rect(
            GridPos::new(5, 0),
            GridPos::new(15, 0),
            SelectionMode::Touching,
            SelectionChange::Toggle,
        );
        assert_eq!(document.selection().len(), 1);
    }

    #[test]
    fn clipboard_copy_paste_and_duplicate_are_atomic() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        assert_eq!(document.copy_selection().unwrap(), 1);
        assert_eq!(document.paste_clipboard(GridPos::new(10, 10)).unwrap(), 1);
        let pasted = document
            .schematic()
            .symbols()
            .find(|candidate| candidate.id != symbol)
            .unwrap()
            .clone();
        assert_eq!(pasted.at, GridPos::new(10, 10));
        assert_eq!(pasted.label, "R2");
        assert!(document.selection().contains(&ObjectRef::Symbol(pasted.id)));
        document.undo().unwrap();
        assert_eq!(document.schematic().symbols().count(), 1);

        document.select_only(Some(ObjectRef::Symbol(symbol)));
        assert_eq!(document.duplicate_selection().unwrap(), 1);
        assert_eq!(document.schematic().symbols().count(), 2);
        document.undo().unwrap();
        assert_eq!(document.schematic().symbols().count(), 1);
    }

    #[test]
    fn group_rotation_and_mirror_transform_the_whole_selection() {
        let mut document = Document::new();
        let first = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        let second = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(10, 0))
            .unwrap();
        document.select_all();
        assert_eq!(
            document.selection_bounds(),
            Some((GridPos::new(-2, 0), GridPos::new(12, 0)))
        );
        document.rotate_selection().unwrap();
        assert_eq!(document.symbol(first).unwrap().at, GridPos::new(5, -5));
        assert_eq!(document.symbol(second).unwrap().at, GridPos::new(5, 5));
        assert_eq!(
            document.symbol(first).unwrap().orientation,
            Orientation::R90
        );
        document.undo().unwrap();
        document.select_all();
        document.mirror_selection().unwrap();
        assert_eq!(document.symbol(first).unwrap().at, GridPos::new(10, 0));
        assert_eq!(document.symbol(second).unwrap().at, GridPos::new(0, 0));
        assert!(document.symbol(first).unwrap().mirrored);
    }

    #[test]
    fn history_jump_restores_named_states() {
        let mut document = Document::new();
        let symbol = document
            .add_symbol(SymbolKind::Resistor, GridPos::new(0, 0))
            .unwrap();
        document
            .add_wire(GridPos::new(0, 0), GridPos::new(10, 0))
            .unwrap();
        document.select_only(Some(ObjectRef::Symbol(symbol)));
        document.rotate_selection().unwrap();
        assert_eq!(document.history_items().len(), 3);
        assert_eq!(document.history_position(), 3);
        document.undo_to(1).unwrap();
        assert_eq!(document.history_position(), 1);
        assert_eq!(document.schematic().wires().count(), 0);
        assert_eq!(document.schematic().symbols().count(), 1);
        document.undo_to(3).unwrap();
        assert_eq!(document.history_position(), 3);
        assert_eq!(document.schematic().wires().count(), 1);
        assert_eq!(
            document.symbol(symbol).unwrap().orientation,
            Orientation::R90
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[test]
        fn series_insertion_and_undo_preserve_invariants(
            length in 4_i32..40,
            offset in 0_i32..40,
        ) {
            let mut document = Document::new();
            let wire = document
                .add_wire(GridPos::new(0, 0), GridPos::new(length, 0))
                .unwrap()[0];
            let at = GridPos::new(offset.clamp(0, length), 0);
            let _ = document.insert_symbol_on_wire(SymbolKind::Resistor, wire, at).unwrap();
            prop_assert!(document.schematic().wires().count() <= 2);
            prop_assert!(document.schematic().validate().is_ok());
            document.undo().unwrap();
            prop_assert_eq!(document.schematic().wires().count(), 1);
            prop_assert!(document.schematic().validate().is_ok());
        }

        #[test]
        fn pasted_selection_keeps_relative_geometry(
            x in -20_i32..20,
            y in -20_i32..20,
            offset in 1_i32..10,
        ) {
            let mut document = Document::new();
            let symbol = document
                .add_symbol(SymbolKind::Resistor, GridPos::new(x, y))
                .unwrap();
            document.select_only(Some(ObjectRef::Symbol(symbol)));
            document.copy_selection().unwrap();
            let target = GridPos::new(x + offset, y + offset);
            document.paste_clipboard(target).unwrap();
            let pasted = document
                .schematic()
                .symbols()
                .find(|candidate| candidate.id != symbol)
                .unwrap();
            prop_assert_eq!(pasted.at, target);
            prop_assert!(document.schematic().validate().is_ok());
            document.undo().unwrap();
            prop_assert_eq!(document.schematic().symbols().count(), 1);
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
