//! Genesis producer-set configuration (#2290 enforce-at-genesis).
//!
//! This is the **config surface** that supplies the initial authorized
//! block-producer committee seeded into consensus state at a **fresh genesis**
//! (`State::seed_genesis_producer_set`). Without it that seeding call has no
//! caller, `State::new()` leaves the producer-set history empty, and the
//! enforcement gate treats an empty set as **UNENFORCED** — i.e. a fresh chain
//! silently is not authenticated at all. This module closes that seam.
//!
//! ## Why a config type distinct from the consensus type
//!
//! [`GenesisProducerMember`] here is the *validated config* type; it maps 1:1
//! onto the consensus type `consensus::producer_set::ProducerMember`
//! (`producer_id`, `payout_address`, `public_key`). They are deliberately
//! separate:
//!
//! * The on-disk/ceremony form carries `public_key` as **64-char lowercase
//!   hex**. Derived serde on `[u8; 32]` emits a JSON array of 32 integers,
//!   which is unusable as an artifact a human operator has to read back and
//!   sign off during the key ceremony.
//! * Config parsing is fail-closed *presentation* validation; the consensus
//!   layer keeps its own authoritative `validate_members` backstop. This module
//!   never replaces that check, it only fails **earlier** and with an operator
//!   -actionable message.
//!
//! ## Resolution precedence
//!
//! Deliberately mirrors the existing genesis-config precedent,
//! `p2p::config::resolve_genesis_hash`, rather than inventing a parallel
//! mechanism:
//!
//! 1. `WRFCOIN_GENESIS_PRODUCER_SET`, when set and non-empty — inline JSON, or
//!    `@/absolute/path.json`. **Any** parse or validation failure is an `Err`
//!    and therefore a startup abort; it never degrades to an empty set.
//! 2. Otherwise, when `WRFCOIN_NETWORK_ID=testnet` — the pinned compile-time
//!    [`TESTNET_GENESIS_PRODUCER_SET`].
//! 3. Otherwise — an empty set: producer auth is *not configured*, therefore
//!    not enforced. Legacy chains and local dev only.
//!
//! ## Why the testnet set is a compiled-in literal, never per-node env
//!
//! The producer set is folded into `state_root`. Two nodes that seed different
//! members compute different state roots for the *same* genesis and fork
//! immediately. So the canonical testnet set is pinned in the binary — one
//! artifact, byte-identical on every node — instead of assembled per-node from
//! environment. `WRFCOIN_GENESIS_PRODUCER_SET` exists for the cut and for
//! development; when it is used it must be the *same* pinned file on every
//! node.
//!
//! ## The empty-set downgrade hole
//!
//! "Enforcement is keyed off a non-empty set" means anything yielding an empty
//! set silently disables authentication. `WRFCOIN_REQUIRE_PRODUCER_AUTH=1`
//! converts "resolved empty" into a hard startup failure and is **mandatory in
//! the cut deployment** — it is the guard against an operator forgetting
//! `WRFCOIN_NETWORK_ID` and silently minting an unenforced chain.
//!
//! ## Env-flag polarity (sweep, #2397 review)
//!
//! A lenient boolean parser is not a defect by itself — it depends on which
//! direction is safe. Swept over this binary's env surface:
//!
//! | Flag | Unrecognized value reads as | Safe? |
//! |---|---|---|
//! | `WRFCOIN_ENABLE_NFT_MUTATION_API` | off | yes — risky API stays disabled |
//! | `WRFCOIN_ENABLE_ORACLE_ADMIN_API` | off | yes — risky API stays disabled |
//! | `WRFCOIN_ENABLE_DEBUG_SEED_API` | off | yes — risky API stays disabled |
//! | `WRFCOIN_MEMPOOL_*` | n/a — already fallible (`Result`) | yes |
//! | `WRFCOIN_REQUIRE_PRODUCER_AUTH` | **off = a disabled security guard** | **NO — fixed here** |
//!
//! The three `WRFCOIN_ENABLE_*` flags *enable* a dangerous surface, so a typo
//! failing to "off" fails safe and they are deliberately left lenient. This one
//! is the opposite polarity and is the only switch in the sweep where a typo
//! removes a protection, so it parses fallibly.
//!
//! **Residual, accepted:** `WRFCOIN_NETWORK_ID` is a *selector*, not a boolean,
//! and cannot be made fallible here — `mainnet`, `dev` and friends are all
//! legitimate values, so a typo like `testnett` is indistinguishable from a
//! real third network and lands in the unenforced legacy branch. There is no
//! local fix; the mitigation is `WRFCOIN_REQUIRE_PRODUCER_AUTH=1`, which now
//! aborts on that path regardless of how the network id was spelled. This is
//! precisely why the cut compose sets it, and why it must not be treated as
//! belt-and-braces.

// #2398 CLOSED THE SEAM: `genesis_producer_members()` in main.rs now calls
// `resolve()`, so every item here has a production caller and the temporary
// `#![allow(dead_code)]` that used to sit on this module is GONE. Do not
// reintroduce it — it was the tripwire for the #2290 defect (a resolver that
// compiles and tests green but is never invoked, leaving a fresh genesis
// unauthenticated). If dead-code warnings reappear here, the wiring regressed.

use serde::Deserialize;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::signature::PublicKey;

#[path = "genesis_producer_pins.rs"]
mod genesis_producer_pins;
use genesis_producer_pins::TESTNET_GENESIS_PRODUCER_SET;

/// Explicit genesis producer-set source: inline JSON, or
/// `@/absolute/path.json`.
pub const GENESIS_PRODUCER_SET_ENV: &str = "WRFCOIN_GENESIS_PRODUCER_SET";
/// Fail-closed switch: when truthy, a resolved-empty producer set aborts
/// startup.
pub const REQUIRE_PRODUCER_AUTH_ENV: &str = "WRFCOIN_REQUIRE_PRODUCER_AUTH";
/// Shared network selector, same variable the p2p genesis-hash resolver reads.
pub const NETWORK_ID_ENV: &str = "WRFCOIN_NETWORK_ID";

/// The only config schema version this binary understands. An unrecognized
/// version is rejected rather than best-effort parsed — a future field that
/// changes consensus meaning must not be silently dropped by an old node.
const SUPPORTED_CONFIG_VERSION: u32 = 1;

/// Upper bound on the genesis producer committee.
///
/// Every member is folded into `state_root` and must be agreed byte-for-byte by
/// every node, so a set this large is a configuration error long before it is a
/// scaling question — the cut seeds two. The ceiling exists so a malformed or
/// hostile document is rejected with a clear message instead of sizing an
/// allocation from a decoded length field (#2111).
const MAX_GENESIS_PRODUCERS: usize = 64;

