use rand::{RngExt, SeedableRng, rngs::StdRng};
use std::env;

fn main() {
    let args: Vec<_> = env::args().collect();
    let id = &args[1];
    let lever_count: usize = args[2].parse().unwrap();
    assert!(lever_count > 0);
    let seed = id.as_bytes().iter().fold(0u64, |seed, byte| seed.wrapping_mul(31).wrapping_add(u64::from(*byte)));
    let mut rng = StdRng::seed_from_u64(seed);
    let base: Vec<usize> = if lever_count <= 3 {
        (1..(1usize << lever_count)).map(|step| step.trailing_zeros() as usize)
            .chain(std::iter::once(lever_count - 1)).collect()
    } else {
        (0..lever_count).chain((0..lever_count).rev()).collect()
    };
    let mut actions: Vec<_> = base.into_iter().map(|index| (index, 20)).collect();
    actions.extend((0..32).map(|_| (rng.random_range(0..lever_count), rng.random_range(1..=8))));
    assert_eq!(actions.len(), (if lever_count <= 3 { (1usize << lever_count) } else { 2 * lever_count }) + 32);
    assert!(actions.iter().all(|(index, wait)| *index < lever_count && (1..=20).contains(wait)));
    print!("[");
    for (step, (index, wait)) in actions.iter().enumerate() {
        if step > 0 { print!(","); }
        print!("{{\"step\":{step},\"index\":{index},\"wait\":{wait}}}");
    }
    println!("]");
}

