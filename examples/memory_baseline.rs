//! Fresh-process Linux RSS, u32/u32. Equal actual nominal capacities, with both
//! bytes/slot and bytes/resident. Inserting capacity keys need not fill every slot.
//! See evaluation/ for paired versions and separate u64/u64 (slab) measurements.
use lru::LruCache;
use pulse_map::ShardedPulseMap;
use std::{hint::black_box, num::NonZeroUsize, process::Command};

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .expect("Linux /proc required")
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .expect("VmRSS missing")
}

fn child(name: &str, capacity: usize) {
    assert!(capacity >= 64 && capacity.is_power_of_two());
    let base = rss_kib();
    macro_rules! measure {
        ($cache:expr, $insert:ident, $len:ident, $settle:expr) => {{
            #[allow(unused_mut)]
            let mut cache = $cache;
            let empty = rss_kib();
            for k in 0..capacity as u32 {
                cache.$insert(k, k);
            }
            ($settle)(&cache);
            let resident = cache.$len() as usize;
            let filled = rss_kib();
            black_box(&cache);
            let bytes = filled.saturating_sub(base) * 1024;
            println!(
                "{name},u32,{capacity},{capacity},{resident},{base},{empty},{filled},{:.2},{:.2}",
                bytes as f64 / capacity as f64,
                bytes as f64 / resident.max(1) as f64
            );
        }};
    }
    match name {
        "pulsemap" => measure!(
            ShardedPulseMap::<u32, u32>::new(capacity / 64),
            insert,
            len,
            |_| {}
        ),
        "quickcache" => measure!(
            quick_cache::sync::Cache::<u32, u32>::new(capacity),
            insert,
            len,
            |_| {}
        ),
        "lru" => measure!(
            LruCache::<u32, u32>::new(NonZeroUsize::new(capacity).unwrap()),
            put,
            len,
            |_| {}
        ),
        "moka" => measure!(
            moka::sync::Cache::<u32, u32>::builder()
                .max_capacity(capacity as u64)
                .initial_capacity(capacity)
                .build(),
            insert,
            entry_count,
            |c: &moka::sync::Cache<u32, u32>| c.run_pending_tasks()
        ),
        _ => panic!("unknown cache: {name}"),
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--child") {
        child(&args[2], args[3].parse().unwrap());
        return;
    }
    println!("cache,type,requested,actual_capacity,resident,base_kib,empty_kib,filled_kib,bytes_per_slot,bytes_per_resident");
    let caps: &[usize] = if args.iter().any(|x| x == "--smoke") {
        &[65536]
    } else {
        &[131072, 524288, 1048576]
    };
    for &cap in caps {
        for name in ["pulsemap", "quickcache", "lru", "moka"] {
            assert!(Command::new(std::env::current_exe().unwrap())
                .args(["--child", name, &cap.to_string()])
                .status()
                .unwrap()
                .success());
        }
    }
}
