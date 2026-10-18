//! Local instant-cell contracts and the shared ownership analysis they depend on.
pub(crate) mod boundaries;
pub(crate) mod cells;
#[cfg(test)]
pub(crate) mod layers;
pub(crate) mod ownership;
#[cfg(test)]
pub(crate) mod regions;
