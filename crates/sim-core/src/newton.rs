//! Newton iterations and continuation strategies shared by DC and transient.

use nalgebra::DVector;
use serde::{Deserialize, Serialize};

use crate::mna::{max_abs, AssemblyMode, MnaCircuit};

/// Strategy used to solve a nonlinear MNA system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NewtonStrategy {
    /// Try the plain solve, then the continuation fallbacks when it fails.
    Auto,
    /// Only run the plain Newton iteration.
    Plain,
    /// Use a decreasing conductance-to-ground continuation.
    Gmin,
    /// Use a source-scaling continuation.
    SourceStepping,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NewtonConfig {
    pub max_iterations: usize,
    pub abs_tolerance: f64,
    pub rel_tolerance: f64,
    pub residual_tolerance: f64,
    pub strategy: NewtonStrategy,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NewtonError {
    SingularMatrix { iteration: usize },
    NoConvergence { iterations: usize, residual: f64 },
    NonFiniteSolution { iteration: usize },
}

#[derive(Clone, Debug)]
pub(crate) struct NewtonOutcome {
    pub solution: DVector<f64>,
    pub iterations: usize,
    pub residual: f64,
}

const GMIN_VALUES: [f64; 7] = [1e-3, 1e-4, 1e-5, 1e-6, 1e-8, 1e-10, 0.0];
const SOURCE_SCALES: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
const MIN_DAMPING: f64 = 0.05;

pub(crate) fn solve_robust(
    circuit: &MnaCircuit,
    config: NewtonConfig,
    mode: AssemblyMode<'_>,
    initial: &DVector<f64>,
    initial_junction_voltages: &[f64],
) -> Result<NewtonOutcome, NewtonError> {
    match config.strategy {
        NewtonStrategy::Plain => solve_once(
            circuit,
            config,
            mode,
            initial,
            initial_junction_voltages,
            0.0,
            1.0,
        ),
        NewtonStrategy::Gmin => {
            solve_gmin(circuit, config, mode, initial, initial_junction_voltages)
        }
        NewtonStrategy::SourceStepping => {
            solve_source_stepping(circuit, config, mode, initial, initial_junction_voltages)
        }
        NewtonStrategy::Auto => {
            let plain = solve_once(
                circuit,
                config,
                mode,
                initial,
                initial_junction_voltages,
                0.0,
                1.0,
            );
            let original = match plain {
                Ok(outcome) => return Ok(outcome),
                Err(error) => error,
            };
            if let Ok(outcome) =
                solve_gmin(circuit, config, mode, initial, initial_junction_voltages)
            {
                return Ok(outcome);
            }
            if let Ok(outcome) =
                solve_source_stepping(circuit, config, mode, initial, initial_junction_voltages)
            {
                return Ok(outcome);
            }
            Err(original)
        }
    }
}

fn solve_gmin(
    circuit: &MnaCircuit,
    config: NewtonConfig,
    mode: AssemblyMode<'_>,
    initial: &DVector<f64>,
    initial_junction_voltages: &[f64],
) -> Result<NewtonOutcome, NewtonError> {
    let mut guess = initial.clone();
    let mut junction_voltages = initial_junction_voltages.to_vec();
    let mut total_iterations = 0;
    let mut last_residual = f64::INFINITY;

    for gmin in GMIN_VALUES {
        let outcome = solve_once(circuit, config, mode, &guess, &junction_voltages, gmin, 1.0)?;
        total_iterations += outcome.iterations;
        last_residual = outcome.residual;
        guess = outcome.solution;
        junction_voltages = circuit.junction_voltages(&guess);
    }

    Ok(NewtonOutcome {
        solution: guess,
        iterations: total_iterations,
        residual: last_residual,
    })
}

fn solve_source_stepping(
    circuit: &MnaCircuit,
    config: NewtonConfig,
    mode: AssemblyMode<'_>,
    initial: &DVector<f64>,
    initial_junction_voltages: &[f64],
) -> Result<NewtonOutcome, NewtonError> {
    let mut guess = initial.clone();
    let mut junction_voltages = initial_junction_voltages.to_vec();
    let mut total_iterations = 0;
    let mut last_residual = f64::INFINITY;

    for source_scale in SOURCE_SCALES {
        let outcome = solve_once(
            circuit,
            config,
            mode,
            &guess,
            &junction_voltages,
            0.0,
            source_scale,
        )?;
        total_iterations += outcome.iterations;
        last_residual = outcome.residual;
        guess = outcome.solution;
        junction_voltages = circuit.junction_voltages(&guess);
    }

    Ok(NewtonOutcome {
        solution: guess,
        iterations: total_iterations,
        residual: last_residual,
    })
}

fn solve_once(
    circuit: &MnaCircuit,
    config: NewtonConfig,
    mode: AssemblyMode<'_>,
    initial: &DVector<f64>,
    initial_junction_voltages: &[f64],
    gmin: f64,
    source_scale: f64,
) -> Result<NewtonOutcome, NewtonError> {
    let mut solution = initial.clone();
    let mut junction_voltages = initial_junction_voltages.to_vec();
    let nonlinear = circuit.has_nonlinear_devices();
    let mut current_residual = if nonlinear {
        circuit.residual(&solution, mode, gmin, source_scale)
    } else {
        f64::INFINITY
    };
    let mut last_residual = current_residual;

    for iteration in 1..=config.max_iterations {
        circuit.limit_junction_voltages(&solution, &mut junction_voltages);
        let (conductance, sources) =
            circuit.assemble(&solution, &junction_voltages, mode, gmin, source_scale);
        let next = conductance
            .lu()
            .solve(&sources)
            .ok_or(NewtonError::SingularMatrix { iteration })?;
        if next.iter().any(|value| !value.is_finite()) {
            return Err(NewtonError::NonFiniteSolution { iteration });
        }

        let mut candidate = next;
        let mut candidate_residual = circuit.residual(&candidate, mode, gmin, source_scale);
        if !candidate_residual.is_finite() {
            return Err(NewtonError::NonFiniteSolution { iteration });
        }

        if nonlinear && current_residual.is_finite() {
            let full_step_residual = candidate_residual;
            if full_step_residual > current_residual {
                let direction = &candidate - &solution;
                let mut damping = 0.5;
                while damping >= MIN_DAMPING {
                    let damped = &solution + damping * &direction;
                    let residual = circuit.residual(&damped, mode, gmin, source_scale);
                    if residual.is_finite() && residual < current_residual {
                        candidate = damped;
                        candidate_residual = residual;
                        break;
                    }
                    damping *= 0.5;
                }
            }
        }

        let update = max_abs(&(&candidate - &solution));
        solution = candidate;
        current_residual = candidate_residual;
        last_residual = candidate_residual;

        if !nonlinear {
            return Ok(NewtonOutcome {
                solution,
                iterations: iteration,
                residual: candidate_residual,
            });
        }

        let scale = max_abs(&solution);
        let update_limit = config.abs_tolerance + config.rel_tolerance * scale;
        if update <= update_limit && candidate_residual <= config.residual_tolerance {
            return Ok(NewtonOutcome {
                solution,
                iterations: iteration,
                residual: candidate_residual,
            });
        }
    }

    Err(NewtonError::NoConvergence {
        iterations: config.max_iterations,
        residual: last_residual,
    })
}
