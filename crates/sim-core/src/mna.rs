//! Modified nodal analysis assembly used by the simulation analyses.

use std::collections::BTreeMap;

use nalgebra::{Complex, DMatrix, DVector};
use thiserror::Error;

use crate::netlist::{
    ComponentId, ComponentKind, ComponentSpec, Netlist, NetlistError, NodeId, ParamValue,
};

const BOLTZMANN: f64 = 1.380_649e-23;
const ELEMENTARY_CHARGE: f64 = 1.602_176_634e-19;
const NOMINAL_TEMPERATURE: f64 = 300.15;
const THERMAL_VOLTAGE: f64 = BOLTZMANN * NOMINAL_TEMPERATURE / ELEMENTARY_CHARGE;
const MAX_EXPONENT: f64 = 80.0;
/// Large conductance used only for the discharged-capacitor initial state.
const INITIAL_CAPACITOR_CONDUCTANCE: f64 = 1e9;

/// Error raised while compiling a validated netlist into MNA equations.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum MnaError {
    #[error(transparent)]
    Netlist(#[from] NetlistError),
    #[error("component {component:?} ({kind}) is not supported by the solver")]
    UnsupportedComponent {
        component: ComponentId,
        kind: ComponentKind,
    },
    #[error("component {component:?} is missing required parameter '{key}'")]
    MissingParameter {
        component: ComponentId,
        key: &'static str,
    },
    #[error("component {component:?} parameter '{key}' must be {expected}")]
    InvalidParameterType {
        component: ComponentId,
        key: &'static str,
        expected: &'static str,
    },
    #[error("component {component:?} parameter '{key}' must be {requirement}, got {value}")]
    InvalidParameterValue {
        component: ComponentId,
        key: &'static str,
        value: f64,
        requirement: &'static str,
    },
    #[error("component {component:?} cannot define a branch between the same node")]
    DegenerateBranch { component: ComponentId },
    #[error("component {component:?} requires v_sat_pos > v_sat_neg (got {positive}, {negative})")]
    InvalidSaturation {
        component: ComponentId,
        positive: f64,
        negative: f64,
    },
}

/// Time mode used while assembling the MNA system.
#[derive(Clone, Copy)]
pub(crate) enum AssemblyMode<'a> {
    /// DC operating point: capacitors open and inductors shorted.
    Dc,
    /// Transient initial state: capacitors discharged and inductors shorted.
    Initial,
    /// Backward-Euler step from a previous solution.
    Transient {
        previous: &'a DVector<f64>,
        step: f64,
    },
}

pub(crate) struct MnaCircuit {
    node_map: Vec<Option<usize>>,
    node_count: usize,
    branch_components: Vec<ComponentId>,
    branches: Vec<BranchElement>,
    resistors: Vec<ResistorElement>,
    capacitors: Vec<CapacitorElement>,
    current_sources: Vec<CurrentSourceElement>,
    nonlinear: Vec<NonlinearDevice>,
    grounds: Vec<ComponentId>,
}

impl MnaCircuit {
    pub(crate) fn from_netlist(netlist: &Netlist) -> Result<Self, MnaError> {
        netlist.validate_for_simulation()?;

        let mut node_map = Vec::with_capacity(netlist.nodes.len());
        let mut node_count = 0;
        for node in &netlist.nodes {
            if node.kind.is_ground() {
                node_map.push(None);
            } else {
                node_map.push(Some(node_count));
                node_count += 1;
            }
        }

        let mut circuit = Self {
            node_map,
            node_count,
            branch_components: Vec::new(),
            branches: Vec::new(),
            resistors: Vec::new(),
            capacitors: Vec::new(),
            current_sources: Vec::new(),
            nonlinear: Vec::new(),
            grounds: Vec::new(),
        };

        for component in &netlist.components {
            circuit.add_component(component)?;
        }
        Ok(circuit)
    }

    pub(crate) fn size(&self) -> usize {
        self.node_count + self.branch_components.len()
    }

    pub(crate) fn node_count(&self) -> usize {
        self.node_count
    }

    pub(crate) fn branch_components(&self) -> &[ComponentId] {
        &self.branch_components
    }

    pub(crate) fn has_nonlinear_devices(&self) -> bool {
        !self.nonlinear.is_empty()
            || self
                .branches
                .iter()
                .any(|branch| matches!(branch.kind, BranchKind::OpAmp { .. }))
    }

    pub(crate) fn nonlinear_count(&self) -> usize {
        self.nonlinear.len()
    }

    pub(crate) fn node_voltage(&self, solution: &DVector<f64>, node: NodeId) -> Option<f64> {
        self.node_map
            .get(usize::try_from(node.0).ok()?)
            .map(|index| index.map_or(0.0, |index| solution[index]))
    }

    pub(crate) fn node_phasor(
        &self,
        solution: &DVector<Complex<f64>>,
        node: NodeId,
    ) -> Option<Complex<f64>> {
        self.node_map
            .get(usize::try_from(node.0).ok()?)
            .map(|index| index.map_or_else(|| Complex::new(0.0, 0.0), |index| solution[index]))
    }

    pub(crate) fn junction_voltages(&self, solution: &DVector<f64>) -> Vec<f64> {
        self.nonlinear
            .iter()
            .map(|device| device.junction_voltage(solution))
            .collect()
    }

    pub(crate) fn limit_junction_voltages(
        &self,
        solution: &DVector<f64>,
        junction_voltages: &mut [f64],
    ) {
        for (device, state) in self.nonlinear.iter().zip(junction_voltages.iter_mut()) {
            let target = device.junction_voltage(solution);
            *state = limit_junction(*state, target, device.limiting_voltage());
        }
    }

