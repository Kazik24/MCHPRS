using System.Diagnostics;
using System.Security.Cryptography;
using System.Text.Json;
using MAIR.Core.Blocks;
using MAIR.Core.Redstone;
using MAIR.Core.Schem;
using MAIR.Core.World;
using MAIR.Core.Rsc.Compilation;

var path=Path.GetFullPath(args[0]);
var origin=args[1].Split(',').Select(int.Parse).ToArray();
var output=Path.GetFullPath(args[2]);
var registry=BlockRegistry.Instance;
var schematic=Schematic.Load(path);
if(schematic.Skipped!=0) throw new Exception($"Skipped {schematic.Skipped} states");
Plot Load(){
    var world=new Plot(0,0,32);
    foreach(var b in schematic.Blocks)world.SetBlock(b.X+origin[0],b.Y+origin[1],b.Z+origin[2],b.State);
    foreach(var c in schematic.Comparators)world.SetComparatorOutput(new(c.X+origin[0],c.Y+origin[1],c.Z+origin[2]),c.Output);
    foreach(var c in schematic.Containers)world.SetContainerSignal(new(c.X+origin[0],c.Y+origin[1],c.Z+origin[2]),c.Signal,false);
    return world;
}
var native=Load(); var compiled=Load();
var clock=Stopwatch.StartNew();
var prepared=Compiler.Prepare(compiled); var engine=Compiler.Finish(compiled,prepared);
var compileMs=clock.Elapsed.TotalMilliseconds;
var positions=schematic.Blocks.Select(b=>new BlockPos(b.X+origin[0],b.Y+origin[1],b.Z+origin[2]))
    .Concat(prepared.Item1.Nodes.Select(n=>n.Position)).Concat(prepared.Item1.Wires.Select(w=>w.Pos)).Distinct()
    .OrderBy(p=>p.Y).ThenBy(p=>p.Z).ThenBy(p=>p.X).ToArray();
var probes=new[]{new BlockPos(155,10,124),new BlockPos(156,10,124),new BlockPos(155,10,126),new BlockPos(119,10,126),new BlockPos(121,10,126)};
object States(Plot w)=>probes.Select(p=>new{position=new[]{p.X,p.Y,p.Z},state=registry.Format(w.Get(p))}).ToArray();
object? firstExact=null,firstSignal=null;
var timeline=new List<object>();
void Compare(int tick,string phase){
    var exact=positions.Where(p=>native.Get(p)!=compiled.Get(p)).ToArray();
    var signal=exact.Where(p=>!(BlockInfo.TypeOf[native.Get(p)]==BlockType.Wire&&BlockInfo.TypeOf[compiled.Get(p)]==BlockType.Wire&&BlockInfo.Power[native.Get(p)]==BlockInfo.Power[compiled.Get(p)])).ToArray();
    object Describe(BlockPos p)=>new{position=new[]{p.X,p.Y,p.Z},interpreter=registry.Format(native.Get(p)),compiled=registry.Format(compiled.Get(p))};
    if(firstExact==null&&exact.Length>0)firstExact=new{tick,phase,count=exact.Length,differences=exact.Take(16).Select(Describe).ToArray()};
    if(firstSignal==null&&signal.Length>0)firstSignal=new{tick,phase,count=signal.Length,differences=signal.Take(16).Select(Describe).ToArray()};
    timeline.Add(new{tick,phase,exact=exact.Length,signal=signal.Length,interpreter=States(native),compiled=States(compiled)});
}
engine.Flush(); Compare(0,"before start");
var start=new BlockPos(195,43,80);
if(!Logic.Use(native,start)||!engine.Use(start))throw new Exception("Start rejected");
engine.Flush(); Compare(0,"after start");
for(int tick=1;tick<=10;tick++){native.Tick();engine.Tick();engine.Flush();Compare(tick,"after tick");}
var report=new{source="unchanged decompiled-reference rebuild, not original released binary",schematic=path,
    schematic_sha256=Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(path))).ToLowerInvariant(),
    dll_sha256=Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(typeof(Schematic).Assembly.Location))).ToLowerInvariant(),
    dll_version=typeof(Schematic).Assembly.GetName().Version?.ToString(),origin,dimensions=new[]{schematic.Width,schematic.Height,schematic.Length},
    blocks=schematic.Blocks.Count,skipped=schematic.Skipped,comparators=schematic.Comparators.Count,containers=schematic.Containers.Count,
    command_entity_limit="existing MAIR loader exposes comparators/containers only; commands not replayed",compile_ms=compileMs,first_exact=firstExact,first_signal=firstSignal,timeline};
File.WriteAllText(output,JsonSerializer.Serialize(report,new JsonSerializerOptions{WriteIndented=true}));
Console.WriteLine(JsonSerializer.Serialize(new{compileMs,firstExact,firstSignal,output}));
