# Graph trace build

Pass summaries and runtime graph events use tracing. Their detailed events are compiled only with `graph-trace`; ordinary builds keep the default static logging caps.

PowerShell server invocation:

```powershell
$env:MCHPRS_LOG = 'mchprs_core::redpiler::graph=trace,mchprs_core::redpiler::passes=debug'
cargo run -p mroww --release --no-default-features --features graph-trace
```

The server reads `MCHPRS_LOG`, defaults to `info`, and writes to its existing log file and stdout. The no-default-features build drops the root crate's `max_level_debug` and `release_max_level_info` caps so these events can reach the subscriber. Keep trace builds separate from runtime benchmarks.
