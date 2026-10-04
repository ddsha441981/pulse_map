//! Seeded traces adapted from the independent pulsemap-eval/workload.rs.
use rand::{rngs::StdRng, Rng, SeedableRng};
use rand_distr::{Distribution, Zipf};

pub fn trace(kind: &str, capacity: usize, count: usize, seed: u64) -> Vec<u64> {
    let mut rng = StdRng::seed_from_u64(seed);
    let space = (capacity * 16) as u64;
    let exponent = match kind {
        "zipf070" => 0.70,
        "zipf099" => 0.99,
        _ => 1.30,
    };
    let zipf = Zipf::new(space, exponent).unwrap();
    (0..count)
        .map(|i| match kind {
            "uniform" => rng.gen_range(0..space),
            "scan" => (i % (capacity * 3)) as u64,
            "hot_scan" => {
                if rng.gen_bool(0.90) {
                    rng.gen_range(0..(capacity * 3 / 4) as u64)
                } else {
                    space + i as u64
                }
            }
            // Three phases with disjoint hot sets; scans alone cannot test adaptation.
            "phase_shift" => {
                let phase = i / (count / 3).max(1);
                phase as u64 * space + rng.gen_range(0..(capacity / 4) as u64)
            }
            _ => zipf.sample(&mut rng) as u64 - 1,
        })
        .collect()
}

/// A separate RNG stream chooses writes independently of key frequency.
pub fn operations(keys: &[u64], seed: u64) -> Vec<(u64, bool)> {
    let mut rng = StdRng::seed_from_u64(seed ^ 0xD1B5_4A32_D192_ED03);
    keys.iter().map(|&k| (k, rng.gen_bool(0.05))).collect()
}

pub fn validate_capacity(cap: usize) {
    assert!(
        cap >= 64 && cap.is_power_of_two(),
        "capacity must be a power of two >= 64"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_trace_and_independent_write_decisions() {
        let keys = trace("zipf099", 1024, 100_000, 42);
        assert_eq!(keys, trace("zipf099", 1024, 100_000, 42));
        let ops = operations(&keys, 42);
        let writes = ops.iter().filter(|(_, write)| *write).count();
        assert!((4500..5500).contains(&writes));
        // The hottest key must appear as both read and write, unlike key % 20.
        assert!(ops.contains(&(0, true)) && ops.contains(&(0, false)));
        assert_eq!(ops, operations(&keys, 42));
    }

    #[test]
    fn phase_trace_changes_working_set() {
        let keys = trace("phase_shift", 1024, 300, 42);
        assert!(keys[..100].iter().all(|&k| k < 256));
        assert!(keys[100..200].iter().all(|&k| (16384..16640).contains(&k)));
    }

    #[test]
    #[should_panic(expected = "power of two")]
    fn reject_rounded_capacity_comparisons() {
        validate_capacity(10_000);
    }
}
