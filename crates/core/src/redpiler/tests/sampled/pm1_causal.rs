use crate::{plot::{PlotWorld,PLOT_WIDTH},world::{World,storage::Chunk},redpiler::{Compiler,CompilerOptions,PublicationMode,TaskMonitor}};
use mchprs_blocks::{BlockPos,blocks::Block,block_entities::BlockEntity};
use serde_json::{json,Value};
use std::{cell::RefCell,path::Path,sync::Arc};

thread_local! {
    static TRACE: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
    static REORDER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
pub(super) fn select_event(runtime:&mut super::Runtime,index:usize,event:super::executor::PistonEvent)->(usize,super::executor::PistonEvent){
    if !REORDER.with(|v|v.get())||runtime.motion_time!=10{return(index,event)}
    let rank=|index:usize| match runtime.program.pistons[index].base {
        BlockPos{x:66,y:41,z:110}=>0, BlockPos{x:77,y:42,z:116}=>1,
        BlockPos{x:76,y:41,z:117}=>2, BlockPos{x:72,y:46,z:116}=>3,
        BlockPos{x:74,y:42,z:111}=>4, _=>usize::MAX,
    };
    let current=rank(index);
    if current==usize::MAX{return(index,event)}
    let candidate=runtime.piston_events.iter().enumerate().filter(|(_, (i,_))|rank(*i)<current).min_by_key(|(_, (i,_))|rank(*i)).map(|(slot,_)|slot);
    let result=if let Some(slot)=candidate{
        let next=runtime.piston_events.remove(slot).unwrap();runtime.piston_events.push_front((index,event));next
    }else{(index,event)};
    if rank(result.0)==4 { REORDER.with(|v|v.set(false)); }
    result
}
pub(super) fn enabled()->bool { TRACE.with(|v|v.borrow().is_some()) }
pub(super) fn record(tick:u64,phase:u64,message:String) { TRACE.with(|v|if let Some(v)=v.borrow_mut().as_mut(){v.push(json!({"tick":tick,"phase":phase,"message":message}));}); }
fn trace(f:impl FnOnce())->Vec<Value>{
    TRACE.with(|v|{assert!(v.borrow().is_none());*v.borrow_mut()=Some(vec![])});
    f(); TRACE.with(|v|v.borrow_mut().take().unwrap())
}
#[derive(serde::Deserialize)]
struct Fixture { min:BlockPos,max:BlockPos,start:BlockPos,blocks:Vec<(BlockPos,u32)>,entities:Vec<(BlockPos,BlockEntity)>,pending:Vec<mchprs_world::TickEntry> }
fn write(root:&Path,name:&str,value:&impl serde::Serialize){std::fs::write(root.join(name),serde_json::to_vec_pretty(value).unwrap()).unwrap();}
fn region(p:BlockPos)->bool {(40..=100).contains(&p.x)&&(30..=54).contains(&p.y)&&(90..=125).contains(&p.z)}

#[test]
#[ignore="PM1 load/counter causal trace, diagnostic artifacts only"]
fn pm1_load_counter_causal_trace(){
    let full=std::env::var_os("PM1_CAUSAL_FULL").is_some();
    let root=std::env::var_os("PM1_CAUSAL_OUTPUT").map(std::path::PathBuf::from).unwrap_or_else(||Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/pm1-causal-first"));
    assert!(!root.exists(),"fresh output directory required");std::fs::create_dir_all(&root).unwrap();
    let mut f:Fixture=serde_json::from_reader(flate2::read::GzDecoder::new(std::fs::File::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scraps/redpiler-regressions/2026-11-04/pm1-piston-slice.json.gz")).unwrap())).unwrap();
    if full {
        let clip=crate::plot::worldedit::load_schematic(std::fs::File::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/PM1_SORT_FIXED.schem")).unwrap()).unwrap();
        f.min=BlockPos::new(8,8,8);f.max=f.min+BlockPos::new(clip.size_x as i32-1,clip.size_y as i32-1,clip.size_z as i32-1);f.blocks.clear();
        for y in 0..clip.size_y {for z in 0..clip.size_z {for x in 0..clip.size_x{
            let id=clip.data.get_entry(((y*clip.size_z+z)*clip.size_x+x)as usize);if id!=0{f.blocks.push((f.min+BlockPos::new(x as i32,y as i32,z as i32),id));}
        }}}
        f.entities=clip.block_entities.into_iter().map(|(p,e)|(f.min+p,e)).collect();
    }
    let mut world=PlotWorld::from_chunks(0,0,(0..PLOT_WIDTH).flat_map(|x|(0..PLOT_WIDTH).map(move|z|Chunk::empty(x,z))).collect(),Default::default());
    world.disable_command_output_limits_for_replay();
    for &(p,id) in &f.blocks {world.set_block(p,Block::from_id(id));}
    for (p,e) in &f.entities {world.set_block_entity(*p,e.clone());}
    for t in &f.pending {world.restore_scheduled_tick(t.clone());}
    let actors:Vec<_>=f.blocks.iter().filter_map(|(p,id)|(region(*p)&&matches!(Block::from_id(*id),Block::Piston{..})).then_some(*p)).collect();
    let initial:Vec<_>=f.blocks.iter().filter(|(p,_)|region(*p)).map(|(p,id)|json!({"pos":p,"id":id,"block":format!("{:?}",Block::from_id(*id))})).collect();
    write(&root,"initial-region.json",&initial);
    let mut projected=world.activation_snapshot();let mut compiler=Compiler::default();
    compiler.compile(&projected,(f.min,f.max),CompilerOptions{publication:Some(PublicationMode::All),..Default::default()},f.pending,Arc::new(TaskMonitor::default())).unwrap();
    let reorder=std::env::var_os("PM1_CAUSAL_REORDER").is_some();
    let mut tick_states=vec![];
    let mut record_state=|tick:u32,world:&PlotWorld,projected:&PlotWorld,compiler:&Compiler|{
        let runtime=compiler.runtime.as_ref().unwrap();
        let native_pending:Vec<_>=world.scheduler().iter_entries().collect();
        let rust_pending=runtime.pending_entries();
        let mut diffs=vec![];let mut entity_diffs=vec![];
        for y in f.min.y..=f.max.y {for z in f.min.z..=f.max.z {for x in f.min.x..=f.max.x{
            let p=BlockPos::new(x,y,z);let a=world.get_block(p);let b=projected.get_block(p);
            if a!=b {diffs.push(json!({"pos":p,"interpreter":format!("{a:?}"),"compiled":format!("{b:?}")}));}
            let a=world.get_block_entity(p).map(|e|serde_json::to_value(e).unwrap());let b=projected.get_block_entity(p).map(|e|serde_json::to_value(e).unwrap());
            if a!=b {entity_diffs.push(json!({"pos":p,"interpreter":a,"compiled":b}));}
        }}}
        let row=json!({"tick":tick,"native_pending":native_pending,"compiled_pending":rust_pending,"differences":diffs,"entity_differences":entity_diffs});
        println!("PM1 tick {tick}: blocks={} entities={} pending native={} rust={}",diffs.len(),entity_diffs.len(),native_pending.len(),rust_pending.len());
        if tick>0&&tick<10{assert!(diffs.is_empty());}
        if tick==10&&!reorder {assert_eq!(diffs.len(),if full{764}else{452});}
        tick_states.push(row);
    };
    let input_native=crate::redstone::instant_piston_tests::capture_all(||{assert!(crate::interaction::use_redstone(world.get_block(f.start),&mut world,f.start,None).is_some());});
    let input_rust=trace(||compiler.try_on_use_block(f.start).unwrap());compiler.try_flush(&mut projected).unwrap();
    write(&root,"native-input.json",&input_native);write(&root,"rust-input.json",&input_rust);record_state(0,&world,&projected,&compiler);
    for tick in 1..=10{
        REORDER.with(|v|v.set(reorder&&tick==10));
        let mut detail=vec![];
        let native=crate::redstone::instant_piston_tests::capture_all(|| {detail=crate::redstone::piston::trace::capture_detailed(&actors,||world.tick_interpreted());});
        let rust=trace(||compiler.tick().unwrap());compiler.try_flush(&mut projected).unwrap();
        write(&root,&format!("native-{tick}.json"),&native);write(&root,&format!("native-pistons-{tick}.json"),&detail);write(&root,&format!("rust-{tick}.json"),&rust);
        record_state(tick,&world,&projected,&compiler);
    }
    write(&root,"states.json",&tick_states);
}
