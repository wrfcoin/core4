//! Crash-safe state persistence using redb.
//!
//! Provides WAL (write-ahead log) for state transitions, periodic checkpoints
//! of the full State, and block archival. All writes for a single block are
//! committed atomically in one redb transaction.
#![allow(clippy::result_large_err)]

use crate::blockchain::block::Block;
use crate::blockchain::chain::{Blockchain, configured_max_in_memory_blocks};
use crate::blockchain::state::{State, StateTransition};
use crate::crypto::hash::Hash;
use crate::storage_encryption::StorageEncryption;
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

const BLOCKS_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("blocks");
const CHECKPOINTS_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("checkpoints");
const WAL_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("wal");
pub const MAX_NODE_ARCHIVE_RECORD_BYTES: usize = 40 * 1024;
pub const MAX_NODE_ARCHIVE_RECORDS: u64 = 4096;
const NODE_RECEIPT_ARCHIVE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("node_receipt_archive_v1");
const META_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

const META_HEIGHT: &str = "height";
const META_TIP: &str = "tip";
const META_WAL_SEQ: &str = "wal_sequence";
const META_LAST_CHECKPOINT: &str = "last_checkpoint_height";
/// Monotonic finalized height persisted alongside `META_HEIGHT`. Required for
/// chain-safety: without this, `Blockchain::finalized_height` resets to 0 on
/// every restart, allowing already-finalized blocks to be reorged (issue #1275).
const META_FINALIZED_HEIGHT: &str = "finalized_height";
/// Key for the permissionless-replay dedup map persisted in the meta table (#1845).
const META_PERMISSIONLESS_REPLAY: &str = "permissionless_replay_seen";
/// #3093: the consensus protocol version of the binary that last ran this
/// volume, recorded at every successful boot. Read by the rollback check
/// before anything is applied. Deliberately a META key rather than an
/// envelope field: additive, no checkpoint-format bump, no #3091 conflict.
const META_PROTOCOL_VERSION: &str = "protocol_version";
const WAL_V2_MAGIC: &[u8] = b"wrfwal2\0";

/// Default number of blocks between full state checkpoints.
/// Override with the `WRFCOIN_CHECKPOINT_INTERVAL` environment variable.
pub mod legacy;
mod node_ticket_protection;
pub mod migrations;

pub const DEFAULT_CHECKPOINT_INTERVAL: u64 = 100;

/// Reserved `CHECKPOINTS_TABLE` key for the **genesis anchor** (#2501).
///
/// Replay is `anchor + log`. Every other checkpoint is written by
/// `persist_block_sync`/`force_checkpoint_sync`, both of which require
/// `height > 0` (`Storage::needs_checkpoint`, and `force_checkpoint`'s early
/// return), so height 0 is unreachable for a periodic or shutdown checkpoint and
/// is unambiguous as the genesis slot. It holds the genesis `State` the node
/// actually started from — which is NOT `State::new()` on a chain with
/// genesis-only state, such as the producer set seeded by
/// `State::seed_genesis_producer_set`.
///
/// Nothing carries that state in the WAL: it is written into genesis `State`
/// directly and no `StateTransition` describes it. So without this anchor,
/// checkpoint-less replay reconstructs a faithful log on top of the WRONG base,
/// and `producer_set_history` comes back empty — read everywhere as "this chain
/// never configured producer auth" (#2501, #2290).
///
/// # The condition that reaches this, stated precisely
///
/// Two things write a post-genesis checkpoint, and BOTH must have failed to run:
///
/// 1. the periodic one, which `Storage::needs_checkpoint` fires only at
///    `height % DEFAULT_CHECKPOINT_INTERVAL == 0` — so never below height 100;
/// 2. the graceful-shutdown one, `Storage::force_checkpoint`, which the node
///    calls at **any** height > 0 on clean exit.
///
/// So the exposed case is an **abrupt** stop below height 100 — crash, OOM,
/// SIGKILL, container kill, power loss — where neither ran. A cleanly shut down
/// node has a checkpoint at whatever height it stopped at and never reaches this
/// path.
///
/// (An earlier revision of this comment claimed heights 1..=99 have no
/// checkpoint at all. That was wrong: it generalised from the *periodic*
/// predicate to all checkpointing and ignored the shutdown writer. Corrected
/// after review, because the next reader reasons from this paragraph.)
pub const GENESIS_ANCHOR_HEIGHT: u64 = 0;

/// Magic prefix of a versioned checkpoint/snapshot envelope (#2727).
///
/// Checkpoints written before this envelope existed are raw
/// `bincode(State)` bytes with no version information, so an incompatible
/// layout change (e.g. #2508 inserting fields mid-struct) surfaced as a
/// misleading serde error on a live node — node container restart-looped on
/// `EXIT_CORRUPT_SNAPSHOT ... invalid value: integer 47474, expected variant
/// index 0 <= i < 3` when the data volume was perfectly intact. The magic
/// lets the reader distinguish "versioned, check the version" from "legacy,
/// try the current layout and name the format gap if it does not fit".
///
/// A legacy checkpoint cannot collide with the magic: raw `bincode(State)`
/// begins with the `accounts` map length as `u64` little-endian, and this
/// magic read as a `u64` is ~6.05e18 — no real account count reaches that.
pub const CHECKPOINT_ENVELOPE_MAGIC: [u8; 8] = *b"WRFCKPT\0";

/// Current checkpoint/snapshot format version (#2727).
///
/// **Bump this whenever the serialized layout of [`State`] (or anything it
/// transitively contains) changes** — bincode is positional and ignores
/// `#[serde(default)]`, so ANY field insertion, removal, or reorder breaks
/// reads of previously written checkpoints. The golden-fixture test in
/// `checkpoint_format_golden_2727` is the tripwire that makes such a change
/// loud at test time; bumping this version is the acknowledgement that makes
/// the incompatibility explicit at *startup* time instead of surfacing as a
/// decode error. Version 1 is the implicit pre-envelope era and is never
/// written. Version 3 includes the recorded genesis faucet address (#3068);
/// version-2 checkpoints are incompatible and refuse before bincode decoding.
/// Version 4 adds Phase B producer settlement state (#3069); version 5
/// appends guardian-account-recovery state (#2172); version 6 session keys
/// (#2170); version 7 authorization declarations (#2168); version 8 paymaster
/// accounts (#2171); version 9 applied feature-schedule extensions (#3277);
/// version 10 per-station accepting-block timestamps for the halt-aware
/// weather skew allowance (#3270); version11 appends the owner ledger
/// (#3345 / #3347); version12 appends the optional Node registry/layout marker.
/// Version13 appends the optional bonded receipt registry.
/// Version14 freezes the S3 receipt layout and adds unminted Node settlement.
/// Version15 appends Node sanctions. Version16 appends the send-allowlist map
/// (written only when non-empty; an empty map keeps the format-15 writer).
/// Version-3 through version-11 checkpoints
/// migrate through registered, frozen layouts before decoding.
pub const CHECKPOINT_FORMAT_VERSION: u32 = 16;

/// Maximum number of decoded blocks [`Storage::scan_persisted_blocks`] holds in
/// memory at once.
///
/// The point of a whole-archive scan is that it does NOT depend on the retention
/// window, so it cannot be allowed to load the whole archive either: at 200k
/// blocks that is gigabytes. 256 blocks is small enough to be irrelevant to peak
/// RSS and large enough that per-chunk overhead disappears against the redb read
/// and bincode decode.
pub const PERSISTED_BLOCK_SCAN_CHUNK: usize = 256;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("redb error: {0}")]
    Redb(#[from] redb::Error),
    #[error("redb database error: {0}")]
    RedbDatabase(#[from] redb::DatabaseError),
    #[error("redb transaction error: {0}")]
    RedbTransaction(#[from] redb::TransactionError),
    #[error("redb table error: {0}")]
    RedbTable(#[from] redb::TableError),
    #[error("redb storage error: {0}")]
    RedbStorage(#[from] redb::StorageError),
    #[error("redb commit error: {0}")]
    RedbCommit(#[from] redb::CommitError),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error(
        "legacy WAL entry at sequence {sequence} uses an incompatible bincode encoding for a JSON-bearing transition: {error}"
    )]
    LegacyWalEntryIncompatible { sequence: u64, error: String },
    #[error("{0}")]
    Corruption(#[from] StorageCorruptionError),
    #[error(
        "WAL entry checksum mismatch at sequence {sequence}: expected {expected:#010x}, got {actual:#010x}"
    )]
    ChecksumMismatch {
        sequence: u64,
        expected: u32,
        actual: u32,
    },
    #[error("state root mismatch after WAL replay: expected {expected}, got {actual}")]
    StateRootMismatch { expected: String, actual: String },
    #[error("spawn_blocking failed: {0}")]
    SpawnBlocking(String),
    #[error("storage encryption error: {0}")]
    Encryption(String),
    /// #3288 3b: a snapshot install was refused because this database already
    /// holds chain history (a persisted height, a block or a WAL entry).
    #[error("{}", SNAPSHOT_INGEST_REQUIRES_FRESH_NODE)]
    ChainHistoryPresent,
}

/// The refusal a snapshot ingest gives on a node that is not fresh (#3288 3b).
///
/// Ingest installs the snapshot's chain as the node's WHOLE history, so it is
/// only sound on a node with none: merging it with existing blocks, WAL or
/// checkpoints would leave data the snapshot does not describe.
pub const SNAPSHOT_INGEST_REQUIRES_FRESH_NODE: &str =
    "snapshot ingest is for fresh nodes; this node has chain history";

#[derive(Debug, Error, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StorageCorruptionError {
    #[error(
        "NC_STORAGE_META_LENGTH_INVALID: meta key '{key}' length mismatch (expected {expected} bytes, got {actual})"
    )]
    MetaLengthInvalid {
        key: String,
        expected: usize,
        actual: usize,
    },
    #[error(
        "NC_STORAGE_WAL_PREFIX_INVALID: WAL entry prefix too short at sequence {sequence} (expected at least 5 bytes, got {actual_len})"
    )]
    WalPrefixInvalid { sequence: u64, actual_len: usize },
    #[error(
        "EXIT_MISSING_SNAPSHOT: checkpoint/snapshot anchor at height {height} is missing; refusing unsafe WAL replay"
    )]
    MissingSnapshot { height: u64 },
    #[error(
        "EXIT_CORRUPT_SNAPSHOT: checkpoint/snapshot at height {height} is unreadable: {error}"
    )]
    SnapshotUnreadable { height: u64, error: String },
    #[error(
        "EXIT_INCOMPATIBLE_CHECKPOINT_FORMAT: the pre-envelope (unversioned) \
         checkpoint/snapshot at height {height} cannot be decoded with this binary's \
         persistence layout (decode detail: {error}). If this volume was written by an \
         OLDER binary, this is a persistence-format gap, not damage — the layout changed \
         between builds (#2727; e.g. #2508 inserted State/CommitRecord fields mid-struct) \
         and the binary that wrote the volume can still read it. Refusing to start rather \
         than misread persisted state. Operator options, decided BEFORE restarting: \
         (1) roll the image back to the binary that wrote this volume — if that reads the \
         volume fine, this was the format gap, not corruption; (2) reset the data volume \
         and rejoin via snapshot (infra/testnet: test_reset_node_from_genesis.sh); \
         (3) point the node at a fresh data directory. There is no in-place migration."
    )]
    LegacyCheckpointIncompatible { height: u64, error: String },
    #[error(
        "EXIT_CHECKPOINT_FORMAT_VERSION_MISMATCH: the checkpoint/snapshot at height {height} \
         carries format version {found}, but this binary supports version {supported} \
         (#2727). The data volume is NOT corrupt — it was written by a binary with a \
         different persistence layout. Refusing to start rather than misread persisted \
         state. Run the binary whose version matches, or reset the volume and rejoin via \
         snapshot (infra/testnet: test_reset_node_from_genesis.sh)."
    )]
    CheckpointFormatVersionMismatch {
        height: u64,
        found: u32,
        supported: u32,
    },
    #[error(
        "EXIT_CHECKPOINT_MIGRATION_UNAVAILABLE: the checkpoint/snapshot at height {height} \
         carries format version {found}; this binary supports version {supported} and has \
         no registered in-place migration path from {found} (#3091). The data volume is NOT \
         corrupt — it was written by an older binary whose layout this release cannot \
         migrate. Refusing to start rather than misread persisted state. Run a binary \
         whose supported version is {found} (or one that registers a {found} -> {supported} \
         migration), or reset the volume and rejoin via snapshot (infra/testnet: \
         test_reset_node_from_genesis.sh)."
    )]
    CheckpointMigrationUnavailable {
        height: u64,
        found: u32,
        supported: u32,
    },
    #[error(
        "EXIT_STATE_ROOT_COMMITMENT_MISMATCH: tip block at height {height} commits to \
         post-state root '{committed}' but checkpoint+WAL replay reconstructed '{replayed}' \
         (#2219); refusing to start from divergent state"
    )]
    StateRootCommitmentMismatch {
        height: u64,
        committed: String,
        replayed: String,
    },
    #[error(
        "EXIT_MISSING_TIP_BLOCK: persisted height is {height} but no block at that height was \
         loaded from BLOCKS_TABLE — the blocks table is missing, truncated, or inconsistent \
         with META_HEIGHT, so the #2219 tip commitment cannot be verified; refusing to start \
         rather than skip the check"
    )]
    MissingTipBlock { height: u64 },
    #[error(
        "EXIT_CORRUPT_GENESIS_ANCHOR: the height-0 genesis anchor is unreadable: {error} \
         (#2501); refusing to fall back to a default genesis, because replaying a faithful \
         WAL onto the wrong base is how genesis-only state — the producer set — goes missing \
         without an error"
    )]
    GenesisAnchorUnreadable { error: String },
    #[error(
        "EXIT_GENESIS_ANCHOR_MISMATCH: this data directory already holds a genesis anchor with \
         state root '{existing}', but startup resolved a genesis with state root '{incoming}' \
         (#2501). These are different chains. Refusing to overwrite the anchor — point the node \
         at an empty data directory to start the new chain, or restore the matching genesis \
         configuration to rejoin the existing one"
    )]
    GenesisAnchorMismatch { existing: String, incoming: String },
}

impl StorageCorruptionError {
    pub const fn error_code(&self) -> &'static str {
        match self {
            Self::MetaLengthInvalid { .. } => "NC_STORAGE_META_LENGTH_INVALID",
            Self::WalPrefixInvalid { .. } => "NC_STORAGE_WAL_PREFIX_INVALID",
            Self::MissingSnapshot { .. } => "EXIT_MISSING_SNAPSHOT",
            Self::SnapshotUnreadable { .. } => "EXIT_CORRUPT_SNAPSHOT",
            Self::LegacyCheckpointIncompatible { .. } => "EXIT_INCOMPATIBLE_CHECKPOINT_FORMAT",
            Self::CheckpointFormatVersionMismatch { .. } => {
                "EXIT_CHECKPOINT_FORMAT_VERSION_MISMATCH"
            },
            Self::CheckpointMigrationUnavailable { .. } => {
                "EXIT_CHECKPOINT_MIGRATION_UNAVAILABLE"
            },
            Self::GenesisAnchorUnreadable { .. } => "EXIT_CORRUPT_GENESIS_ANCHOR",
            Self::GenesisAnchorMismatch { .. } => "EXIT_GENESIS_ANCHOR_MISMATCH",
            Self::StateRootCommitmentMismatch { .. } => "EXIT_STATE_ROOT_COMMITMENT_MISMATCH",
            Self::MissingTipBlock { .. } => "EXIT_MISSING_TIP_BLOCK",
        }
    }
}

/// Why [`gate_then_load_state`] did not produce a chain.
#[derive(Debug)]
pub enum GateThenLoadError<E> {
    /// The protocol gate refused to start the node.
    Gate(E),
    /// The gate passed but loading/replaying the durable state failed.
    Load(StorageError),
}

/// #3301: the boot ordering, as one testable step. `install_gate` must
/// resolve the chain's protocol (signing version, activation table, feature
/// schedule) from DURABLE block 1 (`storage.load_lowest_block()`) and install
/// it; only then is the state loaded, which replays the WAL tail with
/// per-block transitions and finalization.
///
/// The order is the point. [`Storage::load_state`] replays blocks through
/// `feature_active` and the signing version; run before the gate, every block
/// past a feature's activation was re-applied under pre-activation rules (no
/// schedule installed = every feature off), and the node then refused to boot
/// on the #2219 tip commitment. On a chain that schedules nothing, the
/// installed values equal the defaults replay used before, so nothing
/// changes there.
///
/// #3277 S3: the replay also refreshes the applied-extension overlay of the
/// effective feature schedule at every block boundary
/// ([`Storage::load_state_observing_replay`]), so a block replayed past an
/// extension's activation sees it active, as it was when first applied.
pub fn gate_then_load_state<T, E>(
    storage: &Storage,
    install_gate: impl FnOnce(&Storage) -> Result<T, E>,
) -> Result<(T, Option<Blockchain>), GateThenLoadError<E>> {
    let gate = install_gate(storage).map_err(GateThenLoadError::Gate)?;
    // #3277 S3: replay each block under the effective schedule (base from
    // the gate, plus the extensions applied before that block).
    let loaded = storage
        .load_state_observing_replay(State::refresh_effective_feature_schedule)
        .map_err(GateThenLoadError::Load)?;
    Ok((gate, loaded))
}

/// #2219 whole-replay integrity gate: the reconstructed state must match the
/// tip block's committed post-state root.
///
/// The WAL carries only `(height, transition)` pairs, so the per-block checked
/// finalization sink cannot run during replay — this verifies the RESULT of
/// checkpoint+WAL reconstruction once, against the commitment in the tip
/// block's hashed metadata. Locally persisted data; the per-block adversarial
/// checks live on the P2P apply and reorg paths, where untrusted input enters.
///
/// **Must run AFTER the protocol gate** (`assert_startup_protocol_compatibility`)
/// — it resolves the tip height through the genesis-installed activation
/// schedule. Since #3301 the gate runs before `Storage::load_state` too (see
/// [`gate_then_load_state`]), but this check stays a separate, explicit step
/// after both: without an installed gate it would silently fall back to the
/// V1 default and skip on every real V2 boot (codex P1 on PR #2357).
pub fn verify_tip_state_root_commitment(
    blockchain: &Blockchain,
) -> Result<(), StorageCorruptionError> {
    verify_tip_state_root_commitment_at(
        wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(blockchain.height),
        blockchain,
    )
}

/// Version-explicit variant of [`verify_tip_state_root_commitment`], the same
/// pattern as `State::apply_block_finalization_checked_at`: production calls
/// the `active()` wrapper; tests exercise the V2 contract directly.
pub fn verify_tip_state_root_commitment_at(
    version: wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion,
    blockchain: &Blockchain,
) -> Result<(), StorageCorruptionError> {
    use wrfcoin_shared_protocol::blockchain::block::STATE_ROOT_METADATA_KEY;
    use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;

    if version < SigningProtocolVersion::V2 || blockchain.height == 0 {
        return Ok(());
    }
    // FAIL CLOSED on a missing tip block (gpt review of PR #2357): the
    // in-memory window always spans `height - max + 1 ..= height`, so at a
    // non-zero persisted height the tip block is present on every consistent
    // database. Its absence means BLOCKS_TABLE is missing, truncated, or
    // inconsistent with META_HEIGHT — returning Ok here would silently SKIP
    // the mandatory V2 commitment check on exactly the corrupt databases it
    // exists to catch.
    let Some(tip_block) = blockchain.blocks().get(&blockchain.height) else {
        return Err(StorageCorruptionError::MissingTipBlock {
            height: blockchain.height,
        });
    };
    let committed = tip_block
        .metadata
        .get(STATE_ROOT_METADATA_KEY)
        .cloned()
        .unwrap_or_default();
    let replayed = blockchain.state.state_root.to_string();
    if committed != replayed {
        return Err(StorageCorruptionError::StateRootCommitmentMismatch {
            height: blockchain.height,
            committed,
            replayed,
        });
    }
    Ok(())
}

/// Why the durable producer-authority backstop refused the boot (#2290).
#[derive(Debug)]
pub enum PersistedProducerAuthorityError {
    /// A persisted block was produced by a key the on-chain set does not
    /// authorise. `height` is the LOWEST such block, so the operator is pointed
    /// at where the fork began rather than at its tip.
    Unauthorized { height: u64, reason: String },
    /// The durable block archive could not be read. Fail-closed: an unreadable
    /// archive is a refusal, never a silent skip — skipping is exactly how the
    /// in-memory-window version of this check became vacuous.
    Archive(StorageError),
    /// The chain carries producer-SIGNED blocks but holds NO producer set, so
    /// every producer-authority gate would read it as an unconfigured chain and
    /// silently not enforce (#2501). `height` is the lowest signed block.
    ///
    /// Distinct from [`Self::Unauthorized`] because the operator action differs:
    /// `Unauthorized` means this chain contains a block the network rejects (a
    /// fork — investigate that block). This means the chain LOST its producer
    /// set, almost always via checkpoint-less WAL recovery, and the state is
    /// intact on peers — resync rather than investigate.
    SignedBlocksWithoutProducerSet { height: u64 },
}

impl std::fmt::Display for PersistedProducerAuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized { height, reason } => write!(
                f,
                "persisted block at height {height} is not from an authorized producer ({reason})"
            ),
            Self::Archive(e) => write!(
                f,
                "the durable block archive could not be scanned for producer authority: {e}"
            ),
            Self::SignedBlocksWithoutProducerSet { height } => write!(
                f,
                "this chain has NO producer set but its block at height {height} is \
                 producer-signed, so producer authority would not be enforced. The \
                 set is genesis state and is not reconstructible from the WAL, so a \
                 checkpoint-less recovery loses it (#2501) — resync this node from a \
                 peer rather than starting unenforced"
            ),
        }
    }
}

/// Re-verify producer authority over the **whole durable block archive** at boot
/// (#2290).
///
/// # Why not `blockchain.blocks()`
///
/// The first version of this backstop passed the in-memory window
/// (`Blockchain::blocks()`). `Storage::load_state` restores only
/// [`configured_max_in_memory_blocks`] blocks (default 200), so on a chain of
/// any real length the check examined the newest 200 blocks and silently
/// trusted every older one — including the unauthorized block it exists to
/// catch. A reviewer demonstrated this with a 201-block chain and a 1-block
/// window: the helper reported 1 block checked, not 201. An answer that depends
/// on the retention window is an answer that depends on how the operator
/// configured a cache, which is not an acceptable basis for a consensus
/// decision. This reads the durable table, which is the whole chain.
///
/// # Memory
///
/// Streams the archive under ONE redb read transaction (a consistent snapshot)
/// and materialises at most [`PERSISTED_BLOCK_SCAN_CHUNK`] decoded blocks at a
/// time, so a 200k-block chain costs a bounded working set rather than the whole
/// archive in RAM. The scan stops at the first offending chunk; because chunks
/// are visited in ascending height and the shared helper reports the lowest
/// offender within a chunk, the reported height is the global lowest.
///
/// Unenforced when the chain never configured a producer set (empty history) —
/// the same activation semantics as every other call site, so this cannot brick
/// a legacy or dev chain, and it also means an unconfigured chain pays no scan.
///
/// **Must run AFTER `assert_startup_protocol_compatibility`**, for the same
/// reason [`verify_tip_state_root_commitment`] must: the per-block gate reads
/// the genesis-installed activation schedule, and before the gate installs it
/// resolution falls back to the V1 default, which would evaluate the wrong
/// preimage on every real V2 boot (the #2275 uninstalled-lie class).
pub fn verify_persisted_producer_authority(
    storage: &Storage,
    producer_set_history: &[wrfcoin_shared_protocol::consensus::producer_set::ProducerSetChange],
) -> Result<usize, PersistedProducerAuthorityError> {
    use wrfcoin_shared_protocol::consensus::producer_authority::verify_persisted_blocks_authorized;

    // #2501: an empty history means "producer auth not configured" and is
    // deliberately unenforced — that is what keeps legacy and dev chains
    // working. But it is ALSO what a checkpoint-less WAL recovery produces on a
    // chain that WAS enforcing, and that state must not start. Signed blocks
    // discriminate: a never-configured chain has none.
    //
    // At V2 the tip state-root commitment check (#2219) already refuses this,
    // since the set is folded into `state_root`. This is the backstop for V1,
    // where that check is inactive by design because V1 hashes are frozen and
    // carry no commitment — so a pre-cut chain with a seeded set has the loss
    // with none of the detection.
    //
    // Cost: a genuinely unconfigured chain now pays one chunked archive pass at
    // boot instead of an O(1) short-circuit, because proving NO block is signed
    // requires looking at all of them. The degraded case breaks at the first
    // signed block. Accepted — the alternative is a silent hole, and the scan is
    // the same bounded-memory one the authorized path already performs.
    if producer_set_history.is_empty() {
        let mut lowest_signed: Option<u64> = None;
        let scanned = storage
            .scan_persisted_blocks(|chunk| {
                let refs = chunk.iter().map(|(h, b)| (h, b));
                match wrfcoin_shared_protocol::consensus::producer_authority::lowest_signed_block(
                    refs,
                ) {
                    Some(h) => {
                        lowest_signed = Some(h);
                        std::ops::ControlFlow::Break(())
                    },
                    None => std::ops::ControlFlow::Continue(()),
                }
            })
            .map_err(PersistedProducerAuthorityError::Archive)?;
        return match lowest_signed {
            Some(height) => {
                Err(PersistedProducerAuthorityError::SignedBlocksWithoutProducerSet { height })
            },
            None => Ok(scanned),
        };
    }

    let mut violation: Option<(u64, String)> = None;
    let scanned = storage
        .scan_persisted_blocks(|chunk| {
            let refs = chunk.iter().map(|(h, b)| (h, b));
            match verify_persisted_blocks_authorized(refs, producer_set_history) {
                Ok(_) => std::ops::ControlFlow::Continue(()),
                Err(bad) => {
                    violation = Some(bad);
                    std::ops::ControlFlow::Break(())
                },
            }
        })
        .map_err(PersistedProducerAuthorityError::Archive)?;

    match violation {
        Some((height, reason)) => {
            Err(PersistedProducerAuthorityError::Unauthorized { height, reason })
        },
        None => Ok(scanned),
    }
}

