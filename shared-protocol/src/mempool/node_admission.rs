//! Admission policy for node-service transactions: receipts, bond lifecycle and fraud reports.
//!
//! All local policy. Nothing here is consulted by block validation or state apply, so a block
//! carrying a transaction this refuses stays valid, and a node without this module interoperates.
//!
//! These transactions carry a constant, unsigned input. It proves nothing and names no coin, so
//! the pool keys them on sender and nonce instead, which only the signing key can claim. The pool
//! holds no chain state: [`check`] is the half of admission that needs it, and callers run it
//! before offering a transaction from the network or a submit route.
use crate::{
    blockchain::{
        state::State,
        transaction::{Transaction, TransactionType},
    },
    common::types::Address,
    consensus::{
        node_bond_registry::{
            MAX_NODE_BOND_LIFECYCLE_TXS_PER_BLOCK, NODE_BOND_UNITS, NodeBondLifecycle, epoch_at,
        },
        node_fraud,
        node_receipt_registry::{MAX_NODE_RECEIPT_BATCHES_PER_BLOCK, NodeReceiptSubmission},
        node_share_contracts::{MAX_NODE_REGISTRATIONS, NodeBondAction},
        proof_of_publishing::DEFAULT_SUBMISSION_TIME_WINDOW_SECS,
    },
    crypto::chain_binding::ChainIdentity,
};

/// Pending copies one sender may hold per kind. The chain takes two batches per owner per block.
pub const MAX_PENDING_NODE_TXS_PER_SENDER: usize = 2;
/// Pending copies of one kind across all senders: every bond at its per-sender cap, doubled. Once
/// [`check`] has run, only a bonded owner reaches this for receipts.
pub const MAX_PENDING_NODE_TXS_PER_KIND: usize =
    MAX_NODE_REGISTRATIONS * MAX_PENDING_NODE_TXS_PER_SENDER * 2;
/// The owner's next nonce and the one after it: two batches can land in one block.
const NONCE_WINDOW: u64 = 1;

/// Per-block caps the chain enforces, indexed by [`kind_rank`].
const PER_BLOCK_CAPS: [usize; 3] = [
    MAX_NODE_RECEIPT_BATCHES_PER_BLOCK,
    MAX_NODE_BOND_LIFECYCLE_TXS_PER_BLOCK,
    node_fraud::MAX_NODE_FRAUD_REPORTS_PER_BLOCK as usize,
];

/// The pool's conflict key for a node-service transaction.
pub type NodeKey = (u8, Address, u64);

/// Why a node-service transaction is not admitted. Local policy, never block validity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRefusal {
    Unavailable,
    Malformed,
    NotBonded,
    EpochClosed,
    NonceUsed,
    NonceAhead,
    Unfunded,
    Unprovable,
}

impl NodeRefusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "NODE_TX_UNAVAILABLE",
            Self::Malformed => "NODE_TX_MALFORMED",
            Self::NotBonded => "NODE_TX_NOT_BONDED",
            Self::EpochClosed => "NODE_TX_EPOCH_CLOSED",
            Self::NonceUsed => "NODE_TX_NONCE_USED",
            Self::NonceAhead => "NODE_TX_NONCE_AHEAD",
            Self::Unfunded => "NODE_TX_UNFUNDED",
            Self::Unprovable => "NODE_TX_UNPROVABLE",
        }
    }
    pub fn message(self) -> &'static str {
        match self {
            Self::Unavailable => "node bond state is not available on this node",
            Self::Malformed => "the transaction does not parse as its declared kind",
            Self::NotBonded => "the sender does not own the bond this transaction names",
            Self::EpochClosed => "the claims are for an epoch that is not the one being mined",
            Self::NonceUsed => {
                "this nonce is already used on chain; read the account and sign again"
            },
            Self::NonceAhead => "this nonce is too far ahead of the account nonce",
            Self::Unfunded => "the sender account has no funds on this node's chain",
            Self::Unprovable => "this report is not provable against current state",
        }
    }
}

fn kind_rank(kind: TransactionType) -> Option<u8> {
    match kind {
        TransactionType::NodeServiceBatch => Some(0),
        TransactionType::NodeBondLifecycle => Some(1),
        TransactionType::NodeFraudReport => Some(2),
        _ => None,
    }
}

pub fn is_node_keyed(kind: TransactionType) -> bool {
    kind_rank(kind).is_some()
}

pub(crate) fn key(tx: &Transaction) -> Option<NodeKey> {
    kind_rank(tx.tx_type).map(|rank| (rank, tx.sender_address, tx.nonce))
}

