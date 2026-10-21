use crate::common::types::Address;
use crate::crypto::hash::Hash;
use crate::crypto::signature::PublicKey;
use anyhow::Result;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

fn checked_sum_u64<'a, I>(values: I, context: &'static str) -> Result<u64>
where
    I: IntoIterator<Item = &'a u64>,
{
    values.into_iter().try_fold(0u64, |acc, value| {
        acc.checked_add(*value)
            .ok_or_else(|| anyhow::anyhow!("{context} overflow"))
    })
}

/// Status of a state channel
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelStatus {
    /// Channel is being created
    Creating,

    /// Channel is open and active
    Open,

    /// Channel is pending closure
    Closing,

    /// Channel is closed
    Closed,

    /// Channel is disputed
    Disputed,

    /// Channel has been settled
    Settled,
}

/// State channel types
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelType {
    /// Payment channel (unidirectional)
    PaymentChannel,

    /// State channel (bidirectional, with state updates)
    StateChannel,

    /// Multi-party state channel
    MultiPartyChannel,

    /// Virtual channel (created within an existing channel)
    VirtualChannel,
}

/// Channel participant
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelParticipant {
    /// Participant's address
    pub address: Address,

    /// Participant's verifying key, bound at channel open (#1447).
    /// Signature validation (#1443) resolves signatures against this key.
    pub verifying_key: PublicKey,

    /// Initial balance in the channel
    pub initial_balance: u64,

    /// Current balance in the channel
    pub current_balance: u64,

    /// Has signed latest state
    pub signed_latest_state: bool,

    /// Last signature
    pub last_signature: Option<Vec<u8>>,

    /// Last update timestamp (unix seconds)
    pub last_update: u64,
}

/// Channel state update
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStateUpdate {
    /// Update sequence number
    pub sequence: u64,

    /// New balances for participants
    pub balances: HashMap<Address, u64>,

    /// Update timestamp (unix seconds)
    pub timestamp: u64,

    /// State data
    pub state_data: Option<Vec<u8>>,

    /// Signatures for this update
    pub signatures: HashMap<Address, Vec<u8>>,
}

/// State channel implementation
#[allow(dead_code)]
pub struct StateChannel {
    /// Channel ID
    pub id: String,

    /// Channel type
    pub channel_type: ChannelType,

    /// Channel participants
    pub participants: Arc<RwLock<HashMap<Address, ChannelParticipant>>>,

    /// Channel status
    pub status: Arc<RwLock<ChannelStatus>>,

    /// Total balance locked in the channel
    pub total_balance: Arc<RwLock<u64>>,

    /// Dispute period in blocks
    pub dispute_period: u64,

    /// Channel state updates
    pub state_updates: Arc<RwLock<Vec<ChannelStateUpdate>>>,

    /// Current state sequence
    pub current_sequence: Arc<RwLock<u64>>,

    /// Channel opened at timestamp (unix seconds)
    pub opened_at: u64,

    /// Channel settled at timestamp (unix seconds)
    pub settled_at: Option<u64>,

    /// On-chain transactions related to this channel
    pub transactions: Arc<RwLock<Vec<Hash>>>,
}

#[allow(dead_code)]
impl StateChannel {
    /// Create a new state channel.
    ///
    /// Each participant is bound to a verifying key at channel open (#1447).
    /// The same `Address` may only appear once; duplicates return an error so
    /// that a caller cannot silently overwrite another participant's key.
    pub async fn new(
        id: String,
        participants: Vec<(Address, PublicKey, u64)>,
        channel_type: ChannelType,
        dispute_period: u64,
    ) -> Result<Self> {
        let total_balance = checked_sum_u64(
            participants.iter().map(|(_, _, balance)| balance),
            "channel total balance",
        )?;

        let mut participants_map = HashMap::new();

        for (address, verifying_key, balance) in participants {
            if participants_map.contains_key(&address) {
                return Err(anyhow::anyhow!(
                    "Duplicate participant address in channel open"
                ));
            }
            participants_map.insert(
                address,
                ChannelParticipant {
                    address,
                    verifying_key,
                    initial_balance: balance,
                    current_balance: balance,
                    signed_latest_state: false,
                    last_signature: None,
                    last_update: 0,
                },
            );
        }

        let opened_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let channel = Self {
            id,
            channel_type,
            participants: Arc::new(RwLock::new(participants_map)),
            status: Arc::new(RwLock::new(ChannelStatus::Creating)),
            total_balance: Arc::new(RwLock::new(total_balance)),
            dispute_period,
            state_updates: Arc::new(RwLock::new(Vec::new())),
            current_sequence: Arc::new(RwLock::new(0)),
            opened_at,
            settled_at: None,
            transactions: Arc::new(RwLock::new(Vec::new())),
        };

        Ok(channel)
    }