/// A single WAL entry recording one state transition.
#[derive(Debug, Clone)]
struct WalEntry {
    block_height: u64,
    transition: StateTransition,
}

#[derive(Debug, Serialize, Deserialize)]
struct LegacyWalEntry {
    block_height: u64,
    transition: StateTransition,
}

#[derive(Serialize, Deserialize)]
struct WalEntryV2 {
    block_height: u64,
    transition_json: Vec<u8>,
}

/// Crash-safe blockchain storage backed by redb.
///
/// A clone is a read handle over the same redb database; it does not duplicate
/// WAL sequencing ownership.  This lets async callers move read-only range
/// loads to Tokio's blocking pool without holding an async lock there.
#[derive(Clone)]
pub struct Storage {
    db: Arc<Database>,
    encryption: Option<StorageEncryption>,
    wal_sequence: u64,
    checkpoint_interval: u64,
    /// Checkpoints `load_state` migrated IN MEMORY from an older format
    /// (#3091), held until the boot assertions pass and `launch_api` calls
    /// [`Storage::commit_migrated_checkpoints`]. Shared across clones so the
    /// handle the boot path holds sees what `load_state` recorded.
    pending_migrations: Arc<std::sync::Mutex<Vec<MigratedCheckpoint>>>,
}

/// A checkpoint that was migrated in memory and not yet written back (#3091).
#[derive(Debug, Clone)]
pub struct MigratedCheckpoint {
    /// Checkpoint height (the `CHECKPOINTS_TABLE` key).
    pub height: u64,
    /// The envelope version the row carried on disk.
    pub from_version: u32,
    /// The complete current-version envelope to write back.
    pub bytes: Vec<u8>,
}

