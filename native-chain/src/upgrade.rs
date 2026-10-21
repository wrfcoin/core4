//! Node-side upgrade deadline and clean halt (#3093).
//!
//! At boot `launch_api` asks `protocol_support::upgrade_required_at` whether
//! the genesis schedule activates a feature this binary does not implement.
//! If it does, and the local tip is already at or beyond that height, the
//! node refuses to start. Otherwise the deadline is installed here and
//! consulted by the two places a block enters the chain — the `/blocks/mine`
//! producer and the P2P apply path — which refuse anything at or beyond the
//! height and request a **clean halt**: the shutdown task disconnects P2P,
//! writes the checkpoint, stops HTTP, and exits with
//! [`EXIT_CODE_UPGRADE_REQUIRED`]. The node therefore stops at `H − 1` with
//! its volume intact and a message naming the release to install.
//!
//! Process-wide, like the activation table: the deadline is a property of
//! this binary on this chain, not of a request, and every entry point must
//! see the same value. Replaceable (not `OnceLock`) so tests can install and
//! clear it; production installs exactly once at boot.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use tokio::sync::Notify;
use wrfcoin_shared_protocol::crypto::protocol_support::{
    upgrade_required_message, UpgradeRequired, PROTOCOL_VERSION,
};

/// Process exit code for the clean upgrade halt (`EXIT_UPGRADE_REQUIRED`).
pub const EXIT_CODE_UPGRADE_REQUIRED: i32 = 70;
/// Process exit code for a refused rollback (`EXIT_ROLLBACK_REFUSED`).
pub const EXIT_CODE_ROLLBACK_REFUSED: i32 = 71;

/// The activation this binary cannot follow, installed at boot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeDeadline {
    /// Scheduled feature name this binary lacks.
    pub feature: String,
    /// First height this node must not produce, apply or relay.
    pub height: u64,
    /// This binary's protocol version, for the operator message.
    pub running_protocol_version: u32,
}

impl UpgradeDeadline {
    /// Build from the boot-time check.
    pub fn from_required(required: &UpgradeRequired) -> Self {
        Self {
            feature: required.feature.clone(),
            height: required.height,
            running_protocol_version: PROTOCOL_VERSION,
        }
    }

    fn as_required(&self) -> UpgradeRequired {
        UpgradeRequired {
            feature: self.feature.clone(),
            height: self.height,
        }
    }

    /// Whether a block at `height` is at or beyond the deadline.
    pub fn refuses(&self, height: u64) -> bool {
        height >= self.height
    }
}

static DEADLINE: RwLock<Option<UpgradeDeadline>> = RwLock::new(None);
static HALT_REQUESTED: AtomicBool = AtomicBool::new(false);
static HALT: OnceLock<Arc<Notify>> = OnceLock::new();

/// Install (or clear) the process-wide deadline. Production calls this once
/// at boot; tests may call it repeatedly.
pub fn install_upgrade_deadline(deadline: Option<UpgradeDeadline>) {
    if let Ok(mut slot) = DEADLINE.write() {
        *slot = deadline;
    }
}

/// The installed deadline, if any.
pub fn upgrade_deadline() -> Option<UpgradeDeadline> {
    DEADLINE.read().ok().and_then(|slot| slot.clone())
}

/// The deadline that refuses a block at `height`, if one is installed and
/// `height >= deadline.height`. The one question both entry points ask.
pub fn production_refused_at(height: u64) -> Option<UpgradeDeadline> {
    upgrade_deadline().filter(|d| d.refuses(height))
}

/// The signal the shutdown task awaits. A permit is stored if the request
/// lands before anyone waits, so the halt is never lost.
pub fn halt_signal() -> Arc<Notify> {
    Arc::clone(HALT.get_or_init(|| Arc::new(Notify::new())))
}

/// Whether a clean halt has been requested.
pub fn halt_requested() -> bool {
    HALT_REQUESTED.load(Ordering::SeqCst)
}

/// Log the operator-facing `EXIT_UPGRADE_REQUIRED` line and request the
/// clean halt (once; later calls only log). `surface` names the entry point
/// that hit the deadline.
pub fn request_upgrade_halt(deadline: &UpgradeDeadline, at_height: u64, surface: &str) -> String {
    let message = upgrade_required_message(&deadline.as_required(), at_height);
    log::error!("{message} [{surface}]");
    if !HALT_REQUESTED.swap(true, Ordering::SeqCst) {
        log::error!(
            "Upgrade deadline reached at height {}: stopping cleanly (P2P disconnect, checkpoint, \
             HTTP stop) and exiting with code {EXIT_CODE_UPGRADE_REQUIRED} (#3093)",
            deadline.height
        );
        halt_signal().notify_one();
    }
    message
}

