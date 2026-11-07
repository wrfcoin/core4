//! Limited station registrar key (`ConsensusFeature::StationRegistrarKey`).
//!
//! From the feature's activation a station registration may be authorized by a
//! key pinned for this chain instance in
//! [`STATION_REGISTRAR_KEY_PINS`](crate::governance::schedule_extension::STATION_REGISTRAR_KEY_PINS)
//! instead of an oracle-governance key, but only for a NEW phone-class station
//! inside the limits below. The governance path is untouched.
//!
//! # Consensus limits (every node checks these, deterministically)
//!
//! | Rule | Where |
//! |---|---|
//! | `hardware_model == phone-barometer`, tier `unverified`, provenance `real` | [`registrar_shape_refusal`] |
//! | Owner proof present AND bound to `owner_address` | [`registrar_shape_refusal`] |
//! | `station_id == mob- + hash(owner address, slot 0)` | [`registrar_shape_refusal`] |
//! | v2 with `expected_nonce` 0 | [`registrar_shape_refusal`] |
//! | Coordinates exactly on the 0.1 degree grid, no elevation | [`registrar_shape_refusal`] |
//! | Station not already registered, owner-ledger nonce 0 | the apply arm (`State`) |
//! | Fewer than [`PHONE_STATION_CEILING`] ACTIVE phone stations | the apply arm (`State`) |
//! | At most one registrar registration per block | [`RegistrarBlockSlot`] in both extractors |
//!
//! These add no state: the ceiling is counted from the station registry, the
//! per-block slot from the block's own transaction order.
//!
//! # Not consensus
//!
//! The daily total (20), per-IP and per-cell limits (3 each), the 24 h hold and
//! the stop switch are signing-service policy: they decide whether the online
//! key signs at all, and can change without a release. [`policy`] is the
//! reference implementation the service uses; no node reads it.

use crate::blockchain::state::StateTransition;
use crate::blockchain::transaction::StationRegistrationPayload;
use crate::crypto::chain_binding::{ConsensusFeature, feature_active};
use crate::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationRegistry, WeatherStation,
};

/// The only hardware model a registrar key may register.
pub const PHONE_HARDWARE_MODEL: &str = "phone-barometer";

/// Compiled ceiling on ACTIVE `phone-barometer` stations in the registry
/// (however registered). Deactivating a station frees its slot (owner
/// decision, 2026-11-07). A registrar registration that would reach past it is
/// an inert no-op.
pub const PHONE_STATION_CEILING: usize = 128;

/// Domain of the mobile station id, identical to the backend's
/// `deriveMobileStationId` (`backend/shared/mobile-station-messages.ts`).
pub const MOBILE_STATION_ID_DOMAIN: &[u8] = b"wrfcoin_mobile_station_v1";

/// The only slot a registrar key may register: one station per wallet.
pub const MOBILE_STATION_SLOT: u8 = 0;

/// Whether [`ConsensusFeature::StationRegistrarKey`] is in force at `height`.
pub fn station_registrar_key_active(height: u64) -> bool {
    feature_active(ConsensusFeature::StationRegistrarKey, height)
}

/// The registrar keys in force at `height` on this node's chain instance: none
/// before activation, none on a chain with no pinned key, and none on a node
/// that has not installed its chain authority (fail closed).
pub fn registrar_keys_at(height: u64) -> Vec<String> {
    if !station_registrar_key_active(height) {
        return Vec::new();
    }
    crate::governance::schedule_extension::schedule_extension_authority()
        .map(|authority| {
            crate::governance::schedule_extension::station_registrar_keys(authority.genesis_hash())
        })
        .unwrap_or_default()
}

