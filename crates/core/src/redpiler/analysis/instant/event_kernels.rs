//! Compile FIFO-local cell predicates without substituting settled wire values.
use super::regions;
use crate::redpiler::{TaskMonitor, analysis::MAX_GRAPH_PREPARATION_BYTES, program_graph::*};
use rustc_hash::FxHashMap;

pub(crate) fn infer(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<InstantKernel>, String> {
    if monitor.cancelled() {
        return Err("cancelled before instant event compilation".into());
    }
    let Some(catalog) = &graph.plans.instant.boundaries else {
        return Ok(Vec::new());
    };
    let partition = regions::partition(graph, catalog, monitor)?;
    let mut kernels = Vec::new();
    let mut bytes = 0usize;
    for component in &partition.regions {
        if monitor.cancelled() {
            return Err("cancelled while compiling instant events".into());
        }
        if component.cells.is_empty() {
            continue;
        }
        let mut entries = Vec::new();
        let mut routes = Vec::new();
        for &cell in &component.cells {
            let cell = &catalog.cells[cell as usize];
            for use_ in &cell.uses {
                if monitor.cancelled() {
                    return Err("cancelled while reducing instant event entry".into());
                }
                let entry = compile_entry(cell, use_)?;
                bytes = bytes.saturating_add(std::mem::size_of::<InstantEventEntry>() + entry.pins.len() * std::mem::size_of::<CellPin>());
                for pin in &entry.pins {
                    if let CellPin::Gate { inputs, .. } = pin {
                        bytes = bytes.saturating_add(std::mem::size_of_val(inputs.as_ref()));
                    }
                }
                if bytes > MAX_GRAPH_PREPARATION_BYTES / 8 {
                    return Err("instant event programs exceed preparation memory bound".into());
                }
                entries.push(entry);
                routes.extend(use_.activations.iter().map(|event| event.route));
            }
        }
        entries.sort_unstable_by_key(|entry| entry.actor);
        routes.sort_unstable();
        routes.dedup();
        let actors = entries.iter().map(|entry| entry.actor).collect::<Vec<_>>().into_boxed_slice();
        kernels.push(InstantKernel {
            entries: routes.into_boxed_slice(),
            actors,
            // Live wires are retained dependencies, not eliminated or claimed as owned.
            wires: Box::new([]),
            routes: Box::new([]),
            program: KernelProgram::Events(entries.into_boxed_slice()),
        });
    }
    Ok(kernels)
}

fn compile_entry(cell: &CellContract, use_: &CellUse) -> Result<InstantEventEntry, String> {
    // Only this actor's readiness and live power are checked before entering the program.
    let mut constants = FxHashMap::default();
    constants.insert(use_.powered, false);
    for (index, pin) in cell.pins.iter().enumerate() {
        if matches!(pin, CellPin::Read(CellRead::State(StateTest::PistonExtended(actor))) if *actor == use_.actor)
            || matches!(pin, CellPin::Read(CellRead::State(StateTest::PistonAvailability {piston,is:PistonAvailability::Ready})) if *piston == use_.actor)
        {
            constants.insert(CellPinId(index as u32), true);
        }
    }
    let (pins, valid) = reduce_pins(&cell.pins, use_.valid, &constants)?;
    let (pins, valid) = reduce_pins(&pins, valid, &FxHashMap::default())?;
    Ok(InstantEventEntry {
        actor: use_.actor,
        pins: pins.into_boxed_slice(),
        valid,
    })
}

fn reduce_pins(source: &[CellPin], root: CellPinId, constants: &FxHashMap<CellPinId, bool>) -> Result<(Vec<CellPin>, CellPinId), String> {
    let mut needed = vec![false; source.len()];
    needed[root.index()] = true;
    for index in (0..source.len()).rev() {
        if needed[index]
            && !constants.contains_key(&CellPinId(index as u32))
            && let CellPin::Gate { inputs, .. } = &source[index]
        {
            for input in inputs {
                needed[input.index()] = true;
            }
        }
    }
    let mut remap = vec![None; source.len()];
    let mut pins = Vec::new();
    for (index, pin) in source.iter().enumerate() {
        if !needed[index] {
            continue;
        }
        let pin = if let Some(&value) = constants.get(&CellPinId(index as u32)) {
            constant_pin(value)
        } else {
            match pin {
                CellPin::Read(CellRead::Activation(_)) => return Err("acceptance depends on activation".into()),
                CellPin::Read(read) => CellPin::Read(*read),
                CellPin::Gate { kind, inputs } => fold_gate(
                    *kind,
                    inputs.iter().map(|input| remap[input.index()].expect("earlier needed pin")).collect(),
                    &pins,
                ),
            }
        };
        remap[index] = Some(CellPinId(pins.len() as u32));
        pins.push(pin);
    }
    Ok((pins, remap[root.index()].unwrap()))
}

fn constant_pin(value: bool) -> CellPin {
    CellPin::Gate {
        kind: if value { CellGateKind::All } else { CellGateKind::Any },
        inputs: Box::new([]),
    }
}

fn pin_constant(pin: &CellPin) -> Option<bool> {
    match pin {
        CellPin::Gate {
            kind: CellGateKind::All,
            inputs,
        } if inputs.is_empty() => Some(true),
        CellPin::Gate {
            kind: CellGateKind::Any,
            inputs,
        } if inputs.is_empty() => Some(false),
        _ => None,
    }
}

fn fold_gate(kind: CellGateKind, mut inputs: Vec<CellPinId>, pins: &[CellPin]) -> CellPin {
    match kind {
        CellGateKind::Buffer | CellGateKind::Invert => {
            if let Some(value) = pin_constant(&pins[inputs[0].index()]) {
                return constant_pin(value ^ (kind == CellGateKind::Invert));
            }
        }
        CellGateKind::All | CellGateKind::Any => {
            let identity = kind == CellGateKind::All;
            if inputs.iter().any(|id| pin_constant(&pins[id.index()]) == Some(!identity)) {
                return constant_pin(!identity);
            }
            inputs.retain(|id| pin_constant(&pins[id.index()]) != Some(identity));
        }
    }
    CellPin::Gate {
        kind,
        inputs: inputs.into_boxed_slice(),
    }
}
