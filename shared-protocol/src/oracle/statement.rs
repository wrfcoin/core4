//! Read-only **attester statement**: what one oracle identity did over a
//! height range, recomputed from block data alone.
//!
//! Attestation happens on chain, so the chain is already the receipt: every
//! `OracleAttestation` a block includes names its `oracle_id`, and every block
//! that pays attesters records the exact split in its `attester_settlements`
//! metadata. This module folds those two facts into one summary. Anyone can
//! run it against any node's blocks and get the same numbers.
//!
//! Nothing here is consensus. It never reads or writes state, never decides
//! block validity and is never called from block apply. It is shared by the
//! `wrfcoin oracle statement` CLI and the node's local `GET /node/attester`
//! route so both report the same counts.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::attestation::AttesterSettlementRecord;
use crate::blockchain::block::Block;
use crate::consensus::proof_of_publishing::ATTESTER_SETTLEMENTS_METADATA_KEY;

/// Base units per WRF.
pub const UNITS_PER_WRF: u128 = 100_000_000;

/// One included attestation vote, as a block carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncludedVote {
    pub oracle_id: String,
    pub commit_id: String,
    pub approved: bool,
}

/// The two facts the statement needs from one block.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementBlock {
    pub height: u64,
    pub votes: Vec<IncludedVote>,
    pub settlements: Vec<AttesterSettlementRecord>,
}

impl StatementBlock {
    /// Extract the statement facts from a full block.
    ///
    /// A block whose `attester_settlements` metadata does not parse is
    /// reported as an error rather than read as "paid nothing": a statement
    /// that silently drops a payment would be wrong in the one direction the
    /// reader cannot notice.
    pub fn from_block(block: &Block) -> Result<Self, String> {
        let votes = block
            .transactions
            .iter()
            .filter_map(|tx| tx.parse_oracle_attestation_payload())
            .map(|payload| IncludedVote {
                oracle_id: payload.oracle_id,
                commit_id: payload.commit_id,
                approved: payload.approved,
            })
            .collect();
        let settlements = parse_settlements(
            block
                .metadata
                .get(ATTESTER_SETTLEMENTS_METADATA_KEY)
                .map(String::as_str),
        )
        .map_err(|err| format!("block {}: {err}", block.height))?;
        Ok(Self {
            height: block.height,
            votes,
            settlements,
        })
    }
}

/// Parse the `attester_settlements` metadata value. Absent or empty means the
/// block paid no attesters.
pub fn parse_settlements(raw: Option<&str>) -> Result<Vec<AttesterSettlementRecord>, String> {
    match raw.map(str::trim) {
        None | Some("") => Ok(Vec::new()),
        Some(text) => serde_json::from_str(text)
            .map_err(|err| format!("attester_settlements metadata does not parse: {err}")),
    }
}

/// One payment to the oracle, as recorded in block metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementPayment {
    pub height: u64,
    pub decision_id: String,
    pub amount_units: u128,
}

/// Per-epoch counts, when the caller asks for epoch buckets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementEpoch {
    pub epoch: u64,
    pub from_height: u64,
    pub to_height: u64,
    pub attestations_included: u64,
    pub paid_votes: u64,
    pub paid_units: u128,
}

/// Plain height windows of `epoch_blocks` blocks counted from `start_height`:
/// epoch `e` covers `start_height + e * epoch_blocks ..= start_height + (e + 1) * epoch_blocks - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochWindows {
    pub epoch_blocks: u64,
    pub start_height: u64,
}

impl EpochWindows {
    pub fn epoch_of(&self, height: u64) -> Option<u64> {
        if self.epoch_blocks == 0 || height < self.start_height {
            return None;
        }
        Some((height - self.start_height) / self.epoch_blocks)
    }

