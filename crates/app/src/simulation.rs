//! Live transient session controlled by the application shell.

use std::collections::BTreeMap;
use std::time::Duration;

use sim_core::{TransientSample, TransientSession};
use web_time::Instant;

use crate::analysis::{
    prepare_realtime, LiveValues, RunContext, SimulationError, TransientSettings,
};
use crate::document::Document;

/// Wall-clock budget spent on integration during one frame, in seconds.
pub const LIVE_TICK_BUDGET_SECONDS: f64 = 0.005;
/// Smallest integration step used by the live session, in seconds.
///
/// The oscilloscope and the spectrum consume one sample per millisecond, so a
/// finer step only burns CPU. Complete analyses keep the configured step.
pub const LIVE_MIN_STEP_SECONDS: f64 = 1e-3;
/// Maximum number of samples retained for the scopes and the spectrum.
pub const MAX_SAMPLES: usize = 10_000;
/// Sliding time window shown by the scopes, in simulated seconds.
pub const DEFAULT_SCOPE_WINDOW_SECONDS: f64 = 10.0;
/// Smallest selectable scope window, in simulated seconds.
pub const MIN_SCOPE_WINDOW_SECONDS: f64 = 0.5;
/// Largest selectable scope window, in simulated seconds.
///
/// It matches the retained window at the live cadence (`MAX_SAMPLES` × 1 ms).
pub const MAX_SCOPE_WINDOW_SECONDS: f64 = 10.0;
/// Speed multipliers offered by the live controls.
pub const LIVE_SPEEDS: [f64; 6] = [0.25, 0.5, 1.0, 2.0, 5.0, 10.0];

/// Number of dead slots dropped at once when the sample ring is full.
///
/// The oldest slots stay allocated until `TRIM_CHUNK` of them are dead, then a
/// single front drain drops them. A small chunk keeps the worst frame cheap and
/// bounds the over-allocation; no steady-state frame allocates.
const TRIM_CHUNK: usize = 256;
/// Hard safety cap on the steps of one tick, independent of the time budget.
const MAX_STEPS_PER_TICK: usize = 4_096;
/// Longest wall-clock interval turned into simulated time after a host stall.
const MAX_CATCHUP_SECONDS: f64 = 0.25;
/// Window over which the simulated/real-time ratio is measured, in seconds.
const RATE_WINDOW_SECONDS: f64 = 1.0;
/// Ratio under which the engine is reported as too slow.
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
    samples: SampleBuffer,
    last_wall: f64,
    target_time: f64,
    speed: f64,
    paused: bool,
    rate: RateWindow,
    last_steps: usize,
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
        let mut setup = prepare_realtime(extraction, settings, document.revision())?;
        let requested = setup.context.nominal_step.unwrap_or(setup.options.step);
        let live_step = live_step(requested);
        setup.options.step = live_step;
        setup.context.nominal_step = Some(live_step);
        let mut session = TransientSession::new(&setup.netlist, &setup.options)
            .map_err(|error| SimulationError::Transient(error.to_string()))?;
        let mut samples = SampleBuffer::new(MAX_SAMPLES);
        for sample in session.drain_samples() {
            samples.push(sample);
        }
        let start = session.time();
        Ok(Self {
            session,
            context: setup.context,
            synced_revision: document.revision(),
            sample_revision: document.revision(),
            samples,
            last_wall: now,
            target_time: start,
            speed: 1.0,
            paused: false,
            rate: RateWindow::default(),
            last_steps: 0,
        })
    }

    /// Advances the session up to the host clock.
    pub fn tick(&mut self, document: &Document, now: f64) -> Result<TickOutcome, SimulationError> {
        self.tick_with_budget(
            document,
            now,
            Duration::from_secs_f64(LIVE_TICK_BUDGET_SECONDS),
        )
    }

    /// Advances the session with an explicit wall-clock budget.
    fn tick_with_budget(
        &mut self,
        document: &Document,
        now: f64,
        budget: Duration,
    ) -> Result<TickOutcome, SimulationError> {
        self.last_steps = 0;
        match self.sync(document)? {
            Sync::Restart => return Ok(TickOutcome::NeedsRestart),
            Sync::Unchanged | Sync::Updated => {}
        }
        let wall = (now - self.last_wall).max(0.0);
        self.last_wall = now;
        if self.paused {
            self.target_time = self.session.time();
            self.rate.reset();
            return Ok(TickOutcome::Paused);
        }
        // A host stall (window drag, debugger, sleep) must not turn into minutes
        // of catch-up work: the deficit is reported by the rate meter instead.
        self.target_time += wall.min(MAX_CATCHUP_SECONDS) * self.speed;
        let start_time = self.session.time();
        let started = Instant::now();
        let mut steps = 0;
        while self.session.time() < self.target_time && steps < MAX_STEPS_PER_TICK {
            self.session
                .advance()
                .map_err(|error| SimulationError::Transient(error.to_string()))?;
            self.samples.push(self.session.snapshot());
            steps += 1;
            // At least one step runs per tick, so a slow circuit still advances.
            if started.elapsed() >= budget {
                break;
            }
        }
        self.last_steps = steps;
        if steps > 0 {
            self.sample_revision = self.synced_revision;
        }
        let simulated = (self.session.time() - start_time).max(0.0);
        self.rate.record(simulated, wall * self.speed, wall);
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
            .advance()
            .map_err(|error| SimulationError::Transient(error.to_string()))?;
        self.samples.push(self.session.snapshot());
        self.last_steps = 1;
        self.target_time = self.session.time();
        self.sample_revision = self.synced_revision;
        self.rate = RateWindow::default();
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
            self.target_time = self.session.time();
            self.rate = RateWindow::default();
        }
    }

    /// Returns the ratio between simulated and requested time.
    pub const fn ratio(&self) -> f64 {
        self.rate.ratio
    }

    /// Returns whether the engine cannot keep up with the requested speed.
    pub const fn lagging(&self) -> bool {
        self.rate.lagging
    }

    /// Returns the number of steps executed by the latest tick.
    pub const fn last_steps(&self) -> usize {
        self.last_steps
    }

    /// Returns the retained samples in increasing time order.
    pub fn samples(&self) -> &[TransientSample] {
        self.samples.as_slice()
    }

    /// Returns the effective integration step used by the live session.
    pub fn effective_step(&self) -> Option<f64> {
        self.context.nominal_step
    }

    /// Returns the effective step of the live scope traces.
    pub fn scope_step(&self) -> Option<f64> {
        self.context.nominal_step
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
}

