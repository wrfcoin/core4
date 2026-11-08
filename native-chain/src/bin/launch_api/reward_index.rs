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
//!
//! ## Newest first (`order=newest`)
//!
//! State does not say when a credit was paid, but each block's `reward_entries`
//! metadata names the decisions it applied. The refresh keeps
//! `commit id -> payout height` (the latest block naming the commit) by scanning
//! each block once: from disk at startup, then only new blocks from the
//! in-memory window. The last [`RECENT_SCAN_WINDOW`] scanned blocks keep their
//! hash, so a reorg or rollback rewinds and rescans them; a reorg deeper than
//! that, or a chain that went backwards past it, rescans from height 1.
//! Newest-first pages walk `(payout height, commit id)` downwards and the cursor
//! is that pair, so rows paid in later blocks sort above any cursor already
//! handed out and never shift a page in progress. A credit whose block this node
//! does not hold (a snapshot-synced node) has no payout height and sorts after
//! all dated ones, by commit id descending.
//!
//! ## Memory
//!
//! Each commit id string is stored once (`Arc<str>`) and shared by the payout
//! height map and both per-address sets. Per indexed credit that is the id
//! (64 bytes for a hex id) plus about 200 bytes of map and set overhead, so
//! roughly 270 bytes; per address about 150 bytes more. 100,000 credits is
//! about 27 MB. Only positive `Real` decisions get a payout height, so
//! synthetic and zero decisions cost nothing beyond the walk.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Bound::{Excluded, Unbounded};
use std::sync::{Arc, RwLock as StdRwLock};

use tokio::sync::{Mutex, RwLock};
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::blockchain::state::{ContributionRewardRecord, State};
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_native_chain::crypto::hash::Hash;
use wrfcoin_native_chain::storage::Storage;
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
/// Blocks read per lock hold while scanning for payout heights.
const SCAN_CHUNK_BLOCKS: u64 = 256;
/// Stored bytes read per storage read-lock hold while scanning (the first block
/// of a chunk is always read, so a chunk always makes progress).
const SCAN_CHUNK_BYTES: u64 = 8 * 1024 * 1024;
/// Scanned blocks whose hash is kept to detect a reorg. Above the reorg depth
/// limit, so a normal reorg rewinds only the blocks it replaced.
pub const RECENT_SCAN_WINDOW: usize = 64;
/// Block metadata key holding the decisions a block applied.
const REWARD_ENTRIES_METADATA_KEY: &str = "reward_entries";
/// Prefix of a newest-first cursor. Clients treat the cursor as opaque.
const NEWEST_CURSOR_PREFIX: &str = "n1.";

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

/// Position of a credit in newest-first order. Height 0 means the payout block
/// is unknown to this node, which sorts after every dated credit.
type NewestKey = (u64, Arc<str>);

/// One address's keys in both orders.
#[derive(Default)]
struct AddressKeys {
    by_commit: BTreeSet<Arc<str>>,
    newest: BTreeSet<NewestKey>,
}

/// One immutable generation of the index.
pub struct PublishedIndex {
    source: IndexSource,
    by_address: BTreeMap<Address, AddressKeys>,
}

/// One page of an address's paid credits.
#[derive(Debug)]
pub struct CreditPage {
    pub records: Vec<ContributionRewardRecord>,
    pub keys_examined: usize,
    pub next_cursor: Option<String>,
}

/// One newest-first page: each record with its payout height (`None` when this
/// node does not hold the paying block).
#[derive(Debug)]
pub struct NewestCreditPage {
    pub records: Vec<(ContributionRewardRecord, Option<u64>)>,
    pub keys_examined: usize,
    pub next_cursor: Option<String>,
}

/// A newest-first cursor that does not decode.
#[derive(Debug, PartialEq, Eq)]
pub struct BadCursor;

fn encode_newest_cursor(height: u64, commit_id: &str) -> String {
    format!("{NEWEST_CURSOR_PREFIX}{height}.{commit_id}")
}

