// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! 24-hour soak / endurance test for `ShardedPulseMap`.
//!
//! Verifies stability under sustained concurrent load: no panic, no deadlock,
//! monotonic eviction counts, bounded `len()`, RSS growth within tolerance,
//! and sentinel key integrity before TTL expiry.
//!
//! ```bash
//! # Full 24-hour soak
//! cargo run --release --example soak_test --features std
//!
//! # Quick smoke (default CI-friendly duration: 5s unless --duration is set)
//! cargo run --release --example soak_test --features std -- --duration 5
//!
//! # 1-hour CI smoke
//! cargo run --release --example soak_test --features std -- --duration 3600
//! ```

use pulse_map::ShardedPulseMap;
use std::env;
use std::process;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Default duration: 24 hours (per issue). Override with `--duration <secs>`.
const DEFAULT_DURATION_SECS: u64 = 24 * 3600;
/// Minimum duration accepted by CLI (issue asks for min 1h for CI; we allow
/// shorter values so local/PR smoke can finish quickly).
const MIN_DURATION_SECS: u64 = 1;
/// Stats + integrity check interval.
const STATS_INTERVAL: Duration = Duration::from_secs(60);
/// When total duration is under 2 minutes, print stats more often.
const SHORT_STATS_INTERVAL: Duration = Duration::from_secs(1);
/// Writer / reader thread counts from the issue.
const NUM_WRITERS: usize = 8;
const NUM_READERS: usize = 4;
/// Buckets per shard (issue: 4096).
const BUCKETS_PER_SHARD: usize = 4096;
/// Global TTL in insertion epochs (issue: 10_000).
const TTL_EPOCHS: u64 = 10_000;
/// Sentinel keys live this many epochs (short enough to re-check often, long
/// enough that a concurrent writer wave does not expire them mid-check).
const SENTINEL_TTL: u64 = 50_000;
const SENTINEL_COUNT: u64 = 32;
/// Fail if RSS grows more than this fraction from the first sample.
const RSS_GROWTH_LIMIT: f64 = 0.10;

fn parse_duration_secs(args: &[String]) -> u64 {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--duration" {
            if let Some(v) = args.get(i + 1) {
                match v.parse::<u64>() {
                    Ok(n) if n >= MIN_DURATION_SECS => return n,
                    Ok(_) => {
                        eprintln!("error: --duration must be >= {MIN_DURATION_SECS} seconds");
                        process::exit(2);
                    }
                    Err(_) => {
                        eprintln!("error: invalid --duration value: {v}");
                        process::exit(2);
                    }
                }
            }
            eprintln!("error: --duration requires a value in seconds");
            process::exit(2);
        }
        if let Some(rest) = args[i].strip_prefix("--duration=") {
            match rest.parse::<u64>() {
                Ok(n) if n >= MIN_DURATION_SECS => return n,
                Ok(_) => {
                    eprintln!("error: --duration must be >= {MIN_DURATION_SECS} seconds");
                    process::exit(2);
                }
                Err(_) => {
                    eprintln!("error: invalid --duration value: {rest}");
                    process::exit(2);
                }
            }
        }
        i += 1;
    }
    DEFAULT_DURATION_SECS
}

/// Best-effort RSS in bytes. Linux: `/proc/self/status` VmRSS.
/// Other platforms: returns `None` (RSS check skipped).
fn read_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return Some(kb.saturating_mul(1024));
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn fmt_bytes(n: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    format!("{:.1}MB", n as f64 / MB)
}

