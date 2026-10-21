# Generic logical compiler findings

## Transition-cache experiment

The proposed O6 transition-learning policy reopened learning after successful
replays. It is removed: a 60,000-tick COUNTER trace under that policy reported
88,594 misses and 86,071 learned programs (with 1,129,963 hits), indicating
substantial compile/evict churn. A preliminary O6 run during concurrent builds
was 8,778 TPS and is not comparable. Three quiet alternating runs later
measured O5 at a 12,308.7 TPS median and O6 at 10,619.1 TPS, about 14% slower.
The policy therefore has no production hook; the baseline bounded lifetime
attempt limit remains. These figures do not establish that all transition
specialization is unhelpful.

## Callback-cut safety

A proposed optimization treated wire notifications that did not lie on the
clock's direct power path as irrelevant. That cut is unsound without a stronger
proof. Notifications can invoke guarded captures, resample component inputs,
and change which event is first queued. `schedule_with` preserves the first
pending tick, so treating notification order as a stutter can change both due
time and FIFO behavior. The observed clock-to-storage callback path crosses a
conductor gap, but that topology alone does not prove the callbacks inert.
That prototype and its controller closure were removed. These ordering concerns
apply to observable boundary effects; they do not require retaining private
propagation and reset work in the new logical executor.

## Smallest general compiler direction

The instant-layer path already has useful ingredients: explicit boundary
inputs and roles in `LayerInputSource`, hash-consable Boolean `LayerExpr`
nodes, backward slicing in `ReducedLayerProgram`, and allocation-free
topological evaluation. A general combinational compiler can reuse these
rather than infer a counter-specific word. Add signal-strength operations to
the same DAG (input strength, max, attenuation, threshold/predicate, and
component transfer), and compile only closed acyclic slices between explicit
state boundaries. Cycles or ambiguous ownership should fall back to the
existing graph executor.

The sequential boundary retains genuine logical memory, required clocks, and
observable notifications and strength. Private piston pose, propagation, reset
geometry, and FIFO bookkeeping can disappear when their consequences are fully
internal. Model one computation as outputs and proposed next state, with a
separate adapter for the limited outside effects and interpreter reconstruction.
The current compiler publishes that executable DAG; world activation remains
separate work, rather than a claim that a cold artifact already improves TPS.
