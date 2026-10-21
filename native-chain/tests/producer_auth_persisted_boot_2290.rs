//! #2290 boot backstop, over the DURABLE archive and through the REAL binary.
//!
//! Two defects this file exists to keep fixed:
//!
//! 1. **The check only saw the retention window.** Production passed
//!    `blockchain.blocks().iter()`, and `Storage::load_state` restores at most
//!    `configured_max_in_memory_blocks()` blocks (default 200). An unauthorized
//!    block OLDER than that window was silently trusted on every restart — the
//!    exact gap the check exists to close. The regressions here put the rogue
//!    block *below* the window and prove it is still caught.
//!
//! 2. **The production call site was not load-bearing.** A reviewer deleted only
//!    the `main.rs` startup invocation, left the helper intact, and both helper
//!    suites stayed green. A test that calls the helper cannot detect that. The
//!    process-level tests here run the actual `launch_api` binary against a
//!    prepared database, so the only thing that can produce the asserted output
//!    is the call in `main.rs`. Delete it and they go red.
//!
//! Run: cargo test -p wrfcoin-native-chain --test producer_auth_persisted_boot_2290

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rand::SeedableRng;
use rand::rngs::StdRng;

use wrfcoin_native_chain::storage::{
    PERSISTED_BLOCK_SCAN_CHUNK, PersistedProducerAuthorityError, Storage,
    verify_persisted_producer_authority,
};
use wrfcoin_shared_protocol::blockchain::block::{Block, STATE_ROOT_METADATA_KEY};
use wrfcoin_shared_protocol::blockchain::chain::DEFAULT_MAX_IN_MEMORY_BLOCKS;
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::blockchain::transaction::{Transaction, TransactionOutput};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::consensus::block_producer_auth::sign_block_producer;
use wrfcoin_shared_protocol::consensus::producer_set::{ProducerMember, ProducerSetChange};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    GENESIS_CHAIN_ID_KEY, GENESIS_NETWORK_ID_KEY, GENESIS_PROTOCOL_VERSION_KEY, ChainIdentity,
    SigningProtocolVersion,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

// A chain identity that is NOT the `mainnet` default (V2 refuses the fallback
// identity) and NOT `testnet` (which would arm the #2035 pinned-genesis-hash
// gate under `--features p2p`).
const TEST_CHAIN_ID: &str = "wrfcoin-2290-bootcheck";
const TEST_NETWORK_ID: &str = "bootcheck";

fn identity() -> ChainIdentity {
    ChainIdentity::new(TEST_CHAIN_ID, TEST_NETWORK_ID)
}

fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

/// Canonical payout `Address` (0x-hex) derived from a key — the coinbase form
/// the authority gate compares against (§R2).
fn payout_of(pk: &PublicKey) -> String {
    let key: [u8; 32] = pk.as_bytes().try_into().unwrap();
    Address(key[0..20].try_into().unwrap()).to_string()
}

