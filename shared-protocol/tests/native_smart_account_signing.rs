//! Native AA signing foundations only: no transaction admission or activation.
use wrfcoin_shared_protocol::accounts::native_smart_accounts::*;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;
use wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated;
use wrfcoin_shared_protocol::crypto::hash::hash_bytes;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

#[path = "fixtures/native_smart_accounts_golden.rs"]
mod vectors;

fn public(seed: u8) -> [u8; 32] {
    PublicKey::from_private_key(&PrivateKey::from_bytes(&[seed; 32]).unwrap())
        .as_bytes()
        .try_into()
        .unwrap()
}

fn grant() -> NativeSessionGrant {
    NativeSessionGrant {
        public_key: public(8),
        generation: 2,
        targets: vec![Address([11; 20])],
        per_use_cap: 20,
        total_cap: 40,
        valid_from: 8,
        valid_until: 9,
        max_uses: 3,
    }
}

fn terms() -> NativeSponsorTerms {
    NativeSponsorTerms {
        public_key: public(9),
        nonce: 13,
        fee_cap: 14,
        valid_from: 8,
        valid_until: 9,
    }
}

fn actions() -> Vec<NativeAccountAction> {
    let mut guardians = vec![public(7), public(8)];
    guardians.sort();
    let r = NativeRecoveryTarget {
        request_nonce: 3,
        policy_generation: 2,
        new_owner: public(8),
    };
    vec![
        NativeAccountAction::Create { owner: public(7) },
        subject().action,
        NativeAccountAction::RegisterSession(grant()),
        NativeAccountAction::RevokeSession {
            public_key: public(8),
            generation: 2,
        },
        NativeAccountAction::SetRecoveryPolicy(NativeRecoveryPolicy {
            guardians,
            threshold: 2,
            time_lock_blocks: 100,
            cooldown_blocks: 50,
            generation: 2,
        }),
        NativeAccountAction::ApproveRecovery(r.clone()),
        NativeAccountAction::CancelRecovery(r.clone()),
        NativeAccountAction::ExecuteRecovery(r),
        NativeAccountAction::SponsorDeposit { amount: 20 },
        NativeAccountAction::SponsorRequestWithdrawal {
            amount: 20,
            request_nonce: 2,
        },
        NativeAccountAction::SponsorWithdrawal {
            amount: 20,
            request_nonce: 2,
        },
    ]
}

fn golden(body: &[u8], preimage: &[u8], seed: u8, expected: (&str, &str, &str, &str)) {
    assert_eq!(hex::encode(body), expected.0, "canonical body");
    assert_eq!(hex::encode(preimage), expected.1, "full preimage");
    assert_eq!(
        hex::encode(hash_bytes(preimage).as_bytes()),
        expected.2,
        "digest"
    );
    let sk = PrivateKey::from_bytes(&[seed; 32]).unwrap();
    let sig = sk.sign(preimage);
    assert_eq!(sig.to_hex(), expected.3, "signature");
    assert!(PublicKey::from_private_key(&sk).verify(preimage, &sig));
}

