//! Checkpoint format migrations: format N → N+1 on boot, in place, no volume
//! wipe (#3091, root cause #2727).
//!
//! # Contract
//!
//! A checkpoint written by an OLDER binary carries a lower
//! [`CHECKPOINT_FORMAT_VERSION`] in its envelope. Instead of refusing it
//! (`EXIT_CHECKPOINT_FORMAT_VERSION_MISMATCH`), the decode path in
//! `storage.rs` walks the registered migrations from the found version up to
//! the current one and decodes the result with the current layout. Refusal
//! stays for the two cases a migration cannot cover:
//!
//! - a checkpoint written by a NEWER binary (`found > supported`) — still
//!   `EXIT_CHECKPOINT_FORMAT_VERSION_MISMATCH`;
//! - an older version with no registered path — a *named*
//!   `EXIT_CHECKPOINT_MIGRATION_UNAVAILABLE`, never a decode attempt.
//!
//! # Frozen layouts
//!
//! bincode is positional: a migration can only decode version-N bytes with
//! the version-N `State` layout. Those layouts live under
//! [`super::legacy`] as **frozen copies** that are never edited again — each
//! future format adds `state_vN.rs` for the layout it replaces. The golden
//! fixtures in `native-chain/tests/fixtures/checkpoint_state_v{N}_golden.bin`
//! (written by the real version-N code at the time) are the tripwire: if a
//! nested type a frozen layout points at ever changes shape, the frozen
//! decode of that golden fails at test time and the change is loud.
//!
//! # Write-after-verify
//!
//! A migration only produces bytes. Nothing on disk changes until the node
//! has finished every boot assertion (WAL replay, the #2219 tip commitment,
//! the producer-authority backstop); only then does `launch_api` call
//! [`super::Storage::commit_migrated_checkpoints`]. A failed migration or a
//! failed boot therefore leaves the original bytes byte-identical, and an
//! image rollback still reads them.
//!
//! # WAL
//!
//! The WAL did not change layout between formats 3 and 4: entries are
//! `WAL_V2_MAGIC ++ bincode(WalEntryV2 { block_height, transition_json })`
//! and the transition is JSON, which tolerates variants and fields added
//! later. A v3-era WAL therefore replays on a v4 binary unchanged
//! (`wal_v3_era_entry_replays_on_the_current_decoder` pins it). There is no
//! `migrate_wal_entry`; when a format does change the WAL layout, add one
//! here beside the checkpoint migration for the same version pair.

use std::collections::BTreeMap;

use thiserror::Error;
use wrfcoin_shared_protocol::accounts::authorization::AccountAuthorizationDeclaration;
use wrfcoin_shared_protocol::accounts::paymaster::PaymasterAccount;
use wrfcoin_shared_protocol::accounts::recovery::AccountRecoveryState;
use wrfcoin_shared_protocol::accounts::session_keys::OwnerSessionKeys;
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::governance::schedule_extension::AppliedExtension;
use wrfcoin_shared_protocol::oracle::attestation::ProducerSettlementRecord;

use super::legacy;
use super::{CHECKPOINT_ENVELOPE_MAGIC, CHECKPOINT_FORMAT_VERSION};

/// Why a checkpoint could not be migrated.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MigrationError {
    /// No chain of registered migrations leads from `from` to `to`.
    #[error(
        "no registered checkpoint migration path from format {from} to format {to} (#3091)"
    )]
    NoPath { from: u32, to: u32 },
    /// The bytes do not decode as a complete `State` of the version they
    /// claim — truncated, garbage, or mislabelled. This is corruption of the
    /// stored row, not a format gap.
    #[error("checkpoint body does not decode as a complete format-{version} State: {error}")]
    Decode { version: u32, error: String },
    /// The migrated bytes failed to decode as the target version — a bug in
    /// the migration itself, never the operator's data.
    #[error("migrated checkpoint does not decode as format {version}: {error}")]
    Encode { version: u32, error: String },
}

/// One format step. Implementations decode with the FROZEN `from_version`
/// layout and emit the body (envelope stripped) of `to_version`.
pub trait CheckpointMigration: Sync {
    /// The format this migration reads (N).
    fn from_version(&self) -> u32;
    /// The format this migration writes (N+1).
    fn to_version(&self) -> u32;
    /// Migrate one checkpoint BODY (bytes after the 12-byte envelope header)
    /// from `from_version` to `to_version`.
    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError>;
}

/// Every registered migration, one per consecutive version pair.
pub static MIGRATIONS: &[&dyn CheckpointMigration] = &[
    &MigrationV3ToV4,
    &MigrationV4ToV5,
    &MigrationV5ToV6,
    &MigrationV6ToV7,
    &MigrationV7ToV8,
    &MigrationV8ToV9,
    &MigrationV9ToV10,
    &MigrationV10ToV11,
    &MigrationV11ToV12,
    &MigrationV12ToV13,
    &MigrationV13ToV14,
    &MigrationV14ToV15,
];

/// The registered migration that reads `version`, if any.
pub fn migration_from(version: u32) -> Option<&'static dyn CheckpointMigration> {
    MIGRATIONS
        .iter()
        .copied()
        .find(|m| m.from_version() == version)
}

/// Whether a complete chain of migrations exists from `found` up to
/// [`CHECKPOINT_FORMAT_VERSION`].
pub fn has_path(found: u32) -> bool {
    let mut version = found;
    while version < CHECKPOINT_FORMAT_VERSION {
        match migration_from(version) {
            Some(m) => version = m.to_version(),
            None => return false,
        }
    }
    version == CHECKPOINT_FORMAT_VERSION
}

