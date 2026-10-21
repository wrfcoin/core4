//! Deterministic replay primitive for the determinism-assurance epic (#2102, story #2103).
//!
//! This module is the *one canonical input* the rest of determinism testing builds on.
//! It replays a committed, human-reviewable fixture (a set of opening account balances
//! plus an ordered sequence of block operations) through the real
//! [`State::apply_transition`](wrfcoin_shared_protocol::blockchain::state::State) path and
//! records the resulting `state_root` at every height.
//!
//! ## Reuse contract (why this is `pub`)
//!
//! [`replay_fixture`] is deliberately public so consumers do **not** duplicate replay logic:
//!
//! - **#2104** (`determinism-matrix.yml`) runs `replay_fixture` on each OS/arch/profile leg,
//!   serialises the returned `Vec<(u64, StateRoot)>` to `roots-<leg>.json`, and a compare job
//!   fails if any two legs disagree at any height.
//! - **#2183** (data-availability epic) and **#2204** (DePIN epic) reuse the same fixture and
//!   `replay_fixture` as their determinism baseline rather than minting a second fixture.
//!
//! ## Coverage
//!
//! The fixture exercises every transaction-type family the node supports as a state
//! transition: value transfer, staking (lock/unstake), reward, slashing, **weather-observation
//! commit + reveal** (`RegisterCommit`/`ApplyReveal`), and the **oracle-packet path**
//! (`SetOracleConfig` + signed `RecordOracleAttestation`). Oracle keys are derived
//! deterministically from single-byte seeds ([`oracle_keypair`]) so attestations are validly
//! signed without an RNG and reproduce identically on every platform.
//!
//! To extend the fixture with a new operation, add a variant to [`FixtureOp`], map it to a
//! [`StateTransition`] in [`FixtureOp::apply`], append lines to `blocks.jsonl`, and regenerate
//! `expected_roots.json` with `REGENERATE_DETERMINISM_FIXTURE=1`. The interface — genesis +
//! ordered ops + per-height roots — does not change, so downstream consumers are unaffected.
//!
//! **The fixture's opening balances are not a chain genesis allocation.** See [`Genesis`]
//! for why (#2962): the chain has no allocation field and a fresh cut starts every account
//! at zero.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, Transaction,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

/// A state root, as the lowercase hex string produced by `Hash`'s `Display`.
pub type StateRoot = String;

/// Environment variable that, when set to `1`, rewrites `expected_roots.json` from the
/// actual replay instead of asserting against it. Regenerating is a **reviewable change**,
/// never a fix for a failing test — a diff to `expected_roots.json` must be explained in its PR.
pub const REGENERATE_ENV: &str = "REGENERATE_DETERMINISM_FIXTURE";

/// One opening account balance **in this test fixture's replay** (#2962).
///
/// This is *not* a chain genesis allocation and there is no production path that
/// reads it. See [`Genesis`] for why that distinction matters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAccount {
    /// 20-byte address, hex, `0x`-prefixed.
    pub address: String,
    /// Initial balance in base units.
    pub balance: u64,
}

/// The deterministic replay's starting ledger: a fixed set of opening balances.
/// No timestamps, no randomness.
///
/// # This is not the chain's genesis (#2962)
///
/// **The chain has no genesis allocation mechanism.** This type and
/// [`GenesisAccount`] exist only to give `replay_fixture` a reviewable starting
/// state; nothing outside this module constructs or reads them, and no production
/// code path can. A fresh genesis cut therefore starts **every** account at zero.
///
/// The distinction is called out here because this is the repository's single most
/// likely false positive for "genesis allocation exists": the two structs are
/// exactly the shape someone searching for that mechanism would expect to find.
/// What genesis actually writes is `State::seed_genesis_producer_set` (producer
/// history only) and `set_genesis_oracle_declaration` (oracle set + quorum) —
/// neither creates an account. The production genesis artefact cannot be extended
/// by hand either: `GenesisProducerSetFile` / `GenesisProducerEntry` in
/// `native-chain/src/bin/launch_api/genesis_producers.rs` are
/// `#[serde(deny_unknown_fields)]`, so a `"balance"` key is a startup failure.
///
/// Do not rename these types into production naming, and do not wire them to
/// `State` — implementing real allocations is a consensus change (the set must
/// fold byte-identically into `state_root` on every node, and `supply_genesis`
/// must be re-baselined or `INV-SUPPLY-1` halts the chain), not a reuse of this
/// fixture schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Genesis {
    pub accounts: Vec<GenesisAccount>,
}

