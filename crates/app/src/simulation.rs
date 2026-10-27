//! Live transient session controlled by the application shell.

use std::collections::BTreeMap;

use sim_core::{TransientSample, TransientSession};

use crate::analysis::{
    prepare_realtime, LiveValues, RunContext, SimulationError, TransientSettings,
};
use crate::document::Document;

/// Maximum number of integration steps executed during one frame.
pub const MAX_STEPS_PER_FRAME: usize = 800;
/// Maximum number of samples retained for the spectral analyses.
pub const MAX_SAMPLES: usize = 100_000;
/// Sliding time window shown by the scopes, in simulated seconds.
pub const SCOPE_WINDOW_SECONDS: f64 = 10.0;
/// Maximum number of decimated samples kept for the scopes.
const MAX_SCOPE_SAMPLES: usize = 20_000;
/// Speed multipliers offered by the live controls.
pub const LIVE_SPEEDS: [f64; 6] = [0.25, 0.5, 1.0, 2.0, 5.0, 10.0];

const LAG_THRESHOLD: f64 = 0.65;

/// Result of one live tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickOutcome {
    /// The session advanced or stayed at the target time.
    Advanced,
    /// The session is paused.
    Paused,
    /// The edited schematic requires a fresh session.
    NeedsRestart,
}

/// Open-ended transient session with its presentation state.
pub struct LiveSimulation {
    session: TransientSession,
    context: RunContext,
    synced_revision: u64,
    sample_revision: u64,
    samples: Vec<TransientSample>,
    display_samples: Vec<TransientSample>,
    display_stride: usize,
    display_counter: usize,
    last_wall: f64,
    speed: f64,
    paused: bool,
    ratio: f64,
    lag: bool,
    simulated_total: f64,
    wall_total: f64,
}

impl LiveSimulation {
    /// Creates a live session from the current document.
    pub fn start(
        document: &Document,
        settings: &TransientSettings,
        now: f64,
    ) -> Result<Self, SimulationError> {
        let extraction = document
            .extraction()
            .ok_or_else(|| SimulationError::Transient("aucune netlist valide".to_owned()))?;
        let setup = prepare_realtime(extraction, settings, document.revision())?;
        let mut session = TransientSession::new(&setup.netlist, &setup.options)
            .map_err(|error| SimulationError::Transient(error.to_string()))?;
        let samples = session.drain_samples();
        let display_stride = scope_stride(setup.context.nominal_step);
        let display_samples = samples.clone();
        Ok(Self {
            session,
            context: setup.context,
            synced_revision: document.revision(),
            sample_revision: document.revision(),
            samples,
            display_samples,
            display_stride,
            display_counter: 0,
            last_wall: now,
            speed: 1.0,
            paused: false,
            ratio: 1.0,
            lag: false,
            simulated_total: 0.0,
            wall_total: 0.0,
        })
    }

