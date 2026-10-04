use mchprs_save_data::plot_data::Tps;

pub(super) fn fast_rendering(tps: Tps, threshold: i64) -> bool {
    threshold > 0
        && match tps {
            Tps::Limited(rate) => i64::from(rate) > threshold,
            Tps::Unlimited => true,
        }
}

pub(super) fn send_rate(configured: u32, fast: bool, cap: i64) -> u32 {
    if fast {
        configured.min(cap.clamp(1, 1000) as u32)
    } else {
        configured
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rendering_threshold_and_rate_preserve_user_limits() {
        assert!(!fast_rendering(Tps::Limited(100), 100));
        assert!(fast_rendering(Tps::Limited(101), 100));
        assert!(fast_rendering(Tps::Unlimited, 100));
        assert!(!fast_rendering(Tps::Unlimited, 0));
        assert!(!fast_rendering(Tps::Limited(0), 100));
        assert_eq!(send_rate(60, true, 10), 10);
        assert_eq!(send_rate(5, true, 10), 5);
        assert_eq!(send_rate(0, true, 10), 0);
        assert_eq!(send_rate(60, false, 10), 60);
        assert_eq!(send_rate(60, true, -1), 1);
    }
}