    pub(crate) fn assemble(
        &self,
        solution: &DVector<f64>,
        junction_voltages: &[f64],
        mode: AssemblyMode<'_>,
        gmin: f64,
        source_scale: f64,
    ) -> (DMatrix<f64>, DVector<f64>) {
        let size = self.size();
        let mut conductance = DMatrix::zeros(size, size);
        let mut sources = DVector::zeros(size);

        for resistor in &self.resistors {
            stamp_conductance(
                &mut conductance,
                resistor.a,
                resistor.b,
                1.0 / resistor.resistance,
            );
        }

        for source in &self.current_sources {
            stamp_current_source(
                &mut sources,
                source.a,
                source.b,
                source.current * source_scale,
            );
        }

        match mode {
            AssemblyMode::Dc => {}
            AssemblyMode::Initial => {
                for capacitor in &self.capacitors {
                    stamp_conductance(
                        &mut conductance,
                        capacitor.a,
                        capacitor.b,
                        INITIAL_CAPACITOR_CONDUCTANCE,
                    );
                }
            }
            AssemblyMode::Transient { previous, step } => {
                for capacitor in &self.capacitors {
                    let conductance_value = capacitor.capacitance / step;
                    let previous_voltage =
                        node_voltage(previous, capacitor.a) - node_voltage(previous, capacitor.b);
                    stamp_conductance(
                        &mut conductance,
                        capacitor.a,
                        capacitor.b,
                        conductance_value,
                    );
                    stamp_current_source(
                        &mut sources,
                        capacitor.a,
                        capacitor.b,
                        -conductance_value * previous_voltage,
                    );
                }
            }
        }

        for branch in &self.branches {
            if let Some(a) = branch.a {
                conductance[(a, branch.index)] += 1.0;
            }
            if let Some(b) = branch.b {
                conductance[(b, branch.index)] -= 1.0;
            }

            match branch.kind {
                BranchKind::VoltageSource { voltage } => {
                    if let Some(a) = branch.a {
                        conductance[(branch.index, a)] += 1.0;
                    }
                    if let Some(b) = branch.b {
                        conductance[(branch.index, b)] -= 1.0;
                    }
                    sources[branch.index] += voltage * source_scale;
                }
                BranchKind::Inductor {
                    inductance,
                    resistance,
                } => {
                    if let Some(a) = branch.a {
                        conductance[(branch.index, a)] += 1.0;
                    }
                    if let Some(b) = branch.b {
                        conductance[(branch.index, b)] -= 1.0;
                    }
                    conductance[(branch.index, branch.index)] -= resistance;
                    if let AssemblyMode::Transient { previous, step } = mode {
                        let ratio = inductance / step;
                        conductance[(branch.index, branch.index)] -= ratio;
                        sources[branch.index] -= ratio * previous[branch.index];
                    }
                }
                BranchKind::OpAmp {
                    in_plus,
                    in_minus,
                    gain,
                    saturation_positive,
                    saturation_negative,
                } => {
                    if let Some(a) = branch.a {
                        conductance[(branch.index, a)] += 1.0;
                    }
                    let differential =
                        node_voltage(solution, in_plus) - node_voltage(solution, in_minus);
                    let (value, slope) = opamp_operating_point(
                        differential,
                        gain,
                        saturation_positive,
                        saturation_negative,
                    );
                    if let Some(in_plus) = in_plus {
                        conductance[(branch.index, in_plus)] -= slope;
                    }
                    if let Some(in_minus) = in_minus {
                        conductance[(branch.index, in_minus)] += slope;
                    }
                    sources[branch.index] += value - slope * differential;
                }
            }
        }

        for (device, voltage) in self.nonlinear.iter().zip(junction_voltages) {
            device.stamp_at(solution, *voltage, &mut conductance, &mut sources);
        }

        if gmin != 0.0 {
            for node in 0..self.node_count {
                conductance[(node, node)] += gmin;
            }
        }

        (conductance, sources)
    }

    pub(crate) fn assemble_ac(
        &self,
        dc_solution: &DVector<f64>,
        frequency: f64,
        source_phasors: &BTreeMap<ComponentId, Complex<f64>>,
    ) -> (DMatrix<Complex<f64>>, DVector<Complex<f64>>) {
        let size = self.size();
        let mut conductance = DMatrix::zeros(size, size);
        let mut sources = DVector::zeros(size);
        let omega = std::f64::consts::TAU * frequency;

        for resistor in &self.resistors {
            stamp_complex_conductance(
                &mut conductance,
                resistor.a,
                resistor.b,
                Complex::new(1.0 / resistor.resistance, 0.0),
            );
        }

        for capacitor in &self.capacitors {
            stamp_complex_conductance(
                &mut conductance,
                capacitor.a,
                capacitor.b,
                Complex::new(0.0, omega * capacitor.capacitance),
            );
        }

        for source in &self.current_sources {
            let current = source_phasors
                .get(&source.component)
                .copied()
                .unwrap_or_else(|| Complex::new(0.0, 0.0));
            stamp_complex_current_source(&mut sources, source.a, source.b, current);
        }

        for branch in &self.branches {
            if let Some(a) = branch.a {
                conductance[(a, branch.index)] += Complex::new(1.0, 0.0);
            }
            if let Some(b) = branch.b {
                conductance[(b, branch.index)] -= Complex::new(1.0, 0.0);
            }

            match branch.kind {
                BranchKind::VoltageSource { .. } => {
                    if let Some(a) = branch.a {
                        conductance[(branch.index, a)] += Complex::new(1.0, 0.0);
                    }
                    if let Some(b) = branch.b {
                        conductance[(branch.index, b)] -= Complex::new(1.0, 0.0);
                    }
                    sources[branch.index] += source_phasors
                        .get(&branch.component)
                        .copied()
                        .unwrap_or_else(|| Complex::new(0.0, 0.0));
                }
                BranchKind::Inductor {
                    inductance,
                    resistance,
                } => {
                    if let Some(a) = branch.a {
                        conductance[(branch.index, a)] += Complex::new(1.0, 0.0);
                    }
                    if let Some(b) = branch.b {
                        conductance[(branch.index, b)] -= Complex::new(1.0, 0.0);
                    }
                    conductance[(branch.index, branch.index)] -=
                        Complex::new(resistance, omega * inductance);
                }
                BranchKind::OpAmp {
                    in_plus,
                    in_minus,
                    gain,
                    saturation_positive,
                    saturation_negative,
                } => {
                    if let Some(a) = branch.a {
                        conductance[(branch.index, a)] += Complex::new(1.0, 0.0);
                    }
                    let differential =
                        node_voltage(dc_solution, in_plus) - node_voltage(dc_solution, in_minus);
                    let slope = opamp_operating_point(
                        differential,
                        gain,
                        saturation_positive,
                        saturation_negative,
                    )
                    .1;
                    if let Some(in_plus) = in_plus {
                        conductance[(branch.index, in_plus)] -= Complex::new(slope, 0.0);
                    }
                    if let Some(in_minus) = in_minus {
                        conductance[(branch.index, in_minus)] += Complex::new(slope, 0.0);
                    }
                }
            }
        }

        for device in &self.nonlinear {
            device.stamp_ac(dc_solution, &mut conductance);
        }

        (conductance, sources)
    }

