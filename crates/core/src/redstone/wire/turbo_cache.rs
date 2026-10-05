use super::NodeId;

#[derive(Clone, Copy, Default)]
struct Stamp {
    epoch: u32,
    node: u32,
}

#[derive(Default)]
pub(super) struct SpatialNodes {
    sections: Vec<Option<Box<[Stamp; 4096]>>>,
    epoch: u32,
}

impl SpatialNodes {
    pub fn begin(&mut self) {
        if let Some(next) = self.epoch.checked_add(1) {
            self.epoch = next;
        } else {
            for section in self.sections.iter_mut().flatten() {
                section.fill(Stamp::default());
            }
            self.epoch = 1;
        }
    }

    pub fn get(&self, cell: u32) -> Option<NodeId> {
        let section = self.sections.get((cell >> 12) as usize)?.as_ref()?;
        let stamp = section[(cell & 4095) as usize];
        (stamp.epoch == self.epoch).then_some(NodeId { index: stamp.node })
    }

    pub fn put(&mut self, cell: u32, node: NodeId) {
        let section = (cell >> 12) as usize;
        if self.sections.len() <= section {
            self.sections.resize_with(section + 1, || None);
        }
        let section =
            self.sections[section].get_or_insert_with(|| Box::new([Stamp::default(); 4096]));
        section[(cell & 4095) as usize] = Stamp {
            epoch: self.epoch,
            node: node.index,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn epochs_boundaries_and_wrap_do_not_reuse_nodes() {
        let mut nodes = SpatialNodes::default();
        nodes.begin();
        for cell in [0, 4095, 4096, 16_777_215] {
            nodes.put(cell, NodeId { index: cell + 1 });
        }
        for cell in [0, 4095, 4096, 16_777_215] {
            assert_eq!(nodes.get(cell).unwrap().index, cell + 1);
        }
        nodes.begin();
        assert!(nodes.get(0).is_none());
        nodes.epoch = u32::MAX;
        nodes.begin();
        assert!(nodes.get(4096).is_none());
    }
}
