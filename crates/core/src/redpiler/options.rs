//! Command flags and compilation settings.

#[derive(PartialEq, Eq, Debug)]
/// Options that control graph preparation and reset behavior.
pub struct CompilerOptions {
    /// Opt in to the narrowly admitted experimental event executor. The caller
    /// declares computation cycles and must stop it with `Compiler::try_reset`.
    pub experimental_event: bool,
    /// Set by the server from the initiating player's rank, never from flags.
    /// Zero retains the default 1x budget; values are capped at 8x.
    pub budget_multiplier: usize,
    /// `Some(level)` selects that pass tier; `None` selects every configured tier.
    pub optimization_level: Option<usize>,
    /// Only flush visible outputs and controls, including lamps and copper bulbs.
    pub io_only: bool,
    /// Update all blocks in the input region after reset.
    pub update: bool,
}

pub(crate) const MAX_OPTIMIZATION_LEVEL: usize = 6;

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            experimental_event: false,
            budget_multiplier: 0,
            optimization_level: Some(0),
            io_only: false,
            update: false,
        }
    }
}

impl CompilerOptions {
    pub(crate) fn optimization_label(&self) -> String {
        format!("O{}", self.optimization_level.unwrap_or(MAX_OPTIMIZATION_LEVEL))
    }

    pub fn parse(flags: &str) -> Result<CompilerOptions, String> {
        let mut options = Self::default();
        for option in flags.split_whitespace() {
            if option.starts_with("--") {
                match option {
                    "--optimize" => options.optimization_level = None,
                    "--io-only" => options.io_only = true,
                    "--update" => options.update = true,
                    _ => return Err(format!("Unrecognized Redpiler option: {option}")),
                }
            } else if let Some(short_flags) = option.strip_prefix('-') {
                if short_flags.is_empty() {
                    return Err(format!("Unrecognized Redpiler option: {option}"));
                }
                for (index, c) in short_flags.char_indices() {
                    match c.to_ascii_lowercase() {
                        'o' => {
                            let level = &short_flags[index + c.len_utf8()..];
                            options.optimization_level = if level.is_empty() {
                                None
                            } else {
                                let level = level
                                    .parse::<usize>()
                                    .map_err(|_| format!("Invalid Redpiler optimization level: {option}"))?;
                                if level > MAX_OPTIMIZATION_LEVEL {
                                    return Err(format!("Redpiler supports optimization levels 0 through {MAX_OPTIMIZATION_LEVEL}"));
                                }
                                Some(level)
                            };
                            break;
                        }
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
