mod blocks {
    mchprs_proc_macros::minecraft_blocks!();
}

mod protocol {
    mchprs_proc_macros::minecraft_protocol!();
}

// FNV-1a over a canonical encoding: integers and collection/string lengths are
// little-endian u64s, strings are UTF-8, and tuples are length-prefixed collections.
// These fingerprints were captured from the original Python-generated Rust tables.
struct Fingerprint(u64);

impl Fingerprint {
    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }

    fn number(&mut self, number: u64) {
        self.bytes(&number.to_le_bytes());
    }
}

trait Encode {
    fn encode(&self, hash: &mut Fingerprint);
}

macro_rules! numbers {
    ($($ty:ty),*) => {
        $(impl Encode for $ty {
            fn encode(&self, hash: &mut Fingerprint) {
                hash.number(*self as u64);
            }
        })*
    };
}

numbers!(u8, u16, u32, i32);

impl<T: Encode + ?Sized> Encode for &T {
    fn encode(&self, hash: &mut Fingerprint) {
        (**self).encode(hash);
    }
}

impl<T: Encode> Encode for [T] {
    fn encode(&self, hash: &mut Fingerprint) {
        hash.number(self.len() as u64);
        for value in self {
            value.encode(hash);
        }
    }
}

impl Encode for str {
    fn encode(&self, hash: &mut Fingerprint) {
        hash.number(self.len() as u64);
        hash.bytes(self.as_bytes());
    }
}

impl<A: Encode, B: Encode> Encode for (A, B) {
    fn encode(&self, hash: &mut Fingerprint) {
        hash.number(2);
        self.0.encode(hash);
        self.1.encode(hash);
    }
}

impl<A: Encode, B: Encode, C: Encode, D: Encode, E: Encode> Encode for (A, B, C, D, E) {
    fn encode(&self, hash: &mut Fingerprint) {
        hash.number(5);
        self.0.encode(hash);
        self.1.encode(hash);
        self.2.encode(hash);
        self.3.encode(hash);
        self.4.encode(hash);
    }
}

fn fingerprint(value: impl Encode) -> u64 {
    let mut hash = Fingerprint(0xcbf29ce484222325);
    value.encode(&mut hash);
    hash.0
}

#[test]
fn tables_match_python_generated_baseline() {
    use blocks::*;
    assert_eq!(fingerprint(LEGACY_BLOCK_STATES), 0x3ebbc668d2983775);
    assert_eq!(fingerprint(TARGET_TO_LEGACY), 0x09dca877bddffa4d);
    assert_eq!(fingerprint(STATE_TO_BLOCK_INDEX), 0x7eb46cdff85e730d);
    assert_eq!(fingerprint(STATE_SLAB_TYPES), 0x7c8ecc1179a42910);
    assert_eq!(fingerprint(STATE_DRY_IDS), 0x6495fe6017f6b099);
    assert_eq!(fingerprint(BLOCKS), 0x8dd83fe50898c4bd);
    assert_eq!(fingerprint(STATE_PROPERTIES), 0x045341712448039e);
    assert_eq!(fingerprint(LEGACY_ITEMS), 0x121fbe91c1b6aa2f);
    assert_eq!(fingerprint(TARGET_TO_LEGACY_ITEMS), 0xf740a35540ebb66a);
    assert_eq!(fingerprint(ITEMS), 0x0dfe9abb30851aed);
}

#[test]
fn block_entity_ids_match_python_generated_baseline() {
    use blocks::block_entity_types::*;
    let ids = [
        ("BANNER", BANNER),
        ("BARREL", BARREL),
        ("BEACON", BEACON),
        ("BED", BED),
        ("BEEHIVE", BEEHIVE),
        ("BELL", BELL),
        ("BLAST_FURNACE", BLAST_FURNACE),
        ("BREWING_STAND", BREWING_STAND),
        ("BRUSHABLE_BLOCK", BRUSHABLE_BLOCK),
        ("CALIBRATED_SCULK_SENSOR", CALIBRATED_SCULK_SENSOR),
        ("CAMPFIRE", CAMPFIRE),
        ("CHEST", CHEST),
        ("CHISELED_BOOKSHELF", CHISELED_BOOKSHELF),
        ("COMMAND_BLOCK", COMMAND_BLOCK),
        ("COMPARATOR", COMPARATOR),
        ("CONDUIT", CONDUIT),
        ("CRAFTER", CRAFTER),
        ("CREAKING_HEART", CREAKING_HEART),
        ("DAYLIGHT_DETECTOR", DAYLIGHT_DETECTOR),
        ("DECORATED_POT", DECORATED_POT),
        ("DISPENSER", DISPENSER),
        ("DROPPER", DROPPER),
        ("ENCHANTING_TABLE", ENCHANTING_TABLE),
        ("END_GATEWAY", END_GATEWAY),
        ("END_PORTAL", END_PORTAL),
        ("ENDER_CHEST", ENDER_CHEST),
        ("FURNACE", FURNACE),
        ("HANGING_SIGN", HANGING_SIGN),
        ("HOPPER", HOPPER),
        ("JIGSAW", JIGSAW),
        ("JUKEBOX", JUKEBOX),
        ("LECTERN", LECTERN),
        ("MOB_SPAWNER", MOB_SPAWNER),
        ("PISTON", PISTON),
        ("SCULK_CATALYST", SCULK_CATALYST),
        ("SCULK_SENSOR", SCULK_SENSOR),
        ("SCULK_SHRIEKER", SCULK_SHRIEKER),
        ("SHULKER_BOX", SHULKER_BOX),
        ("SIGN", SIGN),
        ("SKULL", SKULL),
        ("SMOKER", SMOKER),
        ("STRUCTURE_BLOCK", STRUCTURE_BLOCK),
        ("TEST_BLOCK", TEST_BLOCK),
        ("TEST_INSTANCE_BLOCK", TEST_INSTANCE_BLOCK),
        ("TRAPPED_CHEST", TRAPPED_CHEST),
        ("TRIAL_SPAWNER", TRIAL_SPAWNER),
        ("VAULT", VAULT),
    ];
    assert_eq!(fingerprint(ids.as_slice()), 0x5d35f2dcc48e8f86);
}

#[test]
fn protocol_constants_match_python_generated_baseline() {
    assert_eq!(protocol::PLAINS_BIOME, 40);
    assert_eq!(protocol::ITEM_COUNT, 1396);
    assert_eq!(protocol::COMPONENT_COUNT, 96);
    assert_eq!(protocol::PLAYER_ENTITY, 148);
    assert_eq!(
        protocol::TAGS,
        include_bytes!("../../../mc_data/1.21.5/tags.bin")
    );
    assert_eq!(
        protocol::REGISTRIES,
        include_bytes!("../../../mc_data/1.21.5/registries.bin")
    );
}
