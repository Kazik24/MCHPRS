use crate::block_entities::{ContainerType, SignalStrength};
use crate::{BlockColorVariant, SignType};
use mchprs_utils::map;

#[derive(Clone, Debug)]
pub struct ItemStack {
    pub item_type: Item,
    pub count: u8,
    pub nbt: Option<nbt::Blob>,
}

impl ItemStack {
    /// Create container item with specified signal strength
    pub fn container_with_ss(container_ty: ContainerType, signal: SignalStrength) -> ItemStack {
        let ss = signal.value();
        let item = match container_ty {
            ContainerType::Barrel => Item::Barrel {},
            ContainerType::Hopper => Item::Hopper {},
            ContainerType::Furnace => Item::Furnace {},
            ContainerType::Chest => Item::from_name("chest").expect("generated chest item"),
        };
        let slots = container_ty.num_slots() as u32;

        let items_needed = match ss {
            0 => 0,
            15 => slots * 64,
            _ => ((32 * slots * ss as u32) as f32 / 7.0 - 1.0).ceil() as u32,
        } as usize;

        let nbt = match items_needed {
            0 => None,
            _ => Some({
                let list = nbt::Value::List({
                    let mut items = Vec::new();
                    for (slot, items_added) in (0..items_needed).step_by(64).enumerate() {
                        let count = (items_needed - items_added).min(64);
                        items.push(nbt::Value::Compound(map! {
                            "Count" => nbt::Value::Byte(count as i8),
                            "id" => nbt::Value::String("minecraft:redstone".to_owned()),
                            "Slot" => nbt::Value::Byte(slot as i8)
                        }));
                    }
                    items
                });

                nbt::Blob::with_content(map! {
                    "BlockEntityTag" => nbt::Value::Compound(map! {
                        "Items" => list,
                        "Id" => nbt::Value::String(container_ty.to_string())
                    })
                })
            }),
        };

        ItemStack {
            item_type: item,
            count: 1,
            nbt,
        }
    }
}

macro_rules! items {
    (
        $(
            $name:ident {
                $(props: {
                    $(
                        $prop_name:ident : $prop_type:ident
                    ),*
                },)?
                get_id: $get_id:expr,
                $( from_id_offset: $get_id_offset:literal, )?
                from_id($id_name:ident): $from_id_pat:pat => {
                    $(
                        $from_id_pkey:ident: $from_id_pval:expr
                    ),*
                },
                $( max_stack: $max_stack:literal, )?
                $( block: $block:literal, )?
            }
        ),*
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Item {
            $(
                $name $({
                    $(
                        $prop_name: $prop_type,
                    )*
                })?
            ),*
        }

        #[allow(clippy::redundant_field_names)]
        impl Item {
            fn legacy_id(self) -> u32 {
                match self {
                    $(
                        Item::$name {
                            $($(
                                $prop_name,
                            )*)?
                        } => $get_id,
                    )*
                }
            }

            fn from_legacy_id(mut id: u32) -> Item {
                match id {
                    $(
                        $from_id_pat => {
                            $( id -= $get_id_offset; )?
                            let $id_name = id;
                            Item::$name {
                                $(
                                    $from_id_pkey: $from_id_pval
                                ),*
                            }
                        },
                    )*
                }
            }

            pub fn is_block(self) -> bool {
                match self {
                    $(
                        $( Item::$name { .. } => $block, )?
                    )*
                    _ => crate::generated::BLOCKS.iter().any(|b|b.0 == self.get_name())
                }
            }

            pub fn max_stack_size(self)->u32 {
                crate::generated::ITEMS.get(self.get_id() as usize).map_or(64,|i|i.1)
            }
        }
    }
}

