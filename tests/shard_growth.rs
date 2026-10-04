#![cfg(all(feature = "std", not(loom)))]

use pulse_map::ShardedPulseMap;

#[test]
#[cfg_attr(miri, ignore)] // large-table boundary probe; small routing tests cover Miri
fn growing_past_shard_bits_increases_residency() {
    let mut counts = Vec::new();
    for buckets in [8192, 16384, 32768] {
        let map = ShardedPulseMap::<u32, u32>::new(buckets);
        for key in 0..1_000_000 {
            map.insert(key, key);
        }
        println!(
            "buckets/shard={buckets} capacity={} residents={} evictions={}",
            map.capacity(),
            map.len(),
            map.eviction_count()
        );
        counts.push(map.len());
    }
    assert!(counts[1] > counts[0]);
    assert!(
        counts[2] > counts[1],
        "doubling allocated capacity must improve this trace's retention"
    );
}

#[test]
fn growth_preserves_resident_inline_and_slab_values() {
    let inline = ShardedPulseMap::<u32, u32>::new(4);
    let slab = ShardedPulseMap::<u64, Vec<u8>>::new(4);
    for k in 0..300u32 {
        inline.insert(k, k + 1);
        slab.insert(k as u64, vec![k as u8; 16]);
    }
    let inline_before: Vec<_> = (0..300)
        .filter_map(|k| inline.peek(&k).map(|v| (k, v)))
        .collect();
    let slab_before: Vec<_> = (0..300)
        .filter_map(|k| slab.peek(&k).map(|v| (k, v)))
        .collect();
    inline.resize_all(16);
    slab.resize_all(16);
    assert_eq!(inline.len(), inline_before.len());
    assert_eq!(slab.len(), slab_before.len());
    for (k, v) in inline_before {
        assert_eq!(inline.get(&k), Some(v));
        assert!(inline.remove(&k));
    }
    for (k, v) in slab_before {
        assert_eq!(slab.get(&k), Some(v));
        assert!(slab.remove(&k));
    }
    assert!(inline.is_empty() && slab.is_empty());
}

#[test]
fn concurrent_updates_during_growth_return_only_matching_values() {
    use std::sync::{Arc, Barrier};
    let map = Arc::new(ShardedPulseMap::<u64, u64>::new(4));
    let barrier = Arc::new(Barrier::new(3));
    std::thread::scope(|scope| {
        for thread in 0..2u64 {
            let (map, barrier) = (&map, &barrier);
            scope.spawn(move || {
                barrier.wait();
                for n in 0..200 {
                    let key = thread * 1000 + n % 16;
                    map.insert(key, key ^ 0xABCD);
                    if let Some(value) = map.get(&key) {
                        assert_eq!(value, key ^ 0xABCD);
                    }
                }
            });
        }
        barrier.wait();
        map.resize_all(8);
        map.resize_all(16);
    });
    assert!(map.len() <= map.capacity());
    for thread in 0..2u64 {
        for n in 0..16 {
            let key = thread * 1000 + n;
            assert_eq!(map.peek(&key), Some(key ^ 0xABCD));
        }
    }
}
