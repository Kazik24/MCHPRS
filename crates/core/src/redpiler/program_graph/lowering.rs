//! Consume a validated draft into immutable instructions and its initial mutable state.
use super::*;

impl CircuitDraft {
    pub(crate) fn lower(self) -> Result<(CircuitProgram, CircuitState), GraphBuildError> {
        self.lower_with_monitor(&crate::redpiler::TaskMonitor::default())
    }

    pub(crate) fn lower_with_monitor(self, monitor: &crate::redpiler::TaskMonitor) -> Result<(CircuitProgram, CircuitState), GraphBuildError> {
        let layers = self.validate_for_lowering(monitor)?;
        self.lower_checked(monitor, layers)
    }

    fn lower_checked(
        mut self,
        monitor: &crate::redpiler::TaskMonitor,
        layers: Option<crate::redpiler::analysis::instant::layers::LayerCompilationContext>,
    ) -> Result<(CircuitProgram, CircuitState), GraphBuildError> {
        if monitor.cancelled() {
            return Err(GraphBuildError::Cancelled);
        }
        let kernel_start = std::time::Instant::now();
        let kernels = crate::redpiler::analysis::instant::layers::lower_kernels_with_context(&self, monitor, layers.as_ref()).map_err(|_| {
            GraphBuildError::InvalidInstantRegion {
                table: "kernel lowering",
                index: 0,
            }
        })?;
        drop(layers);
        let kernel_duration = kernel_start.elapsed();
        let program_start = std::time::Instant::now();
        let mut kernel_by_actor = vec![None; self.pistons.len()];
        for (index, kernel) in kernels.iter().enumerate() {
            for pull in kernel.pulls.iter().filter(|pull| pull.external) {
                kernel_by_actor[pull.actor.index()] = Some(index as u32);
            }
        }

        let mut instant_route_by_route = vec![None; self.routes.len()];
        let mut private_resource_owners = vec![None; self.resources.len()];
        for (proxy_index, proxy) in self.plans.instant.proxies.iter().enumerate() {
            for cell in &proxy.private_cells {
                private_resource_owners[cell.resource.index()] = Some(cell.actor);
            }
            for (route_index, route) in proxy.routes.iter().enumerate() {
                instant_route_by_route[route.route.index()] = Some((proxy_index as u32, route_index as u32));
                // The validated instructions retain every witness; runtime needs only one copy.
                self.routes[route.route.index()].deliveries = Box::new([]);
            }
        }

        // Proxy-owned routes use their own instructions and were cleared above.
        // Never bypass that ownership with a pre-lowering admission certificate.
        for wire in &mut self.wires {
            if wire.turbo_route.is_some_and(|route| instant_route_by_route[route.index()].is_some()) {
                wire.execution.callback_selection = None;
            }
        }
        if !self.wires.is_empty()
            && self.wires.iter().all(|wire| {
                wire.execution
                    .callback_selection
                    .is_some_and(|plan| plan.known == WireCallbackSelection::ALL)
            })
        {
            self.plans.turbo_callback_indices.clear();
        }

        let nodes = self
            .nodes
            .iter()
            .map(|node| NodeSpec {
                position: node.position,
                raw_state: node.raw_state,
                block: node.raw_state.map(Block::from_id),
                kind: node.kind.clone(),
                output_route: node.output_route,
            })
            .collect::<Vec<_>>();

        self.power_links.sort_by_key(|link| link.to);
        let mut power_inputs: FxHashMap<PowerTarget, Range<usize>> = FxHashMap::default();
        for (index, link) in self.power_links.iter().enumerate() {
            power_inputs.entry(link.to).or_insert(index..index).end = index + 1;
        }
        let profile_blocks = self
            .profiles
            .iter()
            .map(|profile| Block::from_id(profile.raw_state))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let node_positions: FxHashMap<_, _> = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| Some((node.position?, NodeId::try_from_index(index)?)))
            .collect();
        let site_positions: FxHashMap<_, _> = self
            .sites
            .iter()
            .enumerate()
            .filter_map(|(index, site)| Some((site.position, SiteId::try_from_index(index)?)))
            .collect();
        let command_blocks = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) = node.kind else {
                    return None;
                };
                let id = NodeId::try_from_index(index)?;
                let block = node.block?;
                let position = node.position?;
                let facing = command_facing(block);
                let command_at = |position: BlockPos| {
                    node_positions
                        .get(&position)
                        .copied()
                        .filter(|&other| matches!(nodes[other.index()].kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })))
                };
                let previous = command_at(position.offset(facing.opposite()));
                let next = command_at(position.offset(facing)).filter(|&other| {
                    matches!(
                        nodes[other.index()].kind,
                        CircuitNodeKind::Electrical(NodeType::CommandBlock { chain: true, .. })
                    )
                });
                Some(CommandBlockPlan {
                    node: id,
                    previous,
                    next,
                    conditional: block.property("conditional") == Some("true"),
                })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let note_blocks = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                if !matches!(node.kind, CircuitNodeKind::Electrical(NodeType::NoteBlock { .. })) {
                    return None;
                }
                let id = NodeId::try_from_index(index)?;
                let position = node.position?;
                Some(NoteBlockPlan {
                    node: id,
                    below: *site_positions.get(&position.offset(BlockFace::Bottom))?,
                    above: *site_positions.get(&position.offset(BlockFace::Top))?,
                })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let mut command_plan_by_node = vec![None; nodes.len()];
        for (index, plan) in command_blocks.iter().enumerate() {
            command_plan_by_node[plan.node.index()] = Some(index as u32);
        }
        let mut note_plan_by_node = vec![None; nodes.len()];
        for (index, plan) in note_blocks.iter().enumerate() {
            note_plan_by_node[plan.node.index()] = Some(index as u32);
        }
        let mut comparator_plan_by_node = vec![None; nodes.len()];
        for (index, plan) in self.comparator_inputs.iter().enumerate() {
            comparator_plan_by_node[plan.node.index()].get_or_insert(index as u32);
        }
        let mut piston_resources = Vec::with_capacity(self.pistons.len());
        for piston in &self.pistons {
            let binding = match piston.lane {
                PistonLane::Empty => None,
                PistonLane::SingleResource { far } => self
                    .resources
                    .iter()
                    .position(|resource| resource.contacts.contains(&piston.near) && resource.contacts.contains(&far))
                    .map(|index| ResourceId::new(index as u32)),
            };
            piston_resources.push(binding);
        }
        let mut resource_buckets = vec![Vec::new(); self.contacts.len()];
        for (index, resource) in self.resources.iter().enumerate() {
            for &contact in resource.contacts.iter() {
                resource_buckets[contact.index()].push(ResourceId::new(index as u32));
            }
        }
        let mut base_piston_buckets = vec![Vec::new(); self.contacts.len()];
        let mut head_piston_buckets = vec![Vec::new(); self.contacts.len()];
        for (index, piston) in self.pistons.iter().enumerate() {
            let id = PistonId::new(index as u32);
            base_piston_buckets[piston.base.index()].push(id);
            head_piston_buckets[piston.near.index()].push(id);
        }

        let mut dust_path_by_wire = vec![None; self.wires.len()];
        for (path_index, path) in self.plans.dust_paths.iter().enumerate() {
            let id = DustPathId::try_from_index(path_index).ok_or(GraphBuildError::TooManyEntries { table: "dust paths" })?;
            for &node in path.wires.iter() {
                let CircuitNodeKind::Wire(wire_index) = self.nodes[node.index()].kind else {
                    unreachable!("dust path references were validated before lowering");
                };
                dust_path_by_wire[wire_index] = Some(id);
            }
        }

        let state = CircuitState {
            classic_signals: self.plans.classic_nets.iter().map(|net| net.initial_signal).collect(),
            classic_active: vec![true; self.plans.classic_nets.len()].into_boxed_slice(),
            components: self.nodes.into_iter().map(|node| node.state).collect::<Vec<_>>().into_boxed_slice(),
            pistons: self.initial_pistons.into_boxed_slice(),
            resources: self.initial_resources.into_boxed_slice(),
        };
        let program = CircuitProgram {
            mixed_classic: self.plans.mixed_classic,
            fixed_classic: self.plans.fixed_classic.program,
            fixed_signals: FixedSignalProgram::lower(&self.plans.fixed_signals, nodes.len(), self.sites.len()),
            classic: ClassicProgram::lower(&self.plans.classic_nets, nodes.len(), self.routes.len()),
            instant: InstantProgram {
                kernels: kernels.into_boxed_slice(),
                kernel_by_actor: kernel_by_actor.into_boxed_slice(),
                proxies: self.plans.instant.proxies.into_boxed_slice(),
                private_resource_owners: private_resource_owners.into_boxed_slice(),
                route_by_route: instant_route_by_route.into_boxed_slice(),
            },
            nodes: nodes.into_boxed_slice(),
            power_links: self.power_links.into_boxed_slice(),
            power_inputs,
            comparator_inputs: self.comparator_inputs.into_boxed_slice(),
            comparator_plan_by_node: comparator_plan_by_node.into_boxed_slice(),
            profiles: self.profiles.into_boxed_slice(),
            profile_blocks,
            guards: self.guards.into_boxed_slice(),
            contacts: self.contacts.into_boxed_slice(),
            routes: self.routes.into_boxed_slice(),
            turbo_callback_indices: self.plans.turbo_callback_indices.into_boxed_slice(),
            wires: self.wires.into_boxed_slice(),
            dust_paths: self.plans.dust_paths.into_boxed_slice(),
            dust_path_by_wire: dust_path_by_wire.into_boxed_slice(),
            sites: self.sites.into_boxed_slice(),
            pistons: self.pistons.into_boxed_slice(),
            resources: self.resources.into_boxed_slice(),
            piston_resources: piston_resources.into_boxed_slice(),
            resource_contacts: ContactCandidates::from_buckets(resource_buckets, "resource contact candidates")?,
            base_pistons: ContactCandidates::from_buckets(base_piston_buckets, "base piston candidates")?,
            head_pistons: ContactCandidates::from_buckets(head_piston_buckets, "head piston candidates")?,
            command_blocks,
            command_plan_by_node: command_plan_by_node.into_boxed_slice(),
            note_blocks,
            note_plan_by_node: note_plan_by_node.into_boxed_slice(),
        };
        monitor.record_lowering(kernel_duration, program_start.elapsed());
        Ok((program, state))
    }
}

