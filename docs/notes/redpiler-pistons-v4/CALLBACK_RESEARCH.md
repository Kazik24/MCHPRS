# Callback compilation and runtime cost

Status: historical. The `graph-profile` instrumentation and commands in this report have been removed; these measurements remain as completed research results.

The callback table is large because it is a compiled description of ordered native notifications for every possible owner and contact state. On the active CPU_BubbleSort episode, the clearest repeated runtime cost is narrower: Turbo wire propagation scans whole per-wire candidate routes to find one target. O0 examined 200.6 million rows for 8.11 million lookups; only 8.81 million examined rows matched the requested target. The first useful follow-up is to index those candidates while preserving their route order and guards.

This is a profiling and inventory report. It does not rewrite callback behavior. Census hooks and the extra `Route::native_event_count` field exist only with `graph-profile`; ordinary compilation and execution collect none of these counters.

## How the compiled callbacks work

Preparation has four relevant steps:

1. [Geometry](../../../crates/core/src/redpiler/analysis/geometry.rs) records compiled cells, piston contacts, possible profiles, and fixed structural relationships.
2. [Electrical preparation](../../../crates/core/src/redpiler/analysis/electrical.rs) assigns nodes and sites, builds power and wire-query plans, and creates guards for changing contact facts.
3. [Callback preparation](../../../crates/core/src/redpiler/analysis/callbacks.rs#L110) builds ordered event templates and expands each event into route deliveries. Component, wire, and piston routes are built separately at [L240](../../../crates/core/src/redpiler/analysis/callbacks.rs#L240), [L276](../../../crates/core/src/redpiler/analysis/callbacks.rs#L276), and [L301](../../../crates/core/src/redpiler/analysis/callbacks.rs#L301).
4. The pass pipeline changes the shared draft, then [lowering](../../../crates/core/src/redpiler/program_graph.rs#L575) validates typed references and callback-link correspondence. `CircuitProgram` keeps ordered routes but drops draft callback links.

The record flow is:

```text
native notification occurrence
  -> Event(position, kind, direction/filter, condition, solid-at, live/captured view)
  -> zero or more profile/contact branches
  -> guarded RouteDelivery rows with one shared sequence per occurrence
  -> LinkSpec rows for deliveries with an explicit receiver
  -> validated route used by Graph, plus draft links for validation/inspection
```

An event sequence is assigned before filtering and branch expansion in [`compile_route`](../../../crates/core/src/redpiler/analysis/callbacks.rs#L375). One event can therefore produce several deliveries with the same sequence, or no delivery at all. Sequence numbers are route-local provenance, not dense delivery indices; gaps are expected. Repeated native notifications keep separate sequence numbers even when they target the same position.

`branches` expands contact sites over the profiles compiled for that contact and attaches either a live profile guard or a captured profile test. It combines those with any event condition and chooses the receiver from the block/profile and event kind ([branch expansion](../../../crates/core/src/redpiler/analysis/callbacks.rs#L515), [filters](../../../crates/core/src/redpiler/analysis/callbacks.rs#L631), [guard combination](../../../crates/core/src/redpiler/analysis/callbacks.rs#L752)). A literal `receiver: None` is not always an effective no-op: for a component-site Update, runtime supplies the component receiver fallback. The runtime checks captured operands, resolves that fallback, then visits the handler in route order ([delivery loop](../../../crates/core/src/redpiler/backend/graph.rs#L3551)).

The records have different jobs:

| Record or operation | Meaning | Runtime role |
| --- | --- | --- |
| Native notification occurrence | One ordered call site in a native notification procedure. | The compiled sequence preserves order and multiplicity. |
| Guarded alternatives | The possible target/contact profiles for one occurrence. | Guards decide which row applies at that instant. |
| Repeated notification | A later native call to the same or another position. | It remains a separate ordered occurrence; a prior call may have changed state or scheduled work. |
| Turbo candidate row | A stored possible receiver for one canonical wire neighbor. | Lookup data; it is not dispatched just because it is stored. |
| Dispatched callback | A passing row with an effective receiver. | It increments the callback count and invokes the component or piston handler. |
| No-op handler visit | A dispatched callback whose receiver handler has no visible state change. | It can still affect scheduling, ordering, observation, or host behavior, so unchanged state alone is not a removal proof. |
| `LinkSpec` / callback link | A source-to-receiver connectivity and validation view of explicit deliveries. | Used by transformations, validation, JSON inspection, and viewers; not retained by `CircuitProgram` and not traversed by ticks. |

The draft callback links are validated against route/sequence multiplicity at [`validate_links`](../../../crates/core/src/redpiler/program_graph.rs#L1591). JSON exports the post-pass `callback_links` and ordered `routes` for inspection ([export construction](../../../crates/core/src/redpiler/analysis/export_json.rs#L1130)); the Python loader and graph viewer consume those tables. The lowered runtime program retains routes, not links. The CPU exports report 1,973,211 draft links at both O0 and O1, approximately 48 MiB in the draft table estimate, but that storage is released at lowering. It is not a tick-time table.

Three concrete route examples from the CPU loader coordinates show the distinction:

| Route | Owner position and role | Native event occurrences | O0 rows | O1 rows | What the rows show |
| --- | --- | ---: | ---: | ---: | --- |
| 6208 | lever `(152,25,121)`, component output | 36 | 36 | 33 | `control_events` concatenates two 18-occurrence neighborhoods: the lever and its support. O0 has 30 rows with no effective receiver and 33 literal `None` receivers; three literal `None` Update rows use the component fallback. O1 leaves three event sequences empty after its no-op proof. |
| 1425 | repeater `(173,14,160)`, component output | 7 | 7 | 7 | `diode_notifications` visits the front block and then its six neighbors. Four rows have no effective receiver. |
| 11535 | wire `(176,9,7)`, shape route | 42 | 51 | 51 | `wire_update_notifications` emits six direct and 36 indirect occurrences. Contact/profile expansion adds nine rows for this wire. Thirty-nine rows have no effective receiver. |
| 11536 | same wire, Turbo candidates | 24 | 30 | 30 | One stored candidate event per canonical neighbor; profile expansion adds six rows. Runtime tests these candidates during a wire walk; it does not dispatch all 30. |
| 94247 | piston `(176,9,227)`, base changed | 18 | 18 | 14 | Six native occurrences have no compiled branch in O0. Two other occurrences expand to four alternatives each. O1 removes four proven identity rows, leaving ten empty sequences. |
| 94249 | same piston, base restored | 19 | 21 | 17 | Restoration prepends an update of the restored base to the 18-event notification sequence. O0 has six empty sequences and two four-row alternatives; O1 leaves ten empty sequences. |

The order comes from the native helpers, not coordinate sorting. Controls use `surrounding_notifications`, which intentionally retains repeated positions and directions ([helper](../../../crates/core/src/redstone/mod.rs#L510)). Diodes visit the front and six neighboring blocks ([helper](../../../crates/core/src/redstone/mod.rs#L158)). Wire changes visit each of six neighbors and each of that neighbor's six offsets ([helper](../../../crates/core/src/redstone/mod.rs#L467)). Piston notification routes follow shape events, observer-only updates, and then neighbor updates ([route construction](../../../crates/core/src/redpiler/analysis/callbacks.rs#L829), [native piston helpers](../../../crates/core/src/redstone/piston.rs#L463)). These produce duplicate positions by design; the route sequence keeps each occurrence distinct.

During execution, `deliver_route_inner` scans a route in order, evaluates each guard, checks captured observations, resolves any component fallback, and dispatches only then. Turbo propagation is a separate path: `wire_route_delivery` scans a wire's ordered candidate route until the first row with the requested target, a captured receiver, and a passing guard; `dispatch_turbo_site` then uses the receiver captured in the wire walk ([Turbo lookup](../../../crates/core/src/redpiler/backend/graph.rs#L2421), [walk dispatch](../../../crates/core/src/redpiler/backend/graph.rs#L2550)).

## Stored-record census

The CPU input uses the pinned schematic SHA-256 `803dbe5d6751b152bea9b134d6c8571cfafa3e840e372f82e41a535b658f1611`. For this approved short episode, the same sign-free in-memory variant was loaded separately for Graph and interpreter; the sign at loader position `(188,10,186)` was replaced by air in both worlds. The schematic and its hash were not changed.

CPU counts below come from the post-pass compiled artifact. Route counts are `route instances / RouteDelivery rows`; the role counts come from owning component plans (`output_route`, `WirePlan`, and `PistonRoutes`), not strings stored on every delivery.

| Route owner and role | O0 routes / rows | O1 routes / rows |
| --- | ---: | ---: |
| Component output, all owners | 11,535 / 78,160 | 11,535 / 77,366 |
| ├ observer | 11,229 / 73,647 | 11,229 / 73,025 |
| ├ repeater | 235 / 2,041 | 235 / 2,041 |
| ├ stone button | 52 / 1,998 | 52 / 1,830 |
| ├ redstone wall torch | 15 / 378 | 15 / 377 |
| ├ redstone torch | 3 / 60 | 3 / 60 |
| └ lever | 1 / 36 | 1 / 33 |
| Wire shape | 41,354 / 1,951,644 | 41,354 / 1,948,569 |
| Wire Turbo candidates | 41,354 / 1,165,711 | 41,354 / 1,163,685 |
| Piston base changed | 16,561 / 331,492 | 16,561 / 277,262 |
| Piston near changed | 16,561 / 300,600 | 16,561 / 238,841 |
| Piston far changed | 16,496 / 361,152 | 16,496 / 308,078 |
| Piston far shape changed | 16,496 / 182,473 | 16,496 / 129,400 |
| Piston base restored | 16,561 / 381,175 | 16,561 / 326,945 |
| Piston head restored | 16,561 / 300,600 | 16,561 / 238,841 |
| Piston resource restored near | 16,496 / 365,544 | 16,496 / 304,045 |
| Piston resource restored far | 16,496 / 394,144 | 16,496 / 341,070 |
| **Total** | **226,471 / 5,812,695** | **226,471 / 5,354,102** |

Component route owners are mostly observers. The CPU has 11,229 observer routes, compared with 235 repeater routes and 71 routes across the remaining controls and torches. Each wire owns two routes: the 42-event shape template and the 24-slot Turbo candidate template. Each single-resource piston owns up to eight separate change/restoration routes. Those templates are compiled per actor because their sources, target sites, direction, guards, sequence provenance, and selected contacts differ.

| Static delivery classification | O0 | O1 |
| --- | ---: | ---: |
| Update / Shape | 4,489,095 / 1,323,600 | 4,483,005 / 871,097 |
| Live / Captured view | 4,646,984 / 1,165,711 | 4,190,417 / 1,163,685 |
| Literal `receiver == None` | 3,839,484 | 3,380,891 |
| Effective no receiver after component-Update fallback | 3,833,394 | 3,380,891 |
| Effective component receiver: control / observer / other / repeater / torch / wire | 392 / 72,198 / 5,894 / 2,496 / 265 / 644,858 | 0 / 72,198 / 196 / 2,496 / 265 / 644,858 |
| Contact / piston base / piston head receiver | 772,699 / 307,162 / 173,337 | 772,699 / 307,162 / 173,337 |

O1 contains 458,593 fewer rows. The static type difference is 452,503 rows with no effective receiver, 5,698 redstone-block/constant rows resolved to an inert component handler, and 392 control rows. Those removed rows all have literal `receiver == None`, so the callback-link count remains unchanged. Opt-01's exact classifier is [`identity_handler`](../../../crates/core/src/redpiler/passes/simplify_guards.rs#L435): it also removes impossible guards and permits only Update deliveries to lever, button, pressure-plate, or constant component nodes, plus receiver-less Shape deliveries to constant sites. It does not remove wire, comparator, observer, piston, command, or note callbacks.

Branching and guards account for a second multiplier. The profile-only inventory records all native event groups before pass removals, including groups that produce zero rows:

| Rows per native sequence | O0 groups | O1 groups |
| ---: | ---: | ---: |
| 0 | 809,711 | 1,268,304 |
| 1 | 3,618,495 | 3,159,902 |
| 2 | 193,181 | 193,181 |
| 3 | 231,798 | 231,798 |
| 4 | 278,111 | 278,111 |

There are 703,402 contact occurrences: 312 reference one profile, 193,181 reference two, 231,798 reference three, and 278,111 reference four. Those multi-profile alternatives account for 1,491,110 rows beyond a single row per occurrence. The inventory found no exact duplicate alternatives within one sequence group. These are syntactic alternatives, not a proof that only one guard can pass throughout a nested execution.

| Guard kind | O0 | O1 |
| --- | ---: | ---: |
| Always | 66 | 1 |
| State | 416,554 | 158,659 |
| Site | 1,620,726 | 280,363 |
| Not | 947,731 | 165,810 |
| All | 691,040 | 159,900 |
| Any | 82,805 | 49,683 |
| Guard records referenced more than once | 201,169 | 310,045 |
| Guard references counted by the inventory walk | 8,862,097 | 6,305,348 |
| Guard expressions larger than four visited nodes | 482,505 | 122,508 |
| Largest guard expression | 19 nodes | 19 nodes |

Routes have median sizes of 22 rows at O0 and 19 at O1; p90 is 44 for both, p99 is 60, and the maximum is 69 at O0 / 68 at O1. Twelve O0 routes tie at 69 rows; for example, wire shape route 39043 belongs to `(178,18,168)`. This is a loader-local position. The full histogram and the largest route owners are in the inventory emitted by the test command below.

The inventory also found 26,695 exact duplicate route bodies out of 226,471 route instances, leaving 199,776 unique bodies. The bodies contain 521,186 duplicate delivery rows at O0 and 429,805 at O1. At 24 bytes per delivery row, those rows represent about 11.93 MiB / 9.84 MiB of duplicate payload before adding shared-slice pointers/refcounts and accounting for allocation layout. Sharing would save memory only; it would not remove any execution.

Small fixture inventories provide a useful check that the CPU is not the only shape represented:

| Fixture and mode | Component routes / rows | Wire shape routes / rows | Turbo candidate routes / rows | Piston routes / rows | Total routes / rows | Contact occurrences; 2/3/4-profile groups |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| `opposing_shared_pistons` O0 | 0 / 0 | 0 / 0 | 0 / 0 | 16 / 318 | 58; 14 / 14 / 30 |
| `opposing_shared_pistons` O1 | 0 / 0 | 0 / 0 | 0 / 0 | 16 / 250 | 58; 14 / 14 / 30 |
| `adder_1bit` O0 | 16 / 226 | 32 / 1,569 | 32 / 922 | 104 / 2,130 | 184 / 4,847 | 606; 146 / 193 / 265 |
| `adder_1bit` O1 | 16 / 212 | 32 / 1,560 | 32 / 916 | 104 / 1,794 | 184 / 4,482 | 606; 146 / 193 / 265 |
| `TEST_BUD_1` O0 | 7 / 127 | 11 / 484 | 11 / 280 | 12 / 211 | 41 / 1,102 | 57; 12 / 26 / 15 |
| `TEST_BUD_1` O1 | 7 / 121 | 11 / 484 | 11 / 280 | 12 / 165 | 41 / 1,050 | 57; 12 / 26 / 15 |

The shared-resource fixture has two pistons and one resource with three contacts; its route inventory has two instances for each of the eight piston roles. Each fixture has zero exact duplicate alternatives within an event group. The full emitted JSON includes per-role route sizes, owner types and representative positions.

The same small-fixture inventory reports delivery kinds/views and receiver families:

| Fixture and mode | Update / Shape | Live / Captured | Effective receiver-family counts | Literal `None` |
| --- | ---: | ---: | --- | ---: |
| `opposing_shared_pistons` O0 | 156 / 162 | 318 / 0 | contact 94; none 194; base 16; head 14 | 194 |
| `opposing_shared_pistons` O1 | 156 / 94 | 250 / 0 | contact 94; none 126; base 16; head 14 | 126 |
| `adder_1bit` O0 | 3,767 / 1,080 | 3,925 / 922 | control 23; observer 75; other 12; repeater 23; wire 481; contact 655; none 3,132; base 284; head 162 | 3,164 |
| `adder_1bit` O1 | 3,735 / 747 | 3,566 / 916 | observer 75; other 3; repeater 23; wire 481; contact 655; none 2,799; base 284; head 162 | 2,799 |
| `TEST_BUD_1` O0 | 998 / 104 | 822 / 280 | control 6; observer 9; repeater 5; torch 12; wire 128; contact 49; none 847; base 34; head 12 | 853 |
| `TEST_BUD_1` O1 | 992 / 58 | 770 / 280 | observer 9; repeater 5; torch 12; wire 128; contact 49; none 801; base 34; head 12 | 801 |

Guard inventory for the same fixtures uses kind order Always / State / Site / Not / All / Any:

| Fixture and mode | Guard counts by kind | Total | Shared records | Expressions >4 nodes / largest |
| --- | --- | ---: | ---: | ---: |
| `opposing_shared_pistons` O0 | 1 / 48 / 0 / 9 / 16 / 10 | 84 | 17 | 12 / 13 |
| `opposing_shared_pistons` O1 | 1 / 18 / 0 / 9 / 11 / 6 | 45 | 26 | 10 / 13 |
| `adder_1bit` O0 | 1 / 326 / 1,284 / 734 / 576 / 65 | 2,986 | 169 | 413 / 19 |
| `adder_1bit` O1 | 1 / 124 / 243 / 135 / 138 / 39 | 680 | 261 | 113 / 19 |
| `TEST_BUD_1` O0 | 2 / 44 / 387 / 239 / 152 / 10 | 834 | 30 | 106 / 19 |
| `TEST_BUD_1` O1 | 1 / 17 / 50 / 30 / 19 / 6 | 123 | 39 | 16 / 16 |

Exact duplicate route bodies / unique bodies were 5 / 11 for the shared-piston fixture, 22 / 162 for `adder_1bit`, and 2 / 39 for `TEST_BUD_1` in both O0 and O1. Each of these inventories found zero byte-identical alternatives within a single native occurrence.

## Runtime work on the 100-tick episode

The release profiling run uses one fresh sign-free world for each Graph mode and one fresh matching interpreter world. Preparation, the starting use action, RAM reads, flush, inventory, and reporting are outside the timed interval. Counter snapshots are taken immediately before and after exactly 100 `tick_with_world` / interpreted tick calls. The command was:

```powershell
cargo bench -p mchprs_core --features graph-profile --bench cpus --locked -- --cpu cpu_bubblesort --graph-short --iterations 1 --output target/callback-cpu-profile.json
```

The file under `target/` is a generated measurement artifact, not a checked-in fixture. The benchmark asserts Graph backend identity and checks RAM equality with the interpreter after the 100 ticks.

| Metric over the tick window | O0 Graph | O1 Graph |
| --- | ---: | ---: |
| Instrumented sample time, one run | 1.895445 s | 1.732064 s |
| Graph callback dispatches in the 100 ticks | 2,059,904 | 2,057,490 |
| Callback count including starting use before the window | 2,059,909 | 2,057,492 |
| Scheduled tick dispatches | 20,005 | 20,005 |
| General route calls | 191,996 | 191,996 |
| General route delivery rows examined | 4,047,834 | 3,499,483 |
| Guard roots evaluated / failures | 12,445,448 / 4,246,121 | 11,704,471 / 4,179,850 |
| Recursive guard operations | 30,346,448 | 29,255,884 |
| Passing rows with no effective receiver | 1,938,557 | 1,392,620 |
| Missing captured guard/delivery operands | 0 / 0 | 0 / 0 |
| Component/wire/command/note state writes that changed stored values | 427,944 | 427,944 |
| Tick schedule attempts / new scheduler insertions | 20,730 / 20,730 | 20,730 / 20,730 |
| Wire walks / nested Turbo wire-node evaluations | 32,505 / 1,039,496 | 32,505 / 1,039,496 |
| Piston sample wrapper calls / early pending-base returns | 317,191 / 14,407 | 317,191 / 14,407 |
| Actual piston samples / accepted actions / deferred publications | 302,784 / 27,884 / 27,884 | 302,784 / 27,884 / 27,884 |
| Final RAM equals interpreter | yes | yes |

Dispatches by receiver and kind were nearly identical across modes:

| Receiver | O0 Update / Shape | O1 Update / Shape |
| --- | ---: | ---: |
| Component | 1,375,300 / 106,200 | 1,372,886 / 106,200 |
| Piston base | 213,243 / 0 | 213,243 / 0 |
| Piston head | 103,948 / 0 | 103,948 / 0 |
| Contact | 0 / 261,213 | 0 / 261,213 |

The work profile classifies handler invocations in the same window as follows. Piston entries count the sample wrapper, so subtract the separately counted 14,407 pending-base early returns to get the 302,784 actual samples.

| Handler family | O0 | O1 |
| --- | ---: | ---: |
| Observer | 124,372 | 124,372 |
| Wire | 312,086 | 312,086 |
| Repeater | 1,456 | 1,456 |
| Comparator | 0 | 0 |
| Torch | 1,320 | 1,320 |
| Lamp / copper bulb / command / note | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| Control | 99 | 0 |
| Piston wrapper | 317,191 | 317,191 |
| Other | 2,671 | 356 |

Guard operations by kind are recursive visits, not just top-level route guards:

| Kind | O0 | O1 |
| --- | ---: | ---: |
| Always | 4,317,453 | 3,722,056 |
| State | 12,975,430 | 12,737,473 |
| Site | 398,459 | 398,459 |
| Not | 3,364,874 | 3,302,174 |
| All | 7,109,230 | 6,971,543 |
| Any | 2,181,002 | 2,124,179 |

The phase counters identify where route calls originate and which route roles dispatch handlers. Rows use the route-role names in the static table above; a `wire_turbo_candidates` dispatch is a callback reached from a Turbo walk, not a traversal of every stored candidate row.

The sum of profile dispatches equals the `GraphExecutionCounts.callbacks` delta for the tick window. The cumulative callback totals include the preceding starting-use action: five callbacks at O0 and two at O1. Schedule insertions and due dispatches also have different boundaries: 20,730 new entries were inserted during the window, while 20,005 scheduled ticks became due and ran; future-deadline work remains pending at the end of the episode.

| Phase | Route calls by role (O0 = O1) | O0 / O1 dispatches by role |
| --- | --- | --- |
| Scheduled ticks | component output 19,989 | component output 5,703 / 5,676; Turbo dispatch 83,313 / 83,313; totals 89,016 / 88,989 |
| Piston actions | wire shape 10,220; base changed 27,884; near changed 41,231; far changed 14,290; far shape changed 13,331; resource-restored-far 3; total 106,959 | wire shape 114,852 / 113,806; Turbo 903,969 / 903,969; base changed 120,823 / 120,823; near changed 129,960 / 129,750; far changed 57,681 / 57,679; far shape 33,903 / 33,903; resource-restored-far 9 / 9; totals 1,361,197 / 1,359,939 |
| Deferred connections | wire shape 9,562; base restored 14,423; head restored 13,445; resource near 14,290; resource far 13,328; total 65,048 | wire shape 103,134 / 102,147; Turbo 286,181 / 286,181; base restored 69,504 / 69,504; head restored 47,123 / 47,053; resource near 49,961 / 49,891; resource far 53,788 / 53,786; totals 609,691 / 608,562 |

Turbo lookup dominates raw row-search work:

| Turbo lookup metric | O0 | O1 |
| --- | ---: | ---: |
| Lookups | 8,109,878 | 8,109,878 |
| Candidate rows inspected | 200,563,785 | 200,294,310 |
| Inspected rows matching the requested target | 8,809,078 | 8,793,727 |
| Inspected rows for other targets | 191,754,707 | 191,500,583 |
| Share of inspected rows for other targets | 95.61% | 95.61% |
| First matching candidate found / no matching candidate | 1,273,463 / 6,836,415 | 1,273,463 / 6,836,415 |

The lookup first checks target equality, receiver presence, captured view, and then its guard. Thus an index that preserves only the ordered rows for the requested canonical neighbor can avoid scanning unrelated targets without dropping callback alternatives. The `turbo_target_entries_scanned` figure counts rows whose target matched before the current search stopped; it is not an estimate of callbacks.

Inclusive phase timings from the same one-sample instrumented run were:

| Bucket | O0 | O1 |
| --- | ---: | ---: |
| Tick phases | 1,895.4 ms | 1,732.1 ms |
| Scheduled drain | 110.9 ms | 97.9 ms |
| Piston actions | 1,174.1 ms | 1,086.3 ms |
| Deferred connections | 610.4 ms | 547.8 ms |
| Route delivery | 2,402.5 ms | 2,197.9 ms |
| Turbo wire walk | 1,429.6 ms | 1,305.6 ms |
| Piston sample | 288.0 ms | 265.1 ms |

These buckets overlap: piston action and deferred-connection phases include nested route and Turbo work, and route delivery can itself include nested callbacks. Do not sum them or interpret them as exclusive handler self-time. `graph-profile` adds per-operation counter updates; the extra Turbo target count tests every scanned row. These one-run times are for locating work, not production latency claims or a stable O0/O1 speed comparison. The interpreter took 0.654655 s in this same single run; its counters are not instrumented by this feature.

The same invocation also ran O1+dust at 1.793574 s. The pass saw 5,017 candidate groups but accepted zero paths, covered zero wires, and elided zero query samples. That timing difference cannot be attributed to dust contraction.

## Retained-table accounting

The table estimator sums vector capacity times element size plus known nested guard/contact storage. It is not RSS, allocator overhead, compilation peak, or total process memory. `graph-profile` adds an 8-byte `native_event_count` to each `Route`; subtracting that profile-only field gives the approximate ordinary-build figures below.

| Stage | O0 profile build | O1 profile build | Approximate ordinary build |
| --- | ---: | ---: | ---: |
| After callback preparation | 397.08 MiB | 397.08 MiB | 395.08 MiB for either mode before passes |
| After lowering | 269.99 MiB | 186.99 MiB | 268.26 MiB O0 / 185.26 MiB O1 |

At lowering, deliveries and guards dominate: O0 retains 133.04 MiB of delivery rows and 92.74 MiB of guards; O1 retains 122.55 MiB of delivery rows and 20.37 MiB of guards. O1 reduces delivery rows by 458,593 and guard records from 3,758,922 to 814,416. The route table and wire-plan counts are unchanged. Draft callback links occupy about 48 MiB at the callbacks stage and are absent after lowering. Delivery payload is counted once per stage; the `lowering input` snapshot is intentionally not added to the `lowering` snapshot.

The parser-stage estimate is materially larger than lowered runtime retention and is the relevant bounded-preparation accounting. Neither this report nor the short profile explains the historical ~42 GiB preparation report; that figure was not reproduced and is not treated as current memory use.

## What could safely change next

| Rank | Proposal and measured surface | Required proof / positive case | Near-match that must remain general | Runtime memory tradeoff |
| ---: | --- | --- | --- | --- |
| 1 | **Index Turbo candidates by canonical target slot.** Covers 41,354 wires and 8.11 million active lookups; 95.61% of examined rows are unrelated targets. Keep every row and its within-target order. | Candidate event groups are contiguous because each canonical-neighbor event is compiled before its profile branches. Carry the canonical slot through `TURBO_ORDER`, then inspect exactly that slot's range with the same captured observation, receiver-presence, and live guard checks. Compare O0/current O1/interpreter on all headings, contact alternatives, nested walks, misses, and observer-visible intermediates. CPU_BubbleSort is a positive workload. | A requested site with multiple profile rows where an earlier guard fails and a later one passes; a miss must stay a miss. Preserve repeated walk visits, layer/heading updates, and `observed.receiver` dispatch. Do not cache a live selector. | A 25-offset `u32` boundary table per wire would be about 3.95 MiB for 41,354 wires, roughly 2.1% of the 185 MiB O1 lowered table estimate. It retains routes and callbacks; it only avoids unrelated scans. Validate route length fits the offset type (the event sequence is already `u32`). |
| 2 | **Remove proven inert general-route rows.** 1.94 million O0 and 1.39 million O1 rows pass their guards in the CPU window and still have no effective receiver; static inventories contain 3.83 million / 3.38 million such rows. | Resolve the same component-Update fallback as Graph first. Remove only rows whose effective receiver is absent for every compiled view/profile and whose guard read is pure; show they do not feed a link, queue admission, site capture, route ordering state, support validation, or an exported runtime dependency. Retain sequence provenance for surviving rows. | A literal `None` component Update that falls back to a component handler; a contact with multiple profile alternatives where a later row can dispatch; any Turbo candidate used for first-match selection; meaningful Shape/support behavior. | Could reduce stored route rows and guard evaluations. Coverage is high but no exact byte estimate is safe until eligible rows are classified by owning site and role. |
| 3 | **Group contact alternatives or factor predicates.** 703,402 occurrences have 2–4 profile references; those alternatives contribute 1,491,110 extra static rows beyond one per occurrence. | Prove guards are mutually exclusive for the complete allowed state/view domain and preserve current sequential evaluation, route sequence, callback multiplicity, and nested state changes. Sharing a pure guard DAG is safer than merging rows; O1 already substantially shares/simplifies guards. | A nested callback changes a live contact selector between alternatives, or alternatives have distinct receiver, captured view, direction, or event state. Do not snapshot live selection at route entry. | Guard factoring may cut retained expressions; row grouping's runtime/work benefit is unproven. No implementation recommended yet. |
| 4 | **Share exact route bodies.** There are 26,695 duplicate route instances, representing 521,186 O0 / 429,805 O1 duplicate delivery payload rows. | Share only byte-identical ordered bodies while keeping each route's source, role, ID, and all references distinct; preserve export materialization. Check hash equality, not hash alone. | Same target sequence but different guard IDs, receiver, view, direction, provenance, or an intentional duplicate occurrence. | At most about 11.93 MiB O0 / 9.84 MiB O1 of delivery payload before shared-allocation overhead. Memory only; it saves no callbacks or search work. |

The first proposal has the clearest local proof and largest measured active-work coverage. A compact index should point into the existing ordered route rather than replace it. `wire_cache::positions` supplies 24 distinct canonical offsets; the builder records route sequence in that order. Thread the slot through the existing heading permutation, preserve each slot's branch range as stored, and retain the current guard evaluation at lookup time. A regression should deliberately put a failing contact alternative before a passing one and assert the selected observed receiver, miss behavior, nested callback order, state, pending work, and reset continuation against general Graph and the interpreter. Then rerun the 100-tick release benchmark without profiling counters and report row reduction, memory overhead, and paired timing separately.

### Receiver identity rules

| Receiver family / event | Current ruling | Why a same-value/no-op observation is insufficient |
| --- | --- | --- |
| Lever, button, pressure plate Update | Opt-01 may remove the neighbor Update because the compiled component handler is identity; user use/press remains its separate input path. | Only remove after resolving the component fallback and preserving route provenance/link multiplicity. |
| Constant-site Update or inert Shape | Opt-01 removes only the classified identity form; callback preparation rejects meaningful un-inventoried update/attachment behavior. | A literal `None` can still fall back for a component Update, and Shape can carry support semantics. |
| Wire Update/Shape and Turbo callbacks | Keep. | Recomputes stored power/sides, conducts signals, and can expose intermediate changes to observers or later walk steps. |
| Comparator | Keep. | Rear/side sampling, subtract/compare mode, saved output, and same-value notifications can feed feedback and observers. |
| Repeater and torch | Keep. | Scheduling, deadline replacement, and delayed feedback are effects even when current output is unchanged. |
| Observer and copper bulb | Keep. | Intermediate notifications, pending cooldowns, and latch transitions are observable. |
| Command and note | Keep. | Command result/output and note sound eligibility are effects outside a simple power comparison. |
| Piston base/head/contact | Keep. | Samples, request ordering, resource selection, accepted actions, and deferred publications can matter without an immediate block-state change. |

The runtime `deliveries_without_effective_receiver` counter is taken only after a route guard passes and component fallback is resolved. The O0/O1 static `literal_receiver_none` counts include component Update rows that receive a runtime fallback; those figures must not be compared as if they were equivalent.

## Reproduction and limits

Focused discovery and reproduction commands:

```powershell
cargo test -p mchprs_core --lib redpiler:: --features graph-profile --locked -- --list
cargo test -p mchprs_core --lib callback_research_inventory --features graph-profile --locked -- --test-threads=1
cargo test -p mchprs_core --lib callback_profile_matches_dispatched_graph_callbacks_for_an_ordered_route --features graph-profile --locked -- --test-threads=1
cargo test -p mchprs_core --lib reference_contract --locked -- --test-threads=1
```

The inventory tests compile O0 and O1 for `opposing_shared_pistons`, `adder_1bit`, and `TEST_BUD_1`; append `--nocapture` to print their full JSON summaries. The profile invariant test checks dispatched-callback totals against the existing Graph work counter. `reference_contract` protects callback order/direction, duplicates and continuation contracts. `graph-profile` fields and inventory functions are cfg-gated; ordinary builds do not maintain the added work counters or route event count.

In this checkout the discovery command listed 233 `redpiler::` tests. The three inventory tests, one profile invariant test, and seven reference-contract tests passed. `cargo check -p mchprs_core --tests --locked`, `cargo check -p mchprs_core --benches --tests --features graph-profile --locked`, `cargo fmt -p mchprs_core -- --check`, and `git diff --check` also passed.

Limits: CPU runtime counts are one episode of the first 100 active ticks, not a complete sort or all possible histories. Interpreter RAM matched at that checkpoint; that does not prove full CPU callback-trace equivalence. No RSS/peak allocator measurement, profiler flame graph, broad fuzz campaign, or uninstrumented five-pair performance comparison was run for this census. Shared-resource, adder, and BUD fixtures were inventoried but not tick-profiled here. The candidate-range idea is a proposal only; it has not been implemented or differentially verified.
