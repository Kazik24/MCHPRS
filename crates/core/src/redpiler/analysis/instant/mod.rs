//! Local instant-cell contracts and the shared ownership analysis they depend on.
// The boundary pass will publish these contracts; until then only their proof tests use them.
#[cfg(test)]
pub(crate) mod cells;
pub(crate) mod ownership;
