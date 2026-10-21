//! Test-only reduced power-query sampling at real accepted piston cuts.
use super::*;

const MAX_RECORDS: usize = 64;

#[derive(Debug, Clone)]
pub(crate) struct Record {
    pub(crate) tick: u64,
    pub(crate) phase: &'static str,
    pub(crate) action: mchprs_world::PistonAction,
    pub(crate) candidate_main: bool,
    pub(crate) candidate_quasi: bool,
    pub(crate) actual_main: u8,
    pub(crate) actual_quasi: u8,
    pub(crate) inputs: Box<[bool]>,
}

#[derive(Debug, Clone)]
pub(crate) struct Report {
    pub(crate) actor: PistonId,
    pub(crate) total_accepted: u64,
    pub(crate) main_matches: u64,
    pub(crate) quasi_matches: u64,
    pub(crate) main_mismatches: u64,
    pub(crate) quasi_mismatches: u64,
    pub(crate) evaluation_errors: u64,
    pub(crate) last_error: Option<String>,
    pub(crate) records: Vec<Record>,
}

pub(crate) struct Probe {
    report: Report,
    node: NodeId,
    circuit: SettledCircuit,
    evaluation: SettledEvaluation,
    inputs: Vec<bool>,
}

impl Probe {
    pub(crate) fn new(actor: PistonId, program: &CircuitProgram, circuit: SettledCircuit) -> Result<Self, String> {
        let node = program
            .pistons()
            .get(actor.index())
            .ok_or_else(|| format!("accepted-boundary probe actor {actor:?} is out of range"))?
            .node;
        let targets = [InputPort::Main, InputPort::Quasi];
        if circuit.outputs.len() != targets.len()
            || circuit.outputs.iter().zip(targets).any(|(output, port)| {
                output.kind != SettledValueKind::Bool
                    || output.roots.len() != 1
                    || output.target != SettledTarget::Power(PowerTarget::Input { node, port })
            })
        {
            return Err("accepted-boundary probe requires Bool Main then Quasi roots for its selected actor".into());
        }
        let evaluation = SettledEvaluation::new(&circuit)?;
        let inputs = vec![false; circuit.program.inputs.len()];
        Ok(Self {
            report: Report {
                actor,
                total_accepted: 0,
                main_matches: 0,
                quasi_matches: 0,
                main_mismatches: 0,
                quasi_mismatches: 0,
                evaluation_errors: 0,
                last_error: None,
                records: Vec::new(),
            },
            node,
            circuit,
            evaluation,
            inputs,
        })
    }

    fn sample(&mut self, backend: &GraphBackend, action: mchprs_world::PistonAction) {
        self.report.total_accepted += 1;
        for (slot, input) in self.inputs.iter_mut().zip(self.circuit.program.inputs.iter()) {
            match backend.sample_settled_input(input.source) {
                Ok(value) => *slot = value,
                Err(error) => {
                    self.report.evaluation_errors += 1;
                    self.report.last_error = Some(error.to_owned());
                    return;
                }
            }
        }
        if let Err(error) = self.evaluation.evaluate(&self.inputs) {
            self.report.evaluation_errors += 1;
            self.report.last_error = Some(error.to_owned());
            return;
        }
        let outputs = self.evaluation.outputs();
        let candidate_main = outputs[0] != 0;
        let candidate_quasi = outputs[1] != 0;
        let actual_main = backend.input_strength(self.node, InputPort::Main);
        let actual_quasi = backend.input_strength(self.node, InputPort::Quasi);
        self.report.main_matches += u64::from(candidate_main == (actual_main > 0));
        self.report.quasi_matches += u64::from(candidate_quasi == (actual_quasi > 0));
        self.report.main_mismatches += u64::from(candidate_main != (actual_main > 0));
        self.report.quasi_mismatches += u64::from(candidate_quasi != (actual_quasi > 0));
        if self.report.records.len() < MAX_RECORDS {
            self.report.records.push(Record {
                tick: backend.future.clock.logical_tick,
                phase: phase_name(backend.future.clock.phase),
                action,
                candidate_main,
                candidate_quasi,
                actual_main,
                actual_quasi,
                inputs: self.inputs.clone().into_boxed_slice(),
            });
        }
    }

    pub(crate) fn into_report(self) -> Report {
        self.report
    }
}

fn phase_name(phase: GraphPhase) -> &'static str {
    match phase {
        GraphPhase::BetweenTicks => "between-ticks",
        GraphPhase::ScheduledTicks => "scheduled-ticks",
        GraphPhase::PistonActions => "piston-actions",
        GraphPhase::DeferredConnections => "deferred-connections",
    }
}

impl GraphBackend {
    pub(super) fn record_accepted_boundary_probe(&mut self, request: PistonRequest, accepted: bool) {
        if !accepted
            || !self
                .test_recorder
                .accepted_boundary_probe
                .as_ref()
                .is_some_and(|probe| probe.report.actor == request.piston)
        {
            return;
        }
        let mut probe = self.test_recorder.accepted_boundary_probe.take().unwrap();
        probe.sample(self, request.action);
        self.test_recorder.accepted_boundary_probe = Some(probe);
    }
}