/// `mob-` + the first 32 hex of
/// `SHA-256("wrfcoin_mobile_station_v1" || 0x00 || address20 || slot)`.
/// `owner_address` is `0x`/`wrf`-prefixed or bare hex of exactly 20 bytes;
/// anything else is `None`.
pub fn mobile_station_id(owner_address: &str, slot: u8) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bare = owner_address
        .trim_start_matches("0x")
        .trim_start_matches("wrf");
    let bytes = hex::decode(bare).ok()?;
    if bytes.len() != 20 {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(MOBILE_STATION_ID_DOMAIN);
    hasher.update([0u8]);
    hasher.update(&bytes);
    hasher.update([slot]);
    let digest = hex::encode(hasher.finalize());
    Some(format!("mob-{}", &digest[..32]))
}

/// Is `value` exactly a multiple of 0.1 degree as stored: does
/// `round(value * 10) / 10` reproduce its bits?
// determinism-ok: one IEEE-754 multiply, round and divide, each correctly rounded on every conforming platform; compared bit for bit, never accumulated.
pub fn on_tenth_degree_grid(value: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    // determinism-ok: correctly rounded IEEE multiply by an exact integer constant.
    let tenths = (value * 10.0).round();
    // determinism-ok: correctly rounded IEEE divide; bitwise comparison.
    (tenths / 10.0).to_bits() == value.to_bits()
}

/// Is a 1e-7 degree fixed-point value on the 0.1 degree grid?
pub fn on_tenth_degree_grid_e7(value_e7: i64) -> bool {
    value_e7 % 1_000_000 == 0
}

/// Why a registrar-signed payload is outside the registrar's stateless limits,
/// or `None` when it is inside them. `fixed_point` is whether
/// `station_coordinates_fixed_point` is active at the carrying height: the
/// grid is then checked on the signed integers.
pub fn registrar_shape_refusal(
    payload: &StationRegistrationPayload,
    fixed_point: bool,
) -> Option<&'static str> {
    if payload.hardware_model != PHONE_HARDWARE_MODEL {
        return Some("hardware_model must be phone-barometer");
    }
    if payload.data_quality_tier != DataQualityTier::Unverified {
        return Some("data_quality_tier must be unverified");
    }
    if payload.provenance_class != ContributionProvenanceClass::Real {
        return Some("provenance_class must be real");
    }
    if !payload.owner_address_is_bound_to_owner_pubkey() {
        return Some("owner proof must be bound to owner_address");
    }
    if mobile_station_id(&payload.owner_address, MOBILE_STATION_SLOT).as_deref()
        != Some(payload.station_id.as_str())
    {
        return Some("station_id must be the owner's slot-0 mob- id");
    }
    if !payload.is_v2() || payload.expected_nonce != Some(0) {
        return Some("must be v2 with expected_nonce 0");
    }
    if payload.elevation_meters.is_some() || payload.elevation_mm.is_some() {
        return Some("elevation must be absent");
    }
    let on_grid = if fixed_point {
        matches!(
            (payload.latitude_e7, payload.longitude_e7),
            (Some(lat), Some(lon)) if on_tenth_degree_grid_e7(lat) && on_tenth_degree_grid_e7(lon)
        )
    } else {
        on_tenth_degree_grid(payload.latitude) && on_tenth_degree_grid(payload.longitude)
    };
    if !on_grid {
        return Some("coordinates must be on the 0.1 degree grid");
    }
    None
}

/// Number of ACTIVE `phone-barometer` stations in `registry`: what the
/// ceiling counts. An inactive station holds no slot.
pub fn phone_station_count(registry: &StationRegistry) -> usize {
    // determinism-ok: a count of matching entries does not depend on iteration order.
    registry
        .entries()
        .filter(|(_, s)| s.is_active && s.hardware_model == PHONE_HARDWARE_MODEL)
        .count()
}

/// Why a registrar registration that passed the stateless checks does not
/// apply to a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrarStateRefusal {
    /// The station id has a record (active or not). Never applies later.
    AlreadyRegistered,
    /// The station's owner-ledger nonce is not 0. Never applies later.
    NonceNotZero,
    /// [`PHONE_STATION_CEILING`] active phone stations. May apply once a slot
    /// is freed.
    CeilingReached,
}

