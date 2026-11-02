using System.Diagnostics;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using MAIR.Core.Blocks;
using MAIR.Core.Redstone;
using MAIR.Core.Schem;
using MAIR.Core.World;
using MAIR.Core.Rsc;
using MAIR.Core.Protocol;
using MAIR.Core.Rsc.Compilation;
using MAIR.Core.Rsc.Engine;

var root = Path.GetFullPath(args[0]);
var id = args[1];
var actionsPath = args.Length > 2 && args[2] == "--metadata" ? "--metadata" : args.Length > 2 ? Path.GetFullPath(args[2]) : null;
var outputPath = args.Length > 3 ? Path.GetFullPath(args[3]) : null;
var definitions = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(root, "test_data/redstonedb/definitions.json")));
var definition = definitions.RootElement.GetProperty("circuits").EnumerateArray().Single(x => x.GetProperty("id").GetString() == id);
var relativeSchematic = definition.GetProperty("schematic").GetString()!;
var schematicPath = Path.Combine(root, "test_data/redstonedb", relativeSchematic);
var schematic = Schematic.Load(schematicPath);
if (schematic.Skipped != 0) throw new InvalidDataException($"MAIR skipped {schematic.Skipped} schematic states");
var levers = schematic.Blocks.Where(block => BlockInfo.TypeOf[block.State] == BlockType.Lever).Select(block => new BlockPos(block.X + 8, block.Y + 8, block.Z + 8)).ToArray();
if (actionsPath == "--metadata")
{
    Console.WriteLine(JsonSerializer.Serialize(new { id, description = definition.GetProperty("description").GetString(), schematic = relativeSchematic, dimensions = new[] { schematic.Width, schematic.Height, schematic.Length }, blocks = schematic.Blocks.Count, skipped = schematic.Skipped, levers = levers.Select(pos => new[] { pos.X, pos.Y, pos.Z }).ToArray() }, new JsonSerializerOptions { WriteIndented = true }));
    return;
}

Plot Load()
{
    var plot = new Plot(0, 0, 32);
    foreach (var block in schematic.Blocks) plot.SetBlock(block.X + 8, block.Y + 8, block.Z + 8, block.State);
    foreach (var comparator in schematic.Comparators) plot.SetComparatorOutput(new(comparator.X + 8, comparator.Y + 8, comparator.Z + 8), comparator.Output);
    foreach (var container in schematic.Containers) plot.SetContainerSignal(new(container.X + 8, container.Y + 8, container.Z + 8), container.Signal, false);
    return plot;
}

var native = Load();
var compiled = Load();
var compileClock = Stopwatch.StartNew();
var prepared = Compiler.Prepare(compiled);
var engine = Compiler.Finish(compiled, prepared);
compileClock.Stop();
var positions = prepared.Item1.Nodes.Select(node => node.Position)
    .Concat(prepared.Item1.Wires.Select(wire => wire.Pos))
    .Distinct()
    .OrderBy(pos => pos.Y).ThenBy(pos => pos.Z).ThenBy(pos => pos.X)
    .ToArray();
var focusWirePos = new BlockPos(14, 9, 8);
var focusWire = id == "8111b000090d8f0b3e1370448bcb64a3" ? prepared.Item1.Wires.Single(wire => wire.Pos == focusWirePos) : null;
object DescribeSource(WireSource source)
{
    var sourcePos = source.Node < 0 ? new BlockPos(-1, -1, -1) : prepared.Item1.Nodes[source.Node].Position;
    return new { node = source.Node, distance = source.Distance, position = Position(sourcePos), type = source.Node < 0 ? "constant15" : prepared.Item1.Nodes[source.Node].Type.ToString(), state = source.Node < 0 ? "constant15" : Format(compiled.Get(sourcePos)) };
}
var focusSourcePositions = focusWire is null ? Array.Empty<BlockPos>() : focusWire.Unconditional.Concat(focusWire.Conditional.SelectMany(condition => condition.PerArrangement).SelectMany(sources => sources)).Where(source => source.Node >= 0).Select(source => prepared.Item1.Nodes[source.Node].Position).Distinct().ToArray();