/// The whole wrapper is unsigned: the owner's signature covers sender, nonce and batch only, so
/// anyone can rewrap a signed batch. Refuse what the producer or the verify budget would drop, or
/// a rewrapped copy could hold the owner's slot and never be mined. Every CLI builder conforms.
pub(crate) fn check_wrapper(tx: &Transaction, now_secs: i64) -> Result<(), &'static str> {
    let wrapper_ok = tx.id == Some(tx.calculate_hash())
        && within_submission_window(tx, now_secs)
        && matches!(tx.inputs.as_slice(), [input] if input.signature.is_none()
            && input.coinbase_data.is_none()
            && input.public_key.as_deref().is_some_and(|key| key.len() <= 64))
        && matches!(tx.outputs.as_slice(), [output] if output.amount == 0
            && output.nft_data.is_none()
            && output.script == "OP_TRUE"
            && output.address == tx.sender_address.to_string());
    if wrapper_ok {
        Ok(())
    } else {
        Err("SP_NODE_WRAPPER: malformed, expired or post-dated node transaction wrapper")
    }
}

/// Block validity bounds a transaction's timestamp by this window, and the producer evicts what
/// falls outside it, so a copy outside it can never be mined.
fn within_submission_window(tx: &Transaction, now_secs: i64) -> bool {
    tx.timestamp.timestamp().abs_diff(now_secs) <= DEFAULT_SUBMISSION_TIME_WINDOW_SECS
}

/// The state-checked half of admission. `height` is the canonical tip. Ordinary kinds pass.
pub fn check(state: &State, tx: &Transaction, height: u64) -> Result<(), NodeRefusal> {
    if !is_node_keyed(tx.tx_type) {
        return Ok(());
    }
    let registry = state
        .node_bond_registry
        .as_ref()
        .ok_or(NodeRefusal::Unavailable)?;
    let sender = tx.sender_address;
    let bonded = |deployment: &[u8; 32]| {
        registry.registrations.get(deployment).is_some_and(|r| {
            r.binding.owner_account == sender && r.principal_units == NODE_BOND_UNITS
        })
    };
    let account = state.accounts.get(&sender);
    match tx.tx_type {
        TransactionType::NodeServiceBatch => {
            // A flood of free keys stops here, before anything is parsed or verified.
            if !registry
                .registrations
                .values()
                .any(|r| r.binding.owner_account == sender)
            {
                return Err(NodeRefusal::NotBonded);
            }
            let p = NodeReceiptSubmission::parse(tx).ok_or(NodeRefusal::Malformed)?;
            if !p.batch.claims.iter().all(|c| bonded(&c.ticket.server_id)) {
                return Err(NodeRefusal::NotBonded);
            }
            // A claim is valid only inside its own epoch, so a late one can never be mined.
            let carrying = epoch_at(registry.activation_height, height.saturating_add(1));
            if !p
                .batch
                .claims
                .iter()
                .all(|c| Some(c.ticket.context.epoch) == carrying)
            {
                return Err(NodeRefusal::EpochClosed);
            }
        },
        TransactionType::NodeBondLifecycle => {
            let p = NodeBondLifecycle::parse(tx).ok_or(NodeRefusal::Malformed)?;
            let a = &p.signed.authorization;
            // Deposit and Update carry a governance co-sign the pool verifies. Exit and Withdraw
            // are signed by the owner alone, so a free key could mint them endlessly.
            if matches!(a.action, NodeBondAction::Exit | NodeBondAction::Withdraw)
                && !registry
                    .registrations
                    .get(&a.deployment_id)
                    .is_some_and(|r| r.binding.owner_account == sender)
            {
                return Err(NodeRefusal::NotBonded);
            }
            if account.is_none() {
                return Err(NodeRefusal::Unfunded);
            }
        },
        TransactionType::NodeFraudReport => {
            if !account.is_some_and(|a| a.spendable_balance.0 > 0) {
                return Err(NodeRefusal::Unfunded);
            }
            let p = node_fraud::parse(tx).ok_or(NodeRefusal::Malformed)?;
            let authority = crate::governance::schedule_extension::schedule_extension_authority()
                .ok_or(NodeRefusal::Unavailable)?;
            let c = authority.context();
            let identity = ChainIdentity::new(c.chain_id, c.network_id);
            let provable = node_fraud::responsible(state, &p, height.saturating_add(1), &identity)
                .is_ok_and(|ids| !ids.is_empty());
            if !provable {
                return Err(NodeRefusal::Unprovable);
            }
        },
        _ => {},
    }
    let nonce = account.map_or(0, |a| a.nonce);
    if tx.nonce < nonce {
        return Err(NodeRefusal::NonceUsed);
    }
    if tx.nonce > nonce.saturating_add(NONCE_WINDOW) {
        return Err(NodeRefusal::NonceAhead);
    }
    Ok(())
}

/// Whether a pending copy can never be mined. A producer learns this at assembly, but a node that
/// never assembles a block would otherwise hold it until restart. `height` is the canonical tip.
pub fn is_stale(state: &State, tx: &Transaction, height: u64, now_secs: i64) -> bool {
    if !is_node_keyed(tx.tx_type) {
        return false;
    }
    if !within_submission_window(tx, now_secs) {
        return true;
    }
    !matches!(
        check(state, tx, height),
        Ok(()) | Err(NodeRefusal::NonceAhead | NodeRefusal::Unavailable)
    )
}

