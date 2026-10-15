//! Dust geometry as Boolean dependencies; no runtime neighbor discovery survives lowering.
use super::*;

impl Compiler<'_> {
    pub(crate) fn neighbor_shape(&mut self, stored: WireShape, raw: WireShape, face: BlockFace) -> Result<WireShape> {
        if face == BlockFace::Top {
            return Ok(stored);
        }
        if face == BlockFace::Bottom {
            return self.regulate_shape(stored, raw);
        }
        let side = match face {
            BlockFace::North => 1,
            BlockFace::South => 0,
            BlockFace::East => 3,
            BlockFace::West => 2,
            _ => unreachable!(),
        };
        let mut replaced = stored;
        replaced.sides[side] = raw.sides[side];
        let regulated = self.regulate_shape(replaced, raw)?;
        let old_cross = self.shape_cross(stored)?;
        let side_connected = self.connected(raw.sides[side])?;
        let side_absent = self.logic.not(side_connected)?;
        let keep_cross = self.logic.and(old_cross, side_absent)?;
        let old_dot = self.shape_dot(stored)?;
        let was_connected = self.logic.not(old_dot)?;
        let new_dot = self.shape_dot(regulated)?;
        let make_cross = self.logic.and(was_connected, new_dot)?;
        let cross = WireShape {
            sides: [WireSide {
                up: Formula::FALSE,
                flat: Formula::TRUE,
            }; 4],
        };
        let updated = self.select_shape(make_cross, cross, regulated)?;
        self.select_shape(keep_cross, stored, updated)
    }

    pub(crate) fn shape_equal(&mut self, left: WireShape, right: WireShape) -> Result<Formula> {
        let mut equal = Formula::TRUE;
        for (left, right) in left.sides.into_iter().zip(right.sides) {
            for (a, b) in [(left.up, right.up), (left.flat, right.flat)] {
                let differs = self.logic.xor(a, b)?;
                let same = self.logic.not(differs)?;
                equal = self.logic.and(equal, same)?;
            }
        }
        Ok(equal)
    }

    pub(crate) fn connected(&mut self, side: WireSide) -> Result<Formula> {
        Ok(self.logic.or(side.up, side.flat)?)
    }

    pub(crate) fn shape_dot(&mut self, shape: WireShape) -> Result<Formula> {
        let mut connected = Formula::FALSE;
        for side in shape.sides {
            let value = self.connected(side)?;
            connected = self.logic.or(connected, value)?;
        }
        Ok(self.logic.not(connected)?)
    }

    pub(crate) fn shape_cross(&mut self, shape: WireShape) -> Result<Formula> {
        let mut result = Formula::TRUE;
        for side in shape.sides {
            result = self.logic.and(result, side.flat)?;
        }
        Ok(result)
    }

    pub(crate) fn select_shape(&mut self, condition: Formula, yes: WireShape, no: WireShape) -> Result<WireShape> {
        let mut result = no;
        for (index, side) in result.sides.iter_mut().enumerate() {
            side.up = self.logic.select(condition, yes.sides[index].up, no.sides[index].up)?;
            side.flat = self.logic.select(condition, yes.sides[index].flat, no.sides[index].flat)?;
        }
        Ok(result)
    }

    pub(crate) fn raw_shape(&mut self, node: NodeId) -> Result<WireShape> {
        let CircuitNodeKind::Wire(index) = self.graph.nodes[node.index()].kind else {
            return Err(CompileError::UnsupportedNode(node));
        };
        let neighbors = self.graph.wires[index].canonical_neighbors;
        let up_view = self.observe(neighbors[3])?;
        let up_solid = self.site_test(&up_view, |variant| variant.block.is_solid())?;
        let clear = self.logic.not(up_solid)?;
        let mut shape = WireShape::literal(RedstoneWire::default());
        for (index, (direction, neighbor, above, below)) in [
            (BlockDirection::North, 4, 20, 17),
            (BlockDirection::South, 5, 21, 18),
            (BlockDirection::East, 1, 13, 12),
            (BlockDirection::West, 0, 8, 7),
        ]
        .into_iter()
        .enumerate()
        {
            let adjacent = self.observe(neighbors[neighbor])?;
            let top = self.observe(neighbors[above])?;
            let bottom = self.observe(neighbors[below])?;
            let direct = self.site_test(&adjacent, |variant| {
                crate::redstone::wire::can_connect_to_uncached(variant.block, direction)
            })?;
            let solid = self.site_test(&adjacent, |variant| variant.block.is_solid())?;
            let top_wire = self.site_test(&top, |variant| matches!(variant.block, Block::RedstoneWire { .. }))?;
            let bottom_wire = self.site_test(&bottom, |variant| matches!(variant.block, Block::RedstoneWire { .. }))?;
            let not_direct = self.logic.not(direct)?;
            let climbs = self.logic.and(clear, top_wire)?;
            let up = self.logic.and(not_direct, climbs)?;
            let no_climb = self.logic.not(climbs)?;
            let not_solid = self.logic.not(solid)?;
            let descends = self.logic.and(not_solid, bottom_wire)?;
            let descends = self.logic.and(no_climb, descends)?;
            let flat = self.logic.or(direct, descends)?;
            shape.sides[index] = WireSide { up, flat };
        }
        Ok(shape)
    }

    pub(crate) fn regulate_shape(&mut self, stored: WireShape, raw: WireShape) -> Result<WireShape> {
        let old_dot = self.shape_dot(stored)?;
        let raw_dot = self.shape_dot(raw)?;
        let keep_dot = self.logic.and(old_dot, raw_dot)?;
        let regulate = self.logic.not(keep_dot)?;
        let mut absent = [Formula::FALSE; 4];
        for (index, side) in raw.sides.into_iter().enumerate() {
            let connected = self.connected(side)?;
            absent[index] = self.logic.not(connected)?;
        }
        let ns_empty = self.logic.and(absent[0], absent[1])?;
        let ew_empty = self.logic.and(absent[2], absent[3])?;
        let mut result = raw;
        for (index, side) in result.sides.iter_mut().enumerate() {
            let perpendicular = if index < 2 { ew_empty } else { ns_empty };
            let add = self.logic.and(absent[index], perpendicular)?;
            let add = self.logic.and(add, regulate)?;
            side.flat = self.logic.or(side.flat, add)?;
        }
        Ok(result)
    }

    pub(crate) fn wire_emits(&mut self, node: NodeId, face: BlockFace) -> Result<Formula> {
        match face {
            BlockFace::Top => return Ok(Formula::TRUE),
            BlockFace::Bottom => return Ok(Formula::FALSE),
            _ => {}
        }
        let stored = self.state.components[node.index()].shape.ok_or(CompileError::UnsupportedNode(node))?;
        let raw = self.raw_shape(node)?;
        let regulated = self.regulate_shape(stored, raw)?;
        let index = match face {
            BlockFace::North => 1,
            BlockFace::South => 0,
            BlockFace::East => 3,
            BlockFace::West => 2,
            _ => unreachable!(),
        };
        self.connected(regulated.sides[index])
    }
}
