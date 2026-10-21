//! Canonical chunk layout (#2183): the one chunking every DA layer agrees on.
//!
//! `CHUNK_SIZE` bytes per chunk, the final chunk zero-padded, chunks indexed by
//! `u16` in payload order. The layout is a pure function of the payload bytes,
//! so a producer, a full node and a light node derive identical chunks (and
//! therefore identical commitments) from identical bytes. #2184's erasure
//! coding and #2185's blob commitment both consume this layout rather than
//! defining their own — the red-team note on the epic named that collision.

use thiserror::Error;

/// Bytes per chunk. 1 KiB keeps a single sample under one MTU-ish request
/// and makes a 64 MiB payload the hard ceiling of a `u16` index space.
pub const CHUNK_SIZE: usize = 1024;

/// Hard ceiling on chunks per payload: the `u16` index space.
pub const MAX_CHUNKS: usize = u16::MAX as usize;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ChunkLayoutError {
    #[error("payload of {len} bytes needs {needed} chunks, above the {MAX_CHUNKS} chunk ceiling")]
    TooLarge { len: usize, needed: usize },
    #[error("an empty payload has no chunks to commit to")]
    Empty,
}

/// Number of chunks a payload of `len` bytes occupies (ceiling division).
pub fn chunk_count(len: usize) -> usize {
    len.div_ceil(CHUNK_SIZE)
}

/// A payload cut into the canonical chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkLayout {
    payload_len: u64,
    chunks: Vec<[u8; CHUNK_SIZE]>,
}

impl ChunkLayout {
    /// Cut `payload` into chunks. Empty payloads and payloads above the
    /// `u16` chunk ceiling are refused rather than silently truncated.
    pub fn for_payload(payload: &[u8]) -> Result<Self, ChunkLayoutError> {
        if payload.is_empty() {
            return Err(ChunkLayoutError::Empty);
        }
        let needed = chunk_count(payload.len());
        if needed > MAX_CHUNKS {
            return Err(ChunkLayoutError::TooLarge {
                len: payload.len(),
                needed,
            });
        }
        let mut chunks = Vec::with_capacity(needed);
        for piece in payload.chunks(CHUNK_SIZE) {
            let mut chunk = [0u8; CHUNK_SIZE];
            chunk[..piece.len()].copy_from_slice(piece);
            chunks.push(chunk);
        }
        Ok(Self {
            payload_len: payload.len() as u64,
            chunks,
        })
    }

    /// Original payload length in bytes (the padding is not payload).
    pub fn payload_len(&self) -> u64 {
        self.payload_len
    }

    /// Number of chunks; fits a `u16` by construction.
    pub fn total_chunks(&self) -> u16 {
        self.chunks.len() as u16
    }

    /// The chunk at `index`, padded.
    pub fn chunk(&self, index: u16) -> Option<&[u8; CHUNK_SIZE]> {
        self.chunks.get(usize::from(index))
    }

    /// All chunks in index order.
    pub fn chunks(&self) -> &[[u8; CHUNK_SIZE]] {
        &self.chunks
    }

    /// Reassemble the payload (drops the padding).
    pub fn to_payload(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.payload_len as usize);
        for chunk in &self.chunks {
            out.extend_from_slice(chunk);
        }
        out.truncate(self.payload_len as usize);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_a_pure_function_of_the_bytes_and_round_trips() {
        let payload: Vec<u8> = (0..(CHUNK_SIZE * 3 + 17))
            .map(|i| (i % 251) as u8)
            .collect();
        let a = ChunkLayout::for_payload(&payload).unwrap();
        let b = ChunkLayout::for_payload(&payload).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.total_chunks(), 4);
        assert_eq!(a.payload_len(), payload.len() as u64);
        assert_eq!(a.to_payload(), payload);
        // The final chunk is zero-padded past the payload.
        let last = a.chunk(3).unwrap();
        assert!(last[17..].iter().all(|b| *b == 0));
        assert!(a.chunk(4).is_none());
    }

    #[test]
    fn exact_multiple_has_no_padding_chunk() {
        let payload = vec![7u8; CHUNK_SIZE * 2];
        let layout = ChunkLayout::for_payload(&payload).unwrap();
        assert_eq!(layout.total_chunks(), 2);
        assert_eq!(chunk_count(CHUNK_SIZE * 2), 2);
        assert_eq!(chunk_count(CHUNK_SIZE * 2 + 1), 3);
        assert_eq!(chunk_count(1), 1);
    }

    #[test]
    fn empty_and_oversized_payloads_are_refused() {
        assert_eq!(ChunkLayout::for_payload(&[]), Err(ChunkLayoutError::Empty));
        let len = CHUNK_SIZE * (MAX_CHUNKS + 1);
        assert_eq!(chunk_count(len), MAX_CHUNKS + 1);
        // Do not allocate 64 MiB in a unit test: exercise the arithmetic only.
        assert!(chunk_count(len) > MAX_CHUNKS);
    }
}
