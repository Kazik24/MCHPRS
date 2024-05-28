use crate::block_entities::ContainerType;
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
    pub fn container_with_ss(container_ty: ContainerType, ss: u8) -> ItemStack {
        let item = match container_ty {
            ContainerType::Barrel => Item::Barrel {},
            ContainerType::Hopper => Item::Hopper {},
            ContainerType::Furnace => Item::Furnace {},
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
            pub fn get_id(self) -> u32 {
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

            pub fn from_id(mut id: u32) -> Item {
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
                    _ => false
                }
            }

            pub fn max_stack_size(self) -> u32 {
                match self {
                    $(
                        $( Item::$name { .. } => $max_stack, )?
                    )*
                    _ => 64,
                }
            }
        }
    }
}

// list of ids: https://github.com/PrismarineJS/minecraft-data/blob/master/data/pc/1.18/items.json
items! {
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
        from_id(id): 354..=371 => {
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

    Bedrock { get_id: 25, from_id(_id): 25 => {}, block: true, },
    TNT { get_id: 143, from_id(_id): 143 => {}, block: true, },
    OakPlanks { get_id: 13, from_id(_id): 13 => {}, block: true, },
    SprucePlanks { get_id: 14, from_id(_id): 14 => {}, block: true, },
    BirchPlanks { get_id: 15, from_id(_id): 15 => {}, block: true, },
    JunglePlanks { get_id: 16, from_id(_id): 16 => {}, block: true, },
    AcaciaPlanks { get_id: 17, from_id(_id): 17 => {}, block: true, },
    DarkOakPlanks { get_id: 18, from_id(_id): 18 => {}, block: true, },
    OakLog { get_id: 38, from_id(_id): 38 => {}, block: true, },
    SpruceLog { get_id: 39, from_id(_id): 39 => {}, block: true, },
    BirchLog { get_id: 40, from_id(_id): 40 => {}, block: true, },
    JungleLog { get_id: 41, from_id(_id): 41 => {}, block: true, },
    AcaciaLog { get_id: 42, from_id(_id): 42 => {}, block: true, },
    DarkOakLog { get_id: 43, from_id(_id): 43 => {}, block: true, },
    StrippedSpruceLog { get_id: 44, from_id(_id): 44 => {}, block: true, },
    StrippedBirchLog { get_id: 45, from_id(_id): 45 => {}, block: true, },
    StrippedJungleLog { get_id: 46, from_id(_id): 46 => {}, block: true, },
    StrippedAcaciaLog { get_id: 47, from_id(_id): 47 => {}, block: true, },
    StrippedDarkOakLog { get_id: 48, from_id(_id): 48 => {}, block: true, },
    StrippedOakLog { get_id: 49, from_id(_id): 49 => {}, block: true, },
    OakWood { get_id: 50, from_id(_id): 50 => {}, block: true, },
    SpruceWood { get_id: 51, from_id(_id): 51 => {}, block: true, },
    BirchWood { get_id: 52, from_id(_id): 52 => {}, block: true, },
    JungleWood { get_id: 53, from_id(_id): 53 => {}, block: true, },
    AcaciaWood { get_id: 54, from_id(_id): 54 => {}, block: true, },
    DarkOakWood { get_id: 55, from_id(_id): 55 => {}, block: true, },
    StrippedOakWood { get_id: 56, from_id(_id): 56 => {}, block: true, },
    StrippedSpruceWood { get_id: 57, from_id(_id): 57 => {}, block: true, },
    StrippedBirchWood { get_id: 58, from_id(_id): 58 => {}, block: true, },
    StrippedJungleWood { get_id: 59, from_id(_id): 59 => {}, block: true, },
    StrippedAcaciaWood { get_id: 60, from_id(_id): 60 => {}, block: true, },
    StrippedDarkOakWood { get_id: 61, from_id(_id): 61 => {}, block: true, },
    Bookshelf { get_id: 144, from_id(_id): 144 => {}, block: true, },
    Sponge { get_id: 70, from_id(_id): 70 => {}, block: true, },
    HayBale { get_id: 404, from_id(_id): 404 => {}, block: true, },
    MossBlock { get_id: 865, from_id(_id): 865 => {}, block: true, },
    Granite { get_id: 2, from_id(_id): 2 => {}, block: true, },
    PolishedGranite { get_id: 3, from_id(_id): 3 => {}, block: true, },
    Diorite { get_id: 4, from_id(_id): 4 => {}, block: true, },
    PolishedDiorite { get_id: 5, from_id(_id): 5 => {}, block: true, },
    Andesite { get_id: 6, from_id(_id): 6 => {}, block: true, },
    PolishedAndesite { get_id: 7, from_id(_id): 7 => {}, block: true, },
    Cobblestone { get_id: 12, from_id(_id): 12 => {}, block: true, },
    GoldOre { get_id: 31, from_id(_id): 31 => {}, block: true, },
    DeepslateGoldOre { get_id: 32, from_id(_id): 32 => {}, block: true, },
    IronOre { get_id: 33, from_id(_id): 33 => {}, block: true, },
    DeepslateIronOre { get_id: 34, from_id(_id): 34 => {}, block: true, },
    CoalOre { get_id: 35, from_id(_id): 35 => {}, block: true, },
    DeepslateCoalOre { get_id: 36, from_id(_id): 36 => {}, block: true, },
    NetherGoldOre { get_id: 37, from_id(_id): 37 => {}, block: true, },
    LapisLazuliOre { get_id: 73, from_id(_id): 73 => {}, block: true, },
    DeepslateLapisLazuliOre { get_id: 74, from_id(_id): 74 => {}, block: true, },
    BlockofLapisLazuli { get_id: 75, from_id(_id): 75 => {}, block: true, },
    ChiseledSandstone { get_id: 78, from_id(_id): 78 => {}, block: true, },
    CutSandstone { get_id: 79, from_id(_id): 79 => {}, block: true, },
    BlockofGold { get_id: 140, from_id(_id): 140 => {}, block: true, },
    BlockofIron { get_id: 141, from_id(_id): 141 => {}, block: true, },
    Bricks { get_id: 142, from_id(_id): 142 => {}, block: true, },
    MossyCobblestone { get_id: 145, from_id(_id): 145 => {}, block: true, },
    Obsidian { get_id: 146, from_id(_id): 146 => {}, block: true, },
    DiamondOre { get_id: 155, from_id(_id): 155 => {}, block: true, },
    DeepslateDiamondOre { get_id: 156, from_id(_id): 156 => {}, block: true, },
    BlockofDiamond { get_id: 157, from_id(_id): 157 => {}, block: true, },
    RedstoneOre { get_id: 187, from_id(_id): 187 => {}, block: true, },
    DeepslateRedstoneOre { get_id: 188, from_id(_id): 188 => {}, block: true, },
    Ice { get_id: 193, from_id(_id): 193 => {}, block: true, },
    Netherrack { get_id: 201, from_id(_id): 201 => {}, block: true, },
    Basalt { get_id: 204, from_id(_id): 204 => {}, block: true, },
    PolishedBasalt { get_id: 205, from_id(_id): 205 => {}, block: true, },
    StoneBricks { get_id: 236, from_id(_id): 236 => {}, block: true, },
    MossyStoneBricks { get_id: 237, from_id(_id): 237 => {}, block: true, },
    CrackedStoneBricks { get_id: 238, from_id(_id): 238 => {}, block: true, },
    ChiseledStoneBricks { get_id: 239, from_id(_id): 239 => {}, block: true, },
    BlockofQuartz { get_id: 350, from_id(_id): 350 => {}, block: true, },
    ChiseledQuartzBlock { get_id: 351, from_id(_id): 351 => {}, block: true, },
    QuartzPillar { get_id: 352, from_id(_id): 352 => {}, block: true, },
    BlockofCoal { get_id: 422, from_id(_id): 422 => {}, block: true, },
    RedSandstone { get_id: 462, from_id(_id): 462 => {}, block: true, },
    ChiseledRedSandstone { get_id: 463, from_id(_id): 463 => {}, block: true, },
    CutRedSandstone { get_id: 464, from_id(_id): 464 => {}, block: true, },
    SmoothStone { get_id: 485, from_id(_id): 485 => {}, block: true, },
    SmoothSandstone { get_id: 486, from_id(_id): 486 => {}, block: true, },
    SmoothQuartzBlock { get_id: 487, from_id(_id): 487 => {}, block: true, },
    SmoothRedSandstone { get_id: 488, from_id(_id): 488 => {}, block: true, },
    PurpurBlock { get_id: 507, from_id(_id): 507 => {}, block: true, },
    PurpurPillar { get_id: 508, from_id(_id): 508 => {}, block: true, },
    RedNetherBricks { get_id: 519, from_id(_id): 519 => {}, block: true, },
    BrickWall { get_id: 668, from_id(_id): 668 => {}, block: true, },
    PrismarineWall { get_id: 669, from_id(_id): 669 => {}, block: true, },
    RedSandstoneWall { get_id: 670, from_id(_id): 670 => {}, block: true, },
    MossyStoneBrickWall { get_id: 671, from_id(_id): 671 => {}, block: true, },
    GraniteWall { get_id: 672, from_id(_id): 672 => {}, block: true, },
    StoneBrickWall { get_id: 673, from_id(_id): 673 => {}, block: true, },
    NetherBrickWall { get_id: 674, from_id(_id): 674 => {}, block: true, },
    AndesiteWall { get_id: 675, from_id(_id): 675 => {}, block: true, },
    RedNetherBrickWall { get_id: 676, from_id(_id): 676 => {}, block: true, },
    SandstoneWall { get_id: 677, from_id(_id): 677 => {}, block: true, },
    EndStoneBrickWall { get_id: 678, from_id(_id): 678 => {}, block: true, },
    DioriteWall { get_id: 679, from_id(_id): 679 => {}, block: true, },
    BlockofNetherite { get_id: 748, from_id(_id): 748 => {}, block: true, },
    AncientDebris { get_id: 749, from_id(_id): 749 => {}, block: true, },
    CryingObsidian { get_id: 750, from_id(_id): 750 => {}, block: true, },
    ChiseledNetherBricks { get_id: 774, from_id(_id): 774 => {}, block: true, },
    CrackedNetherBricks { get_id: 775, from_id(_id): 775 => {}, block: true, },
    QuartzBricks { get_id: 776, from_id(_id): 776 => {}, block: true, },
    OxidizedCopper { get_id: 822, from_id(_id): 822 => {}, block: true, },
    WeatheredCopper { get_id: 823, from_id(_id): 823 => {}, block: true, },
    ExposedCopper { get_id: 824, from_id(_id): 824 => {}, block: true, },
    BlockofCopper { get_id: 825, from_id(_id): 825 => {}, block: true, },
    CopperOre { get_id: 826, from_id(_id): 826 => {}, block: true, },
    DeepslateCopperOre { get_id: 827, from_id(_id): 827 => {}, block: true, },
    OxidizedCutCopper { get_id: 828, from_id(_id): 828 => {}, block: true, },
    WeatheredCutCopper { get_id: 829, from_id(_id): 829 => {}, block: true, },
    ExposedCutCopper { get_id: 830, from_id(_id): 830 => {}, block: true, },
    CutCopper { get_id: 831, from_id(_id): 831 => {}, block: true, },
    WaxedBlockofCopper { get_id: 840, from_id(_id): 840 => {}, block: true, },
    WaxedWeatheredCopper { get_id: 841, from_id(_id): 841 => {}, block: true, },
    WaxedExposedCopper { get_id: 842, from_id(_id): 842 => {}, block: true, },
    WaxedOxidizedCopper { get_id: 843, from_id(_id): 843 => {}, block: true, },
    WaxedOxidizedCutCopper { get_id: 844, from_id(_id): 844 => {}, block: true, },
    WaxedWeatheredCutCopper { get_id: 845, from_id(_id): 845 => {}, block: true, },
    WaxedExposedCutCopper { get_id: 846, from_id(_id): 846 => {}, block: true, },
    PointedDripstone { get_id: 857, from_id(_id): 857 => {}, block: true, },
    DripstoneBlock { get_id: 858, from_id(_id): 858 => {}, block: true, },
    Deepslate { get_id: 871, from_id(_id): 871 => {}, block: true, },
    CobbledDeepslate { get_id: 872, from_id(_id): 872 => {}, block: true, },
    DeepslateTiles { get_id: 880, from_id(_id): 880 => {}, block: true, },
    DeepslateBricks { get_id: 884, from_id(_id): 884 => {}, block: true, },
    CrackedDeepslateBricks { get_id: 889, from_id(_id): 889 => {}, block: true, },
    GrassBlock { get_id: 8, from_id(_id): 8 => {}, block: true, },
    Dirt { get_id: 9, from_id(_id): 9 => {}, block: true, },
    CoarseDirt { get_id: 10, from_id(_id): 10 => {}, block: true, },
    Podzol { get_id: 11, from_id(_id): 11 => {}, block: true, },
    SnowBlock { get_id: 194, from_id(_id): 194 => {}, block: true, },

    Unknown {
        props: {
            id: u32
        },
        get_id: id,
        from_id(id): _ => { id: id },
    }
}

impl Item {
    pub fn from_name(name: &str) -> Option<Item> {
        match name {
            "snowball" => Some(Item::Snowball {}),
            "totem_of_undying" => Some(Item::TotemOfUndying {}),
            "milk_bucket" => Some(Item::MilkBucket {}),
            // Convert some common types of items to fix signal strength of containers
            "redstone" => Some(Item::Redstone {}),
            "stick" => Some(Item::Redstone {}),
            "wooden_shovel" => Some(Item::TotemOfUndying {}),
            _ => None,
        }
    }

    pub fn get_name(self) -> &'static str {
        match self {
            Item::Snowball {} => "snowball",
            Item::TotemOfUndying {} => "totem_of_undying",
            Item::MilkBucket {} => "milk_bucket",
            _ => "redstone",
        }
    }
}
