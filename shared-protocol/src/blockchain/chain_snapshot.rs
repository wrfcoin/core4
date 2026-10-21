//! Snapshot-sync chain entries: the blocks a fast-synced node needs (#3288 3b).
//!
//! A state snapshot (see [`super::state_snapshot`]) is not enough to run a
//! node. The node must also hold:
//!
//! - **the tip hash.** PoP rejects block `H + 1` unless its `prev_block_hash`
//!   is the local tip, so a node that installed only state can never take
//!   another block;
//! - **a window of recent blocks.** Median-time-past reads the last
//!   [`MEDIAN_TIME_PAST_WINDOW`](crate::consensus::proof_of_publishing::MEDIAN_TIME_PAST_WINDOW)
//!   blocks and silently stops applying with fewer, and node services read
//!   further back (the node computes the exact requirement and passes it in as
//!   `required_window`);
//! - **the height-1 genesis block.** The boot path derives the chain's protocol
//!   version from the lowest persisted block and refuses to start unless that
//!   block is the height-1 genesis anchor.
//!
//! The blocks travel as ordinary snapshot entries, so the signed
//! `snapshot_root_hex` covers them. Each key is `chain/block/<height>`, with the
//! height zero-padded to 20 digits so key order is height order. Each value is
//! the canonical CBOR of the [`Block`].
//!
//! [`verify_chain_window`] is the trust anchor. The carried tip block must hash
//! to the SIGNED tip, and it must commit (`STATE_ROOT_METADATA_KEY`) to the
//! SIGNED state root. The snapshot's state is therefore the state the chain
//! itself committed to at block `H`, and anyone can check the tip hash against
//! peers' headers.
//!
//! The commitment exists from protocol V2 (#2219). Like every other commitment
//! check (`State::apply_block_finalization_checked_at`,
//! `verify_tip_state_root_commitment_at`), it is REQUIRED at V2. Below V2 no
//! block carries one, so it is checked only when present.

use crate::blockchain::block::{Block, STATE_ROOT_METADATA_KEY};
use crate::blockchain::state_snapshot::{decode_snapshot_value, encode_snapshot_value};
use crate::crypto::chain_binding::SigningProtocolVersion;
use crate::crypto::hash::Hash;

/// Key prefix of every chain entry in a snapshot.
pub const CHAIN_ENTRY_PREFIX: &str = "chain/";
/// Key prefix of a carried block.
pub const CHAIN_BLOCK_PREFIX: &str = "chain/block/";
/// Height of the genesis block every snapshot carries.
pub const GENESIS_BLOCK_HEIGHT: u64 = 1;

/// The snapshot key of the block at `height`.
pub fn block_entry_key(height: u64) -> Vec<u8> {
    format!("{CHAIN_BLOCK_PREFIX}{height:020}").into_bytes()
}

/// Whether `key` is a chain entry, as opposed to a state entry.
pub fn is_chain_entry(key: &[u8]) -> bool {
    key.starts_with(CHAIN_ENTRY_PREFIX.as_bytes())
}

/// The lowest height of the contiguous block window a snapshot at `height`
/// must carry, given the node's `required_window` (in blocks, counting the tip).
/// The genesis block is carried separately, so this is never below 2.
pub fn window_floor(height: u64, required_window: u64) -> u64 {
    height
        .saturating_add(1)
        .saturating_sub(required_window.max(1))
        .max(GENESIS_BLOCK_HEIGHT + 1)
}

/// Encode `blocks` as snapshot entries.
pub fn export_block_entries(blocks: &[Block]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
    blocks
        .iter()
        .map(|block| {
            let value = encode_snapshot_value(block)
                .map_err(|e| format!("block {} is not encodable: {e}", block.height))?;
            Ok((block_entry_key(block.height), value))
        })
        .collect()
}

