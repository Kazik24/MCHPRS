using System.Text.Json;
using System.Diagnostics;
using MAIR.Core.Blocks;
using MAIR.Core.Redstone;
using MAIR.Core.Schem;
using MAIR.Core.World;
using MAIR.Core.Rsc.Compilation;
using MAIR.Core.Rsc.Engine;
var root=args[0]; var ticks=args.Length>1?int.Parse(args[1]):4096;
var registry=BlockRegistry.Instance; var patchSupport=args.Length>2 && args[2]=="hopper-support";
if(patchSupport){int patched=0;for(int i=0;i<BlockInfo.Power.Length;i++)if(BlockRegistry.WithoutNamespace(registry.GetBlockDef((ushort)i).Name)=="hopper"){BlockInfo.SturdyTop[i]=true;patched++;}Console.WriteLine($"DIAGNOSTIC MODIFICATION: hopper support enabled for {patched} states");}
var schem=Schematic.Load(Path.Combine(root,"test_data/Q2CK@Q2CK_Anpu1_Pong_KBTV.schem"));
if(schem.Skipped!=0) throw new Exception("Skipped states: "+schem.Skipped);
Plot Load() {var w=new Plot(0,0,32); foreach(var b in schem.Blocks)w.SetBlock(b.X+8,b.Y+8,b.Z+8,b.State); foreach(var c in schem.Comparators) w.SetComparatorOutput(new(c.X+8,c.Y+8,c.Z+8),c.Output); foreach(var c in schem.Containers)w.SetContainerSignal(new(c.X+8,c.Y+8,c.Z+8),c.Signal,false);return w;}
string Screen(Plot w){var chars=new List<char>();for(int y=97;y>=66;y--)for(int z=46;z<=77;z++)chars.Add(BlockInfo.Lit[w.Get(new(148,y,z))]?'1':'0');return new string(chars.ToArray());}
string Canon(ushort b){var s=registry.Format(b);if(s.StartsWith("minecraft:"))s=s[10..];if(s.StartsWith("redstone_wire["))return "wire:"+BlockInfo.Power[b];return s;}
Console.WriteLine($"loaded={schem.Blocks.Count} skipped={schem.Skipped} containers={schem.Containers.Count} comparators={schem.Comparators.Count}; dimensions={schem.Width}x{schem.Height}x{schem.Length}");
string CanonPalette(string s){s=s.Replace("minecraft:","");var b=s.IndexOf('[');return b<0?s+"[]":s[..b]+"["+string.Join(",",s[(b+1)..^1].Split(',',StringSplitOptions.RemoveEmptyEntries).Order())+"]";}
var nbt=Nbt.ReadFile(Path.Combine(root,"test_data/Q2CK@Q2CK_Anpu1_Pong_KBTV.schem"));
if(nbt.TryGetValue("Schematic",out var inner)&&inner is Dictionary<string,object> nested)nbt=nested;
var palette=(Dictionary<string,object>)(nbt.TryGetValue("Palette",out var pal)?pal:((Dictionary<string,object>)nbt["Blocks"])["Palette"]);
var paletteDifferences=palette.Keys.Select(s=>new{original=CanonPalette(s),parsed=CanonPalette(registry.Format(registry.Parse(s)))}).Where(p=>p.original!=p.parsed).ToArray();
Console.WriteLine($"palette={palette.Count} non-lossless={paletteDifferences.Length}");
File.WriteAllText(Path.Combine(root,"target/anpu-mair-check/palette.json"),JsonSerializer.Serialize(paletteDifferences,new JsonSerializerOptions{WriteIndented=true}));
var native=Load();var compiled=Load();var stopwatch=Stopwatch.StartNew();
var prepared=Compiler.Prepare(compiled); var engine=Compiler.Finish(compiled,prepared); var compileMs=stopwatch.Elapsed.TotalMilliseconds;
Console.WriteLine($"compiled {compileMs:F2}ms {engine.Stats}");
var focusWire=prepared.Item1.Wires.Single(w=>w.Pos==new BlockPos(99,49,109));
object DescribeSource(MAIR.Core.Rsc.WireSource s){var p=s.Node<0?new BlockPos(-1,-1,-1):prepared.Item1.Nodes[s.Node].Position;return new{node=s.Node,distance=s.Distance,pos=new[]{p.X,p.Y,p.Z},state=s.Node<0?"constant15":registry.Format(compiled.Get(p))};}
var wireReport=new{pos=new[]{99,49,109},unconditional=focusWire.Unconditional.Select(DescribeSource).ToArray(),conditional=focusWire.Conditional.Select(c=>new{group=c.Group,states=c.PerArrangement.Length,sources=c.PerArrangement.SelectMany(s=>s).Distinct().Select(DescribeSource).ToArray()}).ToArray()};
File.WriteAllText(Path.Combine(root,"target/anpu-mair-check/wire-table.json"),JsonSerializer.Serialize(wireReport,new JsonSerializerOptions{WriteIndented=true}));
foreach(var p in new[]{new BlockPos(60,36,15),new BlockPos(60,35,15),new BlockPos(60,34,15),new BlockPos(60,37,15)}) Console.WriteLine($"initial at {p}: {registry.Format(native.Get(p))}; sturdyTop={Sturdy(p)}; conductor={BlockInfo.IsConductor(native.Get(p))}");
bool Sturdy(BlockPos p)=>(bool)typeof(Level).GetMethod("HasSturdyTop",System.Reflection.BindingFlags.NonPublic|System.Reflection.BindingFlags.Instance)!.Invoke(native,new object[]{p,native.Get(p)})!;
var start=new BlockPos(130,76,64); if(!Logic.Use(native,start)||!engine.Use(start))throw new Exception("Start rejected");engine.Flush();
var expected=JsonDocument.Parse(File.ReadAllBytes(Path.Combine(root,"test_data/cpu-references/anpu_screen.json"))).RootElement.EnumerateArray().ToArray();
var ns=new List<object>();var cs=new List<object>();string np=null,cp=null;int firstScreen=-1,firstState=-1,nativeReference=-1,compiledReference=-1;object firstStateData=null;
var positions=schem.Blocks.Where(b=>new[]{BlockType.Wire,BlockType.Piston,BlockType.Observer,BlockType.Comparator,BlockType.Repeater,BlockType.Lamp,BlockType.Torch,BlockType.WallTorch}.Contains(BlockInfo.TypeOf[b.State])).Select(b=>new BlockPos(b.X+8,b.Y+8,b.Z+8)).ToArray();
for(int tick=0;tick<=ticks;tick++){
 var n=Screen(native);var c=Screen(compiled);if(n!=np){ns.Add(new{tick,pixels=n});np=n;}if(c!=cp){cs.Add(new{tick,pixels=c});cp=c;}if(n!=c&&firstScreen<0)firstScreen=tick;
 var ex=expected.Last(f=>f.GetProperty("tick").GetInt32()<=tick).GetProperty("pixels").GetString();if(n!=ex&&nativeReference<0)nativeReference=tick;if(c!=ex&&compiledReference<0)compiledReference=tick;
 if(firstState<0)foreach(var p in positions){var nb=Canon(native.Get(p));var cb=Canon(compiled.Get(p));if(nb!=cb){firstState=tick;firstStateData=new{tick,pos=new[]{p.X,p.Y,p.Z},native=nb,compiled=cb};break;}}
 if(tick==57||tick==58)foreach(var fp in new[]{new BlockPos(60,36,15),new BlockPos(60,35,15),new BlockPos(60,34,15),new BlockPos(60,37,15)})Console.WriteLine($"tick={tick} at{fp}: native={registry.Format(native.Get(fp))} compiled={registry.Format(compiled.Get(fp))}; sturdyTop={Sturdy(fp)}");
 if(tick>=277 && tick<=280){var p=new BlockPos(99,49,109);Console.WriteLine($"tick{tick} probe: native={registry.Format(native.Get(p))} compiled={registry.Format(compiled.Get(p))}");}
 if(tick%512==0)Console.WriteLine($"tick={tick} firstScreen={firstScreen} firstState={firstState} frozenNative={nativeReference} frozenCompiled={compiledReference}");
 if(tick<ticks){native.Tick();engine.Tick();engine.Flush();}
}
var report=new{schematic_sha256="f4068257797399531ce31d56c972af32d0f73f7d8d496ce1b7b8cd1c178decec",source="decompiled reference rebuild; original binary absent",diagnostic_hopper_support_override=patchSupport,compile_ms=compileMs,ticks,first_screen_difference=firstScreen,first_state_difference=firstStateData,first_native_frozen_screen_difference=nativeReference,first_compiled_frozen_screen_difference=compiledReference,native_frames=ns,compiled_frames=cs};
File.WriteAllText(Path.Combine(root,$"target/anpu-mair-check/result-{ticks}{(patchSupport?"-hopper-support":"")}.json"),JsonSerializer.Serialize(report,new JsonSerializerOptions{WriteIndented=true}));
Console.WriteLine("DONE "+JsonSerializer.Serialize(new{firstScreen,firstState,firstStateData,nativeReference,compiledReference,nativeFrames=ns.Count,compiledFrames=cs.Count}));








