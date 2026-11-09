//! Keyless bond Update assembly (new payout account), explicit role approvals
//! and one-shot owner submission.
//!
//! Every refusal here mirrors a node rule, so a packet this command signs is
//! one the node admits and applies:
//! - `shared-protocol/src/consensus/node_bond_registry.rs` `apply_lifecycle`,
//!   `NodeBondAction::Update` arm: the bond is registered, owner and escrow
//!   are unchanged, `amount_units == 0`, `effective_epoch == epoch + 1`, the
//!   bond did not already change this epoch and is not exiting; the owner
//!   nonce matches; the payout is no bond's escrow, no other bond's payout or
//!   owner; the deployment is not jailed.
//! - `NodeBondLifecycle::verify_with_gates`: owner, server, consumer and
//!   governance (or pinned admission) signatures over this authorization, and
//!   exactly one payout approval (ed25519 key, or a passkey assertion once
//!   `passkey_bond_payout` is active).
//! - `SignedNodeBondAuthorization::verify_contract` (node_share_contracts.rs):
//!   genesis, alias bounds and role-key separation.
use anyhow::{Result, ensure};
use clap::Args;
use serde_json::{Value, json};
use std::path::PathBuf;
use wrfcoin_cli::node_operator::{OperatorKey, OperatorRole};
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, TransactionInput, TransactionOutput, TransactionType},
    common::types::Address,
    consensus::{
        node_bond_registry::{NodeBondLifecycle, NodeBondRegistration, node_bond_escrow_address},
        node_share_contracts::*,
    },
    crypto::{chain_binding::ChainIdentity, hash::hash_bytes, signature::PublicKey},
    governance::schedule_extension::bond_admission_keys,
};

use super::node_deposit::{
    NodeEpoch, check_network, id, node_epoch, normalized, number, operator_api,
};

#[derive(Args)]
pub struct UpdateArgs {
    /// The registered bond to update.
    #[clap(long)]
    pub deployment_id: String,
    /// Ed25519 public key of the bond owner; must own the registered bond.
    #[clap(long)]
    pub owner_key: String,
    /// The new payout account. Server key, consumer key, service alias, owner
    /// and escrow are taken unchanged from the bond the node reports.
    #[clap(long)]
    pub payout_address: String,
    /// Ed25519 public key of the new payout account. Omit it when the payout
    /// account is a passkey wallet: it then approves with a WebAuthn assertion
    /// (`node bond attach-payout-passkey`, needs `passkey_bond_payout`).
    #[clap(long)]
    pub payout_key: Option<String>,
    /// JSON file holding the passkey payout account's assertion over this
    /// Update's challenge. Verified locally before the owner signs.
    #[clap(long, conflicts_with = "payout_key")]
    pub payout_passkey_assertion: Option<PathBuf>,
    /// Public key of the co-signer: a genesis governance key, or with
    /// `--admission-cosign` a pinned bond admission key.
    #[clap(long)]
    pub governance_key: String,
    /// The co-signer is a bond admission key: refuse unless `--governance-key`
    /// is pinned for `--genesis-hash` in this binary.
    #[clap(long)]
    pub admission_cosign: bool,
    #[clap(long)]
    pub genesis_hash: String,
    #[clap(long)]
    pub chain_id: String,
    #[clap(long, default_value = "testnet")]
    pub network_id: String,
    /// Approved activation height; must match the canonical bond registry.
    #[clap(long)]
    pub activation_height: u64,
    #[clap(long)]
    pub api_url: String,
    #[clap(long)]
    pub operator_token_file: PathBuf,
    /// Explicit software owner wallet JSON or legacy raw hex seed file.
    #[clap(long)]
    pub key_file: Option<PathBuf>,
    /// Same unsigned packet with server, consumer, governance and payout approvals.
    #[clap(long)]
    pub approvals_file: Option<PathBuf>,
    #[clap(long, default_value = "1000000")]
    pub fee: u64,
    /// New public packet; signed and fsynced before a submission attempt.
    #[clap(long)]
    pub out: Option<PathBuf>,
    /// Assemble and print the public transaction; no private key is loaded.
    #[clap(long)]
    pub dry_run: bool,
}