/// Install `candidate` only if no deadline is installed or it is EARLIER
/// than the installed one: the safe composition of independent deadline
/// sources (genesis, a signed notice, applied schedule extensions) is the
/// lowest — stopping early costs a restart, stopping late is the #1972 fork.
/// Returns whether the deadline changed.
pub fn tighten_upgrade_deadline(candidate: UpgradeDeadline, source: &str) -> bool {
    match upgrade_deadline() {
        Some(existing) if existing.height <= candidate.height => false,
        existing => {
            log::warn!(
                "{source} installs a clean-halt deadline at height {} (feature {:?}; was {:?}): \
                 this binary (protocol v{}) does not implement it and will stop cleanly at \
                 height {} (#3093/#3277)",
                candidate.height,
                candidate.feature,
                existing.map(|d| d.height),
                candidate.running_protocol_version,
                candidate.height.saturating_sub(1)
            );
            install_upgrade_deadline(Some(candidate));
            true
        },
    }
}

/// #3277 S3: after the CANONICAL chain state changes (block apply, reorg,
/// boot load, snapshot import), make `feature_active` reflect its applied
/// schedule extensions and tighten the clean-halt deadline to the first
/// effective activation this binary cannot follow. Tighten-only: a deadline
/// is never relaxed, even if a reorg later drops the extension that set it.
/// `surface` names the caller for the log line.
pub fn refresh_schedule_from_canonical_state(
    state: &wrfcoin_shared_protocol::blockchain::state::State,
    surface: &str,
) -> Option<UpgradeDeadline> {
    state.refresh_effective_feature_schedule();
    let effective = wrfcoin_shared_protocol::crypto::chain_binding::effective_feature_schedule();
    let required =
        wrfcoin_shared_protocol::crypto::protocol_support::upgrade_required_in_feature_schedule(
            &effective,
        )?;
    let candidate = UpgradeDeadline::from_required(&required);
    tighten_upgrade_deadline(candidate.clone(), surface);
    Some(candidate)
}

/// #3277 S5 (F1): install the schedule-extension authority for the chain
/// whose durable block 1 is `block1`, from block 1's hash and the
/// compiled-in pins only. THE single production caller of
/// `install_schedule_extension_authority_from_block1`, reached from boot, from
/// the block-1 hook of an empty-volume node, and from `replay_verify`;
/// `authority_has_one_production_call_site` pins that. A chain without a
/// bootstrap row and pin installs nothing, so every extension fails closed.
pub fn install_schedule_extension_authority_for_block1(
    block1: &wrfcoin_shared_protocol::blockchain::block::Block,
) -> bool {
    let hash = block1.hash.clone().unwrap_or_else(|| block1.calculate_hash()).to_hex();
    match wrfcoin_shared_protocol::governance::schedule_extension::install_schedule_extension_authority_from_block1(&hash, Some(block1)) {
        Some(authority) => {
            log::info!(
                "Schedule-extension authority installed for {}/{} (block 1 {}): {} pinned \
                 governance key(s) (#3277)",
                authority.chain_id(),
                authority.network_id(),
                authority.genesis_hash(),
                authority.allowlist().len()
            );
            true
        },
        None => false,
    }
}

/// #3277 S5 (core4#3368 review P2-1/P3-2): the boot gate for a chain that
/// already has blocks. It reads DURABLE block 1 once and arms everything that
/// block keys:
/// - the signing version, activation table and base feature schedule, with
///   the per-chain bootstrap when the block-1 hash matches;
/// - the schedule-extension authority.
///
/// It is the path every live node takes onto B3 when it restarts on its
/// volume.
///
/// Fail-closed: a storage error, an empty archive, or a lowest block that is
/// not height 1 refuses boot. Booting with "no block 1" would silently drop
/// the bootstrap and the authority, and after `H_ENABLE` the node would
/// reject blocks its peers accept.
pub fn arm_block1_protocol_at_boot(
    storage: &crate::storage::Storage,
    configured: Option<wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion>,
    identity: &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity,
) -> Result<wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion, String> {
    arm_block1_protocol_from_lowest_block(
        storage.load_lowest_block().map_err(|e| e.to_string()),
        configured,
        identity,
    )
}