    pub(crate) fn residual(
        &self,
        solution: &DVector<f64>,
        mode: AssemblyMode<'_>,
        gmin: f64,
        source_scale: f64,
    ) -> f64 {
        let junction_voltages = self.junction_voltages(solution);
        let (conductance, sources) =
            self.assemble(solution, &junction_voltages, mode, gmin, source_scale);
        max_abs(&(conductance * solution - sources))
    }

    /// Returns the voltage used for a component's branch convention.
    pub(crate) fn component_voltage(
        &self,
        component: ComponentId,
        solution: &DVector<f64>,
    ) -> Option<f64> {
        if self.grounds.contains(&component) {
            return Some(0.0);
        }
        if let Some(resistor) = self
            .resistors
            .iter()
            .find(|element| element.component == component)
        {
            return Some(node_voltage(solution, resistor.a) - node_voltage(solution, resistor.b));
        }
        if let Some(capacitor) = self
            .capacitors
            .iter()
            .find(|element| element.component == component)
        {
            return Some(node_voltage(solution, capacitor.a) - node_voltage(solution, capacitor.b));
        }
        if let Some(source) = self
            .current_sources
            .iter()
            .find(|element| element.component == component)
        {
            return Some(node_voltage(solution, source.a) - node_voltage(solution, source.b));
        }
        if let Some(device) = self
            .nonlinear
            .iter()
            .find(|element| element.component() == component)
        {
            return Some(device.primary_voltage(solution));
        }
        if let Some(branch) = self
            .branches
            .iter()
            .find(|element| element.component == component)
        {
            return Some(node_voltage(solution, branch.a) - node_voltage(solution, branch.b));
        }
        None
    }

    /// Returns the current using the component's `a -> b` convention.
    pub(crate) fn component_current(
        &self,
        component: ComponentId,
        solution: &DVector<f64>,
        mode: AssemblyMode<'_>,
    ) -> Option<f64> {
        if self.grounds.contains(&component) {
            return Some(0.0);
        }
        if let Some(resistor) = self
            .resistors
            .iter()
            .find(|element| element.component == component)
        {
            let voltage = node_voltage(solution, resistor.a) - node_voltage(solution, resistor.b);
            return Some(voltage / resistor.resistance);
        }
        if let Some(capacitor) = self
            .capacitors
            .iter()
            .find(|element| element.component == component)
        {
            return match mode {
                AssemblyMode::Dc | AssemblyMode::Initial => Some(0.0),
                AssemblyMode::Transient { previous, step } => {
                    let voltage =
                        node_voltage(solution, capacitor.a) - node_voltage(solution, capacitor.b);
                    let previous_voltage =
                        node_voltage(previous, capacitor.a) - node_voltage(previous, capacitor.b);
                    Some(capacitor.capacitance / step * (voltage - previous_voltage))
                }
            };
        }
        if let Some(source) = self
            .current_sources
            .iter()
            .find(|element| element.component == component)
        {
            return Some(source.current);
        }
        if let Some(device) = self
            .nonlinear
            .iter()
            .find(|element| element.component() == component)
        {
            return Some(device.current_at(solution));
        }
        if let Some(branch) = self
            .branches
            .iter()
            .find(|element| element.component == component)
        {
            return Some(solution[branch.index]);
        }
        None
    }

