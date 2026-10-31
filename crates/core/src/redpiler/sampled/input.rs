#[derive(Clone, Copy, Default)]
pub(super) struct InputState {
    counts: [u32; 16],
    levels: u16,
    pub(super) active: u32,
}

impl InputState {
    pub(super) fn change(&mut self, old: u8, new: u8) {
        debug_assert!(old <= 15 && new <= 15);
        if old == new {
            return;
        }
        if old != 0 {
            self.counts[old as usize] -= 1;
            self.active -= 1;
            if self.counts[old as usize] == 0 {
                self.levels &= !(1 << old);
            }
        }
        if new != 0 {
            self.counts[new as usize] += 1;
            self.active += 1;
            self.levels |= 1 << new;
        }
    }

    pub(super) fn maximum(&self) -> u8 {
        if self.levels == 0 { 0 } else { self.levels.ilog2() as u8 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_strength_changes_preserve_competing_contributions() {
        for old in 0..=15 {
            for new in 0..=15 {
                let mut input = InputState::default();
                input.change(0, 8);
                input.change(0, 8);
                input.change(0, old);
                input.change(old, new);
                assert_eq!(input.maximum(), new.max(8));
                assert_eq!(input.active, 2 + u32::from(new != 0));
                input.change(8, 0);
                assert_eq!(input.maximum(), new.max(8));
                input.change(8, 0);
                assert_eq!(input.maximum(), new);
                input.change(new, 0);
                assert_eq!(input.active, 0);
                assert_eq!(input.maximum(), 0);
                assert_eq!(input.counts, [0; 16]);
            }
        }
    }
}
