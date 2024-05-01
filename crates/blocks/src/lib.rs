pub mod block_entities;
pub mod blocks;
pub mod items;

use mchprs_network::packets::PackedPos;
pub use mchprs_proc_macros::BlockProperty;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

#[derive(PartialEq, Eq, Copy, Clone, Debug, Serialize, Deserialize, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
    pub const fn zero() -> Self {
        Self::new(0, 0, 0)
    }
    pub const fn from_packed(packed: PackedPos) -> Self {
        let (x, y, z) = packed.coords();
        Self::new(x, y, z)
    }
    pub const fn packed(self) -> PackedPos {
        PackedPos::new(self.x, self.y, self.z)
    }
    pub const fn offset(self, face: BlockFace) -> Self {
        match face {
            BlockFace::Bottom => BlockPos::new(self.x, self.y.saturating_sub(1), self.z),
            BlockFace::Top => BlockPos::new(self.x, self.y + 1, self.z),
            BlockFace::North => BlockPos::new(self.x, self.y, self.z - 1),
            BlockFace::South => BlockPos::new(self.x, self.y, self.z + 1),
            BlockFace::West => BlockPos::new(self.x - 1, self.y, self.z),
            BlockFace::East => BlockPos::new(self.x + 1, self.y, self.z),
        }
    }
    pub fn max(self, other: Self) -> Self {
        Self {
            x: std::cmp::max(self.x, other.x),
            y: std::cmp::max(self.y, other.y),
            z: std::cmp::max(self.z, other.z),
        }
    }

    pub fn min(self, other: Self) -> Self {
        Self {
            x: std::cmp::min(self.x, other.x),
            y: std::cmp::min(self.y, other.y),
            z: std::cmp::min(self.z, other.z),
        }
    }
}

impl std::ops::Sub for BlockPos {
    type Output = BlockPos;

