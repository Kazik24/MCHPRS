#[cfg(feature = "graph-trace")]
macro_rules! graph_trace {
    ($tick:expr, $phase:expr, $($arg:tt)*) => {
        tracing::trace!(target: "mchprs_core::redpiler::graph", "tick={} phase={:?} | {}", $tick, $phase, format_args!($($arg)*));
    };
}

#[cfg(not(feature = "graph-trace"))]
macro_rules! graph_trace {
    ($tick:expr, $phase:expr, $($arg:tt)*) => {};
}

pub(crate) use graph_trace;

#[cfg(test)]
mod tests {
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
}