/// The canonical testnet genesis producer set, pinned at build time.
///
/// **PINNED 2026-07-24 from the completed key ceremony** (`n5-node-a` primary,
/// `producer-standby`). Before this, the constant was `None` and a fresh
/// testnet genesis refused to start rather than mint an unauthenticated chain;
/// that gate is now satisfied by real ceremony output.
///
/// **Standby renamed `n5-node-b` -> `producer-standby` (2026-07-31, owner
/// decision).** The standby token moved off the N5 to the owner's workstation,
/// which made a host-derived name wrong. The replacement is *role*-derived so it
/// cannot go stale the next time hardware moves — and it matches the CN already
/// on the token, `CN=wrfcoin-block-producer-standby`. `n5-node-a` keeps its name
/// because it is still on the N5 and is therefore still accurate.
///
/// The rename is only possible **before** the cut: `producer_id` is folded into
/// the canonical producer-set hash and therefore into `state_root`, so on a live
/// chain it would be a consensus break. It also invalidates the standby's prior
/// proof-of-possession transcript, because `producer_key_pop` binds
/// `producer_id` into the signed challenge — the ceremony must be re-run for
/// this member against the new id.
///
/// Everything below is **public**: ed25519 public keys and coinbase payout
/// addresses. The signing private keys are non-exportable on their YubiKeys and
/// have never left them; the payout private keys live only in the owner's
/// password manager. Nothing here is secret and nothing secret belongs here.
///
/// Provenance — `docs/security/PRODUCER_KEY_CEREMONY.md` §6 custody log:
///
/// | `producer_id` | YubiKey serial | PIV slot | Certificate CN |
/// |---|---|---|---|
/// | `n5-node-a` | 35278470 | 9A | `CN=wrfcoin-block-producer-n5` |
/// | `producer-standby` | 35278471 | 9A | `CN=wrfcoin-block-producer-standby` |
///
/// Both members feed `state_root`, so this literal is the single artifact every
/// node must agree on byte-for-byte. Changing it changes the genesis state and
/// therefore the chain. **Pre-P5 there is no way to alter the set on a running
/// chain** (owner decision R111): editing this constant only affects a *fresh*
/// genesis, i.e. a re-cut. `producer-standby` is the standby that makes producer-key
/// loss or compromise survivable without one — see the packet §2 Branch A.
///
/// Slot 9A deliberately, not 9C: 9C holds the snapshot signer on both tokens
/// and was left untouched. Selecting 9A at runtime requires **both**
/// `WRFCOIN_HSM_PRODUCER_KEY_LABEL` = `Private key for PIV Authentication` and
/// `WRFCOIN_HSM_PRODUCER_PUBKEY_LABEL` = `Public key for PIV Authentication`
/// (read back through `libykcs11.so`, object ID 01). Setting only the first
/// silently pairs a 9A private key with the 9C public key.
/// A validated genesis producer, ready to be converted into the consensus
/// `ProducerMember` (identical field set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisProducerMember {
    /// Stable operator identity; does not rotate with the signing key.
    pub producer_id: String,
    /// Canonical `Address` string (`0x` + 40 lowercase hex) — the coinbase
    /// target. Deliberately decoupled from `public_key` (§R2) so signing-key
    /// rotation does not move payouts.
    pub payout_address: String,
    /// ed25519 public key authenticating this producer's block signature.
    pub public_key: [u8; 32],
}

/// On-disk config shape. `deny_unknown_fields` throughout: a mistyped key in a
/// consensus-critical genesis artifact must be a loud failure, not a silently
/// ignored field that leaves a producer out of the set.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenesisProducerSetFile {
    /// JSON has no comments, and this artifact needs its handling rules to
    /// travel with it to the operator running the cut. This ONE field is
    /// explicitly allowed and ignored; every other unknown key is still
    /// rejected, so a mistyped `payout_addres` remains a loud failure.
    #[serde(rename = "_README", default)]
    _readme: Option<serde_json::Value>,
    version: u32,
    members: Vec<GenesisProducerEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenesisProducerEntry {
    producer_id: String,
    payout_address: String,
    public_key: String,
}

/// Resolve the genesis producer set from the process environment.
///
/// `Ok(vec![])` means "not configured, therefore not enforced". Every failure
/// path returns `Err`, which the caller must propagate as a **startup abort** —
/// never as a fallback to the empty set.
pub fn resolve() -> Result<Vec<GenesisProducerMember>, String> {
    resolve_from_values(
        // determinism-ok: startup-only genesis configuration is parsed and committed into the initial State before block processing begins
        std::env::var(GENESIS_PRODUCER_SET_ENV).ok().as_deref(),
        // determinism-ok: startup-only network selection chooses the pinned genesis set before block processing begins
        std::env::var(NETWORK_ID_ENV).ok().as_deref(),
        // determinism-ok: startup-only fail-closed configuration determines whether an absent producer set aborts boot
        std::env::var(REQUIRE_PRODUCER_AUTH_ENV).ok().as_deref(),
        TESTNET_GENESIS_PRODUCER_SET,
    )
}