    fn sub(self, rhs: BlockPos) -> BlockPos {
        BlockPos {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

impl std::ops::Add for BlockPos {
    type Output = BlockPos;

    fn add(self, rhs: BlockPos) -> BlockPos {
        BlockPos {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl std::ops::Mul<i32> for BlockPos {
    type Output = BlockPos;

    fn mul(self, rhs: i32) -> BlockPos {
        BlockPos {
            x: self.x * rhs,
            y: self.y * rhs,
            z: self.z * rhs,
        }
    }
}

impl std::fmt::Display for BlockPos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {}, {})", self.x, self.y, self.z)
    }
}

pub trait BlockProperty: Sized {
    fn encode(self, props: &mut HashMap<&'static str, String>, name: &'static str);
    fn decode(&mut self, props: &HashMap<&str, &str>, name: &str);
}

impl<T> BlockProperty for T
where
    T: ToString + FromStr,
{
    fn encode(self, props: &mut HashMap<&'static str, String>, name: &'static str) {
        props.insert(name, self.to_string());
    }

    fn decode(&mut self, props: &HashMap<&str, &str>, name: &str) {
        if let Some(&str) = props.get(name) {
            if let Ok(val) = str.parse() {
                *self = val;
            }
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockFace {
    Bottom = 0,
    Top = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

impl BlockFace {
    #[inline]
    pub const fn get_id(self) -> u32 {
        self as u32
    }
    #[inline]
    pub const fn try_from_id(id: u32) -> Option<BlockFace> {
        match id {
            0 => Some(BlockFace::Bottom),
            1 => Some(BlockFace::Top),
            2 => Some(BlockFace::North),
            3 => Some(BlockFace::South),
            4 => Some(BlockFace::West),
            5 => Some(BlockFace::East),
            _ => None,
        }
    }
    // pub fn from_id(id: u32) -> BlockFace {

    // }

    pub fn values() -> [BlockFace; 6] {
        use BlockFace::*;
        [Top, Bottom, North, South, East, West]
    }

    pub fn is_horizontal(self) -> bool {
        use BlockFace::*;
        matches!(self, North | South | East | West)
    }

    pub fn unwrap_direction(self) -> BlockDirection {
        match self {
            BlockFace::North => BlockDirection::North,
            BlockFace::South => BlockDirection::South,
            BlockFace::East => BlockDirection::East,
            BlockFace::West => BlockDirection::West,
            _ => panic!("called `unwrap_direction` on {:?}", self),
        }
    }
    #[inline]
    pub const fn opposite(self) -> BlockFace {
        use BlockFace::*;
        match self {
            Bottom => Top,
            Top => Bottom,
            North => South,
            South => North,
            West => East,
            East => West,
        }
    }
}

//todo remove (not needed anymore)
impl From<BlockFace> for BlockFacing {
    #[inline]
    fn from(face: BlockFace) -> BlockFacing {
        match face {
            BlockFace::North => BlockFacing::North,
            BlockFace::South => BlockFacing::South,
            BlockFace::East => BlockFacing::East,
            BlockFace::West => BlockFacing::West,
            BlockFace::Top => BlockFacing::Up,
            BlockFace::Bottom => BlockFacing::Down,
        }
    }
}
impl From<BlockFacing> for BlockFace {
    #[inline]
    fn from(facing: BlockFacing) -> BlockFace {
        match facing {
            BlockFacing::North => BlockFace::North,
            BlockFacing::South => BlockFace::South,
            BlockFacing::East => BlockFace::East,
            BlockFacing::West => BlockFace::West,
            BlockFacing::Up => BlockFace::Top,
            BlockFacing::Down => BlockFace::Bottom,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlockColorVariant {
    White = 0,
    Orange = 1,
    Magenta = 2,
    LightBlue = 3,
    Yellow = 4,
    Lime = 5,
    Pink = 6,
    Gray = 7,
    LightGray = 8,
    Cyan = 9,
    Purple = 10,
    Blue = 11,
    Brown = 12,
    Green = 13,
    Red = 14,
    Black = 15,
}

impl BlockColorVariant {
    pub const fn get_id(self) -> u32 {
        self as u32
    }

    pub fn from_id(id: u32) -> BlockColorVariant {
        use BlockColorVariant::*;
        match id {
            0 => White,
            1 => Orange,
            2 => Magenta,
            3 => LightBlue,
            4 => Yellow,
            5 => Lime,
            6 => Pink,
            7 => Gray,
            8 => LightGray,
            9 => Cyan,
            10 => Purple,
            11 => Blue,
            12 => Brown,
            13 => Green,
            14 => Red,
            15 => Black,
            _ => panic!("invalid BlockColorVariant with id {}", id),
        }
    }
}

impl BlockProperty for BlockColorVariant {
    // Don't encode: the color is encoded in the block name
    fn encode(self, _props: &mut HashMap<&'static str, String>, _name: &'static str) {}
    fn decode(&mut self, _props: &HashMap<&str, &str>, _name: &str) {}
}

#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub enum BlockDirection {
    North,
    South,
    East,
    #[default]
    West,
}

impl BlockDirection {
    #[inline]
    pub const fn opposite(self) -> BlockDirection {
        use BlockDirection::*;
        match self {
            North => South,
            South => North,
            East => West,
            West => East,
        }
    }
    #[inline]
    pub const fn block_face(self) -> BlockFace {
        use BlockDirection::*;
        match self {
            North => BlockFace::North,
            South => BlockFace::South,
            East => BlockFace::East,
            West => BlockFace::West,
        }
    }
    #[inline]
    pub const fn block_facing(self) -> BlockFacing {
        use BlockDirection::*;
        match self {
            North => BlockFacing::North,
            South => BlockFacing::South,
            East => BlockFacing::East,
            West => BlockFacing::West,
        }
    }
    #[inline]
    pub fn from_id(id: u32) -> BlockDirection {
        match id {
            0 => BlockDirection::North,
            1 => BlockDirection::South,
            2 => BlockDirection::West,
            3 => BlockDirection::East,
            _ => panic!("invalid BlockDirection with id {}", id),
        }
    }
    #[inline]
    pub const fn get_id(self) -> u32 {
        match self {
            BlockDirection::North => 0,
            BlockDirection::South => 1,
            BlockDirection::West => 2,
            BlockDirection::East => 3,
        }
    }

    pub const fn rotate(self) -> BlockDirection {
        use BlockDirection::*;
        match self {
            North => East,
            East => South,
            South => West,
            West => North,
        }
    }

    pub fn rotate_ccw(self) -> BlockDirection {
        use BlockDirection::*;
        match self {
            North => West,
            West => South,
            South => East,
            East => North,
        }
    }
    #[inline]
    pub fn from_rotation(rotation: u8) -> Option<BlockDirection> {
        match rotation {
            0 => Some(BlockDirection::South),
            4 => Some(BlockDirection::West),
            8 => Some(BlockDirection::North),
            12 => Some(BlockDirection::East),
            _ => None,
        }
    }
}

impl FromStr for BlockDirection {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "north" => BlockDirection::North,
            "south" => BlockDirection::South,
            "east" => BlockDirection::East,
            "west" => BlockDirection::West,
            _ => return Err(()),
        })
    }
}

impl ToString for BlockDirection {
    fn to_string(&self) -> String {
        match self {
            BlockDirection::North => "north".to_owned(),
            BlockDirection::South => "south".to_owned(),
            BlockDirection::East => "east".to_owned(),
            BlockDirection::West => "west".to_owned(),
        }
    }
}

#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub enum BlockFacing {
    North,
    East,
    South,
    #[default]
    West,
    Up,
    Down,
}

impl BlockFacing {
    pub const fn horizontal_values() -> [Self; 4] {
        [Self::North, Self::South, Self::East, Self::West]
    }
    #[inline]
    pub const fn try_from_id(id: u32) -> Option<BlockFacing> {
        match id {
            0 => Some(BlockFacing::North),
            1 => Some(BlockFacing::East),
            2 => Some(BlockFacing::South),
            3 => Some(BlockFacing::West),
            4 => Some(BlockFacing::Up),
            5 => Some(BlockFacing::Down),
            _ => None,
        }
    }
    #[inline]
    pub const fn get_id(self) -> u32 {
        match self {
            BlockFacing::North => 0,
            BlockFacing::East => 1,
            BlockFacing::South => 2,
            BlockFacing::West => 3,
            BlockFacing::Up => 4,
            BlockFacing::Down => 5,
        }
    }
    #[inline]
    pub const fn offset_pos(self, mut pos: BlockPos, n: i32) -> BlockPos {
        match self {
            BlockFacing::North => pos.z -= n,
            BlockFacing::South => pos.z += n,
            BlockFacing::East => pos.x += n,
            BlockFacing::West => pos.x -= n,
            BlockFacing::Up => pos.y += n,
            BlockFacing::Down => pos.y -= n,
        }
        pos
    }
    #[inline]
    pub const fn rotate(self) -> BlockFacing {
        use BlockFacing::*;
        match self {
            North => East,
            East => South,
            South => West,
            West => North,
            other => other,
        }
    }
    #[inline]
    pub const fn rotate_ccw(self) -> BlockFacing {
        use BlockFacing::*;
        match self {
            North => West,
            West => South,
            South => East,
            East => North,
            other => other,
        }
    }
    #[inline]
    pub const fn opposite(self) -> BlockFacing {
        use BlockFacing::*;
        match self {
            North => South,
            South => North,
            East => West,
            West => East,
            Up => Down,
            Down => Up,
        }
    }
}

impl ToString for BlockFacing {
    fn to_string(&self) -> String {
        match self {
            BlockFacing::North => "north".to_owned(),
            BlockFacing::South => "south".to_owned(),
            BlockFacing::East => "east".to_owned(),
            BlockFacing::West => "west".to_owned(),
            BlockFacing::Up => "up".to_owned(),
            BlockFacing::Down => "down".to_owned(),
        }
    }
}

impl FromStr for BlockFacing {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "north" => BlockFacing::North,
            "south" => BlockFacing::South,
            "east" => BlockFacing::East,
            "west" => BlockFacing::West,
            "up" => BlockFacing::Up,
            "down" => BlockFacing::Down,
            _ => return Err(()),
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SignType(pub u8);

impl SignType {
    pub const fn to_item_type(self) -> u32 {
        self.0 as u32
    }
    pub const fn from_item_type(sign_type: u32) -> Self {
        Self(match sign_type {
            0 => 0, // Oak
            1 => 1, // Spruce
            2 => 2, // Birch
            3 => 4, // Jungle
            4 => 3, // Acacia
            5 => 5, // Dark Oak
            6 => 6, // Crimson
            7 => 7, // Warped
            _ => sign_type as _,
        })
    }
}

impl BlockProperty for SignType {
    // Don't encode
    fn encode(self, _props: &mut HashMap<&'static str, String>, _name: &'static str) {}
    fn decode(&mut self, _props: &HashMap<&str, &str>, _name: &str) {}
}
