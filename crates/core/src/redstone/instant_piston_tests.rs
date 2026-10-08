//! Test-only recorder and behavioral pack regressions. Hooks compile out of servers.
use crate::plot::worldedit::{load_schematic, paste_clipboard, WorldEditClipboard};
use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::world::{
    storage::{Chunk, PalettedBitBuffer},
    World,
};
use mchprs_blocks::blocks::{Block, RotateAmt};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{AdvancePhase, PistonEvent};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

mod io;

thread_local! {
    static RECORDER: RefCell<Option<Recorder>> = const { RefCell::new(None) };
}
#[derive(Default)]
struct Recorder {
    entries: Vec<Value>,
    positions: Option<Vec<BlockPos>>,
    depth: usize,
    action: usize,
    operation: usize,
}

pub(crate) struct CallbackGuard(bool);
impl Drop for CallbackGuard {
    fn drop(&mut self) {
        if self.0 {
            RECORDER.with(|r| {
                if let Some(r) = r.borrow_mut().as_mut() {
                    r.depth -= 1;
                }
            });
        }
    }
}
pub(crate) fn callback(
    world: &impl World,
    block: Block,
    pos: BlockPos,
    dir: Option<BlockFace>,
) -> CallbackGuard {
    let active = RECORDER.with(|r| {
        r.borrow().as_ref().is_some_and(|r| {
            r.positions
                .as_ref()
                .is_none_or(|positions| positions.contains(&pos))
        })
    });
    if active {
        record_operation(
            world,
            "callback",
            json!({"pos":pos,"block":block.get_name(),"dir":dir,
            "piston_power": match block { Block::Piston { piston } => Some(super::piston::should_piston_extend(world,piston.facing,pos)), _ => None }}),
        );
        RECORDER.with(|r| r.borrow_mut().as_mut().unwrap().depth += 1);
    }
    CallbackGuard(active)
}
pub(crate) fn record_event(world: &impl World, kind: &str, event: PistonEvent) {
    record_operation(world, kind, json!(event));
}
pub(crate) fn record_operation(world: &impl World, kind: &str, data: Value) {
    RECORDER.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            if let Some(positions) = &r.positions {
                let pos = data
                    .get("pos")
                    .or_else(|| data.get(0))
                    .and_then(|pos| serde_json::from_value::<BlockPos>(pos.clone()).ok());
                if !pos.is_some_and(|pos| positions.contains(&pos)) {
                    return;
                }
            }
            r.entries.push(json!({"kind":kind,"tick":world.piston_state().logical_tick,
                "phase":world.piston_state().phase,"operation":r.operation,"action":r.action,"depth":r.depth,"data":data}));
        }
    });
}