impl Storage {
    /// Durable public evidence, independent of the expiring HTTP ledger.
    /// Duplicate writes are idempotent; conflicting keys never overwrite.
    pub fn archive_node_receipt(&self, key: &[u8;32], bytes: &[u8]) -> Result<(), StorageError> {
        if bytes.len() > MAX_NODE_ARCHIVE_RECORD_BYTES { return Err(StorageError::Serialization("Node archive bound".into())); }
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(NODE_RECEIPT_ARCHIVE)?;
            let old = table.get(key.as_slice())?.map(|entry| entry.value().to_vec());
            if let Some(old) = old {
                if old != bytes { return Err(StorageError::Serialization("Node archive conflict".into())); }
            } else {
                if table.len()? >= MAX_NODE_ARCHIVE_RECORDS { return Err(StorageError::Serialization("Node archive capacity".into())); }
                table.insert(key.as_slice(), bytes)?;
            }
        }
        txn.commit()?;
        Ok(())
    }
    pub fn archived_node_receipt(&self, key: &[u8;32]) -> Result<Option<Vec<u8>>, StorageError> {
        let txn = self.db.begin_read()?;
        let table = match txn.open_table(NODE_RECEIPT_ARCHIVE) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let result = table.get(key.as_slice())?.map(|entry| entry.value().to_vec());
        Ok(result)
    }

    /// Open or create a storage database at the given path.
    pub fn open(path: &Path, checkpoint_interval: u64) -> Result<Self, StorageError> {
        Self::open_with_encryption(path, None, checkpoint_interval)
    }

    /// Open or create a storage database at the given path with optional value encryption.
    pub fn open_with_encryption(
        path: &Path,
        encryption_key: Option<&str>,
        checkpoint_interval: u64,
    ) -> Result<Self, StorageError> {
        let db = Database::create(path)?;
        let encryption = encryption_key
            .map(StorageEncryption::from_env_key)
            .transpose()
            .map_err(StorageError::Encryption)?;

        // Ensure all tables exist.
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(BLOCKS_TABLE)?;
            let _ = write_txn.open_table(CHECKPOINTS_TABLE)?;
            let _ = write_txn.open_table(WAL_TABLE)?;
            let _ = write_txn.open_table(META_TABLE)?;
        }
        write_txn.commit()?;

        // Read current WAL sequence.
        let wal_sequence = {
            let read_txn = db.begin_read()?;
            let meta = read_txn.open_table(META_TABLE)?;
            match meta.get(META_WAL_SEQ).ok().flatten() {
                Some(v) => decode_u64_meta(
                    &decrypt_value(encryption.as_ref(), v.value())?,
                    META_WAL_SEQ,
                )?,
                None => 0,
            }
        };

        Ok(Self {
            db: Arc::new(db),
            encryption,
            wal_sequence,
            checkpoint_interval,
            pending_migrations: Arc::new(std::sync::Mutex::new(Vec::new())),
        })
    }

    /// Checkpoints `load_state` migrated in memory and has not yet written
    /// back (#3091), as `(height, from_version)`.
    pub fn pending_checkpoint_migrations(&self) -> Vec<(u64, u32)> {
        self.pending_migrations
            .lock()
            .map(|p| p.iter().map(|m| (m.height, m.from_version)).collect())
            .unwrap_or_default()
    }

    /// Write-after-verify (#3091): persist every checkpoint `load_state`
    /// migrated in memory, replacing the older-format row with the
    /// current-version envelope. Call ONLY after the boot assertions (WAL
    /// replay, #2219 tip commitment, #2290 producer authority) have passed —
    /// until then the on-disk bytes stay exactly what the older binary wrote,
    /// so a failed boot or an image rollback still reads them.
    ///
    /// Returns the `(height, from_version)` of each row written. Idempotent:
    /// a second call writes nothing.
    pub fn commit_migrated_checkpoints(&self) -> Result<Vec<(u64, u32)>, StorageError> {
        let pending: Vec<MigratedCheckpoint> = {
            let mut guard = self
                .pending_migrations
                .lock()
                .map_err(|_| StorageError::Serialization("migration lock poisoned".into()))?;
            std::mem::take(&mut *guard)
        };
        if pending.is_empty() {
            return Ok(Vec::new());
        }
        let write_txn = self.db.begin_write()?;
        {
            let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE)?;
            for migrated in &pending {
                debug_assert!(
                    decode_state_checkpoint(&migrated.bytes, migrated.height).is_ok(),
                    "commit_migrated_checkpoints: migrated bytes must decode at height {}",
                    migrated.height
                );
                let encrypted = encrypt_value(self.encryption.as_ref(), &migrated.bytes)?;
                checkpoints.insert(migrated.height, encrypted.as_slice())?;
            }
        }
        write_txn.commit()?;
        let written: Vec<(u64, u32)> = pending.iter().map(|m| (m.height, m.from_version)).collect();
        for migrated in &pending {
            let (height, from) = (migrated.height, migrated.from_version);
            let writer_version =
                u32::from_le_bytes(migrated.bytes[8..12].try_into().expect("version"));
            log::info!(
                "storage: persisted migrated checkpoint at height {height} as format \
                 {writer_version} (was format {from}); the volume no longer needs \
                 a migration on the next boot (#3091)"
            );
        }
        Ok(written)
    }

    /// Load the persisted blockchain state, if any.
    ///
    /// Returns `None` if the database is empty (fresh start).
    /// On success, returns a `Blockchain` reconstructed from the latest
    /// checkpoint plus WAL replay.
    pub fn load_state(&self) -> Result<Option<Blockchain>, StorageError> {
        self.load_state_observing_replay(|_| {})
    }

    /// [`Self::load_state`], calling `on_canonical_state` with the replay
    /// state at every point where it is the canonical state of a block
    /// boundary (#3277 S3): once on the replay anchor (checkpoint, genesis
    /// anchor or empty state) before the first WAL block, and once after each
    /// replayed block's finalization.
    ///
    /// Boot passes [`State::refresh_effective_feature_schedule`] here (via
    /// [`gate_then_load_state`]) so each block is replayed under the effective
    /// feature schedule the producer and the P2P applier used for it: the
    /// state after the PREVIOUS block, exactly as the live hooks refresh it
    /// after every block apply. Plain `load_state` (tests, tooling) observes
    /// nothing and never touches the process-wide schedule.
    pub fn load_state_observing_replay(
        &self,
        mut on_canonical_state: impl FnMut(&State),
    ) -> Result<Option<Blockchain>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let meta = read_txn.open_table(META_TABLE)?;

        // Check if we have any persisted height.
        let height = match meta.get(META_HEIGHT).ok().flatten() {
            Some(v) => decode_u64_meta(&self.decrypt_value(v.value())?, META_HEIGHT)?,
            None => return Ok(None),
        };

        // Load tip hash.
        let tip_bytes = meta
            .get(META_TIP)
            .ok()
            .flatten()
            .map(|v| self.decrypt_value(v.value()))
            .transpose()?
            .unwrap_or_default();
        let tip: Hash = bincode::deserialize(&tip_bytes)
            .map_err(|e| StorageError::Serialization(format!("tip decode failed: {e}")))?;

        // Load latest checkpoint.
        let checkpoints = read_txn.open_table(CHECKPOINTS_TABLE)?;
        let last_checkpoint_height = meta
            .get(META_LAST_CHECKPOINT)
            .ok()
            .flatten()
            .map(|v| self.decrypt_value(v.value()))
            .transpose()?
            .as_deref()
            .map(|bytes| decode_u64_meta(bytes, META_LAST_CHECKPOINT))
            .transpose()?
            .unwrap_or(0);

        // `effective_checkpoint_height` tracks the height from which WAL replay
        // should start. If metadata names a checkpoint, that checkpoint is the
        // safe replay anchor; missing or unreadable anchors must stop startup.
        let effective_checkpoint_height = last_checkpoint_height;
        let mut state = if last_checkpoint_height > 0 {
            match checkpoints.get(last_checkpoint_height)? {
                // #2727: the envelope-aware decoder distinguishes a genuinely
                // corrupt checkpoint from one written by an incompatible
                // binary, and names the format gap with operator options.
                Some(v) => {
                    let (state, migrated) = decode_state_checkpoint_migrating(
                        &self.decrypt_value(v.value())?,
                        last_checkpoint_height,
                    )?;
                    if let Some(migrated) = migrated {
                        // #3091: decoded through a registered migration.
                        // Recorded, NOT written: the write-back happens in
                        // `commit_migrated_checkpoints` after the boot
                        // assertions pass (write-after-verify).
                        log::info!(
                            "storage: migrated checkpoint at height {} from format {} to {} \
                             (in memory; written back after boot verification, #3091)",
                            migrated.height,
                            migrated.from_version,
                            u32::from_le_bytes(migrated.bytes[8..12].try_into().expect("version"))
                        );
                        if let Ok(mut pending) = self.pending_migrations.lock() {
                            pending.push(migrated);
                        }
                    }
                    state
                },
                None => return Err(StorageCorruptionError::MissingSnapshot {
                    height: last_checkpoint_height,
                }
                .into()),
            }
        } else if let Some(genesis) = self.read_genesis_anchor(&checkpoints)? {
            // #2501: no post-genesis checkpoint, so replay anchors on the
            // GENESIS state the node actually started from — not `State::new()`.
            //
            // Genesis-only state is real: `State::seed_genesis_producer_set`
            // writes the authorized producer set straight into genesis `State`,
            // and no `StateTransition` describes it, so the WAL cannot carry it.
            // Anchoring on a default genesis replays a perfectly faithful log
            // onto the wrong base — `producer_set_history` comes back empty,
            // which every gate reads as "this chain never configured producer
            // auth" (#2290). At V2 the #2219 tip commitment turns that into a
            // refusal to start; at V1 the node comes up unenforced. Reached when
            // a node is stopped ABRUPTLY below height 100, so neither the
            // periodic nor the shutdown checkpoint ran.
            log::info!(
                "No post-genesis checkpoint; anchoring WAL replay on the persisted \
                 genesis state (state_root {}) (#2501)",
                genesis.state_root
            );
            genesis
        } else {
            State::new()
        };

        // #3277 S3: the anchor is the canonical state after its block; the
        // first replayed block was applied live under the schedule it implies.
        on_canonical_state(&state);

        // Replay WAL entries after the checkpoint through the same batch
        // transition-apply entrypoint used by the P2P block applier (#1682).
        let wal = read_txn.open_table(WAL_TABLE)?;
        let mut replayed = 0u64;
        // Replay WAL entries grouped by block: apply each block's transitions,
        // then run the SAME per-block finalization the producer and the P2P
        // applier run (`State::apply_block_finalization`). WAL entries are
        // appended in block order, so consecutive entries sharing a
        // `block_height` form one block. Without per-block finalization a
        // restarted node would accumulate stale `pending_commits`/replay-set
        // entries and its `state_root` would drift from the producer's — the
        // same commit-reveal lifecycle divergence fixed on the P2P apply path.
        let node_blocks = read_txn.open_table(BLOCKS_TABLE)?;
        let validate_node_inputs = |state: &State, height: u64, inputs: &[StateTransition]| -> Result<(), StorageError> {
            let active = state.node_share_activation_height().is_some_and(|a| height >= a);
            let has_node_input = inputs.iter().any(|t| matches!(t, StateTransition::BeginNodeServiceBlock { .. } | StateTransition::ApplyNodeBondLifecycle { .. } | StateTransition::ApplyNodeServiceBatch { .. } | StateTransition::ApplyNodeFraudReport { .. }));
            if !active && !has_node_input { return Ok(()); }
            let bytes = node_blocks.get(height)?.ok_or_else(|| StorageError::Serialization("missing archived block for Node WAL context".into()))?;
            let block: Block = bincode::deserialize(&self.decrypt_value(bytes.value())?).map_err(|e| StorageError::Serialization(e.to_string()))?;
            wrfcoin_shared_protocol::consensus::node_bond_registry::validate_block_inputs(&block, inputs, active)
                .map_err(|e| StorageError::Serialization(format!("Node WAL context refused: {e}")))
        };
        let mut pending_height: Option<u64> = None;
        let mut block_transitions: Vec<StateTransition> = Vec::new();
        for entry_result in wal.iter()? {
            let (key, value) = entry_result.map_err(StorageError::RedbStorage)?;
            let seq = key.value();
            let raw = self.decrypt_value(value.value())?;

            // Verify checksum: first 4 bytes are CRC32, rest is payload.
            let (stored_crc, payload) = decode_wal_prefix(&raw, seq)?;
            let computed_crc = crc32fast::hash(payload);
            if stored_crc != computed_crc {
                return Err(StorageError::ChecksumMismatch {
                    sequence: seq,
                    expected: stored_crc,
                    actual: computed_crc,
                });
            }

            let entry = decode_wal_entry(payload, seq)?;

            // Only replay entries after the effective checkpoint height.
            if entry.block_height <= effective_checkpoint_height {
                continue;
            }

            // Block boundary — finalize the previous block before the next.
            if pending_height.is_some_and(|h| h != entry.block_height) {
                let h = pending_height.expect("checked is_some_and above");
                validate_node_inputs(&state, h, &block_transitions)?;
                state
                    .apply_transitions_batch_at_height(
                        h,
                        std::mem::take(&mut block_transitions),
                    )
                    .map_err(|e| {
                        StorageError::Serialization(format!("WAL replay batch error: {e}"))
                    })?;
                state.apply_block_finalization(h);
                state.validate_node_finalized(h).map_err(|e| StorageError::Serialization(format!("WAL Node settlement error: {e}")))?;
                on_canonical_state(&state);
            }
            pending_height = Some(entry.block_height);
            block_transitions.push(entry.transition);
            replayed += 1;
        }

        if let Some(h) = pending_height {
            validate_node_inputs(&state, h, &block_transitions)?;
            state
                .apply_transitions_batch_at_height(h, block_transitions)
                .map_err(|e| {
                    StorageError::Serialization(format!("WAL replay batch error: {e}"))
                })?;
            state.apply_block_finalization(h);
            state.validate_node_finalized(h).map_err(|e| StorageError::Serialization(format!("WAL Node settlement error: {e}")))?;
            on_canonical_state(&state);
        }

        if replayed > 0 {
            log::info!(
                "Replayed {replayed} WAL entries after checkpoint at height {effective_checkpoint_height}"
            );
        }

        // Load only the most recent configured window of blocks from the blocks table.
        let blocks_table = read_txn.open_table(BLOCKS_TABLE)?;
        let total_block_count = blocks_table.len()?;
        let max = configured_max_in_memory_blocks();
        let start_height = height.saturating_sub(max as u64 - 1);
        let mut blocks = BTreeMap::new();
        for entry_result in blocks_table.range(start_height..)? {
            let (key, value) = entry_result.map_err(StorageError::RedbStorage)?;
            let height_key = key.value();
            let block: Block =
                bincode::deserialize(&self.decrypt_value(value.value())?).map_err(|e| {
                    StorageError::Serialization(format!(
                        "block decode failed at height key {height_key}: {e}"
                    ))
                })?;
            blocks.insert(height_key, block);
        }

        // #2219: the whole-replay tip-commitment check does NOT run here. At
        // boot, `load_state` runs AFTER the protocol gate (#3301,
        // [`gate_then_load_state`]: the gate reads genesis metadata from the
        // durable block 1, then this replay runs under the installed version
        // and schedule). But `load_state` is also callable with no gate
        // installed (tests, tooling), where a version read here would be the V1
        // default and the check vacuous on every real V2 chain — the #2275
        // uninstalled-lie class (codex P1 on PR #2357). So the caller runs
        // [`verify_tip_state_root_commitment`] explicitly, after the gate.

        // Load persisted finalized height (issue #1275). Absent on pre-#1275
        // databases — default to 0 in that case, matching the old behavior
        // for existing chains that never persisted this field.
        let finalized_height = meta
            .get(META_FINALIZED_HEIGHT)
            .ok()
            .flatten()
            .map(|v| self.decrypt_value(v.value()))
            .transpose()?
            .as_deref()
            .map(|bytes| decode_u64_meta(bytes, META_FINALIZED_HEIGHT))
            .transpose()?
            .unwrap_or(0);

        let blockchain = Blockchain::from_persisted(
            height,
            tip,
            state,
            blocks,
            total_block_count,
            finalized_height,
        );

        log::info!(
            "Loaded blockchain from storage: height={}, in_memory_blocks={}, total_blocks={}, checkpoint_at={}",
            height,
            blockchain.in_memory_block_count(),
            total_block_count,
            last_checkpoint_height
        );

        Ok(Some(blockchain))
    }

    /// Persist a newly mined block and its state transitions atomically.
    ///
    /// The synchronous redb I/O runs on the tokio blocking thread pool via
    /// `spawn_blocking`, keeping the async runtime responsive for P2P gossip,
    /// sync, and API handlers during disk writes.
    pub async fn persist_block(
        &mut self,
        block: Block,
        transitions: Vec<StateTransition>,
        checkpoint_state_bytes: Option<Vec<u8>>,
    ) -> Result<(), StorageError> {
        // Backward-compat shim: callers that do not supply a finalized
        // height pass 0. This preserves prior behavior for tests; production
        // callers should use `persist_block_with_finalized` so the BFT
        // finalized height is persisted alongside every block commit
        // (issue #1275).
        self.persist_block_with_finalized(block, transitions, checkpoint_state_bytes, 0)
            .await
    }

    /// Same as [`persist_block`] but also persists the current BFT
    /// `finalized_height`, so that a restart cannot silently reset
    /// `Blockchain::finalized_height` to 0 and permit reorgs of
    /// already-finalized blocks (issue #1275).
    pub async fn persist_block_with_finalized(
        &mut self,
        block: Block,
        transitions: Vec<StateTransition>,
        checkpoint_state_bytes: Option<Vec<u8>>,
        finalized_height: u64,
    ) -> Result<(), StorageError> {
        let db = Arc::clone(&self.db);
        let encryption = self.encryption.clone();
        let checkpoint_due = self.needs_checkpoint(block.height);
        let mut wal_seq = self.wal_sequence;

        let new_wal_seq = tokio::task::spawn_blocking(move || {
            persist_block_sync(
                &db,
                encryption.as_ref(),
                &mut wal_seq,
                &block,
                &transitions,
                checkpoint_due,
                checkpoint_state_bytes.as_deref(),
                finalized_height,
            )?;
            Ok::<u64, StorageError>(wal_seq)
        })
        .await
        .map_err(|e| StorageError::SpawnBlocking(e.to_string()))??;

        self.wal_sequence = new_wal_seq;
        Ok(())
    }

    /// Persist just the monotonic BFT `finalized_height` value. Called by
    /// the finality-signal consumer (see `finality_events.rs`) so that
    /// finality advances caused by BFT signals (not tied to a new block)
    /// are also durable across restarts. Idempotent; safe to call with
    /// unchanged values (issue #1275).
    pub async fn persist_finalized_height(
        &self,
        finalized_height: u64,
    ) -> Result<(), StorageError> {
        let db = Arc::clone(&self.db);
        let encryption = self.encryption.clone();
        tokio::task::spawn_blocking(move || {
            persist_finalized_height_sync(&db, encryption.as_ref(), finalized_height)
        })
        .await
        .map_err(|e| StorageError::SpawnBlocking(e.to_string()))?
    }

    pub fn needs_checkpoint(&self, height: u64) -> bool {
        height > 0 && height % self.checkpoint_interval == 0
    }

    // ------------------------------------------------------------------
    // Genesis anchor (#2501)
    // ------------------------------------------------------------------

    /// Persist the genesis `State` as the height-0 replay anchor.
    ///
    /// Called once, from the fresh-genesis startup branch, **before** any block
    /// is persisted. See [`GENESIS_ANCHOR_HEIGHT`] for why replay needs it: the
    /// genesis producer set is written into `State` directly and described by no
    /// `StateTransition`, so a checkpoint-less WAL replay that anchors on
    /// `State::new()` reconstructs the log perfectly onto a base that is missing
    /// it, and the chain comes up unenforced (#2290) or — at V2 — unable to
    /// start at all, with no checkpoint below height 100 to fall back to.
    ///
    /// **Write-once, fail-closed.** Re-persisting an anchor whose `state_root`
    /// differs from the stored one means this data directory belongs to a
    /// different chain, so it is a refusal rather than an overwrite. Re-writing
    /// an identical anchor is a no-op, which keeps restarts and re-runs safe.
    pub async fn persist_genesis_state(&self, state: &State) -> Result<(), StorageError> {
        let db = Arc::clone(&self.db);
        let encryption = self.encryption.clone();
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            persist_genesis_state_sync(&db, encryption.as_ref(), &state)
        })
        .await
        .map_err(|e| StorageError::SpawnBlocking(e.to_string()))?
    }

    /// Read the height-0 genesis anchor, if one was written.
    ///
    /// `Ok(None)` means genuinely absent — a chain created before this existed,
    /// or a dev chain whose genesis is plain `State::new()`. An anchor that is
    /// present but undecodable is an `Err`, never a silent `None`: falling back
    /// to a default genesis is precisely the failure this anchor exists to stop.
    pub fn load_genesis_state(&self) -> Result<Option<State>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let checkpoints = read_txn.open_table(CHECKPOINTS_TABLE)?;
        self.read_genesis_anchor(&checkpoints)
    }

    /// Shared reader so [`Storage::load_state`] can resolve the anchor inside
    /// the read transaction it already holds.
    fn read_genesis_anchor(
        &self,
        checkpoints: &impl ReadableTable<u64, &'static [u8]>,
    ) -> Result<Option<State>, StorageError> {
        let Some(row) = checkpoints.get(GENESIS_ANCHOR_HEIGHT)? else {
            return Ok(None);
        };
        let bytes = self.decrypt_value(row.value())?;
        match decode_state_checkpoint(&bytes, GENESIS_ANCHOR_HEIGHT) {
            Ok(state) => Ok(Some(state)),
            // Preserve the #2501 anchor-specific corruption error; format-gap
            // errors (#2727) pass through with their own operator guidance.
            Err(StorageError::Corruption(StorageCorruptionError::SnapshotUnreadable {
                error,
                ..
            })) => Err(StorageCorruptionError::GenesisAnchorUnreadable { error }.into()),
            Err(other) => Err(other),
        }
    }

    pub fn checkpoint_state_bytes(
        &self,
        height: u64,
        state: &State,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        if self.needs_checkpoint(height) {
            Ok(Some(Self::serialize_state(state)?))
        } else {
            Ok(None)
        }
    }

    fn serialize_state(state: &State) -> Result<Vec<u8>, StorageError> {
        encode_state_checkpoint(state)
    }

    fn decrypt_value(&self, stored: &[u8]) -> Result<Vec<u8>, StorageError> {
        decrypt_value(self.encryption.as_ref(), stored)
    }

    /// Write a full state checkpoint at the given height, pruning WAL entries
    /// at or below that height. Intended for use at shutdown to ensure the
    /// database is in a clean state for fast restart.
    ///
    /// No-op if `height == 0`.
    pub async fn force_checkpoint(&self, state: &State, height: u64) -> Result<(), StorageError> {
        if height == 0 {
            return Ok(());
        }

        let db = Arc::clone(&self.db);
        let encryption = self.encryption.clone();
        let state = state.clone();

        tokio::task::spawn_blocking(move || {
            force_checkpoint_sync(&db, encryption.as_ref(), &state, height)
        })
        .await
        .map_err(|e| StorageError::SpawnBlocking(e.to_string()))?
    }

    /// Whether this database holds any chain history: a persisted height, a
    /// block, or a WAL entry (#3288 3b). The height-0 genesis anchor is not
    /// history; every fresh node writes it before its first block.
    pub fn has_chain_history(&self) -> Result<bool, StorageError> {
        let read_txn = self.db.begin_read()?;
        chain_history_present(
            &read_txn.open_table(META_TABLE)?,
            &read_txn.open_table(BLOCKS_TABLE)?,
            &read_txn.open_table(WAL_TABLE)?,
        )
    }

    /// Install a verified snapshot as this node's whole chain (#3288 3b).
    ///
    /// ONE write transaction writes the carried `blocks` (the height-1 genesis
    /// block plus the window ending at `height`), `META_HEIGHT = height`,
    /// `META_TIP = tip`, the finalized height, and the checkpoint at `height`
    /// with `META_LAST_CHECKPOINT`. A restart then loads exactly this chain:
    /// `load_state` anchors on the checkpoint, the tip block is in the window
    /// for the #2219 commitment check, and the lowest block is the genesis
    /// block the boot path requires.
    ///
    /// Refuses with [`StorageError::ChainHistoryPresent`], checked inside the
    /// same transaction, when the database already has chain history.
    pub async fn install_snapshot(
        &self,
        state: &State,
        height: u64,
        tip: Hash,
        blocks: &[Block],
        finalized_height: u64,
    ) -> Result<(), StorageError> {
        let db = Arc::clone(&self.db);
        let encryption = self.encryption.clone();
        let state_bytes = encode_state_checkpoint(state)?;
        let blocks = blocks.to_vec();
        tokio::task::spawn_blocking(move || {
            install_snapshot_sync(
                &db,
                encryption.as_ref(),
                &state_bytes,
                height,
                tip,
                &blocks,
                finalized_height,
            )
        })
        .await
        .map_err(|e| StorageError::SpawnBlocking(e.to_string()))?
    }

    // ------------------------------------------------------------------
    // Chain reorganization helpers
    // ------------------------------------------------------------------

    /// Load the most recent checkpoint at or before `height`.
    ///
    /// Returns `(checkpoint_height, state)` or `None` if no checkpoint exists
    /// at or below the requested height.
    pub fn load_checkpoint_at_or_before(
        &self,
        height: u64,
    ) -> Result<Option<(u64, State)>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let checkpoints = read_txn.open_table(CHECKPOINTS_TABLE)?;

        // Scan backwards from `height` to find the nearest checkpoint.
        let mut best: Option<(u64, Vec<u8>)> = None;
        for entry_result in checkpoints.range(..=height)? {
            let (key, value) = entry_result.map_err(StorageError::RedbStorage)?;
            let h = key.value();
            let data = self.decrypt_value(value.value())?;
            best = Some((h, data));
        }

        match best {
            Some((h, data)) => match decode_state_checkpoint(&data, h) {
                Ok(state) => Ok(Some((h, state))),
                Err(e) => {
                    log::warn!(
                        "Checkpoint at height {h} could not be decoded ({e}); \
                             treating as absent"
                    );
                    Ok(None)
                },
            },
            None => Ok(None),
        }
    }

    /// Load the lowest-height block persisted in the block archive, if any.
    ///
    /// Used to resolve the genesis/chain-identity block (#1714): this chain's
    /// genesis is the **height-1** block (`height == 1 && prev_block_hash ==
    /// Hash::default()`), not a height-0 block. On a long-running node the
    /// genesis block has been evicted from the in-memory window, so it must be
    /// read from the durable archive. The archive only ever drops blocks
    /// *above* a rollback target, never the lowest blocks, so the first entry
    /// in `BLOCKS_TABLE` is the genesis block.
    pub fn load_lowest_block(&self) -> Result<Option<Block>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let blocks_table = read_txn.open_table(BLOCKS_TABLE)?;
        // Decrypt into an owned buffer while the access guard is alive, then
        // drop it before deserializing to avoid holding the table borrow.
        let plaintext = match blocks_table.first()? {
            Some((_key, value)) => self.decrypt_value(value.value())?,
            None => return Ok(None),
        };
        let block: Block = bincode::deserialize(&plaintext)
            .map_err(|e| StorageError::Serialization(format!("block decode failed: {e}")))?;
        Ok(Some(block))
    }

    /// #3301: whether the durable archive holds NO block while the metadata
    /// records a persisted height — storage that `load_state` would load a
    /// chain from although the protocol gate, reading the archive, would call
    /// the chain uncut. Read-only (one read transaction, no writes), so boot
    /// can refuse this state BEFORE loading anything under a
    /// configuration-derived protocol.
    pub fn archive_empty_but_height_recorded(&self) -> Result<bool, StorageError> {
        let read_txn = self.db.begin_read()?;
        let blocks_table = read_txn.open_table(BLOCKS_TABLE)?;
        if blocks_table.first()?.is_some() {
            return Ok(false);
        }
        let meta = read_txn.open_table(META_TABLE)?;
        let recorded = meta.get(META_HEIGHT)?.is_some();
        Ok(recorded)
    }

    /// Stream **every** block in the durable archive, oldest height first, in
    /// bounded chunks of at most [`PERSISTED_BLOCK_SCAN_CHUNK`] decoded blocks.
    ///
    /// This is the whole-chain counterpart to [`Self::load_blocks_range`], which
    /// requires the caller to already know the range, and to
    /// [`Blockchain::blocks`], which is only the retention window. Boot-time
    /// integrity checks must not be scoped to a cache: use this.
    ///
    /// Memory is bounded by the chunk size, not by the chain length — a 200k
    /// block archive is decoded [`PERSISTED_BLOCK_SCAN_CHUNK`] blocks at a time
    /// and each chunk is dropped before the next is read. The whole scan runs
    /// inside ONE redb read transaction, so `visit` observes a single consistent
    /// snapshot even if writers are active.
    ///
    /// `visit` returns [`std::ops::ControlFlow::Break`] to stop early; the
    /// returned count is the number of blocks decoded and handed to `visit`
    /// before stopping.
    pub fn scan_persisted_blocks<F>(&self, mut visit: F) -> Result<usize, StorageError>
    where
        F: FnMut(&[(u64, Block)]) -> std::ops::ControlFlow<()>,
    {
        let read_txn = self.db.begin_read()?;
        let blocks_table = read_txn.open_table(BLOCKS_TABLE)?;
        let mut chunk: Vec<(u64, Block)> = Vec::with_capacity(PERSISTED_BLOCK_SCAN_CHUNK);
        let mut scanned = 0usize;

        for entry_result in blocks_table.iter()? {
            let (key, value) = entry_result.map_err(StorageError::RedbStorage)?;
            let height_key = key.value();
            let block: Block =
                bincode::deserialize(&self.decrypt_value(value.value())?).map_err(|e| {
                    StorageError::Serialization(format!(
                        "block decode failed at height key {height_key}: {e}"
                    ))
                })?;
            chunk.push((height_key, block));
            scanned += 1;

            if chunk.len() == PERSISTED_BLOCK_SCAN_CHUNK {
                let flow = visit(&chunk);
                chunk.clear();
                if flow.is_break() {
                    return Ok(scanned);
                }
            }
        }

        if !chunk.is_empty() {
            let _ = visit(&chunk);
        }
        Ok(scanned)
    }

    /// Load blocks from the archive in the given height range (inclusive).
    /// Load `start..=end`, stopping once `max_bytes` of stored block data has
    /// been consumed (#2119).
    ///
    /// The unbounded [`Self::load_blocks_range`] decrypts and deserializes the
    /// WHOLE requested range before any caller-side trimming can run, so a
    /// response-side byte budget bounds what is *sent* and not what is *done* —
    /// a hostile request still forces up to `MAX_BLOCKS_PER_REQUEST` full block
    /// decodes. This variant checks the stored length **before** decrypting each
    /// block, so the work stops at the budget instead of after it.
    ///
    /// Always returns at least one block when the range is non-empty, matching
    /// the serve path's rule that a single oversized block is served rather than
    /// stalling the requester forever.
    ///
    /// Returns `(blocks, bytes_consumed)` where the byte count is of the STORED
    /// representation — that is the quantity this bounds, and it is an upper
    /// bound on the serialized response since the stored form carries encryption
    /// overhead on top.
    pub fn load_blocks_range_bounded(
        &self,
        start: u64,
        end: u64,
        max_bytes: u64,
    ) -> Result<(Vec<Block>, u64), StorageError> {
        let read_txn = self.db.begin_read()?;
        let blocks_table = read_txn.open_table(BLOCKS_TABLE)?;
        let mut blocks = Vec::new();
        let mut used = 0u64;
        for entry_result in blocks_table.range(start..=end)? {
            let (_key, value) = entry_result.map_err(StorageError::RedbStorage)?;
            let stored_len = value.value().len() as u64;
            // Decide BEFORE paying for the decrypt+deserialize.
            if !blocks.is_empty() && used.saturating_add(stored_len) > max_bytes {
                break;
            }
            let block: Block = bincode::deserialize(&self.decrypt_value(value.value())?)
                .map_err(|e| StorageError::Serialization(format!("block decode failed: {e}")))?;
            used = used.saturating_add(stored_len);
            blocks.push(block);
        }
        Ok((blocks, used))
    }

    pub fn load_blocks_range(&self, start: u64, end: u64) -> Result<Vec<Block>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let blocks_table = read_txn.open_table(BLOCKS_TABLE)?;
        let mut blocks = Vec::new();
        for entry_result in blocks_table.range(start..=end)? {
            let (_key, value) = entry_result.map_err(StorageError::RedbStorage)?;
            let block: Block = bincode::deserialize(&self.decrypt_value(value.value())?)
                .map_err(|e| StorageError::Serialization(format!("block decode failed: {e}")))?;
            blocks.push(block);
        }
        Ok(blocks)
    }

    /// Load WAL entries for a given block height range and return them
    /// grouped by ascending block height.
    pub fn load_wal_transitions_range(
        &self,
        start_height: u64,
        end_height: u64,
    ) -> Result<Vec<(u64, StateTransition)>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let wal = read_txn.open_table(WAL_TABLE)?;
        let mut result = Vec::new();

        for entry_result in wal.iter()? {
            let (key, value) = entry_result.map_err(StorageError::RedbStorage)?;
            let seq = key.value();
            let raw = self.decrypt_value(value.value())?;

            // Verify checksum: first 4 bytes are CRC32, rest is payload.
            // Unlike rollback/checkpoint helpers that tolerate unparseable entries,
            // this function is used during reorg fork-state reconstruction where a
            // silently-corrupted entry would produce wrong fork state and diverge
            // consensus.  Fail closed here, same as load_state.
            let (stored_crc, payload) = decode_wal_prefix(&raw, seq)?;
            let computed_crc = crc32fast::hash(payload);
            if stored_crc != computed_crc {
                return Err(StorageError::ChecksumMismatch {
                    sequence: seq,
                    expected: stored_crc,
                    actual: computed_crc,
                });
            }

            let entry = decode_wal_entry(payload, seq)?;
            if entry.block_height >= start_height && entry.block_height <= end_height {
                result.push((entry.block_height, entry.transition));
            }
        }

        // Sort by block height for deterministic replay order.
        result.sort_by_key(|(h, _)| *h);
        Ok(result)
    }

    /// Atomically roll back persisted state to `target_height`.
    ///
    /// Removes all blocks with height > `target_height`, deletes their WAL
    /// entries, and updates metadata (height + tip). Runs on the blocking
    /// thread pool.
    pub async fn rollback_to_height(
        &mut self,
        target_height: u64,
        new_tip: Hash,
    ) -> Result<(), StorageError> {
        let db = Arc::clone(&self.db);
        let encryption = self.encryption.clone();
        let new_tip_bytes =
            bincode::serialize(&new_tip).map_err(|e| StorageError::Serialization(e.to_string()))?;

        tokio::task::spawn_blocking(move || {
            rollback_to_height_sync(&db, encryption.as_ref(), target_height, &new_tip_bytes)
        })
        .await
        .map_err(|e| StorageError::SpawnBlocking(e.to_string()))?
    }

    /// Load the permissionless replay-seen map from the meta table (#1845).
    ///
    /// Returns an empty map when no snapshot has been persisted yet.
    pub fn load_permissionless_replay(
        &self,
    ) -> Result<std::collections::HashMap<String, i64>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let meta = read_txn.open_table(META_TABLE)?;
        match meta.get(META_PERMISSIONLESS_REPLAY).ok().flatten() {
            None => Ok(std::collections::HashMap::new()),
            Some(raw) => {
                let decrypted = self.decrypt_value(raw.value())?;
                serde_json::from_slice(&decrypted).map_err(|e| {
                    StorageError::Serialization(format!("replay-seen decode: {e}"))
                })
            }
        }
    }

    /// Persist the permissionless replay-seen map to the meta table (#1845).
    ///
    /// Callers must prune TTL-expired entries before calling so the snapshot
    /// stays bounded.
    /// The protocol version recorded by the last binary that booted this
    /// volume (#3093), or `None` on a volume no such binary has run.
    pub fn load_recorded_protocol_version(&self) -> Result<Option<u32>, StorageError> {
        let read_txn = self.db.begin_read()?;
        let meta = read_txn.open_table(META_TABLE)?;
        match meta.get(META_PROTOCOL_VERSION).ok().flatten() {
            None => Ok(None),
            Some(raw) => {
                let bytes = self.decrypt_value(raw.value())?;
                let arr: [u8; 4] = bytes.as_slice().try_into().map_err(|_| {
                    StorageError::Serialization(format!(
                        "recorded protocol version has {} bytes, expected 4",
                        bytes.len()
                    ))
                })?;
                Ok(Some(u32::from_le_bytes(arr)))
            },
        }
    }

    /// Record this binary's protocol version as the last to run this volume
    /// (#3093). Called after the boot checks pass.
    pub fn record_protocol_version(&self, version: u32) -> Result<(), StorageError> {
        let encrypted = encrypt_value(self.encryption.as_ref(), &version.to_le_bytes())?;
        let write_txn = self.db.begin_write()?;
        {
            let mut meta = write_txn.open_table(META_TABLE)?;
            meta.insert(META_PROTOCOL_VERSION, encrypted.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn save_permissionless_replay(
        &self,
        map: &std::collections::HashMap<String, i64>,
    ) -> Result<(), StorageError> {
        let encoded = serde_json::to_vec(map).map_err(|e| {
            StorageError::Serialization(format!("replay-seen encode: {e}"))
        })?;
        let encrypted = encrypt_value(self.encryption.as_ref(), &encoded)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut meta = write_txn.open_table(META_TABLE)?;
            meta.insert(META_PERMISSIONLESS_REPLAY, encrypted.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }
}

/// Synchronous helper: persist just the BFT finalized height (issue #1275).
///
/// Runs in a tiny dedicated redb transaction so finality-signal updates
/// don't have to wait for a block commit.
fn persist_finalized_height_sync(
    db: &Database,
    encryption: Option<&StorageEncryption>,
    finalized_height: u64,
) -> Result<(), StorageError> {
    let write_txn = db.begin_write()?;
    {
        let mut meta = write_txn.open_table(META_TABLE)?;
        let encrypted = encrypt_value(encryption, &finalized_height.to_le_bytes())?;
        meta.insert(META_FINALIZED_HEIGHT, encrypted.as_slice())?;
    }
    write_txn.commit()?;
    Ok(())
}

/// Synchronous rollback: remove blocks and WAL entries above `target_height`.
fn rollback_to_height_sync(
    db: &Database,
    encryption: Option<&StorageEncryption>,
    target_height: u64,
    new_tip_bytes: &[u8],
) -> Result<(), StorageError> {
    let write_txn = db.begin_write()?;
    {
        // Remove blocks above target_height.
        let mut blocks = write_txn.open_table(BLOCKS_TABLE)?;
        let heights_to_remove: Vec<u64> = blocks
            .range((target_height + 1)..)
            .map_err(StorageError::RedbStorage)?
            .map(|r| r.map(|(k, _)| k.value()))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::RedbStorage)?;
        for h in &heights_to_remove {
            blocks.remove(*h)?;
        }

        // Remove WAL entries for those blocks.
        let mut wal = write_txn.open_table(WAL_TABLE)?;
        let wal_to_remove: Vec<u64> = {
            let mut remove = Vec::new();
            for entry_result in wal.iter()? {
                let (key, value) = entry_result.map_err(StorageError::RedbStorage)?;
                let seq = key.value();
                let raw = decrypt_value(encryption, value.value())?;
                if let Ok((_crc, payload)) = decode_wal_prefix(&raw, seq) {
                    let entry = decode_wal_entry(payload, seq)?;
                    if entry.block_height > target_height {
                        remove.push(seq);
                    }
                }
            }
            remove
        };
        for seq in wal_to_remove {
            wal.remove(seq)?;
        }

        // Update metadata.
        let mut meta = write_txn.open_table(META_TABLE)?;
        let encrypted_height = encrypt_value(encryption, &target_height.to_le_bytes())?;
        let encrypted_tip = encrypt_value(encryption, new_tip_bytes)?;
        meta.insert(META_HEIGHT, encrypted_height.as_slice())?;
        meta.insert(META_TIP, encrypted_tip.as_slice())?;

        // Prune checkpoints above target_height and update META_LAST_CHECKPOINT
        // so a crash-restart after a reorg doesn't replay from a stale anchor
        // (#1854).
        let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE)?;
        let mut stale_checkpoints = Vec::new();
        let mut new_last_checkpoint = 0u64;
        for entry_result in checkpoints.iter()? {
            let (key, _) = entry_result.map_err(StorageError::RedbStorage)?;
            let h = key.value();
            if h > target_height {
                stale_checkpoints.push(h);
            } else if h > new_last_checkpoint {
                new_last_checkpoint = h;
            }
        }
        for h in stale_checkpoints {
            checkpoints.remove(h)?;
        }
        let encrypted_checkpoint_height =
            encrypt_value(encryption, &new_last_checkpoint.to_le_bytes())?;
        meta.insert(META_LAST_CHECKPOINT, encrypted_checkpoint_height.as_slice())?;
    }
    write_txn.commit()?;

    log::info!("Storage rolled back to height {}", target_height);
    Ok(())
}

/// Wrap a serialized [`State`] in the versioned checkpoint envelope (#2727):
/// Activated Node state uses format12. OFF state retains format11 or,
/// with an empty owner ledger, emits the exact
/// deployed format10 prefix and envelope instead. Every checkpoint/anchor write
/// goes through here so a
/// future layout change fails at startup with a *named format gap* instead of
/// a misleading serde error.
fn encode_state_checkpoint(state: &State) -> Result<Vec<u8>, StorageError> {
    migrations::encode_compatible(state).map_err(|e| StorageError::Serialization(e.to_string()))
}

/// Decode checkpoint/anchor bytes written by [`encode_state_checkpoint`] or by
/// a pre-envelope binary (#2727).
///
/// Three outcomes, all fail-closed and none labeled "corrupt" unless the
/// bytes actually are:
/// - **Versioned, matching**: decode; a decode error here is genuine
///   corruption ([`StorageCorruptionError::SnapshotUnreadable`]).
/// - **Versioned, other version**: refuse with
///   [`StorageCorruptionError::CheckpointFormatVersionMismatch`] naming both
///   versions — never attempt the decode, because a positional bincode decode
///   of a different layout can fail confusingly or, worse, succeed silently
///   with wrong values.
/// - **Legacy (no magic)**: raw pre-envelope `bincode(State)`. Decoding with
///   the current layout accepts volumes written by same-layout binaries from
///   before the envelope existed; a failure names the format gap
///   ([`StorageCorruptionError::LegacyCheckpointIncompatible`]) with operator
///   options, because the node container incident showed "CORRUPT" sends the operator
///   to exactly the wrong place when the data is intact.
fn decode_state_checkpoint(bytes: &[u8], height: u64) -> Result<State, StorageError> {
    decode_state_checkpoint_migrating(bytes, height).map(|(state, _)| state)
}

/// [`decode_state_checkpoint`] plus the migration record (#3091): when the
/// envelope carries an OLDER supported version with a registered path, the
/// bytes are migrated in memory and returned alongside the decoded `State`
/// so the caller can write them back after verification. Anchors and reorg
/// helpers use the plain wrapper and never write.
fn decode_state_checkpoint_migrating(
    bytes: &[u8],
    height: u64,
) -> Result<(State, Option<MigratedCheckpoint>), StorageError> {
    let magic_len = CHECKPOINT_ENVELOPE_MAGIC.len();
    let header_len = magic_len + 4;

    // #2881 review, blocker 1: entering a decode branch must be a POSITIVE
    // identification, never a fallthrough, and every length prefix must be
    // bounds-checked before anything trusts it. The original fallthrough sent
    // an envelope whose header was truncated (power loss / killed process
    // mid-write leaves exactly this) into the LEGACY branch, where bincode
    // read the magic itself as the `accounts` map length — 4.17 EiB — and the
    // process SIGABRTed on the allocation. An abort on a wild allocation is
    // strictly worse than the misleading refusal #2727 fixed.

    // Too short to be either format: a legacy checkpoint starts with an 8-byte
    // map length, an envelope with a 12-byte header. No binary of any era
    // wrote fewer bytes than that.
    if bytes.len() < magic_len.max(8) {
        return Err(StorageCorruptionError::SnapshotUnreadable {
            height,
            error: format!(
                "{} bytes is too short to be a checkpoint of either format \
                 (legacy needs >= 8, enveloped needs >= {header_len})",
                bytes.len()
            ),
        }
        .into());
    }

    if bytes[..magic_len] == CHECKPOINT_ENVELOPE_MAGIC {
        // Positively an envelope. It must NEVER fall through to legacy from
        // here — the magic bytes themselves are a wild length prefix.
        if bytes.len() < header_len {
            return Err(StorageCorruptionError::SnapshotUnreadable {
                height,
                error: format!(
                    "truncated checkpoint envelope: magic present but only {} \
                     of the {header_len} header bytes exist — a partially \
                     written checkpoint (power loss / killed process)",
                    bytes.len()
                ),
            }
            .into());
        }
        let found = u32::from_le_bytes(
            bytes[magic_len..header_len]
                .try_into()
                .expect("slice is 4 bytes"),
        );
        if found > CHECKPOINT_FORMAT_VERSION {
            // Written by a NEWER binary: never migrate downward, never decode.
            return Err(StorageCorruptionError::CheckpointFormatVersionMismatch {
                height,
                found,
                supported: CHECKPOINT_FORMAT_VERSION,
            }
            .into());
        }
        if found < CHECKPOINT_FORMAT_VERSION {
            // #3091: an OLDER supported format migrates in place — or refuses
            // by name when no path is registered. A body that does not decode
            // as a complete State of the version it claims is corruption of
            // the row, not a format gap, and says so.
            let migrated = migrations::migrate_to_current(found, &bytes[header_len..])
                .map_err(|e| -> StorageError {
                    match e {
                        migrations::MigrationError::NoPath { .. } => {
                            StorageCorruptionError::CheckpointMigrationUnavailable {
                                height,
                                found,
                                supported: CHECKPOINT_FORMAT_VERSION,
                            }
                            .into()
                        },
                        migrations::MigrationError::Decode { .. } => {
                            StorageCorruptionError::SnapshotUnreadable {
                                height,
                                error: e.to_string(),
                            }
                            .into()
                        },
                        migrations::MigrationError::Encode { .. } => {
                            StorageError::Serialization(e.to_string())
                        },
                    }
                })?;
            let state = bounded_state_decode(&migrated[header_len..]).map_err(|e| {
                StorageError::Serialization(format!(
                    "migrated checkpoint (format {found} -> {CHECKPOINT_FORMAT_VERSION}) at \
                     height {height} does not decode: {e}"
                ))
            })?;
            state.validate_node_bond_anchor(height).map_err(|e| StorageError::Serialization(format!("node bond checkpoint anchor: {e}")))?;
            // Write-after-verification also uses the compatible writer.
            // A deployed v10 row with no owner entries needs no rewrite.
            // Preserve stored map ordering: migration appended defaults to
            // the original payload, so do not reserialize the decoded State.
            // A migration from an older format never invents a send
            // allowlist: the appended map is empty (its u64 zero length).
            let v15_body = &migrated[header_len..migrated.len() - 8];
            let compatible = if !state.send_allowlists.is_empty() {
                migrated.clone()
            } else if state.node_receipt_registry.is_none() {
                // Each absent appended Option occupies one byte. Preserve all
                // prior bytes, including historical map iteration order.
                // Receipt, settlement, and fraud each append an absent Option tag.
                let v12_body = &v15_body[..v15_body.len() - 3];
                if state.node_bond_registry.is_none() {
                    let v11_body = &v12_body[..v12_body.len() - 1];
                    if state.station_owner_ledger.is_empty() { migrations::envelope(10, &v11_body[..v11_body.len() - 8]) }
                    else { migrations::envelope(11, v11_body) }
                } else { migrations::envelope(12, v12_body) }
            } else { migrations::envelope(15, v15_body) };
            let writer_version = u32::from_le_bytes(compatible[8..12].try_into().expect("version"));
            let pending = (found != writer_version).then_some(MigratedCheckpoint {
                height,
                from_version: found,
                bytes: compatible,
            });
            return Ok((state, pending));
        }
        return bounded_state_decode(&bytes[header_len..])
            .and_then(|state| { state.validate_node_bond_anchor(height).map_err(|e| Box::new(bincode::ErrorKind::Custom(e.to_string())))?; Ok((state, None)) })
            .map_err(|e| {
                StorageCorruptionError::SnapshotUnreadable {
                    height,
                    error: e.to_string(),
                }
                .into()
            });
    }

    // Legacy candidacy is also positive, not residual: raw `bincode(State)`
    // begins with the `accounts` map length as u64 LE, and a real map of N
    // entries needs far more than N bytes of encoding. A leading count that
    // exceeds the stored byte length cannot be a checkpoint any binary wrote
    // — refuse it BEFORE bincode is allowed to allocate for it.
    let leading_count = u64::from_le_bytes(bytes[..8].try_into().expect("slice is 8 bytes"));
    if leading_count > bytes.len() as u64 {
        return Err(StorageCorruptionError::SnapshotUnreadable {
            height,
            error: format!(
                "leading length prefix {leading_count} exceeds the stored \
                 {} bytes — not a plausible checkpoint of either format",
                bytes.len()
            ),
        }
        .into());
    }
    bounded_state_decode(bytes)
        .or_else(|current_error| {
            // Same deployed format10 without the historical envelope remains
            // readable, using the strict frozen decoder before appending.
            match migrations::migrate_to_current(10, bytes) {
                Ok(migrated) => bounded_state_decode(&migrated[12..]),
                Err(_) => Err(current_error),
            }
        })
        .and_then(|state| { state.validate_node_bond_anchor(height).map_err(|e| Box::new(bincode::ErrorKind::Custom(e.to_string())))?; Ok((state, None)) })
        .map_err(|e| {
            StorageCorruptionError::LegacyCheckpointIncompatible {
                height,
                error: e.to_string(),
            }
            .into()
        })
}

/// Decode a `State` with bincode's size limit set to the input length (#2881
/// review, blocker 1 defense-in-depth): the leading-prefix check above stops
/// the demonstrated top-level wild allocation, and the limit bounds every
/// NESTED length prefix the same way — bincode refuses with a size-limit
/// error instead of attempting the allocation. Wire-compatible with
/// `bincode::serialize`'s default config (fixint + trailing bytes allowed);
/// valid data reads at most its own length, so the limit never binds on it.
fn bounded_state_decode(bytes: &[u8]) -> Result<State, bincode::Error> {
    use bincode::Options;
    let mut state: State = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit(bytes.len() as u64)
        .deserialize(bytes)?;
    state.restore_node_emission_runtime()
        .map_err(|e| Box::new(bincode::ErrorKind::Custom(e.to_string())))?;
    Ok(state)
}

/// Synchronous genesis-anchor write (#2501). See
/// [`Storage::persist_genesis_state`].
fn persist_genesis_state_sync(
    db: &Database,
    encryption: Option<&StorageEncryption>,
    state: &State,
) -> Result<(), StorageError> {
    let state_bytes = encode_state_checkpoint(state)?;
    debug_assert!(
        decode_state_checkpoint(&state_bytes, GENESIS_ANCHOR_HEIGHT).is_ok(),
        "persist_genesis_state: state checkpoint round-trip failed"
    );

    let incoming_root = state.state_root.to_string();
    let write_txn = db.begin_write()?;
    let wrote = {
        let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE)?;
        // Scoped so the row borrow ends before `checkpoints` is used mutably.
        let existing_root: Option<String> = match checkpoints.get(GENESIS_ANCHOR_HEIGHT)? {
            Some(row) => {
                let bytes = decrypt_value(encryption, row.value())?;
                let existing = match decode_state_checkpoint(&bytes, GENESIS_ANCHOR_HEIGHT) {
                    Ok(state) => state,
                    Err(StorageError::Corruption(
                        StorageCorruptionError::SnapshotUnreadable { error, .. },
                    )) => {
                        return Err(StorageCorruptionError::GenesisAnchorUnreadable { error }
                            .into());
                    },
                    Err(other) => return Err(other),
                };
                Some(existing.state_root.to_string())
            },
            None => None,
        };
        match existing_root {
            // Write-once: an anchor with a different root is a different chain.
            Some(existing) if existing != incoming_root => {
                return Err(StorageCorruptionError::GenesisAnchorMismatch {
                    existing,
                    incoming: incoming_root,
                }
                .into());
            },
            // Identical anchor — no-op, so restarts and re-runs stay safe.
            Some(_) => false,
            None => {
                let encrypted_state = encrypt_value(encryption, state_bytes.as_slice())?;
                checkpoints.insert(GENESIS_ANCHOR_HEIGHT, encrypted_state.as_slice())?;
                true
            },
        }
    };
    write_txn.commit()?;

    if wrote {
        log::info!(
            "Persisted genesis anchor at height {} ({} bytes, state_root {}) — \
             checkpoint-less WAL replay will reconstruct from the real genesis, not a \
             default one (#2501)",
            GENESIS_ANCHOR_HEIGHT,
            state_bytes.len(),
            incoming_root
        );
    } else {
        log::debug!(
            "Genesis anchor already present at state_root {incoming_root}; leaving it \
             untouched (#2501)"
        );
    }
    Ok(())
}