/// Wrap a body in the envelope for `version`
/// (`CHECKPOINT_ENVELOPE_MAGIC ++ version (u32 LE) ++ body`).
pub fn envelope(version: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(CHECKPOINT_ENVELOPE_MAGIC.len() + 4 + body.len());
    bytes.extend_from_slice(&CHECKPOINT_ENVELOPE_MAGIC);
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}

/// Walk `found` → … → [`CHECKPOINT_FORMAT_VERSION`] and return the migrated
/// checkpoint as a COMPLETE current-version envelope, ready for the ordinary
/// decode path. `found == CHECKPOINT_FORMAT_VERSION` is the identity;
/// `found > CHECKPOINT_FORMAT_VERSION` is `NoPath` (a newer binary's bytes
/// are never touched here — the caller refuses them by name).
pub fn migrate_to_current(found: u32, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
    if found > CHECKPOINT_FORMAT_VERSION || !has_path(found) {
        return Err(MigrationError::NoPath {
            from: found,
            to: CHECKPOINT_FORMAT_VERSION,
        });
    }
    let mut version = found;
    let mut current = body.to_vec();
    while version < CHECKPOINT_FORMAT_VERSION {
        let migration = migration_from(version).ok_or(MigrationError::NoPath {
            from: found,
            to: CHECKPOINT_FORMAT_VERSION,
        })?;
        current = migration.migrate(&current)?;
        version = migration.to_version();
    }
    Ok(envelope(version, &current))
}

/// Decode a body as exactly one value of `T`, with the size limit bound to
/// the input (#2881 defence in depth) and trailing bytes REJECTED — a
/// migration must know that the body ends where the frozen layout says it
/// ends, or an appended field would land at the wrong offset.
fn decode_exact<'de, T: serde::Deserialize<'de>>(body: &'de [u8]) -> Result<T, bincode::Error> {
    use bincode::Options;
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .reject_trailing_bytes()
        .with_limit(body.len() as u64)
        .deserialize(body)
}

/// Format 3 → 4 (#3069 added Phase B producer settlement state).
///
/// The only layout difference is one field APPENDED to the end of `State`:
/// `last_producer_settlement: Option<ProducerSettlementRecord>`. Every other
/// serialized field — including the two private ones, `commit_reveal_state`
/// and `oracle_registry` — is byte-identical in position and type (verified
/// against `git show 3c7553a41:shared-protocol/src/blockchain/state.rs`,
/// the v3 layout, and the v4 golden fixture).
///
/// The migration therefore:
/// 1. decodes the body with the frozen [`legacy::state_v3::State`], trailing
///    bytes rejected, which PROVES the bytes are a complete v3 `State` and
///    that the append below lands exactly at its end;
/// 2. appends `bincode(None::<ProducerSettlementRecord>)` — the default for
///    the new field, a node that has never settled a producer;
/// 3. decodes the result with the frozen format-4 layout as a self-check.
///
/// Field-wise construction of the format-4 `State` is not possible from this
/// crate (the private fields have no public constructor), and for an
/// append-only change it would also be strictly weaker: bincode is
/// positional, so the prefix IS the migrated value.
pub struct MigrationV3ToV4;

impl CheckpointMigration for MigrationV3ToV4 {
    fn from_version(&self) -> u32 {
        3
    }

    fn to_version(&self) -> u32 {
        4
    }

    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let _frozen: legacy::state_v3::State =
            decode_exact(body).map_err(|e| MigrationError::Decode {
                version: 3,
                error: e.to_string(),
            })?;

        let appended = bincode::serialize(&None::<ProducerSettlementRecord>).map_err(|e| {
            MigrationError::Encode {
                version: 4,
                error: e.to_string(),
            }
        })?;
        let mut migrated = Vec::with_capacity(body.len() + appended.len());
        migrated.extend_from_slice(body);
        migrated.extend_from_slice(&appended);

        let _v4: legacy::state_v4::State =
            decode_exact(&migrated).map_err(|e| MigrationError::Encode {
                version: 4,
                error: e.to_string(),
            })?;
        Ok(migrated)
    }
}

/// Format 4 → 5 (#2172 appended durable account-recovery state).
///
/// The only layout difference is the trailing
/// `account_recovery: BTreeMap<Address, AccountRecoveryState>` field. A
/// format-4 checkpoint necessarily predates recovery activation, so its
/// deterministic migration value is the empty map. As with v3 → v4, frozen
/// decoding first proves the source body ends at the append boundary, then
/// current-layout decoding proves the emitted bytes are a valid format-5
/// state.
pub struct MigrationV4ToV5;

impl CheckpointMigration for MigrationV4ToV5 {
    fn from_version(&self) -> u32 {
        4
    }

    fn to_version(&self) -> u32 {
        5
    }

    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let _frozen: legacy::state_v4::State =
            decode_exact(body).map_err(|e| MigrationError::Decode {
                version: 4,
                error: e.to_string(),
            })?;

        let empty_recovery = BTreeMap::<Address, AccountRecoveryState>::new();
        let appended = bincode::serialize(&empty_recovery).map_err(|e| MigrationError::Encode {
            version: 5,
            error: e.to_string(),
        })?;
        let mut migrated = Vec::with_capacity(body.len() + appended.len());
        migrated.extend_from_slice(body);
        migrated.extend_from_slice(&appended);

        let current: legacy::state_v5::State =
            decode_exact(&migrated).map_err(|e| MigrationError::Encode {
            version: 5,
            error: e.to_string(),
        })?;
        debug_assert!(current.account_recovery.is_empty());
        Ok(migrated)
    }
}

