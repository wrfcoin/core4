//! #3551: `GET /state/snapshot` peak allocation on a large generated state.
//!
//! The route used to build the whole `StateSnapshot` to report four fields:
//! every entry (state + encoded window blocks) held at once, a `SnapshotEntry`
//! (three heap strings) per entry, then the leaf hashes cloned twice for the
//! Merkle layers. It now streams entries into `SnapshotRootBuilder`, which
//! keeps only key + leaf hash, and encodes one window block at a time.
//!
//! This binary counts live heap bytes with its own global allocator, so it
//! measures both shapes on the same generated data. ONE test function, so no
//! parallel test thread shares the counter. Synthetic bytes stand in
//! for state entries and encoded blocks: the root computation only sees bytes.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use wrfcoin_shared_protocol::blockchain::snapshot_sync::{SnapshotRootBuilder, build_state_snapshot};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged to the system allocator.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `alloc` above with this `layout`.
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Peak live bytes above the live bytes at entry, while `f` runs.
fn peak_above_baseline<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let out = f();
    (out, PEAK.load(Ordering::Relaxed) - base)
}

const STATE_ENTRIES: usize = 50_000;
const BLOCKS: u64 = 160;
const BLOCK_BYTES: usize = 256 * 1024;

fn state_entries() -> Vec<(Vec<u8>, Vec<u8>)> {
    (0..STATE_ENTRIES)
        .map(|i| (format!("state/account/{i:08}").into_bytes(), vec![(i % 251) as u8; 96]))
        .collect()
}

/// The encoded block at `height` (synthetic, deterministic).
fn encoded_block(height: u64) -> (Vec<u8>, Vec<u8>) {
    (format!("chain/block/{height:020}").into_bytes(), vec![(height % 251) as u8; BLOCK_BYTES])
}

#[test]
fn snapshot_summary_peak_allocation_is_bounded_3551() {
    // Old shape: every entry materialised, then the full snapshot built.
    let ((old_root, old_count), old_peak) = peak_above_baseline(|| {
        let mut entries = state_entries();
        entries.extend((1..=BLOCKS).map(encoded_block));
        let snapshot = build_state_snapshot(BLOCKS, "root", &entries);
        (snapshot.snapshot_root_hex.clone(), snapshot.entries.len())
    });

    // New shape: the state entries are still exported at once (the route
    // does too); each is pushed and dropped, and blocks are encoded one at a
    // time.
    let ((new_root, new_count), new_peak) = peak_above_baseline(|| {
        let mut builder = SnapshotRootBuilder::new();
        for (key, value) in state_entries() {
            builder.push(key, &value);
        }
        for height in 1..=BLOCKS {
            let (key, value) = encoded_block(height);
            builder.push(key, &value);
        }
        builder.finish()
    });

    assert_eq!(new_root, old_root, "same root");
    assert_eq!(new_count, old_count, "same entry count");

    let block_window = BLOCKS as usize * BLOCK_BYTES;
    println!(
        "#3551 peak: old {} KiB, new {} KiB (window {} KiB)",
        old_peak / 1024,
        new_peak / 1024,
        block_window / 1024
    );
    // The window of encoded blocks is never held by the new shape.
    assert!(old_peak > block_window, "precondition: old shape holds the whole window ({old_peak})");
    assert!(
        new_peak < block_window / 4,
        "new peak {new_peak} must stay well below the {block_window}-byte window"
    );
    assert!(new_peak * 4 < old_peak, "new peak {new_peak} vs old {old_peak}");

    // RED control for the measurement itself: holding every encoded block
    // (what the old shape did) trips the bound the new shape meets.
    let (_, held_peak) = peak_above_baseline(|| {
        let mut builder = SnapshotRootBuilder::new();
        let blocks: Vec<_> = (1..=BLOCKS).map(encoded_block).collect();
        for (key, value) in blocks {
            builder.push(key, &value);
        }
        builder.finish()
    });
    assert!(held_peak >= block_window / 4, "the bound must detect a held window ({held_peak})");
}
