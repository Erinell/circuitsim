//! Transient analysis with discrete events and energy accounting.

use std::collections::BTreeMap;

use nalgebra::DVector;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dc::{map_newton_error, DcError, DcOptions};
use crate::mna::{AssemblyMode, MnaCircuit, MnaError};
use crate::netlist::{ComponentId, ComponentKind, Netlist, NodeId, ParamValue};
use crate::newton::{solve_robust, NewtonConfig};

/// One change applied between two transient time points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TransientAction {
    /// Replaces the `dc` value of a voltage or current source.
    SetSourceValue { component: ComponentId, value: f64 },
    /// Replaces the `closed` state of a switch.
    SetSwitchClosed {
        component: ComponentId,
        closed: bool,
    },
}

impl TransientAction {
    /// Returns the component affected by this action.
    pub fn component(&self) -> ComponentId {
        match self {
            Self::SetSourceValue { component, .. } | Self::SetSwitchClosed { component, .. } => {
                *component
            }
        }
    }
}

/// Discrete event applied after the sample at its exact time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransientEvent {
    /// Time in seconds.
    pub time: f64,
    /// State change applied at `time`.
    pub action: TransientAction,
}

/// Options controlling a transient analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransientOptions {
    /// Initial time in seconds.
    pub start_time: f64,
    /// Final time in seconds.
    pub stop_time: f64,
    /// Nominal backward-Euler step in seconds.
    pub step: f64,
    /// Maximum number of accepted integration intervals.
    pub max_steps: usize,
    /// Newton and continuation options reused at each step.
    pub solver: DcOptions,
    /// Discrete source and switch events.
    pub events: Vec<TransientEvent>,
}

impl Default for TransientOptions {
    fn default() -> Self {
        Self {
            start_time: 0.0,
            stop_time: 1e-3,
            step: 1e-5,
            max_steps: 1_000_000,
            solver: DcOptions::default(),
            events: Vec::new(),
        }
    }
}

/// One time sample of a transient simulation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransientSample {
    /// Time in seconds.
    pub time: f64,
    /// Voltage of every netlist node, including ground.
    pub node_voltages: BTreeMap<NodeId, f64>,
    /// Current of every branch-forming component.
    pub branch_currents: BTreeMap<ComponentId, f64>,
}

impl TransientSample {
    /// Returns a node voltage when the node is present.
    pub fn node_voltage(&self, node: NodeId) -> Option<f64> {
        self.node_voltages.get(&node).copied()
    }

    /// Returns a branch current for a voltage source, inductor, or opamp output.
    pub fn branch_current(&self, component: ComponentId) -> Option<f64> {
        self.branch_currents.get(&component).copied()
    }
}

/// Energy accumulated by one component during a transient analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentEnergy {
    /// Absorbed energy in joules; reactive components report their stored change.
    pub absorbed: f64,
    /// Change in stored energy in joules for capacitors and inductors.
    pub stored_delta: f64,
}

/// Energy audit of a transient analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnergyAudit {
    /// Energy delivered by independent sources and opamp outputs, in joules.
    pub supplied: f64,
    /// Energy dissipated by resistors, switches, diodes, LEDs, and Zeners.
    pub dissipated: f64,
    /// Final minus initial energy stored in capacitors and inductors.
    pub stored_delta: f64,
    /// Numerical balance residual `supplied - dissipated - stored_delta`.
    pub residual: f64,
    /// Per-component energy, ordered by component identifier.
    pub components: BTreeMap<ComponentId, ComponentEnergy>,
}

/// Transient solution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransientResult {
    /// Samples ordered by increasing time, including the initial operating point.
    pub samples: Vec<TransientSample>,
    /// Energy audit over the complete simulation.
    pub energy: EnergyAudit,
    /// Number of integration intervals.
    pub steps: usize,
    /// Total number of Newton linear systems solved, including continuation stages.
    pub iterations: usize,
}

