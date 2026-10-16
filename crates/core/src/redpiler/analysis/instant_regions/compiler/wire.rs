//! Compile dust reads and writes to scalar expressions; these methods do not execute at runtime.
use super::*;

impl Compiler<'_> {
    pub(super) fn wire_neighbors(&self, node: NodeId) -> Result<[SiteId; 24]> {
        let CircuitNodeKind::Wire(index) = self.graph.nodes[node.index()].kind else {
            return Err(CompileError::UnsupportedNode(node));
        };
        Ok(self.graph.wires[index].canonical_neighbors)
    }

    fn wire_strength(&mut self, view: &SiteView) -> Result<Signal> {
        let mut maximum = Signal::literal(0)?;
        for variant in &view.variants {
            if let Block::RedstoneWire { wire } = variant.block {
                let value = variant
                    .component
                    .as_ref()
                    .map_or(Signal::literal(wire.power)?, |component| component.signal);
                let value = self.logic.signal_select(variant.when, value, Signal::literal(0)?)?;
                maximum = self.logic.maximum(maximum, value)?;
            }
        }
        Ok(maximum)
    }

    pub(crate) fn seed_power(&mut self, node: NodeId) -> Result<Signal> {
        self.step()?;
        let neighbors = self.wire_neighbors(node)?;
        let views = neighbors.into_iter().map(|site| self.observe(site)).collect::<Result<Vec<_>>>()?;
        let up_solid = self.site_test(&views[3], |variant| variant.solid)?;
        let clear = self.logic.not(up_solid)?;
        let mut direct = Signal::literal(0)?;
        let mut dust = Signal::literal(0)?;
        for (face, slot, above, below) in [
            (BlockFace::Top, 3, None, None),
            (BlockFace::Bottom, 2, None, None),
            (BlockFace::North, 4, Some(20), Some(17)),
            (BlockFace::South, 5, Some(21), Some(18)),
            (BlockFace::East, 1, Some(13), Some(12)),
            (BlockFace::West, 0, Some(8), Some(7)),
        ] {
            let power = self.power_no_dust(neighbors[slot], face, None)?;
            direct = self.logic.maximum(direct, power)?;
            let power = self.wire_strength(&views[slot])?;
            dust = self.logic.maximum(dust, power)?;
            if let (Some(above), Some(below)) = (above, below) {
                let transparent = self.site_test(&views[slot], |variant| variant.transparent)?;
                let opaque = self.logic.not(transparent)?;
                let climbs = self.logic.and(clear, opaque)?;
                let power = self.wire_strength(&views[above])?;
                let power = self.logic.signal_select(climbs, power, Signal::literal(0)?)?;
                dust = self.logic.maximum(dust, power)?;
                let solid = self.site_test(&views[slot], |variant| variant.solid)?;
                let descends = self.logic.not(solid)?;
                let power = self.wire_strength(&views[below])?;
                let power = self.logic.signal_select(descends, power, Signal::literal(0)?)?;
                dust = self.logic.maximum(dust, power)?;
            }
        }
        let dust = self.logic.attenuate(dust, 1)?;
        Ok(self.logic.maximum(direct, dust)?)
    }

    /// Captured geometry and dust strengths are frozen; conducted sources are read live.
    pub(crate) fn captured_wire_power(&mut self, node: NodeId, captured: &Capture, heading: usize) -> Result<Signal> {
        debug_assert!(heading < crate::redstone::wire::TURBO_ORDER.len());
        self.step()?;
        let neighbors = self.wire_neighbors(node)?;
        let views = neighbors.map(|site| captured.get(&site).expect("wire neighborhood must be captured before its query"));
        let mut direct = Signal::literal(0)?;
        for (face, slot) in BlockFace::values().into_iter().zip([3, 2, 4, 5, 1, 0]) {
            let power = self.power_no_dust(neighbors[slot], face, Some(views[slot]))?;
            direct = self.logic.maximum(direct, power)?;
        }
        let order = crate::redstone::wire::TURBO_ORDER[0];
        let up_solid = self.site_test(views[order[1]], |variant| variant.solid)?;
        let clear = self.logic.not(up_solid)?;
        let mut dust = Signal::literal(0)?;
        for (slot, down, up) in captured_wire_groups(0) {
            let power = self.wire_strength(views[slot])?;
            dust = self.logic.maximum(dust, power)?;
            let solid = self.site_test(views[slot], |variant| variant.solid)?;
            let transparent = self.site_test(views[slot], |variant| variant.transparent)?;
            let descends = self.logic.not(solid)?;
            let power = self.wire_strength(views[down])?;
            let power = self.logic.signal_select(descends, power, Signal::literal(0)?)?;
            dust = self.logic.maximum(dust, power)?;
            let opaque = self.logic.not(transparent)?;
            let climbs = self.logic.and(clear, opaque)?;
            let climbs = self.logic.and(climbs, solid)?;
            let power = self.wire_strength(views[up])?;
            let power = self.logic.signal_select(climbs, power, Signal::literal(0)?)?;
            dust = self.logic.maximum(dust, power)?;
        }
        let dust = self.logic.attenuate(dust, 1)?;
        Ok(self.logic.maximum(direct, dust)?)
    }

    pub(crate) fn write_wire(&mut self, node: NodeId, when: Formula, value: &Component) -> Result<()> {
        if when == Formula::FALSE {
            return Ok(());
        }
        self.step()?;
        let current = &self.state.components[node.index()];
        let old_shape = current.shape.ok_or(CompileError::UnsupportedNode(node))?;
        let new_shape = value.shape.ok_or(CompileError::UnsupportedNode(node))?;
        let signal = self.logic.signal_select(when, value.signal, current.signal)?;
        let shape = self.select_shape(when, new_shape, old_shape)?;
        let current = &mut self.state.components[node.index()];
        current.signal = signal;
        current.shape = Some(shape);
        self.state_written();
        Ok(())
    }
}