/// Format 5 → 6 (#2170 appended durable session-key state).
///
/// The only layout difference is the trailing, owner-keyed
/// `session_keys` map. A format-5 checkpoint predates session-key activation,
/// so the deterministic migration value is empty. Frozen v5 decoding proves
/// the append boundary before current-layout decoding verifies the result.
pub struct MigrationV5ToV6;

impl CheckpointMigration for MigrationV5ToV6 {
    fn from_version(&self) -> u32 {
        5
    }

    fn to_version(&self) -> u32 {
        6
    }

    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let _frozen: legacy::state_v5::State =
            decode_exact(body).map_err(|e| MigrationError::Decode {
                version: 5,
                error: e.to_string(),
            })?;

        let empty_session_keys = BTreeMap::<Address, OwnerSessionKeys>::new();
        let appended =
            bincode::serialize(&empty_session_keys).map_err(|e| MigrationError::Encode {
                version: 6,
                error: e.to_string(),
            })?;
        let mut migrated = Vec::with_capacity(body.len() + appended.len());
        migrated.extend_from_slice(body);
        migrated.extend_from_slice(&appended);

        let current: legacy::state_v6::State = decode_exact(&migrated).map_err(|e| MigrationError::Encode {
            version: 6,
            error: e.to_string(),
        })?;
        debug_assert!(current.session_keys.is_empty());
        Ok(migrated)
    }
}

/// Format 6 -> 7 (#2168 appended durable authorization declarations).
pub struct MigrationV6ToV7;
impl CheckpointMigration for MigrationV6ToV7 {
    fn from_version(&self) -> u32 { 6 }
    fn to_version(&self) -> u32 { 7 }
    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let _frozen: legacy::state_v6::State = decode_exact(body).map_err(|e| MigrationError::Decode { version: 6, error: e.to_string() })?;
        let appended = bincode::serialize(&BTreeMap::<Address, AccountAuthorizationDeclaration>::new()).map_err(|e| MigrationError::Encode { version: 7, error: e.to_string() })?;
        let mut migrated = Vec::with_capacity(body.len() + appended.len());
        migrated.extend_from_slice(body); migrated.extend_from_slice(&appended);
        let current: legacy::state_v7::State = decode_exact(&migrated).map_err(|e| MigrationError::Encode { version: 7, error: e.to_string() })?;
        debug_assert!(current.authorization_declarations.is_empty());
        Ok(migrated)
    }
}

/// Format 7 -> 8 (#2171 appended durable paymaster accounts).
pub struct MigrationV7ToV8;

impl CheckpointMigration for MigrationV7ToV8 {
    fn from_version(&self) -> u32 {
        7
    }

    fn to_version(&self) -> u32 {
        8
    }

    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let _frozen: legacy::state_v7::State =
            decode_exact(body).map_err(|e| MigrationError::Decode {
                version: 7,
                error: e.to_string(),
            })?;
        let appended = bincode::serialize(&BTreeMap::<Address, PaymasterAccount>::new())
            .map_err(|e| MigrationError::Encode {
                version: 8,
                error: e.to_string(),
            })?;
        let mut migrated = Vec::with_capacity(body.len() + appended.len());
        migrated.extend_from_slice(body);
        migrated.extend_from_slice(&appended);
        let current: legacy::state_v8::State =
            decode_exact(&migrated).map_err(|e| MigrationError::Encode {
                version: 8,
                error: e.to_string(),
            })?;
        debug_assert!(current.paymasters.is_empty());
        Ok(migrated)
    }
}

/// Format 8 -> 9 (#3277 S2 appended the applied feature-schedule extension
/// list). Every format-8 volume predates the extension transaction, so the
/// list is empty by construction.
pub struct MigrationV8ToV9;

impl CheckpointMigration for MigrationV8ToV9 {
    fn from_version(&self) -> u32 {
        8
    }

    fn to_version(&self) -> u32 {
        9
    }

    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let _frozen: legacy::state_v8::State =
            decode_exact(body).map_err(|e| MigrationError::Decode {
                version: 8,
                error: e.to_string(),
            })?;
        let appended = bincode::serialize(&Vec::<AppliedExtension>::new()).map_err(|e| {
            MigrationError::Encode {
                version: 9,
                error: e.to_string(),
            }
        })?;
        let mut migrated = Vec::with_capacity(body.len() + appended.len());
        migrated.extend_from_slice(body);
        migrated.extend_from_slice(&appended);
        let current: legacy::state_v9::State =
            decode_exact(&migrated).map_err(|e| MigrationError::Encode {
                version: 9,
                error: e.to_string(),
            })?;
        debug_assert!(current.feature_schedule_extensions.is_empty());
        Ok(migrated)
    }
}

/// Format 9 -> 10 (#3270 appended the per-station accepting-block
/// timestamps read by the halt-aware weather skew allowance).
///
/// Append-only, like every step since 3 -> 4: the new map is the last field
/// of `State`, and its default is empty — no pre-#3270 node ever wrote it, and
/// the reveal arm writes it only once
/// `ConsensusFeature::WeatherSkewBlockTimeAllowance` is active.
pub struct MigrationV9ToV10;

impl CheckpointMigration for MigrationV9ToV10 {
    fn from_version(&self) -> u32 {
        9
    }

    fn to_version(&self) -> u32 {
        10
    }

    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let _frozen: legacy::state_v9::State =
            decode_exact(body).map_err(|e| MigrationError::Decode {
                version: 9,
                error: e.to_string(),
            })?;
        let appended = bincode::serialize(&BTreeMap::<String, i64>::new()).map_err(|e| {
            MigrationError::Encode {
                version: 10,
                error: e.to_string(),
            }
        })?;
        let mut migrated = Vec::with_capacity(body.len() + appended.len());
        migrated.extend_from_slice(body);
        migrated.extend_from_slice(&appended);
        let current: legacy::state_v10::State =
            decode_exact(&migrated).map_err(|e| MigrationError::Encode {
                version: 10,
                error: e.to_string(),
            })?;
        debug_assert!(current.station_weather_accepted_block_unix.is_empty());
        Ok(migrated)
    }
}

