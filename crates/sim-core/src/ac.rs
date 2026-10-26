//! Small-signal AC analysis and deterministic frequency sweeps.

use std::collections::{BTreeMap, BTreeSet};

use nalgebra::{Complex, DMatrix, DVector};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dc::{solve_dc_internal, DcError, DcOptions, DcResult};
use crate::mna::MnaCircuit;
use crate::netlist::{ComponentId, ComponentKind, ComponentSpec, Netlist, NodeId};

const MAX_AC_POINTS: usize = 1_000_000;

/// Frequency list or generated sweep for an AC analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AcSweep {
    /// Explicit frequencies in hertz, strictly increasing and strictly positive.
    Explicit(Vec<f64>),
    /// Linearly spaced frequencies in hertz.
    Linear {
        /// First frequency in hertz.
        start: f64,
        /// Last frequency in hertz.
        stop: f64,
        /// Number of points, at least one.
        points: usize,
    },
    /// Logarithmically spaced frequencies in hertz.
    Logarithmic {
        /// First frequency in hertz.
        start: f64,
        /// Last frequency in hertz.
        stop: f64,
        /// Number of points, at least one.
        points: usize,
    },
}

impl AcSweep {
    /// Expands and validates the sweep into a deterministic frequency vector.
    pub fn frequencies(&self) -> Result<Vec<f64>, AcError> {
        match self {
            Self::Explicit(frequencies) => {
                if frequencies.is_empty() {
                    return Err(AcError::InvalidSweep {
                        reason: "explicit frequency list must not be empty",
                    });
                }
                if frequencies.len() > MAX_AC_POINTS {
                    return Err(AcError::InvalidSweep {
                        reason: "frequency count exceeds the supported maximum",
                    });
                }
                let mut previous = None;
                for frequency in frequencies {
                    if !frequency.is_finite() || *frequency <= 0.0 {
                        return Err(AcError::InvalidSweep {
                            reason: "frequencies must be finite and strictly positive",
                        });
                    }
                    if let Some(previous) = previous {
                        if *frequency <= previous {
                            return Err(AcError::InvalidSweep {
                                reason: "explicit frequencies must be strictly increasing",
                            });
                        }
                    }
                    previous = Some(*frequency);
                }
                Ok(frequencies.clone())
            }
            Self::Linear {
                start,
                stop,
                points,
            } => {
                validate_sweep_bounds(*start, *stop, *points)?;
                if *points == 1 {
                    return Ok(vec![*start]);
                }
                let span = *stop - *start;
                let denominator = (*points - 1) as f64;
                let mut frequencies = Vec::with_capacity(*points);
                for index in 0..*points {
                    let frequency = if index == 0 {
                        *start
                    } else if index + 1 == *points {
                        *stop
                    } else {
                        let fraction = index as f64 / denominator;
                        *start + span * fraction
                    };
                    if !frequency.is_finite() || frequency <= 0.0 {
                        return Err(AcError::InvalidSweep {
                            reason: "generated frequency is not finite and positive",
                        });
                    }
                    frequencies.push(frequency);
                }
                Ok(frequencies)
            }
            Self::Logarithmic {
                start,
                stop,
                points,
            } => {
                validate_sweep_bounds(*start, *stop, *points)?;
                if *points == 1 {
                    return Ok(vec![*start]);
                }
                let log_start = start.ln();
                let log_stop = stop.ln();
                let span = log_stop - log_start;
                let denominator = (*points - 1) as f64;
                let mut frequencies = Vec::with_capacity(*points);
                for index in 0..*points {
                    let frequency = if index == 0 {
                        *start
                    } else if index + 1 == *points {
                        *stop
                    } else {
                        let fraction = index as f64 / denominator;
                        (log_start + span * fraction).exp()
                    };
                    if !frequency.is_finite() || frequency <= 0.0 {
                        return Err(AcError::InvalidSweep {
                            reason: "generated frequency is not finite and positive",
                        });
                    }
                    frequencies.push(frequency);
                }
                Ok(frequencies)
            }
        }
    }
}

