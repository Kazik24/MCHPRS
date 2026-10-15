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
#[path = "graph/tests/trace.rs"]
mod tests;
