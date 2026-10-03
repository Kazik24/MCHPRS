use super::{Plot, NUM_CHUNKS, PLOT_SCALE, PLOT_SECTIONS, PLOT_WIDTH};
use anyhow::{anyhow, ensure, Context, Result};
use mchprs_save_data::plot_data::{ChunkData, PlotData, Tps, WorldSendRate};
use once_cell::sync::Lazy;
use std::path::Path;
use std::time::Duration;

// TODO: where to put this?
pub fn sleep_time_for_tps(tps: Tps) -> Duration {
    match tps {
        Tps::Limited(tps) => {
            if tps > 10 {
                Duration::from_micros(1_000_000 / tps as u64)
            } else {
                Duration::from_millis(50)
            }
        }
        Tps::Unlimited => Duration::ZERO,
    }
}

pub fn load_plot(path: impl AsRef<Path>) -> Result<PlotData<PLOT_SECTIONS>> {
    let path = path.as_ref();
    let data = if path.exists() {
        PlotData::load_from_file(path, true)
            .with_context(|| format!("error loading plot save file at {}", path.display()))?
    } else {
        EMPTY_PLOT
            .as_ref()
            .map_err(|error| anyhow!("{}", error))?
            .clone()
    };
    ensure!(
        data.chunk_data.len() == NUM_CHUNKS,
        "plot contains {} chunks; plot scale {} requires {}",
        data.chunk_data.len(),
        PLOT_SCALE,
        NUM_CHUNKS
    );
    Ok(data)
}

pub fn empty_plot() -> PlotData<PLOT_SECTIONS> {
    EMPTY_PLOT
        .as_ref()
        .expect("failed to read template plot")
        .clone()
}

static EMPTY_PLOT: Lazy<Result<PlotData<PLOT_SECTIONS>, String>> = Lazy::new(|| {
    let template_path = Path::new("./world/plots/pTEMPLATE");
    if template_path.exists() {
        PlotData::load_from_file(template_path, true)
            .map_err(|error| format!("failed to read template plot: {}", error))
    } else {
        let mut chunks = Vec::new();
        for chunk_x in 0..PLOT_WIDTH {
            for chunk_z in 0..PLOT_WIDTH {
                chunks.push(Plot::generate_chunk(8, chunk_x, chunk_z));
            }
        }
        let chunk_data: Vec<ChunkData<PLOT_SECTIONS>> =
            chunks.iter_mut().map(|c| c.save()).collect();
        Ok(PlotData {
            tps: Tps::Limited(10),
            world_send_rate: WorldSendRate::default(),
            chunk_data,
            pending_ticks: Vec::new(),
        })
    }
});