/// Split snapshot entries into `(state entries, chain entries)`, keeping order.
pub fn split_snapshot_entries(
    entries: &[(Vec<u8>, Vec<u8>)],
) -> (Vec<(Vec<u8>, Vec<u8>)>, Vec<(Vec<u8>, Vec<u8>)>) {
    entries.iter().cloned().partition(|(key, _)| !is_chain_entry(key))
}

/// Why a snapshot's chain entries were refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainWindowError {
    /// A chain key or value could not be decoded.
    Malformed(String),
    /// The snapshot is at height 0, which has no tip block to carry.
    NoTip,
    /// The height-1 genesis block is missing or is not a genesis block.
    Genesis(String),
    /// The blocks after genesis are not one contiguous run ending at `height`.
    NotContiguous { expected: u64, found: u64 },
    /// The window does not reach down far enough for this node's consumers.
    WindowTooShort { required_floor: u64, carried_floor: u64 },
    /// A block carries no hash, or its hash is not its recomputed hash.
    BadBlockHash { height: u64 },
    /// A block's `prev_block_hash` is not the hash of the block below it.
    BrokenLink { height: u64 },
    /// The carried tip block does not hash to the signed tip.
    TipMismatch { signed: String, carried: String },
    /// The tip block carries no state-root commitment.
    MissingStateRootCommitment { height: u64 },
    /// The tip block commits to a different state root than the signed one.
    StateRootCommitmentMismatch { committed: String, signed: String },
}

impl std::fmt::Display for ChainWindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "malformed chain entry: {why}"),
            Self::NoTip => write!(f, "a snapshot at height 0 has no tip block"),
            Self::Genesis(why) => write!(f, "genesis block: {why}"),
            Self::NotContiguous { expected, found } => {
                write!(f, "block window is not contiguous: expected height {expected}, found {found}")
            },
            Self::WindowTooShort { required_floor, carried_floor } => write!(
                f,
                "block window starts at {carried_floor}, this node needs it from {required_floor}"
            ),
            Self::BadBlockHash { height } => {
                write!(f, "block {height} does not carry its own hash")
            },
            Self::BrokenLink { height } => {
                write!(f, "block {height} does not link to the block below it")
            },
            Self::TipMismatch { signed, carried } => {
                write!(f, "carried tip block hashes to {carried}, the signed tip is {signed}")
            },
            Self::MissingStateRootCommitment { height } => {
                write!(f, "tip block {height} carries no state-root commitment")
            },
            Self::StateRootCommitmentMismatch { committed, signed } => write!(
                f,
                "tip block commits to state root {committed}, the signed state root is {signed}"
            ),
        }
    }
}

impl std::error::Error for ChainWindowError {}

fn block_hash(block: &Block) -> Result<Hash, ChainWindowError> {
    let stored = block.hash.ok_or(ChainWindowError::BadBlockHash {
        height: block.height,
    })?;
    if stored.to_hex() != block.calculate_hash().to_hex() {
        return Err(ChainWindowError::BadBlockHash {
            height: block.height,
        });
    }
    Ok(stored)
}

