//! DC operating-point analysis using modified nodal analysis.

use std::collections::BTreeMap;

use nalgebra::DVector;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::mna::{AssemblyMode, MnaCircuit, MnaError};
use crate::netlist::{ComponentId, Netlist, NodeId};
pub use crate::newton::NewtonStrategy;
use crate::newton::{solve_robust, NewtonConfig, NewtonError};

/// Solver options shared by DC, transient, and AC operating-point analyses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DcOptions {
    /// Maximum number of Newton iterations.
    pub max_iterations: usize,
    /// Absolute tolerance applied to the Newton update.
    pub abs_tolerance: f64,
    /// Relative tolerance applied to the Newton update.
    pub rel_tolerance: f64,
    /// Acceptable maximum residual in amperes or volts.
    pub residual_tolerance: f64,
    /// Nonlinear strategy used by the solver.
    pub strategy: NewtonStrategy,
}

impl Default for DcOptions {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            abs_tolerance: 1e-10,
            rel_tolerance: 1e-7,
            residual_tolerance: 1e-7,
            strategy: NewtonStrategy::Auto,
        }
    }
}

impl DcOptions {
    pub(crate) fn validate(&self) -> Result<(), DcError> {
        if self.max_iterations == 0 {
            return Err(DcError::InvalidOptions {
                reason: "max_iterations must be greater than zero",
            });
        }
        if !self.abs_tolerance.is_finite() || self.abs_tolerance <= 0.0 {
            return Err(DcError::InvalidOptions {
                reason: "abs_tolerance must be finite and strictly positive",
            });
        }
        if !self.rel_tolerance.is_finite() || self.rel_tolerance < 0.0 {
            return Err(DcError::InvalidOptions {
                reason: "rel_tolerance must be finite and non-negative",
            });
        }
        if !self.residual_tolerance.is_finite() || self.residual_tolerance <= 0.0 {
            return Err(DcError::InvalidOptions {
                reason: "residual_tolerance must be finite and strictly positive",
            });
        }
        Ok(())
    }
}

/// Error raised by a DC analysis or an operating-point/Newton step.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum DcError {
    #[error(transparent)]
    Circuit(#[from] MnaError),
    #[error(
        "MNA matrix is singular at iteration {iteration}; check floating nodes or source conflicts"
    )]
    SingularMatrix { iteration: usize },
    #[error(
        "Newton solver did not converge after {iterations} iterations (residual {residual:.3e})"
    )]
    NoConvergence { iterations: usize, residual: f64 },
    #[error("solution contains a non-finite value at iteration {iteration}")]
    NonFiniteSolution { iteration: usize },
    #[error("invalid solver options: {reason}")]
    InvalidOptions { reason: &'static str },
}

/// DC operating-point solution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DcResult {
    /// Voltage of every netlist node in volts, including ground at zero.
    pub node_voltages: BTreeMap<NodeId, f64>,
    /// Current of every branch-forming component in amperes.
    ///
    /// Branches are voltage sources, inductors, and operational-amplifier outputs.
    pub branch_currents: BTreeMap<ComponentId, f64>,
    /// Number of linear systems solved.
    pub iterations: usize,
    /// Maximum residual of the final linearized system.
    pub residual: f64,
}

impl DcResult {
    /// Returns a node voltage when the node exists in the result.
    pub fn node_voltage(&self, node: NodeId) -> Option<f64> {
        self.node_voltages.get(&node).copied()
    }

    /// Returns a branch current for a voltage source, inductor, or opamp output.
    pub fn branch_current(&self, component: ComponentId) -> Option<f64> {
        self.branch_currents.get(&component).copied()
    }
}

/// Solved operating point together with the internal MNA state.
pub(crate) struct SolvedDc {
    pub(crate) circuit: MnaCircuit,
    pub(crate) solution: DVector<f64>,
    pub(crate) result: DcResult,
}

/// Solves the DC operating point of a validated netlist.
pub fn solve_dc(netlist: &Netlist, options: DcOptions) -> Result<DcResult, DcError> {
    Ok(solve_dc_internal(netlist, options)?.result)
}

pub(crate) fn solve_dc_internal(
    netlist: &Netlist,
    options: DcOptions,
) -> Result<SolvedDc, DcError> {
    options.validate()?;
    let circuit = MnaCircuit::from_netlist(netlist)?;
    let size = circuit.size();

    if size == 0 {
        let solution = DVector::zeros(0);
        let result = build_result(netlist, &circuit, &solution, 0, 0.0);
        return Ok(SolvedDc {
            circuit,
            solution,
            result,
        });
    }

    let config = NewtonConfig {
        max_iterations: options.max_iterations,
        abs_tolerance: options.abs_tolerance,
        rel_tolerance: options.rel_tolerance,
        residual_tolerance: options.residual_tolerance,
        strategy: options.strategy,
    };
    let initial = DVector::zeros(size);
    let junction_voltages = vec![0.0; circuit.nonlinear_count()];
    let outcome = solve_robust(
        &circuit,
        config,
        AssemblyMode::Dc,
        &initial,
        &junction_voltages,
    )
    .map_err(map_newton_error)?;

    let result = build_result(
        netlist,
        &circuit,
        &outcome.solution,
        outcome.iterations,
        outcome.residual,
    );
    Ok(SolvedDc {
        circuit,
        solution: outcome.solution,
        result,
    })
}

pub(crate) fn map_newton_error(error: NewtonError) -> DcError {
    match error {
        NewtonError::SingularMatrix { iteration } => DcError::SingularMatrix { iteration },
        NewtonError::NoConvergence {
            iterations,
            residual,
        } => DcError::NoConvergence {
            iterations,
            residual,
        },
        NewtonError::NonFiniteSolution { iteration } => DcError::NonFiniteSolution { iteration },
    }
}

fn build_result(
    netlist: &Netlist,
    circuit: &MnaCircuit,
    solution: &DVector<f64>,
    iterations: usize,
    residual: f64,
) -> DcResult {
    let mut node_voltages = BTreeMap::new();
    for node in &netlist.nodes {
        let voltage = circuit.node_voltage(solution, node.id).unwrap_or(0.0);
        node_voltages.insert(node.id, voltage);
    }

    let mut branch_currents = BTreeMap::new();
    for (index, component) in circuit.branch_components().iter().enumerate() {
        branch_currents.insert(*component, solution[circuit.node_count() + index]);
    }

    DcResult {
        node_voltages,
        branch_currents,
        iterations,
        residual,
    }
}
