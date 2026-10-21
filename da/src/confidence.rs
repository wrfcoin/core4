//! The confidence a sample gives (#2186 acceptance: "the confidence
//! relationship is documented with explicit numbers").
//!
//! With `total_chunks` chunks of which `data_chunks` suffice to reconstruct,
//! a producer must withhold at least `w = total_chunks - data_chunks + 1`
//! chunks to make the block unreconstructable. A sampler drawing `s` distinct
//! chunks uniformly then misses every withheld chunk with probability
//! `C(n-w, s) / C(n, s)` (sampling without replacement), which the closed
//! form below computes exactly; `1 - that` is the probability the sampler
//! catches the withholding. `da/README.md` tabulates it.
//!
//! Until #2184 (erasure coding) lands, `data_chunks == total_chunks`, so
//! `w = 1`: a single withheld chunk breaks reconstruction and `s` samples
//! catch it with probability `s / n`. That is the honest number today, and
//! the reason light-node availability claims are stated as a confidence,
//! never as a fact.

/// Reconstruction threshold of a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErasureParams {
    /// Chunks that suffice to reconstruct the payload.
    pub data_chunks: u16,
    /// Chunks in the (possibly erasure-extended) layout.
    pub total_chunks: u16,
}

impl ErasureParams {
    /// No erasure coding: every chunk is needed (the pre-#2184 state).
    pub const fn uncoded(total_chunks: u16) -> Self {
        Self {
            data_chunks: total_chunks,
            total_chunks,
        }
    }

    /// Minimum number of chunks a producer must withhold to prevent
    /// reconstruction.
    pub fn min_withheld_to_break(&self) -> u16 {
        self.total_chunks
            .saturating_sub(self.data_chunks)
            .saturating_add(1)
            .min(self.total_chunks)
    }
}

/// Probability that `samples` distinct uniform samples hit at least one of
/// the `min_withheld_to_break` chunks a withholding producer must hide.
/// Returns 0.0 for degenerate inputs (no chunks, no samples).
pub fn detection_probability(params: ErasureParams, samples: u16) -> f64 {
    let n = f64::from(params.total_chunks);
    let w = f64::from(params.min_withheld_to_break());
    let s = u32::from(samples.min(params.total_chunks));
    if params.total_chunks == 0 || s == 0 {
        return 0.0;
    }
    // P(miss all) = prod_{i=0}^{s-1} (n - w - i) / (n - i)
    let mut miss = 1.0f64;
    for i in 0..s {
        let numerator = n - w - f64::from(i);
        if numerator <= 0.0 {
            miss = 0.0;
            break;
        }
        miss *= numerator / (n - f64::from(i));
    }
    (1.0 - miss).clamp(0.0, 1.0)
}

/// Smallest number of samples whose [`detection_probability`] reaches
/// `target` (a probability in `[0, 1)`), capped at `total_chunks`.
pub fn samples_for_confidence(params: ErasureParams, target: f64) -> u16 {
    if params.total_chunks == 0 {
        return 0;
    }
    let target = target.clamp(0.0, 1.0);
    for s in 1..=params.total_chunks {
        if detection_probability(params, s) >= target {
            return s;
        }
    }
    params.total_chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncoded_layout_detects_with_probability_s_over_n() {
        let p = ErasureParams::uncoded(100);
        assert_eq!(p.min_withheld_to_break(), 1);
        assert!((detection_probability(p, 1) - 0.01).abs() < 1e-12);
        assert!((detection_probability(p, 50) - 0.5).abs() < 1e-12);
        assert!((detection_probability(p, 100) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn half_rate_erasure_code_detects_fast() {
        // 128 of 256 reconstruct: a producer must hide 129 chunks, so each
        // sample catches it with probability > 1/2.
        let p = ErasureParams {
            data_chunks: 128,
            total_chunks: 256,
        };
        assert_eq!(p.min_withheld_to_break(), 129);
        let one = detection_probability(p, 1);
        assert!((one - 129.0 / 256.0).abs() < 1e-12);
        assert!(detection_probability(p, 16) > 0.9999);
        assert_eq!(samples_for_confidence(p, 0.99), 7);
        assert_eq!(samples_for_confidence(p, 0.999999), 19);
    }

    #[test]
    fn samples_for_confidence_is_monotone_and_capped() {
        let p = ErasureParams::uncoded(10);
        assert_eq!(samples_for_confidence(p, 0.0), 1);
        assert_eq!(samples_for_confidence(p, 0.5), 5);
        assert_eq!(samples_for_confidence(p, 1.0), 10);
        assert_eq!(samples_for_confidence(ErasureParams::uncoded(0), 0.5), 0);
        assert_eq!(detection_probability(p, 0), 0.0);
        // Over-sampling saturates rather than overflowing.
        assert!((detection_probability(p, u16::MAX) - 1.0).abs() < 1e-12);
    }
}