/// Input checks that need no node: identifiers, payout key binding, co-signer.
fn static_checks(args: &UpdateArgs) -> Result<()> {
    let owner_key = id(&args.owner_key)?;
    let owner = Address::from_public_key(&PublicKey::from_bytes(&owner_key)?);
    let payout: Address = args.payout_address.parse()?;
    ensure!(payout != owner, "payout must differ from hot bond owner");
    let deployment = id(&args.deployment_id)?;
    ensure!(
        payout != node_bond_escrow_address(&deployment),
        "escrow role collision refused"
    );
    if let Some(payout_key) = args.payout_key.as_deref().map(id).transpose()? {
        ensure!(
            Address::from_public_key(&PublicKey::from_bytes(&payout_key)?) == payout,
            "payout public key/address mismatch"
        );
    }
    let governance_key = id(&args.governance_key)?;
    if args.admission_cosign {
        ensure!(
            bond_admission_keys(&hex::encode(id(&args.genesis_hash)?))
                .contains(&hex::encode(governance_key)),
            "--governance-key is not a bond admission key pinned for this genesis"
        );
    }
    ensure!(
        governance_key != owner_key,
        "governance key must differ from hot bond owner"
    );
    Ok(())
}

/// The registered bond as the node reports it, refused where the node's
/// Update arm would refuse (`apply_lifecycle`).
fn registered_bond(
    view: &Value,
    deployment: &[u8; 32],
    epoch: &NodeEpoch,
) -> Result<NodeBondRegistration> {
    ensure!(
        view["status"] == "REGISTERED" && !view["registration"].is_null(),
        "deployment has no registered bond; use `node bond deposit` to register one"
    );
    ensure!(
        view["jailed"] != true,
        "bond is jailed; the node refuses its lifecycle"
    );
    let r: NodeBondRegistration = serde_json::from_value(view["registration"].clone())
        .map_err(|_| anyhow::anyhow!("bond registration format refused"))?;
    ensure!(
        r.binding.deployment_id == *deployment
            && r.binding.escrow_account == node_bond_escrow_address(deployment),
        "bond registration does not match the deployment"
    );
    ensure!(
        r.exit_effective_height.is_none(),
        "bond is exiting; an Update is refused"
    );
    ensure!(
        r.last_change_epoch != epoch.epoch,
        "bond already changed in epoch {}; wait for the next epoch and reassemble",
        epoch.epoch
    );
    Ok(r)
}

/// Refuse a payout another bond already uses as payout, owner or escrow.
fn payout_free_of_other_bonds(list: &Value, deployment: &[u8; 32], payout: &Address) -> Result<()> {
    let bonds = list["bonds"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("bond list format refused"))?;
    ensure!(
        list["next_after"].is_null(),
        "bond list is paged beyond one read; refusing to guess"
    );
    for bond in bonds {
        let other = bond["deployment_id"]
            .as_str()
            .and_then(|d| id(d).ok())
            .ok_or_else(|| anyhow::anyhow!("bond list format refused"))?;
        if other == *deployment {
            continue;
        }
        let used = ["payout_account", "owner_account"]
            .iter()
            .filter_map(|f| bond[*f].as_str()?.parse::<Address>().ok())
            .chain(std::iter::once(node_bond_escrow_address(&other)))
            .any(|a| a == *payout);
        ensure!(!used, "payout account is already used by another bond");
    }
    Ok(())
}

