//! Options supported by sampled execution.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PublicationMode {
    All,
    Redstone,
    Io,
}

impl PublicationMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "redstone" => Some(Self::Redstone),
            "io" => Some(Self::Io),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Redstone => "redstone",
            Self::Io => "io",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
/// Options that control publication and reset behavior.
pub struct CompilerOptions {
    /// Only flush visible outputs and controls, including lamps and copper bulbs.
    pub io_only: bool,
    /// Request the experimental LLVM backend.
    pub llvm: bool,
    /// Skip wire-shape replay; LLVM also defers wire power when wires have no electrical outputs.
    pub relaxed_mode: bool,
    /// Override the build's default publication scope.
    pub publication: Option<PublicationMode>,
    /// Update all blocks in the input region after reset.
    pub update: bool,
}

impl CompilerOptions {
    pub(crate) fn movement_folding(&self) -> bool {
        self.relaxed_mode && !self.llvm
    }

    pub fn publication_mode(&self) -> PublicationMode {
        if self.io_only {
            PublicationMode::Io
        } else {
            self.publication.unwrap_or(if cfg!(feature = "redpilator-full-publication") {
                PublicationMode::All
            } else {
                PublicationMode::Redstone
            })
        }
    }

    /// Parse compiler flags and reject conflicting publication modes.
    ///
    /// Inputs: The command-line flag string and this options value.
    pub fn parse(flags: &str) -> Result<CompilerOptions, String> {
        let mut options = Self::default();
        for option in flags.split_whitespace() {
            if option.starts_with("--") {
                match option {
                    "--llvm" => options.llvm = true,
                    "--relaxed" => options.relaxed_mode = true,
                    "--publish-all" | "--publish-redstone" | "--publish-io" => {
                        let mode = PublicationMode::parse(option.strip_prefix("--publish-").unwrap()).unwrap();
                        if options.publication.is_some_and(|previous| previous != mode) {
                            return Err("Choose only one publication mode".into());
                        }
                        options.publication = Some(mode);
                    }
                    _ => return Err(format!("Unrecognized Redpilator option: {option}")),
                }
            } else {
                return Err(format!("Unrecognized Redpilator option: {option}"));
            }
        }
        options.io_only = options.publication == Some(PublicationMode::Io);
        Ok(options)
    }
}