/// Synchronous shutdown-checkpoint: write a full State checkpoint at the given
/// height, prune WAL entries at-or-below, and remove old checkpoints.
fn force_checkpoint_sync(
    db: &Database,
    encryption: Option<&StorageEncryption>,
    state: &State,
    height: u64,
) -> Result<(), StorageError> {
    let state_bytes = encode_state_checkpoint(state)?;

    // Round-trip verification (debug only): the root cause of the bincode
    // round-trip failure (#1053) was `skip_serializing_if` on State fields,
    // which is incompatible with bincode's positional format.  Now fixed,
    // but keep a debug assertion as a canary.
    debug_assert!(
        decode_state_checkpoint(&state_bytes, height).is_ok(),
        "force_checkpoint: state checkpoint round-trip failed at height {height}"
    );

    let write_txn = db.begin_write()?;
    {
        let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE)?;
        let encrypted_state = encrypt_value(encryption, state_bytes.as_slice())?;
        checkpoints.insert(height, encrypted_state.as_slice())?;

        let mut meta = write_txn.open_table(META_TABLE)?;
        let encrypted_checkpoint_height = encrypt_value(encryption, &height.to_le_bytes())?;
        meta.insert(META_LAST_CHECKPOINT, encrypted_checkpoint_height.as_slice())?;

        // Prune WAL entries at or below this height.
        let mut wal = write_txn.open_table(WAL_TABLE)?;
        let mut to_remove = Vec::new();
        for entry_result in wal.iter()? {
            let (key, value) = entry_result.map_err(StorageError::RedbStorage)?;
            let seq = key.value();
            let raw = decrypt_value(encryption, value.value())?;
            if let Ok((_crc, payload)) = decode_wal_prefix(&raw, seq) {
                let entry = decode_wal_entry(payload, seq)?;
                if entry.block_height <= height {
                    to_remove.push(seq);
                }
            }
        }
        for seq in to_remove {
            wal.remove(seq)?;
        }

        // Remove old checkpoints (keep only the new one), EXCEPT the height-0
        // genesis anchor: it is the only reconstruction path for genesis-only
        // state, so pruning it here would silently restore #2501 at the first
        // shutdown checkpoint — and because this same transaction prunes the
        // WAL, the loss would not surface until a much later restart.
        let mut old_checkpoints = Vec::new();
        for entry_result in checkpoints.iter()? {
            let (key, _) = entry_result.map_err(StorageError::RedbStorage)?;
            let h = key.value();
            if h != GENESIS_ANCHOR_HEIGHT && h < height {
                old_checkpoints.push(h);
            }
        }
        for h in old_checkpoints {
            checkpoints.remove(h)?;
        }
    }
    write_txn.commit()?;

    log::info!(
        "Shutdown checkpoint at height {} ({} bytes)",
        height,
        state_bytes.len()
    );
    Ok(())
}

fn chain_history_present(
    meta: &impl ReadableTable<&'static str, &'static [u8]>,
    blocks: &impl ReadableTable<u64, &'static [u8]>,
    wal: &impl ReadableTable<u64, &'static [u8]>,
) -> Result<bool, StorageError> {
    Ok(meta.get(META_HEIGHT)?.is_some() || blocks.len()? > 0 || wal.len()? > 0)
}

/// See [`Storage::install_snapshot`].
fn install_snapshot_sync(
    db: &Database,
    encryption: Option<&StorageEncryption>,
    state_bytes: &[u8],
    height: u64,
    tip: Hash,
    blocks: &[Block],
    finalized_height: u64,
) -> Result<(), StorageError> {
    debug_assert!(
        decode_state_checkpoint(state_bytes, height).is_ok(),
        "install_snapshot: state checkpoint round-trip failed at height {height}"
    );
    let write_txn = db.begin_write()?;
    {
        let mut meta = write_txn.open_table(META_TABLE)?;
        let mut blocks_table = write_txn.open_table(BLOCKS_TABLE)?;
        let wal = write_txn.open_table(WAL_TABLE)?;
        if chain_history_present(&meta, &blocks_table, &wal)? {
            return Err(StorageError::ChainHistoryPresent);
        }
        drop(wal);

        for block in blocks {
            let block_bytes = bincode::serialize(block)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            let encrypted_block = encrypt_value(encryption, block_bytes.as_slice())?;
            blocks_table.insert(block.height, encrypted_block.as_slice())?;
        }

        let encrypted_height = encrypt_value(encryption, &height.to_le_bytes())?;
        meta.insert(META_HEIGHT, encrypted_height.as_slice())?;
        let tip_bytes =
            bincode::serialize(&tip).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted_tip = encrypt_value(encryption, tip_bytes.as_slice())?;
        meta.insert(META_TIP, encrypted_tip.as_slice())?;
        let encrypted_finalized = encrypt_value(encryption, &finalized_height.to_le_bytes())?;
        meta.insert(META_FINALIZED_HEIGHT, encrypted_finalized.as_slice())?;

        let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE)?;
        let encrypted_state = encrypt_value(encryption, state_bytes)?;
        checkpoints.insert(height, encrypted_state.as_slice())?;
        let encrypted_checkpoint_height = encrypt_value(encryption, &height.to_le_bytes())?;
        meta.insert(META_LAST_CHECKPOINT, encrypted_checkpoint_height.as_slice())?;
    }
    write_txn.commit()?;
    log::info!(
        "Installed snapshot chain at height {height}: {} blocks, checkpoint {} bytes (#3288)",
        blocks.len(),
        state_bytes.len()
    );
    Ok(())
}

/// Synchronous persist implementation run on the blocking thread pool.
///
/// Block, WAL entries, and metadata are written in one fast redb transaction.
/// If a checkpoint is due, it is written in a **separate** transaction so that
/// the (potentially large) checkpoint I/O on HDD never stalls block persistence.
/// The state bytes are pre-serialized by the caller, outside any transaction.
#[allow(clippy::excessive_nesting)]
fn persist_block_sync(
    db: &Database,
    encryption: Option<&StorageEncryption>,
    wal_sequence: &mut u64,
    block: &Block,
    transitions: &[StateTransition],
    checkpoint_due: bool,
    checkpoint_state_bytes: Option<&[u8]>,
    finalized_height: u64,
) -> Result<(), StorageError> {
    // #2086: one durable transaction for block, WAL, metadata and checkpoint.
    // A checkpoint error must not leave a committed block with stale WAL sequencing.
    let write_txn = db.begin_write()?;

    {
        // Write WAL entries.
        let mut wal = write_txn.open_table(WAL_TABLE)?;
        for transition in transitions {
            *wal_sequence += 1;
            let entry = WalEntry {
                block_height: block.height,
                transition: transition.clone(),
            };
            let payload = encode_wal_entry(&entry)?;
            let crc = crc32fast::hash(&payload);
            let mut value = Vec::with_capacity(4 + payload.len());
            value.extend_from_slice(&crc.to_le_bytes());
            value.extend_from_slice(&payload);
            let encrypted_value = encrypt_value(encryption, value.as_slice())?;
            wal.insert(*wal_sequence, encrypted_value.as_slice())?;
        }

        // Write block.
        let mut blocks = write_txn.open_table(BLOCKS_TABLE)?;
        let block_bytes =
            bincode::serialize(block).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted_block = encrypt_value(encryption, block_bytes.as_slice())?;
        blocks.insert(block.height, encrypted_block.as_slice())?;

        // Update metadata.
        let mut meta = write_txn.open_table(META_TABLE)?;
        let encrypted_height = encrypt_value(encryption, &block.height.to_le_bytes())?;
        meta.insert(META_HEIGHT, encrypted_height.as_slice())?;
        let tip_bytes = bincode::serialize(&block.hash.unwrap_or_else(|| block.calculate_hash()))
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted_tip = encrypt_value(encryption, tip_bytes.as_slice())?;
        meta.insert(META_TIP, encrypted_tip.as_slice())?;
        let encrypted_wal_seq = encrypt_value(encryption, &wal_sequence.to_le_bytes())?;
        meta.insert(META_WAL_SEQ, encrypted_wal_seq.as_slice())?;

        // Persist the BFT finalized height alongside the block height so
        // restarts cannot silently reset it to 0 (issue #1275).
        let encrypted_finalized = encrypt_value(encryption, &finalized_height.to_le_bytes())?;
        meta.insert(META_FINALIZED_HEIGHT, encrypted_finalized.as_slice())?;
    }

    // Checkpoint writes and WAL pruning belong to the same commit boundary.
    if checkpoint_due {
        let state_bytes = checkpoint_state_bytes.ok_or_else(|| {
            StorageError::Serialization(format!(
                "missing checkpoint state bytes for block {}",
                block.height
            ))
        })?;

        // Round-trip verification (debug only): see #1053 fix — the root cause
        // was `skip_serializing_if` on State fields.  Keep a debug assertion as canary.
        debug_assert!(
            decode_state_checkpoint(state_bytes, block.height).is_ok(),
            "persist_block_sync: state checkpoint round-trip failed at height {}",
            block.height
        );

        let encrypted_state = encrypt_value(encryption, state_bytes)?;

        {
            let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE)?;
            checkpoints.insert(block.height, encrypted_state.as_slice())?;

            let mut meta = write_txn.open_table(META_TABLE)?;
            let encrypted_checkpoint_height =
                encrypt_value(encryption, &block.height.to_le_bytes())?;
            meta.insert(META_LAST_CHECKPOINT, encrypted_checkpoint_height.as_slice())?;

            // Prune WAL entries for blocks at or before the checkpoint.
            // All WAL entries at checkpoint time belong to blocks ≤ current height
            // by construction, so key-only iteration is sufficient.
            let mut wal = write_txn.open_table(WAL_TABLE)?;
            let to_remove: Vec<u64> = wal
                .iter()?
                .map(|r| r.map(|(k, _)| k.value()).map_err(StorageError::RedbStorage))
                .collect::<Result<Vec<_>, _>>()?;
            for seq in to_remove {
                wal.remove(seq)?;
            }

            // Remove old checkpoints (keep only the latest), EXCEPT the height-0
            // genesis anchor — see the matching note in `force_checkpoint_sync`.
            let mut old_checkpoints = Vec::new();
            for entry_result in checkpoints.iter()? {
                let (key, _value) = entry_result.map_err(StorageError::RedbStorage)?;
                let h = key.value();
                if h != GENESIS_ANCHOR_HEIGHT && h < block.height {
                    old_checkpoints.push(h);
                }
            }
            for h in old_checkpoints {
                checkpoints.remove(h)?;
            }
        }
    }

    write_txn.commit()?;
    if let Some(state_bytes) = checkpoint_state_bytes.filter(|_| checkpoint_due) {
        log::info!(
            "Created checkpoint at height {} (state size: {} bytes)",
            block.height,
            state_bytes.len()
        );
    }
    Ok(())
}

fn decode_u64_meta(bytes: &[u8], key: &'static str) -> Result<u64, StorageError> {
    if bytes.len() != 8 {
        return Err(StorageCorruptionError::MetaLengthInvalid {
            key: key.to_string(),
            expected: 8,
            actual: bytes.len(),
        }
        .into());
    }
    let raw: [u8; 8] = bytes
        .try_into()
        .map_err(|_| StorageCorruptionError::MetaLengthInvalid {
            key: key.to_string(),
            expected: 8,
            actual: bytes.len(),
        })?;
    Ok(u64::from_le_bytes(raw))
}

fn encrypt_value(
    encryption: Option<&StorageEncryption>,
    plaintext: &[u8],
) -> Result<Vec<u8>, StorageError> {
    match encryption {
        Some(encryption) => encryption
            .encrypt(plaintext)
            .map_err(StorageError::Encryption),
        None => Ok(plaintext.to_vec()),
    }
}

fn decrypt_value(
    encryption: Option<&StorageEncryption>,
    stored: &[u8],
) -> Result<Vec<u8>, StorageError> {
    match encryption {
        Some(encryption) => encryption.decrypt(stored).map_err(StorageError::Encryption),
        None => Ok(stored.to_vec()),
    }
}

fn decode_wal_prefix<'a>(raw: &'a [u8], sequence: u64) -> Result<(u32, &'a [u8]), StorageError> {
    if raw.len() < 5 {
        return Err(StorageCorruptionError::WalPrefixInvalid {
            sequence,
            actual_len: raw.len(),
        }
        .into());
    }

    let crc_bytes: [u8; 4] =
        raw[0..4]
            .try_into()
            .map_err(|_| StorageCorruptionError::WalPrefixInvalid {
                sequence,
                actual_len: raw.len(),
            })?;
    let payload = &raw[4..];
    Ok((u32::from_le_bytes(crc_bytes), payload))
}

fn encode_wal_entry(entry: &WalEntry) -> Result<Vec<u8>, StorageError> {
    let transition_json = serde_json::to_vec(&entry.transition).map_err(|e| {
        StorageError::Serialization(format!("WAL transition JSON encode failed: {e}"))
    })?;
    let encoded = bincode::serialize(&WalEntryV2 {
        block_height: entry.block_height,
        transition_json,
    })
    .map_err(|e| StorageError::Serialization(format!("WAL v2 encode failed: {e}")))?;

    let mut payload = Vec::with_capacity(WAL_V2_MAGIC.len() + encoded.len());
    payload.extend_from_slice(WAL_V2_MAGIC);
    payload.extend_from_slice(&encoded);
    Ok(payload)
}

fn decode_wal_entry(payload: &[u8], sequence: u64) -> Result<WalEntry, StorageError> {
    if let Some(versioned) = payload.strip_prefix(WAL_V2_MAGIC) {
        let entry: WalEntryV2 = bincode::deserialize(versioned).map_err(|e| {
            StorageError::Serialization(format!(
                "WAL v2 entry decode failed at sequence {sequence}: {e}"
            ))
        })?;
        let transition = serde_json::from_slice(&entry.transition_json).map_err(|e| {
            StorageError::Serialization(format!(
                "WAL v2 transition JSON decode failed at sequence {sequence}: {e}"
            ))
        })?;
        return Ok(WalEntry {
            block_height: entry.block_height,
            transition,
        });
    }

    match bincode::deserialize::<LegacyWalEntry>(payload) {
        Ok(entry) => Ok(WalEntry {
            block_height: entry.block_height,
            transition: entry.transition,
        }),
        Err(e) if is_legacy_json_incompatibility(&e) => {
            Err(StorageError::LegacyWalEntryIncompatible {
                sequence,
                error: e.to_string(),
            })
        },
        Err(e) => Err(StorageError::Serialization(format!(
            "WAL entry decode failed at sequence {sequence}: {e}"
        ))),
    }
}

