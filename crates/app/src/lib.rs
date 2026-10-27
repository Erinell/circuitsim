//! CircuitSim egui application.

pub mod analysis;
pub mod app;
pub mod document;
pub mod export;
pub mod simulation;
pub mod ui;
pub mod view;

pub use app::CircuitApp;
pub use document::Document;

/// Window title used by native and web shells.
pub const APP_TITLE: &str = "CircuitSim";