/// Returns the integration step used by the live session.
fn live_step(requested: f64) -> f64 {
    if requested.is_finite() && requested > LIVE_MIN_STEP_SECONDS {
        requested
    } else {
        LIVE_MIN_STEP_SECONDS
    }
}

/// Sample ring with an amortised head trim.
///
/// The oldest slots stay allocated until `TRIM_CHUNK` of them are dead, then a
/// single front drain drops them. A steady-state frame therefore never allocates
/// and never copies the retained window.
struct SampleBuffer {
    samples: Vec<TransientSample>,
    head: usize,
    capacity: usize,
}

impl SampleBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            samples: Vec::new(),
            head: 0,
            capacity: capacity.max(1),
        }
    }

    fn push(&mut self, sample: TransientSample) {
        if self.samples.len() >= self.capacity {
            self.head += 1;
            if self.head >= TRIM_CHUNK {
                self.samples.drain(0..self.head);
                self.head = 0;
            }
        }
        self.samples.push(sample);
    }

    fn as_slice(&self) -> &[TransientSample] {
        &self.samples[self.head..]
    }

    fn last(&self) -> Option<&TransientSample> {
        self.samples.last()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.samples.len() - self.head
    }

    #[cfg(test)]
    fn physical_len(&self) -> usize {
        self.samples.len()
    }
}

/// Sliding measurement of the simulated/real-time ratio.
struct RateWindow {
    simulated: f64,
    wanted: f64,
    wall: f64,
    ratio: f64,
    lagging: bool,
}

impl Default for RateWindow {
    fn default() -> Self {
        Self {
            simulated: 0.0,
            wanted: 0.0,
            wall: 0.0,
            ratio: 1.0,
            lagging: false,
        }
    }
}