pub(crate) fn capture_at(positions: &[BlockPos], f: impl FnOnce()) -> Vec<Value> {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            RECORDER.with(|r| *r.borrow_mut() = None);
        }
    }
    RECORDER.with(|r| {
        let mut recorder = r.borrow_mut();
        assert!(recorder.is_none(), "nested callback capture");
        *recorder = Some(Recorder {
            positions: Some(positions.to_vec()),
            ..Default::default()
        });
    });
    let _guard = Guard;
    f();
    take_callbacks()
}
fn context(action: usize, operation: usize) {
    RECORDER.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            r.action = action;
            r.operation = operation;
        }
    });
}
fn take_callbacks() -> Vec<Value> {
    RECORDER.with(|r| {
        r.borrow_mut()
            .as_mut()
            .map(|r| std::mem::take(&mut r.entries))
            .unwrap_or_default()
    })
}
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn pack() -> PathBuf {
    root().join("test_data/instant-pistons")
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
fn manifests() -> Vec<Value> {
    manifests_at(&pack())
}
fn manifests_at(directory: &Path) -> Vec<Value> {
    let mut paths: Vec<_> = std::fs::read_dir(directory.join("fixtures"))
        .unwrap()
        .map(|p| p.unwrap().path())
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| read(p))
        .filter(|m| !m["cases"].as_array().unwrap().is_empty())
        .collect()
}
fn triple(v: &Value) -> BlockPos {
    BlockPos::new(
        v[0].as_i64().unwrap() as i32,
        v[1].as_i64().unwrap() as i32,
        v[2].as_i64().unwrap() as i32,
    )
}
const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);
fn transform(mut p: BlockPos, dims: (u32, u32, u32), rotation: u32) -> BlockPos {
    let (mut w, _, mut l) = dims;
    for _ in 0..rotation / 90 {
        p = BlockPos::new(l as i32 - 1 - p.z, p.y, p.x);
        std::mem::swap(&mut w, &mut l);
    }
    p + ORIGIN
}
fn dims(cb: &WorldEditClipboard) -> (u32, u32, u32) {
    (cb.size_x, cb.size_y, cb.size_z)
}
fn local_at(i: usize, d: (u32, u32, u32)) -> BlockPos {
    BlockPos::new(
        i as i32 % d.0 as i32,
        (i as u32 / (d.0 * d.2)) as i32,
        (i as u32 / d.0 % d.2) as i32,
    )
}
fn empty() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH)
            .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect(),
        Default::default(),
    )
}
fn load(m: &Value, rotation: u32) -> (PlotWorld, Vec<BlockPos>, (u32, u32, u32)) {
    let bytes = std::fs::read(root().join(m["fixture"].as_str().unwrap())).unwrap();
    assert_eq!(m["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    let mut cb = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let d = dims(&cb);
    assert_eq!(m["dimensions"], json!([d.0, d.1, d.2]));
    assert_eq!(
        m["coordinates"]["loader_offset"],
        json!([cb.offset_x, cb.offset_y, cb.offset_z])
    );
    let mut watch = BTreeSet::new();
    for i in 0..cb.data.entries() {
        let p = local_at(i, d);
        let b = Block::from_id(cb.data.get_entry(i));
        if b != Block::Air {
            watch.insert((p.x, p.y, p.z));
        }
        if let Block::Piston { piston } = b {
            let head = p.offset(piston.facing.into());
            let payload = head.offset(piston.facing.into());
            for q in [head, payload] {
                watch.insert((q.x, q.y, q.z));
            }
        }
    }
    fn ports(v: &Value, watch: &mut BTreeSet<(i32, i32, i32)>) {
        if let Some(a) = v.as_array() {
            if a.len() == 3 && a[0].is_number() {
                let p = triple(v);
                watch.insert((p.x, p.y, p.z));
            } else {
                for x in a {
                    ports(x, watch);
                }
            }
        }
        if let Some(o) = v.as_object() {
            for v in o.values() {
                ports(v, watch);
            }
        }
    }
    ports(&m["ports"], &mut watch);
    if rotation != 0 {
        let (w, l) = if rotation == 90 || rotation == 270 {
            (d.2, d.0)
        } else {
            (d.0, d.2)
        };
        let mut data = PalettedBitBuffer::new((w * d.1 * l) as usize, 9);
        for i in 0..cb.data.entries() {
            let p = transform(local_at(i, d), d, rotation) - ORIGIN;
            let mut b = Block::from_id(cb.data.get_entry(i));
            b.rotate(match rotation {
                90 => RotateAmt::Rotate90,
                180 => RotateAmt::Rotate180,
                270 => RotateAmt::Rotate270,
                _ => unreachable!(),
            });
            data.set_entry(
                ((p.y as u32 * l + p.z as u32) * w + p.x as u32) as usize,
                b.get_id(),
            );
        }
        cb.data = data;
        cb.block_entities = cb
            .block_entities
            .into_iter()
            .map(|(p, e)| (transform(p, d, rotation) - ORIGIN, e))
            .collect();
        cb.size_x = w;
        cb.size_z = l;
    }
    let mut world = empty();
    paste_clipboard(
        &mut world,
        &cb,
        ORIGIN + BlockPos::new(cb.offset_x, cb.offset_y, cb.offset_z),
        false,
    );
    assert_eq!(world.piston_state().phase, AdvancePhase::BetweenTicks);
    assert!(quiet(&world));
    (
        world,
        watch
            .into_iter()
            .map(|(x, y, z)| transform(BlockPos::new(x, y, z), d, rotation))
            .collect(),
        d,
    )
}
fn quiet(w: &PlotWorld) -> bool {
    w.scheduler().iter_entries().next().is_none()
        && w.piston_state().events.is_empty()
        && w.piston_state().motions.is_empty()
}
fn cell(w: &PlotWorld, p: BlockPos) -> Value {
    let b = w.get_block(p);
    json!({"name":b.get_name(),"properties":b.properties(),"raw":b.get_id(),"entity":w.get_block_entity(p),
        "piston_power":match b {Block::Piston{piston}=>Some(super::piston::should_piston_extend(w,piston.facing,p)),_=>None}})
}
fn observe(w: &PlotWorld, m: &Value, d: (u32, u32, u32), r: u32) -> Value {
    fn walk(w: &PlotWorld, v: &Value, d: (u32, u32, u32), r: u32) -> Value {
        if v[0].is_number() {
            cell(w, transform(triple(v), d, r))
        } else {
            Value::Array(
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|p| walk(w, p, d, r))
                    .collect(),
            )
        }
    }
    Value::Object(
        m["ports"]["observations"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), walk(w, v, d, r)))
            .collect(),
    )
}
fn physical(w: &PlotWorld, watch: &[BlockPos]) -> Value {
    // All watched physical cells and global work at completed boundaries.
    json!({"cells":watch.iter().map(|&p|cell(w,p)).collect::<Vec<_>>(),"scheduled":w.scheduler().iter_entries().collect::<Vec<_>>(),"piston_state":w.piston_state()})
}
fn step(
    w: &mut PlotWorld,
    mode: &str,
    action: usize,
    operation: &mut usize,
    mut snap: impl FnMut(&PlotWorld, usize),
) {
    let target = w.piston_state().logical_tick + 1;
    for _ in 0..200_000 {
        *operation += 1;
        context(action, *operation);
        match mode {
            "game" => w.tick_interpreted(),
            "nano" => w.nanotick_advance(1),
            "pico" => w.picotick_advance(1),
            _ => unreachable!(),
        }
        snap(w, *operation);
        if w.piston_state().logical_tick == target
            && w.piston_state().phase == AdvancePhase::BetweenTicks
        {
            return;
        }
    }
    panic!("bounded stepping failed at {target} with {mode}");
}
fn mutate(w: &mut PlotWorld, op: &Value, d: (u32, u32, u32), r: u32) {
    if op["op"] == "construct_retracted" {
        let bases: Vec<_> = op["bases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| transform(triple(v), d, r))
            .collect();
        for &p in &bases {
            let Block::Piston { piston } = w.get_block(p) else {
                panic!("base missing")
            };
            w.set_block(p.offset(piston.facing.into()), Block::Air);
            w.set_block(
                p,
                Block::Piston {
                    piston: piston.extend(false),
                },
            );
        }
        for p in bases {
            super::update(w.get_block(p), w, p, None);
        }
        return;
    }
    let p = transform(triple(&op["pos"]), d, r);
    match op["op"].as_str().unwrap() {
        "lever" => {
            let Block::Lever { mut lever } = w.get_block(p) else {
                panic!("lever missing at {p:?}");
            };
            let powered = op["powered"].as_bool().unwrap();
            if lever.powered != powered {
                // Playerless equivalent of interaction::on_use's lever branch.
                // Preserve both notification paths; permission/sound handling is irrelevant here.
                lever.powered = powered;
                w.set_block(p, Block::Lever { lever });
                super::update_surrounding_blocks(w, p);
                let attachment = match lever.face {
                    mchprs_blocks::blocks::LeverFace::Ceiling => BlockFace::Top,
                    mchprs_blocks::blocks::LeverFace::Floor => BlockFace::Bottom,
                    mchprs_blocks::blocks::LeverFace::Wall => lever.facing.opposite().block_face(),
                };
                super::update_surrounding_blocks(w, p.offset(attachment));
            }
        }
        "destroy" => {
            let b = w.get_block(p);
            assert_ne!(b, Block::Air, "destroy source absent: {p:?}");
            crate::interaction::destroy(b, w, p);
        }
        "notify" => super::update_surrounding_blocks(w, p),
        "raw" | "set_notified" => {
            let mut b = Block::from_name(op["state"].as_str().unwrap()).unwrap();
            if let Some(props) = op["properties"].as_object() {
                b.set_properties(
                    props
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.as_str().unwrap()))
                        .collect(),
                );
            }
            w.set_block(p, b);
            if op["op"] == "set_notified" {
                super::update(b, w, p, None);
                super::update_surrounding_blocks(w, p);
            }
        }
        _ => panic!("unknown action {op}"),
    }
}
fn episode(m: &Value, c: &Value, r: u32, mode: &str, capture: bool) -> Value {
    let mut extended = m.clone();
    if let Some(observations) = c["observations"].as_object() {
        for (k, v) in observations {
            extended["ports"]["observations"][k] = v.clone();
        }
    }
    let m = &extended;
    let (mut w, watch, d) = load(m, r);
    if capture {
        RECORDER.with(|v| *v.borrow_mut() = Some(Recorder::default()));
    }
    let mut samples = Vec::new();
    let mut previous = Vec::new();
    let mut operation = 0;
    let mut boundaries = Vec::new();
    let mut snapshot = |w: &PlotWorld, operation: usize, action: usize, label: &str| {
        if !capture {
            return;
        }
        let cells: Vec<_> = watch.iter().map(|&p| cell(w, p)).collect();
        let changes: Vec<_> = cells
            .iter()
            .enumerate()
            .filter(|(i, v)| previous.get(*i) != Some(*v))
            .map(|(i, v)| json!([i, v]))
            .collect();
        previous = cells;
        samples.push(json!({"label":label,"tick":w.piston_state().logical_tick,"phase":w.piston_state().phase,"operation":operation,"action":action,
            "changes":changes,"observations":observe(w,m,d,r),"scheduled":w.scheduler().iter_entries().collect::<Vec<_>>(),"piston_state":w.piston_state(),"callbacks":take_callbacks()}));
    };
    snapshot(&w, 0, 0, "initial");
    let mut ready_checks = Vec::new();
    for (i, op) in c["actions"].as_array().unwrap().iter().enumerate() {
        let ai = i + 1;
        context(ai, operation);
        match op["op"].as_str().unwrap() {
            "wait" | "wait_ready" => {
                let bound = op["ticks"].as_u64().unwrap();
                let mut used = 0;
                // Require two unchanged quiescent completed boundaries, never a magic tick count.
                let mut stable = 0;
                let mut last = physical(&w, &watch)["cells"].clone();
                for _ in 0..bound {
                    step(&mut w, mode, ai, &mut operation, |w, oi| {
                        snapshot(w, oi, ai, "setup-step")
                    });
                    used += 1;
                    let now = physical(&w, &watch)["cells"].clone();
                    stable = if quiet(&w) && now == last {
                        stable + 1
                    } else {
                        0
                    };
                    last = now;
                    if op["op"] == "wait_ready" && stable >= 2 {
                        break;
                    }
                }
                if op["op"] == "wait_ready" {
                    assert!(
                        stable >= 2,
                        "{} {} did not reach readiness within {bound} ticks",
                        m["id"],
                        c["id"]
                    );
                }
                ready_checks.push(json!({"action":ai,"kind":op["op"],"used_ticks":used,"quiet":quiet(&w),"stable_boundaries":stable}));
            }
            _ => mutate(&mut w, op, d, r),
        }
        snapshot(&w, operation, ai, "after-action");
    }
    let start = w.piston_state().logical_tick;
    let actions = c["actions"].as_array().unwrap().len();
    let mut projections = vec![observe(&w, m, d, r)];
    for _ in 0..c["ticks"].as_u64().unwrap() {
        step(&mut w, mode, actions, &mut operation, |w, oi| {
            snapshot(w, oi, actions, "step")
        });
        projections.push(observe(&w, m, d, r));
        if !capture {
            // Hash the complete serialized state to bound memory in long counter episodes.
            boundaries.push(json!(format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&physical(&w, &watch)).unwrap())
            )));
        }
    }
    let callbacks = take_callbacks();
    if capture {
        RECORDER.with(|v| *v.borrow_mut() = None);
    }
    json!({"schema_version":1,"fixture":m["fixture"],"fixture_sha256":m["sha256"],"case_id":c["id"],"inputs":c["inputs"],"engine":"MCHPRS interpreter",
        "coordinates":m["coordinates"],"rotation":r,"setup":m["setup"],"protocol":m["protocol"],"ordered_stimuli":c["actions"],"readiness_checks":ready_checks,
        "observation_projection":m["ports"]["observations"],"stepping":mode,"start_tick":start,"limits":{"response_ticks":c["ticks"],"operations_per_tick":200000},
        "termination":"completed bounded response; quiescence and periodicity assessed separately", "watch_absolute":watch,
        "samples":samples,"tail_callbacks":callbacks,"completed_boundaries":if capture {Value::Null}else{json!(boundaries)},"projections":projections})
}