pub(crate) fn unsigned(
    args: &UpdateArgs,
    current: &NodeBondAuthorization,
    nonce: u64,
    effective_epoch: u64,
) -> Result<Transaction> {
    static_checks(args)?;
    let owner_key = id(&args.owner_key)?;
    let owner = Address::from_public_key(&PublicKey::from_bytes(&owner_key)?);
    ensure!(
        owner == current.owner_account,
        "owner key does not own this bond"
    );
    ensure!(
        current.genesis_hash == id(&args.genesis_hash)?,
        "bond genesis mismatch"
    );
    let payout: Address = args.payout_address.parse()?;
    ensure!(
        payout != current.payout_account,
        "payout address unchanged; nothing to update"
    );
    let payout_key = args.payout_key.as_deref().map(id).transpose()?;
    ensure!(
        payout_key != Some(current.server_key) && payout_key != Some(current.consumer_key),
        "payout key must differ from dedicated node role keys"
    );
    let authorization = NodeBondAuthorization {
        payout_account: payout,
        action: NodeBondAction::Update,
        amount_units: 0,
        expected_nonce: nonce,
        effective_epoch,
        ..current.clone()
    };
    let p = NodeBondLifecycle {
        signed: SignedNodeBondAuthorization {
            authorization,
            owner_signature: vec![],
            server_signature: vec![],
            consumer_signature: vec![],
        },
        owner_key,
        governance_key: Some(id(&args.governance_key)?),
        governance_signature: vec![],
        payout_key,
        payout_signature: vec![],
        payout_passkey: None,
    };
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_bytes(b"wrfcoin.node_bond_update.input.v1"),
            vout: 0,
            signature: None,
            public_key: Some(hex::encode(owner_key)),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            address: owner.to_string(),
            amount: 0,
            script: "OP_TRUE".into(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::NodeBondLifecycle;
    tx.sender_address = owner;
    tx.nonce = nonce;
    tx.fee = args.fee;
    tx.data = Some(serde_json::to_string(&p)?);
    tx.set_id();
    ensure!(
        serde_json::to_vec(&tx)?.len() <= 40 * 1024,
        "Update packet exceeds cap"
    );
    Ok(tx)
}

pub async fn handle(args: &UpdateArgs) -> Result<Value> {
    // Cheap public input checks precede any key or credential/network operation.
    static_checks(args)?;
    let owner = Address::from_public_key(&PublicKey::from_bytes(&id(&args.owner_key)?)?);
    let deployment = id(&args.deployment_id)?;
    let payout: Address = args.payout_address.parse()?;
    let out = args.out.as_ref();
    if let Some(out) = out {
        ensure!(
            !out.exists(),
            "packet already exists; inspect canonical inclusion before explicit retry"
        );
    }
    let api = operator_api(&args.api_url, &args.operator_token_file)?;
    check_network(&api, &args.chain_id, &args.network_id, &args.genesis_hash).await?;
    let path = format!("/node/bonds/{}", hex::encode(deployment));
    let view = api.receipt_request("GET", &path, None).await?;
    let epoch = node_epoch(&view, args.activation_height)?;
    let current = registered_bond(&view, &deployment, &epoch)?;
    ensure!(
        owner == current.binding.owner_account,
        "owner key does not own this bond"
    );
    let list = api
        .receipt_request("GET", "/node/bonds?limit=50", None)
        .await?;
    payout_free_of_other_bonds(&list, &deployment, &payout)?;
    let account = api
        .receipt_request("GET", &format!("/addresses/{owner}"), None)
        .await?;
    let nonce = number(&account["nonce"])?;
    let expected = unsigned(args, &current.binding, nonce, epoch.effective_epoch()?)?;
    let payload = NodeBondLifecycle::parse(&expected)
        .ok_or_else(|| anyhow::anyhow!("Update packet refused"))?;
    let identity = ChainIdentity::new(&args.chain_id, &args.network_id);
    if args.dry_run {
        if let Some(out) = out {
            super::node_receipt::durable_packet(out, &expected)?;
        }
        let mut plan = json!({"dry_run":true,"transaction":expected,"tx_id":expected.id.map(|id|id.to_hex()),
            "action":"Update","current_payout_account":current.binding.payout_account.to_string(),
            "new_payout_account":payout.to_string(),"expected_nonce":nonce,
            "validated_for_carrying_height":epoch.height.saturating_add(1),
            "effective_epoch":payload.signed.authorization.effective_epoch,"node_height":epoch.height,
            "node_epoch":epoch.epoch,"node_epoch_blocks":epoch.epoch_blocks,"epoch_close_height":epoch.close_height,
            "blocks_left_in_epoch":epoch.blocks_left(),"submitted":false,"included":false});
        if payload.payout_key.is_none() {
            plan["payout_passkey_challenge"] =
                super::node_bond::payout_challenge_json(&payload.signed.authorization, &identity);
        }
        return Ok(plan);
    }
    let out = out.ok_or_else(|| anyhow::anyhow!("--out required for durable Update"))?;
    let approvals = args.approvals_file.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "--approvals-file required: obtain server, consumer, governance and payout approvals over the dry-run packet"
        )
    })?;
    let bytes = super::node_receipt::bounded_read(approvals, 40 * 1024)?;
    let mut tx: Transaction = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("public approval packet refused"))?;
    let mut p = NodeBondLifecycle::parse(&tx)
        .ok_or_else(|| anyhow::anyhow!("public approval packet refused"))?;
    ensure!(
        p.signed.authorization == payload.signed.authorization
            && p.owner_key == payload.owner_key
            && p.governance_key == payload.governance_key
            && p.payout_key == payload.payout_key
            && normalized(&tx)? == normalized(&expected)?,
        "approvals do not match current nonce, epoch or Update plan; reassemble and reapprove"
    );
    // Refuse a changed epoch, bond or nonce before saving or POSTing a signature.
    let fresh = api.receipt_request("GET", &path, None).await?;
    let fresh_epoch = node_epoch(&fresh, args.activation_height)?;
    ensure!(
        fresh_epoch.effective_epoch()? == p.signed.authorization.effective_epoch
            && registered_bond(&fresh, &deployment, &fresh_epoch)? == current,
        "inclusion epoch or bond changed; reassemble and obtain new role approvals"
    );
    let fresh_account = api
        .receipt_request("GET", &format!("/addresses/{owner}"), None)
        .await?;
    ensure!(
        number(&fresh_account["nonce"])? == nonce,
        "owner nonce changed; reassemble and reapprove"
    );
    let key = OperatorKey::load(
        args.key_file
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--key-file required for Update owner"))?,
    )?;
    p.signed.owner_signature = key
        .sign_lifecycle(OperatorRole::Owner, &p.signed.authorization, &identity)?
        .as_bytes()
        .to_vec();
    if let Some(path) = &args.payout_passkey_assertion {
        p.payout_passkey = Some(super::node_bond::read_payout_passkey(
            path,
            &p.signed.authorization,
            &identity,
        )?);
    }
    ensure!(
        p.payout_key.is_some() || p.payout_passkey.is_some(),
        "passkey payout approval required: pass --payout-passkey-assertion or include payout_passkey in the approvals packet"
    );
    // Offline check as the chain will verify it (verify_with_gates); a passkey
    // approval is only accepted by nodes once passkey_bond_payout is active.
    let cosigner = [args.governance_key.to_ascii_lowercase()];
    let (governance_keys, admission_keys): (&[String], &[String]) = if args.admission_cosign {
        (&[], &cosigner)
    } else {
        (&cosigner, &[])
    };
    p.verify_with_gates(
        &identity,
        &id(&args.genesis_hash)?,
        governance_keys,
        admission_keys,
        p.payout_passkey.is_some(),
    )
    .map_err(|_| {
        anyhow::anyhow!("Update approvals do not verify (missing or wrong role signature)")
    })?;
    tx.data = Some(serde_json::to_string(&p)?);
    // Approvals bind the authorization, not the outer timestamp: use the fresh
    // plan's so an off-host approval delay cannot age the transaction.
    tx.timestamp = expected.timestamp;
    tx.set_id();
    ensure!(
        serde_json::to_vec(&tx)?.len() <= 40 * 1024,
        "Update packet exceeds cap"
    );
    super::node_receipt::durable_packet(out, &tx)?;
    let ack = api
        .receipt_request(
            "POST",
            "/node/bonds/submit",
            Some(serde_json::to_value(&tx)?),
        )
        .await;
    let admitted = ack.as_ref().is_ok_and(|a| {
        a["admitted"] == true && a["tx_id"].as_str() == tx.id.map(|h| h.to_hex()).as_deref()
    });
    let view = api
        .receipt_request("GET", &path, None)
        .await
        .unwrap_or(Value::Null);
    let pending =
        view["registration"]["pending_binding"] == serde_json::to_value(&p.signed.authorization)?;
    if !admitted {
        anyhow::bail!(
            "UNCERTAIN; pending_update_observed={pending}; retain packet and inspect nonce/inclusion before explicit retry"
        );
    }
    Ok(
        json!({"tx_id":tx.id.map(|h|h.to_hex()),"admitted":true,"included":false,
        "pending_update_observed":pending,"effective_epoch":p.signed.authorization.effective_epoch,
        "packet":out.display().to_string()}),
    )
}

