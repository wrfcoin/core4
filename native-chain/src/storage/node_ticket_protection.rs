//! Off-chain signer slashing protection. Never rolled back with chain WAL.
use super::Storage;
use redb::{Durability, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};

const TICKETS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("node_ticket_signing_protection_v1");
pub const MAX_NODE_TICKET_RECORDS: u64 = 4096;
const MAX_RECORD_BYTES: usize = 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedTicketRecord {
    ticket_hash: [u8; 32],
    signature: Vec<u8>,
    challenge_deadline: u64,
}
fn decode(bytes: &[u8]) -> anyhow::Result<SignedTicketRecord> {
    anyhow::ensure!(
        bytes.len() <= MAX_RECORD_BYTES,
        "Node signing record oversized"
    );
    let r: SignedTicketRecord = serde_json::from_slice(bytes)?;
    anyhow::ensure!(r.signature.len() == 64, "Node signing record malformed");
    Ok(r)
}
impl Storage {
    /// Key is epoch LE ++ consumer deployment ++ consumer-chosen ticket id.
    /// Exact signed preimage hash and signature are durable before exposure.
    /// The write transaction serializes concurrent signers. Immediate commit
    /// fsyncs before returning, including identical retries after restart.
    /// Only a canonical chain height strictly after the inclusive challenge
    /// deadline permits pruning; wall clocks and WAL rollback never erase it.
    pub fn protected_node_ticket_signature(
        &self,
        key: &[u8; 72],
        ticket_hash: [u8; 32],
        height: u64,
        challenge_deadline: u64,
        sign: impl FnOnce() -> anyhow::Result<Vec<u8>>,
    ) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(
            height <= challenge_deadline,
            "Node signing deadline expired"
        );
        let mut transaction = self.db.begin_write()?;
        transaction.set_durability(Durability::Immediate);
        let signature;
        {
            let mut table = transaction.open_table(TICKETS)?;
            anyhow::ensure!(
                table.len()? <= MAX_NODE_TICKET_RECORDS,
                "Node signing ledger oversized"
            );
            let mut expired = Vec::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                anyhow::ensure!(key.value().len() == 72, "Node signing key malformed");
                if height > decode(value.value())?.challenge_deadline {
                    expired.push(key.value().to_vec());
                }
            }
            for key in expired {
                table.remove(key.as_slice())?;
            }
            let existing = table
                .get(key.as_slice())?
                .map(|v| decode(v.value()))
                .transpose()?;
            if let Some(record) = existing {
                anyhow::ensure!(
                    record.ticket_hash == ticket_hash
                        && record.challenge_deadline == challenge_deadline,
                    "Node ticket slashing-protection conflict"
                );
                signature = record.signature;
            } else {
                anyhow::ensure!(
                    table.len()? < MAX_NODE_TICKET_RECORDS,
                    "Node signing ledger full"
                );
                signature = sign()?;
                anyhow::ensure!(signature.len() == 64, "Node signer signature malformed");
                let bytes = serde_json::to_vec(&SignedTicketRecord {
                    ticket_hash,
                    signature: signature.clone(),
                    challenge_deadline,
                })?;
                anyhow::ensure!(
                    bytes.len() <= MAX_RECORD_BYTES,
                    "Node signing record oversized"
                );
                table.insert(key.as_slice(), bytes.as_slice())?;
            }
        }
        transaction.commit()?;
        Ok(signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signing_ledger_capacity_never_evicts_live_records_and_prunes_only_after_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protection.redb");
        let storage = Storage::open(&path, 10000).unwrap();
        let transaction = storage.db.begin_write().unwrap();
        {
            let mut table = transaction.open_table(TICKETS).unwrap();
            let bytes = serde_json::to_vec(&SignedTicketRecord {
                ticket_hash: [1; 32],
                signature: vec![7; 64],
                challenge_deadline: 100,
            })
            .unwrap();
            for i in 0..MAX_NODE_TICKET_RECORDS {
                let mut key = [0; 72];
                key[..8].copy_from_slice(&i.to_le_bytes());
                table.insert(key.as_slice(), bytes.as_slice()).unwrap();
            }
        }
        transaction.commit().unwrap();
        let live_key = [0; 72];
        assert!(
            storage
                .protected_node_ticket_signature(&live_key, [2; 32], 100, 100, || panic!(
                    "conflicting ticket must refuse before signing"
                ))
                .is_err()
        );
        assert_eq!(
            storage
                .protected_node_ticket_signature(&live_key, [1; 32], 100, 100, || panic!(
                    "identical ticket must reuse its signature"
                ))
                .unwrap(),
            vec![7; 64]
        );
        let mut key = [0; 72];
        key[..8].copy_from_slice(&MAX_NODE_TICKET_RECORDS.to_le_bytes());
        let signed = std::cell::Cell::new(false);
        assert!(
            storage
                .protected_node_ticket_signature(&key, [2; 32], 100, 200, || {
                    signed.set(true);
                    Ok(vec![9; 64])
                })
                .is_err()
        );
        assert!(
            !signed.get(),
            "capacity refusal precedes signature creation"
        );
        assert_eq!(
            storage
                .protected_node_ticket_signature(&key, [2; 32], 101, 200, || {
                    signed.set(true);
                    Ok(vec![9; 64])
                })
                .unwrap(),
            vec![9; 64]
        );
        assert!(signed.get());
        drop(storage);
        let reopened = Storage::open(&path, 10000).unwrap();
        assert_eq!(
            reopened
                .protected_node_ticket_signature(&key, [2; 32], 101, 200, || panic!(
                    "durable retry must not re-sign"
                ))
                .unwrap(),
            vec![9; 64]
        );
    }
}