    fn add_component(&mut self, component: &ComponentSpec) -> Result<(), MnaError> {
        let a = self.pin_unknown(component, 0);
        let b = self.pin_unknown(component, 1);

        match component.kind {
            ComponentKind::Resistor => {
                let resistance = positive_parameter(component, "resistance")?;
                self.resistors.push(ResistorElement {
                    component: component.id,
                    a,
                    b,
                    resistance,
                });
            }
            ComponentKind::Capacitor => {
                let capacitance = positive_parameter(component, "capacitance")?;
                self.capacitors.push(CapacitorElement {
                    component: component.id,
                    a,
                    b,
                    capacitance,
                });
            }
            ComponentKind::Inductor => {
                let inductance = positive_parameter(component, "inductance")?;
                let resistance = if component.params.contains_key("resistance") {
                    non_negative_parameter(component, "resistance")?
                } else {
                    0.0
                };
                self.add_branch(
                    component,
                    a,
                    b,
                    BranchKind::Inductor {
                        inductance,
                        resistance,
                    },
                )?;
            }
            ComponentKind::VoltageSource => {
                let voltage = number_parameter(component, "dc")?;
                self.add_branch(component, a, b, BranchKind::VoltageSource { voltage })?;
            }
            ComponentKind::CurrentSource => {
                let current = number_parameter(component, "dc")?;
                self.current_sources.push(CurrentSourceElement {
                    component: component.id,
                    a,
                    b,
                    current,
                });
            }
            ComponentKind::Ground => self.grounds.push(component.id),
            ComponentKind::Switch => {
                let resistance_on = positive_parameter(component, "ron")?;
                let resistance_off = positive_parameter(component, "roff")?;
                let closed = bool_parameter(component, "closed")?;
                let resistance = if closed {
                    resistance_on
                } else {
                    resistance_off
                };
                self.resistors.push(ResistorElement {
                    component: component.id,
                    a,
                    b,
                    resistance,
                });
            }
            ComponentKind::Diode => {
                let (saturation_current, emission_coefficient) = diode_model(component)?;
                self.nonlinear.push(NonlinearDevice::Diode(DiodeDevice {
                    component: component.id,
                    a,
                    b,
                    saturation_current,
                    emission_coefficient,
                    breakdown: None,
                }));
            }
            ComponentKind::Led => {
                let (saturation_current, emission_coefficient) = led_model(component)?;
                self.nonlinear.push(NonlinearDevice::Diode(DiodeDevice {
                    component: component.id,
                    a,
                    b,
                    saturation_current,
                    emission_coefficient,
                    breakdown: None,
                }));
            }
            ComponentKind::Zener => {
                let breakdown_voltage = positive_parameter(component, "bv")?;
                let breakdown_saturation = positive_parameter(component, "ibv")?;
                let breakdown_emission = positive_parameter(component, "nbv")?;
                self.nonlinear.push(NonlinearDevice::Diode(DiodeDevice {
                    component: component.id,
                    a,
                    b,
                    saturation_current: positive_parameter(component, "is")?,
                    emission_coefficient: positive_parameter(component, "n")?,
                    breakdown: Some(Breakdown {
                        voltage: breakdown_voltage,
                        saturation_current: breakdown_saturation,
                        emission_coefficient: breakdown_emission,
                    }),
                }));
            }
            ComponentKind::ShuntReference => {
                let reference_voltage = positive_parameter(component, "vref")?;
                let reference_current = non_negative_parameter(component, "iref")?;
                let minimum_current = positive_parameter(component, "ik_min")?;
                let maximum_current = positive_parameter(component, "imax")?;
                if maximum_current < minimum_current {
                    return Err(MnaError::InvalidParameterValue {
                        component: component.id,
                        key: "imax",
                        value: maximum_current,
                        requirement: "greater than or equal to ik_min",
                    });
                }
                let dynamic_impedance = positive_parameter(component, "z_ka")?;
                self.nonlinear
                    .push(NonlinearDevice::ShuntReference(ShuntReferenceDevice {
                        component: component.id,
                        reference: self.pin_unknown(component, 0),
                        cathode: self.pin_unknown(component, 1),
                        anode: self.pin_unknown(component, 2),
                        reference_voltage,
                        reference_current,
                        minimum_current,
                        maximum_current,
                        dynamic_impedance,
                    }));
            }
            ComponentKind::Comparator => {
                let input_offset = number_parameter(component, "v_offset")?;
                if input_offset.abs() > 5e-3 {
                    return Err(MnaError::InvalidParameterValue {
                        component: component.id,
                        key: "v_offset",
                        value: input_offset,
                        requirement: "within -5e-3 to 5e-3 volts",
                    });
                }
                let sink_resistance = positive_parameter(component, "r_sink")?;
                let maximum_current = positive_parameter(component, "imax")?;
                if maximum_current > 20e-3 {
                    return Err(MnaError::InvalidParameterValue {
                        component: component.id,
                        key: "imax",
                        value: maximum_current,
                        requirement: "no greater than 20e-3 amperes",
                    });
                }
                self.nonlinear
                    .push(NonlinearDevice::Comparator(ComparatorDevice {
                        component: component.id,
                        input_plus: self.pin_unknown(component, 0),
                        input_minus: self.pin_unknown(component, 1),
                        output: self.pin_unknown(component, 2),
                        positive_supply: self.pin_unknown(component, 3),
                        negative_supply: self.pin_unknown(component, 4),
                        input_offset,
                        sink_resistance,
                        maximum_current,
                    }));
            }
            ComponentKind::OpAmp => {
                let output = self.pin_unknown(component, 2);
                let in_plus = self.pin_unknown(component, 0);
                let in_minus = self.pin_unknown(component, 1);
                let gain = positive_parameter(component, "gain")?;
                let saturation_positive = number_parameter(component, "v_sat_pos")?;
                let saturation_negative = number_parameter(component, "v_sat_neg")?;
                if saturation_positive <= saturation_negative {
                    return Err(MnaError::InvalidSaturation {
                        component: component.id,
                        positive: saturation_positive,
                        negative: saturation_negative,
                    });
                }
                self.add_branch(
                    component,
                    output,
                    None,
                    BranchKind::OpAmp {
                        in_plus,
                        in_minus,
                        gain,
                        saturation_positive,
                        saturation_negative,
                    },
                )?;
            }
            ComponentKind::Custom(_) => {
                return Err(MnaError::UnsupportedComponent {
                    component: component.id,
                    kind: component.kind.clone(),
                });
            }
        }
        Ok(())
    }

    fn add_branch(
        &mut self,
        component: &ComponentSpec,
        a: Option<usize>,
        b: Option<usize>,
        kind: BranchKind,
    ) -> Result<(), MnaError> {
        if a == b {
            return Err(MnaError::DegenerateBranch {
                component: component.id,
            });
        }
        let index = self.node_count + self.branch_components.len();
        self.branch_components.push(component.id);
        self.branches.push(BranchElement {
            component: component.id,
            a,
            b,
            index,
            kind,
        });
        Ok(())
    }

    fn pin_unknown(&self, component: &ComponentSpec, pin: usize) -> Option<usize> {
        component
            .pins
            .get(pin)
            .and_then(|node| self.node_map.get(usize::try_from(node.0).ok()?))
            .copied()
            .flatten()
    }
}

fn opamp_operating_point(
    differential: f64,
    gain: f64,
    saturation_positive: f64,
    saturation_negative: f64,
) -> (f64, f64) {
    let raw = gain * differential;
    if raw <= saturation_negative {
        (saturation_negative, 0.0)
    } else if raw >= saturation_positive {
        (saturation_positive, 0.0)
    } else {
        (raw, gain)
    }
}

struct BranchElement {
    component: ComponentId,
    a: Option<usize>,
    b: Option<usize>,
    index: usize,
    kind: BranchKind,
}