/// Verify a snapshot's chain entries and return the carried blocks, ascending.
///
/// Fail-closed. The entries must hold exactly the height-1 genesis block plus
/// one contiguous run `[floor ..= height]` with
/// `floor <= window_floor(height, required_window)`. Every block must carry its
/// own recomputed hash (which binds the chain identity) and link to the block
/// below it. The tip block must hash to `signed_tip_hex` and commit to
/// `signed_state_root_hex` (required at `version >= V2`, see the module docs).
pub fn verify_chain_window(
    chain_entries: &[(Vec<u8>, Vec<u8>)],
    height: u64,
    signed_tip_hex: &str,
    signed_state_root_hex: &str,
    required_window: u64,
    version: SigningProtocolVersion,
) -> Result<Vec<Block>, ChainWindowError> {
    if height == 0 {
        return Err(ChainWindowError::NoTip);
    }
    let mut blocks = Vec::with_capacity(chain_entries.len());
    for (key, value) in chain_entries {
        let key_text = String::from_utf8_lossy(key);
        let digits = key_text.strip_prefix(CHAIN_BLOCK_PREFIX).ok_or_else(|| {
            ChainWindowError::Malformed(format!("unsupported chain entry `{key_text}`"))
        })?;
        let block: Block = decode_snapshot_value(value)
            .map_err(|e| ChainWindowError::Malformed(format!("`{key_text}`: {e}")))?;
        if block_entry_key(block.height) != *key {
            return Err(ChainWindowError::Malformed(format!(
                "`{key_text}` holds block {} (key digits `{digits}`)",
                block.height
            )));
        }
        blocks.push(block);
    }
    blocks.sort_by_key(|block| block.height);

    // Genesis.
    let genesis = blocks
        .first()
        .filter(|block| block.height == GENESIS_BLOCK_HEIGHT)
        .ok_or_else(|| ChainWindowError::Genesis("the height-1 block is not carried".into()))?;
    if genesis.prev_block_hash.to_hex() != Hash::default().to_hex() {
        return Err(ChainWindowError::Genesis(
            "the height-1 block does not start the chain".into(),
        ));
    }

    // The window after genesis: contiguous, ending at `height`.
    let window = &blocks[1..];
    let carried_floor = window.first().map_or(GENESIS_BLOCK_HEIGHT, |b| b.height);
    for (offset, block) in window.iter().enumerate() {
        let expected = carried_floor + offset as u64;
        if block.height != expected {
            return Err(ChainWindowError::NotContiguous {
                expected,
                found: block.height,
            });
        }
    }
    let top = blocks.last().map_or(0, |b| b.height);
    if top != height {
        return Err(ChainWindowError::NotContiguous {
            expected: height,
            found: top,
        });
    }
    if height > GENESIS_BLOCK_HEIGHT {
        let required_floor = window_floor(height, required_window);
        if carried_floor > required_floor {
            return Err(ChainWindowError::WindowTooShort {
                required_floor,
                carried_floor,
            });
        }
    }

    // Hashes and links. Block 2 links to genesis when the window reaches it;
    // otherwise the one gap in the chain is directly above genesis.
    let mut below: Option<(u64, Hash)> = None;
    for block in &blocks {
        let hash = block_hash(block)?;
        if let Some((below_height, below_hash)) = below {
            if below_height + 1 == block.height
                && block.prev_block_hash.to_hex() != below_hash.to_hex()
            {
                return Err(ChainWindowError::BrokenLink {
                    height: block.height,
                });
            }
        }
        below = Some((block.height, hash));
    }

    // The trust anchor: the signed tip, and the tip's own state commitment.
    let tip = blocks.last().expect("genesis is carried, so blocks is non-empty");
    let carried = block_hash(tip)?.to_hex();
    let signed_tip = signed_tip_hex.trim().to_ascii_lowercase();
    if carried != signed_tip {
        return Err(ChainWindowError::TipMismatch {
            signed: signed_tip,
            carried,
        });
    }
    let Some(committed) = tip
        .metadata
        .get(STATE_ROOT_METADATA_KEY)
        .map(|root| root.trim().to_ascii_lowercase())
    else {
        if version >= SigningProtocolVersion::V2 {
            return Err(ChainWindowError::MissingStateRootCommitment { height });
        }
        return Ok(blocks);
    };
    let signed_root = signed_state_root_hex.trim().to_ascii_lowercase();
    if committed != signed_root {
        return Err(ChainWindowError::StateRootCommitmentMismatch {
            committed,
            signed: signed_root,
        });
    }
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    const V2: SigningProtocolVersion = SigningProtocolVersion::V2;
    const ROOT: &str = "5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a";

    /// A linked chain `1 ..= tip`, each block committing to `ROOT`.
    fn chain(tip: u64) -> Vec<Block> {
        let mut blocks: Vec<Block> = Vec::new();
        let mut prev = Hash::default();
        for height in 1..=tip {
            let mut block = Block::new(1, prev, Vec::new(), 1, height);
            block.timestamp = chrono::DateTime::from_timestamp(1_700_000_000 + height as i64, 7)
                .expect("valid timestamp");
            block.metadata.insert(STATE_ROOT_METADATA_KEY.to_string(), ROOT.to_string());
            block.metadata.insert("note".to_string(), format!("b{height}"));
            let hash = block.calculate_hash();
            block.set_hash(hash);
            prev = hash;
            blocks.push(block);
        }
        blocks
    }

    /// Genesis plus the window `[floor ..= tip]`.
    fn carried(all: &[Block], floor: u64) -> Vec<Block> {
        all.iter().filter(|b| b.height == 1 || b.height >= floor).cloned().collect()
    }

    fn entries(blocks: &[Block]) -> Vec<(Vec<u8>, Vec<u8>)> {
        export_block_entries(blocks).expect("export")
    }

    fn tip_hex(all: &[Block]) -> String {
        all.last().unwrap().hash.unwrap().to_hex()
    }

    #[test]
    fn a_genesis_plus_window_snapshot_verifies_and_round_trips_3288() {
        let all = chain(40);
        let sent = carried(&all, 30);
        let got = verify_chain_window(&entries(&sent), 40, &tip_hex(&all), ROOT, 11, V2).expect("ok");
        assert_eq!(got.len(), sent.len());
        for (a, b) in got.iter().zip(&sent) {
            assert_eq!(a.hash.unwrap().to_hex(), b.hash.unwrap().to_hex());
            assert_eq!(a.calculate_hash().to_hex(), b.calculate_hash().to_hex());
        }
        // Key order is height order (the Merkle build sorts by key).
        let keys: Vec<_> = entries(&sent).into_iter().map(|(k, _)| k).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn the_window_floor_follows_the_required_window_3288() {
        assert_eq!(window_floor(100, 11), 90);
        assert_eq!(window_floor(100, 100), 2, "never below the block after genesis");
        assert_eq!(window_floor(5, 11), 2);
        assert_eq!(window_floor(100, 0), 100, "the tip is always carried");
    }

    #[test]
    fn a_window_shorter_than_required_is_refused_3288() {
        let all = chain(40);
        let sent = carried(&all, 31);
        assert_eq!(
            verify_chain_window(&entries(&sent), 40, &tip_hex(&all), ROOT, 11, V2).err(),
            Some(ChainWindowError::WindowTooShort { required_floor: 30, carried_floor: 31 })
        );
    }

    #[test]
    fn a_missing_genesis_is_refused_3288() {
        let all = chain(20);
        let sent: Vec<_> = all.iter().filter(|b| b.height >= 5).cloned().collect();
        assert!(matches!(
            verify_chain_window(&entries(&sent), 20, &tip_hex(&all), ROOT, 11, V2),
            Err(ChainWindowError::Genesis(_))
        ));
    }

    #[test]
    fn a_gap_inside_the_window_is_refused_3288() {
        let all = chain(20);
        let sent: Vec<_> = carried(&all, 5).into_iter().filter(|b| b.height != 12).collect();
        assert_eq!(
            verify_chain_window(&entries(&sent), 20, &tip_hex(&all), ROOT, 11, V2).err(),
            Some(ChainWindowError::NotContiguous { expected: 12, found: 13 })
        );
    }

    #[test]
    fn a_window_that_stops_below_the_signed_height_is_refused_3288() {
        let all = chain(20);
        let sent = carried(&all[..19], 5);
        assert_eq!(
            verify_chain_window(&entries(&sent), 20, &tip_hex(&all), ROOT, 11, V2).err(),
            Some(ChainWindowError::NotContiguous { expected: 20, found: 19 })
        );
    }

    #[test]
    fn a_tampered_block_is_refused_3288() {
        let all = chain(20);
        let mut sent = carried(&all, 5);
        sent[5].metadata.insert("note".into(), "tampered".into());
        assert_eq!(
            verify_chain_window(&entries(&sent), 20, &tip_hex(&all), ROOT, 11, V2).err(),
            Some(ChainWindowError::BadBlockHash { height: sent[5].height })
        );
    }

    #[test]
    fn a_relinked_block_is_refused_3288() {
        // Each block is self-consistent (its hash is recomputed after the
        // edit), but block 10 no longer links to block 9.
        let all = chain(20);
        let mut sent = carried(&all, 5);
        let at = sent.iter().position(|b| b.height == 10).unwrap();
        sent[at].prev_block_hash = Hash::default();
        let rehash = sent[at].calculate_hash();
        sent[at].set_hash(rehash);
        assert_eq!(
            verify_chain_window(&entries(&sent), 20, &tip_hex(&all), ROOT, 11, V2).err(),
            Some(ChainWindowError::BrokenLink { height: 10 })
        );
    }

    #[test]
    fn a_tip_other_than_the_signed_one_is_refused_3288() {
        let all = chain(20);
        let sent = carried(&all, 5);
        let wrong = "77".repeat(32);
        assert_eq!(
            verify_chain_window(&entries(&sent), 20, &wrong, ROOT, 11, V2).err(),
            Some(ChainWindowError::TipMismatch { signed: wrong, carried: tip_hex(&all) })
        );
    }

    #[test]
    fn a_state_root_the_tip_does_not_commit_to_is_refused_3288() {
        let all = chain(20);
        let sent = carried(&all, 5);
        let other = "33".repeat(32);
        assert_eq!(
            verify_chain_window(&entries(&sent), 20, &tip_hex(&all), &other, 11, V2).err(),
            Some(ChainWindowError::StateRootCommitmentMismatch {
                committed: ROOT.to_string(),
                signed: other,
            })
        );
    }

    #[test]
    fn a_tip_without_a_commitment_is_refused_3288() {
        let mut all = chain(20);
        let tip = all.last_mut().unwrap();
        tip.metadata.remove(STATE_ROOT_METADATA_KEY);
        let rehash = tip.calculate_hash();
        tip.set_hash(rehash);
        let sent = carried(&all, 5);
        assert_eq!(
            verify_chain_window(&entries(&sent), 20, &tip_hex(&all), ROOT, 11, V2).err(),
            Some(ChainWindowError::MissingStateRootCommitment { height: 20 })
        );
    }

    #[test]
    fn below_v2_a_missing_commitment_is_tolerated_but_a_wrong_one_is_not_3288() {
        let v1 = SigningProtocolVersion::V1;
        let mut all = chain(20);
        let tip = all.last_mut().unwrap();
        tip.metadata.remove(STATE_ROOT_METADATA_KEY);
        let rehash = tip.calculate_hash();
        tip.set_hash(rehash);
        let sent = carried(&all, 5);
        assert!(verify_chain_window(&entries(&sent), 20, &tip_hex(&all), ROOT, 11, v1).is_ok());

        let all = chain(20);
        let sent = carried(&all, 5);
        let other = "33".repeat(32);
        assert!(matches!(
            verify_chain_window(&entries(&sent), 20, &tip_hex(&all), &other, 11, v1),
            Err(ChainWindowError::StateRootCommitmentMismatch { .. })
        ));
    }

    #[test]
    fn a_mislabelled_key_is_refused_3288() {
        let all = chain(20);
        let mut sent = entries(&carried(&all, 5));
        sent[3].0 = block_entry_key(999);
        assert!(matches!(
            verify_chain_window(&sent, 20, &tip_hex(&all), ROOT, 11, V2),
            Err(ChainWindowError::Malformed(_))
        ));
    }

    #[test]
    fn split_keeps_state_and_chain_apart_3288() {
        let all = chain(3);
        let mut mixed = entries(&all);
        mixed.push((b"state/accounts".to_vec(), vec![0xa0]));
        let (state, chain) = split_snapshot_entries(&mixed);
        assert_eq!(state.len(), 1);
        assert_eq!(chain.len(), 3);
        assert!(chain.iter().all(|(k, _)| is_chain_entry(k)));
    }
}