/// Append the owner ledger after the deployed weather format10 layout.
pub struct MigrationV10ToV11;
impl CheckpointMigration for MigrationV10ToV11 {
    fn from_version(&self) -> u32 {
        10
    }
    fn to_version(&self) -> u32 {
        11
    }
    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let _: legacy::state_v10::State =
            decode_exact(body).map_err(|e| MigrationError::Decode {
                version: 10,
                error: e.to_string(),
            })?;
        let mut migrated = body.to_vec();
        // The BTreeMap length is a u64 in the fixed-int checkpoint codec.
        migrated.extend_from_slice(&0u64.to_le_bytes());
        let current: legacy::state_v11::State = decode_exact(&migrated).map_err(|e| MigrationError::Encode {
            version: 11,
            error: e.to_string(),
        })?;
        debug_assert!(current.station_owner_ledger.is_empty());
        Ok(migrated)
    }
}

/// Append an absent Node activation marker; never fabricate activated state.
pub struct MigrationV11ToV12;
impl CheckpointMigration for MigrationV11ToV12 {
    fn from_version(&self) -> u32 { 11 }
    fn to_version(&self) -> u32 { 12 }
    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let _: legacy::state_v11::State = decode_exact(body).map_err(|e| MigrationError::Decode { version: 11, error: e.to_string() })?;
        let mut migrated = body.to_vec(); migrated.push(0);
        let current: legacy::state_v12::State = decode_exact(&migrated).map_err(|e| MigrationError::Encode { version: 12, error: e.to_string() })?;
        debug_assert!(current.node_bond_registry.is_none());
        Ok(migrated)
    }
}

/// Frozen v12 appends an absent receipt store. No accepted claims are invented.
pub struct MigrationV12ToV13;
impl CheckpointMigration for MigrationV12ToV13 {
    fn from_version(&self) -> u32 { 12 }
    fn to_version(&self) -> u32 { 13 }
    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let _: legacy::state_v12::State = decode_exact(body).map_err(|e| MigrationError::Decode { version:12, error:e.to_string() })?;
        let mut migrated = body.to_vec(); migrated.push(0);
        let _: legacy::state_v13::State = decode_exact(&migrated).map_err(|e| MigrationError::Encode { version:13, error:e.to_string() })?;
        Ok(migrated)
    }
}

/// S4 never upgrades a chain that activated an incomplete S2/S3 slice.
pub struct MigrationV13ToV14;
impl CheckpointMigration for MigrationV13ToV14 {
    fn from_version(&self) -> u32 { 13 }
    fn to_version(&self) -> u32 { 14 }
    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let frozen: legacy::state_v13::State = decode_exact(body).map_err(|e| MigrationError::Decode { version:13, error:e.to_string() })?;
        if frozen.node_bond_registry.is_some() || frozen.node_receipt_registry.is_some() {
            return Err(MigrationError::Decode { version:13, error:"activated incomplete Node slice cannot migrate into settlement".into() });
        }
        let mut migrated = body.to_vec(); migrated.push(0);
        let _: legacy::state_v14::State = decode_exact(&migrated).map_err(|e| MigrationError::Encode { version:14, error:e.to_string() })?;
        Ok(migrated)
    }
}

/// S5 cannot invent sanctions/history for a chain activated under partial S4.
pub struct MigrationV14ToV15;
impl CheckpointMigration for MigrationV14ToV15 {
    fn from_version(&self) -> u32 { 14 }
    fn to_version(&self) -> u32 { 15 }
    fn migrate(&self, body: &[u8]) -> Result<Vec<u8>, MigrationError> {
        let frozen: legacy::state_v14::State = decode_exact(body).map_err(|e| MigrationError::Decode { version:14, error:e.to_string() })?;
        if frozen.node_bond_registry.is_some() || frozen.node_receipt_registry.is_some() || frozen.node_settlement.is_some() {
            return Err(MigrationError::Decode { version:14, error:"activated incomplete Node S4 cannot migrate into sanctions".into() });
        }
        let mut migrated = body.to_vec(); migrated.push(0);
        let _: State = decode_exact(&migrated).map_err(|e| MigrationError::Encode { version:15, error:e.to_string() })?;
        Ok(migrated)
    }
}