/// Testable core of [`resolve`] — the same value-parameterized shape the rest
/// of this binary uses for env-driven config (`mempool_config_from_values`,
/// etc.) so the precedence rules are unit-testable without mutating process
/// env.
fn resolve_from_values(
    explicit: Option<&str>,
    network_id: Option<&str>,
    require_auth: Option<&str>,
    pinned_testnet: Option<&str>,
) -> Result<Vec<GenesisProducerMember>, String> {
    // Parse the security switch FIRST, and unconditionally.
    //
    // It is only *consulted* below when the set resolves empty, but it must be
    // *validated* on every start: otherwise a typo'd switch is silently tolerated
    // on any run where the set happens to resolve non-empty, and only bites later
    // — on the one boot where it was the last line of defence. A malformed
    // security control is a misconfiguration the moment it is written, not the
    // moment it matters.
    let require_auth = require_producer_auth(require_auth)?;

    let explicit = explicit.map(str::trim).filter(|spec| !spec.is_empty());

    let members = if let Some(spec) = explicit {
        let raw = load_source(spec)?;
        parse_and_validate(&raw)
            .map_err(|e| format!("{GENESIS_PRODUCER_SET_ENV} is invalid: {e}"))?
    } else if network_is_testnet(network_id) {
        match pinned_testnet {
            Some(pinned) => parse_and_validate(pinned).map_err(|e| {
                format!(
                    "the pinned testnet genesis producer set is invalid: {e}. This is a build \
                     defect, not an operator error — the literal in genesis_producers.rs does not \
                     satisfy its own validation."
                )
            })?,
            None => {
                return Err(format!(
                    // secret-leak-ok: interpolates only public environment-variable names; no credential value is loaded here
                    "{NETWORK_ID_ENV}=testnet requests a FRESH genesis but no testnet genesis \
                     producer set has been pinned yet, so the chain would start with block \
                     production unauthenticated (#2290). Run the key ceremony \
                     (docs/security/PRODUCER_KEY_CEREMONY.md) and pin its output in \
                     TESTNET_GENESIS_PRODUCER_SET, or set {GENESIS_PRODUCER_SET_ENV} explicitly \
                     to the pinned set shared by every node."
                ));
            },
        }
    } else {
        Vec::new()
    };

    // The empty-set downgrade hole: enforcement is keyed off a non-empty set, so
    // "resolved empty" is indistinguishable from "auth deliberately off" unless
    // the operator states which they meant.
    if members.is_empty() && require_auth {
        return Err(format!(
            "{REQUIRE_PRODUCER_AUTH_ENV} is set but no genesis producer set resolved, so \
             block-producer authentication would NOT be enforced. Refusing to start an \
             unauthenticated chain. Set {GENESIS_PRODUCER_SET_ENV} to the pinned producer set."
        ));
    }

    Ok(members)
}

/// Load an explicit source spec: `@/absolute/path.json` reads a file, anything
/// else is treated as inline JSON.
fn load_source(spec: &str) -> Result<String, String> {
    match spec.strip_prefix('@') {
        // The exemption must be a TRAILING comment on the read line itself —
        // `no_file_backed_key_load_exists_without_the_permission_check` matches
        // `key-file-exempt:` per-line against the line carrying the read, so an
        // exemption on the preceding line does not register (#2397 → #2398).
        Some(path) => std::fs::read_to_string(path.trim()).map_err(|e| { // key-file-exempt: genesis producer-set config holds PUBLIC keys and payout addresses only; no private key material is ever read here
            format!(
                "cannot read genesis producer set file '{}': {e}",
                path.trim()
            )
        }),
        None => Ok(spec.to_string()),
    }
}

/// Same testnet predicate the p2p genesis-hash resolver uses, so "is this the
/// testnet?" cannot mean two different things in one binary.
///
/// Delegates to `shared-protocol` (#2523): the block-1 oracle gate needs the
/// same answer and cannot reach into this binary, so the definition moved rather
/// than being copied. A second copy is how "is this the testnet?" starts meaning
/// two different things across the workspace instead of within one binary.
fn network_is_testnet(network_id: Option<&str>) -> bool {
    wrfcoin_shared_protocol::crypto::chain_binding::network_is_public_testnet(network_id)
}

/// Recognized spellings for [`REQUIRE_PRODUCER_AUTH_ENV`]. Deliberately short:
/// this is a security switch, so the set of accepted values is a closed list,
/// not a best-effort interpretation.
const REQUIRE_AUTH_ON: [&str; 2] = ["1", "true"];
const REQUIRE_AUTH_OFF: [&str; 2] = ["0", "false"];

/// Parse the fail-closed switch — **fallibly**.
///
/// Unset, or empty (how Docker passes "not set"), is off. `1`/`true` is on,
/// `0`/`false` is off, case-insensitive and trimmed. **Any other non-empty
/// value is an error**, never a silent false.
///
/// This is the whole point: a lenient parser on a switch whose OFF state is
/// unsafe converts an operator's typo into a silently disabled security
/// control. `WRFCOIN_REQUIRE_PRODUCER_AUTH=tru` previously read as "off", so an
/// operator who was actively trying to *enable* authentication got an
/// unauthenticated chain and no indication anything was wrong — the exact
/// silent-downgrade shape §6 of the failure playbook is written to prevent.
///
/// Note the asymmetry with the other boolean flags in this binary
/// (`WRFCOIN_ENABLE_*`): those *enable* a risky surface, so an unrecognized
/// value reading as "off" fails **safe**. Here off is the dangerous direction.
/// Lenient parsing is not the defect by itself — lenient parsing on a switch
/// whose off-state is unsafe is.
fn require_producer_auth(raw: Option<&str>) -> Result<bool, String> {
    let Some(value) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(false);
    };
    if REQUIRE_AUTH_ON
        .iter()
        .any(|on| value.eq_ignore_ascii_case(on))
    {
        return Ok(true);
    }
    if REQUIRE_AUTH_OFF
        .iter()
        .any(|off| value.eq_ignore_ascii_case(off))
    {
        return Ok(false);
    }
    Err(format!(
        "{REQUIRE_PRODUCER_AUTH_ENV}='{value}' is not a recognized value (expected {} to require \
         block-producer authentication, or {} to disable it). Refusing to start rather than \
         guess: on a fail-closed security switch an unrecognized value would read as OFF, so an \
         operator who meant to ENABLE authentication would silently get an unauthenticated chain \
         (#2290).",
        REQUIRE_AUTH_ON.join("/"),
        REQUIRE_AUTH_OFF.join("/")
    ))
}