enum BranchKind {
    VoltageSource {
        voltage: f64,
    },
    Inductor {
        inductance: f64,
        resistance: f64,
    },
    OpAmp {
        in_plus: Option<usize>,
        in_minus: Option<usize>,
        gain: f64,
        saturation_positive: f64,
        saturation_negative: f64,
    },
}

struct ResistorElement {
    component: ComponentId,
    a: Option<usize>,
    b: Option<usize>,
    resistance: f64,
}

struct CapacitorElement {
    component: ComponentId,
    a: Option<usize>,
    b: Option<usize>,
    capacitance: f64,
}

struct CurrentSourceElement {
    component: ComponentId,
    a: Option<usize>,
    b: Option<usize>,
    current: f64,
}

enum NonlinearDevice {
    Diode(DiodeDevice),
    ShuntReference(ShuntReferenceDevice),
    Comparator(ComparatorDevice),
}

impl NonlinearDevice {
    fn component(&self) -> ComponentId {
        match self {
            Self::Diode(device) => device.component,
            Self::ShuntReference(device) => device.component,
            Self::Comparator(device) => device.component,
        }
    }

    fn junction_voltage(&self, solution: &DVector<f64>) -> f64 {
        match self {
            Self::Diode(device) => device.junction_voltage(solution),
            Self::ShuntReference(device) => device.reference_error_at(solution),
            Self::Comparator(device) => device.input_error_at(solution),
        }
    }

    fn limiting_voltage(&self) -> f64 {
        match self {
            Self::Diode(device) => device.limiting_voltage(),
            Self::ShuntReference(device) => device.dynamic_impedance * device.maximum_current,
            Self::Comparator(_) => 1.0,
        }
    }

    fn stamp_at(
        &self,
        solution: &DVector<f64>,
        voltage: f64,
        conductance: &mut DMatrix<f64>,
        sources: &mut DVector<f64>,
    ) {
        match self {
            Self::Diode(device) => {
                let (current, slope) = device.current_and_conductance(voltage);
                let equivalent = current - slope * voltage;
                stamp_conductance(conductance, device.a, device.b, slope);
                stamp_current_source(sources, device.a, device.b, equivalent);
            }
            Self::ShuntReference(device) => {
                let cathode_voltage = device.cathode_voltage_at(solution);
                let (current, reference_slope, cathode_slope) =
                    device.current_and_slopes(voltage, cathode_voltage);
                let equivalent = current
                    - reference_slope * (voltage + device.reference_voltage)
                    - cathode_slope * cathode_voltage;
                stamp_three_terminal_jacobian(
                    conductance,
                    device.cathode,
                    device.anode,
                    device.reference,
                    reference_slope,
                    cathode_slope,
                );
                stamp_current_source(sources, device.cathode, device.anode, equivalent);
                stamp_current_source(
                    sources,
                    device.reference,
                    device.anode,
                    device.reference_current,
                );
            }
            Self::Comparator(device) => {
                let output_voltage = device.output_voltage_at(solution);
                let (current, slope) = device.current_and_conductance(voltage, output_voltage);
                stamp_conductance(conductance, device.output, device.negative_supply, slope);
                stamp_current_source(
                    sources,
                    device.output,
                    device.negative_supply,
                    current - slope * output_voltage,
                );
            }
        }
    }

    fn stamp_ac(&self, solution: &DVector<f64>, conductance: &mut DMatrix<Complex<f64>>) {
        match self {
            Self::Diode(device) => {
                let voltage = device.junction_voltage(solution);
                stamp_complex_conductance(
                    conductance,
                    device.a,
                    device.b,
                    Complex::new(device.current_and_conductance(voltage).1, 0.0),
                );
            }
            Self::ShuntReference(device) => {
                let reference_voltage = device.reference_error_at(solution);
                let cathode_voltage = device.cathode_voltage_at(solution);
                let (_, reference_slope, cathode_slope) =
                    device.current_and_slopes(reference_voltage, cathode_voltage);
                stamp_three_terminal_complex_jacobian(
                    conductance,
                    device.cathode,
                    device.anode,
                    device.reference,
                    reference_slope,
                    cathode_slope,
                );
            }
            Self::Comparator(device) => {
                let input_error = device.input_error_at(solution);
                let output_voltage = device.output_voltage_at(solution);
                let slope = device
                    .current_and_conductance(input_error, output_voltage)
                    .1;
                stamp_complex_conductance(
                    conductance,
                    device.output,
                    device.negative_supply,
                    Complex::new(slope, 0.0),
                );
            }
        }
    }

    fn primary_voltage(&self, solution: &DVector<f64>) -> f64 {
        match self {
            Self::Diode(device) => device.junction_voltage(solution),
            Self::ShuntReference(device) => device.cathode_voltage_at(solution),
            Self::Comparator(device) => device.output_voltage_at(solution),
        }
    }

    fn current_at(&self, solution: &DVector<f64>) -> f64 {
        match self {
            Self::Diode(device) => device.current_at(device.junction_voltage(solution)),
            Self::ShuntReference(device) => {
                let reference_voltage = device.reference_error_at(solution);
                let cathode_voltage = device.cathode_voltage_at(solution);
                device
                    .current_and_slopes(reference_voltage, cathode_voltage)
                    .0
            }
            Self::Comparator(device) => {
                device
                    .current_and_conductance(
                        device.input_error_at(solution),
                        device.output_voltage_at(solution),
                    )
                    .0
            }
        }
    }
}

struct DiodeDevice {
    component: ComponentId,
    a: Option<usize>,
    b: Option<usize>,
    saturation_current: f64,
    emission_coefficient: f64,
    breakdown: Option<Breakdown>,
}

impl DiodeDevice {
    fn junction_voltage(&self, solution: &DVector<f64>) -> f64 {
        node_voltage(solution, self.a) - node_voltage(solution, self.b)
    }

    fn limiting_voltage(&self) -> f64 {
        let emission = self
            .breakdown
            .as_ref()
            .map_or(self.emission_coefficient, |breakdown| {
                self.emission_coefficient
                    .max(breakdown.emission_coefficient)
            });
        emission * THERMAL_VOLTAGE
    }

