//! Independent ADR-003 encoder: no BodyBuilder, canonical_body or production
//! preimage helper. Crypto primitives are shared, encoding and tags are not.
#![allow(dead_code)]
use serde_json::Value;
use sha3::{Digest, Sha3_256};
use wrfcoin_shared_protocol::blockchain::snapshot_sync::SignedSnapshotSyncPayload;
use wrfcoin_shared_protocol::consensus::node_service_receipt::SignedNodeServiceReceipt;

#[derive(Default)]
pub struct Encoder(pub Vec<u8>);
impl Encoder {
    pub fn bytes(&mut self, bytes: &[u8]) {
        self.u64(bytes.len() as u64);
        self.0.extend_from_slice(bytes);
    }
    pub fn text(&mut self, text: &str) {
        self.bytes(text.as_bytes());
    }
    pub fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    pub fn i64(&mut self, value: i64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    pub fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn array(&mut self, value: &Value) {
        self.bytes(
            &value
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect::<Vec<_>>(),
        );
    }
}
pub fn receipt_hash(s: &SignedNodeServiceReceipt) -> [u8; 32] {
    let r = &s.receipt;
    let mut b = Encoder::default();
    b.text(match r.service {
        wrfcoin_shared_protocol::consensus::node_service_receipt::NodeService::SnapshotJoin => {
            "snapshot_join"
        },
        wrfcoin_shared_protocol::consensus::node_service_receipt::NodeService::SyncRange => {
            "sync_range"
        },
    });
    b.text(&r.server_id);
    b.bytes(&r.consumer_pubkey);
    b.u64(r.chain_height);
    b.bytes(&r.subject_hash);
    b.u64(r.units);
    b.i64(r.timestamp);
    b.bytes(&r.nonce);
    let mut e = Encoder::default();
    e.text("wrfcoin.node_service_receipt.v1");
    e.bytes(&b.0);
    e.bytes(&s.signature);
    Sha3_256::digest(&e.0).into()
}
pub fn manifest_hash(s: &SignedSnapshotSyncPayload) -> [u8; 32] {
    let p = &s.payload;
    let mut e = Encoder::default();
    e.text("wrfcoin.node_snapshot_manifest_commitment.v1");
    e.text(&s.signer_id);
    e.i64(s.timestamp);
    e.text(&s.nonce);
    e.u64(p.height);
    e.text(&p.state_root_hex);
    e.text(&p.snapshot_root_hex);
    e.u64(p.entry_count);
    e.text(&p.tip_hash_hex);
    e.u64(p.finalized_height);
    e.text(&s.signature);
    Sha3_256::digest(&e.0).into()
}
pub fn tag(kind: &str) -> &'static str {
    match kind {
        "NodeServiceTicket" => "wrfcoin.node_service_ticket.v1",
        "NodeSettlementEndorsement" => "wrfcoin.node_settlement_endorsement.v1",
        "NodeBondAuthorization" => "wrfcoin.node_bond_authorization.v1",
        "NodeBondPossession" => "wrfcoin.node_bond_possession.v1",
        _ => panic!("unknown reference kind"),
    }
}
pub fn body(kind: &str, v: &Value) -> Vec<u8> {
    let mut e = Encoder::default();
    match kind {
        "NodeServiceTicket" => {
            let c = &v["context"];
            let mut ctx = Encoder::default();
            ctx.array(&c["genesis_hash"]);
            ctx.text(c["chain_id"].as_str().unwrap());
            ctx.text(c["network_id"].as_str().unwrap());
            ctx.u64(c["epoch"].as_u64().unwrap());
            ctx.u64(c["roster_sequence"].as_u64().unwrap());
            ctx.array(&c["roster_hash"]);
            ctx.array(&c["epoch_start_parent"]);
            e.bytes(&ctx.0);
            for name in [
                "server_id",
                "consumer_id",
                "ticket_id",
                "signed_receipt_hash",
                "signed_manifest_hash",
            ] {
                e.array(&v[name]);
            }
        },
        "NodeSettlementEndorsement" => e.array(&v["ticket_preimage_hash"]),
        "NodeBondAuthorization" => {
            for name in [
                "genesis_hash",
                "deployment_id",
                "owner_account",
                "escrow_account",
                "payout_account",
                "server_key",
                "consumer_key",
            ] {
                e.array(&v[name]);
            }
            e.text(v["service_alias"].as_str().unwrap());
            e.u32(match v["action"].as_str().unwrap() {
                "Deposit" => 0,
                "Update" => 1,
                "Exit" => 2,
                "Withdraw" => 3,
                _ => panic!("action"),
            });
            for name in ["amount_units", "expected_nonce", "effective_epoch"] {
                e.u64(v[name].as_u64().unwrap());
            }
        },
        "NodeBondPossession" => {
            e.array(&v["authorization_preimage_hash"]);
            e.u32(match v["role"].as_str().unwrap() {
                "Server" => 0,
                "Consumer" => 1,
                _ => panic!("role"),
            });
        },
        _ => panic!("kind"),
    }
    e.0
}
pub fn preimage(kind: &str, body: &[u8], chain: &str, network: &str) -> Vec<u8> {
    let mut e = Encoder::default();
    e.text(tag(kind));
    e.text(chain);
    e.text(network);
    e.u32(2);
    e.bytes(body);
    e.0
}

pub fn sample_receipt() -> SignedNodeServiceReceipt {
    use wrfcoin_shared_protocol::consensus::node_service_receipt::{
        NodeService, NodeServiceReceipt,
    };
    SignedNodeServiceReceipt {
        receipt: NodeServiceReceipt {
            service: NodeService::SnapshotJoin,
            server_id: "reference.example".into(),
            consumer_pubkey: [5; 32],
            chain_height: 77,
            subject_hash: [3; 32],
            units: 4,
            timestamp: -5,
            nonce: [6; 32],
        },
        signature: vec![7; 64],
    }
}
pub fn sample_manifest() -> SignedSnapshotSyncPayload {
    use wrfcoin_shared_protocol::blockchain::snapshot_sync::SnapshotSyncPayload;
    SignedSnapshotSyncPayload {
        signer_id: "04".repeat(32),
        timestamp: -5,
        nonce: "reference-nonce".into(),
        payload: SnapshotSyncPayload {
            height: 77,
            state_root_hex: "01".repeat(32),
            snapshot_root_hex: "02".repeat(32),
            entry_count: 4,
            tip_hash_hex: "03".repeat(32),
            finalized_height: 76,
        },
        signature: "07".repeat(64),
    }
}
