//! Options supported by sampled execution.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PublicationMode {
    All,
    Redstone,
}

#[derive(PartialEq, Eq, Debug, Default)]
/// Options that control publication and reset behavior.
pub struct CompilerOptions {
    /// Only flush visible outputs and controls, including lamps and copper bulbs.
    pub io_only: bool,
    /// Request the experimental LLVM backend.
    pub llvm: bool,
    /// Override the build's default publication scope.
    pub publication: Option<PublicationMode>,
    /// Update all blocks in the input region after reset.
    pub update: bool,
}

impl CompilerOptions {
    pub fn parse(flags: &str) -> Result<CompilerOptions, String> {
        let mut options = Self::default();
        for option in flags.split_whitespace() {
            if option.starts_with("--") {
                match option {
                    "--io-only" => options.io_only = true,
                    "--llvm" => options.llvm = true,
                    "--publish-all" | "--publish-redstone" => {
                        let mode = if option == "--publish-all" { PublicationMode::All } else { PublicationMode::Redstone };
                        if options.publication.is_some_and(|previous| previous != mode) {
                            return Err("Choose only one publication mode".into());
                        }
                        options.publication = Some(mode);
                    }
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
        if options.io_only && options.publication.is_some() {
            return Err("Choose only one publication mode".into());
        }
        Ok(options)
    }
}
