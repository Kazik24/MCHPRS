# Graph executor tests

Graph test sources live here, grouped by behavior. `components.rs` covers ordinary handlers, `fuzz.rs` owns the small differential fixtures, and `adder.rs` owns the shared adder loader and replay helpers. Their related modules reuse those helpers.

The existing Rust module hierarchy is preserved: moving a source file does not rename its discovered tests. Modules are registered with `#[path]` from `graph.rs` or their original parent module. Wire-selection, fixed-point and trace tests remain children of their production modules so they can test private helpers.

Put new graph tests in this directory and register them under the relevant parent. Reuse the fixture loaders and interpreter comparisons rather than creating another simulator or loader.

```text
cargo test -p mchprs_core --lib redpiler::backend:: --locked -- --test-threads=1
```