    fn current_at(&self, voltage: f64) -> f64 {
        self.current_and_conductance(voltage).0
    }

    fn current_and_conductance(&self, voltage: f64) -> (f64, f64) {
        let thermal = self.emission_coefficient * THERMAL_VOLTAGE;
        let exponent = (voltage / thermal).min(MAX_EXPONENT);
        let exponential = exponent.exp();
        let mut current = self.saturation_current * (exponential - 1.0);
        let mut slope = self.saturation_current / thermal * exponential;

        if let Some(breakdown) = self
            .breakdown
            .as_ref()
            .filter(|breakdown| voltage < -breakdown.voltage)
        {
            let reverse_thermal = breakdown.emission_coefficient * THERMAL_VOLTAGE;
            let reverse_exponent =
                (-(voltage + breakdown.voltage) / reverse_thermal).min(MAX_EXPONENT);
            let reverse_exponential = reverse_exponent.exp();
            current -= breakdown.saturation_current * (reverse_exponential - 1.0);
            slope += breakdown.saturation_current / reverse_thermal * reverse_exponential;
        }

        (current, slope)
    }
}

struct ShuntReferenceDevice {
    component: ComponentId,
    reference: Option<usize>,
    cathode: Option<usize>,
    anode: Option<usize>,
    reference_voltage: f64,
    reference_current: f64,
    minimum_current: f64,
    maximum_current: f64,
    dynamic_impedance: f64,
}

impl ShuntReferenceDevice {
    fn reference_error_at(&self, solution: &DVector<f64>) -> f64 {
        node_voltage(solution, self.reference)
            - node_voltage(solution, self.anode)
            - self.reference_voltage
    }

    fn cathode_voltage_at(&self, solution: &DVector<f64>) -> f64 {
        node_voltage(solution, self.cathode) - node_voltage(solution, self.anode)
    }

    fn current_and_slopes(&self, reference_voltage: f64, cathode_voltage: f64) -> (f64, f64, f64) {
        let commanded = self.minimum_current + reference_voltage / self.dynamic_impedance;
        let (commanded, command_slope) = if commanded <= 0.0 {
            (0.0, 0.0)
        } else if commanded >= self.maximum_current {
            (self.maximum_current, 0.0)
        } else {
            (commanded, 1.0 / self.dynamic_impedance)
        };
        let normalized_headroom = (cathode_voltage / self.reference_voltage).clamp(0.0, 1.0);
        let headroom =
            normalized_headroom * normalized_headroom * (3.0 - 2.0 * normalized_headroom);
        let headroom_slope = if normalized_headroom > 0.0 && normalized_headroom < 1.0 {
            6.0 * normalized_headroom * (1.0 - normalized_headroom) / self.reference_voltage
        } else {
            0.0
        };
        (
            commanded * headroom,
            command_slope * headroom,
            commanded * headroom_slope,
        )
    }
}

struct ComparatorDevice {
    component: ComponentId,
    input_plus: Option<usize>,
    input_minus: Option<usize>,
    output: Option<usize>,
    positive_supply: Option<usize>,
    negative_supply: Option<usize>,
    input_offset: f64,
    sink_resistance: f64,
    maximum_current: f64,
}

impl ComparatorDevice {
    fn input_error_at(&self, solution: &DVector<f64>) -> f64 {
        let supply_voltage = node_voltage(solution, self.positive_supply)
            - node_voltage(solution, self.negative_supply);
        if !(2.0..=36.0).contains(&supply_voltage) {
            1.0
        } else {
            node_voltage(solution, self.input_plus)
                - node_voltage(solution, self.input_minus)
                - self.input_offset
        }
    }

    fn output_voltage_at(&self, solution: &DVector<f64>) -> f64 {
        node_voltage(solution, self.output) - node_voltage(solution, self.negative_supply)
    }

    fn current_and_conductance(&self, input_error: f64, output_voltage: f64) -> (f64, f64) {
        if input_error >= 0.0 {
            return (0.0, 0.0);
        }
        let raw_current = output_voltage / self.sink_resistance;
        if raw_current >= self.maximum_current {
            (self.maximum_current, 0.0)
        } else {
            (raw_current, 1.0 / self.sink_resistance)
        }
    }
}

fn stamp_three_terminal_jacobian(
    matrix: &mut DMatrix<f64>,
    cathode: Option<usize>,
    anode: Option<usize>,
    reference: Option<usize>,
    reference_slope: f64,
    cathode_slope: f64,
) {
    let inputs = [
        (reference, reference_slope),
        (cathode, cathode_slope),
        (anode, -reference_slope - cathode_slope),
    ];
    for (row, sign) in [(cathode, 1.0), (anode, -1.0)] {
        if let Some(row) = row {
            for (column, slope) in inputs {
                if let Some(column) = column {
                    matrix[(row, column)] += sign * slope;
                }
            }
        }
    }
}

fn stamp_three_terminal_complex_jacobian(
    matrix: &mut DMatrix<Complex<f64>>,
    cathode: Option<usize>,
    anode: Option<usize>,
    reference: Option<usize>,
    reference_slope: f64,
    cathode_slope: f64,
) {
    let inputs = [
        (reference, reference_slope),
        (cathode, cathode_slope),
        (anode, -reference_slope - cathode_slope),
    ];
    for (row, sign) in [(cathode, 1.0), (anode, -1.0)] {
        if let Some(row) = row {
            for (column, slope) in inputs {
                if let Some(column) = column {
                    matrix[(row, column)] += Complex::new(sign * slope, 0.0);
                }
            }
        }
    }
}

struct Breakdown {
    voltage: f64,
    saturation_current: f64,
    emission_coefficient: f64,
}

fn node_voltage(solution: &DVector<f64>, node: Option<usize>) -> f64 {
    node.map_or(0.0, |index| solution[index])
}

