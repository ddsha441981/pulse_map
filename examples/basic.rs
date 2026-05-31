// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Basic PulseMap usage — insert, get, remove with typed and raw APIs.

use pulse_map::TypedPulseMap;

fn main() {
    println!("═══════════════════════════════════════════");
    println!("  PulseMap — Basic Usage Example");
    println!("═══════════════════════════════════════════\n");

    // ── 1. TypedPulseMap<String, String> ──
    let mut map: TypedPulseMap<String, String> = TypedPulseMap::new(64);

    map.insert("name".to_string(), "Deendayal".to_string());
    map.insert("lang".to_string(), "Rust".to_string());
    map.insert("project".to_string(), "PulseMap".to_string());

    println!("1. TypedPulseMap<String, String>:");
    println!("   name    = {:?}", map.get(&"name".to_string()));
    println!("   lang    = {:?}", map.get(&"lang".to_string()));
    println!("   missing = {:?}", map.get(&"nope".to_string()));
    println!("   len     = {}", map.len());
    println!();

    // ── 2. TypedPulseMap<String, u64> ──
    let mut scores: TypedPulseMap<String, u64> = TypedPulseMap::new(32);

    scores.insert("alice".to_string(), 100);
    scores.insert("bob".to_string(), 85);
    scores.insert("charlie".to_string(), 92);

    println!("2. TypedPulseMap<String, u64>:");
    println!("   alice   = {:?}", scores.get(&"alice".to_string()));
    println!("   bob     = {:?}", scores.get(&"bob".to_string()));
    println!();

    // ── 3. Remove ──
    let removed = scores.remove(&"bob".to_string());
    println!("3. Remove 'bob': {}", removed);
    println!("   bob after remove = {:?}", scores.get(&"bob".to_string()));
    println!("   len = {}", scores.len());
    println!();

    // ── 4. Stats ──
    println!("4. Stats:");
    println!("   capacity       = {}", map.capacity());
    println!("   load_factor    = {:.1}%", map.load_factor() * 100.0);
    println!("   eviction_count = {}", map.eviction_count());
    println!();

    // ── 5. Contains ──
    println!("5. Contains:");
    println!("   'name'    → {}", map.contains_key(&"name".to_string()));
    println!(
        "   'missing' → {}",
        map.contains_key(&"missing".to_string())
    );

    println!("\n✅ Done!");
}
