//! Data availability for WRFCoin (epic #2183).
//!
//! Three layers, each usable on its own:
//!
//! * [`layout`] — the canonical chunk layout: a payload is cut into
//!   fixed-size chunks, the last one zero-padded, indexed by `u16`.
//! * [`commitment`] — a blake3 Merkle commitment over the chunk layout with
//!   per-chunk inclusion proofs. This is the *commitment shape* #2185 will
//!   place under a consensus feature gate; nothing here writes into a block.
//! * [`light_client`] — the light node (#2188): headers verified against a
//!   tracked producer set, set transitions verified against the previous set,
//!   optional finality certificates, DA sampling per header.
//! * [`sampling`] — the data-availability sampling protocol (#2186): a node
//!   asks for a small subset of chunks, chosen from a **node-local secret** so
//!   the serving side cannot predict which chunks will be requested, from
//!   several distinct peers, and accepts availability only when every sample
//!   returns with a valid proof. A timeout is `Inconclusive`, never
//!   `Unavailable`.
//!
//! Erasure coding (#2184) is not part of this crate yet. [`ErasureParams`]
//! carries `data_chunks` / `total_chunks` so the confidence arithmetic in
//! [`confidence`] is already stated in terms of the reconstruction threshold;
//! until #2184 lands `data_chunks == total_chunks` and withholding a single
//! chunk is enough to make a block unreconstructable — which the numbers in
//! `da/README.md` say plainly rather than hiding.

pub mod commitment;
pub mod confidence;
pub mod layout;
pub mod light_client;
pub mod sampling;

pub use commitment::{
    ChunkInclusionProof, DA_LEAF_DOMAIN, DA_NODE_DOMAIN, DaCommitment, DaCommitmentError,
};
pub use confidence::{ErasureParams, detection_probability, samples_for_confidence};
pub use layout::{CHUNK_SIZE, ChunkLayout, ChunkLayoutError, MAX_CHUNKS, chunk_count};
pub use light_client::{
    BANDWIDTH_BUDGET_PER_BLOCK, DA_COMMITMENT_METADATA_KEY, DaPolicy, FinalityVerifier,
    HeaderCommit, HeaderSource, LightCheckpoint, LightClient, LightClientConfig, LightClientStats,
    MEMORY_BUDGET_BYTES, SyncError, decode_da_commitment, encode_da_commitment,
    parse_da_commitment, resolve_da_commitment,
};
pub use sampling::{
    ChunkResponse, NODE_SECRET_MIN_LEN, PeerPenalty, SampleFetchError, SampleOutcome,
    SamplePenalty, SampleRequest, SampleSource, SamplingConfig, SamplingError, choose_indices,
    sample,
};