string Format(ushort state) => BlockRegistry.Instance.Format(state);
bool Differs(ushort expected, ushort actual) =>
    BlockInfo.TypeOf[expected] == BlockType.Wire && BlockInfo.TypeOf[actual] == BlockType.Wire
        ? BlockInfo.Power[expected] != BlockInfo.Power[actual]
        : expected != actual;
var probePositions = id switch
{
    "8111b000090d8f0b3e1370448bcb64a3" => new[] { new BlockPos(14, 9, 8) },
    "b6a6e870229ace7eb2afd8cb387a514e" => new[] { new BlockPos(12, 9, 10), new BlockPos(12, 9, 11), new BlockPos(12, 9, 12), new BlockPos(12, 9, 13), new BlockPos(10, 9, 22), new BlockPos(10, 9, 23) },
    _ => Array.Empty<BlockPos>()
};
object Probe(int actionStep, string phase, int actionTick) => new
{
    actionStep,
    phase,
    actionTick,
    states = probePositions.Select(pos => new { position = Position(pos), interpreter = Format(native.Get(pos)), compiled = Format(compiled.Get(pos)) }).ToArray()
};

var report = new Report
{
    FixtureId = id,
    Description = definition.GetProperty("description").GetString(),
    Schematic = relativeSchematic,
    SchematicSha256 = Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(schematicPath))).ToLowerInvariant(),
    DllSha256 = Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(typeof(Schematic).Assembly.Location))).ToLowerInvariant(),
    DllVersion = typeof(Schematic).Assembly.GetName().Version?.ToString(),
    DllLocation = typeof(Schematic).Assembly.Location,
    Width = schematic.Width,
    Height = schematic.Height,
    Length = schematic.Length,
    LoadedBlocks = schematic.Blocks.Count,
    SkippedStates = schematic.Skipped,
    LoadedContainers = schematic.Containers.Count,
    LoadedComparators = schematic.Comparators.Count,
    LeverCount = levers.Length,
    LeverPositions = levers.Select(Position).ToArray(),
    CompiledNodeCount = prepared.Item2.Nodes,
    CompiledWireCount = prepared.Item1.Wires.Count,
    TrackedPositionCount = positions.Length,
    CompileMilliseconds = compileClock.Elapsed.TotalMilliseconds,
    Actions = actionsPath is null ? null : JsonSerializer.Deserialize<ActionEntry[]>(File.ReadAllBytes(actionsPath)),
        FocusWireSources = focusWire is null ? null : new { position = Position(focusWire.Pos), initialPower = BlockInfo.Power[compiled.Get(focusWire.Pos)], unconditional = focusWire.Unconditional.Select(DescribeSource).ToArray(), conditional = focusWire.Conditional.Select(condition => new { group = condition.Group, arrangements = condition.PerArrangement.Length, sources = condition.PerArrangement.SelectMany(sources => sources).Distinct().Select(DescribeSource).ToArray() }).ToArray() }
};

