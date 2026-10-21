# ZK circuit correctness contract

Proof verification only proves that the encoded constraints were satisfied. It
does not prove those constraints express the intended native computation. The
`zk-circuits/tests/differential.rs` gate therefore checks both directions for
every circuit family: native acceptance implies circuit satisfaction, and a
deliberately wrong witness or public input implies unsatisfaction.

All randomized cases use fixed xorshift seeds. The gate has no wall-clock,
floating-point, unordered-map, or unseeded-randomness input.

## Canonical constraint counts

These counts use the production Poseidon configuration and are asserted by
`canonical_constraint_counts_do_not_drop_silently`:

| Circuit shape | Constraints |
|---|---:|
| range proof, 64 bits | 306 |
| confidential transfer, 1 input / 1 output / 64 bits | 614 |
| selective disclosure, 64 fields | 7,774 |
| weather model, 128 state samples | 15,794 |

An intentional circuit change must explain any count delta. A reduction is a
security review event because it can indicate an under-constrained witness.

## #819 regression

The old selective-disclosure commitment was a weighted linear sum. The test
constructs two distinct three-field witnesses that collide under that exact
formula, then proves they differ under the production Poseidon commitment and
that the forged witness cannot satisfy the circuit against the original public
commitment. This control makes the regression load-bearing rather than an
abstract claim that changing an input usually changes a hash.

