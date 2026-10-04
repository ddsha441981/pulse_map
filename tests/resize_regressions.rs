#![cfg(all(feature = "std", not(loom)))]

use pulse_map::{ConcurrentPulseMap, ShardedPulseMap};

#[test]
fn empty_string_key_survives_manual_resize() {
    let map = ConcurrentPulseMap::<String, u32>::new(4);
    let empty = String::new();

    map.insert(empty.clone(), 42);
    assert_eq!(map.peek(&empty), Some(42));

    map.resize(8);

    assert_eq!(map.len(), 1);
    assert_eq!(map.get(&empty), Some(42));
    assert!(map.remove(&empty));
    assert_eq!(map.len(), 0);
}

#[test]
fn empty_vec_key_with_slab_value_survives_manual_resize() {
    let map = ConcurrentPulseMap::<Vec<u8>, Vec<u8>>::new(4);
    let empty = Vec::new();
    let value = vec![0xA5; 32];

    map.insert(empty.clone(), value.clone());
    map.resize(8);

    assert_eq!(map.peek(&empty), Some(value));
    assert_eq!(map.len(), 1);
}

#[test]
fn empty_array_key_survives_resize() {
    let map = ConcurrentPulseMap::<[u8; 0], u32>::new(4);

    map.insert([], 7);
    map.resize(8);
    assert_eq!(map.peek(&[]), Some(7));

    assert!(map.remove(&[]));
}

#[test]
fn empty_key_finite_ttl_survives_resize() {
    let map = ConcurrentPulseMap::<String, u32>::new(4);
    let empty = String::new();
    map.insert_ttl(empty.clone(), 7, 1);
    map.insert("other".into(), 1);
    map.resize(8);
    assert_eq!(map.peek(&empty), Some(7));
    map.insert("other".into(), 2);
    assert_eq!(map.peek(&empty), None);
}

#[test]
fn empty_key_never_expire_survives_resize() {
    let map = ConcurrentPulseMap::<String, u32>::new(4);
    let empty = String::new();

    map.insert_ttl(empty.clone(), 9, u64::MAX);
    map.resize(8);
    for key in 0..32u32 {
        map.insert("other".into(), key);
    }

    assert_eq!(map.peek(&empty), Some(9));
}

#[test]
fn empty_key_survives_auto_resize() {
    let map = ConcurrentPulseMap::<String, u32>::with_auto_resize(1);
    let empty = String::new();
    map.insert(empty.clone(), 11);

    for key in 0..3u32 {
        map.insert(key.to_string(), key);
    }
    assert_eq!(map.capacity(), 4);
    assert_eq!(map.peek(&empty), Some(11));
    // Updating another resident entry triggers growth without a new collision.
    map.insert("0".into(), 99);
    assert!(map.capacity() > 4);
    assert_eq!(map.peek(&empty), Some(11));
}

#[test]
fn empty_key_survives_sharded_resize_all() {
    let map = ShardedPulseMap::<String, u32>::new(4);
    let empty = String::new();
    map.insert(empty.clone(), 13);

    map.resize_all(8);

    assert_eq!(map.peek(&empty), Some(13));
}
