//! Memory baseline benchmark.
//!
//! Measures actual resident memory (RSS) for each cache:
//!   1. Right after `new(capacity)` — allocated-but-possibly-lazy footprint
//!   2. After filling to 100% capacity — actual resident footprint once
//!      every page has been touched (this is the honest "per-entry cost"
//!      number, since Linux uses lazy page commit and an untouched
//!      allocation won't show up in RSS even if it was "allocated").
//!
//! Each (cache, capacity) combination runs in its OWN child process, so
//! allocator arena growth/reuse from a previous test can't contaminate the
//! next one's numbers. The child just does the allocation and prints its
//! own RSS; the parent process orchestrates and prints the report.
//!
//! Cargo.toml:
//!   pulse_map = { path = "../pulse_map" }
//!   moka = { version = "0.12", features = ["sync"] }
//!   quick_cache = "0.6"
//!   lru = "0.12"
//!
//! Run:
//!   cargo run --release --example memory_baseline

use lru::LruCache;
use moka::sync::Cache as MokaCache;
use pulse_map::ShardedPulseMap;
use quick_cache::sync::Cache as QuickCache;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::num::NonZeroUsize;
use std::process::Command;

const CAPACITIES: [usize; 3] = [100_000, 500_000, 1_000_000];
const CACHE_NAMES: [&str; 5] = ["pulsemap", "quickcache", "lru", "simple", "moka"];

fn rss_kb() -> u64 {
    if let Ok(status) = fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                if let Some(num) = rest.split_whitespace().next() {
                    return num.parse().unwrap_or(0);
                }
            }
        }
    }
    0
}

/// Child-process entry point: allocate one cache, optionally fill it,
/// then print its own RSS and exit. Reading RSS in a fresh process means
/// there's no leftover heap fragmentation or arena growth from any other
/// test to skew the number.
fn run_child(cache_name: &str, capacity: usize, fill: bool) {
    // baseline RSS before doing anything (process startup cost, ~1-2MB typically)
    let base = rss_kb();

    match cache_name {
        "pulsemap" => {
            let map = ShardedPulseMap::<u32, u32>::new(capacity / 64);
            if fill {
                for i in 0..capacity as u32 {
                    map.insert(i, i);
                }
            }
            println!("RSS_KB={} BASE_KB={}", rss_kb(), base);
        }
        "quickcache" => {
            let cache = QuickCache::<u32, u32>::new(capacity);
            if fill {
                for i in 0..capacity as u32 {
                    cache.insert(i, i);
                }
            }
            println!("RSS_KB={} BASE_KB={}", rss_kb(), base);
        }
        "lru" => {
            let mut cache = LruCache::<u32, u32>::new(NonZeroUsize::new(capacity).unwrap());
            if fill {
                for i in 0..capacity as u32 {
                    cache.put(i, i);
                }
            }
            println!("RSS_KB={} BASE_KB={}", rss_kb(), base);
        }
        "simple" => {
            let mut cache: HashMap<u32, u32> = HashMap::with_capacity(capacity);
            if fill {
                for i in 0..capacity as u32 {
                    cache.insert(i, i);
                }
            }
            println!("RSS_KB={} BASE_KB={}", rss_kb(), base);
        }
        "moka" => {
            let cache = MokaCache::builder()
                .max_capacity(capacity as u64)
                .initial_capacity(capacity)
                .build();
            if fill {
                for i in 0..capacity as u32 {
                    cache.insert(i, i);
                }
                cache.run_pending_tasks(); // flush moka's internal maintenance queue
            }
            println!("RSS_KB={} BASE_KB={}", rss_kb(), base);
        }
        _ => panic!("unknown cache {cache_name}"),
    }
}

fn parse_child_output(stdout: &str) -> Option<(u64, u64)> {
    // expects a line like: RSS_KB=12345 BASE_KB=1234
    for line in stdout.lines() {
        if line.starts_with("RSS_KB=") {
            let mut rss = None;
            let mut base = None;
            for part in line.split_whitespace() {
                if let Some(v) = part.strip_prefix("RSS_KB=") {
                    rss = v.parse().ok();
                } else if let Some(v) = part.strip_prefix("BASE_KB=") {
                    base = v.parse().ok();
                }
            }
            if let (Some(r), Some(b)) = (rss, base) {
                return Some((r, b));
            }
        }
    }
    None
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // Child mode: --child <cache_name> <capacity> <fill: 0|1>
    if args.len() == 5 && args[1] == "--child" {
        let cache_name = &args[2];
        let capacity: usize = args[3].parse().unwrap();
        let fill = args[4] == "1";
        run_child(cache_name, capacity, fill);
        return;
    }

    // Parent/orchestrator mode
    println!("💾 MEMORY BASELINE BENCHMARK 💾");
    println!("Each (cache, capacity) pair runs in its own fresh process — no cross-test contamination.\n");

    let exe = env::current_exe().expect("can't find own binary path");

    println!(
        "{:<12} {:>12} {:>18} {:>18} {:>14}",
        "Cache", "Capacity", "Empty RSS (MB)", "Filled RSS (MB)", "Bytes/entry"
    );
    println!("{}", "-".repeat(78));

    for &name in CACHE_NAMES.iter() {
        for &cap in CAPACITIES.iter() {
            let empty_out = Command::new(&exe)
                .args(["--child", name, &cap.to_string(), "0"])
                .output()
                .expect("failed to spawn child");
            let filled_out = Command::new(&exe)
                .args(["--child", name, &cap.to_string(), "1"])
                .output()
                .expect("failed to spawn child");

            let (empty_rss, empty_base) =
                parse_child_output(&String::from_utf8_lossy(&empty_out.stdout)).unwrap_or((0, 0));
            let (filled_rss, filled_base) =
                parse_child_output(&String::from_utf8_lossy(&filled_out.stdout)).unwrap_or((0, 0));

            let empty_mb = (empty_rss as f64 - empty_base as f64) / 1024.0;
            let filled_mb = (filled_rss as f64 - filled_base as f64) / 1024.0;
            let bytes_per_entry = (filled_mb * 1024.0 * 1024.0) / cap as f64;

            println!(
                "{:<12} {:>12} {:>16.2}MB {:>16.2}MB {:>12.1}B",
                name, cap, empty_mb, filled_mb, bytes_per_entry
            );
        }
        println!();
    }

    println!("Notes:");
    println!("- 'Empty RSS' is the footprint right after new(capacity), before any inserts.");
    println!("  Fixed-capacity structures that pre-allocate and zero their storage (PulseMap,");
    println!("  quick_cache) will show most of their footprint here already.");
    println!("- 'Filled RSS' is after inserting up to 100% capacity — the honest steady-state");
    println!("  number, since Linux won't count untouched allocated pages toward RSS.");
    println!("- 'Bytes/entry' = filled RSS / capacity. Compare this directly against README");
    println!(
        "  claims like '14B packed slot vs 48B LRU pointers' — this is real, not theoretical."
    );
}
