use super::*;

impl EventNetlist {
    /// Admit only a physical probe whose exhaustive samples are a max-of-attenuated-sources net.
    pub(crate) fn from_probe(probe: &crate::redpiler::backend::graph::wire_probe::WireNetlist) -> Result<Self, &'static str> {
        let mut net = Self::from_samples(probe.sources().len(), probe.receivers().len(), probe.samples())?;
        for (index, source) in net.sources.iter_mut().enumerate() {
            *source = if probe.input_mask() & (1 << index) != 0 { 15 } else { 0 };
        }
        Ok(net)
    }

    pub(super) fn from_samples(source_count: usize, node_count: usize, samples: &[Vec<u8>]) -> Result<Self, &'static str> {
        if source_count > 8
            || samples.len() != 1usize << source_count
            || samples.iter().any(|row| row.len() != node_count || row.iter().any(|&value| value > 15))
            || samples[0].iter().any(|&value| value != 0)
        {
            return Err("invalid or nonzero probe baseline");
        }
        let mut links = Vec::new();
        for source in 0..source_count {
            for (target, &value) in samples[1 << source].iter().enumerate() {
                if value != 0 {
                    links.push(Link {
                        source,
                        target,
                        attenuation: 15 - value,
                        contact: None,
                    });
                }
            }
        }
        for (mask, row) in samples.iter().enumerate() {
            for (target, &actual) in row.iter().enumerate() {
                let expected = links
                    .iter()
                    .filter(|link| link.target == target && mask & (1 << link.source) != 0)
                    .map(|link| 15 - link.attenuation)
                    .max()
                    .unwrap_or(0);
                if expected != actual {
                    return Err("probe is not a weighted max net");
                }
            }
        }
        let mut net = Self::new(vec![0; source_count], vec![], 0, node_count, links, vec![], &[])?;
        net.binary_sources = true;
        Ok(net)
    }
    pub(crate) fn new(
        sources: Vec<u8>,
        contacts: Vec<usize>,
        contact_count: usize,
        node_count: usize,
        links: Vec<Link>,
        actions: Vec<Action>,
        notifications: &[(usize, usize)],
    ) -> Result<Self, &'static str> {
        if sources.iter().any(|&strength| strength > 15) || contacts.iter().any(|&contact| contact >= contact_count) {
            return Err("invalid initial source or contact");
        }
        let mut incoming = vec![Vec::new(); node_count];
        for (index, link) in links.iter().enumerate() {
            if link.source >= sources.len()
                || link.target >= node_count
                || link.attenuation > 15
                || link
                    .contact
                    .is_some_and(|(resource, contact)| resource >= contacts.len() || contact >= contact_count)
            {
                return Err("invalid link");
            }
            incoming[link.target].push(index);
        }
        for action in &actions {
            if action.input >= node_count
                || action.threshold >= 15
                || action.resource >= contacts.len()
                || action.from >= contact_count
                || action.to >= contact_count
                || action.from == action.to
            {
                return Err("invalid action");
            }
        }
        let mut contact_watchers = vec![Vec::new(); contacts.len()];
        for &(resource, action) in notifications {
            if resource >= contacts.len() || action >= actions.len() {
                return Err("invalid notification");
            }
            contact_watchers[resource].push(action);
        }
        Ok(Self {
            sources,
            binary_sources: false,
            contacts,
            links,
            actions,
            incoming,
            contact_watchers,
            cycle_consumed: None,
        })
    }

    /// Start a caller-declared computation frame. Physical clock/reset movement does not use this latch.
    pub(crate) fn begin_computation_cycle(&mut self) {
        self.cycle_consumed.get_or_insert_with(|| vec![false; self.contacts.len()]).fill(false);
    }

    pub(super) fn power(&self, target: usize) -> u8 {
        self.incoming[target]
            .iter()
            .map(|&index| self.links[index])
            .filter(|link| link.contact.is_none_or(|(resource, contact)| self.contacts[resource] == contact))
            .map(|link| self.sources[link.source].saturating_sub(link.attenuation))
            .max()
            .unwrap_or(0)
    }

    /// Validate inputs before mutation, drain the supplied FIFO, and sample at the frame boundary.
    pub(crate) fn frame(&mut self, sources: &[u8], requests: &[usize], outputs: &[usize]) -> Result<Vec<u8>, &'static str> {
        if sources.len() != self.sources.len()
            || sources
                .iter()
                .any(|&strength| strength > 15 || (self.binary_sources && strength != 0 && strength != 15))
            || requests.iter().any(|&index| index >= self.actions.len())
            || outputs.iter().any(|&index| index >= self.incoming.len())
        {
            return Err("invalid frame input");
        }
        self.sources.copy_from_slice(sources);
        let mut pending: VecDeque<_> = requests.iter().copied().collect();
        let mut frame_consumed = vec![false; self.contacts.len()];
        while let Some(index) = pending.pop_front() {
            let action = self.actions[index];
            let consumed = self.cycle_consumed.as_ref().unwrap_or(&frame_consumed);
            if consumed[action.resource]
                || self.contacts[action.resource] != action.from
                || (self.power(action.input) > action.threshold) != action.when_powered
            {
                continue;
            }
            self.contacts[action.resource] = action.to;
            if let Some(consumed) = &mut self.cycle_consumed {
                consumed[action.resource] = true;
            } else {
                frame_consumed[action.resource] = true;
            }
            // A contact change is an event even when an input's final power stays the same.
            pending.extend(self.contact_watchers[action.resource].iter().copied());
        }
        Ok(outputs.iter().map(|&target| self.power(target)).collect())
    }
}
