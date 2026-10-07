# Frozen CPU evidence

This directory holds versioned interpreter checkpoints, ordered output/screen/RAM
projections, explicit migrations, and historical capture/benchmark provenance.

Read [CPU reference procedures](../../docs/tests/CPU_REFERENCES.md) for fixture
setup, assertions, long replays, compiled comparisons, benchmarking, and deliberate
new-file capture. The shared harness is
[cpus.rs](../../crates/core/benches/support/cpus.rs).

Retain original expectations when optimizing. Performance JSON describes its
recorded machine and revision; it is not a current runtime guarantee.
