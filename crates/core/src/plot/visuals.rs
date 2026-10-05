use mchprs_save_data::plot_data::PistonAnimation;
use mchprs_save_data::plot_data::Tps;

pub(super) fn fast_rendering(tps: Tps, threshold: i64) -> bool {
    threshold > 0
        && match tps {
            Tps::Limited(rate) => i64::from(rate) > threshold,
            Tps::Unlimited => true,
        }
}

pub(super) fn static_pistons(mode: PistonAnimation, tps: Tps, threshold: i64) -> bool {
    match mode {
        PistonAnimation::Auto => fast_rendering(tps, threshold),
        PistonAnimation::On => false,
        PistonAnimation::Off => true,
    }
}

pub(super) fn send_rate(configured: u32, fast: bool, cap: i64) -> u32 {
    if fast {
        configured.min(cap.clamp(1, 1000) as u32)
    } else {
        configured
    }
}

pub(super) fn visual_send_rate(configured: u32, tps: Tps, threshold: i64, cap: i64) -> u32 {
    send_rate(configured, fast_rendering(tps, threshold), cap)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rendering_threshold_and_rate_preserve_user_limits() {
        assert!(!static_pistons(PistonAnimation::On, Tps::Unlimited, 100));
        assert!(static_pistons(PistonAnimation::Off, Tps::Limited(20), 0));
        assert!(!static_pistons(
            PistonAnimation::Auto,
            Tps::Limited(20),
            100
        ));
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

    #[test]
    fn general_updates_are_capped_independently_of_piston_animation() {
        let rate = |configured, tps| visual_send_rate(configured, tps, 200, 10);
        assert_eq!(rate(60, Tps::Limited(200)), 60);
        assert_eq!(rate(60, Tps::Limited(201)), 10);
        assert_eq!(rate(60, Tps::Limited(1000)), 10);
        assert_eq!(rate(60, Tps::Limited(1001)), 10);
        assert_eq!(rate(60, Tps::Unlimited), 10);
        assert_eq!(rate(3, Tps::Unlimited), 3);
        assert_eq!(rate(0, Tps::Unlimited), 0);
        assert_eq!(rate(60, Tps::Limited(0)), 60);
        assert_eq!(visual_send_rate(60, Tps::Unlimited, 0, 10), 60);
        assert_eq!(visual_send_rate(60, Tps::Unlimited, 200, 5), 5);
        assert!(!static_pistons(PistonAnimation::On, Tps::Unlimited, 200));
        assert_eq!(rate(60, Tps::Unlimited), 10);
    }
}