impl RegistrarStateRefusal {
    /// Whether the same registration could apply to a later state: only a
    /// full ceiling can clear.
    pub fn may_clear(self) -> bool {
        matches!(self, Self::CeilingReached)
    }
}

/// Why a registrar registration that passed the stateless checks does not
/// apply to the current state, or `None` when it does.
pub fn registrar_state_refusal(
    registry: &StationRegistry,
    owner_nonce: u64,
    station: &WeatherStation,
) -> Option<RegistrarStateRefusal> {
    // Any record, active or not: a deactivated `mob-` id is never
    // re-registered through the registrar (only governance can touch it).
    if registry.get(&station.station_id).is_some() {
        return Some(RegistrarStateRefusal::AlreadyRegistered);
    }
    if owner_nonce != 0 {
        return Some(RegistrarStateRefusal::NonceNotZero);
    }
    if phone_station_count(registry) >= PHONE_STATION_CEILING {
        return Some(RegistrarStateRefusal::CeilingReached);
    }
    None
}

/// The per-block registrar slot. Both extractors (the PoP producer/verify path
/// and the P2P/WAL applier) walk a block's transactions in order through one
/// of these: the FIRST registrar registration in the block keeps its
/// transition, whether or not it later applies; every later one yields none.
/// Pure over the block, so producer == applier == WAL replay.
#[derive(Debug, Default)]
pub struct RegistrarBlockSlot {
    used: bool,
}

impl RegistrarBlockSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pass `transition` through, unless it is a registrar registration and
    /// this block already had one.
    pub fn admit(&mut self, transition: Option<StateTransition>) -> Option<StateTransition> {
        match transition {
            Some(StateTransition::ApplyStationRegistrarRegistration { .. }) if self.used => None,
            Some(t @ StateTransition::ApplyStationRegistrarRegistration { .. }) => {
                self.used = true;
                Some(t)
            },
            other => other,
        }
    }
}

/// What block selection does with each registrar registration in a candidate.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RegistrarSelection {
    /// Indices (into the candidate) to take out of the block but KEEP in the
    /// mempool: past the per-block slot, or refused only by a full ceiling.
    pub defer: Vec<usize>,
    /// Indices to take out of the block AND evict: they can never apply
    /// (station already registered, nonce not 0).
    pub evict: Vec<usize>,
}

/// PRODUCER SELECTION ONLY, never validity: pick at most one registrar
/// registration for the next block, and only one that would actually apply.
/// `state` is the parent state; the candidate's station registrations and
/// owner-ledger actions are replayed on a copy in block order, so an earlier
/// transaction that fills the ceiling or registers the same id is seen.
/// Block validity, the classifier and the apply arm are unchanged: a block
/// carrying two still verifies, the second is just inert.
pub fn select_registrar_registrations(
    state: &crate::blockchain::state::State,
    height: u64,
    transactions: &[crate::blockchain::transaction::Transaction],
) -> RegistrarSelection {
    use crate::blockchain::transaction::TransactionType;
    let mut selection = RegistrarSelection::default();
    if !station_registrar_key_active(height) {
        return selection;
    }
    let steps: Vec<Option<StateTransition>> = transactions
        .iter()
        .map(|tx| {
            if tx.tx_type != TransactionType::StationRegistration {
                return None;
            }
            if tx.parse_station_registration().is_some() {
                tx.station_registration_step(height)
            } else {
                tx.station_owner_ledger_transition(height)
            }
        })
        .collect();
    if !steps
        .iter()
        .any(|t| matches!(t, Some(StateTransition::ApplyStationRegistrarRegistration { .. })))
    {
        return selection;
    }
    let mut replay = state.clone();
    let mut slot_used = false;
    for (index, step) in steps.into_iter().enumerate() {
        let Some(transition) = step else { continue };
        if let StateTransition::ApplyStationRegistrarRegistration { station, .. } = &transition {
            // A registration that can never apply is evicted whatever the
            // slot says; one that may apply later waits.
            let nonce = replay.station_owner_ledger_view(&station.station_id).owner_nonce;
            match registrar_state_refusal(&replay.station_registry, nonce, station) {
                Some(refusal) if !refusal.may_clear() => {
                    selection.evict.push(index);
                    continue;
                },
                Some(_) => {
                    selection.defer.push(index);
                    continue;
                },
                None if slot_used => {
                    selection.defer.push(index);
                    continue;
                },
                None => slot_used = true,
            }
        }
        // A refused step is an inert no-op on chain too; ignore it here.
        let _ = replay.apply_transition(transition);
    }
    selection
}