// list of ids: https://github.com/PrismarineJS/minecraft-data/blob/master/data/pc/1.18/items.json
items! {
    Compass {
        get_id: 795,
        from_id(_id): 795 => {},
    },
    // Wooden Axe
    WEWand {
        get_id: 702,
        from_id(_id): 702 => {},
    },
    Snowball {
        get_id: 780,
        from_id(_id): 780 => {},
        max_stack: 16,
    },
    TotemOfUndying {
        get_id: 1010,
        from_id(_id): 1010 => {},
        max_stack: 1,
    },
    MilkBucket {
        get_id: 782,
        from_id(_id): 782 => {},
        max_stack: 1,
    },
    Stone {
        get_id: 1,
        from_id(_id): 1 => {},
        block: true,
    },
    Redstone {
        get_id: 585,
        from_id(_id): 585 => {},
        block: true,
    },
    Glass {
        get_id: 143,
        from_id(_id): 143 => {},
        block: true,
    },
    Sandstone {
        get_id: 146,
        from_id(_id): 146 => {},
        block: true,
    },
    SeaPickle {
        get_id: 156,
        from_id(_id): 156 => {},
        block: true,
    },
    Wool {
        props: {
            color: BlockColorVariant
        },
        get_id: 157 + color.get_id(),
        from_id_offset: 157,
        from_id(id): 157..=172 => {
            color: BlockColorVariant::from_id(id)
        },
        block: true,
    },
    Furnace {
        get_id: 248,
        from_id(_id): 248 => {},
        block: true,
    },
    Lever {
        get_id: 600,
        from_id(_id): 600 => {},
        block: true,
    },
    StonePressurePlate {
        get_id: 190,
        from_id(_id): 190 => {},
        block: true,
    },
    RedstoneTorch {
        get_id: 586,
        from_id(_id): 586 => {},
        block: true,
    },
    StoneButton {
        get_id: 609,
        from_id(_id): 609 => {},
        block: true,
    },
    RedstoneLamp {
        get_id: 607,
        from_id(_id): 607 => {},
        block: true,
    },
    RedstoneBlock {
        get_id: 587,
        from_id(_id): 587 => {},
        block: true,
    },
    Hopper {
        get_id: 595,
        from_id(_id): 595 => {},
        block: true,
    },
    TripwireHook {
        get_id: 604,
        from_id(_id): 604 => {},
        block: true,
    },
    Terracotta {
        get_id: 389,
        from_id(_id): 389 => {},
        block: true,
    },
    ColoredTerracotta {
        props: {
            color: BlockColorVariant
        },
        get_id: 354 + color.get_id(),
        from_id_offset: 354,
        from_id(id): 354..=369 => {
            color: BlockColorVariant::from_id(id)
        },
        block: true,
    },
    Concrete {
        props: {
            color: BlockColorVariant
        },
        get_id: 484 + color.get_id(),
        from_id_offset: 484,
        from_id(id): 484..=499 => {
            color: BlockColorVariant::from_id(id)
        },
        block: true,
    },
    StainedGlass {
        props: {
            color: BlockColorVariant
        },
        get_id: 400 + color.get_id(),
        from_id_offset: 400,
        from_id(id): 400..=415 => {
            color: BlockColorVariant::from_id(id)
        },
        block: true,
    },
    Repeater {
        get_id: 588,
        from_id(_id): 588 => {},
        block: true,
    },
    Comparator {
        get_id: 589,
        from_id(_id): 589 => {},
        block: true,
    },
    Sign {
        props: {
            sign_type: SignType
        },
        get_id: 768 + sign_type.to_item_type(),
        from_id_offset: 768,
        from_id(id): 768..=775 => {
            sign_type: SignType::from_item_type(id)
        },
        block: true,
    },
    Barrel {
        get_id: 1043,
        from_id(_id): 1043 => {},
        block: true,
    },
    Target {
        get_id: 599,
        from_id(_id): 599 => {},
        block: true,
    },
    SmoothStoneSlab {
        get_id: 213,
        from_id(_id): 213 => {},
        block: true,
    },
    QuartzSlab {
        get_id: 221,
        from_id(_id): 221 => {},
        block: true,
    },
    IronTrapdoor {
        get_id: 640,
        from_id(_id): 640 => {},
        block: true,
    },
    Observer {
        get_id: 594,
        from_id(_id): 594 => {},
        block: true,
    },
    Piston {
        props: {
            sticky: bool
        },
        get_id: if sticky { 591 } else { 590 },
        from_id_offset: 590,
        from_id(id): 590..=591 => {
            sticky: id != 0
        },
        block: true,
    },
    Stick {
        get_id: 729,
        from_id(_id): 729 => {},
    },
    NoteBlock {
        get_id: 608,
        from_id(_id): 608 => {},
        block: true,
    },
    Clay {
        get_id: 255,
        from_id(_id): 255 => {},
        block: true,
    },
    GoldBlock {
        get_id: 67,
        from_id(_id): 67 => {},
        block: true,
    },
    PackedIce {
        get_id: 390,
        from_id(_id): 390 => {},
        block: true,
    },
    BoneBlock {
        get_id: 449,
        from_id(_id): 449 => {},
        block: true,
    },
    IronBlock {
        get_id: 65,
        from_id(_id): 65 => {},
        block: true,
    },
    SoulSand {
        get_id: 269,
        from_id(_id): 269 => {},
        block: true,
    },
    Pumpkin {
        get_id: 265,
        from_id(_id): 265 => {},
        block: true,
    },
    EmeraldBlock {
        get_id: 317,
        from_id(_id): 317 => {},
        block: true,
    },
    HayBlock {
        get_id: 372,
        from_id(_id): 372 => {},
        block: true,
    },
    Sand {
        get_id: 37,
        from_id(_id): 37 => {},
        block: true,
    },

    Bedrock { get_id: 36, from_id(_id): 36 => {}, block: true, },
    TNT { get_id: 606, from_id(_id): 606 => {}, block: true, },
    OakPlanks { get_id: 22, from_id(_id): 22 => {}, block: true, },
    SprucePlanks { get_id: 23, from_id(_id): 23 => {}, block: true, },
    BirchPlanks { get_id: 24, from_id(_id): 24 => {}, block: true, },
    JunglePlanks { get_id: 25, from_id(_id): 25 => {}, block: true, },
    AcaciaPlanks { get_id: 26, from_id(_id): 26 => {}, block: true, },
    DarkOakPlanks { get_id: 27, from_id(_id): 27 => {}, block: true, },
    OakLog { get_id: 101, from_id(_id): 101 => {}, block: true, },
    SpruceLog { get_id: 102, from_id(_id): 102 => {}, block: true, },
    BirchLog { get_id: 103, from_id(_id): 103 => {}, block: true, },
    JungleLog { get_id: 104, from_id(_id): 104 => {}, block: true, },
    AcaciaLog { get_id: 105, from_id(_id): 105 => {}, block: true, },
    DarkOakLog { get_id: 106, from_id(_id): 106 => {}, block: true, },
    StrippedSpruceLog { get_id: 110, from_id(_id): 110 => {}, block: true, },
    StrippedBirchLog { get_id: 111, from_id(_id): 111 => {}, block: true, },
    StrippedJungleLog { get_id: 112, from_id(_id): 112 => {}, block: true, },
    StrippedAcaciaLog { get_id: 113, from_id(_id): 113 => {}, block: true, },
    StrippedDarkOakLog { get_id: 114, from_id(_id): 114 => {}, block: true, },
    StrippedOakLog { get_id: 109, from_id(_id): 109 => {}, block: true, },
    OakWood { get_id: 125, from_id(_id): 125 => {}, block: true, },
    SpruceWood { get_id: 126, from_id(_id): 126 => {}, block: true, },
    BirchWood { get_id: 127, from_id(_id): 127 => {}, block: true, },
    JungleWood { get_id: 128, from_id(_id): 128 => {}, block: true, },
    AcaciaWood { get_id: 129, from_id(_id): 129 => {}, block: true, },
    DarkOakWood { get_id: 130, from_id(_id): 130 => {}, block: true, },
    StrippedOakWood { get_id: 117, from_id(_id): 117 => {}, block: true, },
    StrippedSpruceWood { get_id: 118, from_id(_id): 118 => {}, block: true, },
    StrippedBirchWood { get_id: 119, from_id(_id): 119 => {}, block: true, },
    StrippedJungleWood { get_id: 120, from_id(_id): 120 => {}, block: true, },
    StrippedAcaciaWood { get_id: 121, from_id(_id): 121 => {}, block: true, },
    StrippedDarkOakWood { get_id: 122, from_id(_id): 122 => {}, block: true, },
    Bookshelf { get_id: 233, from_id(_id): 233 => {}, block: true, },
    Sponge { get_id: 141, from_id(_id): 141 => {}, block: true, },
    MossBlock { get_id: 199, from_id(_id): 199 => {}, block: true, },
    Granite { get_id: 2, from_id(_id): 2 => {}, block: true, },
    PolishedGranite { get_id: 3, from_id(_id): 3 => {}, block: true, },
    Diorite { get_id: 4, from_id(_id): 4 => {}, block: true, },
    PolishedDiorite { get_id: 5, from_id(_id): 5 => {}, block: true, },
    Andesite { get_id: 6, from_id(_id): 6 => {}, block: true, },
    PolishedAndesite { get_id: 7, from_id(_id): 7 => {}, block: true, },
    Cobblestone { get_id: 21, from_id(_id): 21 => {}, block: true, },
    GoldOre { get_id: 46, from_id(_id): 46 => {}, block: true, },
    DeepslateGoldOre { get_id: 47, from_id(_id): 47 => {}, block: true, },
    IronOre { get_id: 42, from_id(_id): 42 => {}, block: true, },
    DeepslateIronOre { get_id: 43, from_id(_id): 43 => {}, block: true, },
    CoalOre { get_id: 40, from_id(_id): 40 => {}, block: true, },
    DeepslateCoalOre { get_id: 41, from_id(_id): 41 => {}, block: true, },
    NetherGoldOre { get_id: 56, from_id(_id): 56 => {}, block: true, },
    LapisLazuliOre { get_id: 52, from_id(_id): 52 => {}, block: true, },
    DeepslateLapisLazuliOre { get_id: 53, from_id(_id): 53 => {}, block: true, },
    BlockofLapisLazuli { get_id: 145, from_id(_id): 145 => {}, block: true, },
    ChiseledSandstone { get_id: 147, from_id(_id): 147 => {}, block: true, },
    CutSandstone { get_id: 148, from_id(_id): 148 => {}, block: true, },
    Bricks { get_id: 232, from_id(_id): 232 => {}, block: true, },
    MossyCobblestone { get_id: 234, from_id(_id): 234 => {}, block: true, },
    Obsidian { get_id: 235, from_id(_id): 235 => {}, block: true, },
    DiamondOre { get_id: 54, from_id(_id): 54 => {}, block: true, },
    DeepslateDiamondOre { get_id: 55, from_id(_id): 55 => {}, block: true, },
    BlockofDiamond { get_id: 68, from_id(_id): 68 => {}, block: true, },
    RedstoneOre { get_id: 48, from_id(_id): 48 => {}, block: true, },
    DeepslateRedstoneOre { get_id: 49, from_id(_id): 49 => {}, block: true, },
    Ice { get_id: 252, from_id(_id): 252 => {}, block: true, },
    Netherrack { get_id: 268, from_id(_id): 268 => {}, block: true, },
    Basalt { get_id: 271, from_id(_id): 271 => {}, block: true, },
    PolishedBasalt { get_id: 272, from_id(_id): 272 => {}, block: true, },
    StoneBricks { get_id: 283, from_id(_id): 283 => {}, block: true, },
    MossyStoneBricks { get_id: 284, from_id(_id): 284 => {}, block: true, },
    CrackedStoneBricks { get_id: 285, from_id(_id): 285 => {}, block: true, },
    ChiseledStoneBricks { get_id: 286, from_id(_id): 286 => {}, block: true, },
    BlockofQuartz { get_id: 350, from_id(_id): 350 => {}, block: true, },
    ChiseledQuartzBlock { get_id: 349, from_id(_id): 349 => {}, block: true, },
    QuartzPillar { get_id: 352, from_id(_id): 352 => {}, block: true, },
    BlockofCoal { get_id: 59, from_id(_id): 59 => {}, block: true, },
    RedSandstone { get_id: 439, from_id(_id): 439 => {}, block: true, },
    ChiseledRedSandstone { get_id: 440, from_id(_id): 440 => {}, block: true, },
    CutRedSandstone { get_id: 441, from_id(_id): 441 => {}, block: true, },
    SmoothStone { get_id: 231, from_id(_id): 231 => {}, block: true, },
    SmoothSandstone { get_id: 230, from_id(_id): 230 => {}, block: true, },
    SmoothQuartzBlock { get_id: 228, from_id(_id): 228 => {}, block: true, },
    SmoothRedSandstone { get_id: 229, from_id(_id): 229 => {}, block: true, },
    PurpurBlock { get_id: 240, from_id(_id): 240 => {}, block: true, },
    PurpurPillar { get_id: 241, from_id(_id): 241 => {}, block: true, },
    RedNetherBricks { get_id: 448, from_id(_id): 448 => {}, block: true, },
    BrickWall { get_id: 327, from_id(_id): 327 => {}, block: true, },
    PrismarineWall { get_id: 328, from_id(_id): 328 => {}, block: true, },
    RedSandstoneWall { get_id: 329, from_id(_id): 329 => {}, block: true, },
    MossyStoneBrickWall { get_id: 330, from_id(_id): 330 => {}, block: true, },
    GraniteWall { get_id: 331, from_id(_id): 331 => {}, block: true, },
    StoneBrickWall { get_id: 332, from_id(_id): 332 => {}, block: true, },
    NetherBrickWall { get_id: 333, from_id(_id): 333 => {}, block: true, },
    AndesiteWall { get_id: 334, from_id(_id): 334 => {}, block: true, },
    RedNetherBrickWall { get_id: 335, from_id(_id): 335 => {}, block: true, },
    SandstoneWall { get_id: 336, from_id(_id): 336 => {}, block: true, },
    EndStoneBrickWall { get_id: 337, from_id(_id): 337 => {}, block: true, },
    DioriteWall { get_id: 338, from_id(_id): 338 => {}, block: true, },
    BlockofNetherite { get_id: 748, from_id(_id): 748 => {}, block: true, },
    AncientDebris { get_id: 58, from_id(_id): 58 => {}, block: true, },
    CryingObsidian { get_id: 1065, from_id(_id): 1065 => {}, block: true, },
    ChiseledNetherBricks { get_id: 307, from_id(_id): 307 => {}, block: true, },
    CrackedNetherBricks { get_id: 306, from_id(_id): 306 => {}, block: true, },
    QuartzBricks { get_id: 351, from_id(_id): 351 => {}, block: true, },
    OxidizedCopper { get_id: 72, from_id(_id): 72 => {}, block: true, },
    WeatheredCopper { get_id: 71, from_id(_id): 71 => {}, block: true, },
    ExposedCopper { get_id: 70, from_id(_id): 70 => {}, block: true, },
    BlockofCopper { get_id: 66, from_id(_id): 66 => {}, block: true, },
    CopperOre { get_id: 44, from_id(_id): 44 => {}, block: true, },
    DeepslateCopperOre { get_id: 45, from_id(_id): 45 => {}, block: true, },
    OxidizedCutCopper { get_id: 76, from_id(_id): 76 => {}, block: true, },
    WeatheredCutCopper { get_id: 75, from_id(_id): 75 => {}, block: true, },
    ExposedCutCopper { get_id: 74, from_id(_id): 74 => {}, block: true, },
    CutCopper { get_id: 73, from_id(_id): 73 => {}, block: true, },
    WaxedBlockofCopper { get_id: 85, from_id(_id): 85 => {}, block: true, },
    WaxedWeatheredCopper { get_id: 87, from_id(_id): 87 => {}, block: true, },
    WaxedExposedCopper { get_id: 86, from_id(_id): 86 => {}, block: true, },
    WaxedOxidizedCopper { get_id: 88, from_id(_id): 88 => {}, block: true, },
    WaxedOxidizedCutCopper { get_id: 92, from_id(_id): 92 => {}, block: true, },
    WaxedWeatheredCutCopper { get_id: 91, from_id(_id): 91 => {}, block: true, },
    WaxedExposedCutCopper { get_id: 90, from_id(_id): 90 => {}, block: true, },
    PointedDripstone { get_id: 1100, from_id(_id): 1100 => {}, block: true, },
    DripstoneBlock { get_id: 13, from_id(_id): 13 => {}, block: true, },
    Deepslate { get_id: 8, from_id(_id): 8 => {}, block: true, },
    CobbledDeepslate { get_id: 9, from_id(_id): 9 => {}, block: true, },
    DeepslateTiles { get_id: 289, from_id(_id): 289 => {}, block: true, },
    DeepslateBricks { get_id: 287, from_id(_id): 287 => {}, block: true, },
    CrackedDeepslateBricks { get_id: 288, from_id(_id): 288 => {}, block: true, },
    GrassBlock { get_id: 14, from_id(_id): 14 => {}, block: true, },
    Dirt { get_id: 15, from_id(_id): 15 => {}, block: true, },
    CoarseDirt { get_id: 16, from_id(_id): 16 => {}, block: true, },
    Podzol { get_id: 17, from_id(_id): 17 => {}, block: true, },
    SnowBlock { get_id: 253, from_id(_id): 253 => {}, block: true, },

    Unknown {
        props: {
            id: u32
        },
        get_id: id,
        from_id(id): _ => { id: id },
    }
}