impl RateWindow {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn record(&mut self, simulated: f64, wanted: f64, wall: f64) {
        self.simulated += simulated;
        self.wanted += wanted;
        self.wall += wall;
        if self.wall < RATE_WINDOW_SECONDS || self.wanted <= f64::MIN_POSITIVE {
            return;
        }
        self.ratio = (self.simulated / self.wanted).clamp(0.0, 1.0);
        self.lagging = self.ratio < LAG_THRESHOLD;
        self.simulated = 0.0;
        self.wanted = 0.0;
        self.wall = 0.0;
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
    fn live_step_is_floored_at_one_millisecond() {
        assert!((live_step(1e-5) - 1e-3).abs() < f64::EPSILON);
        assert!((live_step(1e-3) - 1e-3).abs() < f64::EPSILON);
        assert!((live_step(2e-3) - 2e-3).abs() < f64::EPSILON);
        assert!((live_step(f64::NAN) - 1e-3).abs() < f64::EPSILON);

        let document = Document::demo().unwrap();
        let session = LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        assert_eq!(session.effective_step(), Some(1e-3));
        assert_eq!(session.scope_step(), Some(1e-3));
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
    fn one_tick_reaches_the_host_clock_when_the_budget_allows() {
        let document = Document::demo().unwrap();
        let mut session =
            LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        // 10.5 ms requested, 1 ms live step: the eleventh step overshoots.
        assert_eq!(
            session.tick(&document, 0.0105).unwrap(),
            TickOutcome::Advanced
        );
        assert_eq!(session.last_steps(), 11);
        assert_eq!(session.samples().len(), 12);
        let times = session
            .samples()
            .iter()
            .map(|sample| sample.time)
            .collect::<Vec<_>>();
        for pair in times.windows(2) {
            assert!((pair[1] - pair[0] - 1e-3).abs() < 1e-12);
        }
        let last = session.last_sample().unwrap().time;
        assert!((0.0105..0.0115).contains(&last), "last time = {last}");
    }

    #[test]
    fn an_exhausted_budget_stops_after_one_step() {
        let document = Document::demo().unwrap();
        let mut session =
            LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        let outcome = session
            .tick_with_budget(&document, 0.1, Duration::ZERO)
            .unwrap();
        assert_eq!(outcome, TickOutcome::Advanced);
        assert_eq!(session.last_steps(), 1);
        assert!((session.last_sample().unwrap().time - 1e-3).abs() < 1e-12);
    }

    #[test]
    fn the_rate_meter_reports_a_budget_it_cannot_follow() {
        let document = Document::demo().unwrap();
        let mut session =
            LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        for index in 1..=30 {
            session
                .tick_with_budget(&document, f64::from(index) * 0.05, Duration::ZERO)
                .unwrap();
        }
        // 30 steps of 1 ms for 1.5 s requested.
        assert!(session.ratio() < 0.1, "ratio = {}", session.ratio());
        assert!(session.lagging());
    }

    #[test]
    fn the_rate_meter_follows_a_healthy_tick() {
        let document = Document::demo().unwrap();
        let mut session =
            LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        for index in 1..=30 {
            session.tick(&document, f64::from(index) * 0.05).unwrap();
        }
        assert!(session.ratio() > 0.95, "ratio = {}", session.ratio());
        assert!(!session.lagging());
    }

    #[test]
    fn the_sample_buffer_keeps_a_bounded_window() {
        let document = Document::demo().unwrap();
        let session = LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        let template = session.samples()[0].clone();
        let mut buffer = SampleBuffer::new(MAX_SAMPLES);
        for index in 0..25_000 {
            let mut sample = template.clone();
            sample.time = f64::from(index);
            buffer.push(sample);
        }
        assert_eq!(buffer.len(), MAX_SAMPLES);
        assert!(buffer.physical_len() <= MAX_SAMPLES * 3 / 2);
        let first = buffer.as_slice()[0].time;
        let last = buffer.last().unwrap().time;
        assert_eq!(last, 24_999.0);
        assert_eq!(first, 24_999.0 - (MAX_SAMPLES as f64 - 1.0));
    }

    #[test]
    fn pause_prevents_advancing() {
        let document = Document::demo().unwrap();
        let mut session =
            LiveSimulation::start(&document, &TransientSettings::default(), 0.0).unwrap();
        session.set_paused(true);
        let time = session.last_sample().unwrap().time;
        assert_eq!(session.tick(&document, 0.5).unwrap(), TickOutcome::Paused);
        assert_eq!(session.last_steps(), 0);
        assert_eq!(session.last_sample().unwrap().time, time);
        session.step_once(&document).unwrap();
        assert!(session.last_sample().unwrap().time > time);
    }
}
