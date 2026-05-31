// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Concurrent PulseMap — multi-threaded usage with Arc.

use pulse_map::ConcurrentPulseMap;
use std::sync::Arc;
use std::thread;

fn main() {
    println!("═══════════════════════════════════════════");
    println!("  ConcurrentPulseMap — Multi-threaded Example");
    println!("═══════════════════════════════════════════\n");

    // Create a thread-safe map with auto-resize
    let map = Arc::new(ConcurrentPulseMap::with_auto_resize(64));

    // ── Spawn 4 writer threads ──
    let mut handles = vec![];

    for thread_id in 0..4 {
        let map = Arc::clone(&map);
        handles.push(thread::spawn(move || {
            for i in 0..250 {
                let key = format!("t{}_key_{}", thread_id, i);
                let val = format!("value_{}", i * thread_id);
                map.insert(key, val);
            }
            println!("  Thread {} — wrote 250 entries", thread_id);
        }));
    }

    // Wait for all writers
    for h in handles {
        h.join().unwrap();
    }

    println!("\n  Total entries: {}", map.len());
    println!("  Capacity:     {}", map.capacity());
    println!("  Load factor:  {:.1}%", map.load_factor() * 100.0);
    println!("  Evictions:    {}", map.eviction_count());

    // ── Concurrent reads ──
    let mut readers = vec![];
    for thread_id in 0..4 {
        let map = Arc::clone(&map);
        readers.push(thread::spawn(move || {
            let mut found = 0;
            for i in 0..250 {
                let key = format!("t{}_key_{}", thread_id, i);
                if map.get(&key).is_some() {
                    found += 1;
                }
            }
            println!("  Reader {} — found {}/250", thread_id, found);
            found
        }));
    }

    let total_found: usize = readers.into_iter().map(|h| h.join().unwrap()).sum();
    println!("\n  Total reads:  {}/1000", total_found);
    println!("\n✅ Thread-safe operations complete!");
}
