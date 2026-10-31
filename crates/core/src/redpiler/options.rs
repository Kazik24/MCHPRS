//! Options supported by sampled execution.

#[derive(PartialEq, Eq, Debug)]
/// Options that control publication and reset behavior.
pub struct CompilerOptions {
    /// Only flush visible outputs and controls, including lamps and copper bulbs.
    pub io_only: bool,
    /// Update all blocks in the input region after reset.
    pub update: bool,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            io_only: false,
            update: false,
        }
    }
}

impl CompilerOptions {
    pub fn parse(flags: &str) -> Result<CompilerOptions, String> {
        let mut options = Self::default();
        for option in flags.split_whitespace() {
            if option.starts_with("--") {
                match option {
                    "--io-only" => options.io_only = true,
                    "--update" => options.update = true,
                    _ => return Err(format!("Unrecognized Redpiler option: {option}")),
                }
            } else if let Some(short_flags) = option.strip_prefix('-') {
                if short_flags.is_empty() {
                    return Err(format!("Unrecognized Redpiler option: {option}"));
                }
                for c in short_flags.chars() {
                    match c.to_ascii_lowercase() {
                        'i' => options.io_only = true,
                        'u' => options.update = true,
                        _ => return Err(format!("Unrecognized Redpiler option: -{c}")),
                    }
                }
            } else {
                return Err(format!("Unrecognized Redpiler option: {option}"));
            }
        }
        Ok(options)
    }
}
