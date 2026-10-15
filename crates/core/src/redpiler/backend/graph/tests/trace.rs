#[test]
fn disabled_trace_does_not_evaluate_arguments() {
    #[cfg(not(feature = "graph-trace"))]
    {
        let evaluated = std::cell::Cell::new(false);
        super::graph_trace!(0, "test", "{}", {
            evaluated.set(true);
            1
        });
        assert!(!evaluated.get());
    }
}
