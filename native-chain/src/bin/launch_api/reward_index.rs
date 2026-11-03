//! Address index over the paid contribution-reward records (API read only).
//!
//! `GET /addresses/{address}/credits` must return a full page for a wallet with
//! few rewards among many. `State::contribution_reward_records` is keyed by
//! commit id alone, so the only way to answer by address from state is to walk
//! it. This module keeps `address -> commit ids` beside the chain instead.
//!
//! ## Outside consensus, by construction
//!
//! This is a module of the `launch_api` binary (a child of its `state.rs`), so `shared-protocol`
//! and the `native-chain` library cannot name it: block validation, state apply,
//! rewards, the state root and snapshot contents have no path to it. It only
//! takes chain READ guards, is never written to disk and never enters a snapshot.
//!
//! ## State stays the source of truth
//!
//! The index holds only keys. Each read re-checks every key against current
//! state, so a rollback, reorg or snapshot sync cannot produce a row state does
//! not hold, and the amount and paid status are always current. Staleness can
//! only hide a record until the next refresh (at most one tick plus one walk).
//!
//! ## Refresh
//!
//! A refresh walks all records in chunks, taking and releasing the read lock for
//! each chunk, and publishes a whole new index by swapping one `Arc`. It runs
//! only when the tip or record count changed, which covers restart, reorg and
//! snapshot sync with one code path and no persisted format. It never runs on a
//! block-production path. If the tip moves during a walk, the index is published
//! under the tip the walk started from, so the next tick walks again.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Unbounded};
use std::sync::{Arc, RwLock as StdRwLock};

use tokio::sync::RwLock;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::blockchain::state::{ContributionRewardRecord, State};
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_native_chain::crypto::hash::Hash;
use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

/// Records read per chain read-lock hold during a refresh.
const REFRESH_CHUNK_RECORDS: usize = 2_000;
/// How often the tip is compared with the indexed tip. Chain 888 makes a block
/// about every 77 s, so this adds little delay and a tick with no change costs
/// one short read lock.
const REFRESH_TICK_SECS: u64 = 10;
/// Most index keys one request may check under the chain read lock. A wallet's
/// keys are nearly all paid, so a real page stops long before this; the bound
/// exists so that one request's lock hold cannot grow with a wallet's history.
pub const MAX_KEYS_EXAMINED_PER_READ: usize = 1_000;

/// What the chain's paid-credit read exposes: a positive `Real` record, stored
/// under its own commit id, whose payout is final. Shared with the older
/// chain-wide read so that the two routes cannot disagree on what is paid.
pub fn is_paid_credit(
    state: &State,
    address: Address,
    commit_id: &str,
    record: &ContributionRewardRecord,
) -> bool {
    record.address == address
        && record.provenance_class == ContributionProvenanceClass::Real
        && record.amount.into_inner() > 0
        && record.commit_id() == Some(commit_id)
        && state.finalized_contribution_rewards.contains(commit_id)
}

/// Chain position a published index was walked from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IndexSource {
    tip: Hash,
    height: u64,
    records: usize,
}

impl IndexSource {
    fn of(chain: &Blockchain) -> Self {
        Self {
            tip: chain.tip,
            height: chain.height,
            records: chain.state.contribution_reward_records.len(),
        }
    }
}

/// One immutable generation of the index.
pub struct PublishedIndex {
    source: IndexSource,
    by_address: BTreeMap<Address, BTreeSet<Box<str>>>,
}

/// One page of an address's paid credits.
#[derive(Debug)]
pub struct CreditPage {
    pub records: Vec<ContributionRewardRecord>,
    pub keys_examined: usize,
    pub next_cursor: Option<String>,
}

impl PublishedIndex {
    /// Chain height this generation was walked from.
    pub fn height(&self) -> u64 {
        self.source.height
    }