    /// Advances the session up to the host clock.
    pub fn tick(&mut self, document: &Document, now: f64) -> Result<TickOutcome, SimulationError> {
        match self.sync(document)? {
            Sync::Restart => return Ok(TickOutcome::NeedsRestart),
            Sync::Unchanged | Sync::Updated => {}
        }
        if self.paused {
            self.last_wall = now;
            return Ok(TickOutcome::Paused);
        }
        let elapsed = (now - self.last_wall).max(0.0);
        self.last_wall = now;
        let start = self.session.time();
        let target = start + elapsed * self.speed;
        let mut steps = 0;
        while self.session.time() < target && steps < MAX_STEPS_PER_FRAME {
            self.session
                .step()
                .map_err(|error| SimulationError::Transient(error.to_string()))?;
            steps += 1;
        }
        let drained = self.session.drain_samples();
        self.ingest(drained);
        let simulated = (self.session.time() - start).max(0.0);
        self.simulated_total += simulated;
        self.wall_total += elapsed;
        let wanted = self.wall_total * self.speed;
        self.ratio = if wanted > f64::MIN_POSITIVE {
            (self.simulated_total / wanted).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.lag = self.ratio < LAG_THRESHOLD && self.wall_total > 0.1;
        if steps > 0 {
            self.sample_revision = self.synced_revision;
        }
        Ok(TickOutcome::Advanced)
    }

    /// Advances exactly one integration step while paused.
    pub fn step_once(&mut self, document: &Document) -> Result<(), SimulationError> {
        if let Sync::Restart = self.sync(document)? {
            return Err(SimulationError::Transient(
                "la topologie a changé ; redémarrez la session".to_owned(),
            ));
        }
        self.session
            .step()
            .map_err(|error| SimulationError::Transient(error.to_string()))?;
        let drained = self.session.drain_samples();
        self.ingest(drained);
        self.sample_revision = self.synced_revision;
        self.ratio = 1.0;
        self.lag = false;
        Ok(())
    }

    /// Pauses or resumes the session.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Returns whether the session is paused.
    pub const fn paused(&self) -> bool {
        self.paused
    }

    /// Returns the playback speed multiplier.
    pub const fn speed(&self) -> f64 {
        self.speed
    }

    /// Applies a valid playback speed.
    pub fn set_speed(&mut self, speed: f64) {
        if speed.is_finite() && speed > 0.0 {
            self.speed = speed.clamp(0.05, 100.0);
            self.simulated_total = 0.0;
            self.wall_total = 0.0;
            self.ratio = 1.0;
            self.lag = false;
        }
    }

    /// Returns the ratio between simulated and requested time.
    pub const fn ratio(&self) -> f64 {
        self.ratio
    }

    /// Returns whether the engine cannot keep up with the requested speed.
    pub const fn lagging(&self) -> bool {
        self.lag
    }

    /// Returns the retained samples in increasing time order.
    pub fn samples(&self) -> &[TransientSample] {
        &self.samples
    }

    /// Returns the decimated samples used by the scopes over a 10 s window.
    pub fn display_samples(&self) -> &[TransientSample] {
        &self.display_samples
    }

    /// Returns the effective step of the decimated scope traces.
    pub fn scope_step(&self) -> Option<f64> {
        self.context
            .nominal_step
            .map(|step| step * self.display_stride as f64)
    }

    /// Returns the name and revision mapping captured at session start.
    pub const fn context(&self) -> &RunContext {
        &self.context
    }

    /// Returns the latest sample.
    pub fn last_sample(&self) -> Option<&TransientSample> {
        self.samples.last()
    }

    /// Returns the document revision that produced the latest sample.
    pub const fn sample_revision(&self) -> u64 {
        self.sample_revision
    }

    /// Maps the latest sample to names for the inspector and canvas overlays.
    pub fn live_values(&self) -> LiveValues {
        let Some(sample) = self.samples.last() else {
            return LiveValues::default();
        };
        let mut node_voltages = BTreeMap::new();
        for (name, node) in &self.context.nodes {
            if let Some(voltage) = sample.node_voltage(*node) {
                node_voltages.insert(name.clone(), voltage);
            }
        }
        let mut branch_currents = BTreeMap::new();
        for (name, component) in &self.context.components {
            if let Some(current) = sample.branch_current(*component) {
                branch_currents.insert(name.clone(), current);
            }
        }
        LiveValues {
            document_revision: self.sample_revision,
            time: Some(sample.time),
            node_voltages,
            branch_currents,
        }
    }

    fn sync(&mut self, document: &Document) -> Result<Sync, SimulationError> {
        if self.synced_revision == document.revision() {
            return Ok(Sync::Unchanged);
        }
        let Some(extraction) = document.extraction() else {
            return Ok(Sync::Restart);
        };
        match self.session.update_netlist(&extraction.netlist) {
            Ok(()) => {
                self.synced_revision = document.revision();
                self.context.document_revision = document.revision();
                Ok(Sync::Updated)
            }
            Err(sim_core::TransientError::TopologyChanged) => Ok(Sync::Restart),
            Err(error) => Err(SimulationError::Transient(error.to_string())),
        }
    }

    fn trim_samples(&mut self) {
        if self.samples.len() > MAX_SAMPLES {
            let excess = self.samples.len() - MAX_SAMPLES;
            self.samples.drain(0..excess);
        }
    }

    fn ingest(&mut self, drained: Vec<TransientSample>) {
        for sample in drained {
            self.display_counter = self.display_counter.wrapping_add(1);
            if self.display_counter.is_multiple_of(self.display_stride) {
                self.display_samples.push(sample.clone());
            }
            self.samples.push(sample);
        }
        self.trim_samples();
        self.trim_display_samples();
    }

    fn trim_display_samples(&mut self) {
        let Some(latest) = self.display_samples.last().map(|sample| sample.time) else {
            return;
        };
        let cutoff = latest - SCOPE_WINDOW_SECONDS;
        let first = self
            .display_samples
            .partition_point(|sample| sample.time < cutoff);
        if first > 0 {
            self.display_samples.drain(0..first);
        }
        if self.display_samples.len() > MAX_SCOPE_SAMPLES {
            let excess = self.display_samples.len() - MAX_SCOPE_SAMPLES;
            self.display_samples.drain(0..excess);
        }
    }
}

fn scope_stride(step: Option<f64>) -> usize {
    let Some(step) = step.filter(|step| step.is_finite() && *step > 0.0) else {
        return 1;
    };
    let stride = (SCOPE_WINDOW_SECONDS / (step * MAX_SCOPE_SAMPLES as f64) - 1e-9).ceil();
    if stride.is_finite() && stride >= 1.0 {
        stride as usize
    } else {
        1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sync {
    Unchanged,
    Updated,
    Restart,
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::{GridPos, SymbolKind};

    #[test]
    fn live_session_starts_from_the_demo() {
        let document = Document::demo().unwrap();
        let session = LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        assert!(session.last_sample().is_some());
        assert!(!session.paused());
        assert_eq!(session.sample_revision(), document.revision());
    }

    #[test]
    fn a_parameter_change_does_not_reset_the_time() {
        let mut document = Document::demo().unwrap();
        let mut session =
            LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        session.tick(&document, 0.05).unwrap();
        let time = session.last_sample().unwrap().time;
        let resistor = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == SymbolKind::Resistor)
            .unwrap()
            .clone();
        let mut changed = resistor.clone();
        changed.params.insert(
            "resistance".to_owned(),
            sim_core::netlist::ParamValue::Number(2_000.0),
        );
        document.update_symbol(changed).unwrap();
        session.tick(&document, 0.10).unwrap();
        assert!(session.last_sample().unwrap().time >= time);
        assert_eq!(session.sample_revision(), document.revision());
    }

    #[test]
    fn a_topology_change_requests_a_restart() {
        let mut document = Document::demo().unwrap();
        let mut session =
            LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        document
            .add_symbol(SymbolKind::Resistor, GridPos::new(-20, -20))
            .unwrap();
        assert_eq!(
            session.tick(&document, 0.05).unwrap(),
            TickOutcome::NeedsRestart
        );
    }

    #[test]
    fn scope_samples_use_a_fixed_decimation_and_window() {
        let document = Document::demo().unwrap();
        let mut session =
            LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        assert_eq!(session.display_samples.len(), 1);
        let step = session.scope_step().unwrap();
        assert!((step - 5e-4).abs() < 1e-12, "scope step = {step}");
        for _ in 0..49 {
            session.step_once(&document).unwrap();
        }
        assert_eq!(session.display_samples.len(), 1);
        session.step_once(&document).unwrap();
        assert_eq!(session.display_samples.len(), 2);
    }

    #[test]
    fn display_samples_are_trimmed_to_ten_seconds() {
        let document = Document::demo().unwrap();
        let mut session =
            LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        let template = session.samples[0].clone();
        session.display_samples.clear();
        for index in 0..=20 {
            let mut sample = template.clone();
            sample.time = f64::from(index) * 0.6;
            session.display_samples.push(sample);
        }
        session.trim_display_samples();
        let first = session.display_samples.first().unwrap().time;
        assert!(first >= 2.0 - 1e-12, "first kept time = {first}");
    }

    #[test]
    fn scope_stride_scales_with_the_integration_step() {
        assert_eq!(scope_stride(Some(1e-5)), 50);
        assert_eq!(scope_stride(Some(1e-3)), 1);
        assert_eq!(scope_stride(Some(1e-6)), 500);
        assert_eq!(scope_stride(None), 1);
        assert_eq!(scope_stride(Some(0.0)), 1);
    }

    #[test]
    fn pause_prevents_advancing() {
        let document = Document::demo().unwrap();
        let mut session =
            LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        session.set_paused(true);
        let time = session.last_sample().unwrap().time;
        assert_eq!(session.tick(&document, 0.5).unwrap(), TickOutcome::Paused);
        assert_eq!(session.last_sample().unwrap().time, time);
        session.step_once(&document).unwrap();
        assert!(session.last_sample().unwrap().time > time);
    }
}
