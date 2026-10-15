//! Typed runtime reads for admitting an AOT instant kernel.
use super::*;
use mchprs_blocks::blocks::RedstoneWireSide;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// A value that the runtime freezes when it admits an instant kernel.
pub(crate) enum InstantInput {
    /// This reads the stored powered bit of a control or stateful electrical component.
    Powered(NodeId),
    /// This reads a repeater's stored lock, independently of its current output.
    Locked(NodeId),
    /// This reads one bit of a comparator's retained analog output.
    SignalBit {
        /// This identifies the component owning the analog output.
        node: NodeId,
        /// This selects a bit in the four-bit strength.
        bit: u8,
    },
    /// This reads a stationary actor's logical extension bit; admission also checks its head.
    Extended(PistonId),
    /// This reads one bit of a shared payload's contact ordinal.
    ResourceBit {
        /// This identifies the conserved shared payload.
        resource: ResourceId,
        /// This selects a bit in the allowed-contact ordinal.
        bit: u8,
    },
    /// This reads a bit of a certified local shape-memory ordinal.
    ShapeBit {
        /// This indexes the entry's bounded local shape codebooks.
        memory: u32,
        /// This is the bit within that codebook's shape ordinal.
        bit: u8,
    },
}

impl InstantInput {
    /// Read one typed binding from the lowered program and its current state.
    pub(crate) fn read(self, program: &CircuitProgram, state: &CircuitState, shapes: &[WireShapeMemory]) -> Option<bool> {
        self.read_from(
            state,
            |resource| {
                Some((
                    state.resources.get(resource.index())?.at,
                    &program.resources().get(resource.index())?.contacts,
                ))
            },
            shapes,
        )
    }

    /// Read a typed binding while the compiler still owns the draft graph.
    pub(crate) fn read_draft(self, graph: &CompileGraph, state: &CircuitState, shapes: &[WireShapeMemory]) -> Option<bool> {
        self.read_from(
            state,
            |resource| {
                Some((
                    state.resources.get(resource.index())?.at,
                    &graph.resources.get(resource.index())?.contacts,
                ))
            },
            shapes,
        )
    }

    fn read_from<'a>(
        self,
        state: &CircuitState,
        resource_contacts: impl Fn(ResourceId) -> Option<(ContactId, &'a [ContactId])>,
        shapes: &[WireShapeMemory],
    ) -> Option<bool> {
        Some(match self {
            Self::Powered(node) => match state.components.get(node.index())? {
                ComponentState::Electrical(value) => value.powered,
                _ => return None,
            },
            Self::Locked(node) => match state.components.get(node.index())? {
                ComponentState::Electrical(value) => value.repeater_locked,
                _ => return None,
            },
            Self::SignalBit { node, bit } => match state.components.get(node.index())? {
                ComponentState::Electrical(value) if bit < 4 => value.output_strength & (1 << bit) != 0,
                _ => return None,
            },
            Self::Extended(actor) => state.pistons.get(actor.index())?.extended,
            Self::ResourceBit { resource, bit } => {
                if u32::from(bit) >= usize::BITS {
                    return None;
                }
                let (at, contacts) = resource_contacts(resource)?;
                let ordinal = contacts.iter().position(|&contact| contact == at)?;
                ordinal & (1 << bit) != 0
            }
            Self::ShapeBit { memory, bit } => {
                if u32::from(bit) >= usize::BITS {
                    return None;
                }
                let memory = shapes.get(memory as usize)?;
                let ComponentState::Wire(wire) = state.components.get(memory.node.index())? else {
                    return None;
                };
                let ordinal = memory.values.iter().position(|&shape| shape == wire_sides(*wire))?;
                ordinal & (1 << bit) != 0
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// A bounded set of stored dust shapes that runtime admission can encode as bits.
pub(crate) struct WireShapeMemory {
    /// This identifies the wire state owner read by the shape input bindings.
    pub(crate) node: NodeId,
    /// These are every stored shape reachable under the certified local geometry updates.
    pub(crate) values: Vec<[RedstoneWireSide; 4]>,
}

/// Return the stored side order used by both symbolic compilation and runtime admission.
pub(crate) fn wire_sides(wire: RedstoneWire) -> [RedstoneWireSide; 4] {
    [wire.north, wire.south, wire.east, wire.west]
}
