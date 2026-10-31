#[derive(Clone, Copy)]
pub(super) struct InputState {
    pub(super) active: u32,
    pub(super) analog: u32,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            active: 0,
            analog: u32::MAX,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Histogram {
    counts: [u32; 16],
    levels: u16,
}

impl InputState {
    pub(super) fn change(&mut self, old: u8, new: u8, histograms: &mut [Histogram]) {
        debug_assert!(old <= 15 && new <= 15);
        if old == new {
            return;
        }
        if old != 0 {
            self.active -= 1;
        }
        if new != 0 {
            self.active += 1;
        }
        if let Some(histogram) = histograms.get_mut(self.analog as usize) {
            if old != 0 {
                histogram.counts[old as usize] -= 1;
                if histogram.counts[old as usize] == 0 {
                    histogram.levels &= !(1 << old);
                }
            }
            if new != 0 {
                histogram.counts[new as usize] += 1;
                histogram.levels |= 1 << new;
            }
        }
    }

    pub(super) fn maximum(&self, histograms: &[Histogram]) -> u8 {
        match histograms.get(self.analog as usize) {
            Some(histogram) if histogram.levels != 0 => histogram.levels.ilog2() as u8,
            Some(_) => 0,
            None => {
                if self.active == 0 {
                    0
                } else {
                    15
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_strength_changes_preserve_competing_contributions() {
        assert_eq!(std::mem::size_of::<InputState>(), 8);
        for old in 0..=15 {
            for new in 0..=15 {
                let mut input = InputState {
                    analog: 0,
                    ..Default::default()
                };
                let mut histograms = [Histogram::default()];
                input.change(0, 8, &mut histograms);
                input.change(0, 8, &mut histograms);
                input.change(0, old, &mut histograms);
                input.change(old, new, &mut histograms);
                assert_eq!(input.maximum(&histograms), new.max(8));
                assert_eq!(input.active, 2 + u32::from(new != 0));
                input.change(8, 0, &mut histograms);
                input.change(8, 0, &mut histograms);
                assert_eq!(input.maximum(&histograms), new);
                input.change(new, 0, &mut histograms);
                assert_eq!(input.active, 0);
                assert_eq!(histograms[0].counts, [0; 16]);
            }
        }
    }
}
