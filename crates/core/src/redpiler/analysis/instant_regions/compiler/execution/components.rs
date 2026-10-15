//! Conditional component transitions; only certified host-free semantics are compiled here.
use super::*;
use mchprs_blocks::blocks::ComparatorMode;

impl Execution {
    /// This is an entry equation only; response execution always uses the ordered handlers below.
    pub(crate) fn steady_component(compiler: &mut Compiler<'_>, node: NodeId) -> Result<Option<Component>> {
        let mut value = compiler.state.components[node.index()].clone();
        match compiler.graph.nodes[node.index()].kind {
            CircuitNodeKind::Electrical(NodeType::Torch) => {
                let input = compiler.input_power(node, InputPort::Main)?;
                let input = compiler.logic.powered(input)?;
                value.powered = compiler.logic.not(input)?;
                value.signal = compiler.logic.signal_select(value.powered, Signal::literal(15)?, Signal::literal(0)?)?;
            }
            CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => {
                let side = compiler.input_power(node, InputPort::Side)?;
                value.locked = compiler.logic.powered(side)?;
                let input = compiler.input_power(node, InputPort::Main)?;
                let input = compiler.logic.powered(input)?;
                value.powered = compiler.logic.select(value.locked, value.powered, input)?;
                value.signal = compiler.logic.signal_select(value.powered, Signal::literal(15)?, Signal::literal(0)?)?;
            }
            CircuitNodeKind::Electrical(NodeType::Comparator { mode, .. }) => {
                (value.signal, value.powered) = Self::comparator(compiler, node, mode)?;
            }
            CircuitNodeKind::Electrical(NodeType::Lamp | NodeType::Hopper) => {
                let input = compiler.input_power(node, InputPort::Main)?;
                value.powered = compiler.logic.powered(input)?;
            }
            _ => return Ok(None),
        }
        Ok(Some(value))
    }

    pub(super) fn set_component(
        &mut self,
        compiler: &mut Compiler<'_>,
        node: NodeId,
        when: Formula,
        powered: Formula,
        signal: Signal,
        locked: Option<Formula>,
        notify: bool,
    ) -> Result<()> {
        if when == Formula::FALSE {
            return Ok(());
        }
        compiler.step()?;
        let current = &mut compiler.state.components[node.index()];
        let power_changed = compiler.logic.xor(powered, current.powered)?;
        let same_signal = compiler.logic.signal_equal(signal, current.signal)?;
        let signal_changed = compiler.logic.not(same_signal)?;
        let mut changed = compiler.logic.or(power_changed, signal_changed)?;
        if let Some(locked) = locked {
            let lock_changed = compiler.logic.xor(locked, current.locked)?;
            changed = compiler.logic.or(changed, lock_changed)?;
            current.locked = compiler.logic.select(when, locked, current.locked)?;
        }
        current.powered = compiler.logic.select(when, powered, current.powered)?;
        current.signal = compiler.logic.signal_select(when, signal, current.signal)?;
        compiler.state_written();
        if notify {
            let changed = compiler.logic.and(when, changed)?;
            self.fire(compiler, node, changed)?;
        }
        Ok(())
    }

    fn clock_update(
        &mut self,
        compiler: &mut Compiler<'_>,
        node: NodeId,
        facing: BlockFace,
        direction: Option<BlockFace>,
        when: Formula,
        captured: Option<&SiteView>,
    ) -> Result<()> {
        let powered = compiler.captured_component(node, captured)?.powered;
        let qualifies = direction.map_or(powered, |direction| if direction == facing { Formula::TRUE } else { Formula::FALSE });
        let when = compiler.logic.and(when, qualifies)?;
        self.work.schedule(compiler, node, 2, TickPriority::Normal, when)?;
        Ok(())
    }