#[test]
fn independent_goldens_cover_every_action_and_signed_subject() {
    use vectors::*;
    let expected = [
        (
            ACTION_0_BODY,
            ACTION_0_PREIMAGE,
            ACTION_0_DIGEST,
            ACTION_0_SIGNATURE,
        ),
        (
            ACTION_1_BODY,
            ACTION_1_PREIMAGE,
            ACTION_1_DIGEST,
            ACTION_1_SIGNATURE,
        ),
        (
            ACTION_2_BODY,
            ACTION_2_PREIMAGE,
            ACTION_2_DIGEST,
            ACTION_2_SIGNATURE,
        ),
        (
            ACTION_3_BODY,
            ACTION_3_PREIMAGE,
            ACTION_3_DIGEST,
            ACTION_3_SIGNATURE,
        ),
        (
            ACTION_4_BODY,
            ACTION_4_PREIMAGE,
            ACTION_4_DIGEST,
            ACTION_4_SIGNATURE,
        ),
        (
            ACTION_5_BODY,
            ACTION_5_PREIMAGE,
            ACTION_5_DIGEST,
            ACTION_5_SIGNATURE,
        ),
        (
            ACTION_6_BODY,
            ACTION_6_PREIMAGE,
            ACTION_6_DIGEST,
            ACTION_6_SIGNATURE,
        ),
        (
            ACTION_7_BODY,
            ACTION_7_PREIMAGE,
            ACTION_7_DIGEST,
            ACTION_7_SIGNATURE,
        ),
        (
            ACTION_8_BODY,
            ACTION_8_PREIMAGE,
            ACTION_8_DIGEST,
            ACTION_8_SIGNATURE,
        ),
        (
            ACTION_9_BODY,
            ACTION_9_PREIMAGE,
            ACTION_9_DIGEST,
            ACTION_9_SIGNATURE,
        ),
        (
            ACTION_10_BODY,
            ACTION_10_PREIMAGE,
            ACTION_10_DIGEST,
            ACTION_10_SIGNATURE,
        ),
    ];
    assert_eq!(actions().len(), expected.len());
    for (action, expected) in actions().into_iter().zip(expected) {
        let s = NativeAccountSubject {
            action,
            ..subject()
        };
        golden(&s.canonical_body(), &s.preimage().unwrap(), 7, expected);
    }
    let s = NativeAccountSubject {
        sponsor: Some(terms()),
        ..subject()
    };
    golden(
        &s.canonical_body(),
        &s.preimage().unwrap(),
        7,
        (
            SPONSORED_BODY,
            SPONSORED_PREIMAGE,
            SPONSORED_DIGEST,
            SPONSORED_SIGNATURE,
        ),
    );
    let sponsor = NativeSponsorSubject::from_account(&s).unwrap();
    golden(
        &sponsor.canonical_body(),
        &sponsor.preimage(),
        9,
        (
            SPONSOR_BODY,
            SPONSOR_PREIMAGE,
            SPONSOR_DIGEST,
            SPONSOR_SIGNATURE,
        ),
    );
    let registration = NativeAccountSubject {
        action: NativeAccountAction::RegisterSession(grant()),
        ..subject()
    };
    let possession = NativeSessionPossession::from_account(&registration).unwrap();
    golden(
        &possession.canonical_body(),
        &possession.preimage(),
        8,
        (
            POSSESSION_BODY,
            POSSESSION_PREIMAGE,
            POSSESSION_DIGEST,
            POSSESSION_SIGNATURE,
        ),
    );
}

#[test]
fn every_envelope_field_changes_the_signed_bytes_or_is_refused() {
    let s = subject();
    let sk = PrivateKey::from_bytes(&[7; 32]).unwrap();
    let pk = PublicKey::from_private_key(&sk);
    let signature = sk.sign(&s.preimage().unwrap());
    let mut changed = Vec::new();
    macro_rules! change {
        ($field:ident, $value:expr) => {{
            let mut c = s.clone();
            c.$field = $value;
            changed.push(c);
        }};
    }
    change!(chain_id, "other".into());
    change!(network_id, "other".into());
    change!(genesis_hash, [99; 32]);
    change!(schema_version, 2);
    change!(target, Address([99; 20]));
    change!(actor, public(8));
    change!(role, NativeActorRole::Session);
    change!(role, NativeActorRole::External);
    change!(nonce, 99);
    change!(owner_epoch, 99);
    change!(valid_from, 7);
    change!(valid_until, 10);
    change!(fee_cap, 99);
    change!(sponsor, Some(terms()));
    for action in actions() {
        if action != s.action {
            change!(action, action);
        }
    }
    for c in changed {
        assert!(
            !c.preimage()
                .map(|p| pk.verify(&p, &signature))
                .unwrap_or(false)
        );
    }
}

#[test]
fn sponsor_signature_binds_each_term_and_account_content_without_signature_cycle() {
    let s = NativeAccountSubject {
        sponsor: Some(terms()),
        ..subject()
    };
    let sk = PrivateKey::from_bytes(&[9; 32]).unwrap();
    let sig = sk.sign(&NativeSponsorSubject::from_account(&s).unwrap().preimage());
    let pk = PublicKey::from_private_key(&sk);
    let mut changed = vec![
        NativeAccountSubject {
            nonce: 99,
            ..s.clone()
        },
        NativeAccountSubject {
            target: Address([99; 20]),
            ..s.clone()
        },
    ];
    for term in [
        NativeSponsorTerms {
            public_key: public(8),
            ..terms()
        },
        NativeSponsorTerms {
            nonce: 99,
            ..terms()
        },
        NativeSponsorTerms {
            fee_cap: 99,
            ..terms()
        },
        NativeSponsorTerms {
            valid_from: 7,
            ..terms()
        },
        NativeSponsorTerms {
            valid_until: 10,
            ..terms()
        },
    ] {
        changed.push(NativeAccountSubject {
            sponsor: Some(term),
            ..s.clone()
        });
    }
    for c in changed {
        assert_ne!(s.preimage().unwrap(), c.preimage().unwrap());
        assert!(!pk.verify(
            &NativeSponsorSubject::from_account(&c).unwrap().preimage(),
            &sig
        ));
    }
    assert!(!pk.verify(&s.preimage().unwrap(), &sig));
    assert!(NativeSponsorSubject::from_account(&subject()).is_err());
}