/// Every claim in this receipt batch is already on chain, whichever wrapper carried it.
pub fn batch_included(state: &State, tx: &Transaction) -> bool {
    let Some(p) = NodeReceiptSubmission::parse(tx) else {
        return false;
    };
    let Some(receipts) = state.node_receipt_registry.as_ref() else {
        return false;
    };
    !p.batch.claims.is_empty()
        && p.batch.claims.iter().all(|claim| {
            receipts
                .accepted
                .get(&crate::consensus::node_receipt_registry::ticket_key(claim))
                .is_some_and(|record| record.claim == *claim)
        })
}

/// Slot order for a block's regular transactions, given them in fee order: node-service kinds
/// first, up to their per-block caps, then everything else in unchanged order.
///
/// The pool checks no balances, so high-fee traffic from free keys could otherwise fill every slot
/// before pruning and keep receipts out of the block. The gate already keeps free keys out of these
/// kinds, so at most 40 slots are reserved. Past a cap, a transaction stays pending for a later
/// block instead of being offered and evicted. Selection only: a block with any other mix is valid.
///
/// The pool holds a sender's next two nonces, and fee order does not follow nonce order. So for any
/// account that has a node-service transaction pending, its transactions trade places among the
/// slots they hold until they run in nonce order. A node-service transaction is held for a later
/// block when one of its account's lower nonces is held at a cap, or is offered as another kind:
/// the producer's trial apply advances the nonce for node-service kinds only, so it would fail
/// there and be evicted. Accounts with no node-service transaction keep fee order.
pub fn node_kinds_first<'a>(
    by_fee: impl Iterator<Item = &'a std::sync::Arc<Transaction>>,
    slots: usize,
) -> Vec<&'a std::sync::Arc<Transaction>> {
    let (mut ordered, mut others): (Vec<_>, Vec<_>) = by_fee.partition(|tx| is_node_keyed(tx.tx_type));
    if ordered.is_empty() {
        others.truncate(slots);
        return others;
    }
    let node_senders: std::collections::BTreeSet<Address> =
        ordered.iter().map(|tx| tx.sender_address).collect();
    ordered.append(&mut others);
    let accounts: Vec<Option<Address>> = ordered
        .iter()
        .map(|tx| Some(nonce_account(tx)).filter(|a| node_senders.contains(a)))
        .collect();
    let mut positions: std::collections::BTreeMap<Address, Vec<usize>> = Default::default();
    for (i, account) in accounts.iter().enumerate() {
        if let Some(account) = account {
            positions.entry(*account).or_default().push(i);
        }
    }
    for slots_held in positions.values() {
        // panic-ok: every index in slots_held comes from enumerating `accounts`, built 1:1 from `ordered` above
        let mut txs: Vec<_> = slots_held.iter().map(|&i| ordered[i]).collect();
        txs.sort_by_key(|tx| tx.nonce);
        for (&i, tx) in slots_held.iter().zip(txs) {
            // panic-ok: same indices as above, all below ordered.len(), and `ordered` has not changed length
            ordered[i] = tx;
        }
    }
    let mut taken = [0usize; 3];
    // Lowest nonce held back per account; its transactions run in nonce order now, so the first
    // one held is the lowest and everything above it waits too.
    let mut held: std::collections::BTreeMap<Address, u64> = Default::default();
    // Lowest nonce per account offered as a kind the trial apply does not advance.
    let mut offered_other: std::collections::BTreeMap<Address, u64> = Default::default();
    // panic-ok: capacity is at most the length of `ordered`, a vector that already exists
    let mut picked = Vec::with_capacity(slots.min(ordered.len()));
    for (tx, account) in ordered.into_iter().zip(accounts) {
        if picked.len() >= slots {
            break;
        }
        if account.is_some_and(|a| held.get(&a).is_some_and(|&n| tx.nonce > n)) {
            continue;
        }
        if let Some(rank) = kind_rank(tx.tx_type) {
            let rank = usize::from(rank);
            let behind_other =
                account.is_some_and(|a| offered_other.get(&a).is_some_and(|&n| tx.nonce > n));
            // panic-ok: kind_rank returns only 0, 1 or 2, and taken and PER_BLOCK_CAPS both have 3 entries
            if behind_other || taken[rank] >= PER_BLOCK_CAPS[rank] {
                if let Some(a) = account {
                    held.entry(a).or_insert(tx.nonce);
                }
                continue;
            }
            taken[rank] += 1; // panic-ok: rank is 0, 1 or 2 (kind_rank); taken has 3 entries
        } else if let Some(a) = account {
            offered_other.entry(a).or_insert(tx.nonce);
        }
        picked.push(tx);
    }
    picked
}

/// The account whose nonce a transaction spends: the transfer's payer, the stake's account, else
/// the sender field the node-service kinds and nonce-checked kinds read.
pub(crate) fn nonce_account(tx: &Transaction) -> Address {
    let named = match tx.tx_type {
        TransactionType::Transfer => tx.parse_transparent_transfer_payload().map(|p| p.from),
        TransactionType::Stake | TransactionType::Unstake => {
            tx.parse_stake_auth_payload().map(|p| p.account)
        },
        _ => None,
    };
    named
        .and_then(|a| a.trim().parse::<Address>().ok())
        .unwrap_or(tx.sender_address)
}