/// Keep the deployed writer while the only new gated store is empty.
/// This is a checkpoint codec, never a signing preimage. Frozen format10
/// validation prevents an accidental field reorder from silently truncating.
pub(super) fn encode_compatible(state: &State) -> Result<Vec<u8>, bincode::Error> {
    let mut payload = bincode::serialize(state)?;
    if state.node_fraud.is_none() {
        if state.node_settlement.is_some() { return Err(Box::new(bincode::ErrorKind::Custom("incomplete Node sanctions marker".into()))); }
        assert_eq!(payload.pop(), Some(0), "absent Node sanctions must be final");
        let _: legacy::state_v14::State = decode_exact(&payload)?;
    }
    if state.node_settlement.is_none() {
        if state.node_receipt_registry.is_some() {
            return Err(Box::new(bincode::ErrorKind::Custom("incomplete Node settlement marker".into())));
        }
        assert_eq!(payload.pop(), Some(0), "absent Node settlement must be final");
        let _: legacy::state_v13::State = decode_exact(&payload)?;
    }
    let version = if state.node_settlement.is_some() { CHECKPOINT_FORMAT_VERSION } else {
        assert_eq!(payload.pop(), Some(0), "absent Node receipt store must be final");
        let _: legacy::state_v12::State = decode_exact(&payload)?;
        if state.node_bond_registry.is_some() { 12 } else {
        assert_eq!(payload.pop(), Some(0), "absent Node registry must be the final field");
        let _: legacy::state_v11::State = decode_exact(&payload)?;
        if state.station_owner_ledger.is_empty() {
        let suffix = 0u64.to_le_bytes();
        assert!(
            payload.ends_with(&suffix),
            "empty owner ledger must be the final field"
        );
        payload.truncate(payload.len() - suffix.len());
        let _: legacy::state_v10::State = decode_exact(&payload)?;
        10
        } else { 11 }
        }
    };
    Ok(envelope(version, &payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden(version: u32) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(format!("checkpoint_state_v{version}_golden.bin"));
        std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
    }

    fn body(envelope: &[u8]) -> &[u8] {
        &envelope[CHECKPOINT_ENVELOPE_MAGIC.len() + 4..]
    }

    /// The format-9 append: an empty applied-extension list (#3277 S2).
    fn appended_extensions() -> Vec<u8> {
        bincode::serialize(&Vec::<AppliedExtension>::new()).expect("serialize empty extension list")
    }

    /// The format-10 append: an empty accepting-block-time map (#3270).
    fn appended_station_block_time() -> Vec<u8> {
        bincode::serialize(&BTreeMap::<String, i64>::new())
            .expect("serialize empty station block-time map")
    }

    /// Everything appended after the paymaster map: formats 9 and 10.
    fn appended_owner_ledger() -> Vec<u8> {
        0u64.to_le_bytes().to_vec()
    }

    fn appended_after_paymasters() -> Vec<u8> {
        [
            appended_extensions(),
            appended_station_block_time(),
            appended_owner_ledger(),
            vec![0, 0, 0, 0],
        ]
        .concat()
    }

    #[test]
    fn registry_is_a_consecutive_chain_ending_at_the_current_version() {
        for m in MIGRATIONS {
            assert_eq!(m.to_version(), m.from_version() + 1, "migrations are N -> N+1");
            assert!(migration_from(m.from_version()).is_some());
        }
        assert!(has_path(CHECKPOINT_FORMAT_VERSION), "identity path");
        assert!(has_path(3), "v3 -> v4 -> v5 -> v6 -> v7 -> v8 is registered");
        assert!(has_path(4), "v4 -> v5 -> v6 -> v7 -> v8 is registered");
        assert!(has_path(5), "v5 -> v6 -> v7 -> v8 is registered");
        assert!(has_path(6), "v6 -> v7 -> v8 is registered");
        assert!(has_path(7), "v7 -> v8 -> v9 is registered");
        assert!(has_path(8), "v8 -> v9 -> v10 is registered");
        assert!(has_path(9), "v9 -> v10 is registered");
        assert!(!has_path(2), "v2 -> v3 is not registered (mid-struct type change, #3091)");
        assert!(!has_path(CHECKPOINT_FORMAT_VERSION + 1), "newer than this binary");
    }

    #[test]
    fn frozen_v3_layout_decodes_the_v3_golden_exactly() {
        // The tripwire: if any nested type the frozen layout points at
        // changes shape, this stops decoding and the change is loud.
        let v3 = golden(3);
        let frozen: legacy::state_v3::State = decode_exact(body(&v3)).expect("frozen v3 decode");
        assert_eq!(frozen.oracle_quorum, 1);
        assert_eq!(frozen.pending_commits.len(), 1);
    }

    #[test]
    fn v3_golden_migrates_to_a_current_envelope_that_decodes() {
        let v3 = golden(3);
        let migrated = migrate_to_current(3, body(&v3)).expect("v3 -> v4 -> v5 -> v6 -> v7 -> v8");
        assert_eq!(&migrated[..CHECKPOINT_ENVELOPE_MAGIC.len()], &CHECKPOINT_ENVELOPE_MAGIC);
        let version = u32::from_le_bytes(migrated[8..12].try_into().unwrap());
        assert_eq!(version, CHECKPOINT_FORMAT_VERSION);
        let state: State = decode_exact(body(&migrated)).expect("current decode");
        assert!(state.last_producer_settlement.is_none());
        assert!(state.account_recovery.is_empty());
        assert!(state.session_keys.is_empty());
        assert!(state.authorization_declarations.is_empty());
        assert!(state.paymasters.is_empty());
        assert_eq!(state.oracle_quorum, 1);
        assert_eq!(state.pending_commits.len(), 1);
        // The migrated body is the v3 body plus exactly the one appended
        // defaults: bincode is positional, so the prefix is byte-identical.
        assert_eq!(&body(&migrated)[..body(&v3).len()], body(&v3));
        let appended_recovery =
            bincode::serialize(&BTreeMap::<Address, AccountRecoveryState>::new())
                .expect("serialize empty recovery map");
        let appended_session_keys =
            bincode::serialize(&BTreeMap::<Address, OwnerSessionKeys>::new())
                .expect("serialize empty session-key map");
        let appended_declarations = bincode::serialize(
            &BTreeMap::<Address, AccountAuthorizationDeclaration>::new(),
        )
        .expect("serialize empty declaration map");
        let appended_paymasters =
            bincode::serialize(&BTreeMap::<Address, PaymasterAccount>::new())
                .expect("serialize empty paymaster map");
        assert_eq!(
            body(&migrated).len(),
            body(&v3).len() + 1 + appended_recovery.len() + appended_session_keys.len()
                + appended_declarations.len() + appended_paymasters.len()
                + appended_after_paymasters().len()
        );
        assert_eq!(
            &body(&migrated)
                [body(&v3).len() + 1..body(&v3).len() + 1 + appended_recovery.len()],
            appended_recovery.as_slice()
        );
        assert_eq!(
            &body(&migrated)[body(&v3).len() + 1 + appended_recovery.len()..
                body(&v3).len() + 1 + appended_recovery.len() + appended_session_keys.len()],
            appended_session_keys.as_slice()
        );
        let declarations_start =
            body(&v3).len() + 1 + appended_recovery.len() + appended_session_keys.len();
        let paymasters_start = declarations_start + appended_declarations.len();
        assert_eq!(
            &body(&migrated)[declarations_start..paymasters_start],
            appended_declarations.as_slice()
        );
        let extensions_start = paymasters_start + appended_paymasters.len();
        assert_eq!(
            &body(&migrated)[paymasters_start..extensions_start],
            appended_paymasters.as_slice()
        );
        assert_eq!(&body(&migrated)[extensions_start..], appended_after_paymasters().as_slice());
    }

    #[test]
    fn frozen_v4_layout_decodes_the_v4_golden_exactly() {
        let v4 = golden(4);
        let frozen: legacy::state_v4::State =
            decode_exact(body(&v4)).expect("frozen v4 decode");
        assert_eq!(frozen.oracle_quorum, 1);
        assert_eq!(frozen.pending_commits.len(), 1);
        assert!(frozen.last_producer_settlement.is_none());
    }

    #[test]
    fn v4_golden_migrates_to_an_empty_account_recovery_map() {
        let v4 = golden(4);
        let migrated = migrate_to_current(4, body(&v4)).expect("v4 -> v5 -> v6 -> v7 -> v8");
        assert_eq!(u32::from_le_bytes(migrated[8..12].try_into().unwrap()), CHECKPOINT_FORMAT_VERSION);
        let state: State = decode_exact(body(&migrated)).expect("current decode");
        assert!(state.account_recovery.is_empty());
        assert!(state.session_keys.is_empty());
        assert!(state.authorization_declarations.is_empty());
        assert!(state.paymasters.is_empty());
        assert_eq!(&body(&migrated)[..body(&v4).len()], body(&v4));
        let appended_recovery =
            bincode::serialize(&BTreeMap::<Address, AccountRecoveryState>::new())
                .expect("serialize empty recovery map");
        let appended_session_keys =
            bincode::serialize(&BTreeMap::<Address, OwnerSessionKeys>::new())
                .expect("serialize empty session-key map");
        let appended_declarations = bincode::serialize(&BTreeMap::<Address, AccountAuthorizationDeclaration>::new()).expect("serialize empty declaration map");
        let appended_paymasters = bincode::serialize(&BTreeMap::<Address, PaymasterAccount>::new()).expect("serialize empty paymaster map");
        assert_eq!(
            body(&migrated).len(),
            body(&v4).len() + appended_recovery.len() + appended_session_keys.len()
                + appended_declarations.len() + appended_paymasters.len()
                + appended_after_paymasters().len()
        );
        assert_eq!(
            &body(&migrated)[body(&v4).len()..body(&v4).len() + appended_recovery.len()],
            appended_recovery.as_slice()
        );
        assert_eq!(
            &body(&migrated)[body(&v4).len() + appended_recovery.len()..body(&v4).len() + appended_recovery.len() + appended_session_keys.len()],
            appended_session_keys.as_slice()
        );
        let declarations_start = body(&v4).len() + appended_recovery.len() + appended_session_keys.len();
        let paymasters_start = declarations_start + appended_declarations.len();
        assert_eq!(&body(&migrated)[declarations_start..paymasters_start], appended_declarations.as_slice());
        let extensions_start = paymasters_start + appended_paymasters.len();
        assert_eq!(&body(&migrated)[paymasters_start..extensions_start], appended_paymasters.as_slice());
        assert_eq!(&body(&migrated)[extensions_start..], appended_after_paymasters().as_slice());
    }

    #[test]
    fn frozen_v5_layout_decodes_the_v5_golden_exactly() {
        let v5 = golden(5);
        let frozen: legacy::state_v5::State =
            decode_exact(body(&v5)).expect("frozen v5 decode");
        assert!(frozen.account_recovery.is_empty());
    }

    #[test]
    fn v5_golden_migrates_to_an_empty_session_key_map() {
        let v5 = golden(5);
        let migrated = migrate_to_current(5, body(&v5)).expect("v5 -> v6 -> v7 -> v8");
        assert_eq!(u32::from_le_bytes(migrated[8..12].try_into().unwrap()), CHECKPOINT_FORMAT_VERSION);
        let state: State = decode_exact(body(&migrated)).expect("current decode");
        assert!(state.session_keys.is_empty());
        assert!(state.authorization_declarations.is_empty());
        assert!(state.paymasters.is_empty());
        assert_eq!(&body(&migrated)[..body(&v5).len()], body(&v5));
        let appended_session_keys =
            bincode::serialize(&BTreeMap::<Address, OwnerSessionKeys>::new())
                .expect("serialize empty session-key map");
        let appended_declarations = bincode::serialize(&BTreeMap::<Address, AccountAuthorizationDeclaration>::new()).expect("serialize empty declaration map");
        let appended_paymasters = bincode::serialize(&BTreeMap::<Address, PaymasterAccount>::new()).expect("serialize empty paymaster map");
        assert_eq!(
            &body(&migrated)[body(&v5).len()..],
            &[
                appended_session_keys.as_slice(),
                appended_declarations.as_slice(),
                appended_paymasters.as_slice(),
                appended_extensions().as_slice(),
                appended_station_block_time().as_slice(),
                appended_owner_ledger().as_slice(),
                &[0, 0, 0, 0], // absent bond, receipt, settlement and fraud stores
            ].concat()
        );
    }

    #[test]
    fn frozen_v6_layout_decodes_the_v6_golden_exactly() {
        let v6 = golden(6);
        let frozen: legacy::state_v6::State =
            decode_exact(body(&v6)).expect("frozen v6 decode");
        assert!(frozen.session_keys.is_empty());
    }

    #[test]
    fn v6_golden_migrates_to_empty_declaration_and_paymaster_maps() {
        let v6 = golden(6);
        let migrated = migrate_to_current(6, body(&v6)).expect("v6 -> v7 -> v8");
        let state: State = decode_exact(body(&migrated)).expect("current decode");
        assert!(state.authorization_declarations.is_empty());
        assert!(state.paymasters.is_empty());
        assert_eq!(&body(&migrated)[..body(&v6).len()], body(&v6));
        let appended_declarations = bincode::serialize(
            &BTreeMap::<Address, AccountAuthorizationDeclaration>::new(),
        )
        .expect("serialize empty declaration map");
        let appended_paymasters =
            bincode::serialize(&BTreeMap::<Address, PaymasterAccount>::new())
                .expect("serialize empty paymaster map");
        assert_eq!(
            &body(&migrated)[body(&v6).len()..],
            &[
                appended_declarations.as_slice(),
                appended_paymasters.as_slice(),
                appended_extensions().as_slice(),
                appended_station_block_time().as_slice(),
                appended_owner_ledger().as_slice(),
                &[0, 0, 0, 0], // absent bond, receipt, settlement and fraud stores
            ]
            .concat()
        );
    }

    #[test]
    fn frozen_v7_layout_decodes_the_v7_golden_exactly() {
        let v7 = golden(7);
        let frozen: legacy::state_v7::State =
            decode_exact(body(&v7)).expect("frozen v7 decode");
        assert!(frozen.authorization_declarations.is_empty());
    }

    #[test]
    fn v7_golden_migrates_by_appending_exactly_an_empty_paymaster_map() {
        let v7 = golden(7);
        let migrated = migrate_to_current(7, body(&v7)).expect("v7 -> v8");
        let state: State = decode_exact(body(&migrated)).expect("current decode");
        assert!(state.paymasters.is_empty());
        assert_eq!(&body(&migrated)[..body(&v7).len()], body(&v7));
        let appended_paymasters =
            bincode::serialize(&BTreeMap::<Address, PaymasterAccount>::new())
                .expect("serialize empty paymaster map");
        assert_eq!(
            &body(&migrated)[body(&v7).len()..],
            &[appended_paymasters.as_slice(), appended_after_paymasters().as_slice()].concat()
        );
    }

    #[test]
    fn frozen_v8_layout_decodes_the_v8_golden_exactly() {
        let v8 = golden(8);
        let frozen: legacy::state_v8::State =
            decode_exact(body(&v8)).expect("frozen v8 decode");
        assert!(frozen.paymasters.is_empty());
    }

    #[test]
    fn v8_golden_migrates_by_appending_exactly_an_empty_extension_list() {
        let v8 = golden(8);
        let migrated = migrate_to_current(8, body(&v8)).expect("v8 -> v9");
        assert_eq!(
            u32::from_le_bytes(migrated[8..12].try_into().unwrap()),
            CHECKPOINT_FORMAT_VERSION
        );
        let state: State = decode_exact(body(&migrated)).expect("current decode");
        assert!(state.feature_schedule_extensions.is_empty());
        assert!(state.station_weather_accepted_block_unix.is_empty());
        assert_eq!(&body(&migrated)[..body(&v8).len()], body(&v8));
        assert_eq!(&body(&migrated)[body(&v8).len()..], appended_after_paymasters().as_slice());
        // An empty list costs exactly its u64 length prefix.
        assert_eq!(appended_extensions(), 0u64.to_le_bytes().to_vec());
    }

    #[test]
    fn frozen_v9_layout_decodes_the_v9_golden_exactly() {
        let v9 = golden(9);
        let frozen: legacy::state_v9::State =
            decode_exact(body(&v9)).expect("frozen v9 decode");
        assert!(frozen.feature_schedule_extensions.is_empty());
    }

    /// #3270: format 10 appends exactly one empty
    /// `station_weather_accepted_block_unix` map, and the v9 prefix is
    /// byte-identical.
    #[test]
    fn v9_golden_migrates_by_appending_exactly_an_empty_station_block_time_map() {
        let v9 = golden(9);
        let migrated = migrate_to_current(9, body(&v9)).expect("v9 -> v10");
        assert_eq!(
            u32::from_le_bytes(migrated[8..12].try_into().unwrap()),
            CHECKPOINT_FORMAT_VERSION
        );
        let state: State = decode_exact(body(&migrated)).expect("current decode");
        assert!(state.station_weather_accepted_block_unix.is_empty());
        assert!(state.feature_schedule_extensions.is_empty());
        assert_eq!(&body(&migrated)[..body(&v9).len()], body(&v9));
        assert_eq!(
            &body(&migrated)[body(&v9).len()..],
            [appended_station_block_time(), appended_owner_ledger(), vec![0, 0, 0, 0]]
                .concat()
                .as_slice()
        );
        // An empty map costs exactly its u64 length prefix.
        assert_eq!(appended_station_block_time(), 0u64.to_le_bytes().to_vec());
    }

    #[test]
    fn a_v10_body_mislabelled_as_v9_is_rejected_by_the_trailing_bytes_check() {
        let v10 = golden(10);
        match migrate_to_current(9, body(&v10)) {
            Err(MigrationError::Decode { version: 9, .. }) => {},
            other => panic!("expected Decode (trailing bytes), got {other:?}"),
        }
    }

    #[test]
    fn frozen_v10_weather_body_migrates_to_v11_without_moving_any_byte() {
        let v9 = golden(9);
        let mut weather = BTreeMap::new();
        weather.insert("station-main-weather".to_string(), 1_790_000_000i64);
        let v10_body = [body(&v9), &bincode::serialize(&weather).unwrap()].concat();
        let frozen: legacy::state_v10::State = decode_exact(&v10_body).unwrap();
        assert_eq!(frozen.station_weather_accepted_block_unix, weather);
        let migrated = migrate_to_current(10, &v10_body).unwrap();
        assert_eq!(&body(&migrated)[..v10_body.len()], &v10_body);
        assert_eq!(&body(&migrated)[v10_body.len()..], &[0; 12]);
        let current: State = decode_exact(body(&migrated)).unwrap();
        assert_eq!(current.station_weather_accepted_block_unix, weather);
        assert!(current.station_owner_ledger.is_empty());
        assert_eq!(
            encode_compatible(&current).unwrap(),
            envelope(10, &v10_body)
        );
        let mut extra = v10_body.clone();
        extra.push(0);
        for invalid in [
            &v10_body[..v10_body.len() - 1],
            extra.as_slice(),
            body(&migrated),
        ] {
            assert!(matches!(
                migrate_to_current(10, invalid),
                Err(MigrationError::Decode { version: 10, .. })
            ));
        }
    }

    #[test]
    fn v11_golden_appends_only_the_empty_owner_ledger_to_deployed_v10() {
        let v10 = golden(10);
        let v11 = golden(11);
        let _: legacy::state_v10::State = decode_exact(body(&v10)).unwrap();
        assert_eq!(&body(&v11)[..body(&v10).len()], body(&v10));
        assert_eq!(&body(&v11)[body(&v10).len()..], 0u64.to_le_bytes());
        assert_eq!(MigrationV10ToV11.migrate(body(&v10)).unwrap(), body(&v11));
        assert_eq!(migrate_to_current(11, body(&v11)).unwrap(), golden(15));
        assert_eq!(&body(&golden(15))[body(&v11).len()..], &[0, 0, 0, 0]);
    }

    #[test]
    fn v13_off_migrates_exactly_and_prior_node_activation_is_refused() {
        let v13=golden(13);
        assert_eq!(migrate_to_current(13,body(&v13)).unwrap(),golden(15));
        for receipt_marker in [false,true] {
            // Independent frozen suffix: bond Option tag/body, then the S3
            // receipt Option tag and its original three-field empty body.
            let old=body(&v13);
            assert_eq!(&old[old.len()-2..],&[0,0]);
            let mut bytes=old[..old.len()-2].to_vec();
            bytes.push(1);
            let bond=wrfcoin_shared_protocol::consensus::node_bond_registry::NodeBondRegistry::begin(3,[7;32],3,[2;32]).unwrap();
            bytes.extend(bincode::serialize(&bond).unwrap());
            bytes.push(u8::from(receipt_marker));
            if receipt_marker {
                bytes.extend(3u64.to_le_bytes());
                bytes.extend(0u64.to_le_bytes());
                bytes.extend(0u64.to_le_bytes());
            }
            let frozen:legacy::state_v13::State=decode_exact(&bytes).unwrap();
            assert!(frozen.node_bond_registry.is_some());
            assert_eq!(frozen.node_receipt_registry.is_some(),receipt_marker);
            assert!(matches!(migrate_to_current(13,&bytes),Err(MigrationError::Decode {version:13,..})));
        }
    }

    #[test]
    fn identity_at_the_current_version_is_a_no_op() {
        let current = golden(CHECKPOINT_FORMAT_VERSION);
        let out = migrate_to_current(CHECKPOINT_FORMAT_VERSION, body(&current)).unwrap();
        assert_eq!(out, current);
    }

    #[test]
    fn v2_golden_has_no_path_and_is_never_decoded() {
        let v2 = golden(2);
        assert_eq!(
            migrate_to_current(2, body(&v2)),
            Err(MigrationError::NoPath {
                from: 2,
                to: CHECKPOINT_FORMAT_VERSION
            })
        );
    }

    #[test]
    fn newer_than_this_binary_has_no_path() {
        assert_eq!(
            migrate_to_current(CHECKPOINT_FORMAT_VERSION + 1, &[0u8; 16]),
            Err(MigrationError::NoPath {
                from: CHECKPOINT_FORMAT_VERSION + 1,
                to: CHECKPOINT_FORMAT_VERSION
            })
        );
    }

    #[test]
    fn garbage_or_truncated_v3_body_is_a_decode_error_not_a_migration() {
        let v3 = golden(3);
        let truncated = &body(&v3)[..body(&v3).len() / 2];
        match migrate_to_current(3, truncated) {
            Err(MigrationError::Decode { version: 3, .. }) => {},
            other => panic!("expected Decode, got {other:?}"),
        }
        match migrate_to_current(3, &[0xFFu8; 64]) {
            Err(MigrationError::Decode { version: 3, .. }) => {},
            other => panic!("expected Decode, got {other:?}"),
        }
    }

    #[test]
    fn a_v4_body_mislabelled_as_v3_is_rejected_by_the_trailing_bytes_check() {
        // Without `reject_trailing_bytes` a v4 body would decode as v3 with
        // one byte left over and the migration would append a SECOND field.
        let v4 = golden(4);
        match migrate_to_current(3, body(&v4)) {
            Err(MigrationError::Decode { version: 3, .. }) => {},
            other => panic!("expected Decode (trailing bytes), got {other:?}"),
        }
    }
}