fn limit_junction(previous: f64, target: f64, thermal_voltage: f64) -> f64 {
    let difference = target - previous;
    if difference > 0.0 {
        previous + thermal_voltage * (1.0 + difference / thermal_voltage).ln()
    } else if difference < 0.0 {
        previous - thermal_voltage * (1.0 - difference / thermal_voltage).ln()
    } else {
        previous
    }
}

fn stamp_conductance(
    conductance: &mut DMatrix<f64>,
    a: Option<usize>,
    b: Option<usize>,
    value: f64,
) {
    if let Some(a) = a {
        conductance[(a, a)] += value;
        if let Some(b) = b {
            conductance[(a, b)] -= value;
        }
    }
    if let Some(b) = b {
        conductance[(b, b)] += value;
        if let Some(a) = a {
            conductance[(b, a)] -= value;
        }
    }
}

fn stamp_current_source(
    sources: &mut DVector<f64>,
    a: Option<usize>,
    b: Option<usize>,
    current: f64,
) {
    if let Some(a) = a {
        sources[a] -= current;
    }
    if let Some(b) = b {
        sources[b] += current;
    }
}

fn stamp_complex_conductance(
    conductance: &mut DMatrix<Complex<f64>>,
    a: Option<usize>,
    b: Option<usize>,
    value: Complex<f64>,
) {
    if let Some(a) = a {
        conductance[(a, a)] += value;
        if let Some(b) = b {
            conductance[(a, b)] -= value;
        }
    }
    if let Some(b) = b {
        conductance[(b, b)] += value;
        if let Some(a) = a {
            conductance[(b, a)] -= value;
        }
    }
}

fn stamp_complex_current_source(
    sources: &mut DVector<Complex<f64>>,
    a: Option<usize>,
    b: Option<usize>,
    current: Complex<f64>,
) {
    if let Some(a) = a {
        sources[a] -= current;
    }
    if let Some(b) = b {
        sources[b] += current;
    }
}

pub(crate) fn max_abs(vector: &DVector<f64>) -> f64 {
    vector.iter().fold(0.0_f64, |maximum, value| {
        if maximum.is_nan() || value.is_nan() {
            f64::NAN
        } else {
            maximum.max(value.abs())
        }
    })
}

fn number_parameter(component: &ComponentSpec, key: &'static str) -> Result<f64, MnaError> {
    match component.params.get(key) {
        Some(ParamValue::Number(value)) if value.is_finite() => Ok(*value),
        Some(ParamValue::Number(value)) => Err(MnaError::InvalidParameterValue {
            component: component.id,
            key,
            value: *value,
            requirement: "finite",
        }),
        Some(ParamValue::Bool(_) | ParamValue::Text(_)) => Err(MnaError::InvalidParameterType {
            component: component.id,
            key,
            expected: "a number",
        }),
        None => Err(MnaError::MissingParameter {
            component: component.id,
            key,
        }),
    }
}

fn positive_parameter(component: &ComponentSpec, key: &'static str) -> Result<f64, MnaError> {
    let value = number_parameter(component, key)?;
    if value <= 0.0 {
        return Err(MnaError::InvalidParameterValue {
            component: component.id,
            key,
            value,
            requirement: "strictly positive",
        });
    }
    Ok(value)
}

fn non_negative_parameter(component: &ComponentSpec, key: &'static str) -> Result<f64, MnaError> {
    let value = number_parameter(component, key)?;
    if value < 0.0 {
        return Err(MnaError::InvalidParameterValue {
            component: component.id,
            key,
            value,
            requirement: "non-negative",
        });
    }
    Ok(value)
}

fn optional_number(component: &ComponentSpec, key: &'static str) -> Result<Option<f64>, MnaError> {
    match component.params.get(key) {
        Some(ParamValue::Number(value)) if value.is_finite() => Ok(Some(*value)),
        Some(ParamValue::Number(value)) => Err(MnaError::InvalidParameterValue {
            component: component.id,
            key,
            value: *value,
            requirement: "finite",
        }),
        Some(ParamValue::Bool(_) | ParamValue::Text(_)) => Err(MnaError::InvalidParameterType {
            component: component.id,
            key,
            expected: "a number",
        }),
        None => Ok(None),
    }
}

fn diode_model(component: &ComponentSpec) -> Result<(f64, f64), MnaError> {
    let emission = optional_number(component, "n")?.unwrap_or(1.0);
    if emission <= 0.0 {
        return Err(MnaError::InvalidParameterValue {
            component: component.id,
            key: "n",
            value: emission,
            requirement: "strictly positive",
        });
    }
    if let Some(saturation) = optional_number(component, "is")? {
        if saturation > 0.0 {
            return Ok((saturation, emission));
        }
        return Err(MnaError::InvalidParameterValue {
            component: component.id,
            key: "is",
            value: saturation,
            requirement: "strictly positive",
        });
    }
    let forward_voltage = positive_parameter(component, "vf")?;
    let forward_current = positive_parameter(component, "if")?;
    let thermal = emission * THERMAL_VOLTAGE;
    let saturation = forward_current / ((forward_voltage / thermal).min(MAX_EXPONENT).exp() - 1.0);
    if saturation.is_finite() && saturation > 0.0 {
        Ok((saturation, emission))
    } else {
        Err(MnaError::InvalidParameterValue {
            component: component.id,
            key: "vf",
            value: forward_voltage,
            requirement: "a finite positive saturation current",
        })
    }
}

fn led_model(component: &ComponentSpec) -> Result<(f64, f64), MnaError> {
    let emission = optional_number(component, "n")?.unwrap_or(2.0);
    if emission <= 0.0 {
        return Err(MnaError::InvalidParameterValue {
            component: component.id,
            key: "n",
            value: emission,
            requirement: "strictly positive",
        });
    }
    if let Some(saturation) = optional_number(component, "is")? {
        if saturation > 0.0 {
            return Ok((saturation, emission));
        }
        return Err(MnaError::InvalidParameterValue {
            component: component.id,
            key: "is",
            value: saturation,
            requirement: "strictly positive",
        });
    }
    let forward_voltage = positive_parameter(component, "vf")?;
    let forward_current = positive_parameter(component, "if")?;
    let thermal = emission * THERMAL_VOLTAGE;
    let saturation = forward_current / ((forward_voltage / thermal).min(MAX_EXPONENT).exp() - 1.0);
    if saturation.is_finite() && saturation > 0.0 {
        Ok((saturation, emission))
    } else {
        Err(MnaError::InvalidParameterValue {
            component: component.id,
            key: "vf",
            value: forward_voltage,
            requirement: "a finite positive saturation current",
        })
    }
}