/// A single fixture operation. This is a stable, human-reviewable schema that maps onto a
/// [`StateTransition`]; keeping our own enum (rather than serialising `StateTransition`
/// directly) means the fixture JSON stays readable and does not churn when the internal
/// transition enum grows unrelated variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FixtureOp {
    /// Credit `amount` to `address` (creates the account if absent).
    Reward {
        address: String,
        amount: u64,
        reason: String,
    },
    /// Move `amount` from spendable to locked stake.
    Stake { address: String, amount: u64 },
    /// Move `amount` from locked stake back to spendable.
    Unstake { address: String, amount: u64 },
    /// Signed value transfer from `from` to `to` at `nonce`.
    Transfer {
        from: String,
        to: String,
        amount: u64,
        nonce: u64,
    },
    /// Reduce `address` stake by `amount` (slashing).
    Slash {
        address: String,
        amount: u64,
        reason: String,
    },
    /// Weather-observation commit: register a commit whose id is derived from the
    /// (data, nonce, station, timestamp, type) — the reveal must repeat the same fields.
    Commit {
        station_id: String,
        timestamp: i64,
        contribution_type: String,
        data: serde_json::Value,
        nonce: String,
    },
    /// Weather-observation reveal: reveal the data behind a prior [`FixtureOp::Commit`].
    Reveal {
        station_id: String,
        timestamp: i64,
        contribution_type: String,
        data: serde_json::Value,
        nonce: String,
    },
    /// Register the oracle set from deterministic seeds (each seed → a fixed ed25519 key).
    SetOracleConfig {
        oracle_seeds: Vec<u8>,
        quorum: usize,
    },
    /// Record a signed oracle attestation from the oracle identified by `oracle_seed`.
    OracleAttest {
        oracle_seed: u8,
        commit_id: String,
        approved: bool,
        confidence: f64,
        timestamp: i64,
    },
}

/// Derive a deterministic ed25519 keypair from a single-byte seed (32 copies of the byte).
/// Same seed → same key on every platform, so the fixture stays reproducible without an RNG.
fn oracle_keypair(seed: u8) -> (PrivateKey, String) {
    let sk = PrivateKey::from_bytes(&[seed; 32]).expect("32-byte seed is a valid private key");
    let oracle_id = PublicKey::from_private_key(&sk).to_string();
    (sk, oracle_id)
}

