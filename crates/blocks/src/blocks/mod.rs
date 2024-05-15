mod props;

use crate::{BlockColorVariant, BlockDirection, BlockFacing, BlockProperty, SignType};
use mchprs_proc_macros::BlockTransform;
pub use props::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub enum FlipDirection {
    FlipX,
    FlipZ,
}

#[derive(Clone, Copy, Debug)]
pub enum RotateAmt {
    Rotate90,
    Rotate180,
    Rotate270,
}

trait BlockTransform {
    fn rotate(&mut self, amt: crate::blocks::RotateAmt) {
        match amt {
            // ez
            RotateAmt::Rotate90 => self.rotate90(),
            RotateAmt::Rotate180 => {
                self.rotate90();
                self.rotate90();
            }
            RotateAmt::Rotate270 => {
                self.rotate90();
                self.rotate90();
                self.rotate90();
            }
        }
    }
    fn rotate90(&mut self);
    fn flip(&mut self, dir: crate::blocks::FlipDirection);
}

macro_rules! noop_block_transform {
    ($($ty:ty),*$(,)?) => {
        $(
            impl BlockTransform for $ty {
                fn rotate90(&mut self) {}
                fn flip(&mut self, _dir: crate::blocks::FlipDirection) {}
            }
        )*
    };
}

noop_block_transform!(
    u8,
    u32,
    bool,
    BlockColorVariant,
    TrapdoorHalf,
    SignType,
    ButtonFace,
    LeverFace,
    ComparatorMode,
    Instrument,
);

impl BlockTransform for BlockDirection {
    fn flip(&mut self, dir: FlipDirection) {
        match dir {
            FlipDirection::FlipX => match self {
                BlockDirection::East => *self = BlockDirection::West,
                BlockDirection::West => *self = BlockDirection::East,
                _ => {}
            },
            FlipDirection::FlipZ => match self {
                BlockDirection::North => *self = BlockDirection::South,
                BlockDirection::South => *self = BlockDirection::North,
                _ => {}
            },
        }
    }

    fn rotate90(&mut self) {
        *self = match self {
            BlockDirection::North => BlockDirection::East,
            BlockDirection::East => BlockDirection::South,
            BlockDirection::South => BlockDirection::West,
            BlockDirection::West => BlockDirection::North,
        }
    }
}
impl BlockTransform for BlockFacing {
    fn flip(&mut self, dir: FlipDirection) {
        match dir {
            FlipDirection::FlipX => match self {
                Self::East => *self = Self::West,
                Self::West => *self = Self::East,
                _ => {}
            },
            FlipDirection::FlipZ => match self {
                Self::North => *self = Self::South,
                Self::South => *self = Self::North,
                _ => {}
            },
        }
    }
    fn rotate90(&mut self) {
        *self = match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
            _ => *self,
        }
    }
}

impl Block {
    pub fn has_block_entity(self) -> bool {
        match self {
            Block::RedstoneComparator { .. }
            | Block::Barrel { .. }
            | Block::Furnace { .. }
            | Block::Hopper { .. }
            | Block::Sign { .. }
            | Block::WallSign { .. }
            | Block::PistonHead { .. }
            | Block::MovingPiston { .. } => true,
            Block::Piston { piston } => piston.extended,
            _ => false,
        }
    }

    pub fn can_place_block_in(self) -> bool {
        matches!(self.get_id(),
            0             // Air
            | 9915..=9916 // Void and Cave air
            | 34..=49     // Water
            | 50..=65     // Lava
            | 1398        // Grass
            | 1399        // Fern
            | 1400        // Dead bush
            | 1401        // Seagrass
            | 1402..=1403 // Tall Seagrass
            | 8143..=8144 // Tall Grass
            | 8145..=8146 // Tall Fern
        )
    }
}

#[test]
fn repeater_id_test() {
    let original = Block::RedstoneRepeater {
        repeater: RedstoneRepeater::new(3, BlockDirection::West, true, false),
    };
    let id = original.get_id();
    assert_eq!(id, 4141);
    let new = Block::from_id(id);
    assert_eq!(new, original);
}

#[test]
fn comparator_id_test() {
    let original = Block::RedstoneComparator {
        comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Subtract, false),
    };
    let id = original.get_id();
    assert_eq!(id, 6895);
    let new = Block::from_id(id);
    assert_eq!(new, original);
}

#[test]
fn test_piston_observers_id_conversions() {
    let ids = (1385..=1415) // pistons
        .chain(9510..=9521) // observers
        .chain(1416..=1439); // piston heads
    for i in ids {
        let block = Block::from_id(i);
        let id = block.get_id();
        assert_eq!(id, i);
    }
}