    /// Keyset page in commit-id order, the order and cursor of the chain-wide
    /// read. `next_cursor` is `None` only when no further paid credit is known.
    pub fn page(
        &self,
        state: &State,
        address: Address,
        limit: usize,
        after: Option<&str>,
    ) -> CreditPage {
        let mut page = CreditPage {
            records: Vec::new(),
            keys_examined: 0,
            next_cursor: None,
        };
        let Some(keys) = self.by_address.get(&address) else {
            return page;
        };
        let mut last_returned: Option<&str> = None;
        let mut last_examined: Option<&str> = None;
        for key in keys.range::<str, _>((after.map_or(Unbounded, Excluded), Unbounded)) {
            if page.keys_examined == MAX_KEYS_EXAMINED_PER_READ {
                // Out of budget, so the page may be short. Resume after the last
                // key checked: there may be more, and skipping none is what counts.
                page.next_cursor = last_examined.map(str::to_owned);
                return page;
            }
            page.keys_examined += 1;
            last_examined = Some(key);
            let Some(record) = state.contribution_reward_records.get(key.as_ref()) else {
                continue;
            };
            if !is_paid_credit(state, address, key, record) {
                continue;
            }
            if page.records.len() == limit {
                // One paid credit beyond the page proves there is another page.
                page.next_cursor = last_returned.map(str::to_owned);
                return page;
            }
            page.records.push(record.clone());
            last_returned = Some(key);
        }
        page
    }
}

/// One chunk of the walk.
struct WalkChunk {
    /// The keys to index.
    keys: Vec<(Address, Box<str>)>,
    /// Records read, indexed or not.
    walked: usize,
    /// The last commit id read.
    last: Option<String>,
}

/// Read one chunk under the caller's chain read guard.
fn read_chunk(chain: &Blockchain, after: Option<&str>) -> WalkChunk {
    let mut keys = Vec::new();
    let mut walked = 0;
    let mut last = None;
    let records = chain
        .state
        .contribution_reward_records
        .range::<str, _>((after.map_or(Unbounded, Excluded), Unbounded))
        .take(REFRESH_CHUNK_RECORDS);
    for (commit_id, record) in records {
        walked += 1;
        last = Some(commit_id);
        // Only what can never become a credit is left out. Paid status and
        // amount can change, so reads check those.
        if record.provenance_class == ContributionProvenanceClass::Real {
            keys.push((record.address, commit_id.as_str().into()));
        }
    }
    WalkChunk {
        keys,
        walked,
        last: last.cloned(),
    }
}

/// Shared handle: the current generation, or `None` until the first walk ends.
#[derive(Default)]
pub struct RewardAddressIndex {
    current: StdRwLock<Option<Arc<PublishedIndex>>>,
}

impl RewardAddressIndex {
    /// Current generation. The lock is released on return, so a reader never
    /// holds it across an await or while waiting for the chain lock.
    pub fn published(&self) -> Option<Arc<PublishedIndex>> {
        self.current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn publish(&self, index: PublishedIndex) {
        *self
            .current
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(index));
    }

    /// Walk the records if the chain moved. Returns whether a generation was
    /// published.
    pub async fn refresh(&self, chain: &RwLock<Blockchain>) -> bool {
        let source = IndexSource::of(&*chain.read().await);
        if self.published().is_some_and(|index| index.source == source) {
            return false;
        }
        let mut by_address: BTreeMap<Address, BTreeSet<Box<str>>> = BTreeMap::new();
        let mut after: Option<String> = None;
        loop {
            let chunk = read_chunk(&*chain.read().await, after.as_deref());
            let walked = chunk.walked;
            after = chunk.last.or(after);
            // Sort into the map with no chain lock held.
            for (address, commit_id) in chunk.keys {
                by_address.entry(address).or_default().insert(commit_id);
            }
            if walked < REFRESH_CHUNK_RECORDS {
                break;
            }
            tokio::task::yield_now().await;
        }
        self.publish(PublishedIndex { source, by_address });
        true
    }
}

/// Keep the index current for the life of the process. The first tick fires at
/// once; requests get `503 REWARD_INDEX_WARMING` until that walk ends, and block
/// production does not wait for it.
pub fn spawn_refresh(index: Arc<RewardAddressIndex>, chain: Arc<RwLock<Blockchain>>) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(REFRESH_TICK_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            index.refresh(&chain).await;
        }
    });
}