fn bool_parameter(component: &ComponentSpec, key: &'static str) -> Result<bool, MnaError> {
    match component.params.get(key) {
        Some(ParamValue::Bool(value)) => Ok(*value),
        Some(ParamValue::Number(_) | ParamValue::Text(_)) => Err(MnaError::InvalidParameterType {
            component: component.id,
            key,
            expected: "a boolean",
        }),
        None => Err(MnaError::MissingParameter {
            component: component.id,
            key,
        }),
    }
}

#[cfg(test)]
mod shunt_reference_tests {
    use super::*;
    use crate::dc::{solve_dc_internal, DcOptions};
    use crate::netlist::{NetlistBuilder, NodeKind};

    #[test]
    fn shunt_reference_current_is_reported_from_cathode_to_anode() {
        let mut builder = NetlistBuilder::new();
        let ground = builder.add_node("0", NodeKind::Ground).unwrap();
        let supply = builder.add_node("supply", NodeKind::Electrical).unwrap();
        let cathode = builder.add_node("k", NodeKind::Electrical).unwrap();
        builder
            .add_component(
                ComponentKind::VoltageSource,
                "V1",
                [("dc".to_owned(), ParamValue::Number(5.0))].into(),
                vec![supply, ground],
            )
            .unwrap();
        builder
            .add_component(
                ComponentKind::Resistor,
                "R1",
                [("resistance".to_owned(), ParamValue::Number(100.0))].into(),
                vec![supply, cathode],
            )
            .unwrap();
        builder
            .add_component(
                ComponentKind::Resistor,
                "RL",
                [("resistance".to_owned(), ParamValue::Number(1_000.0))].into(),
                vec![cathode, ground],
            )
            .unwrap();
        let reference = builder
            .add_component(
                ComponentKind::ShuntReference,
                "U1",
                [
                    ("vref".to_owned(), ParamValue::Number(2.495)),
                    ("iref".to_owned(), ParamValue::Number(2e-6)),
                    ("ik_min".to_owned(), ParamValue::Number(1e-3)),
                    ("imax".to_owned(), ParamValue::Number(0.1)),
                    ("z_ka".to_owned(), ParamValue::Number(0.2)),
                ]
                .into(),
                vec![cathode, cathode, ground],
            )
            .unwrap();
        let netlist = builder.build().unwrap();
        let solved = solve_dc_internal(&netlist, DcOptions::default()).unwrap();
        let voltage = solved
            .circuit
            .component_voltage(reference, &solved.solution)
            .unwrap();
        let current = solved
            .circuit
            .component_current(reference, &solved.solution, AssemblyMode::Dc)
            .unwrap();

        assert!(voltage > 0.0);
        assert!(current > 0.0);
        let output = solved
            .circuit
            .node_voltage(&solved.solution, cathode)
            .unwrap();
        let expected = (5.0 - output) / 100.0 - output / 1_000.0 - 2e-6;
        assert!((current - expected).abs() < 1e-8);
    }
}

#[cfg(test)]
mod comparator_tests {
    use super::*;
    use crate::dc::{solve_dc_internal, DcOptions};
    use crate::netlist::{NetlistBuilder, NodeKind};

    #[test]
    fn comparator_current_is_positive_from_output_to_negative_supply() {
        let mut builder = NetlistBuilder::new();
        let ground = builder.add_node("0", NodeKind::Ground).unwrap();
        let vcc = builder.add_node("vcc", NodeKind::Electrical).unwrap();
        let input_plus = builder.add_node("in_plus", NodeKind::Electrical).unwrap();
        let input_minus = builder.add_node("in_minus", NodeKind::Electrical).unwrap();
        let output = builder.add_node("out", NodeKind::Electrical).unwrap();
        for (label, voltage, node) in [
            ("VCC", 5.0, vcc),
            ("VINP", 1.0, input_plus),
            ("VINM", 2.0, input_minus),
        ] {
            builder
                .add_component(
                    ComponentKind::VoltageSource,
                    label,
                    [("dc".to_owned(), ParamValue::Number(voltage))].into(),
                    vec![node, ground],
                )
                .unwrap();
        }
        builder
            .add_component(
                ComponentKind::Resistor,
                "RPU",
                [("resistance".to_owned(), ParamValue::Number(1_000.0))].into(),
                vec![vcc, output],
            )
            .unwrap();
        let comparator = builder
            .add_component(
                ComponentKind::Comparator,
                "U1",
                [
                    ("v_offset".to_owned(), ParamValue::Number(0.0)),
                    ("r_sink".to_owned(), ParamValue::Number(20.0)),
                    ("imax".to_owned(), ParamValue::Number(0.02)),
                ]
                .into(),
                vec![input_plus, input_minus, output, vcc, ground],
            )
            .unwrap();
        let netlist = builder.build().unwrap();
        let solved = solve_dc_internal(&netlist, DcOptions::default()).unwrap();
        let voltage = solved
            .circuit
            .component_voltage(comparator, &solved.solution)
            .unwrap();
        let current = solved
            .circuit
            .component_current(comparator, &solved.solution, AssemblyMode::Dc)
            .unwrap();
        let output_voltage = solved
            .circuit
            .node_voltage(&solved.solution, output)
            .unwrap();

        assert!(voltage > 0.0);
        assert!(current > 0.0);
        assert!((current - (5.0 - output_voltage) / 1_000.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod tests {
    use super::max_abs;

    #[test]
    fn max_abs_propagates_nan() {
        let vector = nalgebra::DVector::from_vec(vec![1.0, f64::NAN, 3.0]);
        assert!(max_abs(&vector).is_nan());
    }
}
