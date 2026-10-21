#[path = "../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod fixture;
use wrfcoin_shared_protocol::consensus::{node_share_contracts::NodeServiceBatch,node_receipt_registry::{claim_hash,batch_hash}};
use wrfcoin_shared_protocol::crypto::{domain_signing::DomainSeparated,chain_binding::SigningProtocolVersion};
fn main() {
 let state=fixture::state();
 let claim=fixture::claim(&state,1);
 let p=fixture::submission(vec![claim.clone()],1);
 p.verify_authorization(&fixture::identity(),&[7;32]).unwrap();
 let preimage=p.authorization.signing_preimage_for(&fixture::identity(),SigningProtocolVersion::V2);
 let batch=NodeServiceBatch{claims:vec![claim.clone()]};
 let mut bad=claim.clone();bad.consumer_signature[0]^=1;
 let endorsement=wrfcoin_shared_protocol::consensus::node_share_contracts::NodeSettlementEndorsement{ticket_preimage_hash:wrfcoin_shared_protocol::consensus::node_share_contracts::node_contract_digest(&claim.ticket,&fixture::identity())};
 let mut v1=claim.clone();v1.consumer_signature=fixture::key(22).sign(&endorsement.signing_preimage_for(&fixture::identity(),SigningProtocolVersion::V1)).as_bytes().to_vec();
 let mut wrong_domain=endorsement.signing_preimage_for(&fixture::identity(),SigningProtocolVersion::V2);wrong_domain[8]^=1;
 let mut domain=claim.clone();domain.consumer_signature=fixture::key(22).sign(&wrong_domain).as_bytes().to_vec();
 assert!(wrfcoin_shared_protocol::consensus::node_receipt_registry::verify_claim(&state,&v1,fixture::H,&fixture::identity()).is_err());
 assert!(wrfcoin_shared_protocol::consensus::node_receipt_registry::verify_claim(&state,&domain,fixture::H,&fixture::identity()).is_err());
 wrfcoin_shared_protocol::consensus::node_receipt_registry::verify_claim(&state,&claim,fixture::H,&fixture::identity()).unwrap();
 assert!(wrfcoin_shared_protocol::consensus::node_receipt_registry::verify_claim(&state,&bad,fixture::H,&fixture::identity()).is_err());
 println!("{}",serde_json::json!({"authorization":p.authorization,"preimage_hex":hex::encode(preimage),"owner_signature":p.owner_signature,"owner_key":p.owner_key,"batch_hash":hex::encode(batch_hash(&batch).unwrap()),"archive":{"claim_hex":hex::encode(claim.to_wire().unwrap()),"receipt_id":hex::encode(claim_hash(&claim).unwrap()),"included":false},"v1_archive":{"claim_hex":hex::encode(v1.to_wire().unwrap()),"receipt_id":hex::encode(claim_hash(&v1).unwrap()),"included":false},"domain_archive":{"claim_hex":hex::encode(domain.to_wire().unwrap()),"receipt_id":hex::encode(claim_hash(&domain).unwrap()),"included":false},"bad_archive":{"claim_hex":hex::encode(bad.to_wire().unwrap()),"receipt_id":hex::encode(claim_hash(&bad).unwrap()),"included":false}}));
}
