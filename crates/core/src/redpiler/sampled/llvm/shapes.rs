//! Encode the shared sampled shape programs, including their ordered callbacks.
use super::{AUX_TABLES, Program, ScratchError, overflow};
use crate::redpiler::sampled::routing::Axis;

fn id(n: usize) -> Result<u32, ScratchError> {
    u32::try_from(n).map_err(|_| overflow())
}

pub(super) fn encode(p: &Program, t: &mut [Vec<u32>; AUX_TABLES]) -> Result<(), ScratchError> {
    id(p.shape_maps.len())?;
    for map in &p.shape_maps {
        t[57].extend(map.iter().map(|&v| u32::from(v)));
    }
    for range in &p.cell_shape_steps {
        let steps = match range {
            Some(range) => p.shape_steps.get(range.clone()).ok_or(ScratchError { position: None, reason: "invalid shape program range" })?,
            None => &[],
        };
        t[53].extend([if range.is_some() { id(t[54].len() / 7)? } else { u32::MAX }, id(steps.len())?]);
        for step in steps {
            t[54].extend([
                id(step.node)?,
                id(t[55].len() / 2)?,
                id(step.states.axes.len())?,
                id(t[56].len())?,
                id(step.states.ports.len())?,
                id(t[58].len())?,
                id(step.callbacks.len())?,
            ]);
            for &Axis::Token(token, cells) in &step.states.axes {
                t[55].extend([id(token)?, id(cells + 1)?]);
            }
            for state in &step.states.ports {
                let index = if let Some(map) = state {
                    if *map >= p.shape_maps.len() {
                        return Err(ScratchError { position: None, reason: "invalid shape map reference" });
                    }
                    id(*map)?
                } else {
                    u32::MAX
                };
                t[56].push(index);
            }
            for &node in &step.callbacks {
                t[58].push(id(node)?);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::sampled::{routing::Local, shapes::ShapeStep};
    use mchprs_blocks::{BlockPos, blocks::{Block, RedstoneWire}};

    fn wire_program() -> Program {
        let mut program = Program::new(
            vec![BlockPos::new(10, 10, 10)],
            vec![Block::RedstoneWire { wire: RedstoneWire::default() }],
            vec![], vec![[vec![], vec![]]], vec![], vec![], vec![],
        ).unwrap();
        program.prepare_projection();
        program
    }

    #[test]
    fn native_shape_export_distinguishes_missing_empty_and_invalid_programs() {
        let mut program = wire_program();
        let mut tables = std::array::from_fn(|_| Vec::new());
        encode(&program, &mut tables).unwrap();
        assert_eq!(tables[53], vec![u32::MAX, 0]);

        program.shape_programs.insert(program.positions[0], 0..0);
        program.prepare_projection();
        let mut tables = std::array::from_fn(|_| Vec::new());
        encode(&program, &mut tables).unwrap();
        assert_eq!(tables[53], vec![0, 0]);

        program.cell_shape_steps[0] = Some(0..1);
        let mut tables = std::array::from_fn(|_| Vec::new());
        assert_eq!(encode(&program, &mut tables).unwrap_err().reason, "invalid shape program range");
    }

    #[test]
    fn native_shape_export_preserves_pooled_map_ids_and_invalid_arrangements() {
        let mut program = wire_program();
        program.shape_maps.push(std::array::from_fn(|side| side as u8));
        program.shape_steps.push(ShapeStep {
            node: 0, states: Local { axes: vec![], ports: vec![Some(0)] }, callbacks: vec![],
        });
        program.shape_programs.insert(program.positions[0], 0..1);
        program.prepare_projection();
        let mut tables = std::array::from_fn(|_| Vec::new());
        encode(&program, &mut tables).unwrap();
        assert_eq!(tables[56], vec![0]);
        assert_eq!(tables[57], (0..256u32).collect::<Vec<_>>());

        program.shape_steps[0].states.ports[0] = None;
        let mut tables = std::array::from_fn(|_| Vec::new());
        encode(&program, &mut tables).unwrap();
        assert_eq!(tables[56], vec![u32::MAX]);

        program.shape_steps[0].states.ports[0] = Some(1);
        let mut tables = std::array::from_fn(|_| Vec::new());
        assert_eq!(encode(&program, &mut tables).unwrap_err().reason, "invalid shape map reference");
    }
}
