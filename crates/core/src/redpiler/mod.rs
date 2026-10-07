//! Compile world blocks into an electrical graph, then execute it with the direct
//! backend. Piston regions also carry an instant program for moving geometry.

pub mod analysis;
pub(crate) mod backend;
mod compile_graph;
pub mod instant;
mod passes;
mod task_monitor;

use crate::redstone;
use crate::world::{for_each_block_mut_optimized, World};
use backend::direct::DirectBackend;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use mchprs_world::TickEntry;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, trace};

pub use backend::{Queues, TickScheduler};
pub use task_monitor::TaskMonitor;

#[derive(Debug)]
pub enum CompileError {
    AlreadyActive,
    Analysis(analysis::AnalysisError),
    Unsupported(Box<analysis::AnalysisReport>),
    Cancelled,
    Backend(backend::BackendError),
    Graph(compile_graph::GraphError),
    Instant(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyActive => f.write_str("reset the active compiler before recompiling"),
            Self::Analysis(error) => error.fmt(f),
            Self::Unsupported(report) => {
                write!(f, "{}", report.summary())?;
                if let Some(issue) = report.issues.first() {
                    write!(f, "; {issue}")?;
                }
                Ok(())
            }
            Self::Cancelled => f.write_str("compilation cancelled"),
            Self::Backend(error) => error.fmt(f),
            Self::Graph(error) => error.fmt(f),
            Self::Instant(error) => f.write_str(error),
        }
    }
}

impl std::error::Error for CompileError {}

fn block_powered_mut(block: &mut Block) -> Option<&mut bool> {
    Some(match block {
        Block::RedstoneComparator { comparator } => &mut comparator.powered,
        Block::RedstoneTorch { lit } => lit,
        Block::RedstoneWallTorch { lit, .. } => lit,
        Block::RedstoneRepeater { repeater } => &mut repeater.powered,
        Block::Lever { lever } => &mut lever.powered,
        Block::StoneButton { button } => &mut button.powered,
        Block::StonePressurePlate { powered } => powered,
        Block::RedstoneLamp { lit } => lit,
        Block::IronTrapdoor { powered, .. } => powered,
        Block::NoteBlock { powered, .. } => powered,
        _ => return None,
    })
}

#[derive(Default, PartialEq, Eq, Debug)]
pub struct CompilerOptions {
    /// Evaluate logical piston state without movement or reset pulses.
    pub assume_instant: bool,
    /// Set by the server from the initiating player's rank, never from flags.
    /// Zero retains the default 1x budget; values are capped at 8x.
    pub budget_multiplier: usize,
    /// Enable optimization passes which may significantly increase compile times.
    pub optimize: bool,
    /// Export the graph to a binary format. See the [`redpiler_graph`] crate.
    pub export: bool,
    /// Only flush lamp, button, lever, pressure plate, or trapdoor updates.
    pub io_only: bool,
    /// Update all blocks in the input region after reset.
    pub update: bool,
    /// Export a dot file of the backend graph after compilation.
    pub export_dot_graph: bool,
}