/// Startup invariant for a chain restored from PERSISTED state (#2501).
///
/// # The gap this closes
///
/// `resolve` guards the *fresh-genesis* path: if the deployment declares it
/// requires producer auth and no set resolves, it refuses. But `main.rs` seeds
/// the producer set **only** on the fresh branch (`load_state() -> Ok(None)`);
/// on a restart it takes persisted state as-is and never re-resolves. So a node
/// started against a data volume that holds a chain with **no producer set**
/// reaches neither guard:
///
/// * `resolve` is not consulted at all, so `WRFCOIN_REQUIRE_PRODUCER_AUTH` never
///   fires — the set is not "resolved empty", it is never resolved;
/// * the producer-authority boot backstop treats an empty set as "this chain
///   never configured producer auth" and is unenforced by design, which is what
///   keeps legacy and dev chains working.
///
/// Two ways to arrive there, both operationally ordinary:
///
/// 1. **A cut that reuses a data volume.** The operator intends a fresh genesis;
///    the node finds old state, skips seeding, and silently continues the OLD
///    chain unenforced instead of starting the new one.
/// 2. **Checkpoint-less WAL recovery** (#2501). `producer_set_history` is genesis
///    `State` carried by no `StateTransition`, so pure WAL replay reconstructs it
///    as empty on a chain that *was* enforcing.
///
/// Case 2 is separately caught: at V2 by the #2219 tip state-root commitment
/// check, and at any version by the signed-blocks boot backstop. Case 1 is not,
/// because a reused *legacy* volume has unsigned blocks and a genuinely empty
/// set — indistinguishable, from the chain alone, from a legitimate legacy chain.
///
/// **The operator's declared intent is the missing bit of information.** Setting
/// `WRFCOIN_REQUIRE_PRODUCER_AUTH` says "this deployment enforces producer
/// authority", and that is a statement about the deployment, not about one code
/// path. This promotes it from a resolver-path guard to a whole-startup
/// invariant: if you declared you require it, the running chain must have a set.
///
/// Cannot false-fire on a fresh genesis: when the flag is set, `resolve` has
/// already guaranteed a non-empty set, so a freshly-seeded chain never reaches
/// the refusal.
///
/// `raw_require` is the raw env value so the **fallible** parse is preserved — an
/// unrecognized value must abort rather than read as "not required", which is the
/// #2397 polarity rule.
pub fn check_restored_chain_producer_set(
    raw_require: Option<&str>,
    producer_set_is_empty: bool,
) -> Result<(), String> {
    if !require_producer_auth(raw_require)? {
        return Ok(());
    }
    if !producer_set_is_empty {
        return Ok(());
    }
    Err(format!(
        "{REQUIRE_PRODUCER_AUTH_ENV} is set, but the chain restored from persisted state has NO \
         producer set, so block-producer authentication would NOT be enforced. Refusing to start. \
         The genesis producer set is only seeded on a FRESH genesis, so this state cannot acquire \
         one by restarting. Either this data volume predates producer auth — start the cut from a \
         CLEAN volume — or the set was lost in a checkpoint-less WAL recovery, in which case \
         resync this node from a peer (#2501)."
    ))
}

/// Parse and fully validate a genesis producer-set document.
///
/// Every rule here is fail-closed — a rejected document aborts startup rather
/// than yielding a partial or normalized set.
pub fn parse_and_validate(raw: &str) -> Result<Vec<GenesisProducerMember>, String> {
    let file: GenesisProducerSetFile =
        serde_json::from_str(raw).map_err(|e| format!("malformed JSON: {e}"))?;

    if file.version != SUPPORTED_CONFIG_VERSION {
        return Err(format!(
            "unsupported config version {} (this binary understands version {SUPPORTED_CONFIG_VERSION})",
            file.version
        ));
    }

    // An empty set is not "auth off" — in a document the operator deliberately
    // supplied, it is a mistake, and accepting it would silently disable
    // enforcement on a fresh chain.
    if file.members.is_empty() {
        return Err(
            "the producer set is empty; a fresh genesis must authorize at least one block \
             producer or block production is unauthenticated (#2290)"
                .to_string(),
        );
    }

    // Bound the set BEFORE allocating anything sized from it (#2111 input-panic
    // lint). `Vec::with_capacity(file.members.len())` sized an allocation
    // directly from a decoded length field, which panics on capacity overflow;
    // `.collect()` would have the same shape. A growing `Vec::new()` plus an
    // explicit ceiling has neither property.
    //
    // The ceiling is real validation, not lint appeasement: a genesis committee
    // of thousands is a malformed config, and rejecting it loudly beats
    // allocating for it. The cut seeds two members.
    //
    // Honest limit: `serde_json` has already allocated `file.members` by the time
    // this runs, so the ceiling bounds *this* function's amplification and gives
    // the operator a clear error — it does not bound the parse itself. That is
    // acceptable here because the source is startup configuration (a compiled-in
    // constant, or a file/env the operator supplies), not remote input.
    if file.members.len() > MAX_GENESIS_PRODUCERS {
        return Err(format!(
            "the producer set has {} members, which exceeds the maximum of \
             {MAX_GENESIS_PRODUCERS}. A genesis committee this large is almost certainly a \
             malformed config; if it is deliberate, raise MAX_GENESIS_PRODUCERS and re-review \
             the state_root implications.",
            file.members.len()
        ));
    }

    let mut members = Vec::new();
    for (index, entry) in file.members.iter().enumerate() {
        members.push(validate_entry(index, entry)?);
    }

    // Duplicates are checked across the whole set, after per-entry validation, so
    // the operator sees the *specific* structural error first.
    for (i, member) in members.iter().enumerate() {
        for other in &members[..i] {
            if other.producer_id == member.producer_id {
                return Err(format!(
                    "duplicate producer_id '{}' — each producer must appear exactly once",
                    member.producer_id
                ));
            }
            if other.public_key == member.public_key {
                // Verified false positive: the message names the field
                // `public_key` but interpolates only `producer_id` strings, so no
                // key bytes reach the sink — and a producer public key is
                // published in the genesis set regardless.
                return Err(format!(
                    // secret-leak-ok: names the field, interpolates only producer ids
                    "duplicate public_key between producers '{}' and '{}' — one signing key must \
                     not authorize two producer identities",
                    other.producer_id, member.producer_id
                ));
            }
        }
    }

    Ok(members)
}

fn validate_entry(
    index: usize,
    entry: &GenesisProducerEntry,
) -> Result<GenesisProducerMember, String> {
    // A whitespace-padded id is a different string, hashes differently, and would
    // silently produce a set that does not match the ceremony record.
    if entry.producer_id.is_empty() || entry.producer_id.trim() != entry.producer_id {
        return Err(format!(
            "member[{index}]: producer_id must be non-empty and free of leading/trailing \
             whitespace, got '{}'",
            entry.producer_id
        ));
    }

    // Canonical Address: parses AND round-trips to itself. This rejects the
    // no-`0x` form, upper-case hex, and EIP-55 checksum form — all of which parse
    // but are NOT the canonical string, and would be rejected later by the
    // consensus-layer backstop after the chain had already been minted.
    //
    // NOTE: this is `common::types::Address` (`0x` + 40 lowercase hex), NOT
    // `PublicKey::to_address()`, which returns a distinct `wrf…` form. Confusing
    // the two is a recurring defect in this epic.
    let canonical = entry
        .payout_address
        .parse::<Address>()
        .map(|addr| addr.to_string());
    match canonical {
        Ok(ref canonical) if *canonical == entry.payout_address => {},
        Ok(ref canonical) => {
            return Err(format!(
                "member[{index}] ('{}'): payout_address '{}' is not canonical; the canonical form \
                 is '{canonical}' (0x + 40 lowercase hex)",
                entry.producer_id, entry.payout_address
            ));
        },
        Err(_) => {
            return Err(format!(
                "member[{index}] ('{}'): payout_address '{}' is not a valid address. Expected a \
                 canonical Address (0x + 40 lowercase hex) — note this is NOT the 'wrf…' form \
                 returned by PublicKey::to_address()",
                entry.producer_id, entry.payout_address
            ));
        },
    }

    let public_key = parse_public_key(index, &entry.producer_id, &entry.public_key)?;

    Ok(GenesisProducerMember {
        producer_id: entry.producer_id.clone(),
        payout_address: entry.payout_address.clone(),
        public_key,
    })
}

