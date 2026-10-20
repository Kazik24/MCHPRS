//! Typed input expressions read source/state slots directly, without site reconstruction.
use super::*;

impl GraphBackend {
    pub(super) fn instant_drive_power(&self, node: NodeId) -> Option<u8> {
        let CircuitNodeKind::Wire(wire) = self.program.nodes()[node.index()].kind else {
            return None;
        };
        let (query, program) = self.program.instant_drive(wire)?;
        Some(self.evaluate_instant_drive(query, program))
    }

    pub(super) fn instant_drive_query_power(&self, query: u32) -> u8 {
        let program = self.program.instant_drive_program();
        self.evaluate_instant_drive(&program.queries[query as usize], program)
    }

    fn evaluate_instant_drive(&self, query: &DriveExpr, program: &InstantDriveProgram) -> u8 {
        let mut power = query.constant;
        for term in &program.terms[query.terms.start as usize..query.terms.end as usize] {
            if power == 15 {
                break;
            }
            if self.drive_condition(program, term.condition) {
                let value = match term.source {
                    DriveSource::Output(node) => self.stored_component_strength(node),
                    DriveSource::Powered(node) => u8::from(self.node_state(node).powered) * 15,
                    DriveSource::Constant(value) => value,
                };
                power = power.max(value.saturating_sub(term.attenuation));
            }
        }
        power
    }

    pub(super) fn drive_condition(&self, program: &InstantDriveProgram, condition: u32) -> bool {
        match &program.conditions[condition as usize] {
            DriveCondition::Always => true,
            DriveCondition::Never => false,
            DriveCondition::State(test) => self.state.matches(*test),
            DriveCondition::Not(child) => !self.drive_condition(program, *child),
            DriveCondition::All(children) => children.iter().all(|&child| self.drive_condition(program, child)),
            DriveCondition::Any(children) => children.iter().any(|&child| self.drive_condition(program, child)),
            DriveCondition::WireEmits { node, face, raw_sides } => {
                if *face == BlockFace::Top {
                    return true;
                }
                if *face == BlockFace::Bottom {
                    return false;
                }
                let wire = self.wire_state(*node);
                let raw = WireQueryFacts {
                    raw_sides: Some(*raw_sides),
                    ..Default::default()
                }
                .raw_wire(wire.power)
                .unwrap();
                let regulated = crate::redstone::wire::regulate_sides(wire, raw);
                crate::redstone::wire::get_current_side(regulated, face.unwrap_direction().opposite()) != RedstoneWireSide::None
            }
        }
    }
}