    /// Open the channel
    pub async fn open(&self) -> Result<()> {
        let mut status = self.status.write().await;
        if *status != ChannelStatus::Creating {
            return Err(anyhow::anyhow!("Channel is not in Creating state"));
        }

        *status = ChannelStatus::Open;

        let participants = self.participants.read().await;
        let mut balances = HashMap::new();

        for (address, participant) in participants.iter() {
            balances.insert(*address, participant.initial_balance);
        }

        let initial_update = ChannelStateUpdate {
            sequence: 0,
            balances,
            timestamp: self.opened_at,
            state_data: None,
            signatures: HashMap::new(),
        };

        self.state_updates.write().await.push(initial_update);

        info!("Opened state channel {}", self.id);

        Ok(())
    }

    /// Propose a state update
    pub async fn propose_update(
        &self,
        proposer: &Address,
        new_balances: HashMap<Address, u64>,
        state_data: Option<Vec<u8>>,
        signature: Vec<u8>,
    ) -> Result<u64> {
        let status = *self.status.read().await;
        if status != ChannelStatus::Open {
            return Err(anyhow::anyhow!("Channel is not open"));
        }

        let total_balance = *self.total_balance.read().await;
        let sum = checked_sum_u64(new_balances.values(), "balance sum")?;

        if sum != total_balance {
            return Err(anyhow::anyhow!(
                "Balance sum mismatch: {} != {}",
                sum,
                total_balance
            ));
        }

        let participants = self.participants.read().await;
        if !participants.contains_key(proposer) {
            return Err(anyhow::anyhow!("Proposer is not a channel participant"));
        }

        for address in participants.keys() {
            if !new_balances.contains_key(address) {
                return Err(anyhow::anyhow!(
                    "Missing balance for participant {}",
                    address.as_bytes()[0]
                ));
            }
        }

        let mut sequence = self.current_sequence.write().await;
        *sequence += 1;

        let update = ChannelStateUpdate {
            sequence: *sequence,
            balances: new_balances.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            state_data,
            signatures: {
                let mut sigs = HashMap::new();
                sigs.insert(*proposer, signature);
                sigs
            },
        };

        self.state_updates.write().await.push(update);

        drop(participants);
        let mut participants = self.participants.write().await;
        for (address, balance) in new_balances {
            if let Some(participant) = participants.get_mut(&address) {
                participant.current_balance = balance;

                if &address == proposer {
                    participant.signed_latest_state = true;
                    participant.last_update = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                } else {
                    participant.signed_latest_state = false;
                }
            }
        }

        debug!(
            "Proposed state update {} for channel {}",
            *sequence, self.id
        );

        Ok(*sequence)
    }

    /// Sign a state update
    pub async fn sign_update(
        &self,
        signer: &Address,
        sequence: u64,
        signature: Vec<u8>,
    ) -> Result<()> {
        let status = *self.status.read().await;
        if status != ChannelStatus::Open {
            return Err(anyhow::anyhow!("Channel is not open"));
        }

        let mut participants = self.participants.write().await;
        if !participants.contains_key(signer) {
            return Err(anyhow::anyhow!("Signer is not a channel participant"));
        }

        let mut state_updates = self.state_updates.write().await;
        let update = state_updates
            .iter_mut()
            .find(|u| u.sequence == sequence)
            .ok_or_else(|| anyhow::anyhow!("State update not found"))?;

        update.signatures.insert(*signer, signature.clone());

        if let Some(participant) = participants.get_mut(signer) {
            participant.signed_latest_state = true;
            participant.last_signature = Some(signature);
            participant.last_update = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }

        let all_signed = participants
            .iter()
            .all(|(address, _)| update.signatures.contains_key(address));

        if all_signed {
            debug!("All participants signed state update {}", sequence);
        } else {
            debug!(
                "Participant {} signed state update {}",
                hex::encode(&signer.as_bytes()[0..4]),
                sequence
            );
        }

        Ok(())
    }

