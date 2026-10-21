use crate::common::types::Address;
use std::sync::LazyLock;

// Special system addresses for various registries and contracts
// SAFETY: All address literals below are valid 40-character hex strings (with 0x prefix),
// so Address::from_hex will never fail. These are compile-time constants.
pub static NFT_REGISTRY_ADDRESS: LazyLock<Address> =
    LazyLock::new(|| Address::from_hex("0x0000000000000000000000000000000000000101").unwrap());

pub static VERIFICATION_REGISTRY_ADDRESS: LazyLock<Address> =
    LazyLock::new(|| Address::from_hex("0x0000000000000000000000000000000000000102").unwrap());

pub static STATE_ROOT_ADDRESS: LazyLock<Address> =
    LazyLock::new(|| Address::from_hex("0x0000000000000000000000000000000000000001").unwrap());

pub static GOVERNANCE_ADDRESS: LazyLock<Address> =
    LazyLock::new(|| Address::from_hex("0x0000000000000000000000000000000000000103").unwrap());

pub static BRIDGE_ADDRESS: LazyLock<Address> =
    LazyLock::new(|| Address::from_hex("0x0000000000000000000000000000000000000104").unwrap());

/// Base units in one WRF — the workspace currency invariant (`10^8`).
///
/// **Every `u64`/`u128` currency quantity in this workspace is base units, never
/// coins.** Transaction `amount`, `Balance`, `total_minted_supply`, block rewards
/// and `MAX_SUPPLY` are all denominated here; the CLI divides by this constant to
/// render WRF for display (`tools/cli/WrfcoinCLI/src/output.rs`). A literal that
/// *means* a WRF amount must be written `n * WRF_UNITS_PER_COIN`, never as a bare
/// `n`.
///
/// This constant exists because that rule was violated for the block reward and
/// nothing caught it (#2485): `INITIAL_BLOCK_REWARD` was a bare `50`, so a reward
/// documented as "50 WRF" was minted as 50 base units — `0.0000005` WRF, eight
/// orders of magnitude short. Because rewards were then eight orders too small,
/// the block-time normalisation `reward * block_time / GENESIS_BLOCK_TIME` floored
/// to **zero** from height 100_000 onward, permanently. Naming the unit and
/// expressing coin-denominated constants *through* it is what stops that class
/// recurring; `emission_constants_are_denominated_in_base_units_2485` enforces it.
pub const WRF_UNITS_PER_COIN: u64 = 100_000_000;

// Block-related constants
pub const GENESIS_BLOCK_PARENT_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";
pub const BLOCK_GAS_LIMIT: u64 = 30_000_000;
pub const TARGET_BLOCK_TIME_SECONDS: u64 = 15;

/// Block time progression table: block time decreases as network matures.
/// Each entry: (activation_height, target_block_time_seconds).
/// The chain starts with long block times (bootstrapping phase) and
/// transitions to shorter times as participation increases.
///
/// # THIS TABLE IS ALSO THE EMISSION SCHEDULE (#2492)
///
/// It is no longer a throughput knob. `elapsed_seconds_at_height()` integrates
/// these rows to convert a height into wall-clock seconds, and that feeds
/// `period = elapsed / EMISSION_REDUCTION_PERIOD_SECONDS` — so the emission
/// reduction boundary a given height falls in is derived from this table.
///
/// **Editing a row changes tokenomics, not just block cadence.** Adding,
/// removing or retiming an entry moves every subsequent reduction boundary in
/// wall-clock terms and therefore changes when the chain pays less. A change
/// made for throughput reasons will silently be a monetary change.
///
/// Before touching it: `the_emission_parameters_satisfy_the_21m_constraint_2492`
/// pins `R/T × P = 7_000_000 WRF`, and
/// `reduction_boundary_is_consistent_in_each_progression_phase` asserts a live,
/// non-vacuous boundary for **every** row — so growing the table without
/// re-deriving those figures fails loudly rather than drifting.
///
/// The coupling is deliberate: denominating the period in seconds is what
/// decoupled emission from block *count* (#2492), and seconds can only be
/// recovered from heights through this table.
pub const BLOCK_TIME_PROGRESSION: &[(u64, u64)] = &[
    (0, 14_400),     // Genesis: 4 hours
    (1_000, 7_200),  // 2 hours
    (5_000, 3_600),  // 1 hour
    (25_000, 600),   // 10 minutes
    (100_000, 120),  // 2 minutes
    (500_000, 30),   // 30 seconds
    (1_000_000, 15), // 15 seconds
    (5_000_000, 5),  // 5 seconds
    (10_000_000, 1), // 1 second (terminal)
];