/// Lower a pipeline-owned graph without reopening the arbitrary-draft trust boundary.
pub(crate) fn lower_prepared(
    checked: crate::redpiler::passes::rich::ValidatedGraph,
    monitor: &crate::redpiler::TaskMonitor,
) -> Result<LoweredCircuit, GraphBuildError> {
    use crate::redpiler::passes::utils::{lowered_table_statistics, record_lowering_input_tables};
    let (prepared, layers) = checked.into_lowering_parts();
    record_lowering_input_tables(monitor, &prepared.graph)?;
    let (program, state) = prepared.graph.lower_checked(monitor, layers)?;
    monitor.record_preparation_tables(lowered_table_statistics(&program));
    let positions = program.position_index();
    let imported_ticks = prepared
        .entry
        .pending_ticks
        .into_iter()
        .map(|entry| ImportedTick {
            target: positions.get(&entry.tick.pos).copied(),
            position: entry.tick.pos,
            block_type: entry.tick.block_type,
            delay: entry.tick.ticks_left,
            priority: entry.tick.tick_priority,
            current_registry_type: entry.current_registry_type,
        })
        .collect();
    Ok(LoweredCircuit {
        program,
        state,
        logical_tick: prepared.entry.logical_tick,
        imported_ticks,
        positions,
    })
}