macro_rules! blocks { 
    (
        $(
            #simple $simple_name:ident($simple_d:expr, $simple_t:expr)
        ),*
        $(,)?
        $(
            $name:ident {
                $(props: {
                    $(
                        $prop_name:ident : $prop_type:ident
                    ),*
                    $(,)?
                },)?
                get_id: $get_id:expr,
                $( from_id_offset: $get_id_offset:literal, )?
                from_id($id_name:ident): $from_id_pat:pat => {
                    $(
                        $from_id_pkey:ident: $from_id_pval:expr
                    ),*
                    $(,)?
                },
                from_names($name_name:ident): {
                    $(
                        $from_name_pat:pat => {
                            $(
                                $from_name_pkey:ident: $from_name_pval:expr
                            ),*
                            $(,)?
                        }
                    ),*
                    $(,)?
                },
                get_name: $get_name:expr,
                $( solid: $solid:literal, )?
                $( transparent: $transparent:literal, )?
                $( cube: $cube:literal, )?
                $(,)?
            }
        ),*
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Block {
            $(
                $simple_name {},
            )*
            $(
                $name $({
                    $(
                        $prop_name: $prop_type,
                    )*
                })?
            ),*
        }

        #[allow(clippy::redundant_field_names)]
        impl Block {
            pub fn is_solid(self) -> bool {
                match self {
                    $(
                        Block::$simple_name {} => true,
                    )*
                    $(
                        $( Block::$name { .. } => $solid, )?
                    )*
                    _ => false
                }
            }

            pub fn is_transparent(self) -> bool {
                match self {
                    $(
                        Block::$simple_name {} => true,
                    )*
                    $(
                        $( Block::$name { .. } => $transparent, )?
                    )*
                    _ => false
                }
            }

            pub fn is_cube(self) -> bool {
                match self {
                    $(
                        Block::$simple_name {} => true,
                    )*
                    $(
                        $( Block::$name { .. } => $cube, )?
                    )*
                    _ => false
                }
            }

            pub fn is_simple_cube(&self) -> bool {
                !self.has_block_entity() || self.is_cube()
            }

            pub const fn get_id(self) -> u32 {
                match self {
                    $(
                        Block::$simple_name {} => $simple_d,
                    )*
                    $(
                        Block::$name {
                            $($(
                                $prop_name,
                            )*)?
                        } => $get_id,
                    )*
                }
            }

            pub fn from_id(mut id: u32) -> Block {
                match id {
                    $(
                        $simple_d => Block::$simple_name {},
                    )*
                    $(
                        $from_id_pat => {
                            $( id -= $get_id_offset; )?
                            let $id_name = id;
                            Block::$name {
                                $(
                                    $from_id_pkey: $from_id_pval
                                ),*
                            }
                        },
                    )*
                }
            }

            pub fn from_name(name: &str) -> Option<Block> {
                match name {
                    $(
                        $simple_t => Some(Block::$simple_name {}),
                    )*
                    $(
                        $(
                            $from_name_pat => {
                                let $name_name = name;
                                Some(Block::$name {
                                    $(
                                        $from_name_pkey: $from_name_pval
                                    ),*
                                })
                            },
                        )*
                    )*
                    _ => None,
                }
            }

             // Not all props will be part of the name
            #[allow(unused_variables)]
            pub fn get_name(self) -> &'static str {
                match self {
                    $(
                        Block::$simple_name {} => $simple_t,
                    )*
                    $(
                        Block::$name {
                            $($(
                                $prop_name,
                            )*)?
                        } => $get_name,
                    )*
                }
            }

            pub fn set_properties(&mut self, props: HashMap<&str, &str>) {
                match self {
                    $(
                        Block::$simple_name {} => {},
                    )*
                    $(
                        Block::$name {
                            $($(
                                $prop_name,
                            )*)?
                        } => {
                            $($(
                                <$prop_type as BlockProperty>::decode($prop_name, &props, stringify!($prop_name));
                            )*)?
                        },
                    )*
                }
            }

            pub fn properties(&self) -> HashMap<&'static str, String> {
                let mut props = HashMap::new();
                match self {
                    $(
                        Block::$simple_name {} => {},
                    )*
                    $(
                        Block::$name {
                            $($(
                                $prop_name,
                            )*)?
                        } => {
                            $($(
                                <$prop_type as BlockProperty>::encode(*$prop_name, &mut props, stringify!($prop_name));
                            )*)?
                        },
                    )*
                }
                props
            }

            pub fn rotate(&mut self, amt: RotateAmt) {
                match self {
                    $(
                        Block::$simple_name {} => {},
                    )*
                    $(
                        Block::$name {
                            $($(
                                $prop_name,
                            )*)?
                        } => {
                            $($(
                                <$prop_type as BlockTransform>::rotate($prop_name, amt);
                            )*)?
                        },
                    )*
                }
            }

            pub fn flip(&mut self, dir: FlipDirection) {
                match self {
                    $(
                        Block::$simple_name {} => {},
                    )*
                    $(
                        Block::$name {
                            $($(
                                $prop_name,
                            )*)?
                        } => {
                            $($(
                                <$prop_type as BlockTransform>::flip($prop_name, dir);
                            )*)?
                        },
                    )*
                }
            }
        }
    }
}