/// Signing-service policy for the online registrar key. **NOT CONSENSUS**: no
/// node reads this. The service that holds the online key runs every request
/// through a [`RegistrarPolicyLedger`](policy::RegistrarPolicyLedger) before it
/// signs, so a bad day can be stopped before anything reaches the chain. All
/// limits can change without a release.
pub mod policy {
    /// The approved limits (owner, 2026-11-07).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RegistrarPolicyLimits {
        /// One setting turns signing off. Existing stations stay.
        pub enabled: bool,
        /// Registrations accepted in any trailing 24 h, in total.
        pub daily_total: usize,
        /// Per public IP (salted hash), in any trailing 24 h.
        pub daily_per_ip: usize,
        /// Per 0.1 degree cell, in any trailing 24 h.
        pub daily_per_cell: usize,
        /// Seconds between acceptance and signing.
        pub hold_secs: u64,
    }

    impl Default for RegistrarPolicyLimits {
        fn default() -> Self {
            Self {
                enabled: true,
                daily_total: 20,
                daily_per_ip: 3,
                daily_per_cell: 3,
                hold_secs: DAY_SECS,
            }
        }
    }

    /// 24 h in seconds.
    pub const DAY_SECS: u64 = 24 * 60 * 60;

    /// One registration request, as the service sees it. No personal data:
    /// `ip_hash` is a salted hash, `cell` is integer tenths of a degree.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RegistrarRequest {
        pub station_id: String,
        pub ip_hash: String,
        pub cell: (i32, i32),
    }

    /// Why the service refuses or delays a request.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum RegistrarPolicyRefusal {
        Stopped,
        AlreadyRequested,
        DailyTotal,
        DailyPerIp,
        DailyPerCell,
        /// Accepted, but not signable before `until` (unix seconds).
        Held {
            until: u64,
        },
        Unknown,
    }

    #[derive(Debug, Clone)]
    struct Accepted {
        request: RegistrarRequest,
        at: u64,
    }

    /// The service's record of accepted requests (kept 30 days by the service).
    #[derive(Debug, Clone, Default)]
    pub struct RegistrarPolicyLedger {
        accepted: Vec<Accepted>,
    }

    impl RegistrarPolicyLedger {
        pub fn new() -> Self {
            Self::default()
        }

        fn in_window(&self, now: u64) -> impl Iterator<Item = &Accepted> {
            self.accepted.iter().filter(move |a| a.at + DAY_SECS > now)
        }

        /// Accept `request` at `now` (unix seconds) if every limit allows it,
        /// and record it. Accepting starts the hold; it does not sign.
        pub fn accept(
            &mut self,
            limits: &RegistrarPolicyLimits,
            request: RegistrarRequest,
            now: u64,
        ) -> Result<(), RegistrarPolicyRefusal> {
            use RegistrarPolicyRefusal as R;
            if !limits.enabled {
                return Err(R::Stopped);
            }
            if self
                .accepted
                .iter()
                .any(|a| a.request.station_id == request.station_id)
            {
                return Err(R::AlreadyRequested);
            }
            if self.in_window(now).count() >= limits.daily_total {
                return Err(R::DailyTotal);
            }
            if self
                .in_window(now)
                .filter(|a| a.request.ip_hash == request.ip_hash)
                .count()
                >= limits.daily_per_ip
            {
                return Err(R::DailyPerIp);
            }
            if self
                .in_window(now)
                .filter(|a| a.request.cell == request.cell)
                .count()
                >= limits.daily_per_cell
            {
                return Err(R::DailyPerCell);
            }
            self.accepted.push(Accepted { request, at: now });
            Ok(())
        }

        /// May the online key sign `station_id` at `now`? Only an accepted
        /// request whose hold has passed, and only while signing is on.
        pub fn ready_to_release(
            &self,
            limits: &RegistrarPolicyLimits,
            station_id: &str,
            now: u64,
        ) -> Result<(), RegistrarPolicyRefusal> {
            if !limits.enabled {
                return Err(RegistrarPolicyRefusal::Stopped);
            }
            let accepted = self
                .accepted
                .iter()
                .find(|a| a.request.station_id == station_id)
                .ok_or(RegistrarPolicyRefusal::Unknown)?;
            let until = accepted.at.saturating_add(limits.hold_secs);
            if now < until {
                return Err(RegistrarPolicyRefusal::Held { until });
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::policy::*;
    use super::*;

    /// Same vector as the backend's `deriveMobileStationId`
    /// (`backend/shared/mobile-station-messages.ts`).
    #[test]
    fn mobile_station_id_matches_the_backend_derivation() {
        let address = "0x00112233445566778899aabbccddeeff00112233";
        assert_eq!(
            mobile_station_id(address, 0).as_deref(),
            Some("mob-5d270fa8c768f23260a9f9f8de63ab64")
        );
        assert_eq!(
            mobile_station_id(address, 1).as_deref(),
            Some("mob-1aef6cb885098fadc032bac1bf32f47f")
        );
        // The same 20 bytes under the other presentations give the same id.
        assert_eq!(
            mobile_station_id("wrf00112233445566778899aabbccddeeff00112233", 0),
            mobile_station_id(address, 0)
        );
        assert_eq!(mobile_station_id("0x0011", 0), None);
        assert_eq!(mobile_station_id("not hex", 0), None);
    }

    #[test]
    fn tenth_degree_grid() {
        for on in [0.0, -0.0, 46.8, -92.1, 90.0, -180.0, 0.1, 12.3] {
            assert!(on_tenth_degree_grid(on), "{on}");
        }
        for off in [46.78, -92.15, 0.05, 46.800001, f64::NAN, f64::INFINITY] {
            assert!(!on_tenth_degree_grid(off), "{off}");
        }
        assert!(on_tenth_degree_grid_e7(468_000_000));
        assert!(on_tenth_degree_grid_e7(-921_000_000));
        assert!(!on_tenth_degree_grid_e7(467_800_000));
        assert!(!on_tenth_degree_grid_e7(1));
    }

    #[test]
    fn block_slot_admits_one_registrar_registration_and_everything_else() {
        let station = WeatherStation {
            station_id: "mob-x".into(),
            owner_address: "0x00".into(),
            location: crate::station_registry::StationLocation {
                latitude: 0.0,
                longitude: 0.0,
                elevation_meters: None,
            },
            hardware_model: PHONE_HARDWARE_MODEL.into(),
            calibration_date: None,
            is_active: true,
            reputation_score: 0,
            data_quality_tier: DataQualityTier::Unverified,
            provenance_class: ContributionProvenanceClass::Real,
            ring_public_key: None,
        };
        let registrar = || StateTransition::ApplyStationRegistrarRegistration {
            station: station.clone(),
            block_height: 1,
        };
        let governance = || StateTransition::ApplyStationRegistration {
            station: station.clone(),
        };
        let mut slot = RegistrarBlockSlot::new();
        assert!(slot.admit(None).is_none());
        assert!(slot.admit(Some(governance())).is_some());
        assert!(
            slot.admit(Some(registrar())).is_some(),
            "the first takes the slot"
        );
        assert!(
            slot.admit(Some(registrar())).is_none(),
            "a second in the same block yields nothing"
        );
        assert!(
            slot.admit(Some(governance())).is_some(),
            "governance is never limited"
        );
        let mut next_block = RegistrarBlockSlot::new();
        assert!(
            next_block.admit(Some(registrar())).is_some(),
            "a new block has a new slot"
        );
    }

    fn request(n: u32, ip: &str, cell: (i32, i32)) -> RegistrarRequest {
        RegistrarRequest {
            station_id: format!("mob-{n}"),
            ip_hash: ip.into(),
            cell,
        }
    }

    /// Service policy (not consensus): 20 a day in total, rolling 24 h.
    #[test]
    fn policy_refuses_over_twenty_a_day() {
        let limits = RegistrarPolicyLimits::default();
        let mut ledger = RegistrarPolicyLedger::new();
        let t0 = 1_000_000;
        for n in 0..20 {
            // Spread over distinct IPs and cells so only the total binds.
            let r = request(n, &format!("ip{n}"), (n as i32, 0));
            assert_eq!(ledger.accept(&limits, r, t0 + u64::from(n)), Ok(()), "{n}");
        }
        assert_eq!(
            ledger.accept(&limits, request(20, "ip20", (20, 0)), t0 + 100),
            Err(RegistrarPolicyRefusal::DailyTotal)
        );
        // 24 h after the first acceptance, its slot is free again.
        assert_eq!(
            ledger.accept(&limits, request(20, "ip20", (20, 0)), t0 + DAY_SECS),
            Ok(())
        );
    }

    #[test]
    fn policy_refuses_over_three_per_ip_and_per_cell() {
        let limits = RegistrarPolicyLimits::default();
        let mut ledger = RegistrarPolicyLedger::new();
        for n in 0..3 {
            assert_eq!(
                ledger.accept(&limits, request(n, "same-ip", (n as i32, 0)), 10),
                Ok(())
            );
        }
        assert_eq!(
            ledger.accept(&limits, request(3, "same-ip", (9, 9)), 11),
            Err(RegistrarPolicyRefusal::DailyPerIp)
        );
        for n in 10..13 {
            assert_eq!(
                ledger.accept(&limits, request(n, &format!("ip{n}"), (468, -921)), 12),
                Ok(())
            );
        }
        assert_eq!(
            ledger.accept(&limits, request(13, "ip13", (468, -921)), 13),
            Err(RegistrarPolicyRefusal::DailyPerCell)
        );
        assert_eq!(
            ledger.accept(&limits, request(10, "ip99", (1, 1)), 14),
            Err(RegistrarPolicyRefusal::AlreadyRequested)
        );
    }

    #[test]
    fn policy_holds_for_twenty_four_hours_and_has_a_stop_switch() {
        let mut limits = RegistrarPolicyLimits::default();
        let mut ledger = RegistrarPolicyLedger::new();
        let t0 = 5_000;
        ledger
            .accept(&limits, request(1, "ip", (0, 0)), t0)
            .unwrap();
        assert_eq!(
            ledger.ready_to_release(&limits, "mob-1", t0 + DAY_SECS - 1),
            Err(RegistrarPolicyRefusal::Held {
                until: t0 + DAY_SECS
            })
        );
        assert_eq!(
            ledger.ready_to_release(&limits, "mob-1", t0 + DAY_SECS),
            Ok(())
        );
        assert_eq!(
            ledger.ready_to_release(&limits, "mob-2", t0 + DAY_SECS),
            Err(RegistrarPolicyRefusal::Unknown)
        );
        limits.enabled = false;
        assert_eq!(
            ledger.ready_to_release(&limits, "mob-1", t0 + DAY_SECS),
            Err(RegistrarPolicyRefusal::Stopped)
        );
        assert_eq!(
            ledger.accept(&limits, request(2, "ip2", (1, 1)), t0),
            Err(RegistrarPolicyRefusal::Stopped)
        );
    }
}