fn validate_sweep_bounds(start: f64, stop: f64, points: usize) -> Result<(), AcError> {
    if points == 0 {
        return Err(AcError::InvalidSweep {
            reason: "sweep points must be greater than zero",
        });
    }
    if points > MAX_AC_POINTS {
        return Err(AcError::InvalidSweep {
            reason: "sweep points exceed the supported maximum",
        });
    }
    if !start.is_finite() || start <= 0.0 {
        return Err(AcError::InvalidSweep {
            reason: "start frequency must be finite and strictly positive",
        });
    }
    if !stop.is_finite() || stop <= 0.0 {
        return Err(AcError::InvalidSweep {
            reason: "stop frequency must be finite and strictly positive",
        });
    }
    if start > stop {
        return Err(AcError::InvalidSweep {
            reason: "start frequency must not exceed stop frequency",
        });
    }
    if points > 1 && start == stop {
        return Err(AcError::InvalidSweep {
            reason: "stop frequency must exceed start frequency for multiple points",
        });
    }
    Ok(())
}

/// One independent-source phasor used to excite an AC analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcExcitation {
    /// Voltage or current source to excite.
    pub component: ComponentId,
    /// Peak magnitude in volts or amperes.
    pub magnitude: f64,
    /// Phase in degrees relative to the analysis reference.
    pub phase_degrees: f64,
}

/// Options for a small-signal AC analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcOptions {
    /// Frequencies to evaluate.
    pub sweep: AcSweep,
    /// Independent-source excitations; sources absent here are zero.
    pub excitations: Vec<AcExcitation>,
    /// Options used to compute the DC operating point.
    pub solver: DcOptions,
}

impl Default for AcOptions {
    fn default() -> Self {
        Self {
            sweep: AcSweep::Logarithmic {
                start: 1.0,
                stop: 1_000_000.0,
                points: 101,
            },
            excitations: Vec::new(),
            solver: DcOptions::default(),
        }
    }
}

/// Complex phasor in rectangular coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Phasor {
    /// Real part.
    pub re: f64,
    /// Imaginary part.
    pub im: f64,
}

impl Phasor {
    /// Zero phasor.
    pub const ZERO: Self = Self { re: 0.0, im: 0.0 };

    /// Creates a phasor from its rectangular components.
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    /// Creates a phasor from a magnitude and a phase in radians.
    pub fn from_polar(magnitude: f64, phase_radians: f64) -> Self {
        Self {
            re: magnitude * phase_radians.cos(),
            im: magnitude * phase_radians.sin(),
        }
    }

    /// Returns the magnitude of the phasor.
    pub fn magnitude(&self) -> f64 {
        self.re.hypot(self.im)
    }

    /// Returns the phase in radians, in `[-π, π]`.
    pub fn phase(&self) -> f64 {
        self.im.atan2(self.re)
    }

    /// Returns the phase in degrees, in `[-180, 180]`.
    pub fn phase_degrees(&self) -> f64 {
        self.phase().to_degrees()
    }
}

/// AC response at one frequency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcSample {
    /// Frequency in hertz.
    pub frequency: f64,
    /// Node-voltage phasors, including ground at zero.
    pub node_voltages: BTreeMap<NodeId, Phasor>,
    /// Branch-current phasors for voltage sources, inductors, and opamp outputs.
    pub branch_currents: BTreeMap<ComponentId, Phasor>,
}

impl AcSample {
    /// Returns a node-voltage phasor when the node exists in the result.
    pub fn node_voltage(&self, node: NodeId) -> Option<Phasor> {
        self.node_voltages.get(&node).copied()
    }

    /// Returns a branch-current phasor for a voltage source, inductor, or opamp.
    pub fn branch_current(&self, component: ComponentId) -> Option<Phasor> {
        self.branch_currents.get(&component).copied()
    }
}

/// Result of a small-signal AC analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcResult {
    /// Operating point used to linearize nonlinear devices.
    pub operating_point: DcResult,
    /// Responses in the order of the sweep.
    pub samples: Vec<AcSample>,
}

impl AcResult {
    /// Returns the response at an exact frequency in the sweep.
    pub fn sample_at_frequency(&self, frequency: f64) -> Option<&AcSample> {
        self.samples
            .iter()
            .find(|sample| sample.frequency == frequency)
    }
}

