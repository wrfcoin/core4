//! Light-node serving routes (#2188): a header projection and a DA chunk
//! proof over a block's canonical bytes.
//!
//! `GET /chain/header/{height}` returns the [`LightHeader`] a light node
//! verifies (every field the hash covers, transaction *hashes* instead of
//! bodies) plus the DA commitment this node computes over the block's
//! canonical bytes (`bincode`, the same bytes the P2P layer gossips). The
//! commitment is served for convenience; a light node that has not been told
//! to trust a served root uses only the `da_commitment` metadata a producer
//! commits under #2185's feature gate — see `docs/LIGHT_CLIENT.md`.
//!
//! `GET /da/chunk/{height}/{index}` returns one chunk with its inclusion
//! proof against that commitment, which is what the sampler (#2186) asks for.

use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::Serialize;
use wrfcoin_da::{ChunkInclusionProof, ChunkLayout, DaCommitment};
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::light_header::LightHeader;

use crate::helpers::{envelope_error, envelope_success};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct LightHeaderResponse {
    pub header: LightHeader,
    /// Commitment over the block's canonical bytes, computed by this node.
    /// `served`: not verifiable against the header until #2185 commits it.
    pub da_commitment: DaCommitment,
    pub da_commitment_source: &'static str,
}

#[derive(Debug, Serialize)]
pub struct DaChunkResponse {
    pub height: u64,
    pub commitment: DaCommitment,
    pub proof: ChunkInclusionProof,
}

/// The DA payload of a block: its canonical wire bytes.
pub fn block_da_layout(block: &Block) -> Result<ChunkLayout, String> {
    let bytes = bincode::serialize(block).map_err(|e| format!("block does not serialize: {e}"))?;
    ChunkLayout::for_payload(&bytes).map_err(|e| e.to_string())
}

/// Pure core of `GET /chain/header/{height}`.
pub fn light_header_response(block: &Block) -> Result<LightHeaderResponse, String> {
    let layout = block_da_layout(block)?;
    Ok(LightHeaderResponse {
        header: LightHeader::from_block(block),
        da_commitment: DaCommitment::commit(&layout),
        da_commitment_source: "served",
    })
}

/// Pure core of `GET /da/chunk/{height}/{index}`.
pub fn da_chunk_response(block: &Block, index: u16) -> Result<Option<DaChunkResponse>, String> {
    let layout = block_da_layout(block)?;
    let commitment = DaCommitment::commit(&layout);
    match DaCommitment::prove(&layout, index) {
        Ok(proof) => Ok(Some(DaChunkResponse {
            height: block.height,
            commitment,
            proof,
        })),
        Err(_) => Ok(None),
    }
}

/// Window first, then the durable archive — the same lookup
/// `GET /chain/block/{height}` performs.
async fn load_block(
    req: &HttpRequest,
    state: &AppState,
    height: u64,
) -> Result<Block, HttpResponse> {
    let in_memory = {
        let blockchain = state.blockchain.read().await;
        blockchain.block_at_height(height).cloned()
    };
    if let Some(block) = in_memory {
        return Ok(block);
    }
    let storage = state.storage.read().await;
    match storage.load_blocks_range(height, height) {
        Ok(mut blocks) => blocks.pop().ok_or_else(|| {
            envelope_error(
                req,
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                "block not found at requested height",
            )
        }),
        Err(err) => {
            log::error!("Failed to load persisted block {height} for the light-node routes: {err}");
            Err(envelope_error(
                req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "failed to load persisted block",
            ))
        },
    }
}

pub async fn get_chain_header(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<u64>,
) -> impl Responder {
    let height = path.into_inner();
    let block = match load_block(&req, &state, height).await {
        Ok(block) => block,
        Err(resp) => return resp,
    };
    match light_header_response(&block) {
        Ok(response) => envelope_success(&req, StatusCode::OK, response),
        Err(reason) => {
            log::error!("light header projection failed at {height}: {reason}");
            envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "failed to project the block header",
            )
        },
    }
}

pub async fn get_da_chunk(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<(u64, u16)>,
) -> impl Responder {
    let (height, index) = path.into_inner();
    let block = match load_block(&req, &state, height).await {
        Ok(block) => block,
        Err(resp) => return resp,
    };
    match da_chunk_response(&block, index) {
        Ok(Some(response)) => envelope_success(&req, StatusCode::OK, response),
        Ok(None) => envelope_error(
            &req,
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "chunk index is outside the block's DA layout",
        ),
        Err(reason) => {
            log::error!("DA chunk proof failed at {height}/{index}: {reason}");
            envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "failed to build the chunk proof",
            )
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_native_chain::crypto::hash::hash_string;

    fn block(height: u64) -> Block {
        let mut block = Block::new(3, hash_string("prev"), Vec::new(), 1, height);
        block.metadata.insert("state_root".into(), "ab".repeat(32));
        let hash = block.calculate_hash();
        block.set_hash(hash);
        block
    }

    #[test]
    fn header_projection_hashes_to_the_block_hash_and_commitment_verifies_chunks() {
        let block = block(42);
        let response = light_header_response(&block).unwrap();
        assert_eq!(response.header.height, 42);
        assert_eq!(response.header.hash(), block.hash.unwrap());
        assert_eq!(
            response.header.verify_declared_hash().unwrap(),
            block.hash.unwrap()
        );
        assert_eq!(response.da_commitment_source, "served");
        let total = response.da_commitment.total_chunks;
        assert!(total >= 1);
        for index in 0..total {
            let chunk = da_chunk_response(&block, index).unwrap().expect("in range");
            assert_eq!(chunk.commitment, response.da_commitment);
            assert!(
                response.da_commitment.verify(&chunk.proof),
                "chunk {index} proves"
            );
        }
        assert!(
            da_chunk_response(&block, total).unwrap().is_none(),
            "past the layout is not found"
        );
    }

    #[test]
    fn commitment_is_over_the_canonical_wire_bytes() {
        let block = block(7);
        let bytes = bincode::serialize(&block).unwrap();
        let layout = ChunkLayout::for_payload(&bytes).unwrap();
        assert_eq!(
            light_header_response(&block).unwrap().da_commitment,
            DaCommitment::commit(&layout)
        );
    }
}