#[test]
fn session_possession_binds_grant_scope_generation_epoch_and_owner() {
    let s = NativeAccountSubject {
        action: NativeAccountAction::RegisterSession(grant()),
        ..subject()
    };
    let sk = PrivateKey::from_bytes(&[8; 32]).unwrap();
    let pk = PublicKey::from_private_key(&sk);
    let sig = sk.sign(
        &NativeSessionPossession::from_account(&s)
            .unwrap()
            .preimage(),
    );
    let mut changed = vec![
        NativeAccountSubject {
            owner_epoch: 99,
            ..s.clone()
        },
        NativeAccountSubject {
            actor: public(9),
            ..s.clone()
        },
    ];
    macro_rules! change_grant {
        ($field:ident, $value:expr) => {{
            let mut g = grant();
            g.$field = $value;
            changed.push(NativeAccountSubject {
                action: NativeAccountAction::RegisterSession(g),
                ..s.clone()
            });
        }};
    }
    change_grant!(public_key, public(9));
    change_grant!(generation, 99);
    change_grant!(targets, vec![Address([99; 20])]);
    change_grant!(per_use_cap, 19);
    change_grant!(total_cap, 99);
    change_grant!(valid_from, 7);
    change_grant!(valid_until, 10);
    change_grant!(max_uses, 99);
    for c in changed {
        assert!(
            !pk.verify(
                &NativeSessionPossession::from_account(&c)
                    .unwrap()
                    .preimage(),
                &sig
            )
        );
    }
    assert!(NativeSessionPossession::from_account(&subject()).is_err());
    assert!(!pk.verify(&s.preimage().unwrap(), &sig));
}

#[test]
fn shape_refuses_noncanonical_sets_identity_windows_and_arithmetic_overflow() {
    let mut invalid = Vec::new();
    for targets in [
        vec![],
        vec![Address([11; 20]); 2],
        vec![Address([12; 20]), Address([11; 20])],
        vec![Address([11; 20]); 65],
    ] {
        invalid.push(NativeAccountAction::RegisterSession(NativeSessionGrant {
            targets,
            ..grant()
        }));
    }
    invalid.push(NativeAccountAction::Batch(vec![
        NativeTransfer {
            to: Address([11; 20]),
            amount: u64::MAX,
        },
        NativeTransfer {
            to: Address([12; 20]),
            amount: 1,
        },
    ]));
    invalid.push(NativeAccountAction::Batch(vec![NativeTransfer {
        to: Address([11; 20]),
        amount: 0,
    }]));
    for action in actions() {
        if let NativeAccountAction::SetRecoveryPolicy(p) = action {
            for guardians in [vec![], vec![public(7); 2], vec![public(7); 17]] {
                invalid.push(NativeAccountAction::SetRecoveryPolicy(
                    NativeRecoveryPolicy {
                        guardians,
                        ..p.clone()
                    },
                ));
            }
            for threshold in [0, 3] {
                invalid.push(NativeAccountAction::SetRecoveryPolicy(
                    NativeRecoveryPolicy {
                        threshold,
                        ..p.clone()
                    },
                ));
            }
        }
    }
    for action in invalid {
        assert!(
            NativeAccountSubject {
                action,
                ..subject()
            }
            .preimage()
            .is_err()
        );
    }
    for chain_id in ["".into(), "a".repeat(65), " a ".into()] {
        assert!(
            NativeAccountSubject {
                chain_id,
                ..subject()
            }
            .preimage()
            .is_err()
        );
    }
    assert!(
        NativeAccountSubject {
            valid_from: 10,
            ..subject()
        }
        .preimage()
        .is_err()
    );
    assert!(
        NativeAccountSubject {
            action: NativeAccountAction::RegisterSession(NativeSessionGrant {
                per_use_cap: 41,
                ..grant()
            }),
            ..subject()
        }
        .preimage()
        .is_err()
    );
    assert!(
        NativeAccountSubject {
            action: NativeAccountAction::RegisterSession(NativeSessionGrant {
                max_uses: 0,
                ..grant()
            }),
            ..subject()
        }
        .preimage()
        .is_err()
    );
}