impl Item {
    pub fn get_id(self) -> u32 {
        if let Self::Unknown { id } = self {
            return id;
        }
        crate::generated::LEGACY_ITEMS[self.legacy_id() as usize]
    }
    pub fn from_id(id: u32) -> Self {
        let legacy = crate::generated::TARGET_TO_LEGACY_ITEMS
            .get(id as usize)
            .copied()
            .unwrap_or(u32::MAX);
        if legacy == u32::MAX {
            Self::Unknown { id }
        } else {
            match Self::from_legacy_id(legacy) {
                Self::Unknown { .. } => Self::Unknown { id },
                item => item,
            }
        }
    }
    pub fn from_name(name: &str) -> Option<Item> {
        crate::generated::ITEMS
            .iter()
            .position(|i| i.0 == name.trim_start_matches("minecraft:"))
            .map(|i| Self::from_id(i as u32))
    }
    pub fn get_name(self) -> &'static str {
        crate::generated::ITEMS
            .get(self.get_id() as usize)
            .map_or("air", |i| i.0)
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    #[test]
    fn all_target_items_round_trip_and_piston_variants_are_distinct() {
        for id in 0..crate::generated::ITEMS.len() as u32 {
            assert_eq!(Item::from_id(id).get_id(), id, "item {id}");
        }
        assert_eq!(
            Item::from_name("piston").unwrap(),
            Item::Piston { sticky: false }
        );
        assert_eq!(
            Item::from_name("sticky_piston").unwrap(),
            Item::Piston { sticky: true }
        );
        assert_eq!(Item::from_name("stone").unwrap().get_id(), 1);
        assert_eq!(Item::from_name("compass"), Some(Item::Compass));
        assert_eq!(Item::from_id(Item::Compass.get_id()), Item::Compass);
        assert!(!Item::Compass.is_block());
        assert!(Item::from_name("pale_oak_planks").unwrap().is_block());
    }
}