fn member_of(pk: &PublicKey, id: &str) -> ProducerMember {
    ProducerMember {
        producer_id: id.to_string(),
        payout_address: payout_of(pk),
        public_key: pk.as_bytes().try_into().unwrap(),
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wrfcoin-2290-boot-{tag}-{}-{}",
        std::process::id(),
        fastrand::u64(..)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A chain built for the **child process**: a real V2 chain whose genesis
/// records `chain_id` / `network_id` / `protocol_version=2`, and whose tip
/// commits to the checkpointed state root. `launch_api` therefore clears its
/// identity gate, its protocol-version gate and the #2219 tip-commitment gate
/// before it ever reaches the producer-authority backstop — proving the
/// backstop in the configuration a real node runs, not a degraded one.
async fn build_v2_chain(data_dir: &Path, height_count: u64, rogue_height: Option<u64>) {
    build_persisted_chain(
        data_dir,
        height_count,
        rogue_height,
        &identity(),
        SigningProtocolVersion::V2,
        true,
    )
    .await
}

/// A chain built for **in-process** verification.
///
/// `verify_persisted_producer_authority` resolves the gate through
/// `ChainIdentity::active()` / `SigningProtocolVersion::active()` — the
/// process-wide values a node installs at boot. A test binary installs nothing,
/// so it runs on the uninstalled defaults, and the chain must be signed under
/// exactly those or every block fails authentication for the wrong reason.
/// Reading them here rather than pinning V2 keeps these tests about the
/// retention window, which is what they exist to pin; the V2 posture is covered
/// end-to-end by the process-level tests below.
async fn build_local_chain(data_dir: &Path, height_count: u64, rogue_height: Option<u64>) {
    build_persisted_chain(
        data_dir,
        height_count,
        rogue_height,
        ChainIdentity::active(),
        SigningProtocolVersion::active(),
        false,
    )
    .await
}

/// Build and durably persist a chain of `height_count` blocks at
/// `<data_dir>/wrfcoin.redb`, seeded with a one-member producer set.
///
/// Every block is signed by the authorised member and pays its stable
/// `payout_address`, EXCEPT `rogue_height` (when given), which is signed by a
/// key that is not in the set — the shape a node ends up with after acquiring a
/// block through a route that was never gated.
async fn build_persisted_chain(
    data_dir: &Path,
    height_count: u64,
    rogue_height: Option<u64>,
    signing_identity: &ChainIdentity,
    signing_version: SigningProtocolVersion,
    pin_genesis_identity: bool,
) {
    let (member_sk, member_pk) = keypair(1);
    let (rogue_sk, _rogue_pk) = keypair(9_999);
    let member = member_of(&member_pk, "n5-a");

    let mut state = State::new();
    state.producer_set_history = vec![ProducerSetChange::new(0, 0, vec![member.clone()])];
    let committed_state_root = state.state_root.to_string();

    let db_path = data_dir.join("wrfcoin.redb");
    // A large interval keeps `persist_block` from serialising a checkpoint per
    // block; `force_checkpoint` below writes the single anchor `load_state`
    // reads, which is what carries `producer_set_history` across the restart.
    let mut storage = Storage::open(&db_path, 1_000_000).expect("open storage");

    let mut prev = Hash::default();
    for height in 1..=height_count {
        let mut coinbase = Transaction::new(vec![], vec![TransactionOutput {
            amount: 5_000,
            address: member.payout_address.clone(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }]);
        coinbase.set_id();

        let mut block = Block::new(3, prev, vec![coinbase], 1, height);
        if height == 1 && pin_genesis_identity {
            block
                .metadata
                .insert(GENESIS_CHAIN_ID_KEY.to_string(), TEST_CHAIN_ID.to_string());
            block.metadata.insert(
                GENESIS_NETWORK_ID_KEY.to_string(),
                TEST_NETWORK_ID.to_string(),
            );
            block
                .metadata
                .insert(GENESIS_PROTOCOL_VERSION_KEY.to_string(), "2".to_string());
        }
        if height == height_count {
            block.metadata.insert(
                STATE_ROOT_METADATA_KEY.to_string(),
                committed_state_root.clone(),
            );
        }

        // Metadata is inside `canonical_body`, so it must be final before the
        // producer signs.
        let signer = if Some(height) == rogue_height {
            &rogue_sk
        } else {
            &member_sk
        };
        let att = sign_block_producer(&block, signer, signing_identity, signing_version);
        block.producer_pubkey = att.producer_pubkey;
        block.producer_sig = att.producer_sig.to_vec();

        let hash = block.calculate_hash();
        block.set_hash(hash);
        prev = hash;

        storage
            .persist_block(block, vec![], None)
            .await
            .expect("persist block");
    }

    storage
        .force_checkpoint(&state, height_count)
        .await
        .expect("force checkpoint");
}

// ---------------------------------------------------------------------------
// #2501 — "signed blocks but NO producer set" must refuse the boot
// ---------------------------------------------------------------------------

/// Persist `height_count` UNSIGNED blocks and no producer set — a legacy or dev
/// chain that never configured producer authentication.
async fn build_unsigned_chain(data_dir: &Path, height_count: u64) {
    build_unsigned_chain_pinned(data_dir, height_count, false).await
}

/// As above, optionally pinning the genesis chain identity + protocol version so
/// the startup protocol gate passes and a LATER startup check is the thing that
/// decides the boot. Without the pin the node refuses earlier, for an unrelated
/// reason, and a process-level test would prove nothing about the check it names.
async fn build_unsigned_chain_pinned(data_dir: &Path, height_count: u64, pin_identity: bool) {
    let payout = Address::new([0x77u8; 20]).to_string();
    let state = State::new();

    let db_path = data_dir.join("wrfcoin.redb");
    let mut storage = Storage::open(&db_path, 1_000_000).expect("open storage");

    let mut prev = Hash::default();
    for height in 1..=height_count {
        let mut coinbase = Transaction::new(vec![], vec![TransactionOutput {
            amount: 5_000,
            address: payout.clone(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }]);
        coinbase.set_id();
        // No `sign_block_producer` call: producer_pubkey stays all-zero and
        // producer_sig stays empty, which is what an unconfigured chain produces.
        let mut block = Block::new(3, prev, vec![coinbase], 1, height);
        if height == 1 && pin_identity {
            block
                .metadata
                .insert(GENESIS_CHAIN_ID_KEY.to_string(), TEST_CHAIN_ID.to_string());
            block.metadata.insert(
                GENESIS_NETWORK_ID_KEY.to_string(),
                TEST_NETWORK_ID.to_string(),
            );
            block
                .metadata
                .insert(GENESIS_PROTOCOL_VERSION_KEY.to_string(), "1".to_string());
        }
        let hash = block.calculate_hash();
        block.set_hash(hash);
        prev = hash;
        storage
            .persist_block(block, vec![], None)
            .await
            .expect("persist block");
    }
    storage
        .force_checkpoint(&state, height_count)
        .await
        .expect("force checkpoint");
}

/// A chain that carries producer-signed blocks but holds NO producer set must
/// REFUSE to start.
///
/// This is the #2501 state: `producer_set_history` is genesis `State` carried by
/// no `StateTransition`, so a checkpoint-less WAL recovery loses it while the
/// signed blocks remain on disk. Every producer-authority gate then reads the
/// empty set as "this chain never configured producer auth" and enforcement is
/// silently off.
///
/// At V2 the tip state-root commitment check (#2219) already refuses this. This
/// backstop matters at **V1**, where that check returns `Ok` for any version
/// below V2 and a V1 producer stamps no commitment at all — so a pre-cut chain
/// with a seeded set has the loss with none of the detection.
///
/// The history is passed as `&[]` rather than persisted empty because that is
/// the exact production call shape: `main.rs` passes
/// `blockchain.state.producer_set_history`, and after a WAL-only recovery that
/// value IS empty while the archive still holds the signed blocks.
#[tokio::test]
async fn signed_blocks_with_no_producer_set_refuse_the_boot() {
    let dir = temp_dir("2501-signed-no-set");
    build_local_chain(&dir, 3, None).await;

    let storage = Storage::open(&dir.join("wrfcoin.redb"), 1_000_000).expect("reopen");

    // Precondition: the archive really does hold signed blocks, or the refusal
    // below would prove nothing about the discriminator.
    let chain = storage
        .load_state()
        .expect("load_state")
        .expect("a persisted chain must reload");
    let tip = chain.block_at_height(3).expect("tip in window");
    assert!(
        !tip.producer_sig.is_empty() && tip.producer_pubkey != [0u8; 32],
        "precondition: these blocks must be producer-signed"
    );

    match verify_persisted_producer_authority(&storage, &[]) {
        Err(PersistedProducerAuthorityError::SignedBlocksWithoutProducerSet { height }) => {
            assert_eq!(
                height, 1,
                "must name the LOWEST signed height, so the operator is pointed at \
                 where the chain began carrying signatures"
            );
        },
        other => panic!(
            "a chain with signed blocks and no producer set must refuse the boot \
             rather than start unenforced (#2501), got {other:?}"
        ),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// The other half, and the one that decides whether this change is safe: a chain
/// that never configured producer authentication must still **START**.
///
/// An empty producer set is the documented "not configured" signal, and legacy
/// and dev chains legitimately have one. If this refused, the fix above would
/// have bricked every unconfigured chain — a worse failure than the hole it
/// closes. Unsigned blocks are what distinguish the two states.
#[tokio::test]
async fn an_unconfigured_chain_with_unsigned_blocks_still_starts() {
    let dir = temp_dir("2501-unsigned-legacy");
    build_unsigned_chain(&dir, 3).await;

    let storage = Storage::open(&dir.join("wrfcoin.redb"), 1_000_000).expect("reopen");
    let chain = storage
        .load_state()
        .expect("load_state")
        .expect("a persisted chain must reload");

    assert!(
        chain.state.producer_set_history.is_empty(),
        "precondition: this chain must have no producer set"
    );
    let tip = chain.block_at_height(3).expect("tip in window");
    assert!(
        tip.producer_sig.is_empty() && tip.producer_pubkey == [0u8; 32],
        "precondition: these blocks must be UNSIGNED, or the test is not \
         reproducing an unconfigured chain"
    );

    let verdict = verify_persisted_producer_authority(&storage, &[]);
    assert!(
        verdict.is_ok(),
        "an unconfigured chain must still start — refusing here would brick every \
         legacy and dev chain, which is worse than the hole being closed: \
         {verdict:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Defect 1 — the scan must cover the whole durable archive, not the window
// ---------------------------------------------------------------------------

/// THE regression for defect 1, in-process and disk-backed.
///
/// 205 blocks with the default 200-block retention window puts the rogue block
/// at height 2 provably outside anything `load_state` restores — asserted, not
/// assumed, so the test cannot quietly stop reproducing the condition. The
/// window size is the shipped default here rather than an env override, because
/// the reported defect is about the default configuration.
#[tokio::test]
async fn boot_check_catches_a_rogue_block_older_than_the_retention_window() {
    let dir = temp_dir("older-than-window");
    let height_count = DEFAULT_MAX_IN_MEMORY_BLOCKS as u64 + 5;
    build_local_chain(&dir, height_count, Some(2)).await;

    let storage = Storage::open(&dir.join("wrfcoin.redb"), 1_000_000).expect("reopen");
    let blockchain = storage
        .load_state()
        .expect("load_state")
        .expect("a persisted chain must reload");

    assert!(
        blockchain.block_at_height(2).is_none(),
        "the rogue block must be OUTSIDE the in-memory window, or this test is \
         not reproducing the reported condition (in-memory blocks: {})",
        blockchain.in_memory_block_count()
    );
    assert_eq!(blockchain.height, height_count);

    match verify_persisted_producer_authority(&storage, &blockchain.state.producer_set_history) {
        Err(PersistedProducerAuthorityError::Unauthorized { height, .. }) => {
            assert_eq!(
                height, 2,
                "must name the LOWEST offending height so the operator is pointed \
                 at where the fork began"
            );
        },
        other => panic!(
            "an unauthorized block below the retention window must refuse the boot, got {other:?}"
        ),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// The positive half: an all-authorized chain boots, and the reported count is
/// the DURABLE total, not the retention window. If the scan ever regresses to
/// `blockchain.blocks()` this asserts 205 and receives 200.
#[tokio::test]
async fn boot_check_counts_every_persisted_block_not_just_the_window() {
    let dir = temp_dir("counts-all");
    let height_count = DEFAULT_MAX_IN_MEMORY_BLOCKS as u64 + 5;
    build_local_chain(&dir, height_count, None).await;

    let storage = Storage::open(&dir.join("wrfcoin.redb"), 1_000_000).expect("reopen");
    let blockchain = storage
        .load_state()
        .expect("load_state")
        .expect("a persisted chain must reload");

    assert!(
        blockchain.in_memory_block_count() < height_count as usize,
        "the window must be smaller than the chain, or the two counts cannot differ"
    );

    let checked = verify_persisted_producer_authority(&storage, &blockchain.state.producer_set_history)
        .expect("an all-authorized chain must boot");
    assert_eq!(
        checked, height_count as usize,
        "the backstop must certify every DURABLE block; {} would be the retention window",
        blockchain.in_memory_block_count()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// RETARGETED by #2501. This asserted that an empty producer set is always
/// unenforced, *"the backstop must not brick legacy or dev chains, and must not
/// pay for a scan it will not use."*
///
/// The intent was right and the **fixture was wrong**: it built a chain whose
/// every block is producer-signed and called that "a chain that never configured
/// a producer set." A signed chain is definitionally configured — signing is what
/// a seeded set causes. So the fixture was the #2501 *degraded* state (set lost
/// by checkpoint-less WAL recovery, signed blocks still on disk), and asserting
/// "unenforced" over it pinned the hole open.
///
/// Both halves of the original intent are still covered, just correctly:
///
/// * *don't brick unconfigured chains* → `an_unconfigured_chain_with_unsigned_blocks_still_starts`,
///   which uses UNSIGNED blocks, the shape such a chain actually has;
/// * *don't pay for an unused scan* → no longer true, and deliberately so. An
///   unconfigured chain now pays one chunked archive pass at boot, because
///   proving that NO block is signed requires looking at all of them. The
///   tradeoff is recorded at the call site in `storage.rs`.
///
/// Retargeted rather than deleted because its fixture tests a dimension the
/// simple case does not: 12 blocks against a small retention window, so the
/// refusal is proven to come from the WHOLE DURABLE ARCHIVE and not just the
/// in-memory window — the defect this whole file exists to keep fixed.
#[tokio::test]
async fn signed_blocks_below_the_window_with_no_producer_set_refuse_the_boot() {
    let dir = temp_dir("2501-below-window");
    // Sized like its sibling below-window tests: the default window is 200, so a
    // 12-block chain would fit entirely in memory and prove nothing.
    let height_count = DEFAULT_MAX_IN_MEMORY_BLOCKS as u64 + 5;
    build_local_chain(&dir, height_count, None).await;

    let storage = Storage::open(&dir.join("wrfcoin.redb"), 1_000_000).expect("reopen");
    let blockchain = storage
        .load_state()
        .expect("load_state")
        .expect("a persisted chain must reload");

    assert!(
        blockchain.block_at_height(1).is_none(),
        "height 1 must be OUTSIDE the in-memory window, or this is not proving the \
         scan reaches the durable archive (in-memory blocks: {})",
        blockchain.in_memory_block_count()
    );

    match verify_persisted_producer_authority(&storage, &[]) {
        Err(PersistedProducerAuthorityError::SignedBlocksWithoutProducerSet { height }) => {
            assert_eq!(
                height, 1,
                "must name the LOWEST signed height even though it is below the \
                 retention window — a window-only scan would have reported a higher \
                 one, which is the exact vacuous-check defect this file guards"
            );
        },
        other => panic!(
            "signed blocks with no producer set must refuse the boot (#2501), and \
             must be found below the window, got {other:?}"
        ),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// The streaming contract the whole-archive scan rests on: every block is
/// visited, in ascending height, and no single chunk exceeds the documented
/// bound (so the working set does not scale with chain length).
#[tokio::test]
async fn scan_persisted_blocks_streams_the_whole_archive_in_bounded_chunks() {
    let dir = temp_dir("scan-chunks");
    let height_count = PERSISTED_BLOCK_SCAN_CHUNK as u64 + 7;
    build_local_chain(&dir, height_count, None).await;

    let storage = Storage::open(&dir.join("wrfcoin.redb"), 1_000_000).expect("reopen");
    let mut seen: Vec<u64> = Vec::new();
    let mut chunk_sizes: Vec<usize> = Vec::new();
    let scanned = storage
        .scan_persisted_blocks(|chunk| {
            chunk_sizes.push(chunk.len());
            seen.extend(chunk.iter().map(|(h, _)| *h));
            std::ops::ControlFlow::Continue(())
        })
        .expect("scan");

    assert_eq!(scanned, height_count as usize);
    assert_eq!(seen, (1..=height_count).collect::<Vec<_>>());
    assert!(chunk_sizes.len() > 1, "the archive must be delivered in >1 chunk");
    assert!(
        chunk_sizes.iter().all(|n| *n <= PERSISTED_BLOCK_SCAN_CHUNK),
        "no chunk may exceed the documented bound: {chunk_sizes:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Defect 2 — the PRODUCTION CALL SITE must be load-bearing
// ---------------------------------------------------------------------------
//
// These run the built `launch_api` binary. Nothing in this crate's library can
// emit `PRODUCER_AUTH_PERSISTED_VIOLATION` on its own — only `main.rs` does —
// so removing the boot invocation turns the refusal test red, which is exactly
// what a helper-level test cannot do.

/// Spawn the real `launch_api` against a prepared data dir, capturing combined
/// output to a file so we can poll it without threads.
fn spawn_launch_api(data_dir: &Path, window: usize) -> (Child, PathBuf) {
    spawn_launch_api_env(data_dir, window, &[])
}

/// As above, with extra environment. Used by the #2501 restored-state tests,
/// which turn on `WRFCOIN_REQUIRE_PRODUCER_AUTH` and must observe the REAL
/// binary refuse — a helper-level test cannot detect a deleted call site.
fn spawn_launch_api_env(
    data_dir: &Path,
    window: usize,
    extra: &[(&str, &str)],
) -> (Child, PathBuf) {
    let (mut child, log) = spawn_launch_api_inner(data_dir, window, extra);
    let _ = &mut child;
    (child, log)
}

fn spawn_launch_api_inner(
    data_dir: &Path,
    window: usize,
    extra: &[(&str, &str)],
) -> (Child, PathBuf) {
    let log_path = data_dir.join("node.log");
    let out = std::fs::File::create(&log_path).expect("log file");
    let err = out.try_clone().expect("dup log file");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_launch_api"));
    cmd.env("WRFCOIN_DATA_DIR", data_dir)
        .env("WRFCOIN_API_HOST", "127.0.0.1")
        // Ephemeral port: preflight only probes bindability, and a refused boot
        // never reaches the server anyway.
        .env("WRFCOIN_API_PORT", "0")
        .env("ALLOW_UNENCRYPTED_STORAGE", "true")
        .env("WRFCOIN_ALLOW_NO_AUTH", "1")
        .env("WRFCOIN_CHAIN_ID", TEST_CHAIN_ID)
        .env("WRFCOIN_NETWORK_ID", TEST_NETWORK_ID)
        .env("WRFCOIN_MAX_IN_MEMORY_BLOCKS", window.to_string())
        .env("RUST_LOG", "info")
        // The chain pins its version in genesis; a local override would only
        // add a way for this test to disagree with itself.
        .env_remove("WRFCOIN_SIGNING_PROTOCOL_VERSION")
        .env_remove("WRFCOIN_STORAGE_ENCRYPTION_KEY")
        .env_remove("WRFCOIN_OPERATOR_TOKEN")
        .env_remove("WRFCOIN_GENESIS_HASH")
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));

    for (k, v) in extra {
        cmd.env(k, v);
    }

    let child = cmd.spawn().expect("spawn launch_api");
    (child, log_path)
}

fn read_log(path: &Path) -> String {
    let mut s = String::new();
    if let Ok(mut f) = std::fs::File::open(path) {
        let _ = f.read_to_string(&mut s);
    }
    s
}

/// Wait for the child to exit. Returns `None` if it was still running at the
/// deadline (in which case it is killed).
fn wait_for_exit(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => return Some(status),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            },
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Poll the log for `needle` until the deadline, then kill the child.
/// Returns the log contents and whether the needle appeared.
fn wait_for_log(child: &mut Child, log: &Path, needle: &str, timeout: Duration) -> (bool, String) {
    let deadline = Instant::now() + timeout;
    loop {
        let text = read_log(log);
        if text.contains(needle) {
            let _ = child.kill();
            let _ = child.wait();
            return (true, text);
        }
        if child.try_wait().expect("try_wait").is_some() {
            return (false, read_log(log));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return (false, read_log(log));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// THE regression for defect 2, and the second proof of defect 1.
///
/// The rogue block sits at height 2 on a 12-block chain booted with a 4-block
/// retention window, so it is below anything the in-memory window holds. The
/// node must refuse to start.
///
/// Why this covers the CALL SITE: the assertions are on the exit status and the
/// stderr of the real binary. The helper cannot produce either. Delete the
/// invocation from `main.rs` and the process starts normally — red.
#[tokio::test]
async fn launch_api_refuses_to_start_on_a_persisted_rogue_block_below_the_window() {
    let dir = temp_dir("proc-refuse");
    build_v2_chain(&dir, 12, Some(2)).await;

    let (mut child, log) = spawn_launch_api(&dir, 4);
    let status = wait_for_exit(&mut child, Duration::from_secs(120));
    let text = read_log(&log);

    let status = status.unwrap_or_else(|| {
        panic!("launch_api must EXIT on a persisted unauthorized block; it kept running.\n{text}")
    });
    assert!(
        !status.success(),
        "launch_api must exit nonzero, got {status:?}.\n{text}"
    );
    assert!(
        text.contains("PRODUCER_AUTH_PERSISTED_VIOLATION"),
        "refusal must name PRODUCER_AUTH_PERSISTED_VIOLATION.\n{text}"
    );
    assert!(
        text.contains("height 2"),
        "refusal must point at the lowest offending height (2).\n{text}"
    );
    // Self-check: the node really did load a window that excludes height 2. If
    // this line ever reports a window reaching down to 2, the test has stopped
    // reproducing the older-than-retention condition.
    assert!(
        text.contains("in_memory_blocks=4"),
        "the node must have restored a 4-block window (heights 9..=12), so the \
         rogue block at height 2 was reachable ONLY via the durable scan.\n{text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// #2501 — THE CALL SITE IS LOAD-BEARING.
///
/// A deployment that declares `WRFCOIN_REQUIRE_PRODUCER_AUTH=1` and is started
/// against persisted state holding NO producer set must refuse to boot.
///
/// This is the gap neither other guard sees. `resolve` runs only on the
/// fresh-genesis branch, so on a restart the flag is never consulted — the set is
/// not "resolved empty", it is never resolved. And the signed-blocks backstop
/// cannot help either, because these blocks are UNSIGNED: a reused legacy volume
/// is indistinguishable from a legitimate legacy chain when you look only at the
/// chain. The operator's declared intent is the missing information.
///
/// **Why this test runs the real binary.** Deleting the `main.rs` invocation and
/// leaving the helper intact keeps every in-crate test green — I verified that:
/// 323 passed, 0 failed, with the call site removed. That is precisely the #2398
/// defect ("a resolver that compiles and tests green but is never invoked"), and
/// a helper-level test cannot detect it. Only the process can.
#[tokio::test]
async fn launch_api_refuses_when_auth_is_required_but_persisted_state_has_no_producer_set() {
    let dir = temp_dir("2501-proc-refuse");
    build_unsigned_chain_pinned(&dir, 3, true).await;

    let (mut child, log) = spawn_launch_api_env(&dir, 4, &[("WRFCOIN_REQUIRE_PRODUCER_AUTH", "1")]);
    let status = wait_for_exit(&mut child, Duration::from_secs(120));
    let text = read_log(&log);

    let status = status.unwrap_or_else(|| {
        panic!(
            "launch_api must EXIT when it requires producer auth but the restored \
             chain has no producer set; it kept running.\n{text}"
        )
    });
    assert!(
        !status.success(),
        "launch_api must exit nonzero, got {status:?}.\n{text}"
    );
    assert!(
        text.contains("PRODUCER_AUTH_REQUIRED_BUT_UNSEEDED"),
        "the refusal must name PRODUCER_AUTH_REQUIRED_BUT_UNSEEDED, so an operator \
         can tell it apart from the other producer-auth refusals.\n{text}"
    );
    assert!(
        text.contains("CLEAN volume") || text.contains("resync"),
        "and must state a remedy — the two causes (reused volume vs set lost on \
         replay) need different operator actions.\n{text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The control that makes the refusal safe to ship: the SAME binary, the SAME
/// chain with no producer set — but the deployment never declared it requires
/// auth, so it must get PAST the gate.
///
/// Without this, the change above could be bricking every legacy and dev chain
/// and the refusal test would look identical.
#[tokio::test]
async fn launch_api_starts_on_an_unconfigured_chain_when_auth_is_not_required() {
    let dir = temp_dir("2501-proc-pass");
    build_unsigned_chain_pinned(&dir, 3, true).await;

    // No WRFCOIN_REQUIRE_PRODUCER_AUTH at all — the legacy/dev shape.
    let (mut child, log) = spawn_launch_api(&dir, 4);
    let (found, text) = wait_for_log(
        &mut child,
        &log,
        "In-memory block window",
        Duration::from_secs(120),
    );
    let _ = child.kill();

    assert!(
        found,
        "a chain with no producer set must still boot when the deployment never \
         declared it requires producer auth.\n{text}"
    );
    assert!(
        !text.contains("PRODUCER_AUTH_REQUIRED_BUT_UNSEEDED"),
        "and must not trip the #2501 refusal.\n{text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #2290/#2398 — the FRESH-GENESIS seeding call site is load-bearing
// ---------------------------------------------------------------------------
//
// These live in this file to reuse its process harness (`spawn_launch_api_env`,
// `wait_for_log`, `read_log`) rather than duplicate ~200 lines of it. They are
// about the OTHER branch of `load_state()`: the fresh-genesis path that pins the
// producer set into genesis state, which is cut gate B2's actual mechanism.
//
// Why they are needed. `genesis_producer_members_resolves_the_pinned_testnet_committee`
// tests the RESOLVER. Nothing tested the SEEDING. I deleted
// `chain.state.seed_genesis_producer_set(genesis_producers)` from `main.rs` and
// the whole in-crate binary suite stayed green — 323 passed, 0 failed — as did
// this integration target at 10/0. That is the #2398 defect ("a resolver that
// compiles and tests green but is never invoked") on the one call that decides
// whether the new chain enforces producer authority at all.
//
// A chain cut with that call missing comes up looking authenticated and
// enforcing nothing, and every gate downstream reads the empty set as "this
// chain never configured producer auth" — the failure mode is silent by
// construction.

/// A FRESH genesis on the testnet network must seed the pinned ceremony
/// committee into genesis state, through the real binary.
///
/// Asserts the seeding log line AND the member count, because a resolver that
/// returned an empty set would still take the fresh-genesis branch — only the
/// count distinguishes "seeded" from "seeded nothing".
///
/// # What this proves, and what it does NOT — established by mutation
///
/// | mutation | caught? |
/// |---|---|
/// | delete the whole `if !genesis_producers.is_empty() { … }` block | **yes** — the #2398 shape, and the realistic regression |
/// | make `genesis_producer_members()` return an empty vec | **yes** — the count assertion fails |
/// | delete ONLY the `seed_genesis_producer_set(..)` call, keeping the log | **NO** |
///
/// That last row is a real limit and it is stated rather than glossed. The
/// `log::info!` sits *after* the call inside the same block, so removing the
/// call alone leaves the line printing and this test blind. It is an unrealistic
/// edit — nobody deletes a call and keeps its success message — but it means the
/// assertion is on a LOG that is only causally tied to the resolver and the
/// branch, not to the state mutation itself.
///
/// So: this proves the resolver → fresh-genesis-branch wiring is live, which is
/// exactly the #2398 defect (`genesis_producer_members()` hard-coded to
/// `Ok(vec![])`, seeding dead code). It does not independently prove
/// `seed_genesis_producer_set` mutated state. That the seeding changes state at
/// all is proven separately by `producer_set_is_folded_into_state_root` in
/// `producer_signed_wal_parity_2226.rs`.
///
/// **To close the remaining gap** the test has to observe persisted state rather
/// than a log: start fresh (seeds), shut down GRACEFULLY so `main.rs`'s shutdown
/// checkpoint writes `producer_set_history`, then restart with
/// `WRFCOIN_REQUIRE_PRODUCER_AUTH=1` and require that the restored-state
/// invariant does NOT refuse. That needs SIGTERM rather than `Child::kill`'s
/// SIGKILL, so it is deliberately left as a follow-up rather than bolted on
/// here — a flaky signal-timing test in a cut-critical file would be worse than
/// the gap it closes.
#[tokio::test]
async fn fresh_genesis_seeds_the_pinned_producer_set() {
    let dir = temp_dir("2398-fresh-seed");
    std::fs::create_dir_all(&dir).expect("data dir");
    // No chain built: an EMPTY data dir is what makes this the fresh-genesis
    // branch (`load_state() -> Ok(None)`).

    let (mut child, log) = spawn_launch_api_env(
        &dir,
        4,
        // The pinned set resolves via precedence rule 2, which keys on the
        // network id. This is the cut deployment's own configuration.
        &[
            ("WRFCOIN_NETWORK_ID", "testnet"),
            // This synthetic non-777 cut must record its V1 schedule before
            // boot can reach the producer-seeding invariant under test.
            ("WRFCOIN_GENESIS_ACTIVATION_SCHEDULE", "0:v1"),
        ],
    );
    let (found, text) = wait_for_log(
        &mut child,
        &log,
        "Seeded genesis producer set",
        Duration::from_secs(120),
    );
    let _ = child.kill();

    assert!(
        found,
        "a fresh testnet genesis must SEED the producer set. If this is absent, \
         the chain came up with an empty producer_set_history and producer \
         authority is unenforced from block 1 — silently, because every gate \
         reads an empty set as 'never configured' (#2290/#2398).\n{text}"
    );
    assert!(
        text.contains("Seeded genesis producer set (3 member(s))"),
        "the seeded set must be the pinned ceremony committee, not an empty one. \
         Asserting the COUNT is what separates 'seeded' from 'seeded nothing' — \
         a resolver returning an empty vec still takes this branch.\n{text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The discriminating control: the SAME binary and the SAME empty data dir, but
/// a network id that selects no pinned set — and the deployment declares it
/// requires producer auth. The resolver's own guard must abort the boot.
///
/// Without this, `fresh_genesis_seeds_the_pinned_producer_set` could be passing
/// because the node seeds unconditionally rather than because the network id
/// selected the committee.
#[tokio::test]
async fn fresh_genesis_refuses_when_auth_is_required_but_no_set_resolves() {
    let dir = temp_dir("2398-fresh-noset");
    std::fs::create_dir_all(&dir).expect("data dir");

    let (mut child, log) = spawn_launch_api_env(&dir, 4, &[
        // Not "testnet", so precedence rule 2 does not fire and no set resolves.
        ("WRFCOIN_NETWORK_ID", "not-a-pinned-network"),
        ("WRFCOIN_REQUIRE_PRODUCER_AUTH", "1"),
    ]);
    let status = wait_for_exit(&mut child, Duration::from_secs(120));
    let text = read_log(&log);

    let status = status.unwrap_or_else(|| {
        panic!(
            "launch_api must EXIT when it requires producer auth and no genesis \
             set resolves; it kept running.\n{text}"
        )
    });
    assert!(
        !status.success(),
        "launch_api must exit nonzero, got {status:?}.\n{text}"
    );
    assert!(
        text.contains("no genesis producer set resolved"),
        "the refusal must come from the resolver's own empty-set guard, so it is \
         attributable rather than incidental.\n{text}"
    );
    assert!(
        !text.contains("Seeded genesis producer set"),
        "and it must refuse BEFORE seeding anything.\n{text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The control that keeps the refusal test honest: the same binary, the same
/// env, the same chain shape — but every block authorized — gets PAST the gate.
/// It also pins the count at the durable total (12) rather than the window (4),
/// so reverting the scan to `blockchain.blocks()` fails here too.
#[tokio::test]
async fn launch_api_starts_and_certifies_every_persisted_block_when_all_are_authorized() {
    let dir = temp_dir("proc-pass");
    build_v2_chain(&dir, 12, None).await;

    let (mut child, log) = spawn_launch_api(&dir, 4);
    let (found, text) = wait_for_log(
        &mut child,
        &log,
        "Producer-authority boot check passed",
        Duration::from_secs(120),
    );

    assert!(
        found,
        "an all-authorized chain must clear the boot backstop.\n{text}"
    );
    assert!(
        !text.contains("PRODUCER_AUTH_PERSISTED_VIOLATION"),
        "no violation may be reported for an all-authorized chain.\n{text}"
    );
    assert!(
        text.contains("12 persisted block(s) verified"),
        "the node must certify all 12 DURABLE blocks, not the 4-block window.\n{text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