/// Error raised by an AC analysis or a frequency sweep.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum AcError {
    #[error(transparent)]
    OperatingPoint(#[from] DcError),
    #[error("invalid AC sweep: {reason}")]
    InvalidSweep { reason: &'static str },
    #[error("invalid AC excitation {index}: {reason}")]
    InvalidExcitation { index: usize, reason: &'static str },
    #[error("AC matrix is singular at {frequency:.6e} Hz")]
    SingularMatrix { frequency: f64 },
    #[error("AC system contains a non-finite value at {frequency:.6e} Hz")]
    NonFiniteSystem { frequency: f64 },
    #[error("AC solution contains a non-finite value at {frequency:.6e} Hz")]
    NonFiniteSolution { frequency: f64 },
}

/// Solves a small-signal AC sweep around a DC operating point.
pub fn solve_ac(netlist: &Netlist, options: AcOptions) -> Result<AcResult, AcError> {
    let AcOptions {
        sweep,
        excitations,
        solver,
    } = options;
    let frequencies = sweep.frequencies()?;
    let source_phasors = build_source_phasors(netlist, &excitations)?;
    let solved = solve_dc_internal(netlist, solver).map_err(AcError::OperatingPoint)?;

    let mut samples = Vec::with_capacity(frequencies.len());
    for frequency in frequencies {
        let (matrix, right_hand_side) =
            solved
                .circuit
                .assemble_ac(&solved.solution, frequency, &source_phasors);
        if !matrix_is_finite(&matrix) || !vector_is_finite(&right_hand_side) {
            return Err(AcError::NonFiniteSystem { frequency });
        }
        let solution = if solved.circuit.size() == 0 {
            DVector::zeros(0)
        } else {
            matrix
                .lu()
                .solve(&right_hand_side)
                .ok_or(AcError::SingularMatrix { frequency })?
        };
        if !vector_is_finite(&solution) {
            return Err(AcError::NonFiniteSolution { frequency });
        }
        samples.push(build_sample(netlist, &solved.circuit, &solution, frequency));
    }

    Ok(AcResult {
        operating_point: solved.result,
        samples,
    })
}

fn build_source_phasors(
    netlist: &Netlist,
    excitations: &[AcExcitation],
) -> Result<BTreeMap<ComponentId, Complex<f64>>, AcError> {
    let mut phasors = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (index, excitation) in excitations.iter().enumerate() {
        if !seen.insert(excitation.component) {
            return Err(AcError::InvalidExcitation {
                index,
                reason: "duplicate excitation for the same component",
            });
        }
        let component =
            component_by_id(netlist, excitation.component).ok_or(AcError::InvalidExcitation {
                index,
                reason: "excitation target does not exist",
            })?;
        if !matches!(
            &component.kind,
            ComponentKind::VoltageSource | ComponentKind::CurrentSource
        ) {
            return Err(AcError::InvalidExcitation {
                index,
                reason: "excitation target must be a voltage or current source",
            });
        }
        if !excitation.magnitude.is_finite() || excitation.magnitude < 0.0 {
            return Err(AcError::InvalidExcitation {
                index,
                reason: "magnitude must be finite and non-negative",
            });
        }
        if !excitation.phase_degrees.is_finite() {
            return Err(AcError::InvalidExcitation {
                index,
                reason: "phase must be finite",
            });
        }
        let phase = excitation.phase_degrees.to_radians();
        phasors.insert(
            excitation.component,
            Complex::new(
                excitation.magnitude * phase.cos(),
                excitation.magnitude * phase.sin(),
            ),
        );
    }
    Ok(phasors)
}

fn component_by_id(netlist: &Netlist, component: ComponentId) -> Option<&ComponentSpec> {
    let index = usize::try_from(component.0).ok()?;
    netlist
        .components
        .get(index)
        .filter(|candidate| candidate.id == component)
}

fn build_sample(
    netlist: &Netlist,
    circuit: &MnaCircuit,
    solution: &DVector<Complex<f64>>,
    frequency: f64,
) -> AcSample {
    let mut node_voltages = BTreeMap::new();
    for node in &netlist.nodes {
        let value = circuit
            .node_phasor(solution, node.id)
            .unwrap_or_else(|| Complex::new(0.0, 0.0));
        node_voltages.insert(node.id, to_phasor(value));
    }

    let mut branch_currents = BTreeMap::new();
    for (index, component) in circuit.branch_components().iter().enumerate() {
        let value = solution[circuit.node_count() + index];
        branch_currents.insert(*component, to_phasor(value));
    }

    AcSample {
        frequency,
        node_voltages,
        branch_currents,
    }
}

fn to_phasor(value: Complex<f64>) -> Phasor {
    Phasor::new(value.re, value.im)
}

fn matrix_is_finite(matrix: &DMatrix<Complex<f64>>) -> bool {
    matrix
        .iter()
        .all(|value| value.re.is_finite() && value.im.is_finite())
}

fn vector_is_finite(vector: &DVector<Complex<f64>>) -> bool {
    vector
        .iter()
        .all(|value| value.re.is_finite() && value.im.is_finite())
}