// Transaction-related constants
pub const BASE_TRANSACTION_GAS: u64 = 21_000;
pub const NFT_CREATION_BASE_GAS: u64 = 50_000;
pub const NFT_VERIFICATION_BASE_GAS: u64 = 35_000;
/// Declared maximum transaction `data` payload, 2 MiB.
///
/// **INERT (#2700). This constant is consumed by no code and enforces nothing.**
/// Do not read it as a protocol bound; it is a declaration only.
///
/// The **operative** control on transaction size is
/// `Config::max_transaction_size`, default **100 KB** — about **20× tighter**
/// than this value. It is checked at mempool admission
/// (`Mempool::add_transaction`, "Transaction exceeds maximum size") and fires
/// first at the same call site, so a transaction large enough to reach this
/// ceiling has already been rejected. Measured, not inferred: a 2 MiB `data`
/// payload comes back rejected by `max_transaction_size`.
///
/// Two narrower caps also bind before this one, per transaction type:
/// commit/reveal payloads at 64 KiB and oracle attestations at 32 KiB, both in
/// `mempool::pool`.
///
/// **The value is deliberately left at 2 MiB.** Re-pointing it at the operative
/// 100 KB would be a parameter change to a member of the declared-ceilings
/// family, and a size limit becomes consensus-relevant the moment anything
/// enforces it — that is a ratification decision, not a documentation fix.
/// Enforcing it as written would *loosen* nothing today but would create a
/// second, looser bound whose relationship to `max_transaction_size` nobody has
/// decided.
///
/// If you are here because you want a transaction-size limit enforced, change
/// `Config::max_transaction_size`, which already runs. See
/// `docs/security/RESOURCE_CEILINGS.md` and #2700.
pub const MAX_TRANSACTION_DATA_SIZE: usize = 2 * 1024 * 1024; // 2MB — INERT, see doc comment
pub const DEFAULT_GAS_PRICE: u64 = 1_000_000_000; // 1 Gwei
/// Base commit window at terminal block time (1s). Used by
/// `commit_window_blocks(height)` for dynamic scaling.
pub const BASE_COMMIT_WINDOW_BLOCKS: u64 = 720;
/// Base reveal window at terminal block time (1s). Used by
/// `reveal_window_blocks(height)` for dynamic scaling.
pub const BASE_REVEAL_WINDOW_BLOCKS: u64 = 1_440;
/// Wall-clock cap on commit/reveal windows (bug #884 / issue #1040) — with
/// one deliberate exception: `window_blocks_capped` gives the 3-block
/// functional floor PRECEDENCE over this cap, because a sub-3-block window is
/// unusable. At the two slowest tiers the floor therefore wins and the window
/// exceeds this cap: 3 × 14,400s = 12h at genesis and 3 × 7,200s = 6h at
/// h=1,000 (accepted 2026-07-30, core4#2622). Every tier from h=5,000 on
/// remains under this value.
pub const MAX_COMMIT_REVEAL_WALL_CLOCK_SECONDS: u64 = 14_400;
pub const DEFAULT_ORACLE_QUORUM: usize = 2;

/// Compute the BFT-safe oracle quorum for a given oracle set size (#811).
///
/// Formula: `max(2, floor(N * 2 / 3) + 1)`
///
/// This is the standard BFT 2/3+1 threshold — a majority of honest oracles must
/// agree before data is accepted. With N=12 oracles, quorum=9 means an attacker
/// needs to compromise 9 of 12 oracles (75%) to fabricate weather data.
///
/// - 1 oracle  → max(2, 0+1) = 2  (capped at minimum — single oracle is untrusted)
/// - 3 oracles → max(2, 2+1) = 3  (unanimous required)
/// - 6 oracles → max(2, 4+1) = 5
/// - 12 oracles → max(2, 8+1) = 9
/// - 100 oracles → max(2, 66+1) = 67
pub fn compute_oracle_quorum(oracle_set_size: usize) -> usize {
    (oracle_set_size * 2 / 3 + 1).max(DEFAULT_ORACLE_QUORUM)
}

/// Minimum compute work factor required for fee-less weather data submissions.
pub const MIN_COMPUTE_WORK_FACTOR: u64 = 1_000;

// Consensus-related constants
pub const MINIMUM_DIFFICULTY: u64 = 1_000;
pub const DIFFICULTY_ADJUSTMENT_WINDOW: usize = 2016; // ~2 weeks with 10 minute blocks

// Computational NFT constants
pub const MAX_NFT_METADATA_SIZE: usize = 1_048_576; // 1MB
pub const MIN_VERIFICATION_THRESHOLD: u8 = 3; // Minimum verifiers required
pub const REPUTATION_SCALE_FACTOR: f64 = 10.0;
pub const DEFAULT_ACCURACY_THRESHOLD: f64 = 0.75; // 75% accuracy required

// Networking constants
pub const DEFAULT_P2P_PORT: u16 = 6334;
pub const MAX_PEERS: usize = 50;
pub const DEFAULT_CONNECTION_TIMEOUT_MS: u64 = 5000;

// System limits
pub const MAX_BLOCK_SIZE: usize = 32 * 1024 * 1024; // 32MB — raised from 8MB for 1K+ station scaling (#841)
pub const MAX_STATE_CACHE_SIZE: usize = 100_000; // Maximum entries in state cache
pub const MEMPOOL_CAPACITY: usize = 50_000; // Maximum transactions in mempool — raised from 5K for 1K+ station scaling (#841)

// Governance defaults
pub const GOVERNANCE_DEFAULT_QUORUM_PERCENT: u64 = 33;
pub const GOVERNANCE_DEFAULT_DEPOSIT: u128 = 1_000;
pub const GOVERNANCE_DEFAULT_VOTING_PERIOD_BLOCKS: u64 = 7_200;