// list of block states: https://github.com/PrismarineJS/minecraft-data/blob/master/data/pc/1.18/blocks.json
blocks! {
    // Stone {
    //     get_id: 1,
    //     from_id(_id): 1 => {},
    //     from_names(_name): {
    //         "stone" => {}
    //     },
    //     get_name: "stone",
    //     solid: true,
    //     cube: true,
    // },
    #simple Stone(1, "stone"),
    #simple Bedrock(25, "bedrock"),
    #simple TNT(143, "tnt"),
    #simple OakPlanks(13, "oak_planks"),
    #simple SprucePlanks(14, "spruce_planks"),
    #simple BirchPlanks(15, "birch_planks"),
    #simple JunglePlanks(16, "jungle_planks"),
    #simple AcaciaPlanks(17, "acacia_planks"),
    #simple DarkOakPlanks(18, "dark_oak_planks"),
    #simple OakLog(38, "oak_log"),
    #simple SpruceLog(39, "spruce_log"),
    #simple BirchLog(40, "birch_log"),
    #simple JungleLog(41, "jungle_log"),
    #simple AcaciaLog(42, "acacia_log"),
    #simple DarkOakLog(43, "dark_oak_log"),
    #simple StrippedSpruceLog(44, "stripped_spruce_log"),
    #simple StrippedBirchLog(45, "stripped_birch_log"),
    #simple StrippedJungleLog(46, "stripped_jungle_log"),
    #simple StrippedAcaciaLog(47, "stripped_acacia_log"),
    #simple StrippedDarkOakLog(48, "stripped_dark_oak_log"),
    #simple StrippedOakLog(49, "stripped_oak_log"),
    #simple OakWood(50, "oak_wood"),
    #simple SpruceWood(51, "spruce_wood"),
    #simple BirchWood(52, "birch_wood"),
    #simple JungleWood(53, "jungle_wood"),
    #simple AcaciaWood(54, "acacia_wood"),
    #simple DarkOakWood(55, "dark_oak_wood"),
    #simple StrippedOakWood(56, "stripped_oak_wood"),
    #simple StrippedSpruceWood(57, "stripped_spruce_wood"),
    #simple StrippedBirchWood(58, "stripped_birch_wood"),
    #simple StrippedJungleWood(59, "stripped_jungle_wood"),
    #simple StrippedAcaciaWood(60, "stripped_acacia_wood"),
    #simple StrippedDarkOakWood(61, "stripped_dark_oak_wood"),
    #simple Bookshelf(144, "bookshelf"),
    #simple Sponge(70, "sponge"),
    #simple HayBale(404, "hay_block"),
    #simple MossBlock(865, "moss_block"),
    #simple Granite(2, "granite"),
    #simple PolishedGranite(3, "polished_granite"),
    #simple Diorite(4, "diorite"),
    #simple PolishedDiorite(5, "polished_diorite"),
    #simple Andesite(6, "andesite"),
    #simple PolishedAndesite(7, "polished_andesite"),
    #simple Cobblestone(12, "cobblestone"),
    #simple GoldOre(31, "gold_ore"),
    #simple DeepslateGoldOre(32, "deepslate_gold_ore"),
    #simple IronOre(33, "iron_ore"),
    #simple DeepslateIronOre(34, "deepslate_iron_ore"),
    #simple CoalOre(35, "coal_ore"),
    #simple DeepslateCoalOre(36, "deepslate_coal_ore"),
    #simple NetherGoldOre(37, "nether_gold_ore"),
    #simple LapisLazuliOre(73, "lapis_ore"),
    #simple DeepslateLapisLazuliOre(74, "deepslate_lapis_ore"),
    #simple BlockofLapisLazuli(75, "lapis_block"),
    #simple ChiseledSandstone(78, "chiseled_sandstone"),
    #simple CutSandstone(79, "cut_sandstone"),
    #simple BlockofGold(140, "gold_block"),
    #simple BlockofIron(141, "iron_block"),
    #simple Bricks(142, "bricks"),
    #simple MossyCobblestone(145, "mossy_cobblestone"),
    #simple Obsidian(146, "obsidian"),
    #simple DiamondOre(155, "diamond_ore"),
    #simple DeepslateDiamondOre(156, "deepslate_diamond_ore"),
    #simple BlockofDiamond(157, "diamond_block"),
    #simple RedstoneOre(187, "redstone_ore"),
    #simple DeepslateRedstoneOre(188, "deepslate_redstone_ore"),
    #simple Ice(193, "ice"),
    #simple Netherrack(201, "netherrack"),
    #simple Basalt(204, "basalt"),
    #simple PolishedBasalt(205, "polished_basalt"),
    #simple StoneBricks(236, "stone_bricks"),
    #simple MossyStoneBricks(237, "mossy_stone_bricks"),
    #simple CrackedStoneBricks(238, "cracked_stone_bricks"),
    #simple ChiseledStoneBricks(239, "chiseled_stone_bricks"),
    #simple BlockofQuartz(350, "quartz_block"),
    #simple ChiseledQuartzBlock(351, "chiseled_quartz_block"),
    #simple QuartzPillar(352, "quartz_pillar"),
    #simple BlockofCoal(422, "coal_block"),
    #simple RedSandstone(462, "red_sandstone"),
    #simple ChiseledRedSandstone(463, "chiseled_red_sandstone"),
    #simple CutRedSandstone(464, "cut_red_sandstone"),
    #simple SmoothStone(485, "smooth_stone"),
    #simple SmoothSandstone(486, "smooth_sandstone"),
    #simple SmoothQuartzBlock(487, "smooth_quartz"),
    #simple SmoothRedSandstone(488, "smooth_red_sandstone"),
    #simple PurpurBlock(507, "purpur_block"),
    #simple PurpurPillar(508, "purpur_pillar"),
    #simple RedNetherBricks(519, "red_nether_bricks"),
    #simple BrickWall(668, "brick_wall"),
    #simple PrismarineWall(669, "prismarine_wall"),
    #simple RedSandstoneWall(670, "red_sandstone_wall"),
    #simple MossyStoneBrickWall(671, "mossy_stone_brick_wall"),
    #simple GraniteWall(672, "granite_wall"),
    #simple StoneBrickWall(673, "stone_brick_wall"),
    #simple NetherBrickWall(674, "nether_brick_wall"),
    #simple AndesiteWall(675, "andesite_wall"),
    #simple RedNetherBrickWall(676, "red_nether_brick_wall"),
    #simple SandstoneWall(677, "sandstone_wall"),
    #simple EndStoneBrickWall(678, "end_stone_brick_wall"),
    #simple DioriteWall(679, "diorite_wall"),
    #simple BlockofNetherite(748, "netherite_block"),
    #simple AncientDebris(749, "ancient_debris"),
    #simple CryingObsidian(750, "crying_obsidian"),
    #simple ChiseledNetherBricks(774, "chiseled_nether_bricks"),
    #simple CrackedNetherBricks(775, "cracked_nether_bricks"),
    #simple QuartzBricks(776, "quartz_bricks"),
    #simple OxidizedCopper(822, "oxidized_copper"),
    #simple WeatheredCopper(823, "weathered_copper"),
    #simple ExposedCopper(824, "exposed_copper"),
    #simple BlockofCopper(825, "copper_block"),
    #simple CopperOre(826, "copper_ore"),
    #simple DeepslateCopperOre(827, "deepslate_copper_ore"),
    #simple OxidizedCutCopper(828, "oxidized_cut_copper"),
    #simple WeatheredCutCopper(829, "weathered_cut_copper"),
    #simple ExposedCutCopper(830, "exposed_cut_copper"),
    #simple CutCopper(831, "cut_copper"),
    #simple WaxedBlockofCopper(840, "waxed_copper_block"),
    #simple WaxedWeatheredCopper(841, "waxed_weathered_copper"),
    #simple WaxedExposedCopper(842, "waxed_exposed_copper"),
    #simple WaxedOxidizedCopper(843, "waxed_oxidized_copper"),
    #simple WaxedOxidizedCutCopper(844, "waxed_oxidized_cut_copper"),
    #simple WaxedWeatheredCutCopper(845, "waxed_weathered_cut_copper"),
    #simple WaxedExposedCutCopper(846, "waxed_exposed_cut_copper"),
    #simple PointedDripstone(857, "pointed_dripstone"),
    #simple DripstoneBlock(858, "dripstone_block"),
    #simple Deepslate(871, "deepslate"),
    #simple CobbledDeepslate(872, "cobbled_deepslate"),
    #simple DeepslateTiles(880, "deepslate_tiles"),
    #simple DeepslateBricks(884, "deepslate_bricks"),
    #simple CrackedDeepslateBricks(889, "cracked_deepslate_bricks"),
    #simple GrassBlock(8, "grass_block"),
    #simple Dirt(9, "dirt"),
    #simple CoarseDirt(10, "coarse_dirt"),
    #simple Podzol(11, "podzol"),
    #simple SnowBlock(194, "snow_block"),
    
    Air {
        get_id: 0,
        from_id(_id): 0 => {},
        from_names(_name): {
            "air" => {}
        },
        get_name: "air",
    },

    Glass {
        get_id: 262,
        from_id(_id): 262 => {},
        from_names(_name): {
            "glass" => {}
        },
        get_name: "glass",
        transparent: true,
        cube: true,
    },
    Glowstone {
        get_id: 4082,
        from_id(_id): 4082 => {},
        from_names(_name): {
            "glowstone" => {}
        },
        get_name: "glowstone",
        transparent: true,
        cube: true,
    },
    RedstoneWire {
        props: {
            wire: RedstoneWire
        },
        get_id: {
            wire.east.get_id() * 432
                + wire.north.get_id() * 144
                + wire.power as u32 * 9
                + wire.south.get_id() * 3
                + wire.west.get_id()
                + 2114
        },
        from_id_offset: 2114,
        from_id(id): 2114..=3409 => {
            wire: RedstoneWire::new(
                RedstoneWireSide::from_id(id % 432 / 144),
                RedstoneWireSide::from_id(id % 9 / 3),
                RedstoneWireSide::from_id(id / 432),
                RedstoneWireSide::from_id(id % 3),
                (id % 144 / 9) as u8,
            )
        },
        from_names(_name): {
            "redstone_wire" => {
                wire: Default::default()
            }
        },
        get_name: "redstone_wire",
    },
    WallSign {
        props: {
            sign_type: SignType,
            facing: BlockDirection
        },
        get_id: 1 + (sign_type.to_item_type() << 3) as u32 + (facing.get_id() << 1) + match sign_type.0 {
            0..=5 => 3802,
            6..=7 => 15973 - (6 << 3),
            _ => unreachable!(),
        },
        from_id_offset: 0,
        from_id(id): 3802..=3849 | 15973..=15988 => {
            sign_type: SignType::from_item_type(match id {
                3802..=3849 => (id - 3802) >> 3,
                15973..=15988 => ((id - 15973) >> 3) + 6,
                _ => unreachable!(),
            }),
            facing: BlockDirection::from_id((match id {
                3802..=3849 => id - 3802,
                15973..=15988 => id - 15973,
                _ => unreachable!(),
            } & 0b110) >> 1)
        },
        from_names(_name): {
            "oak_wall_sign" => {
                sign_type: SignType(0),
                facing: Default::default()
            },
            "spruce_wall_sign" => {
                sign_type: SignType(1),
                facing: Default::default()
            },
            "birch_wall_sign" => {
                sign_type: SignType(2),
                facing: Default::default()
            },
            "acacia_wall_sign" => {
                sign_type: SignType(3),
                facing: Default::default()
            },
            "jungle_wall_sign" => {
                sign_type: SignType(4),
                facing: Default::default()
            },
            "dark_oak_wall_sign" => {
                sign_type: SignType(5),
                facing: Default::default()
            },
            "crimson_wall_sign" => {
                sign_type: SignType(6),
                facing: Default::default()
            },
            "warped_wall_sign" => {
                sign_type: SignType(7),
                facing: Default::default()
            }
        },
        get_name: match sign_type.0 {
            0 => "oak_wall_sign",
            1 => "spruce_wall_sign",
            2 => "birch_wall_sign",
            3 => "acacia_wall_sign",
            4 => "jungle_wall_sign",
            5 => "dark_oak_wall_sign",
            6 => "crimson_wall_sign",
            7 => "warped_wall_sign",
            _ => "invalid_wall_sign"
        },
    },
    Lever {
        props: {
            lever: Lever
        },
        get_id: {
            (lever.face.get_id() << 3)
                + (lever.facing.get_id() << 1)
                + !lever.powered as u32
                + 3850
        },
        from_id_offset: 3850,
        from_id(id): 3850..=3873 => {
            lever: Lever::new(
                LeverFace::from_id(id >> 3),
                BlockDirection::from_id((id >> 1) & 0b11),
                (id & 1) == 0
            )
        },
        from_names(_name): {
            "lever" => {
                lever: Default::default()
            }
        },
        get_name: "lever",
    },
    StoneButton {
        props: {
            button: StoneButton
        },
        get_id: {
            (button.face.get_id() << 3)
                + (button.facing.get_id() << 1)
                + !button.powered as u32
                + 3966
        },
        from_id_offset: 3966,
        from_id(id): 3966..=3989 => {
            button: StoneButton::new(ButtonFace::from_id(id >> 3), BlockDirection::from_id((id >> 1) & 0b11), (id & 1) == 0)
        },
        from_names(_name): {
            "stone_button" => {
                button: Default::default()
            }
        },
        get_name: "stone_button",
    },
    Sign {
        props: {
            sign_type: SignType,
            rotation: u8
        },
        get_id: 1 + ((sign_type.to_item_type() << 5) as u32) + (rotation << 1) as u32 + match sign_type.0 {
            0..=5 => 3438,
            6..=7 => 15909 - (6 << 5),
            _ => unreachable!(),
        },
        from_id_offset: 0,
        from_id(id): 3438..=3629 | 15909..=15972 => {
            sign_type: SignType::from_item_type(match id {
                3438..=3629 => (id - 3438) >> 5,
                15909..=15972 => ((id - 15909) >> 5) + 6,
                _ => unreachable!(),
            }),
            rotation: ((match id {
                3438..=3629 => id - 3438,
                15909..=15972 => id - 15909,
                _ => unreachable!(),
            } & 0b11110) >> 1) as u8
        },
        from_names(_name): {
            "oak_sign" => {
                sign_type: SignType(0),
                rotation: 0
            },
            "spruce_sign" => {
                sign_type: SignType(1),
                rotation: 0
            },
            "birch_sign" => {
                sign_type: SignType(2),
                rotation: 0
            },
            "acacia_sign" => {
                sign_type: SignType(3),
                rotation: 0
            },
            "jungle_sign" => {
                sign_type: SignType(4),
                rotation: 0
            },
            "dark_oak_sign" => {
                sign_type: SignType(5),
                rotation: 0
            },
            "crimson_sign" => {
                sign_type: SignType(6),
                rotation: 0
            },
            "warped_sign" => {
                sign_type: SignType(7),
                rotation: 0
            }
        },
        get_name: match sign_type.0 {
            0 => "oak_sign",
            1 => "spruce_sign",
            2 => "birch_sign",
            3 => "acacia_sign",
            4 => "jungle_sign",
            5 => "dark_oak_sign",
            6 => "crimson_sign",
            7 => "warped_sign",
            _ => "invalid_sign"
        },
    },
    RedstoneTorch {
        props: {
            lit: bool
        },
        get_id: if lit {
            3956
        } else {
            3957
        },
        from_id_offset: 3956,
        from_id(id): 3956..=3957 => {
            lit: id == 0
        },
        from_names(_name): {
            "redstone_torch" => {
                lit: true
            }
        },
        get_name: "redstone_torch",
    },
    RedstoneWallTorch {
        props: {
            lit: bool,
            facing: BlockDirection
        },
        get_id: (facing.get_id() << 1) + (!lit as u32) + 3958,
        from_id_offset: 3958,
        from_id(id): 3958..=3965 => {
            lit: (id & 1) == 0,
            facing: BlockDirection::from_id(id >> 1)
        },
        from_names(_name): {
            "redstone_wall_torch" => {
                lit: true,
                facing: Default::default()
            }
        },
        get_name: "redstone_wall_torch",
    },
    RedstoneRepeater {
        props: {
            repeater: RedstoneRepeater
        },
        get_id: {
            (repeater.delay as u32 - 1) * 16
                + repeater.facing.get_id() * 4
                + !repeater.locked as u32 * 2
                + !repeater.powered as u32
                + 4100
        },
        from_id_offset: 4100,
        from_id(id): 4100..=4163 => {
            repeater: RedstoneRepeater::new(
                (id >> 4) as u8 + 1,
                BlockDirection::from_id((id >> 2) & 3),
                ((id >> 1) & 1) == 0,
                (id & 1) == 0
            )
        },
        from_names(_name): {
            "repeater" => {
                repeater: Default::default()
            }
        },
        get_name: "repeater",
    },
    RedstoneLamp {
        props: {
            lit: bool
        },
        get_id: if lit {
            5361
        } else {
            5362
        },
        from_id_offset: 5361,
        from_id(id): 5361..=5362 => {
            lit: id == 0
        },
        from_names(_name): {
            "redstone_lamp" => {
                lit: false
            }
        },
        get_name: "redstone_lamp",
        solid: true,
        cube: true,
    },
    TripwireHook {
        props: {
            direction: BlockDirection
        },
        get_id: match direction {
            BlockDirection::North => 5474,
            BlockDirection::South => 5476,
            BlockDirection::West => 5478,
            BlockDirection::East => 5480,
        },
        from_id_offset: 5474,
        from_id(id): 5474..=5480 => {
            direction: BlockDirection::from_id(id / 2)
        },
        from_names(_name): {
            "tripwire_hook" => {
                direction: Default::default()
            }
        },
        get_name: "tripwire_hook",
    },
    RedstoneComparator {
        props: {
            comparator: RedstoneComparator
        },
        get_id: {
            comparator.facing.get_id() * 4
                + comparator.mode.get_id() * 2
                + !comparator.powered as u32
                + 6884
        },
        from_id_offset: 6884,
        from_id(id): 6884..=6899 => {
            comparator: RedstoneComparator::new(
                BlockDirection::from_id(id >> 2),
                ComparatorMode::from_id((id >> 1) & 1),
                (id & 1) == 0
            )
        },
        from_names(_name): {
            "comparator" => {
                comparator: Default::default()
            }
        },
        get_name: "comparator",
    },
    RedstoneBlock {
        get_id: 6932,
        from_id(_id): 6932 => {},
        from_names(_name): {
            "redstone_block" => {}
        },
        get_name: "redstone_block",
        transparent: true,
        cube: true,
    },

    Observer {
        props: {
            observer: RedstoneObserver
        },
        get_id: if observer.powered {
            (observer.facing.get_id() << 1) + 9510
        } else {
            (observer.facing.get_id() << 1) + 9511
        },
        from_id_offset: 9510,
        from_id(id): 9510..=9521 => {
            observer: RedstoneObserver{
                facing: BlockFacing::try_from_id(id >> 1).unwrap(),
                powered: id & 1 == 0,
            }
        },
        from_names(_name): {
            "observer" => {
                observer: Default::default()
            }
        },
        get_name: "observer",
        transparent: true,
        cube: true,
    },

    //todo maybe in the future add slime blocks
    Piston{
        props: {
            piston: RedstonePiston
        },
        get_id: match (piston.sticky, piston.extended) {
            (true, true) => piston.facing.get_id() + 1385,
            (true, false) => piston.facing.get_id() + 1391,
            (false, true) => piston.facing.get_id() + 1404,
            (false, false) => piston.facing.get_id() + 1410,
        },
        from_id(id): 1385..=1396 | 1404..=1415 => {
            piston: match id {
                1385..=1396 => RedstonePiston{
                    sticky: true,
                    extended: (id - 1385) / 6 == 0,
                    facing: BlockFacing::try_from_id((id - 1385) % 6).unwrap(),
                },
                _ => RedstonePiston{
                    sticky: false,
                    extended: (id - 1404) / 6 == 0,
                    facing: BlockFacing::try_from_id((id - 1404) % 6).unwrap(),
                },
            }
        },
        from_names(_name): {
            "piston" => {
                piston: RedstonePiston{ sticky: false, ..Default::default() }
            },
            "sticky_piston" => {
                piston: RedstonePiston{ sticky: true, ..Default::default() }
            }
        },
        get_name: if piston.sticky { "sticky_piston" } else { "piston" },
        solid: false,
        transparent: true,
        cube: true,
    },
    PistonHead {
        props: {
            head: RedstonePistonHead
        },
        get_id: (head.facing.get_id() * 4) + (head.sticky as u32) + (((!head.short) as u32) * 2) + 1416,
        from_id_offset: 1416,
        from_id(id): 1416..=1439 => {
            head: RedstonePistonHead{
                facing: BlockFacing::try_from_id(id >> 2).unwrap(),
                sticky: id & 1 != 0,
                short: id & 2 == 0,
            }
        },
        from_names(_name): {
            "piston_head" => {
                head: Default::default()
            }
        },
        get_name: "piston_head",
        solid: false,
        transparent: true,
        cube: true,
    },
    MovingPiston {
        props: {
            moving: RedstoneMovingPiston,
        },
        get_id: (moving.facing.get_id() << 1) + (moving.sticky as u32) + 1456,
        from_id_offset: 1456,
        from_id(id): 1456..=1467 => {
            moving: RedstoneMovingPiston{
                facing: BlockFacing::try_from_id(id >> 1).unwrap(),
                sticky: id & 1 != 0,
            }
        },
        from_names(_name): {
            "moving_piston" => {
                moving: Default::default()
            }
        },
        get_name: "moving_piston",
        solid: false,
        transparent: true,
        cube: true,
    },
    SeaPickle {
        props: {
            pickles: u8
        },
        get_id: ((pickles - 1) << 1) as u32 + 9891,
        from_id_offset: 9891,
        from_id(id): 9891..=9897 => {
            pickles: (id >> 1) as u8 + 1
        },
        from_names(_name): {
            "sea_pickle" => {
                pickles: 1
            }
        },
        get_name: "sea_pickle",
    },
    Target {
        get_id: 16014,
        from_id(_id): 16014 => {},
        from_names(_name): {
            "target" => {}
        },
        get_name: "target",
        solid: true,
        cube: true,
    },
    StonePressurePlate {
        props: {
            powered: bool
        },
        get_id: 3874 + !powered as u32,
        from_id_offset: 3874,
        from_id(id): 3874..=3875 => {
            powered: id == 0
        },
        from_names(_name): {
            "stone_pressure_plate" => {
                powered: false
            }
        },
        get_name: "stone_pressure_plate",
    },
    Cake {
        props: {
            bites: u8
        },
        get_id: 4093 + bites as u32,
        from_id_offset: 4093,
        from_id(id): 4093..=4099 => {
            bites: id as u8
        },
        from_names(_name): {
            "cake" => {
                bites: 0
            }
        },
        get_name: "cake",
    },
    Barrel {
        get_id: 15042,
        from_id(_id): 15042 => {},
        from_names(_name): {
            "barrel" => {}
        },
        get_name: "barrel",
        solid: true,
        cube: true,
    },
    Hopper {
        get_id: 6939,
        from_id(_id): 6939 => {},
        from_names(_name): {
            "hopper" => {}
        },
        get_name: "hopper",
        transparent: true,
        cube: true,
    },
    Sandstone {
        get_id: 278,
        from_id(_id): 278 => {},
        from_names(_name): {
            "sandstone" => {}
        },
        get_name: "sandstone",
        solid: true,
        cube: true,
    },
    StoneBrick {
        get_id: 4564,
        from_id(_id): 4564 => {},
        from_names(_name): {
            "stone_bricks" => {}
        },
        get_name: "stone_bricks",
        solid: true,
        cube: true,
    },
    CoalBlock {
        get_id: 8133,
        from_id(_id): 8133 => {},
        from_names(_name): {
            "coal_block" => {}
        },
        get_name: "coal_block",
        solid: true,
        cube: true,
    },
    Furnace {
        get_id: 3431,
        from_id(_id): 3431 => {},
        from_names(_name): {
            "furnace" => {}
        },
        get_name: "furnace",
        solid: true,
        cube: true,
    },
    Quartz {
        get_id: 6944,
        from_id(_id): 6944 => {},
        from_names(_name): {
            "quartz_block" => {}
        },
        get_name: "quartz_block",
        solid: true,
        cube: true,
    },
    SmoothQuartz {
        get_id: 8666,
        from_id(_id): 8666 => {},
        from_names(_name): {
            "smooth_quartz" => {}
        },
        get_name: "smooth_quartz",
        solid: true,
        cube: true,
    },
    SmoothStoneSlab {
        get_id: 8593,
        from_id(_id): 8593 => {},
        from_names(_name): {
            "smooth_stone_slab" => {}
        },
        get_name: "smooth_stone_slab[type=top]",
        transparent: true,
        cube: true,
    },
    QuartzSlab {
        get_id: 8641,
        from_id(_id): 8641 => {},
        from_names(_name): {
            "quartz_slab" => {}
        },
        get_name: "quartz_slab",
        transparent: true,
        cube: true,
    },
    Cauldron {
        props: {
            level: u8
        },
        get_id: level as u32 + 5342,
        from_id_offset: 5342,
        from_id(id): 5342..=5345 => {
            level: id as u8
        },
        from_names(_name): {
            "cauldron" => {
                level: 0
            },
            "water_cauldron" => {
                level: 3
            }
        },
        get_name: match level {
            0 => "cauldron",
            _ => "water_cauldron"
        },
        transparent: true,
        cube: false,
    },
    Composter {
        props: {
            level: u8
        },
        get_id: level as u32 + 16005,
        from_id_offset: 16005,
        from_id(id): 16005..=16013 => {
            level: id as u8
        },
        from_names(_name): {
            "composter" => {
                level: 0
            }
        },
        get_name: "composter",
        transparent: true,
        // FIXME: You can place repeaters and comparators on it, but not wires?
        cube: true,
    },
    Concrete {
        props: {
            color: BlockColorVariant
        },
        get_id: color.get_id() + 9688,
        from_id_offset: 9688,
        from_id(id): 9688..=9703 => {
            color: BlockColorVariant::from_id(id)
        },
        from_names(_name): {
            "white_concrete" => { color: BlockColorVariant::White },
            "orange_concrete" => { color: BlockColorVariant::Orange },
            "magenta_concrete" => { color: BlockColorVariant::Magenta },
            "light_blue_concrete" => { color: BlockColorVariant::LightBlue },
            "yellow_concrete" => { color: BlockColorVariant::Yellow },
            "lime_concrete" => { color: BlockColorVariant::Lime },
            "pink_concrete" => { color: BlockColorVariant::Pink },
            "gray_concrete" => { color: BlockColorVariant::Gray },
            "light_gray_concrete" => { color: BlockColorVariant::LightGray },
            "cyan_concrete" => { color: BlockColorVariant::Cyan },
            "purple_concrete" => { color: BlockColorVariant::Purple },
            "blue_concrete" => { color: BlockColorVariant::Blue },
            "brown_concrete" => { color: BlockColorVariant::Brown },
            "green_concrete" => { color: BlockColorVariant::Green },
            "red_concrete" => { color: BlockColorVariant::Red },
            "black_concrete" => { color: BlockColorVariant::Black }
        },
        get_name: match color {
            BlockColorVariant::White => "white_concrete",
            BlockColorVariant::Orange => "orange_concrete",
            BlockColorVariant::Magenta => "magenta_concrete",
            BlockColorVariant::LightBlue => "light_blue_concrete",
            BlockColorVariant::Yellow => "yellow_concrete",
            BlockColorVariant::Lime => "lime_concrete",
            BlockColorVariant::Pink => "pink_concrete",
            BlockColorVariant::Gray => "gray_concrete",
            BlockColorVariant::LightGray => "light_gray_concrete",
            BlockColorVariant::Cyan => "cyan_concrete",
            BlockColorVariant::Purple => "purple_concrete",
            BlockColorVariant::Blue => "blue_concrete",
            BlockColorVariant::Brown => "brown_concrete",
            BlockColorVariant::Green => "green_concrete",
            BlockColorVariant::Red => "red_concrete",
            BlockColorVariant::Black => "black_concrete",
        },
        solid: true,
        cube: true,
    },
    StainedGlass {
        props: {
            color: BlockColorVariant
        },
        get_id: color.get_id() + 4164,
        from_id_offset: 4164,
        from_id(id): 4164..=4179 => {
            color: BlockColorVariant::from_id(id)
        },
        from_names(_name): {
            "white_stained_glass" => { color: BlockColorVariant::White },
            "orange_stained_glass" => { color: BlockColorVariant::Orange },
            "magenta_stained_glass" => { color: BlockColorVariant::Magenta },
            "light_blue_stained_glass" => { color: BlockColorVariant::LightBlue },
            "yellow_stained_glass" => { color: BlockColorVariant::Yellow },
            "lime_stained_glass" => { color: BlockColorVariant::Lime },
            "pink_stained_glass" => { color: BlockColorVariant::Pink },
            "gray_stained_glass" => { color: BlockColorVariant::Gray },
            "light_gray_stained_glass" => { color: BlockColorVariant::LightGray },
            "cyan_stained_glass" => { color: BlockColorVariant::Cyan },
            "purple_stained_glass" => { color: BlockColorVariant::Purple },
            "blue_stained_glass" => { color: BlockColorVariant::Blue },
            "brown_stained_glass" => { color: BlockColorVariant::Brown },
            "green_stained_glass" => { color: BlockColorVariant::Green },
            "red_stained_glass" => { color: BlockColorVariant::Red },
            "black_stained_glass" => { color: BlockColorVariant::Black }
        },
        get_name: match color {
            BlockColorVariant::White => "white_stained_glass",
            BlockColorVariant::Orange => "orange_stained_glass",
            BlockColorVariant::Magenta => "magenta_stained_glass",
            BlockColorVariant::LightBlue => "light_blue_stained_glass",
            BlockColorVariant::Yellow => "yellow_stained_glass",
            BlockColorVariant::Lime => "lime_stained_glass",
            BlockColorVariant::Pink => "pink_stained_glass",
            BlockColorVariant::Gray => "gray_stained_glass",
            BlockColorVariant::LightGray => "light_gray_stained_glass",
            BlockColorVariant::Cyan => "cyan_stained_glass",
            BlockColorVariant::Purple => "purple_stained_glass",
            BlockColorVariant::Blue => "blue_stained_glass",
            BlockColorVariant::Brown => "brown_stained_glass",
            BlockColorVariant::Green => "green_stained_glass",
            BlockColorVariant::Red => "red_stained_glass",
            BlockColorVariant::Black => "black_stained_glass",
        },
        transparent: true,
        cube: true,
    },
    Terracotta {
        get_id: 8132,
        from_id(_id): 8132 => {},
        from_names(_name): {
            "terracotta" => {}
        },
        get_name: "terracotta",
        solid: true,
        cube: true,
    },
    ColoredTerracotta {
        props: {
            color: BlockColorVariant
        },
        get_id: color.get_id() + 7065,
        from_id_offset: 7065,
        from_id(id): 7065..=7080 => {
            color: BlockColorVariant::from_id(id)
        },
        from_names(_name): {
            "white_terracotta" => { color: BlockColorVariant::White },
            "orange_terracotta" => { color: BlockColorVariant::Orange },
            "magenta_terracotta" => { color: BlockColorVariant::Magenta },
            "light_blue_terracotta" => { color: BlockColorVariant::LightBlue },
            "yellow_terracotta" => { color: BlockColorVariant::Yellow },
            "lime_terracotta" => { color: BlockColorVariant::Lime },
            "pink_terracotta" => { color: BlockColorVariant::Pink },
            "gray_terracotta" => { color: BlockColorVariant::Gray },
            "light_gray_terracotta" => { color: BlockColorVariant::LightGray },
            "cyan_terracotta" => { color: BlockColorVariant::Cyan },
            "purple_terracotta" => { color: BlockColorVariant::Purple },
            "blue_terracotta" => { color: BlockColorVariant::Blue },
            "brown_terracotta" => { color: BlockColorVariant::Brown },
            "green_terracotta" => { color: BlockColorVariant::Green },
            "red_terracotta" => { color: BlockColorVariant::Red },
            "black_terracotta" => { color: BlockColorVariant::Black }
        },
        get_name: match color {
            BlockColorVariant::White => "white_terracotta",
            BlockColorVariant::Orange => "orange_terracotta",
            BlockColorVariant::Magenta => "magenta_terracotta",
            BlockColorVariant::LightBlue => "light_blue_terracotta",
            BlockColorVariant::Yellow => "yellow_terracotta",
            BlockColorVariant::Lime => "lime_terracotta",
            BlockColorVariant::Pink => "pink_terracotta",
            BlockColorVariant::Gray => "gray_terracotta",
            BlockColorVariant::LightGray => "light_gray_terracotta",
            BlockColorVariant::Cyan => "cyan_terracotta",
            BlockColorVariant::Purple => "purple_terracotta",
            BlockColorVariant::Blue => "blue_terracotta",
            BlockColorVariant::Brown => "brown_terracotta",
            BlockColorVariant::Green => "green_terracotta",
            BlockColorVariant::Red => "red_terracotta",
            BlockColorVariant::Black => "black_terracotta",
        },
        solid: true,
        cube: true,
    },
    Wool {
        props: {
            color: BlockColorVariant
        },
        get_id: color.get_id() + 1440,
        from_id_offset: 1440,
        from_id(id): 1440..=1455 => {
            color: BlockColorVariant::from_id(id)
        },
        from_names(_name): {
            "white_wool" => { color: BlockColorVariant::White },
            "orange_wool" => { color: BlockColorVariant::Orange },
            "magenta_wool" => { color: BlockColorVariant::Magenta },
            "light_blue_wool" => { color: BlockColorVariant::LightBlue },
            "yellow_wool" => { color: BlockColorVariant::Yellow },
            "lime_wool" => { color: BlockColorVariant::Lime },
            "pink_wool" => { color: BlockColorVariant::Pink },
            "gray_wool" => { color: BlockColorVariant::Gray },
            "light_gray_wool" => { color: BlockColorVariant::LightGray },
            "cyan_wool" => { color: BlockColorVariant::Cyan },
            "purple_wool" => { color: BlockColorVariant::Purple },
            "blue_wool" => { color: BlockColorVariant::Blue },
            "brown_wool" => { color: BlockColorVariant::Brown },
            "green_wool" => { color: BlockColorVariant::Green },
            "red_wool" => { color: BlockColorVariant::Red },
            "black_wool" => { color: BlockColorVariant::Black }
        },
        get_name: match color {
            BlockColorVariant::White => "white_wool",
            BlockColorVariant::Orange => "orange_wool",
            BlockColorVariant::Magenta => "magenta_wool",
            BlockColorVariant::LightBlue => "light_blue_wool",
            BlockColorVariant::Yellow => "yellow_wool",
            BlockColorVariant::Lime => "lime_wool",
            BlockColorVariant::Pink => "pink_wool",
            BlockColorVariant::Gray => "gray_wool",
            BlockColorVariant::LightGray => "light_gray_wool",
            BlockColorVariant::Cyan => "cyan_wool",
            BlockColorVariant::Purple => "purple_wool",
            BlockColorVariant::Blue => "blue_wool",
            BlockColorVariant::Brown => "brown_wool",
            BlockColorVariant::Green => "green_wool",
            BlockColorVariant::Red => "red_wool",
            BlockColorVariant::Black => "black_wool",
        },
        solid: true,
        cube: true,
    },
    IronTrapdoor {
        props: {
            facing: BlockDirection,
            half: TrapdoorHalf,
            powered: bool
        },
        get_id: {
            facing.get_id() * 16
                + half.get_id() * 8
                + !powered as u32 * 6
                + 7788
        },
        from_id_offset: 7788,
        from_id(id): 7788..=7850 => {
            facing: BlockDirection::from_id(id >> 4),
            half: TrapdoorHalf::from_id((id >> 3) & 1),
            powered: ((id >> 1) & 1) == 0
        },
        from_names(_name): {
            "iron_trapdoor" => {
                facing: Default::default(),
                half: TrapdoorHalf::Bottom,
                powered: false
            }
        },
        get_name: "iron_trapdoor",
    },
    NoteBlock {
        props: {
            instrument: Instrument,
            note: u32,
            powered: bool
        },
        get_id: {
            instrument.get_id() * 50
                + note * 2
                + !powered as u32
                + 281
        },
        from_id_offset: 281,
        from_id(id): 281..=1080 => {
            instrument: Instrument::from_id((id >> 1) / 25),
            note: (id >> 1) % 25,
            powered: (id & 1) == 0
        },
        from_names(_name): {
            "note_block" => {
                instrument: Instrument::Harp,
                note: 0,
                powered: false
            }
        },
        get_name: "note_block",
        solid: true,
        cube: true,
    },
    Clay {
        get_id: 4016,
        from_id(_id): 4016 => {},
        from_names(_name): {
            "clay" => {}
        },
        get_name: "clay",
        solid: true,
        cube: true,
    },
    GoldBlock {
        get_id: 1483,
        from_id(_id): 1483 => {},
        from_names(_name): {
            "gold_block" => {}
        },
        get_name: "gold_block",
        solid: true,
        cube: true,
    },
    PackedIce {
        get_id: 8134,
        from_id(_id): 8134 => {},
        from_names(_name): {
            "packed_ice" => {}
        },
        get_name: "packed_ice",
        solid: true,
        cube: true,
    },
    BoneBlock {
        get_id: 9507,
        from_id(_id): 9506..=9508 => {},
        from_names(_name): {
            "bone_block" => {}
        },
        get_name: "bone_block",
        solid: true,
        cube: true,
    },
    IronBlock {
        get_id: 1484,
        from_id(_id): 1484 => {},
        from_names(_name): {
            "iron_block" => {}
        },
        get_name: "iron_block",
        solid: true,
        cube: true,
    },
    SoulSand {
        get_id: 4069,
        from_id(_id): 4069 => {},
        from_names(_name): {
            "soul_sand" => {}
        },
        get_name: "soul_sand",
        solid: true,
        cube: true,
    },
    Pumpkin {
        get_id: 4067,
        from_id(_id): 4067 => {},
        from_names(_name): {
            "pumpkin" => {}
        },
        get_name: "pumpkin",
        solid: true,
        cube: true,
    },
    EmeraldBlock {
        get_id: 5609,
        from_id(_id): 5609 => {},
        from_names(_name): {
            "emerald_block" => {}
        },
        get_name: "emerald_block",
        solid: true,
        cube: true,
    },
    HayBlock {
        get_id: 8114,
        from_id(_id): 8113..=8115 => {},
        from_names(_name): {
            "hay_block" => {}
        },
        get_name: "hay_block",
        solid: true,
        cube: true,
    },
    Sand {
        get_id: 66,
        from_id(_id): 66 => {},
        from_names(_name): {
            "sand" => {}
        },
        get_name: "sand",
        solid: true,
        cube: true,
    },
    Unknown {
        props: {
            id: u32
        },
        get_id: id,
        from_id(id): _ => { id: id },
        from_names(name): {},
        get_name: "unknown",
        solid: true,
        cube: true,
    }
}

// TODO make macro for building blocks