fn captured_wire_groups(heading: usize) -> [(usize, usize, usize); 4] {
    let order = crate::redstone::wire::TURBO_ORDER[heading];
    std::array::from_fn(|index| (order[4 + index], order[[8, 10, 12, 14][index]], order[[9, 11, 13, 15][index]]))
}

#[cfg(test)]
mod tests {
    use super::captured_wire_groups;
    use crate::redstone::wire::TURBO_ORDER;

    fn scalar_power(heading: usize, power: [u8; 24], solid: [bool; 24], transparent: [bool; 24]) -> u8 {
        let direct = [3, 2, 4, 5, 1, 0].into_iter().map(|slot| power[slot]).max().unwrap();
        let order = TURBO_ORDER[heading];
        let clear = !solid[order[1]];
        let mut dust = 0;
        for (slot, down, up) in captured_wire_groups(heading) {
            dust = dust.max(power[slot]);
            if !solid[slot] {
                dust = dust.max(power[down]);
            }
            if clear && solid[slot] && !transparent[slot] {
                dust = dust.max(power[up]);
            }
        }
        direct.max(dust.saturating_sub(1))
    }

    #[test]
    fn captured_power_uses_the_same_neighbor_groups_for_every_heading() {
        let expected = captured_wire_groups(0);
        let mut expected = expected.to_vec();
        expected.sort_unstable();
        for heading in 0..4 {
            assert_eq!(TURBO_ORDER[heading][1], 3, "the support-above slot is heading independent");
            let mut actual = captured_wire_groups(heading).to_vec();
            actual.sort_unstable();
            assert_eq!(actual, expected, "heading {heading}");
        }

        let mut seed = 0x6d2b_79f5_u32;
        for _ in 0..256 {
            let mut next = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                seed
            };
            let power = std::array::from_fn(|_| (next() & 15) as u8);
            let solid = std::array::from_fn(|_| next() & 1 != 0);
            let transparent = std::array::from_fn(|_| next() & 1 != 0);
            let expected = scalar_power(0, power, solid, transparent);
            for heading in 1..4 {
                assert_eq!(scalar_power(heading, power, solid, transparent), expected, "heading {heading}");
            }
        }
    }
}
