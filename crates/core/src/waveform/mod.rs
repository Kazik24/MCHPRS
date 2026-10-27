//! Plot-owned capture; network and serialization run on the dedicated API worker.
pub(crate) mod api;
mod recorder;
#[cfg(test)]
mod tests;

use crate::config::CONFIG;
use crate::plot::memory_budget::{Budget, Reservation};
use once_cell::sync::Lazy;
pub(crate) use recorder::{Probe, Recorder, signal_value};
use std::sync::Arc;

pub(super) const PAGE_BYTES: usize = 512 * 1024;
pub(super) const WORK_BYTES: usize = 4 * 1024 * 1024;
pub(super) const TEXT_BYTES: usize = 64 * 1024;
pub(super) static STORED: Lazy<Arc<Budget>> = Lazy::new(|| Budget::new(mib(CONFIG.waveform_memory_mib)));
pub(super) static WORK: Lazy<Arc<Budget>> = Lazy::new(|| Budget::new(mib(CONFIG.waveform_work_memory_mib)));
pub(super) fn mib(value: u32) -> usize {
    (value as usize).checked_mul(1024 * 1024).expect("waveform memory size overflow")
}
pub(super) fn reserve(budget: &Arc<Budget>, bytes: usize) -> Result<Reservation, String> {
    budget
        .reserve(bytes)
        .ok_or_else(|| "Waveform memory is full; stop another capture and retry.".into())
}

pub(crate) fn validate_config() -> Result<(), String> {
    let c = &*CONFIG;
    if c.waveform_max_signals == 0
        || c.waveform_max_signals > 1024
        || c.waveform_max_ticks == 0
        || c.waveform_max_ticks > 10000
        || c.waveform_builder_signals == 0
        || c.waveform_builder_ticks == 0
        || c.waveform_other_signals == 0
        || c.waveform_other_ticks == 0
        || c.waveform_plot_memory_mib < 2
        || c.waveform_memory_mib < c.waveform_plot_memory_mib
        || c.waveform_text_memory_mib >= c.waveform_plot_memory_mib
        || c.waveform_max_records == 0
        || c.waveform_max_text_events == 0
        || c.waveform_work_memory_mib < 4
    {
        return Err("Invalid waveform caps or memory budgets (hard maximum: 1024 signals, 10000 ticks)".into());
    }
    let url = reqwest::Url::parse(&c.waveform_public_url).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err("waveform_public_url must be an HTTP(S) URL".into());
    }
    Ok(())
}