impl FixtureOp {
    fn apply(&self, state: &mut State, block_height: u64) -> Result<(), String> {
        let t = match self {
            FixtureOp::Reward {
                address,
                amount,
                reason,
            } => StateTransition::ApplyReward {
                address: parse_addr(address)?,
                amount: Balance::new(*amount as u128),
                reason: reason.clone(),
            },
            FixtureOp::Stake { address, amount } => StateTransition::LockToStake {
                address: parse_addr(address)?,
                amount: Balance::new(*amount as u128),
            },
            FixtureOp::Unstake { address, amount } => StateTransition::ReleaseToSpendable {
                address: parse_addr(address)?,
                amount: Balance::new(*amount as u128),
            },
            FixtureOp::Transfer {
                from,
                to,
                amount,
                nonce,
            } => StateTransition::BalanceTransfer {
                from: parse_addr(from)?,
                to: parse_addr(to)?,
                amount: Balance::new(*amount as u128),
                nonce: Some(*nonce),
                current_block: block_height,
            },
            FixtureOp::Slash {
                address,
                amount,
                reason,
            } => StateTransition::Slash {
                address: parse_addr(address)?,
                amount: Balance::new(*amount as u128),
                reason: reason.clone(),
            },
            // The remaining ops build richer payloads and are applied inline below.
            FixtureOp::Commit {
                station_id,
                timestamp,
                contribution_type,
                data,
                nonce,
            } => {
                let commit_id = Transaction::compute_commit_id(
                    data,
                    nonce,
                    station_id,
                    *timestamp,
                    contribution_type,
                );
                StateTransition::RegisterCommit {
                    payload: CommitPayload {
                        commit_id,
                        station_id: station_id.clone(),
                        timestamp: *timestamp,
                        contribution_type: contribution_type.clone(),
                        auth_pubkey: None,
                        auth_signature: None,
                    },
                    block_height,
                }
            },
            FixtureOp::Reveal {
                station_id,
                timestamp,
                contribution_type,
                data,
                nonce,
            } => {
                let commit_id = Transaction::compute_commit_id(
                    data,
                    nonce,
                    station_id,
                    *timestamp,
                    contribution_type,
                );
                StateTransition::ApplyReveal {
                    payload: RevealPayload {
                        commit_id,
                        station_id: station_id.clone(),
                        timestamp: *timestamp,
                        contribution_type: contribution_type.clone(),
                        data: data.clone(),
                        nonce: nonce.clone(),
                        data_cid: None,
                        data_hash: None,
                        storage_provider: None,
                        provenance: None,
                        manifest: None,
                        verification_receipt: None,
                        compute_receipt: None,
                        quality_tier: None,
                    },
                    block_height,
                    // The fixture carries no block time ("No timestamps"). The
                    // #3270 feature is unscheduled here, so the carried header
                    // timestamp is inert and never reaches the state root.
                    block_timestamp: 0,
                }
            },
            FixtureOp::SetOracleConfig {
                oracle_seeds,
                quorum,
            } => StateTransition::SetOracleConfig {
                oracle_set: oracle_seeds.iter().map(|&s| oracle_keypair(s).1).collect(),
                oracle_quorum: *quorum,
            },
            FixtureOp::OracleAttest {
                oracle_seed,
                commit_id,
                approved,
                confidence,
                timestamp,
            } => {
                let (sk, oracle_id) = oracle_keypair(*oracle_seed);
                let nonce = format!("{oracle_id}:{commit_id}:{timestamp}");
                let mut payload = OracleAttestationPayload {
                    commit_id: commit_id.clone(),
                    oracle_id,
                    approved: *approved,
                    signature: None,
                    confidence: Some(*confidence),
                    timestamp: *timestamp,
                    nonce: Some(nonce),
                };
                let message = payload.signing_message();
                payload.signature = Some(sk.sign(message.as_bytes()).to_string());
                StateTransition::RecordOracleAttestation {
                    payload,
                    current_block: block_height,
                }
            },
        };
        state
            .apply_transition(t)
            .map_err(|e| format!("apply failed at height {block_height}: {e:?}"))
    }
}

/// One block in the fixture: an ordered list of operations applied at `height`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureBlock {
    pub height: u64,
    pub ops: Vec<FixtureOp>,
}

fn parse_addr(s: &str) -> Result<Address, String> {
    Address::from_hex(s).map_err(|e| format!("bad address {s:?}: {e:?}"))
}

/// Build the post-genesis [`State`] from `genesis.json`, root recomputed once.
fn genesis_state(dir: &Path) -> State {
    let raw = fs::read_to_string(dir.join("genesis.json")).expect("read genesis.json");
    let genesis: Genesis = serde_json::from_str(&raw).expect("parse genesis.json");
    let mut state = State::new();
    for acct in &genesis.accounts {
        let addr = parse_addr(&acct.address).expect("genesis address");
        state.create_account(addr, Balance::new(acct.balance as u128));
    }
    state.recalculate_state_root();
    state
}