if (report.Actions is not null)
{
    var tickTotal = 0;
    foreach (var action in report.Actions)
    {
        if ((uint)action.Index >= (uint)levers.Length) throw new InvalidDataException($"Action {action.Step} lever index {action.Index} outside {levers.Length} levers");
        var lever = levers[action.Index];
        if (id == "8111b000090d8f0b3e1370448bcb64a3" && action.Step == 39)
        {
            bool IsFocus(BlockPos pos) => Math.Abs(pos.X - focusWirePos.X) <= 4 && Math.Abs(pos.Y - focusWirePos.Y) <= 4 && Math.Abs(pos.Z - focusWirePos.Z) <= 4;
            object[] Scheduled(IEnumerable<TickScheduler.ScheduledTick> ticks) => ticks.Where(tick => IsFocus(tick.Pos) || focusSourcePositions.Contains(tick.Pos)).OrderBy(tick => tick.TriggerTick).ThenBy(tick => tick.Priority).ThenBy(tick => tick.Order).Select(tick => (object)new { position = Position(tick.Pos), block = tick.Block.Name, tick.TriggerTick, priority = tick.Priority.ToString(), tick.Order }).ToArray();
            report.PreInputCheckpoint = new { actionStep = action.Step, totalTicks = tickTotal, leverIndex = action.Index, lever = Position(lever), target = Position(focusWirePos), interpreter = Format(native.Get(focusWirePos)), compiled = Format(compiled.Get(focusWirePos)), focusSources = focusSourcePositions.Select(pos => new { position = Position(pos), state = Format(compiled.Get(pos)) }).ToArray(), interpreterPending = Scheduled(native.Ticks.Pending), compiledPending = Scheduled(engine.GetPendingTicks().Ticks) };
        }
        if (!Logic.Use(native, lever) || !engine.Use(lever)) throw new InvalidOperationException($"MAIR rejected lever {action.Index} at {lever}");
        engine.Flush();
        var postUseDifferences = positions.Where(pos => Differs(native.Get(pos), compiled.Get(pos)))
            .Select(pos => new { position = Position(pos), interpreter = Format(native.Get(pos)), compiled = Format(compiled.Get(pos)) }).Take(8).ToArray();
        if (report.FirstPostUseDifference is null && postUseDifferences.Length > 0)
            report.FirstPostUseDifference = new { actionStep = action.Step, leverIndex = action.Index, lever = Position(lever), differences = postUseDifferences };
        if (probePositions.Length > 0 && (id != "b6a6e870229ace7eb2afd8cb387a514e" || action.Step <= 1) && (id != "8111b000090d8f0b3e1370448bcb64a3" || action.Step == 39))
            report.ProbeCheckpoints.Add(Probe(action.Step, "post-use", 0));
        for (var tick = 1; tick <= action.Wait; tick++)
        {
            native.Tick();
            engine.Tick();
            engine.Flush();
            tickTotal++;
            if (probePositions.Length > 0 && (id != "b6a6e870229ace7eb2afd8cb387a514e" || action.Step <= 1) && (id != "8111b000090d8f0b3e1370448bcb64a3" || action.Step == 39))
                report.ProbeCheckpoints.Add(Probe(action.Step, "tick", tick));
            var differences = new List<object>();
            foreach (var pos in positions)
            {
                var expected = native.Get(pos);
                var actual = compiled.Get(pos);
                if (Differs(expected, actual)) differences.Add(new { position = Position(pos), interpreter = Format(expected), compiled = Format(actual) });
            }
            report.Timeline.Add(new { actionStep = action.Step, leverIndex = action.Index, lever = Position(lever), wait = action.Wait, actionTick = tick, totalTicks = tickTotal, differenceCount = differences.Count });
            if (report.FirstDifference is null && differences.Count > 0)
            {
                report.FirstDifference = new { actionStep = action.Step, leverIndex = action.Index, lever = Position(lever), wait = action.Wait, actionTick = tick, totalTicks = tickTotal, differences = differences.Take(8).ToArray() };
            }
        }
    }
}

var json = JsonSerializer.Serialize(report, new JsonSerializerOptions { WriteIndented = true });
if (outputPath is not null) File.WriteAllText(outputPath, json);
Console.WriteLine(json);

static int[] Position(BlockPos pos) => [pos.X, pos.Y, pos.Z];

sealed class ActionEntry
{
    [JsonPropertyName("step")] public int Step { get; set; }
    [JsonPropertyName("index")] public int Index { get; set; }
    [JsonPropertyName("wait")] public int Wait { get; set; }
}

sealed class Report
{
    public string FixtureId { get; set; } = "";
    public string? Description { get; set; }
    public string Schematic { get; set; } = "";
    public string SchematicSha256 { get; set; } = "";
    public string DllSha256 { get; set; } = "";
    public string? DllVersion { get; set; }
    public string DllLocation { get; set; } = "";
    public int Width { get; set; }
    public int Height { get; set; }
    public int Length { get; set; }
    public int LoadedBlocks { get; set; }
    public int SkippedStates { get; set; }
    public int LoadedContainers { get; set; }
    public int LoadedComparators { get; set; }
    public int LeverCount { get; set; }
    public int[][] LeverPositions { get; set; } = [];
    public int CompiledNodeCount { get; set; }
    public int CompiledWireCount { get; set; }
    public int TrackedPositionCount { get; set; }
    public double CompileMilliseconds { get; set; }
    public ActionEntry[]? Actions { get; set; }
    public object? FirstDifference { get; set; }
    public object? FirstPostUseDifference { get; set; }
    public object? FocusWireSources { get; set; }
    public object? PreInputCheckpoint { get; set; }
    public List<object> ProbeCheckpoints { get; set; } = [];
    public List<object> Timeline { get; set; } = [];
}





