//! The goal of this module is to make upgrading to newer version of mchprs
//! easier by providing automatic conversion from old world data.
//!
//! Eventually this module might help recover currupted plot data.
//!
//! In the future it might be nice to have this as an optional dependency or
//! seperate download. As our save format changes in the future, the fixer
//! module may become quite big.

use super::{PlotData, PlotLoadError};
use crate::plot_data::VERSION;
use std::fs;
use std::path::Path;
use tracing::debug;

pub mod legacy_1_18;

#[derive(Debug)]
pub enum FixInfo {
    InvalidHeader,
    OldVersion { version: u32 },
}

pub fn try_fix<const NUM_SECTIONS: usize>(
    path: impl AsRef<Path>,
    info: FixInfo,
    save_fixed_plot: bool,
) -> Result<Option<PlotData<NUM_SECTIONS>>, PlotLoadError> {
    debug!("Trying to fix plot with {:?}", info);
    let result = match info {
        FixInfo::OldVersion { version: 1 } => {
            let data = fs::read(&path)?;
            Some(legacy_1_18::decode(&data[12..])?)
        }
        FixInfo::OldVersion { version: 2 } => return Err(PlotLoadError::ConversionUnavailable(2)),
        FixInfo::InvalidHeader => return Err(PlotLoadError::InvalidHeader),
        FixInfo::OldVersion { version: 0 } => {
            let data = fs::read(&path)?;
            Some(legacy_1_18::decode_v0(&data[12..])?)
        }
        _ => None,
    };

    Ok(match result {
        Some(data) => {
            data.validate()?;
            if save_fixed_plot {
                // Serialize before touching the original; retain it on any conversion failure.
                bincode::serialize(&data)?;
                crate::atomic::backup(path.as_ref())?;
                data.save_to_file(&path)?;
            }
            debug!("Successfully converted plot to version {}", VERSION);
            Some(data)
        }
        None => None,
    })
}