    pub(super) fn update_component(
        &mut self,
        compiler: &mut Compiler<'_>,
        node: NodeId,
        kind: DeliveryKind,
        when: Formula,
        captured: Option<&SiteView>,
    ) -> Result<()> {
        let direction = match kind {
            DeliveryKind::Update { direction } => direction,
            DeliveryKind::Shape { direction } => {
                return if matches!(compiler.graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) {
                    compiler.shape_wire(node, direction, when, captured, self)
                } else {
                    Ok(())
                };
            }
        };
        match compiler.graph.nodes[node.index()].kind {
            CircuitNodeKind::Wire(_) => compiler.seed_wire(node, when, captured, self),
            CircuitNodeKind::Observer { facing, .. } | CircuitNodeKind::BuiltinReset { facing, .. } => {
                self.clock_update(compiler, node, facing, direction, when, captured)
            }
            CircuitNodeKind::Electrical(NodeType::Torch) => {
                let lit = compiler.captured_component(node, captured)?.powered;
                let input = compiler.input_power(node, InputPort::Main)?;
                let input = compiler.logic.powered(input)?;
                let differs = compiler.logic.xor(lit, input)?;
                let qualifies = compiler.logic.not(differs)?;
                let when = compiler.logic.and(when, qualifies)?;
                self.work.schedule(compiler, node, 2, TickPriority::Normal, when)?;
                Ok(())
            }
            CircuitNodeKind::Electrical(NodeType::Repeater { delay, facing_diode }) => {
                self.update_repeater(compiler, node, delay, facing_diode, when, captured)
            }
            CircuitNodeKind::Electrical(NodeType::Comparator { facing_diode, mode, .. }) => {
                let (signal, powered) = Self::comparator(compiler, node, mode)?;
                let current = &compiler.state.components[node.index()];
                let equal = compiler.logic.signal_equal(signal, current.signal)?;
                let strength_changed = compiler.logic.not(equal)?;
                let power_changed = compiler.logic.xor(powered, current.powered)?;
                let changed = compiler.logic.or(strength_changed, power_changed)?;
                let when = compiler.logic.and(when, changed)?;
                self.work.schedule(
                    compiler,
                    node,
                    2,
                    if facing_diode { TickPriority::High } else { TickPriority::Normal },
                    when,
                )?;
                Ok(())
            }
            CircuitNodeKind::Electrical(NodeType::Lamp) => {
                let lit = compiler.captured_component(node, captured)?.powered;
                let input = compiler.input_power(node, InputPort::Main)?;
                let input = compiler.logic.powered(input)?;
                let not_input = compiler.logic.not(input)?;
                let off = compiler.logic.and(lit, not_input)?;
                let off = compiler.logic.and(when, off)?;
                self.work.schedule(compiler, node, 4, TickPriority::Normal, off)?;
                let not_lit = compiler.logic.not(lit)?;
                let on = compiler.logic.and(not_lit, input)?;
                let on = compiler.logic.and(when, on)?;
                self.set_component(compiler, node, on, Formula::TRUE, Signal::literal(0)?, None, false)
            }
            CircuitNodeKind::Electrical(NodeType::Hopper) => {
                let powered = compiler.captured_component(node, captured)?.powered;
                let input = compiler.input_power(node, InputPort::Main)?;
                let input = compiler.logic.powered(input)?;
                let differs = compiler.logic.xor(powered, input)?;
                let when = compiler.logic.and(when, differs)?;
                self.set_component(compiler, node, when, input, compiler.state.components[node.index()].signal, None, false)
            }
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Constant)
            | CircuitNodeKind::Contact(_) => Ok(()),
            CircuitNodeKind::Piston(actor) => self.sample(compiler, actor, when),
            _ => Err(CompileError::UnsupportedNode(node)),
        }
    }

    fn update_repeater(
        &mut self,
        compiler: &mut Compiler<'_>,
        node: NodeId,
        delay: u8,
        facing_diode: bool,
        when: Formula,
        captured: Option<&SiteView>,
    ) -> Result<()> {
        let old = compiler.captured_component(node, captured)?;
        let side = compiler.input_power(node, InputPort::Side)?;
        let locked = compiler.logic.powered(side)?;
        let lock_changed = compiler.logic.xor(old.locked, locked)?;
        let lock_changed = compiler.logic.and(when, lock_changed)?;
        let signal = compiler.logic.signal_select(old.powered, Signal::literal(15)?, Signal::literal(0)?)?;
        self.set_component(compiler, node, lock_changed, old.powered, signal, Some(locked), false)?;
        let input = compiler.input_power(node, InputPort::Main)?;
        let input = compiler.logic.powered(input)?;
        let changed = compiler.logic.xor(input, old.powered)?;
        let unlocked = compiler.logic.not(locked)?;
        let active = compiler.logic.and(when, unlocked)?;
        let active = compiler.logic.and(active, changed)?;
        if facing_diode {
            self.work.schedule(compiler, node, delay as usize * 2, TickPriority::Highest, active)?;
        } else {
            let not_input = compiler.logic.not(input)?;
            let off = compiler.logic.and(active, not_input)?;
            let on = compiler.logic.and(active, input)?;
            self.work.schedule(compiler, node, delay as usize * 2, TickPriority::Higher, off)?;
            self.work.schedule(compiler, node, delay as usize * 2, TickPriority::High, on)?;
        }
        Ok(())
    }

    pub(super) fn tick_component(&mut self, compiler: &mut Compiler<'_>, node: NodeId, when: Formula) -> Result<()> {
        let current = compiler.state.components[node.index()].clone();
        match compiler.graph.nodes[node.index()].kind {
            CircuitNodeKind::Wire(_) => Ok(()),
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } => {
                let powered = compiler.logic.not(current.powered)?;
                let schedule = compiler.logic.and(when, powered)?;
                self.work.schedule(compiler, node, 2, TickPriority::Normal, schedule)?;
                let signal = compiler.logic.signal_select(powered, Signal::literal(15)?, Signal::literal(0)?)?;
                let notify = matches!(compiler.graph.nodes[node.index()].kind, CircuitNodeKind::Observer { .. });
                self.set_component(compiler, node, when, powered, signal, None, notify)
            }
            CircuitNodeKind::Electrical(NodeType::Torch) => {
                let input = compiler.input_power(node, InputPort::Main)?;
                let input = compiler.logic.powered(input)?;
                let differs = compiler.logic.xor(current.powered, input)?;
                let same = compiler.logic.not(differs)?;
                let when = compiler.logic.and(when, same)?;
                let powered = compiler.logic.not(current.powered)?;
                let signal = compiler.logic.signal_select(powered, Signal::literal(15)?, Signal::literal(0)?)?;
                self.set_component(compiler, node, when, powered, signal, None, true)
            }
            CircuitNodeKind::Electrical(NodeType::Repeater { delay, .. }) => {
                let unlocked = compiler.logic.not(current.locked)?;
                let active = compiler.logic.and(when, unlocked)?;
                let input = compiler.input_power(node, InputPort::Main)?;
                let input = compiler.logic.powered(input)?;
                let absent = compiler.logic.not(input)?;
                let off = compiler.logic.and(current.powered, absent)?;
                let off = compiler.logic.and(active, off)?;
                let unpowered = compiler.logic.not(current.powered)?;
                let on = compiler.logic.and(active, unpowered)?;
                let followup = compiler.logic.and(on, absent)?;
                self.work.schedule(compiler, node, delay as usize * 2, TickPriority::Higher, followup)?;
                self.set_component(compiler, node, off, Formula::FALSE, Signal::literal(0)?, None, true)?;
                self.set_component(compiler, node, on, Formula::TRUE, Signal::literal(15)?, None, true)
            }
            CircuitNodeKind::Electrical(NodeType::Comparator { mode, .. }) => {
                let (signal, powered) = Self::comparator(compiler, node, mode)?;
                let same = compiler.logic.signal_equal(signal, current.signal)?;
                let changed = if mode == ComparatorMode::Compare {
                    Formula::TRUE
                } else {
                    compiler.logic.not(same)?
                };
                let when = compiler.logic.and(when, changed)?;
                self.set_component(compiler, node, when, powered, signal, None, false)?;
                self.fire(compiler, node, when)
            }
            CircuitNodeKind::Electrical(NodeType::Lamp) => {
                let input = compiler.input_power(node, InputPort::Main)?;
                let input = compiler.logic.powered(input)?;
                let absent = compiler.logic.not(input)?;
                let off = compiler.logic.and(current.powered, absent)?;
                let off = compiler.logic.and(when, off)?;
                self.set_component(compiler, node, off, Formula::FALSE, Signal::literal(0)?, None, false)
            }
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::PressurePlate | NodeType::Hopper | NodeType::Constant)
            | CircuitNodeKind::Contact(_) => Ok(()),
            CircuitNodeKind::Piston(actor) => self.sample(compiler, actor, when),
            _ => Err(CompileError::UnsupportedNode(node)),
        }
    }

    fn override_signal(compiler: &Compiler<'_>, source: ComparatorOverride) -> Result<Signal> {
        Ok(match source {
            ComparatorOverride::Literal(value) => Signal::literal(value)?,
            ComparatorOverride::LiveComponentOutput(node) => compiler.state.components[node.index()].signal,
        })
    }

    fn comparator(compiler: &mut Compiler<'_>, node: NodeId, mode: ComparatorMode) -> Result<(Signal, Formula)> {
        let mut input = Signal::literal(0)?;
        if let Some(plan) = compiler.graph.comparator_inputs.iter().find(|plan| plan.node == node).copied() {
            input = if let Some(source) = plan.direct_override {
                Self::override_signal(compiler, source)?
            } else {
                let rear = compiler.input_power(node, InputPort::Main)?;
                if let Some((_, source, guard)) = plan.far_override {
                    let maximum = compiler.logic.maximum(rear, Signal::literal(plan.far_threshold)?)?;
                    let above_threshold = compiler.logic.signal_equal(maximum, rear)?;
                    let below_threshold = compiler.logic.not(above_threshold)?;
                    let guard = compiler.guard(guard, None)?;
                    let use_override = compiler.logic.and(below_threshold, guard)?;
                    let value = Self::override_signal(compiler, source)?;
                    compiler.logic.signal_select(use_override, value, rear)?
                } else {
                    rear
                }
            };
        }
        let side = compiler.input_power(node, InputPort::Side)?;
        let maximum = compiler.logic.maximum(input, side)?;
        let at_least = compiler.logic.signal_equal(maximum, input)?;
        let equal = compiler.logic.signal_equal(input, side)?;
        let input_nonzero = compiler.logic.powered(input)?;
        let output = match mode {
            ComparatorMode::Compare => compiler.logic.signal_select(at_least, input, Signal::literal(0)?)?,
            ComparatorMode::Subtract => {
                let mut borrow = Formula::FALSE;
                let mut bits = [Formula::FALSE; 4];
                for (index, (a, b)) in input.bits().into_iter().zip(side.bits()).enumerate() {
                    let difference = compiler.logic.xor(a, b)?;
                    bits[index] = compiler.logic.xor(difference, borrow)?;
                    let not_a = compiler.logic.not(a)?;
                    let b_or_borrow = compiler.logic.or(b, borrow)?;
                    let starts = compiler.logic.and(not_a, b_or_borrow)?;
                    let carries = compiler.logic.and(b, borrow)?;
                    borrow = compiler.logic.or(starts, carries)?;
                }
                compiler.logic.signal_select(borrow, Signal::literal(0)?, Signal::from_bits(bits))?
            }
        };
        let enough = if mode == ComparatorMode::Compare {
            at_least
        } else {
            let unequal = compiler.logic.not(equal)?;
            compiler.logic.and(at_least, unequal)?
        };
        Ok((output, compiler.logic.and(input_nonzero, enough)?))
    }
}
