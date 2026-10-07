use mchprs_blocks::blocks::ComparatorMode;
use smallvec::SmallVec;
use std::num::NonZeroU8;
use std::ops::{Index, IndexMut};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// # Safety
    /// The index must fit in `u32` and belong to the `Nodes` used to dereference it.
    /// That node array must keep its indices valid for the lifetime of this ID.
    pub unsafe fn from_index(index: usize) -> NodeId {
        NodeId(index as u32)
    }
}

/// Fixed node storage. IDs are validated during compilation so runtime indexing
/// can skip bounds checks. Never use an ID from another backend's node array.
#[derive(Default)]
pub struct Nodes {
    nodes: Box<[Node]>,
}

impl Nodes {
    pub fn new(nodes: Box<[Node]>) -> Nodes {
        Nodes { nodes }
    }

    pub fn get(&self, idx: usize) -> NodeId {
        assert!(idx < self.nodes.len(), "node index out of bounds: {idx}");
        NodeId(idx as u32)
    }

    pub fn inner(&self) -> &[Node] {
        &self.nodes
    }

    pub fn inner_mut(&mut self) -> &mut [Node] {
        &mut self.nodes
    }

    pub fn into_inner(self) -> Box<[Node]> {
        self.nodes
    }
}

impl Index<NodeId> for Nodes {
    type Output = Node;

    fn index(&self, index: NodeId) -> &Self::Output {
        // Safety: IDs belong to this fixed array and were checked during compilation.
        unsafe { self.nodes.get_unchecked(index.0 as usize) }
    }
}

impl IndexMut<NodeId> for Nodes {
    fn index_mut(&mut self, index: NodeId) -> &mut Self::Output {
        // Safety: same invariant as immutable indexing.
        unsafe { self.nodes.get_unchecked_mut(index.0 as usize) }
    }
}

/// Packed update link: 27 bits for the node, one for the side channel, and
/// four for attenuation. Keep this layout small for the runtime update loop.
#[derive(Clone, Copy)]
pub struct ForwardLink {
    data: u32,
}

impl ForwardLink {
    pub fn new(id: NodeId, side: bool, attenuation: u8) -> Self {
        assert!(id.index() < (1 << 27));
        // The clamp_weights pass removes links that cannot carry power.
        assert!(attenuation < 15);
        Self {
            data: (id.index() as u32) << 5 | if side { 1 << 4 } else { 0 } | attenuation as u32,
        }
    }

    pub fn node(self) -> NodeId {
        unsafe {
            // safety: ForwardLink is constructed using a NodeId
            NodeId::from_index((self.data >> 5) as usize)
        }
    }

    pub fn side(self) -> bool {
        self.data & (1 << 4) != 0
    }

    pub fn attenuation(self) -> u8 {
        (self.data & 0b1111) as u8
    }
}

impl std::fmt::Debug for ForwardLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForwardLink")
            .field("node", &self.node())
            .field("side", &self.side())
            .field("attenuation", &self.attenuation())
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum NodeType {
    Repeater {
        delay: u8,
        facing_diode: bool,
    },
    Torch,
    Observer,
    Comparator {
        mode: ComparatorMode,
        far_input: Option<NonMaxU8>,
        facing_diode: bool,
    },
    Lamp,
    Button,
    Lever,
    PressurePlate,
    Trapdoor,
    Wire,
    Constant,
    CommandBlock {
        repeating: bool,
        chain: bool,
        automatic: bool,
    },
    InstantSource,
    NoteBlock {
        noteblock_id: u16,
    },
}

#[repr(align(16))]
#[derive(Debug, Clone, Default)]
pub struct NodeInput {
    /// Number of incoming links currently supplying each strength, from 0 to 15.
    /// Compilation caps each channel at 255 links to keep these counters in `u8`.
    pub strength_counts: [u8; 16],
}

#[derive(Debug, Clone, Copy)]
pub struct NonMaxU8(NonZeroU8);

impl NonMaxU8 {
    pub fn new(value: u8) -> Option<Self> {
        NonZeroU8::new(value + 1).map(Self)
    }

    pub fn get(self) -> u8 {
        self.0.get() - 1
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub ty: NodeType,
    pub default_inputs: NodeInput,
    pub side_inputs: NodeInput,
    pub updates: SmallVec<[ForwardLink; 10]>,
    pub is_io: bool,

    /// Powered or lit
    pub powered: bool,
    /// Only for repeaters
    pub locked: bool,
    pub output_power: u8,
    pub changed: bool,
    pub pending_tick: bool,
}