#[test]
fn distinct_sets_accept_limits_and_refuse_limit_plus_one() {
    for count in [MAX_NATIVE_TARGETS, MAX_NATIVE_TARGETS + 1] {
        let targets = (0..count).map(|i| Address([i as u8; 20])).collect();
        let s = NativeAccountSubject {
            action: NativeAccountAction::RegisterSession(NativeSessionGrant { targets, ..grant() }),
            ..subject()
        };
        assert_eq!(s.preimage().is_ok(), count == MAX_NATIVE_TARGETS);
    }
    for count in [MAX_NATIVE_GUARDIANS, MAX_NATIVE_GUARDIANS + 1] {
        let mut guardians: Vec<_> = (0..count).map(|i| public(i as u8 + 20)).collect();
        guardians.sort();
        let s = NativeAccountSubject {
            action: NativeAccountAction::SetRecoveryPolicy(NativeRecoveryPolicy {
                guardians,
                threshold: 2,
                time_lock_blocks: 100,
                cooldown_blocks: 50,
                generation: 2,
            }),
            ..subject()
        };
        assert_eq!(s.preimage().is_ok(), count == MAX_NATIVE_GUARDIANS);
    }
    let s = NativeAccountSubject {
        action: NativeAccountAction::Batch(vec![
            NativeTransfer {
                to: Address([11; 20]),
                amount: 1
            };
            MAX_NATIVE_CALLS
        ]),
        ..subject()
    };
    assert!(s.preimage().is_ok());
}

#[test]
fn address_derivation_is_chain_genesis_creator_nonce_bound() {
    let chain = ChainIdentity::new("aa-test", "local");
    let address = native_account_address(&chain, &[3; 32], &public(7), 5).unwrap();
    assert_eq!(hex::encode(address.0), vectors::ADDRESS);
    for other in [
        native_account_address(
            &ChainIdentity::new("other", "local"),
            &[3; 32],
            &public(7),
            5,
        )
        .unwrap(),
        native_account_address(&chain, &[99; 32], &public(7), 5).unwrap(),
        native_account_address(&chain, &[3; 32], &public(8), 5).unwrap(),
        native_account_address(&chain, &[3; 32], &public(7), 99).unwrap(),
    ] {
        assert_ne!(address, other);
    }
}

fn subject() -> NativeAccountSubject {
    let key = PrivateKey::from_bytes(&[7; 32]).unwrap();
    NativeAccountSubject {
        chain_id: "aa-test".into(),
        network_id: "local".into(),
        genesis_hash: [3; 32],
        schema_version: 1,
        target: Address([4; 20]),
        actor: PublicKey::from_private_key(&key)
            .as_bytes()
            .try_into()
            .unwrap(),
        role: NativeActorRole::Owner,
        nonce: 5,
        owner_epoch: 6,
        valid_from: 8,
        valid_until: 9,
        fee_cap: 10,
        action: NativeAccountAction::Batch(vec![NativeTransfer {
            to: Address([11; 20]),
            amount: 12,
        }]),
        sponsor: None,
    }
}

#[test]
fn native_subject_binds_epoch_nonce_chain_and_batch() {
    let sk = PrivateKey::from_bytes(&[7; 32]).unwrap();
    let pk = PublicKey::from_private_key(&sk);
    let s = subject();
    let sig = sk.sign(&s.preimage().unwrap());
    assert!(pk.verify(&s.preimage().unwrap(), &sig));
    for changed in [
        NativeAccountSubject {
            nonce: 99,
            ..s.clone()
        },
        NativeAccountSubject {
            owner_epoch: 99,
            ..s.clone()
        },
        NativeAccountSubject {
            chain_id: "other".into(),
            ..s.clone()
        },
        NativeAccountSubject {
            action: NativeAccountAction::Batch(vec![NativeTransfer {
                to: Address([11; 20]),
                amount: 99,
            }]),
            ..s.clone()
        },
    ] {
        assert!(!pk.verify(&changed.preimage().unwrap(), &sig));
    }
}

#[test]
fn native_signing_refuses_empty_and_oversized_batches() {
    let mut s = subject();
    for count in [0, MAX_NATIVE_CALLS + 1] {
        s.action = NativeAccountAction::Batch(vec![
            NativeTransfer {
                to: Address([11; 20]),
                amount: 1
            };
            count
        ]);
        assert!(s.preimage().is_err());
    }
}