fn fmt_elapsed(d: Duration) -> String {
    let total = d.as_secs();
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

fn fmt_ops(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.2}B", n as f64 / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1e3)
    } else {
        format!("{n}")
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let duration_secs = parse_duration_secs(&args);
    let duration = Duration::from_secs(duration_secs);
    let stats_every = if duration_secs < 120 {
        SHORT_STATS_INTERVAL
    } else {
        STATS_INTERVAL
    };

    println!("═══════════════════════════════════════════");
    println!("  ShardedPulseMap soak test");
    println!(
        "  duration={}s writers={NUM_WRITERS} readers={NUM_READERS}",
        duration_secs
    );
    println!("  buckets/shard={BUCKETS_PER_SHARD} ttl={TTL_EPOCHS}");
    println!("═══════════════════════════════════════════\n");

    let map = Arc::new(ShardedPulseMap::<u64, u64>::new(BUCKETS_PER_SHARD));
    map.set_ttl(TTL_EPOCHS);

    let stop = Arc::new(AtomicBool::new(false));
    let ops = Arc::new(AtomicU64::new(0));
    let fail = Arc::new(AtomicBool::new(false));
    let fail_reason = Arc::new(std::sync::Mutex::new(String::new()));

    // ── Writers ──
    let mut handles = Vec::new();
    for tid in 0..NUM_WRITERS {
        let map = Arc::clone(&map);
        let stop = Arc::clone(&stop);
        let ops = Arc::clone(&ops);
        handles.push(
            thread::Builder::new()
                .name(format!("writer-{tid}"))
                .spawn(move || {
                    let mut i: u64 = tid as u64;
                    while !stop.load(Ordering::Relaxed) {
                        // Spread keys so shards fill under TTL churn.
                        let key = i.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (tid as u64) << 48;
                        map.insert(key, key ^ 0xDEAD_BEEF);
                        ops.fetch_add(1, Ordering::Relaxed);
                        i = i.wrapping_add(NUM_WRITERS as u64);
                    }
                })
                .expect("spawn writer"),
        );
    }

    // ── Readers ──
    for tid in 0..NUM_READERS {
        let map = Arc::clone(&map);
        let stop = Arc::clone(&stop);
        let ops = Arc::clone(&ops);
        handles.push(
            thread::Builder::new()
                .name(format!("reader-{tid}"))
                .spawn(move || {
                    let mut i: u64 = tid as u64;
                    while !stop.load(Ordering::Relaxed) {
                        let key = i.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                            ^ ((tid as u64) % NUM_WRITERS as u64) << 48;
                        let _ = map.get(&key);
                        ops.fetch_add(1, Ordering::Relaxed);
                        i = i.wrapping_add(NUM_READERS as u64);
                    }
                })
                .expect("spawn reader"),
        );
    }

    let start = Instant::now();
    let mut last_stats = Instant::now();
    let mut last_evictions: usize = 0;
    let mut initial_rss: Option<u64> = None;
    let mut last_rss: Option<u64> = None;
    let mut tick: u64 = 0;

    // Prime a few inserts so capacity/len are meaningful before first sample.
    for s in 0..SENTINEL_COUNT {
        map.insert_ttl(u64::MAX - s, s.wrapping_mul(3) + 1, SENTINEL_TTL);
    }
    thread::sleep(Duration::from_millis(50));

    while start.elapsed() < duration && !fail.load(Ordering::SeqCst) {
        let sleep_for = stats_every
            .checked_sub(last_stats.elapsed())
            .unwrap_or(Duration::from_millis(0));
        if !sleep_for.is_zero() {
            thread::sleep(sleep_for.min(Duration::from_millis(200)));
        }
        if last_stats.elapsed() < stats_every && start.elapsed() < duration {
            continue;
        }
        last_stats = Instant::now();
        tick += 1;

        let len = map.len();
        let cap = map.capacity();
        let load = if cap == 0 {
            0.0
        } else {
            (len as f64 / cap as f64) * 100.0
        };
        let evictions = map.eviction_count();
        let total_ops = ops.load(Ordering::Relaxed);
        let rss = read_rss_bytes();
        if initial_rss.is_none() {
            initial_rss = rss;
        }
        last_rss = rss;

        let rss_s = rss.map(fmt_bytes).unwrap_or_else(|| "n/a".into());
        println!(
            "[{}] len={len} cap={cap} load={load:.1}% evictions={evictions} rss={rss_s} ops={}",
            fmt_elapsed(start.elapsed()),
            fmt_ops(total_ops)
        );

        // Failure: len exceeds capacity
        if len > cap {
            let msg = format!("len ({len}) exceeded capacity ({cap})");
            eprintln!("FAIL: {msg}");
            let mut g = fail_reason.lock().unwrap();
            if g.is_empty() {
                *g = msg;
            }
            fail.store(true, Ordering::SeqCst);
            break;
        }

        // Failure: eviction_count must be monotonic (never wraps/resets downward)
        if evictions < last_evictions {
            let msg = format!("eviction_count decreased: {last_evictions} -> {evictions}");
            eprintln!("FAIL: {msg}");
            let mut g = fail_reason.lock().unwrap();
            if g.is_empty() {
                *g = msg;
            }
            fail.store(true, Ordering::SeqCst);
            break;
        }
        last_evictions = evictions;

        // Failure: RSS growth > 10% from initial (Linux only)
        if let (Some(init), Some(now)) = (initial_rss, rss) {
            if init > 0 {
                let growth = (now as f64 - init as f64) / init as f64;
                if growth > RSS_GROWTH_LIMIT {
                    let msg = format!(
                        "RSS grew {:.1}% ({} -> {}), limit {:.0}%",
                        growth * 100.0,
                        fmt_bytes(init),
                        fmt_bytes(now),
                        RSS_GROWTH_LIMIT * 100.0
                    );
                    eprintln!("FAIL: {msg}");
                    let mut g = fail_reason.lock().unwrap();
                    if g.is_empty() {
                        *g = msg;
                    }
                    fail.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }

        // Sentinel integrity: re-insert then verify immediately
        for s in 0..SENTINEL_COUNT {
            let k = u64::MAX - s;
            let v = s.wrapping_mul(3) + 1 + tick; // rotate expected value
            map.insert_ttl(k, v, SENTINEL_TTL);
            match map.get(&k) {
                Some(got) if got == v => {}
                Some(got) => {
                    let msg = format!("sentinel {k} wrong value: got {got}, want {v}");
                    eprintln!("FAIL: {msg}");
                    let mut g = fail_reason.lock().unwrap();
                    if g.is_empty() {
                        *g = msg;
                    }
                    fail.store(true, Ordering::SeqCst);
                    break;
                }
                None => {
                    let msg = format!("sentinel {k} missing after insert");
                    eprintln!("FAIL: {msg}");
                    let mut g = fail_reason.lock().unwrap();
                    if g.is_empty() {
                        *g = msg;
                    }
                    fail.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
        if fail.load(Ordering::SeqCst) {
            break;
        }
    }

    stop.store(true, Ordering::SeqCst);
    for h in handles {
        // Writers/readers exit promptly after stop; join to surface panics.
        if let Err(e) = h.join() {
            eprintln!("FAIL: worker thread panicked: {e:?}");
            fail.store(true, Ordering::SeqCst);
            let mut g = fail_reason.lock().unwrap();
            if g.is_empty() {
                *g = format!("worker panic: {e:?}");
            }
        }
    }

    let elapsed = start.elapsed();
    let total_ops = ops.load(Ordering::Relaxed);
    let rss_drift = match (initial_rss, last_rss) {
        (Some(a), Some(b)) => {
            let delta = b as i64 - a as i64;
            let sign = if delta >= 0 { "+" } else { "-" };
            format!("{sign}{}", fmt_bytes(delta.unsigned_abs()))
        }
        _ => "n/a".into(),
    };

    if fail.load(Ordering::SeqCst) {
        let reason = fail_reason.lock().unwrap().clone();
        eprintln!(
            "[{}] FAILED — {reason}. Total ops: {}, RSS drift: {rss_drift}",
            fmt_elapsed(elapsed),
            fmt_ops(total_ops)
        );
        process::exit(1);
    }

    println!(
        "[{}] PASSED — soak complete. Total ops: {}, RSS drift: {rss_drift}",
        fmt_elapsed(elapsed),
        fmt_ops(total_ops)
    );
}