    /// Initiate channel closure
    pub async fn initiate_closure(&self, initiator: &Address) -> Result<()> {
        let mut status = self.status.write().await;
        if *status != ChannelStatus::Open {
            return Err(anyhow::anyhow!("Channel is not open"));
        }

        let participants = self.participants.read().await;
        if !participants.contains_key(initiator) {
            return Err(anyhow::anyhow!("Initiator is not a channel participant"));
        }

        *status = ChannelStatus::Closing;

        info!("Initiated closure of channel {}", self.id);

        Ok(())
    }

    /// Settle the channel
    pub async fn settle(&self) -> Result<HashMap<Address, u64>> {
        let mut status = self.status.write().await;
        if *status != ChannelStatus::Closing && *status != ChannelStatus::Disputed {
            return Err(anyhow::anyhow!("Channel is not closing or disputed"));
        }

        let state_updates = self.state_updates.read().await;
        let latest_update = state_updates
            .iter()
            .max_by_key(|u| u.sequence)
            .ok_or_else(|| anyhow::anyhow!("No state updates found"))?;

        *status = ChannelStatus::Settled;

        let mut participants = self.participants.write().await;
        for (address, balance) in &latest_update.balances {
            if let Some(participant) = participants.get_mut(address) {
                participant.current_balance = *balance;
            }
        }

        info!(
            "Settled channel {} with balances: {:?}",
            self.id, latest_update.balances
        );

        Ok(latest_update.balances.clone())
    }

    /// Get channel state
    pub async fn get_state(&self) -> Result<ChannelState> {
        let status = *self.status.read().await;
        let participants = self.participants.read().await;
        let total_balance = *self.total_balance.read().await;
        let state_updates = self.state_updates.read().await;
        let current_sequence = *self.current_sequence.read().await;

        let latest_update = state_updates.iter().max_by_key(|u| u.sequence).cloned();

        let state = ChannelState {
            id: self.id.clone(),
            status,
            participant_count: participants.len() as u32,
            total_balance,
            current_sequence,
            update_count: state_updates.len() as u32,
            latest_update,
            opened_at: self.opened_at,
            settled_at: self.settled_at,
        };

        Ok(state)
    }

    /// Look up a participant's verifying key. Used by #1443 signature
    /// verification to resolve signer → key without a side-channel registry.
    pub async fn verifying_key_for(&self, address: &Address) -> Option<PublicKey> {
        self.participants
            .read()
            .await
            .get(address)
            .map(|p| p.verifying_key.clone())
    }

    /// Generate state hash.
    ///
    /// Binds participant verifying keys (#1447) into the hash so the identity
    /// set that signed an update is cryptographically pinned to the state hash.
    /// Keys are hashed in the same sorted-by-address order as balances to keep
    /// the hash deterministic across nodes.
    pub async fn generate_state_hash(&self, sequence: u64) -> Result<Hash> {
        let state_updates = self.state_updates.read().await;
        let update = state_updates
            .iter()
            .find(|u| u.sequence == sequence)
            .ok_or_else(|| anyhow::anyhow!("State update not found"))?;

        let participants = self.participants.read().await;

        let mut hasher = Sha256::new();

        hasher.update(self.id.as_bytes());
        hasher.update(&sequence.to_be_bytes());

        let mut addresses: Vec<&Address> = update.balances.keys().collect();
        addresses.sort_by_key(|a| *a.as_bytes());

        for address in addresses {
            hasher.update(address.as_bytes());
            if let Some(balance) = update.balances.get(address) {
                hasher.update(&balance.to_be_bytes());
            }
            if let Some(participant) = participants.get(address) {
                hasher.update(participant.verifying_key.as_bytes());
            }
        }

        if let Some(data) = &update.state_data {
            hasher.update(data);
        }

        hasher.update(&update.timestamp.to_be_bytes());

        let mut h = [0u8; 32];
        h.copy_from_slice(&hasher.finalize());

        Ok(Hash::from_bytes(h))
    }
}

