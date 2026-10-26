//! Core circuit models shared by the simulator, editor, and worker.

pub mod ac;
pub mod dc;
mod mna;
pub mod netlist;
mod newton;
pub mod transient;
pub mod units;

pub use ac::{solve_ac, AcError, AcExcitation, AcOptions, AcResult, AcSample, AcSweep, Phasor};
pub use dc::{solve_dc, DcError, DcOptions, DcResult, NewtonStrategy};
pub use mna::MnaError;
pub use transient::{
    simulate_transient, ComponentEnergy, EnergyAudit, TransientAction, TransientError,
    TransientEvent, TransientOptions, TransientResult, TransientSample,
};