fn fixture(id: &str) -> Value {
    manifests().into_iter().find(|m| m["id"] == id).unwrap()
}
fn named_case(m: &Value, id: &str) -> Value {
    m["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap()
        .clone()
}
fn power(v: &Value) -> u8 {
    v["properties"]["power"].as_str().unwrap().parse().unwrap()
}
fn bit(v: &Value) -> Option<u16> {
    match v["name"].as_str().unwrap() {
        "air" => Some(1),
        "redstone_block" => Some(0),
        "moving_piston" => None,
        n => panic!("invalid output {n}"),
    }
}
fn applied(t: &Value) -> Vec<&Value> {
    t["samples"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["callbacks"].as_array().unwrap())
        .filter(|c| c["kind"] == "event_applied")
        .collect()
}

#[test]
fn exact_saved_imports_remain_ready_without_notifications() {
    for m in manifests() {
        for r in m["rotations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_u64().unwrap() as u32)
        {
            let (mut w, watch, _) = load(&m, r);
            let initial = physical(&w, &watch)["cells"].clone();
            for _ in 0..24 {
                w.tick_interpreted();
                assert!(quiet(&w), "{} r{r} saved work", m["id"]);
                assert_eq!(
                    physical(&w, &watch)["cells"],
                    initial,
                    "{} r{r} saved stability",
                    m["id"]
                );
            }
        }
    }
}

#[test]
fn quiescent_torch_and_external_rearm_protocols_allow_another_falling_response() {
    for (id, base, payload, resets) in [
        ("instant_torch", [0, 1, 3], [0, 1, 5], true),
        ("instant_down_torch_reset", [0, 3, 3], [0, 1, 3], true),
        ("instant_reset_redstone", [0, 1, 3], [0, 1, 5], false),
    ] {
        let m = fixture(id);
        let (mut w, watch, d) = load(&m, 0);
        let source = m["ports"]["inputs"]["input"].clone();
        mutate(&mut w, &json!({"op":"destroy","pos":source}), d, 0);
        for _ in 0..12 {
            w.tick_interpreted();
        }
        assert!(quiet(&w), "{id} endpoint");
        mutate(
            &mut w,
            &json!({"op":"set_notified","pos":source,"state":"redstone_block"}),
            d,
            0,
        );
        let mut stable = 0;
        let mut last = physical(&w, &watch)["cells"].clone();
        for _ in 0..32 {
            w.tick_interpreted();
            let now = physical(&w, &watch)["cells"].clone();
            stable = if quiet(&w) && now == last {
                stable + 1
            } else {
                0
            };
            last = now;
            if stable >= 2 {
                break;
            }
        }
        assert!(stable >= 2, "{id} source rearm");
        let bp = transform(BlockPos::new(base[0], base[1], base[2]), d, 0);
        let Block::Piston { piston } = w.get_block(bp) else {
            panic!("{id} ready base missing")
        };
        assert!(piston.extended);
        assert!(matches!(
            w.get_block(bp.offset(piston.facing.into())),
            Block::PistonHead { .. }
        ));
        assert_eq!(
            w.get_block(transform(
                BlockPos::new(payload[0], payload[1], payload[2]),
                d,
                0
            )),
            Block::RedstoneBlock {}
        );
        assert!(power(&observe(&w, &m, d, 0)["output"]) > 0);
        mutate(&mut w, &json!({"op":"destroy","pos":source}), d, 0);
        w.tick_interpreted();
        assert_eq!(power(&observe(&w, &m, d, 0)["output"]), 0);
        for _ in 0..5 {
            w.tick_interpreted();
        }
        assert_eq!(
            power(&observe(&w, &m, d, 0)["output"]) > 0,
            resets,
            "{id} second reset"
        );
    }
}

#[test]
fn single_falling_responses_distinguish_autoreset_latch_and_forced_power() {
    for (id, periodic, reset) in [
        ("instant_observer", true, true),
        ("instant_down", true, true),
        ("instant_reset_redstone_2", true, true),
        ("instant_torch", false, true),
        ("instant_down_torch_reset", false, true),
        ("instant_reset_redstone", false, false),
        ("instant_blocked", false, true),
    ] {
        let m = fixture(id);
        let t = episode(&m, &named_case(&m, "falling"), 0, "pico", true);
        let p = t["projections"].as_array().unwrap();
        assert_eq!(
            power(&p[1]["output"]) == 0,
            id != "instant_blocked",
            "{id} first falling response"
        );
        if id == "instant_blocked" {
            assert!(applied(&t).is_empty());
            continue;
        }
        assert_eq!(applied(&t)[0]["data"]["action"], "Retract");
        assert_eq!(applied(&t)[0]["tick"], 1);
        assert_eq!(power(&p[6]["output"]) > 0, reset, "{id} reset output");
        assert_eq!(
            power(&p[7]["output"]) == 0,
            periodic || !reset,
            "{id} recurring response"
        );
        if periodic {
            for n in 1..=18 {
                assert_eq!(p[n], p[n + 6], "{id} period at {n}");
            }
        }
    }
}

#[test]
fn conjunction_or_inhibition_and_xor_have_contextual_first_wave_tables() {
    for id in ["and_1", "and_2", "and_3", "or_1", "not_1", "xor_simple"] {
        let m = fixture(id);
        for c in m["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["id"].as_str().unwrap().starts_with("events-"))
        {
            let a = c["inputs"][0].as_u64().unwrap() != 0;
            let b = c["inputs"][1].as_u64().unwrap() != 0;
            let expected = match id {
                "and_1" | "and_2" => a && b,
                "and_3" => a && b && c["id"] == "events-11-ba",
                "or_1" => a || b,
                "not_1" => a && !b,
                "xor_simple" => a ^ b,
                _ => unreachable!(),
            };
            for r in [0, 90, 180, 270] {
                let t = episode(&m, c, r, "game", false);
                assert_eq!(
                    power(&t["projections"][1]["output"]) == 0,
                    expected,
                    "{id} {} r{r}",
                    c["id"]
                );
            }
        }
    }
}

#[test]
fn chain_causality_is_fifo_retraction_then_callback_then_downstream_event() {
    let m = fixture("instant_chain");
    let t = episode(&m, &named_case(&m, "falling"), 0, "pico", true);
    let entries: Vec<_> = t["samples"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["callbacks"].as_array().unwrap())
        .collect();
    let first = entries
        .iter()
        .position(|c| c["kind"] == "event_execute" && c["tick"] == 1)
        .unwrap();
    let enqueue = entries
        .iter()
        .position(|c| c["kind"] == "event_enqueue" && c["data"]["pos"]["z"] == 46)
        .unwrap();
    let second = entries
        .iter()
        .position(|c| c["kind"] == "event_execute" && c["data"]["pos"]["z"] == 46)
        .unwrap();
    assert!(first < enqueue && enqueue < second);
    assert_eq!(entries[enqueue]["operation"], entries[first]["operation"]);
    assert_eq!(entries[enqueue]["depth"], 1);
    assert_eq!(entries[second]["tick"], 1);
    assert_ne!(entries[first]["operation"], entries[second]["operation"]);
    assert_eq!(entries[enqueue - 1]["kind"], "callback");
    assert_eq!(entries[enqueue - 1]["data"]["piston_power"], false);
}

#[test]
fn adder_inhibition_changes_dust_connections_before_pending_target_validation() {
    for (id, case, inhibit, target, wire) in [
        (
            "adder_1bit",
            "prepared-1-1-0",
            [6, 3, 1],
            [11, 2, 1],
            [8, 2, 1],
        ),
        (
            "adder_1bit",
            "prepared-0-1-1",
            [12, 3, 1],
            [16, 2, 1],
            [14, 2, 1],
        ),
        (
            "adder_11bits",
            "prepared-1023-1-0",
            [8, 3, 41],
            [13, 2, 41],
            [10, 2, 41],
        ),
        (
            "adder_11bits",
            "prepared-1023-1-0",
            [14, 3, 37],
            [18, 2, 37],
            [16, 2, 37],
        ),
    ] {
        let m = fixture(id);
        let mut protocol = named_case(&m, case);
        // This causal assertion needs only the first wave; reset has separate tests.
        protocol["ticks"] = json!(1);
        let t = episode(&m, &protocol, 0, "pico", true);
        let pos = |p: [i32; 3]| json!(ORIGIN + BlockPos::new(p[0], p[1], p[2]));
        let tick = t["start_tick"].as_u64().unwrap() + 1;
        let entries: Vec<_> = t["samples"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["callbacks"].as_array().unwrap())
            .filter(|e| e["tick"] == tick)
            .collect();
        let enqueue = entries
            .iter()
            .position(|e| e["kind"] == "event_enqueue" && e["data"]["pos"] == pos(target))
            .unwrap();
        let fired = entries
            .iter()
            .position(|e| e["kind"] == "event_applied" && e["data"]["pos"] == pos(inhibit))
            .unwrap();
        let validation = entries
            .iter()
            .position(|e| e["kind"] == "event_execute" && e["data"]["pos"] == pos(target))
            .unwrap();
        assert!(
            enqueue < fired && fired < validation,
            "{id} {case} pending target must be inhibited before validation"
        );
        assert!(
            !entries
                .iter()
                .any(|e| e["kind"] == "event_applied" && e["data"]["pos"] == pos(target)),
            "{id} {case} target retraction canceled"
        );
        let target_index = t["watch_absolute"]
            .as_array()
            .unwrap()
            .iter()
            .position(|p| *p == pos(target))
            .unwrap();
        let wire_index = t["watch_absolute"]
            .as_array()
            .unwrap()
            .iter()
            .position(|p| *p == pos(wire))
            .unwrap();
        let mut cells = std::collections::BTreeMap::new();
        let mut pending_unpowered = false;
        let mut witnessed = false;
        for s in t["samples"].as_array().unwrap() {
            for c in s["changes"].as_array().unwrap() {
                cells.insert(c[0].as_u64().unwrap() as usize, c[1].clone());
            }
            if s["tick"] != tick {
                continue;
            }
            if s["operation"].as_u64().unwrap() < entries[fired]["operation"].as_u64().unwrap()
                && cells[&target_index]["piston_power"] == false
            {
                pending_unpowered = true;
            }
            if s["operation"] == entries[fired]["operation"] {
                assert_eq!(cells[&target_index]["piston_power"], true);
                assert!(power(&cells[&wire_index]) > 0);
                assert!(cells[&wire_index]["properties"]
                    .as_object()
                    .unwrap()
                    .values()
                    .any(|v| v == "up"));
                witnessed = true;
            }
        }
        assert!(
            pending_unpowered && witnessed,
            "{id} {case} causal power restoration"
        );
    }
}

#[test]
fn one_bit_and_corrected_eleven_bit_arithmetic_have_separate_valid_windows() {
    for id in ["adder_1bit", "adder_11bits"] {
        let m = fixture(id);
        for c in m["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["id"].as_str().unwrap().starts_with("prepared-"))
        {
            let t = episode(&m, c, 0, "game", false);
            assert!(t["readiness_checks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["quiet"] == true && r["stable_boundaries"].as_u64().unwrap() >= 2));
            let sum = c["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_u64().unwrap() as u16)
                .sum::<u16>();
            for tick in 1..=3 {
                let p = &t["projections"][tick];
                if id == "adder_1bit" {
                    assert_eq!(bit(&p["sum"]), Some(sum & 1), "{} at {tick}", c["id"]);
                    assert_eq!(
                        u16::from(power(&p["carry"]) == 0),
                        sum >> 1,
                        "{} carry at {tick}",
                        c["id"]
                    );
                } else {
                    let value = p["sum"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .enumerate()
                        .fold(Some(0), |v, (i, b)| Some(v? | (bit(b)? << i)));
                    assert_eq!(value, Some(sum & 2047), "{} at {tick}", c["id"]);
                }
            }
            if id == "adder_1bit" {
                for tick in 4..=5 {
                    assert_eq!(
                        u16::from(power(&t["projections"][tick]["carry"]) == 0),
                        sum >> 1,
                        "{} later carry at {tick}",
                        c["id"]
                    );
                }
            }
        }
    }
}

#[test]
fn counter_free_running_clock_increments_persistent_bud_bank() {
    let m = fixture("counter_basic");
    let (mut w, _, d) = load(&m, 0);
    mutate(&mut w, &json!({"op":"destroy","pos":[7,8,0]}), d, 0);
    let read = |w: &PlotWorld| {
        let mut value = 0u16;
        for i in 0..16 {
            match w.get_block(ORIGIN + BlockPos::new(5, 11, 3 + 2 * i)) {
                Block::Piston { piston } => value |= u16::from(!piston.extended) << i,
                _ => return None,
            }
        }
        Some(value)
    };
    assert_eq!(read(&w), Some(0));
    for tick in 1..=96 {
        w.tick_interpreted();
        if tick % 6 == 0 {
            assert_eq!(read(&w), Some(tick as u16 / 6));
        }
        if tick % 6 == 5 {
            assert_eq!(
                read(&w),
                Some((tick as u16 + 1) / 6),
                "memory persistence at {tick}"
            );
        }
    }
}

#[test]
fn negation_records_transient_order_and_repeater_filters_same_wave() {
    let m = fixture("not_1");
    let ab = episode(&m, &named_case(&m, "events-11-ab"), 0, "pico", true);
    let ba = episode(&m, &named_case(&m, "events-11-ba"), 0, "pico", true);
    let falls = |t: &Value| {
        t["samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["tick"] == 1 && power(&s["observations"]["output"]) == 0)
    };
    assert!(falls(&ab));
    assert!(!falls(&ba));
    for order in ["ab", "ba"] {
        let probe = episode(
            &m,
            &named_case(&m, &format!("probe-repeater-{order}")),
            0,
            "pico",
            true,
        );
        assert!(probe["projections"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["probe"]["properties"]["powered"] == "true"));
    }
}

#[test]
fn supplied_nanotick_counterexample_activates_consumer_before_inhibit_arrives() {
    let m = fixture("nanotick_example");
    for r in [0, 90, 180, 270] {
        let t = episode(&m, &named_case(&m, "falling"), r, "pico", true);
        assert_eq!(power(&t["projections"][1]["output"]), 0);
        assert!(power(&t["projections"][1]["inhibited_net"]) > 0);
        let events = applied(&t);
        let d = (4, 5, 14);
        let out = transform(BlockPos::new(1, 2, 9), d, r);
        let inhibit = transform(BlockPos::new(2, 2, 5), d, r);
        let oi = events
            .iter()
            .position(|e| e["data"]["pos"] == json!(out))
            .unwrap();
        let ii = events
            .iter()
            .position(|e| e["data"]["pos"] == json!(inhibit))
            .unwrap();
        assert!(oi < ii);
        assert_eq!(events[oi]["tick"], events[ii]["tick"]);
    }
}

#[test]
fn shared_or_conserves_one_payload_and_illegal_group_starves_a_reset() {
    for id in ["or_1", "or_interpreter_illigal"] {
        let m = fixture(id);
        for order in ["ab", "ba"] {
            let t = episode(
                &m,
                &named_case(&m, &format!("events-11-{order}")),
                0,
                "pico",
                true,
            );
            let allowed = if id == "or_1" {
                [[0, 2, 4], [1, 2, 4], [0, 2, 5]]
            } else {
                [[0, 2, 0], [1, 2, 0], [0, 2, 1]]
            };
            let indices: Vec<_> = allowed
                .into_iter()
                .map(|p| {
                    t["watch_absolute"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .position(|v| *v == json!(ORIGIN + BlockPos::new(p[0], p[1], p[2])))
                        .unwrap()
                })
                .collect();
            let mut cells = std::collections::BTreeMap::new();
            for s in t["samples"].as_array().unwrap() {
                for c in s["changes"].as_array().unwrap() {
                    cells.insert(c[0].as_u64().unwrap() as usize, c[1].clone());
                }
                let payloads = indices
                    .iter()
                    .filter(|i| {
                        cells[i]["name"] == "redstone_block"
                            || cells[i]["entity"]["MovingPiston"]["block_state"]
                                == Block::RedstoneBlock {}.get_id()
                    })
                    .count();
                assert_eq!(
                    payloads, 1,
                    "{id} {order} tick {} operation {}",
                    s["tick"], s["operation"]
                );
            }
            if id == "or_interpreter_illigal" {
                let (mut w, _, d) = load(&m, 0);
                for op in named_case(&m, &format!("events-11-{order}"))["actions"]
                    .as_array()
                    .unwrap()
                {
                    mutate(&mut w, op, d, 0);
                }
                for _ in 0..6 {
                    w.tick_interpreted();
                }
                let extended=[[0,2,2],[2,2,0]].iter().filter(|p|matches!(w.get_block(ORIGIN+BlockPos::new(p[0],p[1],p[2])),Block::Piston{piston}if piston.extended)).count();
                assert_eq!(
                    extended, 1,
                    "shared payload does not close both dust resets"
                );
            }
        }
    }
}

#[test]
fn downloaded_edge_case_drops_an_inflight_payload_and_later_recaptures_it() {
    let m = fixture("mchprs_redstone_update_edgecase");
    for r in [0, 90, 180, 270] {
        let (mut w, _, d) = load(&m, r);
        mutate(&mut w, &json!({"op":"destroy","pos":[6,5,6]}), d, r);
        for _ in 0..5 {
            w.tick_interpreted();
        }
        assert_eq!(
            w.get_block(transform(BlockPos::new(2, 2, 2), d, r)),
            Block::RedstoneBlock {}
        );
        assert_eq!(
            w.get_block(transform(BlockPos::new(2, 3, 2), d, r)),
            Block::Air
        );
        assert!(matches!(
            w.get_block(transform(BlockPos::new(2, 4, 2), d, r)),
            Block::MovingPiston { .. }
        ));
        for _ in 0..4 {
            w.tick_interpreted();
        }
        assert_eq!(
            w.get_block(transform(BlockPos::new(2, 2, 2), d, r)),
            Block::Air
        );
        let head = transform(BlockPos::new(2, 3, 2), d, r);
        assert!(
            matches!(w.get_block_entity(head),Some(mchprs_blocks::block_entities::BlockEntity::MovingPiston(e))if !e.extending&&!e.source&&e.block_state==Block::RedstoneBlock {}.get_id())
        );
    }
}

fn subset(actual: &Value, expected: &Value) {
    match expected {
        Value::Object(o) => {
            for (k, v) in o {
                subset(&actual[k], v)
            }
        }
        Value::Array(a) => {
            assert_eq!(actual.as_array().unwrap().len(), a.len());
            for (a, e) in actual.as_array().unwrap().iter().zip(a) {
                subset(a, e);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}
#[test]
#[ignore = "requires locally generated java-projections.json; see tools/README.md"]
fn frozen_java_port_waveforms_match_current_binaries_and_protocols() {
    let reference = read(&pack().join("java-projections.json"));
    for c in reference["cases"].as_array().unwrap() {
        let m = fixture(c["fixture_id"].as_str().unwrap());
        assert_eq!(c["fixture_sha256"], m["sha256"]);
        assert_eq!(c["server_sha1"], "e6ec2f64e6080b9b5d9b471b291c33cc7f509733");
        let protocol = named_case(&m, c["case_id"].as_str().unwrap());
        assert_eq!(c["actions"], protocol["actions"]);
        let actual = episode(
            &m,
            &protocol,
            c["rotation"].as_u64().unwrap() as u32,
            "game",
            false,
        );
        subset(&actual["projections"], &c["projections"]);
    }
}

#[test]
fn strength_one_falls_but_positive_changes_and_held_zero_are_not_new_roots() {
    use mchprs_blocks::block_entities::BlockEntity;
    use mchprs_blocks::blocks::RedstoneComparator;
    let m = fixture("instant_observer");
    let (mut w, _, d) = load(&m, 0);
    let p = ORIGIN + BlockPos::new(0, 1, 0);
    let mut comparator = RedstoneComparator::default();
    comparator.facing = mchprs_blocks::BlockDirection::North;
    comparator.powered = true;
    // Derived physical analog driver: rear redstone block (15), then a furnace
    // with the downloaded edge case's 13-redstone inventory (strength 1).
    let rear = p.offset(BlockFace::North);
    w.set_block(rear, Block::RedstoneBlock {});
    w.set_block(p, Block::RedstoneComparator { comparator });
    w.set_block_entity(
        p,
        BlockEntity::Comparator {
            output_strength: 15,
        },
    );
    super::update_surrounding_blocks(&mut w, p);
    let edge = load_schematic(std::io::Cursor::new(
        std::fs::read(pack().join("MCHPRS_REDSTONE_UPDATE_EDGECASE.schem")).unwrap(),
    ))
    .unwrap();
    w.set_block(
        rear,
        Block::Furnace {
            facing: mchprs_blocks::BlockDirection::North,
            lit: false,
        },
    );
    w.set_block_entity(rear, edge.block_entities[&BlockPos::new(1, 3, 3)].clone());
    super::update_surrounding_blocks(&mut w, rear);
    for _ in 0..4 {
        w.tick_interpreted();
    }
    assert!(quiet(&w));
    assert_eq!(power(&cell(&w, p.offset(BlockFace::South))), 1);
    assert!(
        matches!(w.get_block(transform(BlockPos::new(0,1,3),d,0)),Block::Piston {piston} if piston.extended)
    );
    crate::interaction::destroy(w.get_block(rear), &mut w, rear);
    for _ in 0..2 {
        w.tick_interpreted();
    }
    assert_eq!(power(&cell(&w, p.offset(BlockFace::South))), 0);
    assert!(matches!(
        w.get_block(transform(BlockPos::new(0, 1, 3), d, 0)),
        Block::MovingPiston { .. }
    ));
    for _ in 0..12 {
        w.tick_interpreted();
    }
    let before = json!(w.piston_state());
    let requests = json!(w.scheduler().iter_entries().collect::<Vec<_>>());
    super::update_surrounding_blocks(&mut w, p);
    assert_eq!(json!(w.piston_state()), before);
    assert_eq!(
        json!(w.scheduler().iter_entries().collect::<Vec<_>>()),
        requests
    );
}

#[test]
fn pack_completed_boundaries_agree_for_game_nano_pico() {
    for m in manifests() {
        for c in m["cases"].as_array().unwrap() {
            // Exercise every supported episode; rotations compare independently.
            for r in m["rotations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_u64().unwrap() as u32)
            {
                let game = episode(&m, c, r, "game", false);
                for mode in ["nano", "pico"] {
                    let fine = episode(&m, c, r, mode, false);
                    assert_eq!(
                        game["completed_boundaries"], fine["completed_boundaries"],
                        "{} {} rotation {r} {mode}",
                        m["id"], c["id"]
                    );
                    assert_eq!(game["projections"], fine["projections"]);
                }
            }
        }
    }
}

#[test]
#[ignore = "explicit deterministic artifact capture, not an oracle update"]
fn capture_pack() {
    let directory = std::env::var_os("INSTANT_CAPTURE_DIR")
        .map(PathBuf::from)
        .expect("set INSTANT_CAPTURE_DIR to a new directory");
    std::fs::create_dir_all(&directory).unwrap();
    let filter = std::env::var("INSTANT_FIXTURE").unwrap_or_default();
    let source = std::env::var_os("INSTANT_PACK_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(pack);
    for m in manifests_at(&source)
        .into_iter()
        .filter(|m| filter.is_empty() || m["id"].as_str().unwrap() == filter)
    {
        for c in m["cases"].as_array().unwrap() {
            for r in m["rotations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_u64().unwrap() as u32)
            {
                let name = format!(
                    "mchprs-{}-{}-r{r}.json",
                    m["id"].as_str().unwrap(),
                    c["id"].as_str().unwrap()
                );
                let path = directory.join(name);
                assert!(!path.exists(), "refusing overwrite {}", path.display());
                let mut trace = episode(&m, c, r, "pico", true);
                trace["engine_identity"] = read(&source.join("source-baseline.json"));
                trace["capture_command"]=json!("INSTANT_CAPTURE_DIR=<new-dir> cargo test -p mchprs_core --lib redstone::instant_piston_tests::capture_pack -- --ignored --exact --test-threads=1");
                std::fs::write(&path, serde_json::to_vec(&trace).unwrap()).unwrap();
                println!("captured {}", path.display());
            }
        }
    }
}
