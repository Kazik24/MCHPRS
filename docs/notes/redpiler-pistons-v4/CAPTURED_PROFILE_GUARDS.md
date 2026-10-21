# Captured profile guard specialization, 2026-10-20

The BubbleSort census found 39,550,778 recursive captured-profile checks in a
complete 10,036-tick sort. Other SiteTest kinds had no calls in that census.
See [the sampling and guard census](PISTON_SAMPLE_REUSE_CENSUS.md).

The existing guard IR already distinguishes a captured profile comparison from
geometry and power queries. Its evaluator now compares the profile ID stored
in the selected observation directly. It previously reconstructed a resolved
site, block and properties before comparing the same ID.

This uses the existing compiled representation. It adds no pass, flag, lookup
table, cache, allocation or scheduling operation. It runs wherever that guard
already runs, including the current O5 pipeline. Live guards and the other
captured site-test kinds retain their existing evaluator.

## Equivalence

Capture selection is unchanged: a matching single-site observation takes
precedence; otherwise the full captured-site array supplies the observation.
A missing observation is false before any enclosing negation. A captured
profile identity remains the captured identity after a piston/resource changes
the live occupant. The evaluator does not replace it with a live contact read.

For valid observations produced by Graph, resolving a captured site preserves
`observed.profile` as `resolved.profile`. Block reconstruction is therefore
unnecessary for this particular test. The change preserves callback counts,
sample points, FIFO requests, request sources and pending-work construction.

The differential test compares the shortcut against the old resolver formula
for every site/profile pair in the admitted instant-reset fixture. It covers
component, constant and contact sites, changed piston/resource state, capture
precedence, absent captures and negation. All 613 redpiler tests passed;
71 ignored research tests were outside that broad run.

A separate ignored full-sort run passed 10,037 initial/per-tick RAM checks
against the frozen interpreter trace, including sorted output 0 through 15.
Its complete census report exactly matched the pre-change report, including
all 27 execution counters and guard/reuse counts. This is not a TPS result.

## Measurement artifacts

`target/captured-profile-guards/` contains the frozen production candidate,
diagnostic executables, build logs, differential/broad test log, full-sort
census, source provenance, and the alternating five-pair benchmark driver and
verifier. Production timing is still pending completion of that batch. One clean
pair passed all equivalence checks; a concurrent Cargo/rustc build contaminated
the next pair, which was rejected. Completed clean pairs are retained and the
agent retries incomplete pairs only after the observed build processes finish.
There is no five-pair aggregate or new accepted TPS result yet.

Candidate SHA-256:
`b8110d2e09cb795bbf8fe48121de8efed002a6cc02f6c5e8e380170822c5bed6`.
Frozen reference SHA-256:
`f815f5eb789db5f2284999052b9054a2de6e815b6b79723aa4ccbf15de7a019d`.

The candidate was built with release fat LTO from a detached checkout of
`e25dc60` plus the recorded change. This separates it from concurrent Counter
logical-lowering research in the main workspace. The first new differential
test fixture was rejected as an ordinary piston push; the final diagnostic
build uses the existing admitted fixture. That test-only correction does not
change production code.