fn parse_public_key(index: usize, producer_id: &str, raw: &str) -> Result<[u8; 32], String> {
    // Require the exact canonical encoding: 64 lowercase hex characters. Accepting
    // `0x` prefixes or upper case would let the same key appear in two spellings,
    // defeating the duplicate-key check.
    if raw.len() != 64
        || !raw
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err(format!(
            "member[{index}] ('{producer_id}'): public_key must be exactly 64 lowercase hex \
             characters (32 bytes, no '0x' prefix), got '{raw}'"
        ));
    }

    let bytes = hex::decode(raw).map_err(|e| {
        format!("member[{index}] ('{producer_id}'): public_key is not valid hex: {e}")
    })?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| format!("member[{index}] ('{producer_id}'): public_key must be 32 bytes"))?;

    // Not every 32-byte string is a valid ed25519 point. Reject here rather than
    // letting an unusable key be minted into genesis consensus state.
    PublicKey::from_bytes(&bytes).map_err(|e| {
        format!("member[{index}] ('{producer_id}'): public_key is not a valid ed25519 key: {e}")
    })?;

    Ok(bytes)
}

#[cfg(test)]
pub(crate) mod tests {
    use rand::SeedableRng;
    use wrfcoin_shared_protocol::crypto::signature::PrivateKey;

    use super::*;

    /// Deterministic (public key hex, canonical payout address) for a seed.
    fn key_and_address(seed: u64) -> (String, String) {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        let key: [u8; 32] = pk.as_bytes().try_into().expect("ed25519 keys are 32 bytes");
        let address = Address(key[0..20].try_into().expect("20-byte prefix")).to_string();
        (hex::encode(key), address)
    }