    /// Inclusive height range of `epoch`, or `None` on overflow.
    pub fn range(&self, epoch: u64) -> Option<(u64, u64)> {
        let from = epoch
            .checked_mul(self.epoch_blocks)?
            .checked_add(self.start_height)?;
        let to = from.checked_add(self.epoch_blocks.checked_sub(1)?)?;
        Some((from, to))
    }
}

/// The statement for one oracle over one height range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleStatement {
    pub oracle_id: String,
    pub from_height: u64,
    pub to_height: u64,
    pub blocks_scanned: u64,
    /// `OracleAttestation` transactions from this oracle included in blocks.
    pub attestations_included: u64,
    pub approve_votes: u64,
    pub reject_votes: u64,
    /// Decisions this oracle was paid for: its approve vote landed in the
    /// decision's quorum-crossing block (the on-time votes).
    pub paid_votes: u64,
    /// Sum of those payments, base units.
    pub paid_units: u128,
    /// Approve votes from this oracle for a decision settled in the range that
    /// did not pay it (the vote missed the crossing block).
    pub unpaid_approve_votes: u64,
    /// Decisions settled in the range, paid to anyone.
    pub decisions_settled: u64,
    pub first_attested_height: Option<u64>,
    pub last_attested_height: Option<u64>,
    pub payments: Vec<StatementPayment>,
    /// Present when epoch windows were requested; one entry per epoch that
    /// overlaps the range, including epochs with no activity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epochs: Option<Vec<StatementEpoch>>,
}

impl OracleStatement {
    /// Epochs with at least one included attestation.
    pub fn active_epochs(&self) -> Option<u64> {
        self.epochs
            .as_ref()
            .map(|epochs| epochs.iter().filter(|e| e.attestations_included > 0).count() as u64)
    }
}

/// Fold blocks into a statement for `oracle_id` over `from_height..=to_height`.
///
/// Blocks outside the range are ignored; blocks may arrive in any order. The
/// oracle id is compared case-insensitively (ids are hex).
pub fn compute_statement<'a, I>(
    oracle_id: &str,
    from_height: u64,
    to_height: u64,
    blocks: I,
    epochs: Option<EpochWindows>,
) -> OracleStatement
where
    I: IntoIterator<Item = &'a StatementBlock>,
{
    let id = oracle_id.trim().to_ascii_lowercase();
    let mut scanned = BTreeSet::new();
    let mut included = 0u64;
    let mut approve = 0u64;
    let mut reject = 0u64;
    let mut first = None;
    let mut last: Option<u64> = None;
    let mut approved_commits: BTreeSet<String> = BTreeSet::new();
    let mut settled: BTreeMap<String, bool> = BTreeMap::new();
    let mut payments = Vec::new();
    let mut per_height_votes: BTreeMap<u64, u64> = BTreeMap::new();

    for block in blocks {
        if block.height < from_height || block.height > to_height {
            continue;
        }
        if !scanned.insert(block.height) {
            continue;
        }
        for vote in &block.votes {
            if !vote.oracle_id.eq_ignore_ascii_case(&id) {
                continue;
            }
            included += 1;
            *per_height_votes.entry(block.height).or_default() += 1;
            first = Some(first.map_or(block.height, |h: u64| h.min(block.height)));
            last = Some(last.map_or(block.height, |h| h.max(block.height)));
            if vote.approved {
                approve += 1;
                approved_commits.insert(vote.commit_id.to_ascii_lowercase());
            } else {
                reject += 1;
            }
        }
        for record in &block.settlements {
            let decision = record.decision_id.to_ascii_lowercase();
            let mut paid_here = false;
            for allocation in &record.allocations {
                if allocation.attester_id.eq_ignore_ascii_case(&id) {
                    paid_here = true;
                    payments.push(StatementPayment {
                        height: block.height,
                        decision_id: record.decision_id.clone(),
                        amount_units: allocation.amount,
                    });
                }
            }
            let entry = settled.entry(decision).or_insert(false);
            *entry |= paid_here;
        }
    }

    payments.sort_by(|a, b| (a.height, &a.decision_id).cmp(&(b.height, &b.decision_id)));
    let paid_units = payments.iter().map(|p| p.amount_units).sum();
    let unpaid_approve_votes = approved_commits
        .iter()
        .filter(|commit| settled.get(*commit) == Some(&false))
        .count() as u64;

    let epochs = epochs.filter(|w| w.epoch_blocks > 0).map(|windows| {
        let mut out = Vec::new();
        let (Some(first_epoch), Some(last_epoch)) = (
            windows.epoch_of(from_height.max(windows.start_height)),
            windows.epoch_of(to_height),
        ) else {
            return out;
        };
        for epoch in first_epoch..=last_epoch {
            let Some((lo, hi)) = windows.range(epoch) else { break };
            let lo = lo.max(from_height);
            let hi = hi.min(to_height);
            let in_window = |h: u64| h >= lo && h <= hi;
            let window_payments = payments.iter().filter(|p| in_window(p.height));
            out.push(StatementEpoch {
                epoch,
                from_height: lo,
                to_height: hi,
                attestations_included: per_height_votes
                    .range(lo..=hi)
                    .map(|(_, n)| *n)
                    .sum(),
                paid_votes: window_payments.clone().count() as u64,
                paid_units: window_payments.map(|p| p.amount_units).sum(),
            });
        }
        out
    });

    OracleStatement {
        oracle_id: id,
        from_height,
        to_height,
        blocks_scanned: scanned.len() as u64,
        attestations_included: included,
        approve_votes: approve,
        reject_votes: reject,
        paid_votes: payments.len() as u64,
        paid_units,
        unpaid_approve_votes,
        decisions_settled: settled.len() as u64,
        first_attested_height: first,
        last_attested_height: last,
        payments,
        epochs,
    }
}

