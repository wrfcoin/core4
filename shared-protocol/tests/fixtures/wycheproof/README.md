# Wycheproof vectors (vendored)

`upstream/ecdsa_secp256r1_sha256_p1363_test.json` is the unmodified `testvectors_v1` file from
<https://github.com/C2SP/wycheproof> (Apache-2.0), vendored 2026-09-11 for #2169. It is executed
by `shared-protocol/tests/passkey_webauthn_2169.rs` against the pure P-256 verifier
(`wrfcoin_privacy::p256_ecdsa::verify_sha256`): every `valid` case must verify, every `invalid`
case must not, `acceptable` cases may go either way. The low-`s` consensus rule is tested
separately, because Wycheproof's `valid` set deliberately includes high-`s` twins (tcId 1,
"signature malleability").
