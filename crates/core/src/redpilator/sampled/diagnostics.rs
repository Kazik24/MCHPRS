//! Optional memory and routing diagnostics, kept out of execution and publication.
use super::{Runtime, llvm, movement, program};

impl Runtime {
    /// Report retained program, runtime and native-encoding storage.
    ///
    /// Inputs: This runtime and its shared immutable program.
    ///
    /// Walk each vector and nested table, estimate capacity in bytes, and print grouped diagnostics
    /// including compact-index savings.
    pub(in crate::redpilator) fn log_memory_layout(&self) {
        fn vector<T>(scope: &str, name: &str, values: &Vec<T>) {
            eprintln!(
                "Redpilator memory: {scope}.{name} len={} capacity={} element_bytes={} buffer_bytes={}",
                values.len(),
                values.capacity(),
                std::mem::size_of::<T>(),
                values.capacity() * std::mem::size_of::<T>()
            );
        }
        fn rows<T>(name: &str, values: &Vec<Vec<T>>) {
            let entries: usize = values.iter().map(Vec::len).sum();
            let payload_capacity: usize = values.iter().map(Vec::capacity).sum();
            let bytes = values.capacity() * std::mem::size_of::<Vec<T>>() + payload_capacity * std::mem::size_of::<T>();
            eprintln!(
                "Redpilator memory: program.{name} rows={} entries={entries} payload_capacity={payload_capacity} buffer_bytes={bytes}",
                values.len()
            );
        }
        fn small_rows(name: &str, values: &Vec<smallvec::SmallVec<[usize; 2]>>) {
            let entries: usize = values.iter().map(|row| row.len()).sum();
            let spilled = values.iter().filter(|row| row.spilled());
            let payload_capacity: usize = spilled.map(|row| row.capacity()).sum();
            let bytes = values.capacity() * std::mem::size_of::<smallvec::SmallVec<[usize; 2]>>() + payload_capacity * std::mem::size_of::<usize>();
            eprintln!(
                "Redpilator memory: program.{name} rows={} entries={entries} spilled_payload_capacity={payload_capacity} buffer_bytes={bytes}",
                values.len()
            );
        }
        macro_rules! vectors {
            ($owner:expr, $scope:literal, $($field:ident),+ $(,)?) => { $(vector($scope, stringify!($field), &($owner).$field);)+ };
        }
        macro_rules! rows {
            ($($field:ident),+ $(,)?) => { $(rows(stringify!($field), &self.program.$field);)+ };
        }
        eprintln!("Redpilator memory: selected retained vector capacities; excludes maps, allocator overhead and unlisted deep payloads");
        match llvm::LlvmProgram::encode(&self.program) {
            Ok(llvm) => eprintln!(
                "Redpilator LLVM ABI: version={} components={} operations={} links={} logical_blob_bytes={}",
                llvm.descriptor.abi_version,
                llvm.descriptor.component_count,
                llvm.descriptor.operation_count,
                llvm.descriptor.link_count,
                llvm.blob.len()
            ),
            Err(error) => eprintln!("Redpilator LLVM ABI: export rejected: {}", error.reason),
        }
        vectors!(
            self.program,
            "program",
            piston_by_component,
            repeater_by_component,
            comparator_by_component,
            watched_by_component,
            cell_components,
            component_cells,
            positions,
            initial,
            projection_positions,
            cell_geometry,
            output_programs,
            operations,
            links,
            side_links,
            input_arrangements,
            arrangement_links,
            movement_groups,
            movement_flips,
            movement_steps,
            movement_wires,
            movement_transitions,
            movement_ranges,
            shape_steps,
            shape_maps,
            cell_shape_steps,
            effective_links
        );
        rows!(
            outgoing_arrangements,
            side_outputs,
            comparators_by_override,
            reactive_by_source,
            reactive_neighbours,
            token_cells,
            token_movement_groups,
            movement_cells,
            movement_fixed_ranges
        );
        let shape_ports: usize = self.program.shape_steps.iter().map(|step| step.states.ports.len()).sum();
        let shape_valid_ports: usize = self
            .program
            .shape_steps
            .iter()
            .map(|step| step.states.ports.iter().flatten().count())
            .sum();
        let shape_port_capacity: usize = self.program.shape_steps.iter().map(|step| step.states.ports.capacity()).sum();
        eprintln!(
            "Redpilator shape maps: unique_maps={} valid_ports={shape_valid_ports} invalid_ports={} unpooled_map_bytes={} unique_map_bytes={} port_buffer_bytes={}",
            self.program.shape_maps.len(),
            shape_ports - shape_valid_ports,
            shape_valid_ports * 256,
            self.program.shape_maps.len() * 256,
            shape_port_capacity * std::mem::size_of::<Option<usize>>()
        );
        let arrangement_payload: usize = self
            .program
            .input_arrangements
            .iter()
            .map(|group| {
                group.tokens.capacity() * std::mem::size_of::<usize>()
                    + group.token_states.capacity() * std::mem::size_of::<Vec<usize>>()
                    + group
                        .token_states
                        .iter()
                        .map(|row| row.capacity() * std::mem::size_of::<usize>())
                        .sum::<usize>()
                    + group.conditional_sources.capacity() * std::mem::size_of::<program::ArrangementSource>()
                    + group.inputs.capacity() * std::mem::size_of::<Vec<(program::ArrangementSource, u8)>>()
                    + group
                        .inputs
                        .iter()
                        .map(|row| row.capacity() * std::mem::size_of::<(program::ArrangementSource, u8)>())
                        .sum::<usize>()
            })
            .sum();
        let link_payload: usize = self.program.arrangement_links.iter().map(|link| link.active.capacity()).sum();
        let axis_payload: usize = self
            .program
            .movement_groups
            .iter()
            .map(|group| {
                group.axes.capacity() * std::mem::size_of::<movement::MovementAxis>()
                    + group
                        .axes
                        .iter()
                        .map(|axis| {
                            axis.states.capacity() * std::mem::size_of::<usize>()
                                + (axis.depart.capacity() + axis.arrive.capacity()) * std::mem::size_of::<std::ops::Range<usize>>()
                        })
                        .sum::<usize>()
            })
            .sum();
        eprintln!(
            "Redpilator memory: arrangement_deep_bytes={arrangement_payload} arrangement_link_deep_bytes={link_payload} movement_group_deep_bytes={axis_payload}"
        );
        small_rows("incoming", &self.program.incoming);
        small_rows("outgoing", &self.program.outgoing);
        small_rows("input_groups_by_target", &self.program.input_groups_by_target);
        small_rows("source_watchers", &self.program.source_watchers);
        small_rows("token_piston_components", &self.program.token_piston_components);
        small_rows("home_watchers", &self.program.home_watchers);
        small_rows("cell_tokens", &self.program.cell_tokens);
        small_rows("cell_pistons", &self.program.cell_pistons);
        small_rows("cell_observers", &self.program.cell_observers);
        vectors!(
            self,
            "runtime",
            piston_motion,
            cell_motion,
            updated_in_phase,
            comparator_outputs,
            input_levels,
            input_histograms,
            blocks,
            contributions,
            dirty,
            dirty_cells,
            group_arrangements,
            arrangement_contributions,
            pending_counts,
            movement_switched,
            movement_changed_wires,
            changed_wires,
            switched,
            note_queued,
            watched_strengths,
            reactive_wire_strengths,
            token_positions,
            token_digits,
            piston_extended,
            piston_head_present,
            piston_head_moving,
            piston_base_moving,
            motions,
            free_motion_slots
        );
        let index_slots = self.program.piston_by_component.capacity()
            + self.program.repeater_by_component.capacity()
            + self.program.comparator_by_component.capacity()
            + self.program.watched_by_component.capacity()
            + self.program.cell_components.capacity()
            + self.piston_motion.capacity()
            + self.cell_motion.capacity();
        let stamp_slots = self.updated_in_phase.capacity();
        let compact_bytes = index_slots * std::mem::size_of::<Option<std::num::NonZeroUsize>>() + stamp_slots * std::mem::size_of::<u64>();
        let wide_equivalent_bytes = index_slots * std::mem::size_of::<Option<usize>>() + stamp_slots * std::mem::size_of::<Option<u64>>();
        eprintln!(
            "Redpilator memory: compact_index_stamp_buffers={compact_bytes} wide_equivalent_buffers={wide_equivalent_bytes} saved_bytes={}",
            wide_equivalent_bytes - compact_bytes
        );
    }

    #[cfg(feature = "redpilator-trace")]
    pub(in crate::redpilator) fn log_routing_metrics(&self) {
        if std::env::var_os("MCHPRS_REDPILATOR_PHASE_TIMINGS").is_some() {
            let [changes, readers, visits] = self.routing_metrics;
            eprintln!(
                "Redpilator routing runtime: token_changes={changes} readers={readers} wire_visits={visits} operations={}",
                self.operations
            );
        }
    }
}