/// [`arm_block1_protocol_at_boot`] after its storage read, so the fail-closed
/// cases can be driven directly.
pub fn arm_block1_protocol_from_lowest_block(
    lowest: Result<Option<wrfcoin_shared_protocol::blockchain::block::Block>, String>,
    configured: Option<wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion>,
    identity: &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity,
) -> Result<wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion, String> {
    let block1 = match lowest {
        Ok(Some(block)) if block.height == 1 => block,
        Ok(Some(block)) => {
            return Err(format!(
                "the lowest persisted block is height {}, not block 1; refusing to boot \
                 without the block-1 protocol inputs (#3277)",
                block.height
            ));
        },
        Ok(None) => {
            return Err("the durable archive holds no block 1 on a chain with blocks; refusing \
                        to boot without the block-1 protocol inputs (#3277)"
                .to_string());
        },
        Err(e) => {
            return Err(format!(
                "failed to read block 1 from durable storage: {e}; refusing to boot without \
                 the block-1 protocol inputs (#3277)"
            ));
        },
    };
    let hash = block1.hash.clone().unwrap_or_else(|| block1.calculate_hash()).to_hex();
    wrfcoin_shared_protocol::crypto::chain_binding::validate_testnet_genesis(&block1)?;
    let pairs: Vec<(&str, &str)> = block1
        .metadata
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let version =
        wrfcoin_shared_protocol::crypto::chain_binding::assert_startup_protocol_compatibility_for_block1(
            pairs,
            configured,
            identity,
            Some(&hash),
        )
        .map_err(|e| e.to_string())?;
    install_schedule_extension_authority_for_block1(&block1);
    Ok(version)
}

/// #3277 F4: when block 1 becomes canonical on a node that booted on an
/// empty volume, re-resolve the base feature schedule from block 1 itself
/// instead of the cut-time configuration the node started with. A no-op on
/// any other block, and on a node whose base already came from block 1.
///
/// Fail-closed: if block 1's schedule cannot be resolved, this binary cannot
/// follow the chain, so a clean-halt deadline is installed at height 2.
pub fn reresolve_base_schedule_if_block1(block: &wrfcoin_shared_protocol::blockchain::block::Block) {
    use wrfcoin_shared_protocol::crypto::chain_binding as cb;
    if block.height != 1 {
        return;
    }
    // #3277 S5: block 1 is now canonical on a node that booted without it,
    // so the schedule-extension authority can be derived from it.
    install_schedule_extension_authority_for_block1(block);
    if !cb::base_feature_schedule_is_provisional() {
        return;
    }
    match cb::reresolve_base_feature_schedule_from_block1(block) {
        Ok(schedule) => log::info!(
            "Base feature schedule re-resolved from synced block 1: {} entr{} (#3277 F4)",
            schedule.entries().len(),
            if schedule.entries().len() == 1 { "y" } else { "ies" }
        ),
        Err(e) => {
            log::error!(
                "Block 1 carries a feature schedule this binary cannot resolve ({e:?}); \
                 stopping cleanly before height 2 (#3277 F4)"
            );
            tighten_upgrade_deadline(
                UpgradeDeadline {
                    feature: "unresolvable genesis activation schedule".into(),
                    height: 2,
                    running_protocol_version: PROTOCOL_VERSION,
                },
                "block 1 schedule re-resolution",
            );
        },
    }
}

/// Test hook: clear the deadline and the halt flag.
pub fn reset_for_tests() {
    install_upgrade_deadline(None);
    HALT_REQUESTED.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `HALT_REQUESTED` and the installed deadline are process-wide, and
    /// cargo runs these tests on parallel threads: one test's
    /// `reset_for_tests()` could clear the flag between the other's request
    /// and its assertion. Each test holds this guard from its first reset to
    /// its last assertion.
    static GLOBAL_STATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serialised() -> std::sync::MutexGuard<'static, ()> {
        GLOBAL_STATE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn deadline(height: u64) -> UpgradeDeadline {
        UpgradeDeadline {
            feature: "future_feature".into(),
            height,
            running_protocol_version: PROTOCOL_VERSION,
        }
    }

    #[test]
    fn deadline_refuses_at_and_beyond_the_height_only() {
        let _serial = serialised();
        reset_for_tests();
        assert_eq!(production_refused_at(10), None, "no deadline installed");
        install_upgrade_deadline(Some(deadline(10)));
        assert_eq!(production_refused_at(9), None);
        assert_eq!(production_refused_at(10), Some(deadline(10)));
        assert_eq!(production_refused_at(u64::MAX), Some(deadline(10)));
        reset_for_tests();
        assert_eq!(upgrade_deadline(), None);
    }

    #[test]
    fn a_halt_is_requested_once_and_the_message_names_everything() {
        let _serial = serialised();
        reset_for_tests();
        let d = deadline(10);
        assert!(!halt_requested());
        let message = request_upgrade_halt(&d, 10, "test");
        assert!(halt_requested());
        assert!(message.contains("EXIT_UPGRADE_REQUIRED"));
        assert!(message.contains("\"future_feature\""));
        assert!(message.contains("height 10"));
        assert!(message.contains(&format!("protocol v{PROTOCOL_VERSION}")));
        // Idempotent.
        request_upgrade_halt(&d, 11, "test");
        assert!(halt_requested());
        reset_for_tests();
    }
}