/// Load the ordered block sequence from `blocks.jsonl` (one JSON object per line).
fn load_blocks(dir: &Path) -> Vec<FixtureBlock> {
    let raw = fs::read_to_string(dir.join("blocks.jsonl")).expect("read blocks.jsonl");
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<FixtureBlock>(l).expect("parse blocks.jsonl line"))
        .collect()
}

/// Replay the fixture at `dir` from genesis and return `(height, state_root)` for **every**
/// height — genesis (height 0) first, then one entry per block in `blocks.jsonl`.
///
/// Panics (rather than returning `Result`) if a fixture operation fails to apply, because a
/// fixture that does not apply cleanly is a broken fixture, not a runtime condition to handle.
/// This is the reuse entry point for #2104 / #2183 / #2204.
pub fn replay_fixture(dir: &Path) -> Vec<(u64, StateRoot)> {
    let mut state = genesis_state(dir);
    let mut roots = vec![(0u64, state.state_root.to_string())];
    for block in load_blocks(dir) {
        for op in &block.ops {
            op.apply(&mut state, block.height)
                .expect("fixture op must apply");
        }
        roots.push((block.height, state.state_root.to_string()));
    }
    roots
}

/// Replay the fixture, but serialise and restore the [`State`] after `restart_after` blocks
/// (a bincode round-trip standing in for a node restart / IBD reload), then continue. The
/// returned roots MUST equal [`replay_fixture`]'s — a restart that changes any root is a
/// determinism bug in serialization.
pub fn replay_fixture_with_restart(dir: &Path, restart_after: usize) -> Vec<(u64, StateRoot)> {
    let mut state = genesis_state(dir);
    let mut roots = vec![(0u64, state.state_root.to_string())];
    for (i, block) in load_blocks(dir).into_iter().enumerate() {
        if i == restart_after {
            let bytes = bincode::serialize(&state).expect("serialize state");
            state = bincode::deserialize(&bytes).expect("deserialize state");
        }
        for op in &block.ops {
            op.apply(&mut state, block.height)
                .expect("fixture op must apply");
        }
        roots.push((block.height, state.state_root.to_string()));
    }
    roots
}

/// The recorded expected roots: `{ "<height>": "<state_root hex>" }`.
fn expected_path(dir: &Path) -> PathBuf {
    dir.join("expected_roots.json")
}

/// Assert `actual` matches the recorded `expected_roots.json` at **every** height. Panics with
/// the first divergent height and both roots. Honours [`REGENERATE_ENV`]: when set to `1`, it
/// rewrites `expected_roots.json` from `actual` and returns without asserting.
pub fn assert_root_parity(actual: &[(u64, StateRoot)], dir: &Path) {
    let path = expected_path(dir);
    if std::env::var(REGENERATE_ENV).as_deref() == Ok("1") {
        let map: BTreeMap<String, String> = actual
            .iter()
            .map(|(h, r)| (h.to_string(), r.clone()))
            .collect();
        let json = serde_json::to_string_pretty(&map).expect("serialize expected roots");
        fs::write(&path, json + "\n").expect("write expected_roots.json");
        eprintln!(
            "REGENERATED {} ({} heights) — this is a reviewable change, not a test fix",
            path.display(),
            actual.len()
        );
        return;
    }
    let raw = fs::read_to_string(&path).expect("read expected_roots.json");
    let expected: BTreeMap<String, String> =
        serde_json::from_str(&raw).expect("parse expected_roots.json");
    assert_eq!(
        actual.len(),
        expected.len(),
        "height count mismatch: replay produced {} heights, expected file has {}",
        actual.len(),
        expected.len()
    );
    for (height, root) in actual {
        let want = expected
            .get(&height.to_string())
            .unwrap_or_else(|| panic!("expected_roots.json has no entry for height {height}"));
        assert_eq!(
            root, want,
            "state_root divergence at height {height}: replay={root} expected={want}"
        );
    }
}

/// Absolute path to the committed determinism fixture directory, independent of CWD.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/determinism")
}