/// Error raised by a transient analysis.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum TransientError {
    #[error("invalid transient options: {reason}")]
    InvalidOptions { reason: &'static str },
    #[error("invalid transient event {index}: {reason}")]
    InvalidEvent { index: usize, reason: &'static str },
    #[error(transparent)]
    Circuit(#[from] MnaError),
    #[error("initial operating point failed: {source}")]
    InitialDc {
        #[source]
        source: DcError,
    },
    #[error("transient step at {time:.6e} s failed: {source}")]
    Step {
        time: f64,
        #[source]
        source: DcError,
    },
    #[error("transient analysis exceeds the maximum of {limit} integration steps")]
    TooManySteps { limit: usize },
    #[error("live update requires the same netlist topology")]
    TopologyChanged,
}

/// Solves a transient analysis with backward Euler and discrete events.
pub fn simulate_transient(
    netlist: &Netlist,
    options: TransientOptions,
) -> Result<TransientResult, TransientError> {
    validate_options(&options)?;
    let events = prepare_events(netlist, &options)?;
    let timeline = build_timeline(&options, &events)?;

    let mut working = netlist.clone();
    let mut circuit = MnaCircuit::from_netlist(&working)?;
    let initial = solve_initial(&circuit, &options.solver)?;
    let mut previous = initial.solution;
    let mut junction_voltages = circuit.junction_voltages(&previous);
    let mut iterations = initial.iterations;
    let mut samples = vec![build_sample(
        &working,
        &circuit,
        &previous,
        options.start_time,
    )];
    let initial_stored = stored_energies(&working, &circuit, &previous);
    let mut absorbed = BTreeMap::new();

    let mut event_index = 0;
    let mut steps = 0;
    for window in timeline.windows(2) {
        let time = window[0];
        let next_time = window[1];

        let mut changed = false;
        while event_index < events.len() && events[event_index].event.time <= time {
            let event = &events[event_index];
            apply_action(&mut working, &event.event.action, event.original_index)?;
            event_index += 1;
            changed = true;
        }
        if changed {
            circuit = MnaCircuit::from_netlist(&working)?;
            junction_voltages = circuit.junction_voltages(&previous);
        }

        let step = next_time - time;
        let mode = AssemblyMode::Transient {
            previous: &previous,
            step,
        };
        let outcome = solve_step(
            &circuit,
            &options.solver,
            mode,
            &previous,
            &junction_voltages,
            next_time,
        )?;
        iterations += outcome.iterations;
        accumulate_interval(&working, &circuit, &outcome.solution, step, &mut absorbed);

        previous = outcome.solution;
        junction_voltages = circuit.junction_voltages(&previous);
        samples.push(build_sample(&working, &circuit, &previous, next_time));
        steps += 1;
    }

    let final_stored = stored_energies(&working, &circuit, &previous);
    let energy = build_energy_audit(&working, &absorbed, &initial_stored, &final_stored);

    Ok(TransientResult {
        samples,
        energy,
        steps,
        iterations,
    })
}

/// Incremental backward-Euler transient session.
///
/// The session owns the state between two fixed steps. The caller supplies the
/// pacing; it is intended for real-time probes and never exposes a partial
/// sample as a complete `TransientResult`.
pub struct TransientSession {
    working: Netlist,
    circuit: MnaCircuit,
    previous: DVector<f64>,
    junction_voltages: Vec<f64>,
    time: f64,
    step: f64,
    solver: DcOptions,
    events: Vec<IndexedEvent>,
    event_index: usize,
    iterations: usize,
    steps: usize,
    initial_stored: BTreeMap<ComponentId, f64>,
    absorbed: BTreeMap<ComponentId, f64>,
    samples: Vec<TransientSample>,
}

impl TransientSession {
    /// Creates a session and computes its initial operating point.
    pub fn new(netlist: &Netlist, options: &TransientOptions) -> Result<Self, TransientError> {
        validate_options(options)?;
        let events = prepare_events(netlist, options)?;
        let working = netlist.clone();
        let circuit = MnaCircuit::from_netlist(&working)?;
        let initial = solve_initial(&circuit, &options.solver)?;
        let previous = initial.solution;
        let junction_voltages = circuit.junction_voltages(&previous);
        let initial_stored = stored_energies(&working, &circuit, &previous);
        let sample = build_sample(&working, &circuit, &previous, options.start_time);
        Ok(Self {
            working,
            circuit,
            previous,
            junction_voltages,
            time: options.start_time,
            step: options.step,
            solver: options.solver.clone(),
            events,
            event_index: 0,
            iterations: initial.iterations,
            steps: 0,
            initial_stored,
            absorbed: BTreeMap::new(),
            samples: vec![sample],
        })
    }

    /// Returns the latest sample.
    pub fn sample(&self) -> Option<&TransientSample> {
        self.samples.last()
    }

    /// Removes and returns accumulated samples.
    pub fn drain_samples(&mut self) -> Vec<TransientSample> {
        std::mem::take(&mut self.samples)
    }

    /// Current simulated time.
    pub const fn time(&self) -> f64 {
        self.time
    }

    /// Applies parameter changes while preserving time and dynamic state.
    pub fn update_netlist(&mut self, netlist: &Netlist) -> Result<(), TransientError> {
        if !same_topology(&self.working, netlist) {
            return Err(TransientError::TopologyChanged);
        }
        let circuit = MnaCircuit::from_netlist(netlist)?;
        self.working = netlist.clone();
        self.circuit = circuit;
        self.junction_voltages = self.circuit.junction_voltages(&self.previous);
        Ok(())
    }

    /// Advances exactly one fixed step without building a sample.
    ///
    /// Pacing callers use this instead of [`Self::step`] and request a
    /// [`Self::snapshot`] only when they need to display the state, which avoids
    /// one pair of `BTreeMap`s per integration step.
    pub fn advance(&mut self) -> Result<(), TransientError> {
        let time = self.time;
        let next_time = time + self.step;
        if !next_time.is_finite() || next_time <= time {
            return Err(TransientError::InvalidOptions {
                reason: "transient time overflow",
            });
        }

        let mut changed = false;
        while self.event_index < self.events.len()
            && self.events[self.event_index].event.time <= time
        {
            let event = &self.events[self.event_index];
            apply_action(&mut self.working, &event.event.action, event.original_index)?;
            self.event_index += 1;
            changed = true;
        }
        if changed {
            self.circuit = MnaCircuit::from_netlist(&self.working)?;
            self.junction_voltages = self.circuit.junction_voltages(&self.previous);
        }

        let step = next_time - time;
        let mode = AssemblyMode::Transient {
            previous: &self.previous,
            step,
        };
        let outcome = solve_step(
            &self.circuit,
            &self.solver,
            mode,
            &self.previous,
            &self.junction_voltages,
            next_time,
        )?;
        self.iterations += outcome.iterations;
        accumulate_interval(
            &self.working,
            &self.circuit,
            &outcome.solution,
            step,
            &mut self.absorbed,
        );
        self.previous = outcome.solution;
        self.junction_voltages = self.circuit.junction_voltages(&self.previous);
        self.time = next_time;
        self.steps += 1;
        Ok(())
    }

    /// Builds a sample of the current state on demand.
    pub fn snapshot(&self) -> TransientSample {
        build_sample(&self.working, &self.circuit, &self.previous, self.time)
    }

    /// Advances exactly one fixed step and returns the resulting sample.
    ///
    /// The sample is also retained for [`Self::finish`]; use [`Self::advance`]
    /// and [`Self::snapshot`] when every intermediate sample is not needed.
    pub fn step(&mut self) -> Result<TransientSample, TransientError> {
        self.advance()?;
        let sample = self.snapshot();
        self.samples.push(sample.clone());
        Ok(sample)
    }

    /// Number of integration intervals accepted so far.
    pub const fn steps(&self) -> usize {
        self.steps
    }

    /// Finishes the session with a complete energy audit.
    pub fn finish(self) -> TransientResult {
        let final_stored = stored_energies(&self.working, &self.circuit, &self.previous);
        let energy = build_energy_audit(
            &self.working,
            &self.absorbed,
            &self.initial_stored,
            &final_stored,
        );
        TransientResult {
            samples: self.samples,
            energy,
            steps: self.steps,
            iterations: self.iterations,
        }
    }
}

fn same_topology(current: &Netlist, next: &Netlist) -> bool {
    current.nodes == next.nodes
        && current.components.len() == next.components.len()
        && current
            .components
            .iter()
            .zip(&next.components)
            .all(|(left, right)| {
                left.id == right.id
                    && left.kind == right.kind
                    && left.label == right.label
                    && left.pins == right.pins
            })
}

fn validate_options(options: &TransientOptions) -> Result<(), TransientError> {
    if !options.start_time.is_finite() || !options.stop_time.is_finite() {
        return Err(TransientError::InvalidOptions {
            reason: "start_time and stop_time must be finite",
        });
    }
    if options.stop_time < options.start_time {
        return Err(TransientError::InvalidOptions {
            reason: "stop_time must be greater than or equal to start_time",
        });
    }
    if !options.step.is_finite() || options.step <= 0.0 {
        return Err(TransientError::InvalidOptions {
            reason: "step must be finite and strictly positive",
        });
    }
    if options.max_steps == 0 {
        return Err(TransientError::InvalidOptions {
            reason: "max_steps must be greater than zero",
        });
    }
    match options.solver.validate() {
        Ok(()) => Ok(()),
        Err(DcError::InvalidOptions { reason }) => Err(TransientError::InvalidOptions { reason }),
        Err(_) => Err(TransientError::InvalidOptions {
            reason: "solver options are invalid",
        }),
    }
}

struct IndexedEvent {
    original_index: usize,
    event: TransientEvent,
}

fn prepare_events(
    netlist: &Netlist,
    options: &TransientOptions,
) -> Result<Vec<IndexedEvent>, TransientError> {
    let mut events = Vec::with_capacity(options.events.len());
    for (index, event) in options.events.iter().enumerate() {
        if !event.time.is_finite() {
            return Err(TransientError::InvalidEvent {
                index,
                reason: "event time must be finite",
            });
        }
        if event.time < options.start_time || event.time >= options.stop_time {
            return Err(TransientError::InvalidEvent {
                index,
                reason: "event time must be in [start_time, stop_time)",
            });
        }
        validate_action(netlist, &event.action, index)?;
        events.push(IndexedEvent {
            original_index: index,
            event: event.clone(),
        });
    }

    events.sort_by(|left, right| {
        left.event
            .time
            .total_cmp(&right.event.time)
            .then_with(|| {
                left.event
                    .action
                    .component()
                    .cmp(&right.event.action.component())
            })
            .then_with(|| action_rank(&left.event.action).cmp(&action_rank(&right.event.action)))
    });

    for pair in events.windows(2) {
        if pair[0].event.time == pair[1].event.time
            && pair[0].event.action.component() == pair[1].event.action.component()
        {
            return Err(TransientError::InvalidEvent {
                index: pair[1].original_index,
                reason: "duplicate event for the same component at the same time",
            });
        }
    }
    Ok(events)
}

fn validate_action(
    netlist: &Netlist,
    action: &TransientAction,
    index: usize,
) -> Result<(), TransientError> {
    let component = action.component();
    let target = netlist
        .components
        .get(usize::try_from(component.0).unwrap_or(usize::MAX))
        .ok_or(TransientError::InvalidEvent {
            index,
            reason: "event target component does not exist",
        })?;
    match action {
        TransientAction::SetSourceValue { value, .. } => {
            if !value.is_finite() {
                return Err(TransientError::InvalidEvent {
                    index,
                    reason: "source value must be finite",
                });
            }
            if !matches!(
                target.kind,
                ComponentKind::VoltageSource | ComponentKind::CurrentSource
            ) {
                return Err(TransientError::InvalidEvent {
                    index,
                    reason: "event target must be a voltage or current source",
                });
            }
        }
        TransientAction::SetSwitchClosed { .. } => {
            if target.kind != ComponentKind::Switch {
                return Err(TransientError::InvalidEvent {
                    index,
                    reason: "event target must be a switch",
                });
            }
        }
    }
    Ok(())
}

fn action_rank(action: &TransientAction) -> u8 {
    match action {
        TransientAction::SetSourceValue { .. } => 0,
        TransientAction::SetSwitchClosed { .. } => 1,
    }
}

fn apply_action(
    netlist: &mut Netlist,
    action: &TransientAction,
    original_index: usize,
) -> Result<(), TransientError> {
    let component = netlist
        .components
        .get_mut(usize::try_from(action.component().0).unwrap_or(usize::MAX))
        .ok_or(TransientError::InvalidEvent {
            index: original_index,
            reason: "event target component does not exist",
        })?;
    match action {
        TransientAction::SetSourceValue { value, .. } => {
            if !matches!(
                component.kind,
                ComponentKind::VoltageSource | ComponentKind::CurrentSource
            ) {
                return Err(TransientError::InvalidEvent {
                    index: original_index,
                    reason: "event target must be a voltage or current source",
                });
            }
            component
                .params
                .insert("dc".to_owned(), ParamValue::Number(*value));
        }
        TransientAction::SetSwitchClosed { closed, .. } => {
            if component.kind != ComponentKind::Switch {
                return Err(TransientError::InvalidEvent {
                    index: original_index,
                    reason: "event target must be a switch",
                });
            }
            component
                .params
                .insert("closed".to_owned(), ParamValue::Bool(*closed));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct TimelinePoint {
    time: f64,
    event: bool,
    boundary: bool,
}

fn build_timeline(
    options: &TransientOptions,
    events: &[IndexedEvent],
) -> Result<Vec<f64>, TransientError> {
    let mut points = vec![TimelinePoint {
        time: options.start_time,
        event: false,
        boundary: true,
    }];
    let mut previous = options.start_time;
    let mut nominal_steps = 0;
    while previous < options.stop_time {
        nominal_steps += 1;
        if nominal_steps > options.max_steps {
            return Err(TransientError::TooManySteps {
                limit: options.max_steps,
            });
        }
        let candidate = options.start_time + nominal_steps as f64 * options.step;
        let next = if candidate.is_finite() && candidate < options.stop_time {
            candidate
        } else {
            options.stop_time
        };
        if next <= previous {
            return Err(TransientError::InvalidOptions {
                reason: "step is too small for the selected time range",
            });
        }
        points.push(TimelinePoint {
            time: next,
            event: false,
            boundary: next == options.stop_time,
        });
        previous = next;
    }

    for event in events {
        points.push(TimelinePoint {
            time: event.event.time,
            event: true,
            boundary: false,
        });
    }
    points.sort_by(|left, right| left.time.total_cmp(&right.time));

    let mut deduplicated: Vec<TimelinePoint> = Vec::with_capacity(points.len());
    for point in points {
        let mut replace_last = false;
        if let Some(last) = deduplicated.last() {
            let scale = last.time.abs().max(point.time.abs());
            let tolerance = (options.step * 1e-9)
                .max(4.0 * f64::EPSILON * scale)
                .min(options.step * 0.5);
            if (point.time - last.time).abs() <= tolerance {
                if point.event {
                    if last.event || last.boundary {
                        if point.time == last.time {
                            continue;
                        }
                    } else {
                        replace_last = true;
                    }
                } else if !point.boundary && !last.boundary {
                    continue;
                }
            }
        }
        if replace_last {
            deduplicated.pop();
        }
        deduplicated.push(point);
    }

    if deduplicated.len().saturating_sub(1) > options.max_steps {
        return Err(TransientError::TooManySteps {
            limit: options.max_steps,
        });
    }
    Ok(deduplicated.into_iter().map(|point| point.time).collect())
}

fn solve_initial(
    circuit: &MnaCircuit,
    options: &DcOptions,
) -> Result<crate::newton::NewtonOutcome, TransientError> {
    let config = NewtonConfig {
        max_iterations: options.max_iterations,
        abs_tolerance: options.abs_tolerance,
        rel_tolerance: options.rel_tolerance,
        residual_tolerance: options.residual_tolerance,
        strategy: options.strategy,
    };
    let size = circuit.size();
    if size == 0 {
        return Ok(crate::newton::NewtonOutcome {
            solution: DVector::zeros(0),
            iterations: 0,
            residual: 0.0,
        });
    }
    let initial = DVector::zeros(size);
    let junction_voltages = vec![0.0; circuit.nonlinear_count()];
    solve_robust(
        circuit,
        config,
        AssemblyMode::Initial,
        &initial,
        &junction_voltages,
    )
    .map_err(|source| TransientError::InitialDc {
        source: map_newton_error(source),
    })
}

fn solve_step(
    circuit: &MnaCircuit,
    options: &DcOptions,
    mode: AssemblyMode<'_>,
    initial: &DVector<f64>,
    junction_voltages: &[f64],
    time: f64,
) -> Result<crate::newton::NewtonOutcome, TransientError> {
    if circuit.size() == 0 {
        return Ok(crate::newton::NewtonOutcome {
            solution: DVector::zeros(0),
            iterations: 0,
            residual: 0.0,
        });
    }
    let config = NewtonConfig {
        max_iterations: options.max_iterations,
        abs_tolerance: options.abs_tolerance,
        rel_tolerance: options.rel_tolerance,
        residual_tolerance: options.residual_tolerance,
        strategy: options.strategy,
    };
    solve_robust(circuit, config, mode, initial, junction_voltages).map_err(|source| {
        TransientError::Step {
            time,
            source: map_newton_error(source),
        }
    })
}

fn build_sample(
    netlist: &Netlist,
    circuit: &MnaCircuit,
    solution: &DVector<f64>,
    time: f64,
) -> TransientSample {
    let mut node_voltages = BTreeMap::new();
    for node in &netlist.nodes {
        node_voltages.insert(
            node.id,
            circuit.node_voltage(solution, node.id).unwrap_or(0.0),
        );
    }
    let mut branch_currents = BTreeMap::new();
    for (index, component) in circuit.branch_components().iter().enumerate() {
        branch_currents.insert(*component, solution[circuit.node_count() + index]);
    }
    TransientSample {
        time,
        node_voltages,
        branch_currents,
    }
}

fn stored_energies(
    netlist: &Netlist,
    circuit: &MnaCircuit,
    solution: &DVector<f64>,
) -> BTreeMap<ComponentId, f64> {
    let mut energies = BTreeMap::new();
    for component in &netlist.components {
        let energy = match component.kind {
            ComponentKind::Capacitor => {
                let voltage = circuit
                    .component_voltage(component.id, solution)
                    .unwrap_or(0.0);
                let capacitance = component
                    .params
                    .get("capacitance")
                    .and_then(ParamValue::as_number)
                    .unwrap_or(0.0);
                0.5 * capacitance * voltage * voltage
            }
            ComponentKind::Inductor => {
                let current = circuit
                    .component_current(component.id, solution, AssemblyMode::Dc)
                    .unwrap_or(0.0);
                let inductance = component
                    .params
                    .get("inductance")
                    .and_then(ParamValue::as_number)
                    .unwrap_or(0.0);
                0.5 * inductance * current * current
            }
            _ => 0.0,
        };
        energies.insert(component.id, energy);
    }
    energies
}

fn accumulate_interval(
    netlist: &Netlist,
    circuit: &MnaCircuit,
    solution: &DVector<f64>,
    step: f64,
    absorbed: &mut BTreeMap<ComponentId, f64>,
) {
    for component in &netlist.components {
        if !matches!(
            component.kind,
            ComponentKind::Resistor
                | ComponentKind::VoltageSource
                | ComponentKind::CurrentSource
                | ComponentKind::Switch
                | ComponentKind::Diode
                | ComponentKind::Led
                | ComponentKind::Zener
                | ComponentKind::ShuntReference
                | ComponentKind::Comparator
                | ComponentKind::OpAmp
        ) {
            continue;
        }
        let voltage = circuit
            .component_voltage(component.id, solution)
            .unwrap_or(0.0);
        let current = circuit
            .component_current(component.id, solution, AssemblyMode::Dc)
            .unwrap_or(0.0);
        let reference_power = if component.kind == ComponentKind::ShuntReference {
            let reference = component
                .pins
                .first()
                .and_then(|node| circuit.node_voltage(solution, *node))
                .unwrap_or(0.0);
            let anode = component
                .pins
                .get(2)
                .and_then(|node| circuit.node_voltage(solution, *node))
                .unwrap_or(0.0);
            let reference_current = component
                .params
                .get("iref")
                .and_then(ParamValue::as_number)
                .unwrap_or(0.0);
            (reference - anode) * reference_current
        } else {
            0.0
        };
        let entry = absorbed.entry(component.id).or_insert(0.0);
        *entry += (voltage * current + reference_power) * step;
    }
}

fn build_energy_audit(
    netlist: &Netlist,
    absorbed: &BTreeMap<ComponentId, f64>,
    initial_stored: &BTreeMap<ComponentId, f64>,
    final_stored: &BTreeMap<ComponentId, f64>,
) -> EnergyAudit {
    let mut supplied = 0.0;
    let mut dissipated = 0.0;
    let mut stored_delta_total = 0.0;
    let mut components = BTreeMap::new();

    for component in &netlist.components {
        let initial = initial_stored.get(&component.id).copied().unwrap_or(0.0);
        let final_value = final_stored.get(&component.id).copied().unwrap_or(0.0);
        let stored_delta = final_value - initial;
        let interval_absorbed = absorbed.get(&component.id).copied().unwrap_or(0.0);
        let component_energy = match component.kind {
            ComponentKind::Capacitor | ComponentKind::Inductor => {
                stored_delta_total += stored_delta;
                ComponentEnergy {
                    absorbed: stored_delta,
                    stored_delta,
                }
            }
            ComponentKind::Resistor
            | ComponentKind::Switch
            | ComponentKind::Diode
            | ComponentKind::Led
            | ComponentKind::Zener
            | ComponentKind::ShuntReference
            | ComponentKind::Comparator => {
                dissipated += interval_absorbed;
                ComponentEnergy {
                    absorbed: interval_absorbed,
                    stored_delta: 0.0,
                }
            }
            ComponentKind::VoltageSource | ComponentKind::CurrentSource | ComponentKind::OpAmp => {
                supplied -= interval_absorbed;
                ComponentEnergy {
                    absorbed: interval_absorbed,
                    stored_delta: 0.0,
                }
            }
            ComponentKind::Ground | ComponentKind::Custom(_) => ComponentEnergy {
                absorbed: 0.0,
                stored_delta: 0.0,
            },
        };
        components.insert(component.id, component_energy);
    }

    let residual = supplied - dissipated - stored_delta_total;
    EnergyAudit {
        supplied,
        dissipated,
        stored_delta: stored_delta_total,
        residual,
        components,
    }
}