impl CompilerOptions {
    pub fn parse(flags: &str) -> Result<CompilerOptions, String> {
        let mut options = Self::default();
        for option in flags.split_whitespace() {
            if option.starts_with("--") {
                match option {
                    "--assume-instant" => options.assume_instant = true,
                    "--optimize" => options.optimize = true,
                    "--export" => options.export = true,
                    "--io-only" => options.io_only = true,
                    "--update" => options.update = true,
                    "--export-dot" => options.export_dot_graph = true,
                    _ => return Err(format!("Unrecognized Redpiler option: {option}")),
                }
            } else if let Some(short_flags) = option.strip_prefix('-') {
                if short_flags.is_empty() {
                    return Err(format!("Unrecognized Redpiler option: {option}"));
                }
                for c in short_flags.chars() {
                    match c.to_ascii_lowercase() {
                        'o' => options.optimize = true,
                        'e' => options.export = true,
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

#[derive(Default)]
pub struct Compiler {
    backend: Option<DirectBackend>,
    options: CompilerOptions,
    warnings: Vec<String>,
}

impl Compiler {
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
    pub fn is_active(&self) -> bool {
        self.backend.is_some()
    }

    pub fn current_flags(&self) -> Option<&CompilerOptions> {
        self.backend.as_ref().map(|_| &self.options)
    }

    pub fn compile<W: World>(
        &mut self,
        world: &W,
        bounds: (BlockPos, BlockPos),
        options: CompilerOptions,
        ticks: Vec<TickEntry>,
        monitor: Arc<TaskMonitor>,
    ) -> Result<(), CompileError> {
        debug!("Starting compile");
        let start = Instant::now();

        if self.is_active() {
            return Err(CompileError::AlreadyActive);
        }
        monitor.set_budget_multiplier(options.budget_multiplier);
        let report = analysis::analyze(
            world,
            bounds,
            &ticks,
            &monitor,
            analysis::AnalysisLimits::for_budget(monitor.budget_multiplier()),
        )
        .map_err(CompileError::Analysis)?;
        if report.pistons.is_empty() && !report.can_compile() {
            return Err(CompileError::Unsupported(Box::new(report)));
        }

        let ticks: Vec<_> = ticks
            .into_iter()
            .filter(|entry| {
                entry
                    .block_type
                    .is_none_or(|kind| world.get_block(entry.pos).registry_id() == kind)
            })
            .collect();
        let input = CompilerInput {
            world,
            bounds,
            ticks: &ticks,
            boundaries: None,
        };
        let (graph, instant) = if report.pistons.is_empty() {
            (
                passes::run_passes(&options, &input, &monitor).map_err(CompileError::Graph)?,
                Vec::new(),
            )
        } else {
            let (graph, program) =
                instant::program::prepare(world, &report, &ticks, &options, monitor.clone())
                    .map_err(CompileError::Instant)?;
            (graph, program)
        };

        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }

        // Stage a fresh backend. Reusing one can leave aliases, scheduler work
        // or side tables from a previous compilation. Publish only on success.
        let mut backend = DirectBackend::default();
        trace!("Compiling backend");
        monitor.set_message("Compiling backend".to_string());
        backend
            .compile(graph, ticks, &options, instant)
            .map_err(CompileError::Backend)?;
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        monitor.inc_progress();

        self.backend = Some(backend);
        self.options = options;
        self.warnings = report.pistons.iter()
            .filter(|p| p.piston.extended && p.diagnostics.contains(&analysis::PistonDiagnostic::MissingOrMismatchedHead))
            .map(|p| format!("Extended piston at {:?} has no matching saved head at {:?}; the runtime starts from its saved geometry.", p.pos, p.head))
            .collect();
        for warning in &self.warnings {
            tracing::warn!("Redpiler: {warning}");
        }
        debug!("Compile completed in {:?}", start.elapsed());
        Ok(())
    }

    pub fn reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) {
        if let Some(mut backend) = self.backend.take() {
            backend.reset(world, self.options.io_only);
        }

        if self.options.update {
            let (first_pos, second_pos) = bounds;
            for_each_block_mut_optimized(world, first_pos, second_pos, |world, pos| {
                let block = world.get_block(pos);
                redstone::update(block, world, pos, None);
            });
        }
        self.options = Default::default();
    }

    fn backend(&mut self) -> &mut DirectBackend {
        self.backend
            .as_mut()
            .expect("tried to get redpiler backend when inactive")
    }

    pub fn tick(&mut self) {
        self.backend().tick();
    }

    pub fn on_use_block(&mut self, pos: BlockPos) {
        self.backend().on_use_block(pos);
    }

    pub fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        self.backend().set_pressure_plate(pos, powered);
    }

    pub fn flush<W: World>(&mut self, world: &mut W) {
        let io_only = self.options.io_only;
        self.backend().flush(world, io_only);
    }

    pub fn tick_with_world<W: World>(&mut self, world: &mut W) {
        self.backend().tick_with_world(world);
    }

    pub fn inspect(&mut self, pos: BlockPos) {
        if let Some(backend) = &mut self.backend {
            backend.inspect(pos);
        } else {
            debug!("cannot inspect when backend is not running");
        }
    }
}

pub struct CompilerInput<'w, W: World> {
    pub world: &'w W,
    pub bounds: (BlockPos, BlockPos),
    pub ticks: &'w [TickEntry],
    pub(crate) boundaries: Option<&'w instant::boundary::Boundaries<'w>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_options() {
        let input = "-iO -U --export";
        let expected_options = CompilerOptions {
            assume_instant: false,
            budget_multiplier: 0,
            io_only: true,
            optimize: true,
            export: true,
            update: true,
            export_dot_graph: false,
        };
        let options = CompilerOptions::parse(input).unwrap();

        assert_eq!(options, expected_options);
    }

    #[test]
    fn compilation_budget_cannot_be_selected_by_command_flags() {
        assert!(CompilerOptions::parse("--budget-multiplier=8").is_err());
        let monitor = TaskMonitor::default();
        assert_eq!(monitor.budget_multiplier(), 1);
        monitor.set_budget_multiplier(usize::MAX);
        assert_eq!(monitor.budget_multiplier(), 8);
    }

    #[test]
    fn logical_mode_is_explicit_and_unknown_flags_fail() {
        assert!(
            CompilerOptions::parse("--assume-instant -oi")
                .unwrap()
                .assume_instant
        );
        assert!(!CompilerOptions::parse("").unwrap().assume_instant);
        for flags in ["--assume-instnat", "-ox", "-", "assume-instant"] {
            assert!(CompilerOptions::parse(flags).is_err(), "{flags}");
        }
    }
}
