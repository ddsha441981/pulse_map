use pulse_map::ShardedPulseMap;
use moka::sync::Cache as MokaCache;
use quick_cache::sync::Cache as QuickCache;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

fn main() {
    println!("🚀 RUNNING ALL CONSOLIDATED PULSE_MAP SCENARIOS 🚀\n");
    
    run_ttl_expiry();
    println!("--------------------------------------------------");
    run_high_eviction();
    println!("--------------------------------------------------");
    run_large_payloads();
    println!("--------------------------------------------------");
    run_vip_protection();
    println!("--------------------------------------------------");
    run_competitor_comparison();
    println!("--------------------------------------------------");
    run_stress_test();
    
    println!("\n✅ ALL SCENARIOS COMPLETED SUCCESSFULLY!");
}

fn run_ttl_expiry() {
    println!("🧪 Scenario 1: TTL Expiry & Epoch Tracking");
    let map = Arc::new(ShardedPulseMap::<String, String>::new(128));
    map.set_ttl(10);
    
    map.insert("global_ttl_key".to_string(), "will_expire".to_string());
    map.insert_ttl("short_ttl_key".to_string(), "fast_expire".to_string(), 5);
    map.insert_ttl("immortal_key".to_string(), "lives_forever".to_string(), u32::MAX);

    for i in 0..11 {
        map.insert(format!("dummy_{}", i), "data".to_string());
    }
    // Just verifying no panics and functionality completes
    println!("   - TTL test executed.");
}

fn run_high_eviction() {
    println!("🧪 Scenario 2: High Eviction (Fixed Capacity)");
    let map = Arc::new(ShardedPulseMap::<u32, u32>::new(16));
    let mut handles = vec![];
    for t in 0..4 {
        let m = Arc::clone(&map);
        handles.push(thread::spawn(move || {
            for i in 0..20_000 {
                m.insert((t * 10_000) + (i % 10_000), i);
            }
        }));
    }
    for h in handles { h.join().unwrap(); }
    assert!(map.len() <= map.capacity());
    println!("   - Map Capacity Maintained: {} (Evictions: {})", map.capacity(), map.eviction_count());
}

fn run_large_payloads() {
    println!("🧪 Scenario 3: Large Payloads & Slab Contention");
    let map = Arc::new(ShardedPulseMap::<u32, Vec<u8>>::with_auto_resize(64));
    let mut handles = vec![];
    for t in 0..4 {
        let m = Arc::clone(&map);
        handles.push(thread::spawn(move || {
            for i in 0..500 {
                m.insert(t * 1000 + i, vec![(i % 255) as u8; 10 * 1024]);
            }
        }));
    }
    for h in handles { h.join().unwrap(); }
    println!("   - Large payload slab allocation survived concurrency.");
}

fn run_vip_protection() {
    println!("🧪 Scenario 4: Bot Flood vs VIP Protection");
    let map = Arc::new(ShardedPulseMap::<u32, u32>::new(16));
    for vip_id in 0..10 {
        for _ in 0..500 { map.insert(vip_id, 9999); }
    }
    
    let mut handles = vec![];
    for t in 0..10 {
        let m = Arc::clone(&map);
        handles.push(thread::spawn(move || {
            for i in 0..2_000 { m.insert(1000 + (t * 2_000) + i, 1); }
        }));
    }
    for h in handles { h.join().unwrap(); }
    
    let mut vips_survived = 0;
    for vip_id in 0..10 {
        if map.get(&vip_id).is_some() { vips_survived += 1; }
    }
    println!("   - VIP Survival Rate: {}/10 against cache pollution", vips_survived);
}

fn run_competitor_comparison() {
    println!("🧪 Scenario 5: VIP Survival vs Moka vs QuickCache");
    let pulse_map = Arc::new(ShardedPulseMap::<u32, u32>::new(16));
    let moka_cache = Arc::new(MokaCache::builder().max_capacity(1024).build());
    let quick_cache = Arc::new(QuickCache::new(1024));

    for vip_id in 0..10 {
        for _ in 0..5 {
            pulse_map.insert(vip_id, 9999);
            moka_cache.insert(vip_id, 9999);
            quick_cache.insert(vip_id, 9999);
        }
    }
    
    moka_cache.run_pending_tasks();

    let mut handles = vec![];
    for t in 0..10 {
        let p_map = Arc::clone(&pulse_map);
        let m_cache = Arc::clone(&moka_cache);
        let q_cache = Arc::clone(&quick_cache);
        
        handles.push(thread::spawn(move || {
            for i in 0..10_000 {
                let bot_ip = 1000 + (t * 10_000) + i;
                p_map.insert(bot_ip, 1);
                m_cache.insert(bot_ip, 1);
                q_cache.insert(bot_ip, 1);
            }
        }));
    }
    for h in handles { h.join().unwrap(); }
    moka_cache.run_pending_tasks();
    
    println!("   - Competitors handled the flood without panicking.");
}

fn run_stress_test() {
    println!("🧪 Scenario 6: Extreme Stress Test");
    let map = Arc::new(ShardedPulseMap::<String, Vec<u8>>::with_auto_resize(16));
    let start = Instant::now();
    let mut handles = vec![];

    for t_id in 0..8 {
        let m = Arc::clone(&map);
        handles.push(thread::spawn(move || {
            let mut local_hits = 0;
            for i in 0..100_000 {
                let mut seed = (t_id as u64 * 1_000_000) + i as u64;
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let key_idx = (seed >> 32) % 10_000;
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let op_type = (seed >> 32) % 10;
                
                let key = format!("t{}_{}", t_id, key_idx);
                match op_type {
                    0..=5 => m.insert(key, vec![42; 4]),
                    6..=8 => if m.get(&key).is_some() { local_hits += 1; },
                    _ => { m.remove(&key); }
                }
            }
            local_hits
        }));
    }
    
    let mut total_hits = 0;
    for h in handles { total_hits += h.join().unwrap(); }
    
    println!("   - Stress test complete. Hits: {}, Evictions: {}, Time: {:.2?}", 
             total_hits, map.eviction_count(), start.elapsed());
}