/// Render base units as WRF with eight decimals, no rounding.
pub fn format_units(units: u128) -> String {
    format!("{}.{:08}", units / UNITS_PER_WRF, units % UNITS_PER_WRF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::attestation::AttesterAllocation;

    const ME: &str = "c4dbd69bc84a4651db0a03ccc142a019c0856126ac3c920e361cba2269034725"; // pragma: allowlist secret -- public ed25519 oracle id (a public key), test data
    const HOUSE: &str = "781fd34338fa7f846cd4032256179b6f389404cd519fa6eaa3ed097a5a83dcdb"; // pragma: allowlist secret -- public ed25519 oracle id (a public key), test data

    fn vote(oracle: &str, commit: &str, approved: bool) -> IncludedVote {
        IncludedVote {
            oracle_id: oracle.into(),
            commit_id: commit.into(),
            approved,
        }
    }

    fn settle(decision: &str, height: u64, paid: &[(&str, u128)]) -> AttesterSettlementRecord {
        AttesterSettlementRecord {
            decision_id: decision.into(),
            finalization_height: height,
            allocations: paid
                .iter()
                .map(|(id, amount)| AttesterAllocation {
                    attester_id: (*id).into(),
                    amount: *amount,
                })
                .collect(),
        }
    }

    fn sample() -> Vec<StatementBlock> {
        vec![
            StatementBlock {
                height: 100,
                votes: vec![vote(ME, "aa", true), vote(HOUSE, "aa", true)],
                settlements: vec![settle("aa", 100, &[(HOUSE, 50), (ME, 50)])],
            },
            // My vote on bb lands one block after the crossing: unpaid.
            StatementBlock {
                height: 101,
                votes: vec![vote(HOUSE, "bb", true)],
                settlements: vec![settle("bb", 101, &[(HOUSE, 100)])],
            },
            StatementBlock {
                height: 102,
                votes: vec![vote(ME, "bb", true), vote(ME, "cc", false)],
                settlements: vec![],
            },
            StatementBlock {
                height: 600,
                votes: vec![vote(&ME.to_ascii_uppercase(), "dd", true)],
                settlements: vec![settle("DD", 600, &[(ME, 41_666_667)])],
            },
        ]
    }

    #[test]
    fn counts_included_paid_and_late_votes() {
        let blocks = sample();
        let s = compute_statement(ME, 0, 1000, &blocks, None);
        assert_eq!(s.blocks_scanned, 4);
        assert_eq!(s.attestations_included, 4);
        assert_eq!(s.approve_votes, 3);
        assert_eq!(s.reject_votes, 1);
        assert_eq!(s.paid_votes, 2);
        assert_eq!(s.paid_units, 50 + 41_666_667);
        assert_eq!(s.unpaid_approve_votes, 1, "bb was settled without paying me");
        assert_eq!(s.decisions_settled, 3);
        assert_eq!(s.first_attested_height, Some(100));
        assert_eq!(s.last_attested_height, Some(600));
        assert!(s.epochs.is_none());
    }

    #[test]
    fn range_bounds_are_inclusive_and_duplicates_ignored() {
        let mut blocks = sample();
        blocks.push(blocks[0].clone());
        let s = compute_statement(ME, 101, 102, &blocks, None);
        assert_eq!(s.blocks_scanned, 2);
        assert_eq!(s.attestations_included, 2);
        assert_eq!(s.paid_votes, 0);
        assert_eq!(s.unpaid_approve_votes, 1);
        let all = compute_statement(ME, 0, 1000, &blocks, None);
        assert_eq!(all.attestations_included, 4, "a repeated height counts once");
    }

    #[test]
    fn epoch_buckets_include_quiet_epochs() {
        let blocks = sample();
        let windows = EpochWindows {
            epoch_blocks: 256,
            start_height: 0,
        };
        let s = compute_statement(ME, 0, 767, &blocks, Some(windows));
        let epochs = s.epochs.clone().unwrap();
        assert_eq!(epochs.len(), 3);
        assert_eq!((epochs[0].from_height, epochs[0].to_height), (0, 255));
        assert_eq!(epochs[0].attestations_included, 3);
        assert_eq!(epochs[0].paid_votes, 1);
        assert_eq!(epochs[1].attestations_included, 0);
        assert_eq!(epochs[2].attestations_included, 1);
        assert_eq!(epochs[2].paid_units, 41_666_667);
        assert_eq!(s.active_epochs(), Some(2));
    }

    #[test]
    fn epoch_windows_clip_to_the_range() {
        let w = EpochWindows {
            epoch_blocks: 512,
            start_height: 10,
        };
        assert_eq!(w.epoch_of(9), None);
        assert_eq!(w.epoch_of(10), Some(0));
        assert_eq!(w.epoch_of(522), Some(1));
        assert_eq!(w.range(1), Some((522, 1033)));
        let s = compute_statement(ME, 600, 700, &sample(), Some(w));
        let epochs = s.epochs.unwrap();
        assert_eq!(epochs.len(), 1);
        assert_eq!((epochs[0].from_height, epochs[0].to_height), (600, 700));
    }

    #[test]
    fn settlements_metadata_parses_the_chain_shape() {
        let raw = r#"[{"decision_id":"262031","finalization_height":28025,"allocations":[{"attester_id":"781f","amount":41666667},{"attester_id":"c4db","amount":41666666}]}]"#;
        let records = parse_settlements(Some(raw)).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].allocations[1].amount, 41_666_666);
        assert!(parse_settlements(None).unwrap().is_empty());
        assert!(parse_settlements(Some("  ")).unwrap().is_empty());
        assert!(parse_settlements(Some("{not json")).is_err());
    }

    #[test]
    fn units_format_without_rounding() {
        assert_eq!(format_units(41_666_667), "0.41666667");
        assert_eq!(format_units(333_333_334), "3.33333334");
        assert_eq!(format_units(0), "0.00000000");
    }
}