/// Decode a cursor from [`PublishedIndex::newest_page`].
pub fn decode_newest_cursor(cursor: &str) -> Result<(u64, &str), BadCursor> {
    let rest = cursor.strip_prefix(NEWEST_CURSOR_PREFIX).ok_or(BadCursor)?;
    let (height, commit_id) = rest.split_once('.').ok_or(BadCursor)?;
    if height.is_empty() || !height.bytes().all(|b| b.is_ascii_digit()) || commit_id.is_empty() {
        return Err(BadCursor);
    }
    Ok((height.parse().map_err(|_| BadCursor)?, commit_id))
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
        for key in keys
            .by_commit
            .range::<str, _>((after.map_or(Unbounded, Excluded), Unbounded))
        {
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

    /// Keyset page newest first: payout height descending, then commit id
    /// descending. `after` is a cursor this method returned, decoded with
    /// [`decode_newest_cursor`]. Same budget and paid-credit check as [`Self::page`].
    pub fn newest_page(
        &self,
        state: &State,
        address: Address,
        limit: usize,
        after: Option<(u64, &str)>,
    ) -> NewestCreditPage {
        let mut page = NewestCreditPage {
            records: Vec::new(),
            keys_examined: 0,
            next_cursor: None,
        };
        let Some(keys) = self.by_address.get(&address) else {
            return page;
        };
        let upper = after.map(|(height, commit_id)| (height, Arc::<str>::from(commit_id)));
        let range = match &upper {
            Some(bound) => keys.newest.range((Unbounded, Excluded(bound))),
            None => keys.newest.range::<NewestKey, _>(..),
        };
        let mut last_returned: Option<&NewestKey> = None;
        let mut last_examined: Option<&NewestKey> = None;
        for key in range.rev() {
            if page.keys_examined == MAX_KEYS_EXAMINED_PER_READ {
                page.next_cursor = last_examined.map(|(h, id)| encode_newest_cursor(*h, id));
                return page;
            }
            page.keys_examined += 1;
            last_examined = Some(key);
            let (height, commit_id) = key;
            let Some(record) = state.contribution_reward_records.get(commit_id.as_ref()) else {
                continue;
            };
            if !is_paid_credit(state, address, commit_id, record) {
                continue;
            }
            if page.records.len() == limit {
                page.next_cursor = last_returned.map(|(h, id)| encode_newest_cursor(*h, id));
                return page;
            }
            page.records
                .push((record.clone(), (*height > 0).then_some(*height)));
            last_returned = Some(key);
        }
        page
    }
}

/// Payout heights learned from block metadata. Owned by the refresh, carried
/// from one generation to the next so that each block is scanned once.
#[derive(Default)]
struct PayoutHeights {
    /// Highest height scanned; every height at or below it has been looked at.
    scanned_to: u64,
    /// Tip hash when `scanned_to` was reached.
    scanned_tip: Option<Hash>,
    /// Latest block height naming each positive `Real` decision.
    by_commit: HashMap<Arc<str>, u64>,
    /// The last [`RECENT_SCAN_WINDOW`] scanned blocks: hash and the commits each
    /// named, so a reorg can undo exactly those.
    recent: BTreeMap<u64, (Option<Hash>, Vec<Arc<str>>)>,
}

/// What one scanned block contributes.
struct ScannedBlock {
    height: u64,
    hash: Option<Hash>,
    reward_entries: Option<String>,
}

impl ScannedBlock {
    fn of(block: &Block) -> Self {
        Self {
            height: block.height,
            hash: block.hash,
            reward_entries: block.metadata.get(REWARD_ENTRIES_METADATA_KEY).cloned(),
        }
    }
}

impl PayoutHeights {
    /// The commit id shared with this map, so a string is stored once.
    fn shared_key(&self, commit_id: &str) -> Option<(Arc<str>, u64)> {
        self.by_commit
            .get_key_value(commit_id)
            .map(|(key, height)| (key.clone(), *height))
    }

    /// Undo scanned blocks the chain no longer has.
    fn rewind(&mut self, chain: &Blockchain) {
        let window_full = self.recent.len() >= RECENT_SCAN_WINDOW;
        let mut rewound_below: Option<u64> = None;
        while let Some((height, (hash, commits))) = self.recent.pop_last() {
            // An evicted block is older than any reorg the chain accepts.
            let kept = height <= chain.height
                && chain
                    .block_at_height(height)
                    .is_none_or(|block| block.hash == hash);
            if kept {
                self.recent.insert(height, (hash, commits));
                break;
            }
            for commit in commits {
                if self.by_commit.get(&commit) == Some(&height) {
                    self.by_commit.remove(&commit);
                }
            }
            rewound_below = Some(height);
        }
        if let Some(height) = rewound_below {
            if self.recent.is_empty() && window_full {
                // Deeper than the window: start over.
                *self = Self::default();
                return;
            }
            self.scanned_to = self.scanned_to.min(height - 1);
        }
        if self.scanned_to > chain.height {
            self.scanned_to = chain.height;
        }
        if self.scanned_to < chain.height || self.scanned_tip != Some(chain.tip) {
            self.scanned_tip = None;
        }
    }

    fn apply(&mut self, block: ScannedBlock) {
        let mut commits = Vec::new();
        if let Some(raw) = block.reward_entries {
            // A block whose metadata does not decode was refused by consensus or
            // is not this node's to judge; it simply names no commits here.
            if let Ok(entries) = serde_json::from_str::<Vec<ContributionRewardRecord>>(&raw) {
                for entry in entries {
                    if entry.provenance_class != ContributionProvenanceClass::Real
                        || entry.amount.into_inner() == 0
                    {
                        continue;
                    }
                    let Some(commit_id) = entry.commit_id() else {
                        continue;
                    };
                    let key = match self.shared_key(commit_id) {
                        Some((key, _)) => key,
                        None => Arc::from(commit_id),
                    };
                    self.by_commit.insert(key.clone(), block.height);
                    commits.push(key);
                }
            }
        }
        self.recent.insert(block.height, (block.hash, commits));
        while self.recent.len() > RECENT_SCAN_WINDOW {
            self.recent.pop_first();
        }
        self.scanned_to = self.scanned_to.max(block.height);
    }

    /// Scan every block above `scanned_to` up to the tip: the in-memory window
    /// under the chain read lock, older heights from `storage`. Never holds the
    /// chain and storage locks together.
    async fn scan(&mut self, chain: &RwLock<Blockchain>, storage: Option<&RwLock<Storage>>) {
        enum Step {
            Done(Hash),
            Memory(Vec<ScannedBlock>),
            Disk { start: u64, end: u64 },
        }
        loop {
            let step = {
                let chain = chain.read().await;
                self.rewind(&chain);
                let start = self.scanned_to + 1;
                if start > chain.height {
                    Step::Done(chain.tip)
                } else {
                    let end = chain.height.min(start + SCAN_CHUNK_BLOCKS - 1);
                    let first_in_memory = chain.blocks().keys().next().copied();
                    match first_in_memory {
                        Some(first) if first <= start => Step::Memory(
                            chain
                                .blocks_range(start, end)
                                .map(ScannedBlock::of)
                                .collect(),
                        ),
                        Some(first) => Step::Disk {
                            start,
                            end: end.min(first - 1),
                        },
                        None => Step::Disk { start, end },
                    }
                }
            };
            match step {
                Step::Done(tip) => {
                    self.scanned_tip = Some(tip);
                    return;
                },
                Step::Memory(blocks) => {
                    let end = blocks.last().map(|block| block.height);
                    for block in blocks {
                        self.apply(block);
                    }
                    if let Some(end) = end {
                        self.scanned_to = self.scanned_to.max(end);
                    }
                },
                Step::Disk { start, end } => {
                    let loaded = match storage {
                        Some(storage) => {
                            let storage = storage.read().await;
                            match storage.load_blocks_range_bounded(start, end, SCAN_CHUNK_BYTES) {
                                Ok((blocks, _)) => blocks,
                                Err(err) => {
                                    log::warn!(
                                        "reward index: blocks {start}..={end} unreadable, \
                                         their credits get no payout height: {err}"
                                    );
                                    Vec::new()
                                },
                            }
                        },
                        None => Vec::new(),
                    };
                    // A short read stops at the last block read; an empty one
                    // means this node does not hold those blocks.
                    let reached = loaded.last().map_or(end, |block| block.height);
                    for block in &loaded {
                        self.apply(ScannedBlock::of(block));
                    }
                    self.scanned_to = self.scanned_to.max(reached);
                },
            }
            tokio::task::yield_now().await;
        }
    }

    /// Whether the scan covers the chain this guard shows, so that a credit with
    /// no payout height really has none this node can find.
    fn covers(&self, chain: &Blockchain) -> bool {
        self.scanned_to == chain.height && self.scanned_tip == Some(chain.tip)
    }
}

/// One chunk of the walk.
struct WalkChunk {
    /// The keys to index, with the newest-first position when it is known.
    /// `None` leaves the credit out of the newest-first order until the next
    /// walk: it was paid in a block the scan has not reached yet.
    keys: Vec<(Address, Arc<str>, Option<u64>)>,
    /// Records read, indexed or not.
    walked: usize,
    /// The last commit id read.
    last: Option<String>,
}

/// Read one chunk under the caller's chain read guard.
fn read_chunk(chain: &Blockchain, heights: &PayoutHeights, after: Option<&str>) -> WalkChunk {
    let covered = heights.covers(chain);
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
            let (key, newest) = match heights.shared_key(commit_id) {
                Some((key, height)) => (key, Some(height)),
                None => (Arc::from(commit_id.as_str()), covered.then_some(0)),
            };
            keys.push((record.address, key, newest));
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
    /// Held for a whole refresh, which also keeps two refreshes from overlapping.
    heights: Mutex<PayoutHeights>,
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

    /// Walk the records if the chain moved, finding payout heights in the
    /// in-memory blocks only. Returns whether a generation was published.
    #[cfg(test)]
    pub async fn refresh(&self, chain: &RwLock<Blockchain>) -> bool {
        self.refresh_with_storage(chain, None).await
    }

    /// [`Self::refresh`], also reading blocks older than the in-memory window
    /// from `storage` (the first walk after startup).
    pub async fn refresh_with_storage(
        &self,
        chain: &RwLock<Blockchain>,
        storage: Option<&RwLock<Storage>>,
    ) -> bool {
        let mut heights = self.heights.lock().await;
        let source = IndexSource::of(&*chain.read().await);
        if self.published().is_some_and(|index| index.source == source) {
            return false;
        }
        heights.scan(chain, storage).await;
        let mut by_address: BTreeMap<Address, AddressKeys> = BTreeMap::new();
        let mut after: Option<String> = None;
        loop {
            let chunk = read_chunk(&*chain.read().await, &heights, after.as_deref());
            let walked = chunk.walked;
            after = chunk.last.or(after);
            // Sort into the map with no chain lock held.
            for (address, commit_id, newest) in chunk.keys {
                let keys = by_address.entry(address).or_default();
                if let Some(height) = newest {
                    keys.newest.insert((height, commit_id.clone()));
                }
                keys.by_commit.insert(commit_id);
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
pub fn spawn_refresh(
    index: Arc<RewardAddressIndex>,
    chain: Arc<RwLock<Blockchain>>,
    storage: Arc<RwLock<Storage>>,
) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(REFRESH_TICK_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            index.refresh_with_storage(&chain, Some(&storage)).await;
        }
    });
}
