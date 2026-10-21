// shared-protocol/benches/crypto_bench.rs
//
// Criterion benchmarks for hot cryptographic paths.
// Run with: cargo bench -p wrfcoin-shared-protocol --bench crypto_bench

use criterion::{criterion_group, criterion_main, Criterion};
use rand::rngs::OsRng;
use wrfcoin_shared_protocol::crypto::{
    hash::{blake3_hash, hash_bytes, sha256_hash, sha3_hash},
    signature::{PrivateKey, PublicKey},
};

fn bench_blake3_hash(c: &mut Criterion) {
    let data = vec![0u8; 256];
    c.bench_function("hash_blake3_256b", |b| b.iter(|| blake3_hash(&data)));
}

fn bench_sha3_hash(c: &mut Criterion) {
    let data = vec![0u8; 256];
    c.bench_function("hash_sha3_256b", |b| b.iter(|| sha3_hash(&data)));
}

fn bench_sha256_hash(c: &mut Criterion) {
    let data = vec![0u8; 256];
    c.bench_function("hash_sha256_256b", |b| b.iter(|| sha256_hash(&data)));
}

fn bench_hash_default(c: &mut Criterion) {
    let data = vec![0u8; 256];
    c.bench_function("hash_default_256b", |b| b.iter(|| hash_bytes(&data)));
}

fn bench_ed25519_sign(c: &mut Criterion) {
    let mut csprng = OsRng;
    let private_key = PrivateKey::generate(&mut csprng);
    let msg = b"wrfcoin benchmark message for signing";
    c.bench_function("ed25519_sign", |b| b.iter(|| private_key.sign(msg)));
}

fn bench_ed25519_verify(c: &mut Criterion) {
    let mut csprng = OsRng;
    let private_key = PrivateKey::generate(&mut csprng);
    let public_key = PublicKey::from_private_key(&private_key);
    let msg = b"wrfcoin benchmark message for verify";
    let sig = private_key.sign(msg);
    c.bench_function("ed25519_verify", |b| {
        b.iter(|| public_key.verify(msg, &sig))
    });
}

criterion_group!(
    benches,
    bench_blake3_hash,
    bench_sha3_hash,
    bench_sha256_hash,
    bench_hash_default,
    bench_ed25519_sign,
    bench_ed25519_verify,
);
criterion_main!(benches);