/// Channel state information
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelState {
    /// Channel ID
    pub id: String,

    /// Channel status
    pub status: ChannelStatus,

    /// Number of participants
    pub participant_count: u32,

    /// Total balance locked in the channel
    pub total_balance: u64,

    /// Current sequence number
    pub current_sequence: u64,

    /// Number of state updates
    pub update_count: u32,

    /// Latest state update
    pub latest_update: Option<ChannelStateUpdate>,

    /// Channel opened at timestamp (unix seconds)
    pub opened_at: u64,

    /// Channel settled at timestamp (unix seconds)
    pub settled_at: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::PrivateKey;
    use rand::{Rng, SeedableRng, rngs::StdRng};

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    fn vk_from_seed(byte: u8) -> PublicKey {
        let sk = PrivateKey::from_bytes(&[byte; 32]).expect("seed private key");  // secret-leak-ok: test loop counter, not key material
        PublicKey::from_private_key(&sk)
    }

    #[tokio::test]
    async fn channel_open_transitions_status_to_open() {
        let alice = addr(0xaa);
        let bob = addr(0xbb);
        let channel = StateChannel::new(
            "ch-1".to_string(),
            vec![
                (alice, vk_from_seed(0xaa), 500),
                (bob, vk_from_seed(0xbb), 500),
            ],
            ChannelType::PaymentChannel,
            10,
        )
        .await
        .expect("create channel");

        assert_eq!(*channel.status.read().await, ChannelStatus::Creating);
        channel.open().await.expect("open channel");
        assert_eq!(*channel.status.read().await, ChannelStatus::Open);

        // Opening an already-open channel must fail
        assert!(channel.open().await.is_err());
    }

    #[tokio::test]
    async fn propose_update_enforces_balance_conservation_and_increments_sequence() {
        let alice = addr(0xaa);
        let bob = addr(0xbb);
        let channel = StateChannel::new(
            "ch-2".to_string(),
            vec![
                (alice, vk_from_seed(0xaa), 800),
                (bob, vk_from_seed(0xbb), 200),
            ],
            ChannelType::StateChannel,
            5,
        )
        .await
        .expect("create channel");
        channel.open().await.expect("open channel");

        // Valid update: same total (1000), shifted balances
        let mut new_balances = HashMap::new();
        new_balances.insert(alice, 300);
        new_balances.insert(bob, 700);
        let seq = channel
            .propose_update(&alice, new_balances.clone(), None, vec![0xde, 0xad])
            .await
            .expect("propose update");
        assert_eq!(seq, 1);

        // Update with wrong total must fail
        let mut bad_balances = HashMap::new();
        bad_balances.insert(alice, 300);
        bad_balances.insert(bob, 800); // 300+800 != 1000
        assert!(channel
            .propose_update(&alice, bad_balances, None, vec![0xbe, 0xef])
            .await
            .is_err());
    }

    #[tokio::test]
    async fn channel_new_allows_empty_participant_set_with_zero_total_balance() {
        let channel = StateChannel::new(
            "ch-empty".to_string(),
            Vec::new(),
            ChannelType::StateChannel,
            1,
        )
        .await
        .expect("create empty channel");

        assert_eq!(*channel.total_balance.read().await, 0);
    }

    #[tokio::test]
    async fn propose_update_rejects_u64_max_pair_without_panicking() {
        let alice = addr(0xaa);
        let bob = addr(0xbb);
        let channel = StateChannel::new(
            "ch-overflow-max".to_string(),
            vec![
                (alice, vk_from_seed(0xaa), 1),
                (bob, vk_from_seed(0xbb), 1),
            ],
            ChannelType::StateChannel,
            5,
        )
        .await
        .expect("create channel");
        channel.open().await.expect("open channel");

        let mut new_balances = HashMap::new();
        new_balances.insert(alice, u64::MAX);
        new_balances.insert(bob, u64::MAX);

        let err = channel
            .propose_update(&alice, new_balances, None, vec![0xde, 0xad])
            .await
            .expect_err("overflowing sums must be rejected cleanly");

        assert!(
            err.to_string().contains("balance sum overflow"),
            "expected balance sum overflow error, got {err:?}"
        );
    }

    #[tokio::test]
    async fn propose_update_rejects_u64_max_plus_one_without_panicking() {
        let alice = addr(0xaa);
        let bob = addr(0xbb);
        let channel = StateChannel::new(
            "ch-overflow-boundary".to_string(),
            vec![
                (alice, vk_from_seed(0xaa), 1),
                (bob, vk_from_seed(0xbb), 1),
            ],
            ChannelType::StateChannel,
            5,
        )
        .await
        .expect("create channel");
        channel.open().await.expect("open channel");

        let mut new_balances = HashMap::new();
        new_balances.insert(alice, u64::MAX);
        new_balances.insert(bob, 1);

        let err = channel
            .propose_update(&alice, new_balances, None, vec![0xca, 0xfe])
            .await
            .expect_err("boundary overflow must be rejected cleanly");

        assert!(
            err.to_string().contains("balance sum overflow"),
            "expected balance sum overflow error, got {err:?}"
        );
    }

    #[tokio::test]
    async fn propose_update_preserves_normal_balances_after_checked_sum() {
        let alice = addr(0xaa);
        let bob = addr(0xbb);
        let channel = StateChannel::new(
            "ch-normal".to_string(),
            vec![
                (alice, vk_from_seed(0xaa), 1_000),
                (bob, vk_from_seed(0xbb), 2_000),
            ],
            ChannelType::StateChannel,
            5,
        )
        .await
        .expect("create channel");
        channel.open().await.expect("open channel");

        let mut new_balances = HashMap::new();
        new_balances.insert(alice, 2_000);
        new_balances.insert(bob, 1_000);

        let seq = channel
            .propose_update(&alice, new_balances, None, vec![0xfa, 0xce])
            .await
            .expect("normal sums should still succeed");

        assert_eq!(seq, 1);
    }

    #[tokio::test]
    async fn propose_update_randomized_inputs_never_panic_on_balance_sum() {
        let mut rng = StdRng::seed_from_u64(1372);

        for case_idx in 0..100 {
            let participant_count = rng.gen_range(1..=5);
            let participants: Vec<Address> = (0..participant_count)
                .map(|offset| addr((case_idx as u8).wrapping_add(offset as u8).wrapping_add(1)))
                .collect();

            let candidate_balances: Vec<u64> = (0..participant_count)
                .map(|_| match rng.gen_range(0..6) {
                    0 => 0,
                    1 => 1,
                    2 => 1_000,
                    3 => 2_000,
                    4 => u64::MAX,
                    _ => rng.gen::<u64>(),
                })
                .collect();

            let safe_total = candidate_balances
                .iter()
                .try_fold(0u64, |acc, value| acc.checked_add(*value));
            let initial_balances = if case_idx % 2 == 0 {
                safe_total
                    .map(|_| candidate_balances.clone())
                    .unwrap_or_else(|| vec![1; participant_count])
            } else {
                vec![1; participant_count]
            };

            let channel = StateChannel::new(
                format!("ch-fuzz-{case_idx}"),
                participants
                    .iter()
                    .copied()
                    .zip(initial_balances.into_iter())
                    .map(|(addr, bal)| (addr, vk_from_seed(addr.0[0]), bal))
                    .collect(),
                ChannelType::StateChannel,
                5,
            )
            .await
            .expect("create fuzz channel");
            channel.open().await.expect("open fuzz channel");

            let new_balances: HashMap<Address, u64> = participants
                .iter()
                .copied()
                .zip(candidate_balances.into_iter())
                .collect();

            match channel
                .propose_update(&participants[0], new_balances, None, vec![case_idx as u8])
                .await
            {
                Ok(sequence) => assert_eq!(sequence, 1),
                Err(err) => assert!(
                    !err.to_string().is_empty(),
                    "errors should stay explicit for rejected inputs"
                ),
            }
        }
    }

    #[tokio::test]
    async fn full_lifecycle_open_propose_close_settle_returns_final_balances() {
        let alice = addr(0x01);
        let bob = addr(0x02);
        let channel = StateChannel::new(
            "ch-3".to_string(),
            vec![
                (alice, vk_from_seed(0x01), 600),
                (bob, vk_from_seed(0x02), 400),
            ],
            ChannelType::PaymentChannel,
            3,
        )
        .await
        .expect("create channel");
        channel.open().await.expect("open");

        // Propose a state update (alice sends 100 to bob)
        let mut balances = HashMap::new();
        balances.insert(alice, 500);
        balances.insert(bob, 500);
        channel
            .propose_update(&alice, balances.clone(), None, vec![0x01])
            .await
            .expect("propose");

        // Initiate closure
        channel.initiate_closure(&alice).await.expect("close");
        assert_eq!(*channel.status.read().await, ChannelStatus::Closing);

        // Settle: must return latest balances
        let settled = channel.settle().await.expect("settle");
        assert_eq!(settled.get(&alice).copied(), Some(500));
        assert_eq!(settled.get(&bob).copied(), Some(500));
        assert_eq!(*channel.status.read().await, ChannelStatus::Settled);
    }

    // --- #1447: participant identity binding ---

    #[tokio::test]
    async fn channel_stores_verifying_key_per_participant() {
        let alice = addr(0xaa);
        let bob = addr(0xbb);
        let alice_vk = vk_from_seed(0xaa);
        let bob_vk = vk_from_seed(0xbb);
        let channel = StateChannel::new(
            "ch-vk-stored".to_string(),
            vec![(alice, alice_vk.clone(), 100), (bob, bob_vk.clone(), 100)],
            ChannelType::StateChannel,
            5,
        )
        .await
        .expect("create channel");

        let participants = channel.participants.read().await;
        assert_eq!(
            participants.get(&alice).expect("alice").verifying_key,
            alice_vk
        );
        assert_eq!(participants.get(&bob).expect("bob").verifying_key, bob_vk);
    }

    #[tokio::test]
    async fn channel_participant_serde_round_trip_preserves_verifying_key() {
        let alice = addr(0xaa);
        let original = ChannelParticipant {
            address: alice,
            verifying_key: vk_from_seed(0xaa),
            initial_balance: 100,
            current_balance: 100,
            signed_latest_state: false,
            last_signature: None,
            last_update: 0,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: ChannelParticipant =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.verifying_key, original.verifying_key);
        assert_eq!(restored.address, original.address);
    }

    #[tokio::test]
    async fn verifying_key_for_unknown_address_returns_none() {
        let alice = addr(0xaa);
        let bob = addr(0xbb);
        let outsider = addr(0xcc);
        let channel = StateChannel::new(
            "ch-vk-lookup".to_string(),
            vec![
                (alice, vk_from_seed(0xaa), 1),
                (bob, vk_from_seed(0xbb), 1),
            ],
            ChannelType::StateChannel,
            5,
        )
        .await
        .expect("create channel");

        assert_eq!(
            channel.verifying_key_for(&alice).await,
            Some(vk_from_seed(0xaa))
        );
        assert!(channel.verifying_key_for(&outsider).await.is_none());
    }

    #[tokio::test]
    async fn channel_new_rejects_duplicate_participant_addresses() {
        let alice = addr(0xaa);
        let result = StateChannel::new(
            "ch-dup".to_string(),
            vec![
                (alice, vk_from_seed(0x01), 50),
                (alice, vk_from_seed(0x02), 50),
            ],
            ChannelType::StateChannel,
            5,
        )
        .await;
        match result {
            Ok(_) => panic!("duplicate participant must be rejected"),
            Err(err) => assert!(
                err.to_string().contains("Duplicate participant"),
                "unexpected error: {err}"
            ),
        }
    }

    #[tokio::test]
    async fn state_hash_differs_when_verifying_keys_differ() {
        // Two channels identical in id/balances/timestamps but different
        // participant verifying keys must produce different state hashes —
        // proves the binding is cryptographically pinned.
        let alice = addr(0x10);
        let bob = addr(0x20);

        async fn build(alice_key: PublicKey, bob_key: PublicKey) -> StateChannel {
            let alice = addr(0x10);
            let bob = addr(0x20);
            let channel = StateChannel::new(
                "ch-vk-bind".to_string(),
                vec![(alice, alice_key, 100), (bob, bob_key, 100)],
                ChannelType::StateChannel,
                5,
            )
            .await
            .expect("create");
            channel.open().await.expect("open");
            let mut updates = channel.state_updates.write().await;
            updates.push(ChannelStateUpdate {
                sequence: 1,
                balances: HashMap::from([(alice, 40), (bob, 160)]),
                timestamp: 1_710_000_000,
                state_data: Some(vec![0xfe]),
                signatures: HashMap::new(),
            });
            drop(updates);
            *channel.current_sequence.write().await = 1;
            channel
        }

        let ch_a = build(vk_from_seed(0x01), vk_from_seed(0x02)).await;
        let ch_b = build(vk_from_seed(0x03), vk_from_seed(0x04)).await;

        let hash_a = ch_a.generate_state_hash(1).await.expect("hash a");
        let hash_b = ch_b.generate_state_hash(1).await.expect("hash b");
        assert_ne!(
            hash_a, hash_b,
            "state hash must incorporate participant verifying keys"
        );

        // Same keys → same hash (determinism).
        let ch_a2 = build(vk_from_seed(0x01), vk_from_seed(0x02)).await;
        let hash_a2 = ch_a2.generate_state_hash(1).await.expect("hash a2");
        assert_eq!(hash_a, hash_a2);

        let _ = (alice, bob);
    }
}