fn is_legacy_json_incompatibility(error: &Box<bincode::ErrorKind>) -> bool {
    error
        .to_string()
        .contains("Bincode does not support the serde::Deserializer::deserialize_any method")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::block::Block;
    use crate::blockchain::chain::Blockchain;
    use crate::blockchain::state::{
        AuditEventKind, AuditEventRecord, ContributionRewardRecord, StateTransition,
    };
    use crate::common::types::{Address, Balance};

    /// #2219: the post-gate tip-commitment check, version-explicit. The
    /// `active()` wrapper is exercised on real boots via `launch_api` main;
    /// here the V2 contract is pinned without touching the process OnceLock.
    #[test]
    fn tip_state_root_commitment_gate_2219() {
        use crate::blockchain::state::State;
        use std::collections::BTreeMap;
        use wrfcoin_shared_protocol::blockchain::block::STATE_ROOT_METADATA_KEY;
        use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;

        let mut state = State::new();
        state.recalculate_state_root();
        let honest_root = state.state_root.to_string();

        let build = |committed: Option<String>, state: State| {
            let mut block = Block::new(1, Default::default(), Vec::new(), 1, 5);
            if let Some(c) = committed {
                block.metadata.insert(STATE_ROOT_METADATA_KEY.to_string(), c);
            }
            let mut blocks = BTreeMap::new();
            blocks.insert(5u64, block);
            Blockchain::from_persisted(5, Default::default(), state, blocks, 5, 0)
        };

        // V2 + matching commitment → starts.
        let chain = build(Some(honest_root.clone()), state.clone());
        verify_tip_state_root_commitment_at(SigningProtocolVersion::V2, &chain)
            .expect("matching tip commitment must pass");

        // V2 + wrong commitment → refuses (the codex-P1 regression case: this
        // must fire when called post-gate, not silently skip).
        let chain = build(Some("ab".repeat(32)), state.clone());
        let err = verify_tip_state_root_commitment_at(SigningProtocolVersion::V2, &chain)
            .expect_err("divergent tip commitment must refuse startup");
        assert!(matches!(
            err,
            StorageCorruptionError::StateRootCommitmentMismatch { height: 5, .. }
        ));

        // V2 + missing commitment → refuses (empty string never equals a root).
        let chain = build(None, state.clone());
        verify_tip_state_root_commitment_at(SigningProtocolVersion::V2, &chain)
            .expect_err("missing tip commitment must refuse at V2");

        // V2 + NO tip block in the window at a non-zero height → refuses
        // (fail-open finding from the gpt review of PR #2357: a missing/
        // truncated BLOCKS_TABLE must not silently skip the commitment check).
        let empty_window =
            Blockchain::from_persisted(5, Default::default(), state.clone(), BTreeMap::new(), 5, 0);
        let err = verify_tip_state_root_commitment_at(SigningProtocolVersion::V2, &empty_window)
            .expect_err("missing tip block at V2 must refuse startup, not skip the check");
        assert!(matches!(
            err,
            StorageCorruptionError::MissingTipBlock { height: 5 }
        ));

        // V1 → no-op regardless.
        let chain = build(None, state);
        verify_tip_state_root_commitment_at(SigningProtocolVersion::V1, &chain)
            .expect("V1 must not require a commitment");
    }

    fn temp_db_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("wrfcoin-storage-test-{}.redb", fastrand::u64(..)))
    }

    fn addr(b: u8) -> Address {
        Address([b; 20])
    }

    fn test_storage_key(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    fn json_bearing_transition() -> StateTransition {
        StateTransition::RecordAuditEvent {
            event: AuditEventRecord {
                event_id: 0,
                block_height: 1,
                timestamp: 1_717_171_717,
                kind: AuditEventKind::GovernanceProposalExecuted,
                subject_id: Some("proposal-7".to_string()),
                actor_id: Some("operator-1".to_string()),
                details: Some(serde_json::json!({
                    "source": "wal-regression",
                    "nested": {
                        "station_id": "station-42",
                        "temperatures": [21.25, 22.5],
                    }
                })),
            },
        }
    }

    fn assert_transition_matches(actual: &StateTransition, expected: &StateTransition) {
        let actual = serde_json::to_value(actual).expect("actual transition should serialize");
        let expected =
            serde_json::to_value(expected).expect("expected transition should serialize");
        assert_eq!(actual, expected);
    }

    /// An NFT mutation transition's deterministic `updated_at` must survive the
    /// WAL v2 encode -> decode round-trip byte-identically (#1968). A replaying
    /// node must read the exact timestamp the producer stamped, or its
    /// state_root forks.
    #[test]
    fn nft_transfer_transition_survives_wal_v2_roundtrip() {
        let entry = WalEntry {
            block_height: 7,
            transition: StateTransition::TransferNFT {
                nft_id: crate::common::types::NFTId::new("nft-1".to_string()),
                from: addr(1),
                to: addr(2),
                updated_at: 1_717_171_717,
            },
        };
        let encoded = encode_wal_entry(&entry).expect("encode should succeed");
        let decoded = decode_wal_entry(&encoded, 7).expect("decode should succeed");
        assert_eq!(decoded.block_height, 7);
        assert_transition_matches(&decoded.transition, &entry.transition);
    }

    #[test]
    fn contribution_reward_transition_survives_wal_v2_roundtrip() {
        let entry = WalEntry {
            block_height: 7,
            transition: StateTransition::ApplyContributionRewardDecision {
                record: ContributionRewardRecord {
                    address: addr(0x46),
                    amount: Balance::new(0),
                    reason: "contribution:wal-provenance-1".to_string(),
                    station_id: "station-wal-synthetic".to_string(),
                    provenance_class:
                        wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Synthetic,
                },
                current_block: 7,
            },
        };
        let encoded = encode_wal_entry(&entry).expect("typed reward WAL encode must succeed");
        let decoded =
            decode_wal_entry(&encoded, 7).expect("typed reward WAL decode must succeed");
        assert_transition_matches(&decoded.transition, &entry.transition);
    }

    #[tokio::test]
    async fn contribution_reward_transition_replays_from_wal_into_canonical_state() {
        let path = temp_db_path();
        let commit_id = "wal-provenance-replay-1";
        let transition = StateTransition::ApplyContributionRewardDecision {
            record: ContributionRewardRecord {
                address: addr(0x47),
                amount: Balance::new(0),
                reason: format!("contribution:{commit_id}"),
                station_id: "station-wal-synthetic".to_string(),
                provenance_class:
                    wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Synthetic,
            },
            current_block: 7,
        };

        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            blockchain
                .apply_state_transition(transition.clone())
                .expect("producer-shaped typed transition applies");
            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            block.set_hash(block.calculate_hash());
            blockchain.add_block(block.clone()).unwrap();
            storage
                .persist_block(
                    block.clone(),
                    vec![transition],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        let storage = Storage::open(&path, 100).unwrap();
        let recovered = storage
            .load_state()
            .unwrap()
            .expect("WAL replay must recover a chain");
        let record = recovered
            .state
            .contribution_reward_records
            .get(commit_id)
            .expect("typed reward record must survive WAL replay");
        assert_eq!(
            record.provenance_class,
            wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Synthetic
        );
        assert!(recovered
            .state
            .finalized_contribution_rewards
            .contains(commit_id));
        assert!(recovered.state.get_account(&record.address).is_none());

        let _ = std::fs::remove_file(path);
    }

    /// Backward compat: a pre-#1968 WAL v2 entry whose transition JSON predates
    /// the `updated_at` field must still decode (defaulting to 0) so old WALs
    /// remain replayable instead of failing closed.
    #[test]
    fn legacy_nft_transfer_wal_entry_without_updated_at_decodes_to_zero() {
        // Hand-build the pre-#1968 transition JSON (no `updated_at` key).
        let legacy_json = serde_json::to_vec(&serde_json::json!({
            "TransferNFT": { "nft_id": "nft-1", "from": addr(1), "to": addr(2) }
        }))
        .unwrap();
        let encoded = {
            let v2 = bincode::serialize(&WalEntryV2 {
                block_height: 9,
                transition_json: legacy_json,
            })
            .unwrap();
            let mut payload = Vec::with_capacity(WAL_V2_MAGIC.len() + v2.len());
            payload.extend_from_slice(WAL_V2_MAGIC);
            payload.extend_from_slice(&v2);
            payload
        };
        let decoded = decode_wal_entry(&encoded, 9).expect("legacy entry should decode");
        match decoded.transition {
            StateTransition::TransferNFT { updated_at, .. } => assert_eq!(updated_at, 0),
            other => panic!("expected TransferNFT, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn persist_and_reload_single_block() {
        let path = temp_db_path();

        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(1);
            blockchain.state.create_account(user, Balance(0));

            let transition = StateTransition::ApplyReward {
                address: user,
                amount: Balance(500),
                reason: "mining".to_string(),
            };
            blockchain
                .apply_state_transition(transition.clone())
                .unwrap();

            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            storage
                .persist_block(
                    block.clone(),
                    vec![transition],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        {
            let storage = Storage::open(&path, 100).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap()
                .expect("should have persisted state");

            assert_eq!(blockchain.height, 1);
            let account = blockchain.state.get_account(&addr(1)).unwrap();
            assert_eq!(account.balance, Balance(500));
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn encrypted_storage_persist_and_reload_single_block() {
        let path = temp_db_path();
        let key = test_storage_key(0x11);

        {
            let mut storage = Storage::open_with_encryption(&path, Some(&key), 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(42);
            blockchain.state.create_account(user, Balance(0));

            let transition = StateTransition::ApplyReward {
                address: user,
                amount: Balance(250),
                reason: "encrypted-mining".to_string(),
            };
            blockchain
                .apply_state_transition(transition.clone())
                .unwrap();

            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            storage
                .persist_block(
                    block.clone(),
                    vec![transition],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        {
            let storage = Storage::open_with_encryption(&path, Some(&key), 100).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap()
                .expect("should have persisted encrypted state");

            assert_eq!(blockchain.height, 1);
            let account = blockchain.state.get_account(&addr(42)).unwrap();
            assert_eq!(account.balance, Balance(250));
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn checkpoint_prunes_wal() {
        // #1053 fix: with `skip_serializing_if` removed, State bincode
        // round-trips correctly, so persist_block_sync can now write a real
        // checkpoint at height 5 (interval=5) and prune WAL entries <= 5.
        // After 6 blocks only the entry at height 6 remains in the WAL.
        //
        // This test verifies:
        //   1. WAL entries are pruned after a successful checkpoint
        //   2. State is fully recoverable from checkpoint + remaining WAL
        //   3. Balance is correct (60 WRF = 6 blocks * 10 each)
        let path = temp_db_path();

        {
            let mut storage = Storage::open(&path, 5).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(2);
            blockchain.state.create_account(user, Balance(0));

            for height in 1..=6 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(10),
                    reason: "block".to_string(),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Verify WAL entries are pruned after the successful checkpoint at height 5.
        // Only the entry at height 6 should remain.
        {
            let db = Database::create(&path).unwrap();
            let read_txn = db.begin_read().unwrap();
            let wal = read_txn.open_table(WAL_TABLE).unwrap();
            assert_eq!(
                wal.len().unwrap(),
                1,
                "Only 1 WAL entry (height 6) should remain after checkpoint at height 5"
            );
        }

        // Verify full state recovery via load_state (checkpoint + WAL replay of height 6).
        {
            let storage = Storage::open(&path, 5).unwrap();
            let blockchain = storage.load_state().unwrap().expect("should recover");

            assert_eq!(blockchain.height, 6, "height must be recovered correctly");

            let account = blockchain
                .state
                .get_account(&addr(2))
                .expect("account must exist after WAL replay");
            assert_eq!(
                account.balance,
                Balance(60),
                "balance must be 60 (6 blocks * 10 WRF) after full WAL replay"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Regression guard: State must survive bincode serialize→deserialize
    /// round-trip for checkpoints to work.  If this test fails, checkpoint
    /// persistence is broken and WAL entries will accumulate indefinitely.
    ///
    /// Root cause of the original failure (#1053): `#[serde(skip_serializing_if)]`
    /// is incompatible with bincode's positional (non-self-describing) format.
    /// Removing those attributes fixed the round-trip.
    #[test]
    fn state_bincode_roundtrip() {
        let state = State::new();
        let bytes = bincode::serialize(&state).expect("serialize");
        let recovered: State =
            bincode::deserialize(&bytes).expect("State bincode round-trip must succeed");
        // Verify key fields survived the round-trip.
        assert_eq!(recovered.oracle_quorum, state.oracle_quorum);
        assert_eq!(recovered.accounts.len(), state.accounts.len());
    }

    /// #2922 sweep: `state_bincode_roundtrip` above uses `State::new()`, whose
    /// `governance.treasury_transactions` is empty — an empty `Vec` can never
    /// exercise a `None`-bearing element's own `skip_serializing_if`, so the
    /// #1053 canary never saw this. `TreasuryTransactionRecord::memo` and
    /// `::proposal_id` still carry `skip_serializing_if` (added after #1053,
    /// never swept for it), and `TreasuryTransactionRecord` is reachable from
    /// the checkpointed `State` via `governance.treasury_transactions`. A
    /// treasury transaction with no memo and no linked proposal is the
    /// ordinary shape (most are), not a rare one — so this is not a
    /// hypothetical: `force_checkpoint_sync`'s `debug_assert!` (see above)
    /// would already be panicking in any debug build that checkpoints one.
    /// RED on current main; must go GREEN once `skip_serializing_if` is
    /// removed from both fields, matching #1053's fix for `State` itself.
    #[test]
    fn state_bincode_roundtrip_survives_a_none_bearing_treasury_transaction() {
        let mut state = State::new();
        state
            .governance
            .treasury_transactions
            .push(wrfcoin_shared_protocol::blockchain::state::TreasuryTransactionRecord {
                tx_type: "withdrawal".to_string(),
                amount: 1_000,
                address: "wrf1test".to_string(),
                block_height: 42,
                memo: None,
                proposal_id: None,
            });

        let bytes = bincode::serialize(&state).expect("serialize");
        let recovered: State = bincode::deserialize(&bytes).unwrap_or_else(|e| {
            panic!(
                "a State whose treasury_transactions carries a None-bearing \
                 record must survive the same checkpoint round-trip #1053 \
                 already fixed for State's own fields: {e}"
            )
        });

        assert_eq!(
            recovered.governance.treasury_transactions.len(),
            state.governance.treasury_transactions.len()
        );
    }

    #[test]
    fn legacy_json_wal_entries_fail_bincode_roundtrip_but_v2_codec_succeeds() {
        let transition = json_bearing_transition();
        let legacy_entry = LegacyWalEntry {
            block_height: 1,
            transition: transition.clone(),
        };
        let legacy_bytes = bincode::serialize(&legacy_entry).expect("legacy wal should serialize");
        let legacy_err = bincode::deserialize::<LegacyWalEntry>(&legacy_bytes)
            .expect_err("legacy wal should fail to deserialize once JSON enters the transition");
        assert!(
            is_legacy_json_incompatibility(&legacy_err),
            "expected deserialize_any legacy failure, got: {legacy_err}"
        );

        let encoded = encode_wal_entry(&WalEntry {
            block_height: 1,
            transition: transition.clone(),
        })
        .expect("v2 wal encoding should succeed");
        let decoded = decode_wal_entry(&encoded, 77).expect("v2 wal decoding should succeed");
        assert_eq!(decoded.block_height, 1);
        assert_transition_matches(&decoded.transition, &transition);
    }

    #[test]
    fn decode_wal_entry_supports_legacy_non_json_entries() {
        let transition = StateTransition::ApplyReward {
            address: addr(9),
            amount: Balance(99),
            reason: "legacy".to_string(),
        };
        let legacy_bytes = bincode::serialize(&LegacyWalEntry {
            block_height: 3,
            transition: transition.clone(),
        })
        .expect("legacy wal should serialize");

        let decoded = decode_wal_entry(&legacy_bytes, 3).expect("legacy wal should decode");
        assert_eq!(decoded.block_height, 3);
        assert_transition_matches(&decoded.transition, &transition);
    }

    #[tokio::test]
    async fn persist_and_reload_json_bearing_transition_from_wal() {
        let path = temp_db_path();

        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let transition = json_bearing_transition();
            blockchain
                .apply_state_transition(transition.clone())
                .expect("json-bearing transition should apply");

            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            storage
                .persist_block(
                    block.clone(),
                    vec![transition],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        {
            let storage = Storage::open(&path, 100).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap()
                .expect("json-bearing wal should reload");

            assert_eq!(blockchain.height, 1);
            assert_eq!(blockchain.state.audit_events.len(), 1);
            assert_eq!(
                blockchain.state.audit_events[0].details,
                Some(serde_json::json!({
                    "source": "wal-regression",
                    "nested": {
                        "station_id": "station-42",
                        "temperatures": [21.25, 22.5],
                    }
                }))
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn encrypted_storage_checkpoint_and_wal_replay_work() {
        let path = temp_db_path();
        let key = test_storage_key(0x22);

        {
            let mut storage = Storage::open_with_encryption(&path, Some(&key), 5).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(43);
            blockchain.state.create_account(user, Balance(0));

            for height in 1..=6 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(10),
                    reason: "encrypted-block".to_string(),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        {
            let storage = Storage::open_with_encryption(&path, Some(&key), 5).unwrap();
            let blockchain = storage.load_state().unwrap().expect("should recover");
            let blocks = storage.load_blocks_range(1, 6).unwrap();

            assert_eq!(blockchain.height, 6);
            assert_eq!(blocks.len(), 6);
            assert_eq!(blocks.last().unwrap().height, 6);
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn encrypted_storage_fails_closed_on_plaintext_db() {
        let path = temp_db_path();
        let key = test_storage_key(0x33);

        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(44);
            blockchain.state.create_account(user, Balance(0));

            let transition = StateTransition::ApplyReward {
                address: user,
                amount: Balance(25),
                reason: "plaintext".to_string(),
            };
            blockchain
                .apply_state_transition(transition.clone())
                .unwrap();

            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            storage
                .persist_block(
                    block.clone(),
                    vec![transition],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        let err = match Storage::open_with_encryption(&path, Some(&key), 100) {
            Ok(_) => panic!("plaintext db should fail closed"),
            Err(err) => err,
        };
        assert!(matches!(err, StorageError::Encryption(_)));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn encrypted_storage_fails_with_wrong_key() {
        let path = temp_db_path();
        let write_key = test_storage_key(0x44);
        let wrong_key = test_storage_key(0x55);

        {
            let mut storage = Storage::open_with_encryption(&path, Some(&write_key), 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(45);
            blockchain.state.create_account(user, Balance(0));

            let transition = StateTransition::ApplyReward {
                address: user,
                amount: Balance(35),
                reason: "encrypted".to_string(),
            };
            blockchain
                .apply_state_transition(transition.clone())
                .unwrap();

            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            storage
                .persist_block(
                    block.clone(),
                    vec![transition],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        let err = match Storage::open_with_encryption(&path, Some(&wrong_key), 100) {
            Ok(_) => panic!("wrong storage key should fail"),
            Err(err) => err,
        };
        assert!(matches!(err, StorageError::Encryption(_)));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn corrupted_wal_entry_detected() {
        let path = temp_db_path();

        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(3);
            blockchain.state.create_account(user, Balance(0));

            let transition = StateTransition::ApplyReward {
                address: user,
                amount: Balance(100),
                reason: "test".to_string(),
            };
            blockchain
                .apply_state_transition(transition.clone())
                .unwrap();

            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            storage
                .persist_block(
                    block.clone(),
                    vec![transition],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        // Corrupt the WAL entry.
        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut wal = write_txn.open_table(WAL_TABLE).unwrap();
                wal.insert(1u64, &[0xFF, 0xFF, 0xFF, 0xFF, 0x00][..])
                    .unwrap();
            }
            write_txn.commit().unwrap();
        }

        {
            let storage = Storage::open(&path, 100).unwrap();
            let result = storage.load_state();
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                err.to_string().contains("checksum mismatch"),
                "Expected checksum error, got: {err}"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    async fn persist_until_checkpoint(path: &std::path::Path, interval: u64, user: Address) {
        let mut storage = Storage::open(path, interval).unwrap();
        let mut blockchain = Blockchain::default();
        blockchain.state.create_account(user, Balance(0));

        for height in 1..=interval {
            let transition = StateTransition::ApplyReward {
                address: user,
                amount: Balance(11),
                reason: "checkpoint-anchor-test".to_string(),
            };
            blockchain
                .apply_state_transition(transition.clone())
                .unwrap();

            let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            storage
                .persist_block(
                    block.clone(),
                    vec![transition],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn missing_latest_checkpoint_fails_closed() {
        let path = temp_db_path();
        let checkpoint_height = 2;

        persist_until_checkpoint(&path, checkpoint_height, addr(60)).await;

        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE).unwrap();
                checkpoints.remove(checkpoint_height).unwrap();
            }
            write_txn.commit().unwrap();
        }

        let storage = Storage::open(&path, checkpoint_height).unwrap();
        let err = storage
            .load_state()
            .expect_err("missing checkpoint anchor must fail closed");
        let err_text = err.to_string();
        match &err {
            StorageError::Corruption(StorageCorruptionError::MissingSnapshot { height }) => {
                assert_eq!(*height, checkpoint_height);
            },
            other => panic!("expected MissingSnapshot, got {other}"),
        }
        assert!(
            err_text.contains("EXIT_MISSING_SNAPSHOT") && err_text.contains("checkpoint")
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn corrupt_latest_checkpoint_fails_closed() {
        let path = temp_db_path();
        let checkpoint_height = 2;

        persist_until_checkpoint(&path, checkpoint_height, addr(61)).await;

        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE).unwrap();
                checkpoints
                    .insert(checkpoint_height, &[0xCA, 0xFE, 0xBA, 0xBE][..])
                    .unwrap();
            }
            write_txn.commit().unwrap();
        }

        let storage = Storage::open(&path, checkpoint_height).unwrap();
        let err = storage
            .load_state()
            .expect_err("corrupt checkpoint anchor must fail closed");
        let err_text = err.to_string();
        // #2881 review: four garbage bytes are too short to be a checkpoint
        // of EITHER format (legacy starts with an 8-byte length, an envelope
        // with a 12-byte header), so this is positively identified as corrupt
        // — not routed to the legacy format-gap error, and never decoded.
        match &err {
            StorageError::Corruption(StorageCorruptionError::SnapshotUnreadable {
                height,
                error,
            }) => {
                assert_eq!(*height, checkpoint_height);
                assert!(error.contains("too short"), "got: {error}");
            },
            other => panic!("expected SnapshotUnreadable, got {other}"),
        }
        assert!(err_text.contains("EXIT_CORRUPT_SNAPSHOT") && err_text.contains("checkpoint"));

        let _ = std::fs::remove_file(&path);
    }

    /// #2881 review, blocker 1: a checkpoint row holding the envelope magic
    /// with a truncated header — what a power loss or killed process leaves
    /// mid-write — must be a NAMED refusal. Before the fix it fell through to
    /// the legacy branch, bincode read the magic itself as the accounts-map
    /// length, and the process SIGABRTed attempting a 4.17 EiB allocation
    /// (`memory allocation of 4176864071625776080 bytes failed`, reproduced).
    /// An abort on a wild allocation is strictly worse than the misleading
    /// refusal #2727 fixed.
    #[tokio::test]
    async fn truncated_envelope_header_is_a_named_refusal_not_an_abort() {
        for truncated_len in [CHECKPOINT_ENVELOPE_MAGIC.len(), CHECKPOINT_ENVELOPE_MAGIC.len() + 2]
        {
            let path = temp_db_path();
            let checkpoint_height = 2;
            persist_until_checkpoint(&path, checkpoint_height, addr(66)).await;
            {
                let db = Database::create(&path).unwrap();
                let write_txn = db.begin_write().unwrap();
                {
                    let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE).unwrap();
                    let mut bytes = CHECKPOINT_ENVELOPE_MAGIC.to_vec();
                    bytes.resize(truncated_len, 0);
                    checkpoints.insert(checkpoint_height, bytes.as_slice()).unwrap();
                }
                write_txn.commit().unwrap();
            }
            let storage = Storage::open(&path, checkpoint_height).unwrap();
            let err = storage
                .load_state()
                .expect_err("truncated envelope header must fail closed");
            match &err {
                StorageError::Corruption(StorageCorruptionError::SnapshotUnreadable {
                    height,
                    error,
                }) => {
                    assert_eq!(*height, checkpoint_height);
                    assert!(
                        error.contains("truncated checkpoint envelope"),
                        "len {truncated_len}: refusal must name the truncation, got: {error}"
                    );
                },
                other => panic!("expected SnapshotUnreadable, got {other}"),
            }
            let _ = std::fs::remove_file(&path);
        }
    }

    /// #2881 review, blocker 1: an unenveloped row whose leading length
    /// prefix exceeds the stored byte count cannot be a checkpoint any binary
    /// wrote. It is refused by the bounds check BEFORE bincode may allocate
    /// for it — the same wild-allocation class as the truncated magic, minus
    /// the magic.
    #[tokio::test]
    async fn implausible_leading_length_prefix_is_refused_before_allocation() {
        let path = temp_db_path();
        let checkpoint_height = 2;
        persist_until_checkpoint(&path, checkpoint_height, addr(67)).await;
        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE).unwrap();
                // 16 bytes claiming u64::MAX leading entries.
                let mut bytes = u64::MAX.to_le_bytes().to_vec();
                bytes.extend_from_slice(&[0u8; 8]);
                checkpoints.insert(checkpoint_height, bytes.as_slice()).unwrap();
            }
            write_txn.commit().unwrap();
        }
        let storage = Storage::open(&path, checkpoint_height).unwrap();
        let err = storage
            .load_state()
            .expect_err("wild leading length prefix must fail closed");
        match &err {
            StorageError::Corruption(StorageCorruptionError::SnapshotUnreadable {
                height,
                error,
            }) => {
                assert_eq!(*height, checkpoint_height);
                assert!(
                    error.contains("exceeds the stored"),
                    "refusal must name the implausible prefix, got: {error}"
                );
            },
            other => panic!("expected SnapshotUnreadable, got {other}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    /// #2727: a checkpoint carrying the envelope (magic + matching version)
    /// whose payload does not decode IS genuine corruption — the version says
    /// the layout matches, so a decode failure means damaged bytes, and the
    /// CORRUPT classification stays accurate for exactly this case.
    #[tokio::test]
    async fn versioned_corrupt_checkpoint_still_reports_corrupt() {
        let path = temp_db_path();
        let checkpoint_height = 2;

        persist_until_checkpoint(&path, checkpoint_height, addr(62)).await;

        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE).unwrap();
                let mut bytes = Vec::new();
                bytes.extend_from_slice(&CHECKPOINT_ENVELOPE_MAGIC);
                bytes.extend_from_slice(&CHECKPOINT_FORMAT_VERSION.to_le_bytes());
                bytes.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);
                checkpoints.insert(checkpoint_height, bytes.as_slice()).unwrap();
            }
            write_txn.commit().unwrap();
        }

        let storage = Storage::open(&path, checkpoint_height).unwrap();
        let err = storage
            .load_state()
            .expect_err("versioned corrupt checkpoint must fail closed");
        match &err {
            StorageError::Corruption(StorageCorruptionError::SnapshotUnreadable {
                height, ..
            }) => assert_eq!(*height, checkpoint_height),
            other => panic!("expected SnapshotUnreadable, got {other}"),
        }
        assert!(err.to_string().contains("EXIT_CORRUPT_SNAPSHOT"));

        let _ = std::fs::remove_file(&path);
    }

    /// #2727 regression, reproduced from the live node container incident without a
    /// node: the committed fixture was serialized by ACTUAL 5007ce747 code
    /// (the last main commit before the 2026-07-01 image build) in a detached
    /// worktree, holding a pending commit accepted at height 47474 — the
    /// shape that produced `invalid value: integer 47474, expected variant
    /// index 0 <= i < 3` on the live node (#2508 inserted
    /// `CommitRecord.provenance_class` before `committed_at`, so the old
    /// height's low 4 bytes land on the new enum discriminant).
    ///
    /// The assertion is the OPERATOR-FACING half: the failure must name the
    /// format gap with recovery options, and must NOT present as CORRUPT —
    /// "corrupt" sent the operator hunting for damaged data that was intact.
    #[tokio::test]
    async fn pre_2508_checkpoint_names_the_format_gap_not_corruption() {
        const JULY_FIXTURE: &[u8] =
            include_bytes!("../tests/fixtures/state_5007ce747_pre2508.bin");
        let path = temp_db_path();
        let checkpoint_height = 2;

        persist_until_checkpoint(&path, checkpoint_height, addr(63)).await;

        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE).unwrap();
                checkpoints.insert(checkpoint_height, JULY_FIXTURE).unwrap();
            }
            write_txn.commit().unwrap();
        }

        let storage = Storage::open(&path, checkpoint_height).unwrap();
        let err = storage
            .load_state()
            .expect_err("pre-#2508 checkpoint must fail closed");
        let err_text = err.to_string();
        match &err {
            StorageError::Corruption(StorageCorruptionError::LegacyCheckpointIncompatible {
                height,
                error,
            }) => {
                assert_eq!(*height, checkpoint_height);
                // The exact node container error signature, reproduced from real
                // old-layout bytes — the proof the root cause is the #2508
                // mid-struct insertion, not damaged data.
                assert!(
                    error.contains("invalid value: integer `47474`")
                        && error.contains("expected variant index 0 <= i < 3"),
                    "decode detail should reproduce the CT 101 signature, got: {error}"
                );
            },
            other => panic!("expected LegacyCheckpointIncompatible, got {other}"),
        }
        assert!(err_text.contains("EXIT_INCOMPATIBLE_CHECKPOINT_FORMAT"));
        assert!(err_text.contains("test_reset_node_from_genesis.sh"));
        assert!(err_text.contains("not damage"));
        assert!(
            !err_text.contains("EXIT_CORRUPT_SNAPSHOT"),
            "a format gap must not present as corruption (#2727)"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// #2727: a versioned checkpoint from a DIFFERENT format version refuses
    /// with both versions named, before any decode is attempted.
    #[tokio::test]
    async fn future_format_version_refuses_with_named_versions() {
        let path = temp_db_path();
        let checkpoint_height = 2;

        persist_until_checkpoint(&path, checkpoint_height, addr(64)).await;

        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE).unwrap();
                let mut bytes = Vec::new();
                bytes.extend_from_slice(&CHECKPOINT_ENVELOPE_MAGIC);
                bytes.extend_from_slice(&99u32.to_le_bytes());
                bytes.extend_from_slice(&[0u8; 16]);
                checkpoints.insert(checkpoint_height, bytes.as_slice()).unwrap();
            }
            write_txn.commit().unwrap();
        }

        let storage = Storage::open(&path, checkpoint_height).unwrap();
        let err = storage
            .load_state()
            .expect_err("format-version mismatch must fail closed");
        match &err {
            StorageError::Corruption(
                StorageCorruptionError::CheckpointFormatVersionMismatch {
                    height,
                    found,
                    supported,
                },
            ) => {
                assert_eq!(*height, checkpoint_height);
                assert_eq!(*found, 99);
                assert_eq!(*supported, CHECKPOINT_FORMAT_VERSION);
            },
            other => panic!("expected CheckpointFormatVersionMismatch, got {other}"),
        }
        assert!(
            err.to_string()
                .contains("EXIT_CHECKPOINT_FORMAT_VERSION_MISMATCH")
        );

        let _ = std::fs::remove_file(&path);
    }

    /// #2727 backward compatibility: a volume written by a SAME-layout binary
    /// from before the envelope existed (raw `bincode(State)`, no magic) must
    /// still load. Simulated by stripping the envelope header off a freshly
    /// written checkpoint — byte-identical to what a pre-envelope build of
    /// this same layout would have persisted. Without this branch, shipping
    /// the envelope would itself recreate #2727 on every deployed volume.
    #[tokio::test]
    async fn legacy_same_layout_checkpoint_still_loads() {
        let path = temp_db_path();
        let checkpoint_height = 2;

        persist_until_checkpoint(&path, checkpoint_height, addr(65)).await;

        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE).unwrap();
                let stored = checkpoints
                    .get(checkpoint_height)
                    .unwrap()
                    .expect("checkpoint row exists")
                    .value()
                    .to_vec();
                assert_eq!(
                    &stored[..CHECKPOINT_ENVELOPE_MAGIC.len()],
                    &CHECKPOINT_ENVELOPE_MAGIC,
                    "fresh checkpoints must carry the envelope"
                );
                let legacy = stored[CHECKPOINT_ENVELOPE_MAGIC.len() + 4..].to_vec();
                checkpoints.insert(checkpoint_height, legacy.as_slice()).unwrap();
            }
            write_txn.commit().unwrap();
        }

        let storage = Storage::open(&path, checkpoint_height).unwrap();
        let loaded = storage
            .load_state()
            .expect("same-layout legacy checkpoint must load")
            .expect("state present");
        assert_eq!(loaded.height, checkpoint_height);

        let _ = std::fs::remove_file(&path);
    }

    fn golden_fixture_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn golden_fixture_path(version: u32) -> std::path::PathBuf {
        golden_fixture_dir().join(format!("checkpoint_state_v{version}_golden.bin"))
    }

    /// #2727 layout tripwire, made non-launderable per the #2881 review
    /// (blocker 2): the fixture for the CURRENT `CHECKPOINT_FORMAT_VERSION`
    /// is committed ONCE and the regenerator refuses to overwrite it, so a
    /// `State`-layout change cannot be silenced by regeneration. The only
    /// green path after a layout change is: bump `CHECKPOINT_FORMAT_VERSION`,
    /// then regenerate (which only writes the ABSENT new version's file).
    /// Every older version's fixture is kept and must refuse with the NAMED
    /// version mismatch — which is exactly what a deployed volume of that era
    /// will experience, so the historical scan is a live rehearsal of the
    /// operator-facing behaviour.
    #[test]
    fn golden_checkpoint_fixture_decodes_with_current_layout() {
        let current = golden_fixture_path(CHECKPOINT_FORMAT_VERSION);
        let bytes = std::fs::read(&current).unwrap_or_else(|e| {
            panic!(
                "no golden fixture for CHECKPOINT_FORMAT_VERSION {CHECKPOINT_FORMAT_VERSION} \
                 ({e}). If you just bumped the version after a State-layout change, run \
                 `cargo test -p wrfcoin-native-chain --lib regenerate_golden_checkpoint_fixture \
                 -- --ignored` to create {current:?} — and commit it plus the version bump \
                 together."
            )
        });
        let state = decode_state_checkpoint(&bytes, 47474)
            .expect("golden fixture must decode with the current layout — if you changed \
                     State's serialized layout, bump CHECKPOINT_FORMAT_VERSION (the \
                     regenerator refuses to overwrite this version's fixture precisely so \
                     the change cannot be silenced)");
        assert_eq!(state.oracle_quorum, 1);
        assert_eq!(state.pending_commits.len(), 1);

        // Historical fixtures: every older version must refuse with the NAMED
        // mismatch, never decode silently and never crash.
        let mut older_seen = 0u32;
        for entry in std::fs::read_dir(golden_fixture_dir()).expect("fixtures dir") {
            let name = entry.expect("dir entry").file_name();
            let name = name.to_string_lossy().into_owned();
            let Some(version) = name
                .strip_prefix("checkpoint_state_v")
                .and_then(|rest| rest.strip_suffix("_golden.bin"))
                .and_then(|v| v.parse::<u32>().ok())
            else {
                continue;
            };
            if version >= CHECKPOINT_FORMAT_VERSION {
                continue;
            }
            older_seen += 1;
            let old_bytes = std::fs::read(golden_fixture_path(version)).expect("old fixture");
            // #3091: an older fixture with a registered migration path now
            // DECODES (through the migration) instead of refusing; one
            // without a path refuses with the NAMED migration gap — never
            // the version mismatch (reserved for newer binaries) and never
            // a silent decode or a crash.
            match decode_state_checkpoint_migrating(&old_bytes, 47474) {
                Ok((state, Some(migrated))) => {
                    assert!(
                        migrations::has_path(version),
                        "v{version} decoded without a registered path"
                    );
                    assert_eq!(migrated.from_version, version);
                    assert_eq!(migrated.height, 47474);
                    assert_eq!(state.oracle_quorum, 1);
                    assert_eq!(state.pending_commits.len(), 1);
                    // The migrated envelope is a current-version checkpoint.
                    let (again, none) = decode_state_checkpoint_migrating(&migrated.bytes, 47474)
                        .expect("migrated bytes decode on the current path");
                    assert!(none.is_none(), "a migrated checkpoint needs no second migration");
                    assert_eq!(again.state_root, state.state_root);
                },
                Ok((state, None)) => {
                    assert_eq!(
                        version, 10,
                        "only the deployed compatible writer avoids rewriting"
                    );
                    assert!(state.station_owner_ledger.is_empty());
                    assert_eq!(encode_state_checkpoint(&state).unwrap(), old_bytes);
                }
                Err(StorageError::Corruption(
                    StorageCorruptionError::CheckpointMigrationUnavailable {
                        found,
                        supported,
                        ..
                    },
                )) => {
                    assert!(
                        !migrations::has_path(version),
                        "v{version} has a registered path but refused"
                    );
                    assert_eq!(found, version);
                    assert_eq!(supported, CHECKPOINT_FORMAT_VERSION);
                },
                other => panic!(
                    "v{version} fixture must migrate or refuse with the named migration gap \
                     under version {CHECKPOINT_FORMAT_VERSION}, got {other:?}"
                ),
            }
        }
        // With the base version (2) there are no older fixtures yet; after the
        // first bump this asserts the historical rehearsal actually ran.
        if CHECKPOINT_FORMAT_VERSION > 2 {
            assert!(
                older_seen > 0,
                "a bumped version must keep at least one older fixture as the \
                 named-mismatch rehearsal"
            );
        }
    }

    #[test]
    fn compatible_migration_preserves_a_multi_account_payload_prefix() {
        let deployed = std::fs::read(golden_fixture_path(10)).unwrap();
        let mut state = decode_state_checkpoint(&deployed, 47474).unwrap();
        for seed in 1..=24 {
            state.create_account(
                wrfcoin_shared_protocol::common::types::Address([seed; 20]),
                Balance(seed as u128),
            );
        }
        assert!(state.station_weather_accepted_block_unix.is_empty());
        let v10 = encode_state_checkpoint(&state).unwrap();
        // The only format10 suffix is the empty accepting-block-time map.
        let v9 = migrations::envelope(9, &v10[12..v10.len() - 8]);
        let (_, pending) = decode_state_checkpoint_migrating(&v9, 47474).unwrap();
        let pending = pending.expect("v9 needs verified migration write-back");
        assert_eq!(pending.from_version, 9);
        assert_eq!(
            pending.bytes, v10,
            "migration must preserve the stored map iteration order"
        );
    }

    #[test]
    fn owner_ledger_empty_writer_is_byte_identical_to_deployed_v10() {
        let deployed = std::fs::read(golden_fixture_path(10)).unwrap();
        let (mut state, pending) = decode_state_checkpoint_migrating(&deployed, 47474).unwrap();
        assert!(
            pending.is_none(),
            "a compatible deployed row needs no rewrite"
        );
        let root = state.state_root_material();
        assert_eq!(encode_state_checkpoint(&state).unwrap(), deployed);
        let legacy = decode_state_checkpoint(&deployed[12..], 47474).unwrap();
        assert_eq!(legacy.state_root_material(), root);
        assert_eq!(encode_state_checkpoint(&legacy).unwrap(), deployed);
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(&dir.path().join("compatible-owner.redb"), 1).unwrap();
        assert_eq!(
            storage
                .checkpoint_state_bytes(47474, &state)
                .unwrap()
                .unwrap(),
            deployed
        );
        state.station_owner_ledger.insert(
            "station-owner-fixture".into(),
            wrfcoin_shared_protocol::blockchain::station_owner::StationOwnerLedgerEntry {
                owner_nonce: 7,
                frozen: Some(
                    wrfcoin_shared_protocol::blockchain::station_owner::StationFreeze {
                        since_height: 47474,
                    },
                ),
                ..Default::default()
            },
        );
        let encoded = encode_state_checkpoint(&state).unwrap();
        assert_eq!(u32::from_le_bytes(encoded[8..12].try_into().unwrap()), 11);
        assert_eq!(&encoded[12..deployed.len()], &deployed[12..]);
        assert_ne!(state.state_root_material(), root);
        let (restored, pending) = decode_state_checkpoint_migrating(&encoded, 47474).unwrap();
        assert!(pending.is_none());
        assert_eq!(restored.station_owner_ledger, state.station_owner_ledger);
        assert_eq!(restored.state_root_material(), state.state_root_material());
        assert_eq!(encode_state_checkpoint(&restored).unwrap(), encoded);
    }

    /// Regenerator for the golden fixture. **Refuses to overwrite the current
    /// version's fixture** (#2881 review, blocker 2): regenerating in place
    /// after a layout change would silence the tripwire without the
    /// `CHECKPOINT_FORMAT_VERSION` bump that makes deployed volumes refuse
    /// with a named gap. The only way to a green tripwire after a layout
    /// change is bump-then-regenerate:
    /// `cargo test -p wrfcoin-native-chain --lib \
    ///   regenerate_golden_checkpoint_fixture -- --ignored`
    #[test]
    #[ignore = "writes the CURRENT version's golden fixture, and only if absent; bump CHECKPOINT_FORMAT_VERSION first after a layout change"]
    fn regenerate_golden_checkpoint_fixture() {
        let path = golden_fixture_path(CHECKPOINT_FORMAT_VERSION);
        assert!(
            !path.exists(),
            "REFUSING to overwrite {path:?}: the v{CHECKPOINT_FORMAT_VERSION} golden \
             already exists. If you changed State's serialized layout, bump \
             CHECKPOINT_FORMAT_VERSION first — regenerating in place would silence \
             the layout tripwire without the version bump (#2881 review, blocker 2). \
             If you truly need to rebuild THIS version's fixture byte-for-byte \
             unchanged-layout (e.g. repo surgery), delete the file manually and rerun."
        );
        let mut state = State::new();
        state.pending_commits.insert(
            "golden-commit".to_string(),
            wrfcoin_shared_protocol::contributions::commit_reveal::CommitRecord {
                commit_id: "golden-commit".to_string(),
                station_id: "golden-station".to_string(),
                contribution_type: "weather_data".to_string(),
                provenance_class:
                    wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Real,
                committed_at: 47474,
                commit_expires_at: 47484,
                reveal_expires_at: 47494,
            },
        );
        state.oracle_set.insert("golden-oracle".to_string());
        state.oracle_quorum = 1;
        let bytes = migrations::envelope(
            CHECKPOINT_FORMAT_VERSION,
            &bincode::serialize(&state).unwrap(),
        );
        std::fs::write(&path, &bytes).expect("write golden fixture");
        eprintln!("wrote {} bytes to {path:?}", bytes.len());
    }

    #[test]
    fn malformed_wal_sequence_meta_length_detected_on_open() {
        let path = temp_db_path();

        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut meta = write_txn.open_table(META_TABLE).unwrap();
                meta.insert(META_WAL_SEQ, &[0xAA, 0xBB, 0xCC][..]).unwrap();
            }
            write_txn.commit().unwrap();
        }

        let err = match Storage::open(&path, 100) {
            Ok(_) => panic!("invalid wal_sequence length should fail"),
            Err(err) => err,
        };
        match err {
            StorageError::Corruption(StorageCorruptionError::MetaLengthInvalid {
                key,
                expected,
                actual,
            }) => {
                assert_eq!(key, META_WAL_SEQ);
                assert_eq!(expected, 8);
                assert_eq!(actual, 3);
            },
            other => panic!("expected MetaLengthInvalid, got {other}"),
        }

        let _ = std::fs::remove_file(&path);
    }

    /// #3301: boot refuses "archive empty but a height recorded" BEFORE
    /// loading. The detector is read-only and distinguishes the three shapes.
    #[tokio::test]
    async fn archive_empty_but_height_recorded_detects_only_the_inconsistent_shape() {
        // Fresh volume: consistent (a genuinely uncut chain).
        let fresh = temp_db_path();
        let storage = Storage::open(&fresh, 100).expect("open fresh");
        assert!(!storage.archive_empty_but_height_recorded().unwrap());
        assert!(storage.load_state().unwrap().is_none());
        drop(storage);
        let _ = std::fs::remove_file(&fresh);

        // A normal persisted chain: consistent.
        let normal = temp_db_path();
        {
            let mut storage = Storage::open(&normal, 100).expect("open");
            let mut block = Block::new(1, Hash::default(), vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            storage.persist_block(block, vec![], None).await.expect("persist");
            assert!(!storage.archive_empty_but_height_recorded().unwrap());
        }
        let _ = std::fs::remove_file(&normal);

        // Inconsistent: a recorded height and no block. `load_state` would
        // load a chain from this, which is exactly why boot must refuse first.
        let broken = temp_db_path();
        {
            let db = Database::create(&broken).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut meta = write_txn.open_table(META_TABLE).unwrap();
                meta.insert(META_HEIGHT, &7u64.to_le_bytes()[..]).unwrap();
                let _blocks = write_txn.open_table(BLOCKS_TABLE).unwrap();
            }
            write_txn.commit().unwrap();
        }
        let storage = Storage::open(&broken, 100).expect("open broken");
        assert!(storage.archive_empty_but_height_recorded().unwrap());
        assert!(
            storage.load_lowest_block().unwrap().is_none(),
            "the gate alone would call this chain uncut"
        );
        drop(storage);
        let _ = std::fs::remove_file(&broken);
    }

    #[test]
    fn malformed_height_meta_length_detected_on_load() {
        let path = temp_db_path();

        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut meta = write_txn.open_table(META_TABLE).unwrap();
                meta.insert(META_HEIGHT, &[0x01, 0x02, 0x03, 0x04][..])
                    .unwrap();
            }
            write_txn.commit().unwrap();
        }

        let storage = Storage::open(&path, 100).expect("storage should open");
        let err = storage
            .load_state()
            .expect_err("invalid height length should fail");
        match err {
            StorageError::Corruption(StorageCorruptionError::MetaLengthInvalid {
                key,
                expected,
                actual,
            }) => {
                assert_eq!(key, META_HEIGHT);
                assert_eq!(expected, 8);
                assert_eq!(actual, 4);
            },
            other => panic!("expected MetaLengthInvalid, got {other}"),
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn malformed_wal_prefix_length_detected() {
        let path = temp_db_path();

        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(4);
            blockchain.state.create_account(user, Balance(0));

            let transition = StateTransition::ApplyReward {
                address: user,
                amount: Balance(100),
                reason: "test".to_string(),
            };
            blockchain
                .apply_state_transition(transition.clone())
                .unwrap();

            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            storage
                .persist_block(
                    block.clone(),
                    vec![transition],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut wal = write_txn.open_table(WAL_TABLE).unwrap();
                wal.insert(1u64, &[0x01, 0x02, 0x03, 0x04][..]).unwrap();
            }
            write_txn.commit().unwrap();
        }

        let storage = Storage::open(&path, 100).unwrap();
        let err = storage
            .load_state()
            .expect_err("short wal prefix should fail");
        match err {
            StorageError::Corruption(StorageCorruptionError::WalPrefixInvalid {
                sequence,
                actual_len,
            }) => {
                assert_eq!(sequence, 1);
                assert_eq!(actual_len, 4);
            },
            other => panic!("expected WalPrefixInvalid, got {other}"),
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_database_returns_none() {
        let path = temp_db_path();
        let storage = Storage::open(&path, 100).unwrap();
        let result = storage.load_state().unwrap();
        assert!(result.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corruption_error_is_serializable_with_stable_code() {
        let err = StorageCorruptionError::WalPrefixInvalid {
            sequence: 9,
            actual_len: 3,
        };
        let encoded = serde_json::to_string(&err).expect("corruption error should serialize");
        let decoded: StorageCorruptionError =
            serde_json::from_str(&encoded).expect("corruption error should deserialize");
        assert_eq!(decoded, err);
        assert_eq!(decoded.error_code(), "NC_STORAGE_WAL_PREFIX_INVALID");
    }

    #[tokio::test]
    async fn force_checkpoint_writes_and_prunes() {
        let path = temp_db_path();

        {
            // Use a large interval so no automatic checkpoint fires.
            let mut storage = Storage::open(&path, 1000).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(5);
            blockchain.state.create_account(user, Balance(0));

            for height in 1..=3 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(10),
                    reason: "block".to_string(),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }

            // Force a shutdown checkpoint at height 3.
            storage
                .force_checkpoint(&blockchain.state, 3)
                .await
                .unwrap();
        }

        // Reopen and verify recovery without WAL replay.
        {
            let storage = Storage::open(&path, 1000).unwrap();
            let blockchain = storage.load_state().unwrap().expect("should recover");
            assert_eq!(blockchain.height, 3);
            let account = blockchain.state.get_account(&addr(5)).unwrap();
            assert_eq!(account.balance, Balance(30));
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn force_checkpoint_noop_at_height_zero() {
        let path = temp_db_path();
        let storage = Storage::open(&path, 100).unwrap();
        let state = State::new();
        // Should succeed without error (no-op).
        storage.force_checkpoint(&state, 0).await.unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn multiple_transitions_per_block() {
        let path = temp_db_path();

        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let alice = addr(10);
            let bob = addr(11);
            blockchain.state.create_account(alice, Balance(0));
            blockchain.state.create_account(bob, Balance(0));

            // Use tracked transitions so the WAL captures the full balance history.
            let t0 = StateTransition::ApplyReward {
                address: alice,
                amount: Balance(1000),
                reason: "initial".to_string(),
            };
            let t1 = StateTransition::ApplyReward {
                address: alice,
                amount: Balance(200),
                reason: "mining".to_string(),
            };
            let t2 = StateTransition::BalanceTransfer {
                from: alice,
                to: bob,
                amount: Balance(50),
                nonce: Some(0),
                current_block: 0,
            };
            blockchain.apply_state_transition(t0.clone()).unwrap();
            blockchain.apply_state_transition(t1.clone()).unwrap();
            blockchain.apply_state_transition(t2.clone()).unwrap();

            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            storage
                .persist_block(
                    block.clone(),
                    vec![t0, t1, t2],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        {
            let storage = Storage::open(&path, 100).unwrap();
            let blockchain = storage.load_state().unwrap().expect("should recover");

            assert_eq!(blockchain.height, 1);
            let alice_account = blockchain.state.get_account(&addr(10)).unwrap();
            let bob_account = blockchain.state.get_account(&addr(11)).unwrap();
            assert_eq!(alice_account.balance, Balance(1150)); // 1000 + 200 - 50
            assert_eq!(bob_account.balance, Balance(50));
        }

        let _ = std::fs::remove_file(&path);
    }

    // ------------------------------------------------------------------
    // Chain reorganization storage tests (#820)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn rollback_to_height_removes_blocks_and_wal() {
        let path = temp_db_path();

        {
            let mut storage = Storage::open(&path, 1000).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(20);
            blockchain.state.create_account(user, Balance(0));

            // Persist 10 blocks.
            for height in 1..=10u64 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(10),
                    reason: "block".to_string(),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }

            // Rollback to height 5.
            let block5 = blockchain.block_at_height(5).unwrap();
            let tip5 = block5.hash.unwrap_or_else(|| block5.calculate_hash());
            storage.rollback_to_height(5, tip5).await.unwrap();
        }

        // Reopen and verify.
        {
            let storage = Storage::open(&path, 1000).unwrap();

            // Blocks 6-10 should be gone.
            let blocks = storage.load_blocks_range(1, 10).unwrap();
            assert_eq!(blocks.len(), 5, "Should only have blocks 1-5");
            assert_eq!(blocks.last().unwrap().height, 5);

            // WAL entries for blocks 6-10 should be gone too.
            let wal = storage.load_wal_transitions_range(6, 10).unwrap();
            assert!(wal.is_empty(), "WAL for heights 6-10 should be pruned");

            // WAL entries for blocks 1-5 should still exist.
            let wal_early = storage.load_wal_transitions_range(1, 5).unwrap();
            assert!(!wal_early.is_empty(), "WAL for heights 1-5 should remain");
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn load_checkpoint_at_or_before_finds_nearest() {
        let path = temp_db_path();

        {
            // Checkpoint every 5 blocks.
            let mut storage = Storage::open(&path, 5).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(21);
            blockchain.state.create_account(user, Balance(0));

            for height in 1..=12u64 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(10),
                    reason: "block".to_string(),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Reopen and query checkpoints.
        // Note: persist_block_sync keeps only the latest checkpoint,
        // so only the checkpoint at height 10 should survive.
        {
            let storage = Storage::open(&path, 5).unwrap();

            // Query for height 12 — should find the checkpoint at height 10.
            let cp = storage.load_checkpoint_at_or_before(12).unwrap();
            let (cp_height, state) =
                cp.expect("checkpoint must exist now that bincode round-trip works (#1053 fix)");

            // Verify checkpoint state is consistent.
            let account = state.get_account(&addr(21)).unwrap();
            assert_eq!(
                account.balance,
                Balance(u128::from(cp_height) * 10),
                "Checkpoint state at height {} should have {} balance",
                cp_height,
                cp_height * 10,
            );

            // Query before the checkpoint should return None.
            let cp_before = storage
                .load_checkpoint_at_or_before(cp_height.saturating_sub(1))
                .unwrap();
            assert!(
                cp_before.is_none(),
                "No checkpoint should exist before height {}",
                cp_height
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn load_blocks_range_returns_correct_blocks() {
        let path = temp_db_path();

        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(22);
            blockchain.state.create_account(user, Balance(0));

            for height in 1..=10u64 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(5),
                    reason: "test".to_string(),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        {
            let storage = Storage::open(&path, 100).unwrap();
            let blocks = storage.load_blocks_range(3, 7).unwrap();
            assert_eq!(blocks.len(), 5);
            assert_eq!(blocks.first().unwrap().height, 3);
            assert_eq!(blocks.last().unwrap().height, 7);
        }

        let _ = std::fs::remove_file(&path);
    }

    // Regression test for #1714: the genesis/chain-identity block is the lowest
    // persisted block (height 1 on this chain). `load_lowest_block` must return
    // it from the durable archive even though it has been evicted from the
    // in-memory window, so `/network/info` can report a stable `genesis_hash`.
    #[tokio::test]
    async fn load_lowest_block_returns_genesis_block() {
        let path = temp_db_path();

        // Empty archive: no lowest block.
        {
            let storage = Storage::open(&path, 100).unwrap();
            assert!(storage.load_lowest_block().unwrap().is_none());
        }

        let mut genesis_hash = None;
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(23);
            blockchain.state.create_account(user, Balance(0));

            // Persist the height-1 genesis block first, then later blocks.
            for height in 1..=10u64 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(5),
                    reason: "test".to_string(),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let prev = if height == 1 {
                    Hash::default()
                } else {
                    blockchain.tip
                };
                let mut block = Block::new(3, prev, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                if height == 1 {
                    genesis_hash = Some(hash);
                }
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        {
            let storage = Storage::open(&path, 100).unwrap();
            let lowest = storage
                .load_lowest_block()
                .unwrap()
                .expect("genesis block must be persisted");
            assert_eq!(lowest.height, 1, "lowest persisted block is the genesis");
            assert_eq!(lowest.prev_block_hash, Hash::default());
            assert_eq!(lowest.hash, genesis_hash);
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn load_blocks_range_preserves_persisted_timestamps() {
        use chrono::TimeZone;

        let path = temp_db_path();
        let first_timestamp = chrono::Utc
            .timestamp_millis_opt(1_700_000_000_123)
            .single()
            .expect("fixture timestamp should be valid");
        let second_timestamp = first_timestamp + chrono::Duration::seconds(1);

        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(23);
            blockchain.state.create_account(user, Balance(0));

            for (height, timestamp) in [(1u64, first_timestamp), (2u64, second_timestamp)] {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(5),
                    reason: "timestamp-roundtrip".to_string(),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                block.timestamp = timestamp;
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        {
            let storage = Storage::open(&path, 100).unwrap();
            let blocks = storage.load_blocks_range(1, 2).unwrap();
            assert_eq!(blocks.len(), 2);
            assert_eq!(blocks[0].timestamp, first_timestamp);
            assert_eq!(blocks[1].timestamp, second_timestamp);
            assert!(
                blocks[1].timestamp > blocks[0].timestamp,
                "persisted block timestamps should remain strictly increasing after round-trip"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    // ------------------------------------------------------------------
    // Fix #870 regression tests
    // ------------------------------------------------------------------

    /// Verifies that a checkpoint IS created now that bincode round-trip
    /// works (#1053 fix), and that META_LAST_CHECKPOINT is set correctly.
    #[tokio::test]
    async fn checkpoint_created_after_roundtrip_fix() {
        let path = temp_db_path();
        let interval = 5u64;

        {
            let mut storage = Storage::open(&path, interval).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(30);
            blockchain.state.create_account(user, Balance(0));

            for height in 1..=interval {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(7),
                    reason: "fix870-a".to_string(),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Now that bincode round-trip works, a checkpoint row SHOULD exist
        // and WAL entries should be pruned.
        {
            let db = Database::create(&path).unwrap();
            let read_txn = db.begin_read().unwrap();

            let checkpoints = read_txn.open_table(CHECKPOINTS_TABLE).unwrap();
            let cp_entry = checkpoints.get(interval).unwrap();
            assert!(
                cp_entry.is_some(),
                "checkpoint row must exist now that bincode round-trip works"
            );

            let meta = read_txn.open_table(META_TABLE).unwrap();
            let last_cp = meta.get(META_LAST_CHECKPOINT).unwrap();
            assert!(
                last_cp.is_some(),
                "META_LAST_CHECKPOINT must be set after successful checkpoint"
            );

            // WAL should be empty — all entries pruned after checkpoint.
            let wal = read_txn.open_table(WAL_TABLE).unwrap();
            assert_eq!(
                wal.len().unwrap(),
                0,
                "WAL entries must be pruned after successful checkpoint"
            );
        }

        // State is recoverable from checkpoint (no WAL replay needed).
        {
            let storage = Storage::open(&path, interval).unwrap();
            let blockchain = storage.load_state().unwrap().expect("should recover");
            assert_eq!(blockchain.height, interval);
            let account = blockchain
                .state
                .get_account(&addr(30))
                .expect("account must exist after checkpoint load");
            assert_eq!(
                account.balance,
                Balance(7 * interval as u128),
                "balance must be {} after checkpoint load",
                7 * interval
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Verifies WAL entries are pruned after a successful checkpoint now
    /// that bincode round-trip works (#1053 fix).
    #[tokio::test]
    async fn wal_pruned_after_successful_checkpoint() {
        let path = temp_db_path();
        let interval = 4u64;

        {
            let mut storage = Storage::open(&path, interval).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(31);
            blockchain.state.create_account(user, Balance(0));

            for height in 1..=interval {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(3),
                    reason: "fix870-b".to_string(),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // WAL should be empty — checkpoint at height 4 pruned all entries.
        {
            let db = Database::create(&path).unwrap();
            let read_txn = db.begin_read().unwrap();
            let wal = read_txn.open_table(WAL_TABLE).unwrap();
            assert_eq!(
                wal.len().unwrap(),
                0,
                "WAL entries must be pruned after successful checkpoint"
            );
        }

        // State is fully recoverable from checkpoint.
        {
            let storage = Storage::open(&path, interval).unwrap();
            let blockchain = storage.load_state().unwrap().expect("should recover");
            assert_eq!(blockchain.height, interval);
            let account = blockchain
                .state
                .get_account(&addr(31))
                .expect("account must exist after checkpoint load");
            assert_eq!(
                account.balance,
                Balance(3 * interval as u128),
                "balance must be {} after checkpoint load",
                3 * interval
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Regression test for issue #1275: `finalized_height` must survive a
    /// node restart. Before the fix, `Blockchain::from_persisted` hardcoded
    /// `finalized_height: 0`, so every reopen silently disarmed the
    /// `plan_reorg` BFT finality guard and permitted reorgs of
    /// already-finalized blocks.
    #[tokio::test]
    async fn finalized_height_survives_restart() {
        let path = temp_db_path();

        // Phase 1: persist a block and persist a finalized height.
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(7);
            blockchain.state.create_account(user, Balance(0));

            let transition = StateTransition::ApplyReward {
                address: user,
                amount: Balance(1),
                reason: "finalized-height-test".to_string(),
            };
            blockchain
                .apply_state_transition(transition.clone())
                .unwrap();

            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            // Advance the in-memory BFT finalized height to a non-zero
            // value. This simulates the finality-signal consumer firing.
            blockchain.set_finalized_height(42);
            assert_eq!(blockchain.finalized_height(), 42);

            // Persist the block *and* the finalized height through the
            // new persist_block_with_finalized path (issue #1275).
            storage
                .persist_block_with_finalized(
                    block.clone(),
                    vec![transition],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                    blockchain.finalized_height(),
                )
                .await
                .unwrap();
        }

        // Phase 2: reopen storage — loader must restore finalized_height.
        {
            let storage = Storage::open(&path, 100).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap()
                .expect("storage must have persisted state");

            assert_eq!(
                blockchain.finalized_height(),
                42,
                "finalized_height must survive restart (#1275): reopening \
                 must not silently reset it to 0 and rearm reorgs of \
                 already-finalized blocks"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    // ─────────────────────────────────────────────────────────────────────
    // D5 — Storage-fault-injection tests (#1627)
    //
    // The WAL is the authoritative record of every state transition.  Any
    // byte-level corruption must cause `load_state` to return `Err` rather
    // than silently building on partial or incorrect state.
    //
    // The existing single-entry tests (`corrupted_wal_entry_detected`,
    // `malformed_wal_prefix_length_detected`) cover the trivial 1-block case.
    // The test below covers the gap: **mid-sequence corruption** — entries
    // before the fault are valid, but the fault must still prevent `load_state`
    // from returning an `Ok` chain built on only the good prefix entries.
    // ─────────────────────────────────────────────────────────────────────

    /// A CRC32 mismatch in a mid-sequence WAL entry (entry 2 of 3) must cause
    /// `load_state` to return `Err(ChecksumMismatch)` — not `Ok` with a chain
    /// at height 1 built from the two entries that were read before the fault.
    ///
    /// Regression target: if the WAL replay loop is refactored to `break` or
    /// skip on corruption instead of `return Err`, this test catches it.
    /// The storage layer must enforce an all-or-nothing contract: a partially
    /// corrupted WAL is just as fatal as a fully corrupted one.
    #[tokio::test]
    async fn load_state_fails_closed_on_mid_sequence_wal_checksum_corruption() {
        let path = temp_db_path();

        // Phase 1: persist 3 blocks → 3 WAL entries (seq 1, 2, 3).
        // checkpoint_interval=100 keeps all WAL entries alive after persist.
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xD5);
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=3 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(10),
                    reason: format!("d5-mid-seq-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Phase 2: directly corrupt WAL entry at sequence 2 (the middle one)
        // by flipping the first byte of its 4-byte CRC prefix.
        // WAL format: [4 bytes CRC32 LE][payload…]
        {
            let db = redb::Database::create(&path).unwrap();
            let read_txn = db.begin_read().unwrap();
            let wal = read_txn.open_table(WAL_TABLE).unwrap();
            assert_eq!(
                wal.len().unwrap(),
                3,
                "expect 3 WAL entries before fault injection"
            );

            // Collect all entries so we can mutate sequence 2 without
            // holding a reference into the read transaction.
            let entries: Vec<(u64, Vec<u8>)> = wal
                .iter()
                .unwrap()
                .map(|r| {
                    let (k, v) = r.unwrap();
                    (k.value(), v.value().to_vec())
                })
                .collect();
            drop(wal);
            drop(read_txn);

            // Find and corrupt sequence 2.
            let write_txn = db.begin_write().unwrap();
            {
                let mut wal_w = write_txn.open_table(WAL_TABLE).unwrap();
                for (seq, mut bytes) in entries {
                    if seq == 2 {
                        assert!(bytes.len() >= 5, "entry 2 must have at least 5 bytes");
                        bytes[0] ^= 0xFF; // flip first byte of CRC
                    }
                    wal_w.insert(seq, bytes.as_slice()).unwrap();
                }
            }
            write_txn.commit().unwrap();
        }

        // Phase 3: reopen storage.  `load_state` replays WAL entries in
        // sequence order; entry 1 applies cleanly, then entry 2 triggers the
        // CRC guard.  The function must return `Err` — NOT `Ok(chain at h=1)`
        // built from the entries processed before the fault.
        {
            let storage = Storage::open(&path, 100).unwrap();
            let result = storage.load_state();
            assert!(
                result.is_err(),
                "load_state must return Err on mid-sequence WAL corruption, \
                 not Ok with a partial chain built from pre-fault entries"
            );
            assert!(
                matches!(
                    result.unwrap_err(),
                    StorageError::ChecksumMismatch { sequence: 2, .. }
                ),
                "error must identify the corrupted sequence number"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Regression test for the `load_wal_transitions_range` CRC gap.
    ///
    /// Before this fix `_crc` was silently discarded, so a bit-flip in a WAL
    /// entry used during reorg fork-state reconstruction would produce wrong
    /// fork state without error.  After the fix, `load_wal_transitions_range`
    /// must return `ChecksumMismatch` — same fail-closed contract as
    /// `load_state`.
    #[tokio::test]
    async fn load_wal_transitions_range_fails_closed_on_checksum_corruption() {
        let path = temp_db_path();

        // Phase 1: persist 3 blocks.  checkpoint_interval=100 keeps all WAL
        // entries alive so the corruption test can reach them.
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xE7);
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=3 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(5),
                    reason: format!("e7-range-crc-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Phase 2: corrupt WAL sequence 2 by flipping the first byte of its
        // 4-byte CRC prefix, identical to the load_state corruption test.
        {
            let db = redb::Database::create(&path).unwrap();
            let read_txn = db.begin_read().unwrap();
            let wal = read_txn.open_table(WAL_TABLE).unwrap();
            assert_eq!(
                wal.len().unwrap(),
                3,
                "expect 3 WAL entries before fault injection"
            );

            let entries: Vec<(u64, Vec<u8>)> = wal
                .iter()
                .unwrap()
                .map(|r| {
                    let (k, v) = r.unwrap();
                    (k.value(), v.value().to_vec())
                })
                .collect();
            drop(wal);
            drop(read_txn);

            let write_txn = db.begin_write().unwrap();
            {
                let mut wal_w = write_txn.open_table(WAL_TABLE).unwrap();
                for (seq, mut bytes) in entries {
                    if seq == 2 {
                        assert!(bytes.len() >= 5, "entry 2 must have at least 5 bytes");
                        bytes[0] ^= 0xFF; // flip first byte of CRC prefix
                    }
                    wal_w.insert(seq, bytes.as_slice()).unwrap();
                }
            }
            write_txn.commit().unwrap();
        }

        // Phase 3: call load_wal_transitions_range spanning the corrupted
        // entry.  Before the fix this returned Ok (CRC was discarded); after
        // the fix it must return Err(ChecksumMismatch { sequence: 2, .. }).
        {
            let storage = Storage::open(&path, 100).unwrap();
            let result = storage.load_wal_transitions_range(1, 3);
            assert!(
                result.is_err(),
                "load_wal_transitions_range must return Err on WAL CRC corruption, \
                 not Ok with silently-corrupted fork state"
            );
            assert!(
                matches!(
                    result.unwrap_err(),
                    StorageError::ChecksumMismatch { sequence: 2, .. }
                ),
                "error must identify the corrupted sequence number"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// D4 storage-fault follow-up — issue #1604.
    ///
    /// If META_LAST_CHECKPOINT points to a checkpoint height but that
    /// checkpoint row is missing, load_state must fail closed. Replaying
    /// WAL entries strictly after that height cannot reconstruct the
    /// authoritative pre-checkpoint state.
    #[tokio::test]
    async fn load_state_fails_closed_when_checkpoint_meta_points_to_missing_row() {
        let path = temp_db_path();

        // Phase 1: persist two blocks with checkpoint interval=2 so a
        // checkpoint is written at height 2 and WAL <=2 is pruned.
        {
            let mut storage = Storage::open(&path, 2).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xD4);
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=2 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(10),
                    reason: format!("d4-missing-checkpoint-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Phase 2: fault injection — delete checkpoint row at height 2
        // but keep META_LAST_CHECKPOINT=2 intact.
        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut checkpoints = write_txn.open_table(CHECKPOINTS_TABLE).unwrap();
                checkpoints.remove(2u64).unwrap();
            }
            write_txn.commit().unwrap();
        }

        // Phase 3: reopen — must fail closed, not return Ok with a state
        // rebuilt from an impossible replay window.
        {
            let storage = Storage::open(&path, 2).unwrap();
            let err = storage
                .load_state()
                .expect_err("missing checkpoint row referenced by metadata must fail closed");
            assert!(
                matches!(
                    err,
                    StorageError::Corruption(StorageCorruptionError::MissingSnapshot { height: 2 })
                ),
                "error must surface as MissingSnapshot for the missing checkpoint row, got {err}"
            );
            assert!(
                err.to_string().contains("EXIT_MISSING_SNAPSHOT")
                    && err.to_string().contains("height 2"),
                "error must identify the missing checkpoint height"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Abrupt-restart slice — issue #1605.
    ///
    /// Simulates a kill mid-write that leaves the tail WAL entry shorter than
    /// the minimum valid prefix (< 5 bytes).  `load_state` must fail closed
    /// rather than silently replaying a partial chain.
    #[tokio::test]
    async fn wal_replay_truncated_tail_below_min_length_fails_closed() {
        let path = temp_db_path();

        // Phase 1: persist 3 clean blocks → WAL seq 1, 2, 3.
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xA1);
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=3 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(10),
                    reason: format!("abrupt-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Phase 2: replace seq 3 with 3 bytes — simulates a truncated write
        // that left fewer bytes than the minimum WAL prefix (4-byte CRC + at
        // least 1 byte of payload = 5 bytes minimum).
        {
            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut wal = write_txn.open_table(WAL_TABLE).unwrap();
                wal.insert(3u64, &[0xDE, 0xAD, 0xBE][..]).unwrap();
            }
            write_txn.commit().unwrap();
        }

        // Phase 3: reopen — load_state must return WalPrefixInvalid for seq 3,
        // not Ok with a partial chain at height 2.
        {
            let storage = Storage::open(&path, 100).unwrap();
            let result = storage.load_state();
            assert!(
                result.is_err(),
                "load_state must fail closed on truncated tail WAL entry, \
                 not silently return a partial chain"
            );
            match result.unwrap_err() {
                StorageError::Corruption(StorageCorruptionError::WalPrefixInvalid {
                    sequence,
                    actual_len,
                }) => {
                    assert_eq!(
                        sequence, 3,
                        "error must identify the truncated tail sequence"
                    );
                    assert_eq!(actual_len, 3, "error must report the actual byte count");
                },
                other => panic!("expected WalPrefixInvalid, got: {other}"),
            }
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Abrupt-restart slice — issue #1605.
    ///
    /// Simulates a kill mid-write that left a valid CRC + magic header but a
    /// truncated bincode body (partial payload write completed before the
    /// process was killed).  The CRC check passes because the CRC was computed
    /// over the truncated content; decode must then fail with a Serialization
    /// error.  `load_state` must fail closed rather than returning a partial
    /// chain.
    #[tokio::test]
    async fn wal_replay_truncated_tail_valid_magic_fails_closed() {
        let path = temp_db_path();

        // Phase 1: persist 3 clean blocks → WAL seq 1, 2, 3.
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xA2);
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=3 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(10),
                    reason: format!("abrupt-magic-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Phase 2: replace seq 3 with [valid CRC32][WAL_V2_MAGIC only] — the
        // magic was written but the bincode body was not.  CRC is computed over
        // the magic-only payload so the CRC guard is satisfied; the failure
        // must come from bincode decode.
        {
            let truncated_payload = WAL_V2_MAGIC.to_vec(); // magic but no bincode body
            let crc = crc32fast::hash(&truncated_payload);
            let mut raw = crc.to_le_bytes().to_vec();
            raw.extend_from_slice(&truncated_payload);

            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut wal = write_txn.open_table(WAL_TABLE).unwrap();
                wal.insert(3u64, raw.as_slice()).unwrap();
            }
            write_txn.commit().unwrap();
        }

        // Phase 3: reopen — load_state must return a Serialization error for
        // seq 3 (bincode decode of empty slice fails), not Ok with a partial
        // chain at height 2.
        {
            let storage = Storage::open(&path, 100).unwrap();
            let result = storage.load_state();
            assert!(
                result.is_err(),
                "load_state must fail closed when WAL tail has valid magic but no bincode body"
            );
            assert!(
                matches!(result.unwrap_err(), StorageError::Serialization(_)),
                "error must be a Serialization decode failure for the truncated bincode body"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Companion regression: `persist_finalized_height` (the dedicated
    /// path used by the finality-signal consumer) must also make the
    /// new value durable on its own, without requiring a block commit.
    #[tokio::test]
    async fn persist_finalized_height_alone_survives_restart() {
        let path = temp_db_path();

        // Phase 1: persist a block with finalized=0, then bump finalized
        // via the standalone method only.
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();

            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            storage
                .persist_block_with_finalized(
                    block.clone(),
                    vec![],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                    0,
                )
                .await
                .unwrap();

            // Simulate a later BFT signal raising the finalized height.
            storage.persist_finalized_height(77).await.unwrap();
        }

        // Phase 2: reopen — finalized_height must be the post-signal value.
        {
            let storage = Storage::open(&path, 100).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap()
                .expect("storage must have persisted state");

            assert_eq!(
                blockchain.finalized_height(),
                77,
                "finalized_height must reflect the last persist_finalized_height \
                 call (#1275)"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    // -----------------------------------------------------------------------
    // Issue #1605 — WAL replay correctness at three abrupt-restart heights
    // -----------------------------------------------------------------------

    /// (b) Clean block boundary — issue #1605.
    ///
    /// Persists 5 blocks without triggering a checkpoint (interval=100), then
    /// drops the Storage (simulating SIGKILL between block writes).  On
    /// restart `load_state` must reconstruct the chain at exactly height=5 by
    /// replaying all 5 WAL entries from the empty-state baseline.
    ///
    /// A second restart from the already-recovered database must return the
    /// same height (idempotency).
    #[tokio::test]
    async fn wal_replay_clean_block_boundary_recovers_to_last_committed_and_is_idempotent() {
        let path = temp_db_path();
        const BLOCKS: u64 = 5;
        const REWARD: u128 = 20;

        // Phase 1: persist 5 blocks (no checkpoint).
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xB0);
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=BLOCKS {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(REWARD),
                    reason: format!("wal-replay-b-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
            // Storage drops here — simulates abrupt kill after last block commit.
        }

        // Phase 2: first restart — must recover to height=5 via pure WAL replay.
        {
            let storage = Storage::open(&path, 100).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap()
                .expect("WAL replay must produce a valid chain after abrupt restart");

            assert_eq!(
                blockchain.height, BLOCKS,
                "recovered chain height must match last committed block (b)"
            );
            let account = blockchain
                .state
                .get_account(&addr(0xB0))
                .expect("account must survive WAL replay");
            assert_eq!(
                account.balance,
                Balance(BLOCKS as u128 * REWARD),
                "account balance must equal sum of all replayed rewards"
            );
        }

        // Phase 3: second restart — idempotency.
        {
            let storage = Storage::open(&path, 100).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap()
                .expect("second load_state must succeed");

            assert_eq!(
                blockchain.height, BLOCKS,
                "second restart must return the same height (idempotency)"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// (c) State-root checkpoint boundary — issue #1605.
    ///
    /// Persists 7 blocks with checkpoint interval=5 so a full checkpoint is
    /// written at height 5 and WAL entries ≤ 5 are pruned.  Heights 6–7
    /// remain in the WAL.  After dropping the Storage (abrupt kill) and
    /// reopening, `load_state` must:
    ///   • load the checkpoint at height 5, then
    ///   • replay the two remaining WAL entries, producing height=7.
    ///
    /// A second restart must yield the same height (idempotency).
    #[tokio::test]
    async fn wal_replay_checkpoint_boundary_recovers_and_is_idempotent() {
        let path = temp_db_path();
        const CHECKPOINT_AT: u64 = 5;
        const TOTAL_BLOCKS: u64 = 7;
        const REWARD: u128 = 15;

        // Phase 1: persist 7 blocks; checkpoint fires at height 5.
        {
            let mut storage = Storage::open(&path, CHECKPOINT_AT).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xC0);
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=TOTAL_BLOCKS {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(REWARD),
                    reason: format!("wal-replay-c-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
            // Storage drops here — simulates abrupt kill after block 7.
        }

        // Sanity-check: exactly 2 WAL entries should remain (heights 6 and 7).
        {
            let db = redb::Database::create(&path).unwrap();
            let read_txn = db.begin_read().unwrap();
            let wal = read_txn.open_table(WAL_TABLE).unwrap();
            assert_eq!(
                wal.len().unwrap(),
                (TOTAL_BLOCKS - CHECKPOINT_AT) as u64,
                "WAL must contain only post-checkpoint entries"
            );
        }

        // Phase 2: first restart — checkpoint load + WAL replay.
        {
            let storage = Storage::open(&path, CHECKPOINT_AT).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap()
                .expect("must recover from checkpoint + WAL replay after abrupt restart");

            assert_eq!(
                blockchain.height, TOTAL_BLOCKS,
                "recovered height must equal last committed block (c)"
            );
            let account = blockchain
                .state
                .get_account(&addr(0xC0))
                .expect("account must survive checkpoint + WAL replay");
            assert_eq!(
                account.balance,
                Balance(TOTAL_BLOCKS as u128 * REWARD),
                "balance must equal sum of all rewards including pre-checkpoint entries"
            );
        }

        // Phase 3: second restart — idempotency.
        {
            let storage = Storage::open(&path, CHECKPOINT_AT).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap()
                .expect("second load_state from checkpoint must succeed");

            assert_eq!(
                blockchain.height, TOTAL_BLOCKS,
                "second restart must return the same height (idempotency, checkpoint path)"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// #2029: exercise the storage-backed fork-state *reconstruction* primitives
    /// that `try_reorg` (launch_api/p2p_events.rs ~850-914) runs on every live
    /// P2P reorg — `load_checkpoint_at_or_before` + `load_wal_transitions_range`
    /// + per-height `apply_block_finalization`. No other test drives this path:
    /// the existing reorg tests build fork state in-memory, and the WAL-replay
    /// tests above exercise `load_state()` (full restart), not the
    /// checkpoint+range reconstruction `try_reorg` uses to rebuild fork-point
    /// state before `execute_reorg`.
    ///
    /// Asserts the reconstructed fork-point `state_root` (not just balances,
    /// cf. #1982) equals the producer's finalized root at the fork height —
    /// the exact producer/replay parity surface behind the #1776/#1810
    /// cross-node state-divergence incidents.
    #[tokio::test]
    async fn reorg_fork_state_reconstruction_matches_direct_apply() {
        let path = temp_db_path();
        const CHECKPOINT_AT: u64 = 3;
        const TOTAL_BLOCKS: u64 = 5;
        // Storage keeps only the latest checkpoint, so with interval 3 the sole
        // surviving checkpoint is at height 3. The fork height sits *above* that
        // checkpoint and *below* the tip (5), so reconstruction MUST replay WAL
        // entries on top of the checkpoint — exactly the try_reorg path.
        const FORK_HEIGHT: u64 = 4;
        const REWARD: u128 = 15;
        let user = addr(0xD0);

        // Producer path: apply → finalize per height, capture the finalized root
        // at the fork height, and persist blocks + WAL + checkpoints to storage.
        let mut expected_root_at_fork = [0u8; 32];
        {
            let mut storage = Storage::open(&path, CHECKPOINT_AT).unwrap();
            let mut blockchain = Blockchain::default();
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=TOTAL_BLOCKS {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(REWARD),
                    reason: format!("reorg-recon-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();
                // Per-height finalization, byte-identical to the producer and to
                // the try_reorg WAL-replay reconstruction below.
                blockchain.state.apply_block_finalization(height);

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                let ckpt = storage
                    .checkpoint_state_bytes(block.height, &blockchain.state)
                    .unwrap();
                storage
                    .persist_block(block.clone(), vec![transition], ckpt)
                    .await
                    .unwrap();

                if height == FORK_HEIGHT {
                    expected_root_at_fork = blockchain.state.state_root.0;
                }
            }
        }

        // Reconstruction path: mirror try_reorg's checkpoint-load + WAL-range +
        // per-height finalize sequence exactly.
        {
            let storage = Storage::open(&path, CHECKPOINT_AT).unwrap();
            let (cp_height, mut fork_state) = storage
                .load_checkpoint_at_or_before(FORK_HEIGHT)
                .unwrap()
                .expect("a checkpoint at or before the fork height must exist");
            assert_eq!(
                cp_height, CHECKPOINT_AT,
                "nearest surviving checkpoint at/below the fork height must be height 3"
            );
            assert!(
                cp_height < FORK_HEIGHT,
                "reconstruction must replay WAL on top of the checkpoint"
            );

            let transitions = storage
                .load_wal_transitions_range(cp_height + 1, FORK_HEIGHT)
                .unwrap();
            assert!(
                !transitions.is_empty(),
                "WAL range (cp+1..=fork) must contain the post-checkpoint transitions"
            );

            // Group per height and finalize each block — identical to try_reorg.
            let mut pending_height: Option<u64> = None;
            let mut block_transitions: Vec<StateTransition> = Vec::new();
            for (h, t) in transitions {
                if pending_height.is_some_and(|ph| ph != h) {
                    let ph = pending_height.unwrap();
                    fork_state
                        .apply_transitions_batch_at_height(
                            ph,
                            std::mem::take(&mut block_transitions),
                        )
                        .unwrap();
                    fork_state.apply_block_finalization(ph);
                }
                pending_height = Some(h);
                block_transitions.push(t);
            }
            if let Some(ph) = pending_height {
                fork_state
                    .apply_transitions_batch_at_height(ph, block_transitions)
                    .unwrap();
                fork_state.apply_block_finalization(ph);
            }

            assert_eq!(
                fork_state.state_root.0, expected_root_at_fork,
                "reconstructed fork-point state_root must equal the producer's \
                 finalized root at the fork height (producer/replay parity, #1776/#1810)"
            );
            let acct = fork_state
                .get_account(&user)
                .expect("account must be present in the reconstructed fork state");
            assert_eq!(
                acct.balance,
                Balance(FORK_HEIGHT as u128 * REWARD),
                "reconstructed balance must equal the sum of rewards through the fork height"
            );
        }

        // Crash-restart parity variant: a full storage reload (`load_state`,
        // the WAL-replay path a restarted node uses) must reconstruct the same
        // tip `state_root` as a chain that applied every block directly.
        {
            let storage = Storage::open(&path, CHECKPOINT_AT).unwrap();
            let reloaded = storage
                .load_state()
                .unwrap()
                .expect("must reload chain state from checkpoint + WAL");

            let mut reference = Blockchain::default();
            reference.state.create_account(user, Balance(0));
            for height in 1u64..=TOTAL_BLOCKS {
                reference
                    .apply_state_transition(StateTransition::ApplyReward {
                        address: user,
                        amount: Balance(REWARD),
                        reason: format!("reorg-recon-{height}"),
                    })
                    .unwrap();
                reference.state.apply_block_finalization(height);
            }

            assert_eq!(
                reloaded.height, TOTAL_BLOCKS,
                "reloaded height must equal the last committed block"
            );
            assert_eq!(
                reloaded.state.state_root.0, reference.state.state_root.0,
                "crash-restart reload must reconstruct the same state_root as direct apply"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Error path — issue #1605.
    ///
    /// Verifies that the two truncated-tail scenarios tested above
    /// (`wal_replay_truncated_tail_*`) return typed `StorageError` variants and
    /// never `panic!` / `unwrap`.  This is a meta-test that documents the
    /// contract: all three abrupt-restart cases either succeed (b, c) or
    /// fail with a typed error (a) — never abort/panic.
    #[tokio::test]
    async fn wal_replay_truncated_tail_returns_typed_error_not_panic() {
        let path = temp_db_path();

        // Persist 2 clean blocks.
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xA9);
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=2 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(10),
                    reason: format!("typed-err-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Corrupt the tail WAL entry (seq 2) below minimum length.
        {
            let db = redb::Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut wal = write_txn.open_table(WAL_TABLE).unwrap();
                wal.insert(2u64, &[0xDE, 0xAD][..]).unwrap();
            }
            write_txn.commit().unwrap();
        }

        // Reopen: must return a typed error, not panic.
        let storage = Storage::open(&path, 100).unwrap();
        let result = storage.load_state();

        assert!(
            result.is_err(),
            "corrupted tail entry must produce an error"
        );
        // The error must be displayable (not just an opaque panic).
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.is_empty(),
            "StorageError must have a non-empty Display string"
        );
        // Must not be a generic "unknown" — error code or context must be present.
        assert!(
            msg.contains("WAL") || msg.contains("sequence") || msg.contains("prefix"),
            "error message must identify the WAL entry problem, got: {msg}"
        );

        let _ = std::fs::remove_file(&path);
    }

    // Issue #1735 — State-root determinism and WAL semantic-tampering detection
    // -----------------------------------------------------------------------

    /// WAL replay is state-root deterministic — two restarts from the same
    /// database must produce byte-identical state roots.
    ///
    /// This catches non-deterministic replay bugs: HashMap iteration order,
    /// missing collection sorts, randomised data, or skipped entries would all
    /// cause the second restart to produce a different state root.
    #[tokio::test]
    async fn wal_replay_state_root_determinism_across_restarts() {
        let path = temp_db_path();
        const BLOCKS: u64 = 4;
        const REWARD: u128 = 25;

        // Phase 1: persist 4 blocks (checkpoint interval=100 → no checkpoint).
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xE1);
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=BLOCKS {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(REWARD),
                    reason: format!("determinism-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Phase 2: first restart — record state_root.
        let state_root_first = {
            let storage = Storage::open(&path, 100).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap()
                .expect("WAL replay must succeed on first restart");
            assert_eq!(
                blockchain.height, BLOCKS,
                "recovered height must match last committed block"
            );
            let account = blockchain.state.get_account(&addr(0xE1)).unwrap();
            assert_eq!(
                account.balance,
                Balance(BLOCKS as u128 * REWARD),
                "balance must equal sum of all replayed rewards"
            );
            blockchain.state.state_root.0
        };

        // Phase 3: second restart — state_root must be identical.
        {
            let storage = Storage::open(&path, 100).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap()
                .expect("WAL replay must succeed on second restart");
            assert_eq!(
                blockchain.state.state_root.0,
                state_root_first,
                "WAL replay must produce byte-identical state_root across restarts \
                 (non-determinism would allow silent state divergence between nodes)"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Semantically-tampered WAL entry (valid bincode, valid CRC, mutated amount)
    /// produces a different state_root than honest replay and loads with wrong state.
    ///
    /// SECURITY GAP: `StorageError::StateRootMismatch` is defined (storage.rs ~line 69)
    /// but never emitted — `load_state` currently returns `Ok` with wrong state instead
    /// of detecting the divergence.  When `META_STATE_ROOT` is wired up (#1735),
    /// `load_state` must return `StorageError::StateRootMismatch` for this scenario.
    ///
    /// This test acts as a regression anchor: if tampering somehow becomes
    /// undetectable (state_root no longer diverges), the test will fail.
    #[tokio::test]
    async fn wal_semantic_tampering_diverges_from_expected_state_root() {
        let path = temp_db_path();
        const REWARD: u128 = 50;

        // Phase 1: persist 3 blocks — each adds Balance(50).
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xE2);
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=3u64 {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(REWARD),
                    reason: format!("tamper-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Capture honest state_root before tampering.
        let honest_state_root = {
            let storage = Storage::open(&path, 100).unwrap();
            let blockchain = storage.load_state().unwrap().unwrap();
            assert_eq!(
                blockchain.state.get_account(&addr(0xE2)).unwrap().balance,
                Balance(3 * REWARD),
                "honest replay must yield correct balance"
            );
            blockchain.state.state_root.0
        };

        // Phase 2: tamper WAL seq 2 — change Balance(50) → Balance(999).
        // Re-encode with a correct CRC so the CRC guard in load_state is satisfied.
        // This simulates an adversary who can write to the WAL database directly.
        {
            // Read raw bytes from WAL seq 2 (no encryption in tests).
            // Format: [4-byte CRC32 LE][WAL_V2_MAGIC][bincode(WalEntryV2)]
            let tampered_raw = {
                let db = Database::create(&path).unwrap();
                let read_txn = db.begin_read().unwrap();
                let wal = read_txn.open_table(WAL_TABLE).unwrap();
                let raw: Vec<u8> = wal.get(2u64).unwrap().unwrap().value().to_vec();
                assert!(raw.len() > 4, "WAL entry must have CRC prefix");
                let payload = &raw[4..];
                let versioned = payload
                    .strip_prefix(WAL_V2_MAGIC)
                    .expect("seq 2 must be a v2 entry");
                let mut v2: WalEntryV2 = bincode::deserialize(versioned)
                    .expect("seq 2 must decode as WalEntryV2");
                let original: StateTransition =
                    serde_json::from_slice(&v2.transition_json)
                        .expect("seq 2 JSON must decode as StateTransition");

                let tampered = match original {
                    StateTransition::ApplyReward { address, reason, .. } => {
                        StateTransition::ApplyReward {
                            address,
                            amount: Balance(999),
                            reason,
                        }
                    },
                    other => panic!("seq 2 must be ApplyReward, got {other:?}"),
                };
                v2.transition_json = serde_json::to_vec(&tampered).unwrap();

                let bincode_body = bincode::serialize(&v2).unwrap();
                let mut new_payload = WAL_V2_MAGIC.to_vec();
                new_payload.extend_from_slice(&bincode_body);
                let new_crc = crc32fast::hash(&new_payload);
                let mut new_raw = new_crc.to_le_bytes().to_vec();
                new_raw.extend_from_slice(&new_payload);
                new_raw
            };

            let db = Database::create(&path).unwrap();
            let write_txn = db.begin_write().unwrap();
            {
                let mut wal = write_txn.open_table(WAL_TABLE).unwrap();
                wal.insert(2u64, tampered_raw.as_slice()).unwrap();
            }
            write_txn.commit().unwrap();
        }

        // Phase 3: reload after tampering.
        // load_state currently returns Ok (StateRootMismatch gap — #1735).
        {
            let storage = Storage::open(&path, 100).unwrap();
            let blockchain = storage
                .load_state()
                .unwrap() // No error today — this is the gap documented by this test.
                .expect("tampered WAL loads silently until META_STATE_ROOT is implemented");

            // State reflects the injected amount: seq1(50) + tampered-seq2(999) + seq3(50).
            let account = blockchain.state.get_account(&addr(0xE2)).unwrap();
            assert_ne!(
                account.balance,
                Balance(3 * REWARD),
                "tampered WAL must not yield the correct balance"
            );
            assert_eq!(
                account.balance,
                Balance(REWARD + 999u128 + REWARD),
                "tampered WAL balance must reflect the injected amount (50 + 999 + 50 = 1099)"
            );

            // State root diverges from honest replay — detection is possible in principle.
            // This assert is the regression anchor: if state_root no longer reflects
            // the tampered balance, the detection mechanism has regressed.
            assert_ne!(
                blockchain.state.state_root.0,
                honest_state_root,
                "tampered WAL must produce a different state_root than honest replay — \
                 once META_STATE_ROOT is wired up (#1735) this path must return \
                 StorageError::StateRootMismatch instead of Ok"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    // ── #1735 hardening: WAL-only multi-block replay ──────────────────────────

    /// Write 5 blocks WITHOUT any checkpoint (checkpoint_interval set
    /// astronomically high), close storage, re-open, and verify that
    /// `load_state()` recovers the full cumulative balance from WAL entries
    /// alone (no snapshot anchor).
    ///
    /// This is the complement of `checkpoint_pruning_and_wal_replay_recovery`
    /// which uses a checkpoint at height 5: here the only recovery path is
    /// full WAL replay from genesis, confirming the WAL-only code branch is
    /// exercised end-to-end.
    #[tokio::test]
    async fn wal_only_multi_block_replay_recovers_correct_state() {
        let path = temp_db_path();
        const REWARD: Balance = Balance(7);
        const BLOCKS: u64 = 5;

        // Write BLOCKS blocks with no checkpoint (interval = u64::MAX / 2 → never fires)
        {
            let mut storage = Storage::open(&path, u64::MAX / 2).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(55);
            blockchain.state.create_account(user, Balance(0));

            for h in 1..=BLOCKS {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: REWARD,
                    reason: "wal-only-test".to_string(),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, h);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                // `checkpoint_state_bytes` returns None because no checkpoint
                // interval is hit — so NO snapshot is written to disk.
                let checkpoint_bytes = storage
                    .checkpoint_state_bytes(block.height, &blockchain.state)
                    .unwrap();
                assert!(
                    checkpoint_bytes.is_none(),
                    "sanity: no checkpoint should be written at height {h} with u64::MAX/2 interval"
                );
                storage
                    .persist_block(block, vec![transition], checkpoint_bytes)
                    .await
                    .unwrap();
            }
        }

        // Confirm CHECKPOINTS table is empty — pure WAL scenario.
        {
            let db = Database::create(&path).unwrap();
            let read_txn = db.begin_read().unwrap();
            let checkpoints = read_txn
                .open_table(CHECKPOINTS_TABLE)
                .unwrap();
            assert_eq!(
                checkpoints.len().unwrap(),
                0u64,
                "no checkpoint must exist when interval is u64::MAX/2"
            );
            let wal = read_txn.open_table(WAL_TABLE).unwrap();
            assert_eq!(
                wal.len().unwrap(),
                BLOCKS,
                "WAL must hold exactly {BLOCKS} entries (one per block)"
            );
        }

        // Reload and verify full state recovery from WAL only.
        {
            let storage = Storage::open(&path, u64::MAX / 2).unwrap();
            let blockchain = storage
                .load_state()
                .expect("load_state must not error on WAL-only database")
                .expect("load_state must return Some after writing blocks");

            assert_eq!(
                blockchain.height, BLOCKS,
                "recovered height must equal the number of blocks written"
            );

            let account = blockchain
                .state
                .get_account(&addr(55))
                .expect("account must exist after WAL-only replay");

            let expected = Balance(REWARD.0 * BLOCKS as u128);
            assert_eq!(
                account.balance, expected,
                "balance after WAL-only replay must equal {BLOCKS} blocks"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Write 3 blocks each with multiple state transitions per block (reward
    /// multiple addresses), then reload via WAL-only replay and verify that
    /// every per-block transition batch is applied exactly once, in order.
    ///
    /// This exercises the block-boundary grouping in `load_state` — the loop
    /// that accumulates WAL entries sharing the same `block_height` into a
    /// single `apply_transitions_batch` call before advancing to the next block.
    /// A bug here would typically manifest as: only the last transition per
    /// block applies (if the batch is accidentally cleared), or transitions from
    /// block N+1 contaminate the batch for block N.
    #[tokio::test]
    async fn wal_multi_transition_per_block_replay_correct() {
        let path = temp_db_path();
        const N_BLOCKS: u64 = 3;
        const ADDRS: [u8; 3] = [11, 12, 13]; // one address per "sensor" transition
        const REWARD: u128 = 100;

        // Write N_BLOCKS blocks, each with len(ADDRS) reward transitions.
        {
            let mut storage = Storage::open(&path, u64::MAX / 2).unwrap();
            let mut blockchain = Blockchain::default();

            // Pre-create all accounts so apply_transitions_batch doesn't error on missing accounts.
            for &b in &ADDRS {
                blockchain.state.create_account(addr(b), Balance(0));
            }

            for h in 1..=N_BLOCKS {
                let transitions: Vec<StateTransition> = ADDRS
                    .iter()
                    .map(|&b| StateTransition::ApplyReward {
                        address: addr(b),
                        amount: Balance(REWARD),
                        reason: format!("block-{h}-addr-{b}"),
                    })
                    .collect();

                for t in &transitions {
                    blockchain.apply_state_transition(t.clone()).unwrap();
                }

                let mut block = Block::new(3, blockchain.tip, vec![], 1, h);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                let ckpt = storage
                    .checkpoint_state_bytes(block.height, &blockchain.state)
                    .unwrap();
                storage
                    .persist_block(block, transitions, ckpt)
                    .await
                    .unwrap();
            }
        }

        // Reload from WAL and verify each address received exactly N_BLOCKS × REWARD.
        {
            let storage = Storage::open(&path, u64::MAX / 2).unwrap();
            let blockchain = storage
                .load_state()
                .expect("load_state must not error")
                .expect("load_state must return Some");

            assert_eq!(blockchain.height, N_BLOCKS);

            let expected = Balance(REWARD * N_BLOCKS as u128);
            for &b in &ADDRS {
                let account = blockchain
                    .state
                    .get_account(&addr(b))
                    .unwrap_or_else(|| panic!("addr({b}) must exist after replay"));
                assert_eq!(
                    account.balance, expected,
                    "addr({b}): expected balance {expected:?} after {N_BLOCKS} blocks, \
                     each contributing {REWARD} WRF"
                );
            }
        }

        let _ = std::fs::remove_file(&path);
    }

    // ── #1735 hardening: WAL replay corruption edge cases ────────────────────

    /// CRC corruption in the FIRST WAL entry (seq 1, no valid predecessors)
    /// must cause `load_state` to return `Err(ChecksumMismatch)` immediately.
    ///
    /// Complements `load_state_fails_closed_on_mid_sequence_wal_checksum_corruption`
    /// which corrupts seq 2 of 3 (leaving seq 1 valid). This test exercises
    /// the "very first iteration" code path: the replay loop must not need a
    /// successfully-applied predecessor entry before it can fire the CRC guard.
    #[tokio::test]
    async fn wal_first_entry_crc_corruption_fails_closed() {
        let path = temp_db_path();

        // Phase 1: write 1 block → WAL seq 1 only, no checkpoint.
        {
            let mut storage = Storage::open(&path, 100).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xF1);
            blockchain.state.create_account(user, Balance(0));

            let transition = StateTransition::ApplyReward {
                address: user,
                amount: Balance(10),
                reason: "first-entry-crc".to_string(),
            };
            blockchain
                .apply_state_transition(transition.clone())
                .unwrap();

            let mut block = Block::new(3, blockchain.tip, vec![], 1, 1);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();

            storage
                .persist_block(
                    block.clone(),
                    vec![transition],
                    storage
                        .checkpoint_state_bytes(block.height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        // Phase 2: corrupt WAL seq 1 — flip the first byte of its CRC prefix.
        {
            let db = redb::Database::create(&path).unwrap();
            let read_txn = db.begin_read().unwrap();
            let wal = read_txn.open_table(WAL_TABLE).unwrap();
            assert_eq!(wal.len().unwrap(), 1, "expect exactly 1 WAL entry");

            let raw: Vec<u8> = wal.get(1u64).unwrap().unwrap().value().to_vec();
            drop(wal);
            drop(read_txn);

            let mut corrupted = raw;
            assert!(corrupted.len() >= 5, "WAL entry must have at least 5 bytes");
            corrupted[0] ^= 0xFF; // flip first byte of 4-byte CRC prefix

            let write_txn = db.begin_write().unwrap();
            {
                let mut wal_w = write_txn.open_table(WAL_TABLE).unwrap();
                wal_w.insert(1u64, corrupted.as_slice()).unwrap();
            }
            write_txn.commit().unwrap();
        }

        // Phase 3: reopen — load_state must error on the very first WAL entry.
        {
            let storage = Storage::open(&path, 100).unwrap();
            let result = storage.load_state();
            assert!(
                result.is_err(),
                "load_state must fail closed when the FIRST WAL entry has a CRC mismatch"
            );
            assert!(
                matches!(
                    result.unwrap_err(),
                    StorageError::ChecksumMismatch { sequence: 1, .. }
                ),
                "error must identify sequence 1 as the corrupted entry"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    /// CRC corruption in a WAL entry that was written AFTER a valid checkpoint
    /// must cause `load_state` to return `Err(ChecksumMismatch)` — the
    /// checkpoint anchor does not suppress CRC verification for subsequent
    /// WAL replay.
    ///
    /// Setup: 5 blocks, checkpoint interval=3 fires at height 3 (pruning WAL
    /// entries 1–3). WAL entries 4 and 5 survive. Corrupting entry 5 must
    /// fail the reload even though the checkpoint at height 3 is intact.
    #[tokio::test]
    async fn wal_post_checkpoint_crc_corruption_fails_closed() {
        let path = temp_db_path();
        const CHECKPOINT_AT: u64 = 3;
        const TOTAL_BLOCKS: u64 = 5;

        // Phase 1: write 5 blocks; checkpoint fires at height 3.
        {
            let mut storage = Storage::open(&path, CHECKPOINT_AT).unwrap();
            let mut blockchain = Blockchain::default();
            let user = addr(0xF2);
            blockchain.state.create_account(user, Balance(0));

            for height in 1u64..=TOTAL_BLOCKS {
                let transition = StateTransition::ApplyReward {
                    address: user,
                    amount: Balance(7),
                    reason: format!("post-ckpt-crc-{height}"),
                };
                blockchain
                    .apply_state_transition(transition.clone())
                    .unwrap();

                let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
                let hash = block.calculate_hash();
                block.set_hash(hash);
                blockchain.add_block(block.clone()).unwrap();

                storage
                    .persist_block(
                        block.clone(),
                        vec![transition],
                        storage
                            .checkpoint_state_bytes(block.height, &blockchain.state)
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        }

        // Verify post-checkpoint state: 2 WAL entries (blocks 4 and 5) survive.
        let (seq_4, seq_5) = {
            let db = redb::Database::create(&path).unwrap();
            let read_txn = db.begin_read().unwrap();
            let wal = read_txn.open_table(WAL_TABLE).unwrap();
            assert_eq!(
                wal.len().unwrap(),
                (TOTAL_BLOCKS - CHECKPOINT_AT) as u64,
                "only post-checkpoint WAL entries (blocks 4 and 5) must survive"
            );
            let seqs: Vec<u64> = wal
                .iter()
                .unwrap()
                .map(|r| r.unwrap().0.value())
                .collect();
            assert_eq!(seqs.len(), 2, "must have exactly 2 surviving sequences");
            (seqs[0], seqs[1])
        };

        // Phase 2: corrupt the LAST surviving WAL entry (seq 5 — block 5).
        {
            let db = redb::Database::create(&path).unwrap();
            let read_txn = db.begin_read().unwrap();
            let wal = read_txn.open_table(WAL_TABLE).unwrap();
            let raw: Vec<u8> = wal.get(seq_5).unwrap().unwrap().value().to_vec();
            drop(wal);
            drop(read_txn);

            let mut corrupted = raw;
            assert!(corrupted.len() >= 5, "WAL entry must have at least 5 bytes");
            corrupted[0] ^= 0xFF; // flip first CRC byte

            let write_txn = db.begin_write().unwrap();
            {
                let mut wal_w = write_txn.open_table(WAL_TABLE).unwrap();
                wal_w.insert(seq_5, corrupted.as_slice()).unwrap();
            }
            write_txn.commit().unwrap();
        }

        // Phase 3: reopen — checkpoint at height 3 loads cleanly, WAL entry
        // seq_4 replays cleanly, but seq_5 is corrupt → must return Err.
        let _ = seq_4; // seq_4 is valid; seq_5 is corrupt
        {
            let storage = Storage::open(&path, CHECKPOINT_AT).unwrap();
            let result = storage.load_state();
            assert!(
                result.is_err(),
                "load_state must fail closed when a post-checkpoint WAL entry is CRC-corrupted"
            );
            assert!(
                matches!(
                    result.unwrap_err(),
                    StorageError::ChecksumMismatch { sequence, .. } if sequence == seq_5
                ),
                "error must identify the corrupted post-checkpoint sequence ({seq_5})"
            );
        }

        let _ = std::fs::remove_file(&path);
    }
}
