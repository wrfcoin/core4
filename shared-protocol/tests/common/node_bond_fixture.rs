use wrfcoin_shared_protocol::{
    blockchain::state::{State, StateTransition},
    common::types::{Address, Balance},
    consensus::{node_bond_registry::*, node_share_contracts::*},
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion},
        domain_signing::{DomainSeparated, sign_domain_for},
        signature::{PrivateKey, PublicKey},
    },
    governance::schedule_extension::{
        AppliedExtension, ScheduleEntry, install_schedule_extension_authority_for_tests,
    },
};
pub const A: u64 = 3;
pub fn key(byte: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[byte; 32]).unwrap()
}
pub fn pk(byte: u8) -> [u8; 32] {
    PublicKey::from_private_key(&key(byte))
        .as_bytes()
        .try_into()
        .unwrap()
}
pub fn address(byte: u8) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(&key(byte)))
}
pub fn signature<T: DomainSeparated>(subject: &T, byte: u8) -> Vec<u8> {
    sign_domain_for(
        &key(byte),
        subject,
        &ChainIdentity::new("777", "testnet"),
        SigningProtocolVersion::V2,
    )
    .as_bytes()
    .to_vec()
}
pub fn auth(
    owner: u8,
    id: u8,
    nonce: u64,
    action: NodeBondAction,
    epoch: u64,
) -> NodeBondLifecycle {
    let a = NodeBondAuthorization {
        genesis_hash: [7; 32],
        deployment_id: [id; 32],
        owner_account: address(owner),
        escrow_account: node_bond_escrow_address(&[id; 32]),
        payout_account: address(owner),
        server_key: pk(owner + 1),
        consumer_key: pk(owner + 2),
        service_alias: format!("node-{id}"),
        action,
        amount_units: if action == NodeBondAction::Deposit || action == NodeBondAction::Withdraw {
            NODE_BOND_UNITS
        } else {
            0
        },
        expected_nonce: nonce,
        effective_epoch: epoch,
    };
    let online = matches!(action, NodeBondAction::Deposit | NodeBondAction::Update);
    let digest = node_contract_digest(&a, &ChainIdentity::new("777", "testnet"));
    let signed = SignedNodeBondAuthorization {
        owner_signature: signature(&a, owner),
        server_signature: if online {
            signature(
                &NodeBondPossession {
                    authorization_preimage_hash: digest,
                    role: NodeBondKeyRole::Server,
                },
                owner + 1,
            )
        } else {
            vec![]
        },
        consumer_signature: if online {
            signature(
                &NodeBondPossession {
                    authorization_preimage_hash: digest,
                    role: NodeBondKeyRole::Consumer,
                },
                owner + 2,
            )
        } else {
            vec![]
        },
        authorization: a.clone(),
    };
    NodeBondLifecycle {
        signed,
        owner_key: pk(owner),
        governance_key: online.then(|| pk(99)),
        governance_signature: if online { signature(&a, 99) } else { vec![] },
        payout_key: online.then(|| pk(owner)),
        payout_signature: if online { signature(&a, owner) } else { vec![] },
    }
}
pub fn state() -> State {
    state_on([7; 32], [2; 32])
}
pub fn state_on(genesis: [u8; 32], parent: [u8; 32]) -> State {
    // Identical synthetic schedule for every fixture, including parallel tests.
    use wrfcoin_shared_protocol::crypto::chain_binding::{
        FeatureSchedule, install_feature_schedule,
    };
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("node_share_settlement".into(), A),
            ("account_authorization_dispatch".into(), A),
        ])
        .unwrap(),
    );
    install_schedule_extension_authority_for_tests(
        "777",
        "testnet",
        &hex::encode(genesis),
        vec![hex::encode(pk(99))],
    );
    let mut s = State::new();
    s.genesis_emission_config = Some(wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig { chain_id:"777".into(), faucet_address:None , ..Default::default() });
    s.feature_schedule_extensions.push(AppliedExtension {
        sequence: 0,
        carried_at_height: 0,
        entries: vec![ScheduleEntry::new("node_share_settlement", A)],
        version_steps: vec![],
        min_protocol_version: 2,
        release_commit: "test-only-explicit-schedule".into(),
        signers: vec![hex::encode(pk(99))],
    });
    s.create_account(address(10), Balance::new(u128::from(NODE_BOND_UNITS) * 3));
    s.create_account(address(20), Balance::new(u128::from(NODE_BOND_UNITS) * 3));
    s.apply_transition(StateTransition::BeginNodeServiceBlock {
        block_height: A,
        parent_hash: parent,
    })
    .unwrap();
    s.apply_block_finalization(A);
    s
}
pub fn apply(
    s: &mut State,
    p: NodeBondLifecycle,
    height: u64,
) -> Result<(), wrfcoin_shared_protocol::blockchain::state::StateError> {
    s.apply_transition(StateTransition::ApplyNodeBondLifecycle {
        payload: p,
        block_height: height,
    })
}
pub fn advance(s: &mut State, height: u64) {
    let from = s.node_bond_registry.as_ref().unwrap().block_height;
    for h in from + 1..=height {
        s.apply_transition(StateTransition::BeginNodeServiceBlock {
            block_height: h,
            parent_hash: [(h % 251) as u8; 32],
        })
        .unwrap();
        s.apply_block_finalization(h);
    }
}
pub fn resign(p: &mut NodeBondLifecycle, owner: u8) {
    let a = &p.signed.authorization;
    let digest = node_contract_digest(a, &ChainIdentity::new("777", "testnet"));
    p.signed.owner_signature = signature(a, owner);
    p.governance_signature = signature(a, 99);
    p.payout_signature = signature(a, owner);
    p.signed.server_signature = signature(
        &NodeBondPossession {
            authorization_preimage_hash: digest,
            role: NodeBondKeyRole::Server,
        },
        owner + 1,
    );
    p.signed.consumer_signature = signature(
        &NodeBondPossession {
            authorization_preimage_hash: digest,
            role: NodeBondKeyRole::Consumer,
        },
        owner + 2,
    );
}
pub fn auth_on(
    owner: u8,
    id: u8,
    nonce: u64,
    action: NodeBondAction,
    epoch: u64,
    genesis: [u8; 32],
) -> NodeBondLifecycle {
    let mut p = auth(owner, id, nonce, action, epoch);
    p.signed.authorization.genesis_hash = genesis;
    resign(&mut p, owner);
    p
}
pub fn transaction(
    p: &NodeBondLifecycle,
    fee: u64,
) -> wrfcoin_shared_protocol::blockchain::transaction::Transaction {
    use wrfcoin_shared_protocol::blockchain::transaction::{
        Transaction, TransactionInput, TransactionOutput, TransactionType,
    };
    let a = &p.signed.authorization;
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(b"synthetic-node-input"),
            vout: 0,
            signature: None,
            public_key: Some(hex::encode(p.owner_key)),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            address: a.owner_account.to_string(),
            amount: 0,
            script: "OP_TRUE".into(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::NodeBondLifecycle;
    tx.sender_address = a.owner_account;
    tx.nonce = a.expected_nonce;
    tx.fee = fee;
    tx.data = Some(serde_json::to_string(p).unwrap());
    tx.set_id();
    tx
}