#[cfg(test)]
mod tests {
    use super::super::node_bond::{
        BondCommands, BondSignerRole, fixture, sign_packet, submittable,
    };
    use super::*;
    use std::sync::{Arc, Mutex};
    use wrfcoin_shared_protocol::{
        accounts::passkey::test_support::SoftwareAuthenticator, blockchain::state::State,
        crypto::signer::EnvKeySigner,
    };

    const DEPLOYMENT: [u8; 32] = [1; 32];
    const HEIGHT: u64 = 1_100;

    fn identity() -> ChainIdentity {
        ChainIdentity::new("777", "testnet")
    }
    fn run<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Runtime::new().unwrap().block_on(f)
    }
    /// A canonical state holding the fixture bond (owner 10, deployment 1,
    /// registered in epoch 0), advanced to `HEIGHT` (epoch 1 at 1024 blocks).
    fn chain() -> State {
        let mut s = fixture::state();
        fixture::apply(
            &mut s,
            fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1),
            fixture::A,
        )
        .unwrap();
        fixture::advance(&mut s, HEIGHT);
        s
    }
    /// `GET /node/bonds/{id}` exactly as routes/node_bonds.rs `inspect` serves it.
    fn view(s: &State) -> Value {
        let r = &s.node_bond_registry.as_ref().unwrap().registrations[&DEPLOYMENT];
        json!({"height":HEIGHT.to_string(),"activation_height":fixture::A.to_string(),"epoch":"1",
            "deployment_id":hex::encode(DEPLOYMENT),"status":"REGISTERED","principal_base_units":"1",
            "jailed":false,"registration":r})
    }
    fn list(others: Vec<Value>) -> Value {
        json!({"height":HEIGHT.to_string(),"bonds":others,"next_after":null})
    }
    fn dir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("wrfcoin-bond-update-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn secret(path: &std::path::Path, text: &str) -> PathBuf {
        std::fs::write(path, text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        path.to_path_buf()
    }
    /// A synthetic test seed file (fixture keys are `[byte; 32]`).
    fn seed(dir: &std::path::Path, byte: u8) -> PathBuf {
        secret(&dir.join(format!("seed-{byte}")), &hex::encode([byte; 32]))
    }
    struct Node {
        server: mockito::ServerGuard,
        submitted: Arc<Mutex<Option<Transaction>>>,
        _mocks: Vec<mockito::Mock>,
    }
    fn node(view: &Value, nonce: u64, list: &Value) -> Node {
        let mut server = mockito::Server::new();
        let envelope = |data: Value| {
            json!({"ok":true,"data":data,"error":null,"meta":{"request_id":"r"}}).to_string()
        };
        let submitted = Arc::new(Mutex::new(None));
        let seen = submitted.clone();
        let owner = fixture::address(10);
        let mocks = vec![
            server
                .mock("GET", "/network/info")
                .with_body(json!({"network_id":"testnet","chain_id":"777","genesis_hash":format!("0x{}", hex::encode([7u8; 32]))}).to_string())
                .create(),
            server
                .mock("GET", format!("/node/bonds/{}", hex::encode(DEPLOYMENT)).as_str())
                .with_body(view.to_string())
                .create(),
            server
                .mock("GET", "/node/bonds?limit=50")
                .with_body(list.to_string())
                .create(),
            server
                .mock("GET", format!("/addresses/{owner}").as_str())
                .with_body(envelope(json!({"address":owner.to_string(),"balance":"1","nonce":nonce.to_string()})))
                .create(),
            // routes/node_bonds.rs `submit`: 202 {"tx_id","admitted":true,"included":false}.
            server
                .mock("POST", "/node/bonds/submit")
                .with_status(202)
                .with_body_from_request(move |request| {
                    let tx: Transaction = serde_json::from_slice(request.body().unwrap()).unwrap();
                    let id = tx.id.map(|h| h.to_hex());
                    *seen.lock().unwrap() = Some(tx);
                    json!({"tx_id":id,"admitted":true,"included":false}).to_string().into_bytes()
                })
                .create(),
        ];
        Node {
            server,
            submitted,
            _mocks: mocks,
        }
    }
    fn args(
        node: &Node,
        dir: &std::path::Path,
        payout: Address,
        payout_key: Option<[u8; 32]>,
    ) -> UpdateArgs {
        UpdateArgs {
            deployment_id: hex::encode(DEPLOYMENT),
            owner_key: hex::encode(fixture::pk(10)),
            payout_address: payout.to_string(),
            payout_key: payout_key.map(hex::encode),
            payout_passkey_assertion: None,
            governance_key: hex::encode(fixture::pk(99)),
            admission_cosign: false,
            genesis_hash: hex::encode([7u8; 32]),
            chain_id: "777".into(),
            network_id: "testnet".into(),
            activation_height: fixture::A,
            api_url: node.server.url(),
            operator_token_file: secret(&dir.join("token"), "synthetic-update-cli-bearer"), // secret-leak-ok: fixed synthetic bearer for a mock server only.
            key_file: None,
            approvals_file: None,
            fee: 1_000_000,
            out: None,
            dry_run: true,
        }
    }
    /// Server and consumer sign through `node bond sign` with key files, the
    /// governance key through the HSM-role signer.
    fn approve_roles(dir: &std::path::Path, unsigned: &std::path::Path) -> PathBuf {
        let mut current = unsigned.to_path_buf();
        for (role, byte, name) in [
            (BondSignerRole::Server, 11, "server.json"),
            (BondSignerRole::Consumer, 12, "consumer.json"),
        ] {
            let out = dir.join(name);
            run(super::super::node_bond::handle(&BondCommands::Sign {
                transaction_file: current.clone(),
                key_file: Some(seed(dir, byte)),
                role,
                chain_id: "777".into(),
                network_id: "testnet".into(),
                out: out.clone(),
            }))
            .unwrap();
            current = out;
        }
        let mut tx: Transaction =
            serde_json::from_slice(&std::fs::read(&current).unwrap()).unwrap();
        let governance = EnvKeySigner::from_private_key(fixture::key(99), "synthetic-governance");
        sign_packet(
            &mut tx,
            BondSignerRole::Governance,
            &identity(),
            &governance,
        )
        .unwrap();
        let out = dir.join("governance.json");
        std::fs::write(&out, serde_json::to_vec(&tx).unwrap()).unwrap();
        out
    }

    /// Build an Update from the canonical fixture bond, approve it through the
    /// CLI's own sign path, submit it once, and apply the submitted packet to
    /// the real state transition (`apply_lifecycle`): the node accepts it and
    /// records the new payout as the pending binding.
    #[test]
    fn update_round_trips_through_sign_submit_and_the_node_apply() {
        let mut s = chain();
        let nonce = s.accounts[&fixture::address(10)].nonce;
        let dir = dir("ed25519");
        let node = node(&view(&s), nonce, &list(vec![]));
        let payout = fixture::address(30);
        let mut a = args(&node, &dir, payout, Some(fixture::pk(30)));
        a.out = Some(dir.join("unsigned.json"));
        let plan = run(handle(&a)).unwrap();
        assert_eq!(plan["action"], "Update");
        assert_eq!(plan["effective_epoch"], 2);
        assert_eq!(
            plan["current_payout_account"],
            fixture::address(10).to_string()
        );
        assert!(plan.get("payout_passkey_challenge").is_none());
        let unsigned: Transaction =
            serde_json::from_slice(&std::fs::read(dir.join("unsigned.json")).unwrap()).unwrap();
        let built = NodeBondLifecycle::parse(&unsigned)
            .unwrap()
            .signed
            .authorization;
        let current = &s.node_bond_registry.as_ref().unwrap().registrations[&DEPLOYMENT].binding;
        assert_eq!(
            built,
            NodeBondAuthorization {
                payout_account: payout,
                action: NodeBondAction::Update,
                amount_units: 0,
                expected_nonce: nonce,
                effective_epoch: 2,
                ..current.clone()
            }
        );

        let governance = approve_roles(&dir, &dir.join("unsigned.json"));
        let mut tx: Transaction =
            serde_json::from_slice(&std::fs::read(&governance).unwrap()).unwrap();
        let payout_signer = EnvKeySigner::from_private_key(fixture::key(30), "synthetic-payout");
        sign_packet(&mut tx, BondSignerRole::Payout, &identity(), &payout_signer).unwrap();
        let approvals = dir.join("approvals.json");
        std::fs::write(&approvals, serde_json::to_vec(&tx).unwrap()).unwrap();

        a.dry_run = false;
        a.approvals_file = Some(approvals);
        a.key_file = Some(seed(&dir, 10));
        a.out = Some(dir.join("signed.json"));
        let done = run(handle(&a)).unwrap();
        assert_eq!(done["admitted"], true);
        let sent = node.submitted.lock().unwrap().clone().unwrap();
        assert_eq!(done["tx_id"], json!(sent.id.map(|h| h.to_hex())));
        assert_eq!(
            serde_json::to_vec(&sent).unwrap(),
            std::fs::read(dir.join("signed.json"))
                .map(|b| {
                    serde_json::to_vec(&serde_json::from_slice::<Transaction>(&b).unwrap()).unwrap()
                })
                .unwrap()
        );
        // The node's admission check (mempool pool.rs) and block apply.
        let payload = NodeBondLifecycle::parse(&sent).unwrap();
        payload
            .verify(
                &identity(),
                &[7; 32],
                &[hex::encode(fixture::pk(99))],
                HEIGHT + 1,
            )
            .unwrap();
        fixture::advance(&mut s, HEIGHT + 1);
        fixture::apply(&mut s, payload, HEIGHT + 1).unwrap();
        let r = &s.node_bond_registry.as_ref().unwrap().registrations[&DEPLOYMENT];
        assert_eq!(r.pending_binding.as_ref().unwrap().payout_account, payout);
        assert_eq!(r.last_change_epoch, 1);
        assert_eq!(s.accounts[&fixture::address(10)].nonce, nonce + 1);
        // `submit-update` takes the signed Update; the other submit commands
        // refuse it, and `submit-update` refuses a Deposit. Offline only.
        let signed = dir.join("signed.json");
        assert_eq!(
            submittable(
                &BondCommands::SubmitUpdate {
                    transaction_file: signed.clone()
                },
                &signed
            )
            .unwrap()
            .id,
            sent.id
        );
        for other in [
            BondCommands::RegisterBond {
                transaction_file: signed.clone(),
            },
            BondCommands::Exit {
                transaction_file: signed.clone(),
            },
            BondCommands::Withdraw {
                transaction_file: signed.clone(),
            },
        ] {
            let error = submittable(&other, &signed).unwrap_err();
            assert!(
                error.to_string().contains("lifecycle action mismatch"),
                "{error}"
            );
        }
        let deposit = dir.join("deposit.json");
        std::fs::write(
            &deposit,
            serde_json::to_vec(&fixture::transaction(
                &fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1),
                1_000_000,
            ))
            .unwrap(),
        )
        .unwrap();
        let error = submittable(
            &BondCommands::SubmitUpdate {
                transaction_file: deposit.clone(),
            },
            &deposit,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("lifecycle action mismatch"),
            "{error}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The owner's case: a passkey wallet payout. The dry run prints the
    /// Update's challenge, `attach-payout-passkey` verifies and attaches the
    /// wallet's assertion, and the submitted packet verifies as the node
    /// verifies it once `passkey_bond_payout` is active (and not before).
    #[test]
    fn passkey_payout_update_round_trips_through_attach() {
        let s = chain();
        let nonce = s.accounts[&fixture::address(10)].nonce;
        let dir = dir("passkey");
        let wallet = SoftwareAuthenticator::new(&[0x3c; 32], "wrfcoin.com");
        let node = node(&view(&s), nonce, &list(vec![]));
        let mut a = args(&node, &dir, wallet.account(), None);
        a.out = Some(dir.join("unsigned.json"));
        let plan = run(handle(&a)).unwrap();
        let challenge = &plan["payout_passkey_challenge"];
        assert_eq!(challenge["action"], "Update");
        assert_eq!(challenge["payout_account"], wallet.account().to_hex());
        let governance = approve_roles(&dir, &dir.join("unsigned.json"));

        // Approvals without the passkey assertion are refused before any POST.
        a.dry_run = false;
        a.approvals_file = Some(governance.clone());
        a.key_file = Some(seed(&dir, 10));
        a.out = Some(dir.join("refused.json"));
        let error = run(handle(&a)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("passkey payout approval required"),
            "{error}"
        );
        assert!(!dir.join("refused.json").exists());
        assert!(node.submitted.lock().unwrap().is_none());

        let tx: Transaction = serde_json::from_slice(&std::fs::read(&governance).unwrap()).unwrap();
        let bytes = node_bond_payout_passkey_challenge(
            &NodeBondLifecycle::parse(&tx).unwrap().signed.authorization,
            &identity(),
        );
        assert_eq!(challenge["challenge_hex"], hex::encode(bytes));
        let assertion = dir.join("approved.json");
        std::fs::write(
            &assertion,
            serde_json::to_vec(
                &json!({"payout_passkey": wallet.authorization_for(&wallet.assert(&bytes))}),
            )
            .unwrap(),
        )
        .unwrap();
        let attached = dir.join("update-approvals.json");
        run(super::super::node_bond::handle(
            &BondCommands::AttachPayoutPasskey {
                transaction_file: governance,
                payout_passkey_assertion: assertion,
                chain_id: "777".into(),
                network_id: "testnet".into(),
                out: attached.clone(),
            },
        ))
        .unwrap();
        a.approvals_file = Some(attached);
        a.out = Some(dir.join("signed.json"));
        assert_eq!(run(handle(&a)).unwrap()["admitted"], true);
        let sent = node.submitted.lock().unwrap().clone().unwrap();
        let payload = NodeBondLifecycle::parse(&sent).unwrap();
        assert!(payload.payout_key.is_none() && payload.payout_passkey.is_some());
        let keys = [hex::encode(fixture::pk(99))];
        payload
            .verify_with_gates(&identity(), &[7; 32], &keys, &[], true)
            .unwrap();
        assert!(
            payload
                .verify_with_gates(&identity(), &[7; 32], &keys, &[], false)
                .is_err()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Every refusal the node's Update arm would make is made before a key is
    /// read or anything is posted.
    #[test]
    fn update_refusals_mirror_the_node() {
        let s = chain();
        let nonce = s.accounts[&fixture::address(10)].nonce;
        let base = view(&s);
        let payout = fixture::address(30);
        let refused = |view: &Value, list: &Value, edit: &dyn Fn(&mut UpdateArgs), want: &str| {
            let dir = dir("refused");
            let node = node(view, nonce, list);
            let mut a = args(&node, &dir, payout, Some(fixture::pk(30)));
            edit(&mut a);
            let error = run(handle(&a)).unwrap_err().to_string();
            assert!(error.contains(want), "want {want:?}, got {error:?}");
            assert!(node.submitted.lock().unwrap().is_none());
            std::fs::remove_dir_all(dir).unwrap();
        };
        let none = list(vec![]);
        let with = |f: &dyn Fn(&mut Value)| {
            let mut v = base.clone();
            f(&mut v);
            v
        };
        // Wrong owner: owner 20 does not own deployment 1.
        refused(
            &base,
            &none,
            &|a| a.owner_key = hex::encode(fixture::pk(20)),
            "owner key does not own this bond",
        );
        // No registered bond.
        refused(
            &with(&|v| {
                v["status"] = json!("UNKNOWN");
                v["registration"] = Value::Null;
            }),
            &none,
            &|_| {},
            "no registered bond",
        );
        // Already changed this epoch (apply: last_change_epoch == epoch).
        refused(
            &with(&|v| v["registration"]["last_change_epoch"] = json!(1)),
            &none,
            &|_| {},
            "already changed in epoch 1",
        );
        // Exiting.
        refused(
            &with(&|v| v["registration"]["exit_effective_height"] = json!(2_051)),
            &none,
            &|_| {},
            "exiting",
        );
        // Jailed.
        refused(
            &with(&|v| v["jailed"] = json!(true)),
            &none,
            &|_| {},
            "jailed",
        );
        // Payout unchanged.
        refused(
            &with(&|v| {
                v["registration"]["binding"]["payout_account"] =
                    serde_json::to_value(payout).unwrap()
            }),
            &none,
            &|_| {},
            "payout address unchanged",
        );
        // Payout already another bond's payout.
        let other = list(vec![
            json!({"deployment_id":hex::encode([2u8; 32]),"payout_account":payout.to_string(),"owner_account":fixture::address(20).to_string()}),
        ]);
        refused(&base, &other, &|_| {}, "already used by another bond");
        // Payout is this bond's escrow, payout equals the owner, key/address mismatch.
        refused(
            &base,
            &none,
            &|a| {
                a.payout_address = node_bond_escrow_address(&DEPLOYMENT).to_string();
                a.payout_key = None;
            },
            "escrow role collision",
        );
        refused(
            &base,
            &none,
            &|a| {
                a.payout_address = fixture::address(10).to_string();
                a.payout_key = None;
            },
            "payout must differ from hot bond owner",
        );
        refused(
            &base,
            &none,
            &|a| a.payout_key = Some(hex::encode(fixture::pk(31))),
            "payout public key/address mismatch",
        );
        // Bond on another chain.
        refused(
            &base,
            &none,
            &|a| a.genesis_hash = hex::encode([8u8; 32]),
            "API chain/genesis mismatch",
        );

        // Stale nonce: approvals made at nonce N are refused once the owner's
        // account nonce moved on, before the owner key is read.
        let dir = dir("stale");
        let first = node(&base, nonce, &none);
        let mut a = args(&first, &dir, payout, Some(fixture::pk(30)));
        a.out = Some(dir.join("unsigned.json"));
        run(handle(&a)).unwrap();
        let approvals = approve_roles(&dir, &dir.join("unsigned.json"));
        let moved = node(&base, nonce + 1, &none);
        let mut b = args(&moved, &dir, payout, Some(fixture::pk(30)));
        b.dry_run = false;
        b.approvals_file = Some(approvals.clone());
        b.key_file = Some(dir.join("missing-owner-key"));
        b.out = Some(dir.join("signed.json"));
        let error = run(handle(&b)).unwrap_err().to_string();
        assert!(error.contains("approvals do not match"), "{error}");
        // Missing approval: no payout signature over an ed25519-payout Update.
        let mut c = args(&first, &dir, payout, Some(fixture::pk(30)));
        c.dry_run = false;
        c.approvals_file = Some(approvals);
        c.key_file = Some(seed(&dir, 10));
        c.out = Some(dir.join("signed.json"));
        let error = run(handle(&c)).unwrap_err().to_string();
        assert!(error.contains("Update approvals do not verify"), "{error}");
        assert!(!dir.join("signed.json").exists());
        assert!(
            first.submitted.lock().unwrap().is_none() && moved.submitted.lock().unwrap().is_none()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