    fn doc_with(members: &str) -> String {
        format!(r#"{{"version":1,"members":[{members}]}}"#)
    }

    fn member_json(id: &str, seed: u64) -> String {
        let (key, address) = key_and_address(seed);
        format!(r#"{{"producer_id":"{id}","payout_address":"{address}","public_key":"{key}"}}"#)
    }

    #[test]
    fn accepts_a_well_formed_two_member_set() {
        let doc = doc_with(&format!(
            "{},{}",
            member_json("n5-node-a", 1),
            member_json("n5-node-b", 2)
        ));
        let members = parse_and_validate(&doc).expect("a well-formed set is accepted");

        assert_eq!(members.len(), 2);
        assert_eq!(members[0].producer_id, "n5-node-a");
        assert_eq!(members[1].producer_id, "n5-node-b");
        // The payout address survives as the canonical string, unmodified.
        let (_, expected_address) = key_and_address(1);
        assert_eq!(members[0].payout_address, expected_address);
    }

    #[test]
    fn rejects_duplicate_producer_id() {
        // Same id, DIFFERENT keys — so only the id check can catch this.
        let doc = doc_with(&format!(
            "{},{}",
            member_json("dup", 1),
            member_json("dup", 2)
        ));
        let err = parse_and_validate(&doc).expect_err("duplicate producer_id must be rejected");
        assert!(
            err.contains("duplicate producer_id"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_duplicate_public_key() {
        // Different ids, SAME key — so only the key check can catch this.
        let (key, address) = key_and_address(1);
        let entry = |id: &str| {
            format!(r#"{{"producer_id":"{id}","payout_address":"{address}","public_key":"{key}"}}"#)
        };
        let doc = doc_with(&format!("{},{}", entry("node-a"), entry("node-b")));
        let err = parse_and_validate(&doc).expect_err("duplicate public_key must be rejected");
        assert!(
            err.contains("duplicate public_key"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_non_canonical_payout_address() {
        let (key, address) = key_and_address(1);
        // Each of these PARSES as an Address but is not the canonical string.
        let non_canonical = [
            address.trim_start_matches("0x").to_string(), // missing 0x prefix
            format!("0x{}", address.trim_start_matches("0x").to_uppercase()), // upper-case hex
        ];
        for variant in non_canonical {
            let doc = doc_with(&format!(
                r#"{{"producer_id":"n5","payout_address":"{variant}","public_key":"{key}"}}"#
            ));
            let err = parse_and_validate(&doc)
                .expect_err("a non-canonical payout address must be rejected");
            assert!(
                err.contains("not canonical") || err.contains("not a valid address"),
                "unexpected error for '{variant}': {err}"
            );
        }
    }

    #[test]
    fn rejects_wrf_form_payout_address() {
        // The exact confusion this epic keeps hitting: PublicKey::to_address()
        // returns a `wrf…` string, which is NOT a canonical Address.
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        let wrf_form = pk.to_address();
        assert!(wrf_form.starts_with("wrf"), "precondition: wrf form");

        let (key, _) = key_and_address(1);
        let doc = doc_with(&format!(
            r#"{{"producer_id":"n5","payout_address":"{wrf_form}","public_key":"{key}"}}"#
        ));
        let err = parse_and_validate(&doc).expect_err("wrf-form payout address must be rejected");
        assert!(err.contains("payout_address"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_empty_member_set() {
        let err = parse_and_validate(r#"{"version":1,"members":[]}"#)
            .expect_err("an empty producer set must be rejected");
        assert!(err.contains("empty"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_an_oversized_member_set() {
        // Bounded before anything is sized from the decoded length (#2111).
        let entries: Vec<String> = (0..=MAX_GENESIS_PRODUCERS)
            .map(|i| member_json(&format!("n{i}"), i as u64 + 1))
            .collect();
        let doc = doc_with(&entries.join(","));
        let err = parse_and_validate(&doc).expect_err("an oversized producer set must be rejected");
        assert!(
            err.contains("exceeds the maximum"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn accepts_a_set_exactly_at_the_maximum() {
        // The bound is a ceiling, not an off-by-one that rejects the legal max.
        let entries: Vec<String> = (0..MAX_GENESIS_PRODUCERS)
            .map(|i| member_json(&format!("n{i}"), i as u64 + 1))
            .collect();
        let doc = doc_with(&entries.join(","));
        let members = parse_and_validate(&doc).expect("a set at exactly the maximum is legal");
        assert_eq!(members.len(), MAX_GENESIS_PRODUCERS);
    }

    #[test]
    fn rejects_unsupported_version() {
        let doc = format!(r#"{{"version":2,"members":[{}]}}"#, member_json("n5", 1));
        let err = parse_and_validate(&doc).expect_err("an unknown schema version must be rejected");
        assert!(
            err.contains("unsupported config version"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_unknown_field() {
        // A mistyped key must be loud, not silently dropped.
        let (key, address) = key_and_address(1);
        let doc = doc_with(&format!(
            r#"{{"producer_id":"n5","payout_addres":"{address}","payout_address":"{address}","public_key":"{key}"}}"#
        ));
        let err = parse_and_validate(&doc).expect_err("an unknown field must be rejected");
        assert!(err.contains("malformed JSON"), "unexpected error: {err}");
    }

    #[test]
    fn allows_the_readme_header_but_still_rejects_typos() {
        // The shipped template carries its handling rules in `_README`; an operator
        // must be able to keep that header in the real file.
        let (key, address) = key_and_address(1);
        let entry =
            format!(r#"{{"producer_id":"n5","payout_address":"{address}","public_key":"{key}"}}"#);
        let with_readme =
            format!(r#"{{"_README":["handling rules"],"version":1,"members":[{entry}]}}"#);
        assert!(
            parse_and_validate(&with_readme).is_ok(),
            "_README must be allowed"
        );

        // ...but the allowance must not have opened the door to arbitrary keys.
        let with_typo = format!(r#"{{"verison":1,"version":1,"members":[{entry}]}}"#);
        assert!(
            parse_and_validate(&with_typo).is_err(),
            "an unknown top-level key must still be rejected"
        );
    }

    #[test]
    fn rejects_malformed_public_key() {
        let (key, address) = key_and_address(1);
        let bad_keys = [
            key.to_uppercase(),    // upper-case hex: a second spelling of one key
            format!("0x{key}"),    // 0x-prefixed
            key[..62].to_string(), // too short
            "zz".repeat(32),       // not hex
        ];
        for bad in bad_keys {
            let doc = doc_with(&format!(
                r#"{{"producer_id":"n5","payout_address":"{address}","public_key":"{bad}"}}"#
            ));
            assert!(
                parse_and_validate(&doc).is_err(),
                "public_key '{bad}' must be rejected"
            );
        }
    }

    #[test]
    fn rejects_producer_id_with_surrounding_whitespace() {
        let (key, address) = key_and_address(1);
        let doc = doc_with(&format!(
            r#"{{"producer_id":" n5 ","payout_address":"{address}","public_key":"{key}"}}"#
        ));
        let err = parse_and_validate(&doc).expect_err("padded producer_id must be rejected");
        assert!(err.contains("whitespace"), "unexpected error: {err}");
    }

    // --- the PINNED testnet set (2026-07-24 ceremony) --------------------------

    /// The ceremony output, restated independently of the constant. If someone
    /// edits `TESTNET_GENESIS_PRODUCER_SET`, this test fails until they also
    /// edit these expectations — which is the point: the pinned set is
    /// consensus state, and changing it silently changes the chain.
    const CEREMONY_A: (&str, &str, &str) = (
        "n5-node-a",
        "0x680d51f7543ca70029866956376306d47ce60ed0",
        "1e5180e7add06bbe07cf2698ec9d4882ae1cfb54b13de67bae92a4e625ceec1a",
    );
    const CEREMONY_B: (&str, &str, &str) = (
        "producer-standby",
        "0x39ee2e595a55c71256282fe5c42a398594985b1a",
        "cd77a1f439ec6896727f585e81228795c8008c8f8db7763bb9c42afa8d119aaa",
    );
    /// The third seat (#2971), seated for real on 2026-09-10: payout address
    /// designated by the owner (#3078), producer key generated in the mango
    /// mini's SoftHSM token (PKCS#11 object `wrfcoin-producer-mango`, no
    /// YubiKey — the release signer resolves it through `WRFCOIN_HSM_PRODUCER_KEY_LABEL`).
    /// Restated here so this test tracks the pinned set rather than silently
    /// passing over the newest entry.
    const CEREMONY_C: (&str, &str, &str) = (
        "mango-mini",
        "0xc536d2026661522cb8631b3c723d4af43784a990",
        "1f7a4ec7699e361390cb89297a2b4441e676b17c95b122063eda25b1f64e0f1e", // pragma: allowlist secret -- public genesis key (mango-mini, SoftHSM)
    );

    #[test]
    fn pinned_testnet_set_is_present_and_valid() {
        let pinned = TESTNET_GENESIS_PRODUCER_SET.expect(
            "the testnet producer set must be pinned — a fresh testnet genesis cannot be \
                     minted without it (#2290)",
        );

        // Full fail-closed validation: schema version, non-empty, canonical
        // payout addresses, well-formed and on-curve ed25519 keys, no duplicate
        // producer_id or public_key, no unknown fields.
        let members =
            parse_and_validate(pinned).expect("the pinned set must satisfy its own validation");

        assert_eq!(
            members.len(),
            3,
            "the roster seats exactly three producers (#2971)"
        );
    }

    #[test]
    fn pinned_testnet_set_yields_exactly_the_ceremony_producers() {
        let members =
            parse_and_validate(TESTNET_GENESIS_PRODUCER_SET.expect("pinned")).expect("valid");

        for (member, expected) in members.iter().zip([CEREMONY_A, CEREMONY_B, CEREMONY_C]) {
            let (id, payout, pubkey) = expected;
            assert_eq!(member.producer_id, id);
            assert_eq!(
                member.payout_address, payout,
                "{id}: payout must be the ceremony address, byte-for-byte — a wrong payout does \
                 not stall the chain, it silently misdirects every block reward"
            );
            assert_eq!(
                hex::encode(member.public_key),
                pubkey,
                "{id}: signing key mismatch"
            );
        }
    }

    #[test]
    fn pinned_testnet_producers_have_distinct_keys_and_payouts() {
        let members =
            parse_and_validate(TESTNET_GENESIS_PRODUCER_SET.expect("pinned")).expect("valid");

        // A seat only buys anything if it is genuinely a separate key on a
        // separate device. Identical values would pass `validate_members`'
        // duplicate check only if they were literally equal — this asserts the
        // stronger property that every seat is an independent producer.
        //
        // Checked over ALL PAIRS rather than just [0] vs [1]: with a third seat
        // (#2971) a fixed-index comparison keeps passing while covering nothing
        // about the member it never looks at, which is exactly how a duplicated
        // key would reach a cut unnoticed.
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                let (a, b) = (&members[i], &members[j]);
                assert_ne!(
                    a.public_key, b.public_key,
                    "{} and {} must be distinct keys",
                    a.producer_id, b.producer_id
                );
                assert_ne!(
                    a.payout_address, b.payout_address,
                    "{} and {} must each have their own payout address",
                    a.producer_id, b.producer_id
                );
                assert_ne!(a.producer_id, b.producer_id);
            }
        }
    }

    #[test]
    fn pinned_testnet_keys_are_valid_ed25519_points() {
        // `parse_and_validate` already rejects off-curve keys, but assert it
        // directly: a key that is 64 valid hex characters but not a curve point
        // would be seeded into genesis and could never sign anything.
        for (id, _, pubkey) in [CEREMONY_A, CEREMONY_B, CEREMONY_C] {
            let bytes: [u8; 32] = hex::decode(pubkey)
                .expect("64 hex chars")
                .try_into()
                .expect("32 bytes");
            PublicKey::from_bytes(&bytes)
                .unwrap_or_else(|e| panic!("{id}: pinned key is not a valid ed25519 point: {e}"));
        }
    }

    #[test]
    fn testnet_network_id_resolves_the_pinned_set_without_extra_config() {
        // The cut path: WRFCOIN_NETWORK_ID=testnet and nothing else. This is what
        // makes a fresh genesis boot ENFORCED rather than unauthenticated.
        let members =
            resolve_from_values(None, Some("testnet"), None, TESTNET_GENESIS_PRODUCER_SET)
                .expect("testnet must resolve the pinned set");

        assert_eq!(members.len(), 3);
        assert_eq!(members[0].producer_id, CEREMONY_A.0);
        assert_eq!(members[1].producer_id, CEREMONY_B.0);
        assert_eq!(members[2].producer_id, CEREMONY_C.0);

        // ...and it satisfies the fail-closed switch, so the cut compose's
        // WRFCOIN_REQUIRE_PRODUCER_AUTH=1 does not abort startup.
        let with_guard = resolve_from_values(
            None,
            Some("testnet"),
            Some("1"),
            TESTNET_GENESIS_PRODUCER_SET,
        )
        .expect("a non-empty pinned set satisfies REQUIRE_PRODUCER_AUTH");
        assert_eq!(with_guard.len(), 3);
    }

    // --- resolution precedence -------------------------------------------------

    #[test]
    fn explicit_source_wins_over_pinned_testnet() {
        let doc = doc_with(&member_json("explicit", 1));
        let pinned = doc_with(&member_json("pinned", 2));
        let members = resolve_from_values(Some(&doc), Some("testnet"), None, Some(&pinned))
            .expect("resolves");
        assert_eq!(members[0].producer_id, "explicit");
    }

    #[test]
    fn testnet_falls_back_to_pinned_set() {
        let pinned = doc_with(&member_json("pinned", 2));
        let members =
            resolve_from_values(None, Some("testnet"), None, Some(&pinned)).expect("resolves");
        assert_eq!(members[0].producer_id, "pinned");
    }

    #[test]
    fn unconfigured_non_testnet_resolves_empty() {
        let members = resolve_from_values(None, None, None, None).expect("resolves");
        assert!(members.is_empty(), "unconfigured means not enforced");
    }

    #[test]
    fn testnet_without_a_pinned_set_is_a_hard_failure() {
        // Fresh testnet genesis with no ceremony output pinned must NOT silently
        // start an unauthenticated chain.
        let err = resolve_from_values(None, Some("testnet"), None, None)
            .expect_err("unpinned testnet genesis must refuse to start");
        assert!(
            err.contains("no testnet genesis producer set"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn invalid_explicit_source_aborts_and_never_degrades_to_empty() {
        // The core downgrade hole: a broken config must not fall through to the
        // empty (= unenforced) set.
        let err = resolve_from_values(Some(r#"{"version":1,"members":[]}"#), None, None, None)
            .expect_err("an invalid explicit set must abort startup");
        assert!(
            err.contains(GENESIS_PRODUCER_SET_ENV),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn require_producer_auth_rejects_a_resolved_empty_set() {
        let err = resolve_from_values(None, None, Some("1"), None)
            .expect_err("require-auth plus an empty set must abort startup");
        assert!(
            err.contains("would NOT be enforced"),
            "unexpected error: {err}"
        );
    }

    // --- the fail-closed switch itself (Frank #2397 blocker 1) ------------------

    #[test]
    fn malformed_require_auth_switch_aborts_instead_of_reading_as_off() {
        // The reported bug: `tru` read as false, so an unconfigured fresh chain
        // resolved an empty set and started UNAUTHENTICATED — while the operator
        // believed they had just enabled authentication.
        for typo in ["tru", "ture", "yes", "on", "enabled", "2", "-1", "off "] {
            let err = resolve_from_values(None, None, Some(typo), None)
                .expect_err("a malformed switch must abort, not silently disable the guard");
            assert!(
                err.contains(REQUIRE_PRODUCER_AUTH_ENV) && err.contains("not a recognized value"),
                "'{typo}' must fail as an unrecognized switch value, got: {err}"
            );
        }
    }

    // ---- #2501: the restored-state producer-set invariant --------------------

    /// The refusal case: the deployment declares it requires producer auth and
    /// the restored chain has no set. This is where a cut that reuses a data
    /// volume lands, and the one state no other guard sees.
    #[test]
    fn restored_chain_with_no_producer_set_is_refused_when_auth_is_required() {
        let err = check_restored_chain_producer_set(Some("1"), true)
            .expect_err("required + unseeded must refuse");
        assert!(
            err.contains(REQUIRE_PRODUCER_AUTH_ENV),
            "the refusal must name the flag that triggered it: {err}"
        );
        assert!(
            err.contains("CLEAN volume") && err.contains("resync"),
            "and must name BOTH remedies, because the two causes need different \
             operator actions — a reused volume vs a set lost on replay: {err}"
        );
    }

    /// A restored chain that HAS a set is the ordinary restart, and must start.
    #[test]
    fn restored_chain_with_a_producer_set_starts() {
        assert_eq!(check_restored_chain_producer_set(Some("1"), false), Ok(()));
    }

    /// The flag off or absent must not brick legacy and dev chains, which
    /// legitimately have no producer set. This is the assertion that makes the
    /// refusal above safe to ship — without it the change would turn a silent
    /// hole into a mass outage, which is the worse failure.
    #[test]
    fn an_unconfigured_deployment_is_unaffected() {
        for raw in [None, Some(""), Some("0"), Some("false")] {
            assert_eq!(
                check_restored_chain_producer_set(raw, true),
                Ok(()),
                "raw={raw:?} must not refuse — a deployment that never declared it \
                 requires producer auth keeps working with no set"
            );
        }
    }

    /// The fallible parse survives promotion to a whole-startup invariant: an
    /// unrecognized value ABORTS rather than reading as "not required". Off is
    /// the dangerous direction for this switch (#2397 polarity), so a typo by an
    /// operator who meant to ENABLE auth must never silently disable this check.
    #[test]
    fn an_unrecognized_flag_value_aborts_rather_than_reading_as_off() {
        for typo in ["tru", "yes-please", "enabled?", "2"] {
            let err = check_restored_chain_producer_set(Some(typo), false)
                .expect_err("an unrecognized value must abort");
            assert!(
                err.contains("not a recognized value"),
                "typo={typo:?} must abort on the parse, not be treated as off: {err}"
            );
        }
    }

    #[test]
    fn require_auth_accepts_documented_on_and_off_spellings() {
        for on in ["1", "true", "TRUE", "  True  "] {
            let err = resolve_from_values(None, None, Some(on), None)
                .expect_err("an empty set with the guard ON must abort");
            assert!(err.contains("would NOT be enforced"), "for '{on}': {err}");
        }
        for off in ["0", "false", "FALSE", "  0 ", ""] {
            let members = resolve_from_values(None, None, Some(off), None)
                .unwrap_or_else(|e| panic!("'{off}' must be accepted as OFF: {e}"));
            assert!(members.is_empty());
        }
    }

    #[test]
    fn malformed_require_auth_switch_aborts_even_when_the_set_is_non_empty() {
        // The switch is only *consulted* when the set is empty, but it must be
        // *validated* on every start — otherwise a typo survives unnoticed until
        // the one boot where it was the last line of defence.
        let doc = doc_with(&member_json("n5", 1));
        let err = resolve_from_values(Some(&doc), None, Some("tru"), None)
            .expect_err("a malformed security switch must abort even when it is moot this run");
        assert!(
            err.contains(REQUIRE_PRODUCER_AUTH_ENV),
            "unexpected error: {err}"
        );
        assert!(
            err.contains("tru"),
            "the error must name the offending value: {err}"
        );
    }

    #[test]
    fn require_producer_auth_is_satisfied_by_a_non_empty_set() {
        let doc = doc_with(&member_json("n5", 1));
        let members = resolve_from_values(Some(&doc), None, Some("true"), None).expect("resolves");
        assert_eq!(members.len(), 1);
    }

    // --- the real env seam ------------------------------------------------------
    //
    // `resolve()` is the function startup actually calls; the tests above exercise
    // its value-parameterized core. These drive the **process environment** so the
    // env-var wiring itself is covered — a resolver that is correct but reads the
    // wrong variable would pass every test above.
    //
    // Process env is global, so these serialize on a mutex. The full
    // process-level proof (spawn the binary, assert exit code) needs #2398's
    // wiring, since nothing calls `resolve()` in this PR yet — see the PR comment.

    // The process-env lock lives in `helpers::test_env` and is THE single lock
    // for this binary. It used to be a local static here, which did not exclude
    // `preflight_tests`' separate mutex over `WRFCOIN_NETWORK_ID` (#2398).
    use crate::helpers::test_env;

    /// Run `body` with the three env vars set to the given values (None =
    /// unset), restoring the previous environment afterwards even on panic.
    ///
    /// `pub(crate)` so #2398's startup-wiring tests in `main.rs` serialize on
    /// **this same** `ENV_LOCK`. A second, independent mutex over the same
    /// three process-global variables would race with the tests below.
    pub(crate) fn with_env<T>(
        set: Option<&str>,
        network: Option<&str>,
        require: Option<&str>,
        body: impl FnOnce() -> T,
    ) -> T {
        let guard = test_env::lock();
        let vars = [
            (GENESIS_PRODUCER_SET_ENV, set),
            (NETWORK_ID_ENV, network),
            (REQUIRE_PRODUCER_AUTH_ENV, require),
        ];
        let saved: Vec<_> = vars
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
        for (k, v) in saved {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        drop(guard);
        match out {
            Ok(v) => v,
            Err(p) => std::panic::resume_unwind(p),
        }
    }

    #[test]
    fn env_seam_malformed_require_auth_aborts_startup() {
        // Frank's exact reproduction, driven through the real environment:
        // WRFCOIN_REQUIRE_PRODUCER_AUTH=tru, no producer-set config, no testnet
        // selector. Before the fix this resolved an empty set and the node
        // carried on to start an unauthenticated chain.
        let err = with_env(None, None, Some("tru"), resolve)
            .expect_err("a typo'd fail-closed switch must abort startup");
        assert!(
            err.contains(REQUIRE_PRODUCER_AUTH_ENV) && err.contains("not a recognized value"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn env_seam_reads_each_variable_from_the_environment() {
        // Guards against the resolver reading the wrong variable name.
        let doc = doc_with(&member_json("from-env", 1));
        let members = with_env(Some(&doc), None, Some("1"), resolve).expect("resolves from env");
        assert_eq!(members[0].producer_id, "from-env");

        // NETWORK_ID is genuinely consulted: testnet resolves the PINNED set.
        //
        // Before the 2026-07-24 ceremony this asserted a hard failure, because
        // TESTNET_GENESIS_PRODUCER_SET was None. It is now pinned with real
        // ceremony output, so the correct assertion is that testnet yields the
        // real committee. The unpinned-abort path is still covered by
        // `testnet_without_a_pinned_set_is_a_hard_failure`, which passes None
        // explicitly rather than relying on the constant.
        let members = with_env(None, Some("testnet"), None, resolve)
            .expect("testnet resolves the pinned genesis producer set");
        assert_eq!(members.len(), 3, "the pinned testnet set has three members");

        // And the legacy branch really is reachable when nothing is configured.
        let members = with_env(None, None, None, resolve).expect("legacy branch resolves");
        assert!(members.is_empty());
    }

    #[test]
    fn blank_explicit_source_is_treated_as_unset() {
        // An empty env var is how Docker passes "not set"; it must fall through to
        // the pinned set rather than being parsed as empty JSON.
        let pinned = doc_with(&member_json("pinned", 2));
        let members = resolve_from_values(Some("   "), Some("testnet"), None, Some(&pinned))
            .expect("resolves");
        assert_eq!(members[0].producer_id, "pinned");
    }
}
