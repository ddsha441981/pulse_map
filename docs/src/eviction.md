# Eviction Strategy

The minimum-priority Full slot in the target bucket is evicted when no free slot
is chosen. Raw insertion also reclaims expired slots; concurrent insertion currently
chooses free slots or the policy victim, without an expired-first search.

```text
priority = (frequency << 3) | recency
frequency: 0..15, saturating
recency:   0..7
new entry: frequency=0, recency=1
hit:       frequency += 1 (capped), recency=7
           other Full slots' recency -= 1 (floored at zero)
```

Frequency does **not** age. Recency decays on accesses to neighbours, not elapsed
time. This protects established hot entries from scans, but can prevent adaptation
to a new working set. Saturation caps counters; it does not guarantee old entries
will become cold. Current traces measure both scan resistance and phase changes.

Raw/typed get updates the atomic MetaWord immediately. Concurrent get records a
lossy access event, applied during insert in a bounded batch. Missed events can
change policy accuracy; they do not change stored key/value bytes. Peek does not
promote priority. Updating an existing key promotes it and refreshes its TTL.

Eviction is a local approximation, not exact global LRU or global LFU. Increasing
capacity reduces collisions but does not guarantee every insertion survives.
`u64::MAX` TTL only prevents expiry; it does not pin entries against eviction.
