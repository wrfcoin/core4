//! Keyless Deposit assembly, explicit role approvals and one-shot owner submission.
use anyhow::{Result, ensure};
use clap::Args;
use serde_json::{Value, json};
use std::path::PathBuf;
use wrfcoin_cli::{
    ApiClient, ApiConfig,
    node_operator::{OperatorKey, OperatorRole},
};
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, TransactionInput, TransactionOutput, TransactionType},
    common::types::Address,
    consensus::{
        node_bond_registry::{NODE_BOND_UNITS, NodeBondLifecycle, node_bond_escrow_address},
        node_share_contracts::*,
    },
    crypto::{chain_binding::ChainIdentity, hash::hash_bytes, signature::PublicKey},
};

#[derive(Args)]
pub struct DepositArgs {
    #[clap(long)]
    pub deployment_id: String,
    #[clap(long)]
    pub owner_key: String,
    #[clap(long)]
    pub payout_address: String,
    /// Ed25519 public key of the payout account. Omit it when the payout
    /// account is a passkey wallet: the payout then approves with a WebAuthn
    /// assertion (`--payout-passkey-assertion`, needs `passkey_bond_payout`).
    #[clap(long)]
    pub payout_key: Option<String>,
    /// JSON file holding the passkey payout account's assertion over this
    /// Deposit's challenge (the `payout_passkey` object, or a lifecycle /
    /// transaction carrying it). Verified locally before the owner signs.
    #[clap(long, conflicts_with = "payout_key")]
    pub payout_passkey_assertion: Option<PathBuf>,
    #[clap(long)]
    pub server_key: String,
    #[clap(long)]
    pub consumer_key: String,
    #[clap(long)]
    pub governance_key: String,
    #[clap(long)]
    pub service_alias: String,
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
    /// Assemble and print public transaction/hash; no private key is loaded.
    #[clap(long)]
    pub dry_run: bool,
}

pub(crate) fn id(value: &str) -> Result<[u8; 32]> {
    ensure!(value.len() == 64, "expected 32-byte public hex identifier");
    let bytes = hex::decode(value).map_err(|_| anyhow::anyhow!("public hex identifier refused"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("public hex identifier refused"))
}
/// Parse a 32-byte hash the NODE reported. launch_api emits genesis hashes as
/// `0x` + 64 hex (routes/network.rs `get_network_info`), while the CLI's own
/// arguments are bare 64 hex; accept the optional prefix only on API reads so
/// `id()` stays strict for operator input.
pub(crate) fn api_hash(value: &str) -> Option<[u8; 32]> {
    let bare = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    id(bare).ok()
}
fn number(v: &Value) -> Result<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .ok_or_else(|| anyhow::anyhow!("canonical height/nonce unavailable"))
}

/// The node-epoch lengths a chain can have: the protocol default, or the
/// shorter length a chain records in its genesis. Only the node knows which one
/// this chain uses; `node_epoch_blocks()` is the default in this process,
/// because the genesis-derived authority is installed in nodes, never here.
const KNOWN_NODE_EPOCH_BLOCKS: [u64; 2] = [512, NODE_EPOCH_BLOCKS];
/// Refuse this close to an epoch's end: a Deposit included in the next epoch
/// is refused, and it takes more than a few blocks to reach inclusion.
const EPOCH_END_MARGIN_BLOCKS: u64 = 3;

/// The node's own epoch at its own tip, checked against the known lengths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NodeEpoch {
    height: u64,
    epoch: u64,
    /// `None` only in early epoch 0, where every known length still fits.
    epoch_blocks: Option<u64>,
    /// Last height of `epoch`; the earliest close when the length is open.
    close_height: u64,
}

impl NodeEpoch {
    /// Trust only what the node reported, and refuse unless it is coherent. A
    /// transaction carried inside `epoch` must name `epoch + 1`, so the margin
    /// also guarantees every candidate length agrees on the carrying epoch.
    fn from_node(activation: u64, height: u64, epoch: u64) -> Result<Self> {
        let offset = height
            .checked_sub(activation)
            .ok_or_else(|| anyhow::anyhow!("Deposit refused before activation height"))?;
        let mut fits = KNOWN_NODE_EPOCH_BLOCKS
            .into_iter()
            .filter(|length| offset / length == epoch);
        let shortest = fits.next().ok_or_else(|| {
            anyhow::anyhow!(
                "node reports epoch {epoch} at height {height} (activation {activation}), \
                 which fits no known epoch length; refusing to guess an effective epoch"
            )
        })?;
        let epoch_blocks = fits.next().is_none().then_some(shortest);
        let close_height = epoch
            .checked_add(1)
            .and_then(|next| next.checked_mul(shortest))
            .and_then(|span| span.checked_add(activation))
            .map(|end| end - 1)
            .ok_or_else(|| anyhow::anyhow!("epoch overflow"))?;
        let view = Self {
            height,
            epoch,
            epoch_blocks,
            close_height,
        };
        ensure!(
            view.blocks_left() >= EPOCH_END_MARGIN_BLOCKS,
            "too close to the end of epoch {epoch} (closes at height {close_height}, tip {height}); \
             wait for the next epoch and reassemble"
        );
        Ok(view)
    }
    /// Blocks after the next one (the carrying block) still inside this epoch.
    fn blocks_left(&self) -> u64 {
        self.close_height
            .saturating_sub(self.height.saturating_add(1))
    }
    fn effective_epoch(&self) -> Result<u64> {
        self.epoch
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("epoch overflow"))
    }
}

fn node_epoch(registration: &Value, activation: u64) -> Result<NodeEpoch> {
    ensure!(
        number(&registration["activation_height"])? == activation,
        "canonical activation height mismatch"
    );
    NodeEpoch::from_node(
        activation,
        number(&registration["height"])?,
        number(&registration["epoch"])?,
    )
}

fn normalized(tx: &Transaction) -> Result<Value> {
    let mut v = serde_json::to_value(tx)?;
    for field in ["id", "timestamp", "data"] {
        v.as_object_mut().unwrap().remove(field);
    }
    Ok(v)
}

fn unsigned(args: &DepositArgs, nonce: u64, effective_epoch: u64) -> Result<Transaction> {
    let owner_key = id(&args.owner_key)?;
    let owner = Address::from_public_key(&PublicKey::from_bytes(&owner_key)?);
    let payout: Address = args.payout_address.parse()?;
    let payout_key = args.payout_key.as_deref().map(id).transpose()?;
    ensure!(payout != owner, "payout must differ from hot bond owner");
    if let Some(payout_key) = payout_key {
        ensure!(
            Address::from_public_key(&PublicKey::from_bytes(&payout_key)?) == payout,
            "payout public key/address mismatch"
        );
    }
    let governance_key = id(&args.governance_key)?;
    ensure!(
        governance_key != owner_key,
        "governance key must differ from hot bond owner"
    );
    let server_key = id(&args.server_key)?;
    let consumer_key = id(&args.consumer_key)?;
    ensure!(
        payout_key != Some(server_key) && payout_key != Some(consumer_key),
        "payout key must differ from dedicated node role keys"
    );
    ensure!(
        server_key != consumer_key && owner_key != server_key && owner_key != consumer_key,
        "dedicated node role keys must differ"
    );
    ensure!(
        !args.service_alias.is_empty() && args.service_alias.len() <= MAX_NODE_ALIAS_BYTES,
        "service alias refused"
    );
    let deployment = id(&args.deployment_id)?;
    let escrow = node_bond_escrow_address(&deployment);
    ensure!(
        owner != escrow && payout != escrow,
        "escrow role collision refused"
    );
    let p = NodeBondLifecycle {
        signed: SignedNodeBondAuthorization {
            authorization: NodeBondAuthorization {
                genesis_hash: id(&args.genesis_hash)?,
                deployment_id: deployment,
                owner_account: owner,
                escrow_account: escrow,
                payout_account: payout,
                server_key,
                consumer_key,
                service_alias: args.service_alias.clone(),
                action: NodeBondAction::Deposit,
                amount_units: NODE_BOND_UNITS,
                expected_nonce: nonce,
                effective_epoch,
            },
            owner_signature: vec![],
            server_signature: vec![],
            consumer_signature: vec![],
        },
        owner_key,
        governance_key: Some(governance_key),
        governance_signature: vec![],
        payout_key,
        payout_signature: vec![],
        payout_passkey: None,
    };
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_bytes(b"wrfcoin.node_bond_deposit.input.v1"),
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
        "Deposit packet exceeds cap"
    );
    Ok(tx)
}

pub async fn handle(args: &DepositArgs) -> Result<Value> {
    // Cheap public input checks precede any key or credential/network operation.
    let _ = unsigned(args, 0, 1)?;
    let owner = Address::from_public_key(&PublicKey::from_bytes(&id(&args.owner_key)?)?);
    let out = args.out.as_ref();
    if let Some(out) = out {
        ensure!(
            !out.exists(),
            "packet already exists; inspect canonical inclusion before explicit retry"
        );
    }
    let url =
        url::Url::parse(&args.api_url).map_err(|_| anyhow::anyhow!("public API URL refused"))?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "public API URL refused"
    );
    let secret = super::node_receipt::secret_file(&args.operator_token_file, 16 * 1024)?;
    let token = std::str::from_utf8(&secret)
        .map_err(|_| anyhow::anyhow!("operator credential refused"))?
        .trim();
    let available = !token.is_empty();
    ensure!(available, "operator credential unavailable"); // secret-leak-ok: boolean presence and fixed error only.
    let api = ApiClient::new_without_redirects(ApiConfig {
        base_url: args.api_url.trim_end_matches('/').into(),
        bearer_token: Some(token.to_owned()),
        ..ApiConfig::default()
    })?;
    let network = api.receipt_request("GET", "/network/info", None).await?;
    ensure!(
        network["chain_id"].as_str() == Some(args.chain_id.as_str())
            && network["network_id"].as_str() == Some(args.network_id.as_str())
            && network["genesis_hash"].as_str().and_then(api_hash) == Some(id(&args.genesis_hash)?),
        "API chain/genesis mismatch"
    );
    let tip = api.receipt_request("GET", "/blockchain", None).await?;
    ensure!(
        number(&tip["height"])? >= args.activation_height,
        "Deposit refused before activation height {}",
        args.activation_height
    );
    let path = format!("/node/bonds/{}", hex::encode(id(&args.deployment_id)?));
    let registration = api.receipt_request("GET", &path, None).await?;
    let epoch = node_epoch(&registration, args.activation_height)?;
    ensure!(
        registration["status"] == "UNKNOWN" && registration["registration"].is_null(),
        "deployment already registered or unavailable"
    );
    let carrying = epoch
        .height
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("height overflow"))?;
    let account = api
        .receipt_request("GET", &format!("/addresses/{owner}"), None)
        .await?;
    let nonce = number(&account["nonce"])?;
    let expected = unsigned(args, nonce, epoch.effective_epoch()?)?;
    let payload = NodeBondLifecycle::parse(&expected).unwrap();
    if args.dry_run {
        if let Some(out) = out {
            super::node_receipt::durable_packet(out, &expected)?;
        }
        let mut plan = json!({"dry_run":true,"transaction":expected,"tx_id":expected.id.map(|id|id.to_hex()),"validated_for_carrying_height":carrying,"effective_epoch":payload.signed.authorization.effective_epoch,"node_height":epoch.height,"node_epoch":epoch.epoch,"node_epoch_blocks":epoch.epoch_blocks,"epoch_close_height":epoch.close_height,"blocks_left_in_epoch":epoch.blocks_left(),"submitted":false,"included":false});
        if payload.payout_key.is_none() {
            // Passkey payout: the challenge the payout wallet signs.
            let identity = ChainIdentity::new(&args.chain_id, &args.network_id);
            plan["payout_passkey_challenge"] =
                super::node_bond::payout_challenge_json(&payload.signed.authorization, &identity);
        }
        return Ok(plan);
    }
    let out = out.ok_or_else(|| anyhow::anyhow!("--out required for durable Deposit"))?;
    let approvals = args.approvals_file.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "--approvals-file required: obtain four other role approvals over the dry-run packet"
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
        "approvals do not match current nonce, epoch or Deposit plan; reassemble and reapprove"
    );
    // Refuse a changed inclusion epoch/nonce before saving or POSTing the signature.
    let fresh = api.receipt_request("GET", &path, None).await?;
    ensure!(
        node_epoch(&fresh, args.activation_height)?.effective_epoch()?
            == p.signed.authorization.effective_epoch,
        "inclusion epoch changed; reassemble and obtain new role approvals"
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
            .ok_or_else(|| anyhow::anyhow!("--key-file required for Deposit owner"))?,
    )?;
    let identity = ChainIdentity::new(&args.chain_id, &args.network_id);
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
    // Offline check as the chain will verify it; a passkey approval is only
    // accepted by nodes once passkey_bond_payout is active.
    p.verify_with_passkey_payout(
        &identity,
        &id(&args.genesis_hash)?,
        &[args.governance_key.to_ascii_lowercase()],
        p.payout_passkey.is_some(),
    )?;
    tx.data = Some(serde_json::to_string(&p)?);
    // Lifecycle approvals bind the authorization, not the outer timestamp.
    // Use the fresh canonical plan so an off-host approval delay cannot age
    // the submitted transaction beyond the producer admission window.
    tx.timestamp = expected.timestamp;
    tx.set_id();
    ensure!(
        serde_json::to_vec(&tx)?.len() <= 40 * 1024,
        "Deposit packet exceeds cap"
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
    let registered =
        view["registration"]["binding"] == serde_json::to_value(&p.signed.authorization)?;
    if !admitted {
        anyhow::bail!(
            "UNCERTAIN; registration_observed={registered}; retain packet and inspect nonce/inclusion before explicit retry"
        );
    }
    Ok(
        json!({"tx_id":tx.id.map(|h|h.to_hex()),"admitted":true,"included":false,"registration_observed":registered,"effective_epoch":p.signed.authorization.effective_epoch,"packet":out.display().to_string()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_hash_accepts_launch_api_0x_form_and_bare_hex() {
        let bare = hex::encode([7u8; 32]);
        assert_eq!(api_hash(&bare), Some([7; 32]));
        assert_eq!(api_hash(&format!("0x{bare}")), Some([7; 32]));
        assert_eq!(
            api_hash(&format!("0X{}", bare.to_uppercase())),
            Some([7; 32])
        );
        // "unknown" is what /network/info reports when genesis is unresolved.
        assert_eq!(api_hash("unknown"), None);
        assert_eq!(api_hash(&format!("0x0x{bare}")), None);
        assert_eq!(api_hash(&format!("0x{}", &bare[2..])), None);
        // Operator-supplied arguments stay strict.
        assert!(id(&format!("0x{bare}")).is_err());
    }

    #[test]
    fn node_epoch_takes_the_nodes_epoch_and_refuses_what_it_cannot_explain() {
        // This process never installs the chain authority: the local default
        // would say epoch 12 here, on a chain whose node says 24.
        assert_eq!(node_epoch_blocks(), 1_024);
        let live = NodeEpoch::from_node(2, 12_321, 24).unwrap();
        assert_eq!(live.effective_epoch().unwrap(), 25);
        assert_eq!(live.epoch_blocks, Some(512));
        assert_eq!(live.close_height, 12_801);
        assert_eq!(live.blocks_left(), 479);
        // A default-length chain past epoch 0 keeps working.
        let default = NodeEpoch::from_node(2, 12_321, 12).unwrap();
        assert_eq!(default.effective_epoch().unwrap(), 13);
        assert_eq!(default.epoch_blocks, Some(1_024));
        assert_eq!(default.close_height, 13_313);
        // Early epoch 0: both lengths fit, epoch 1 is right for both, and the
        // earliest possible close is what counts.
        let early = NodeEpoch::from_node(2, 100, 0).unwrap();
        assert_eq!(early.effective_epoch().unwrap(), 1);
        assert_eq!(early.epoch_blocks, None);
        assert_eq!(early.close_height, 513);
        // Epoch 0 past 512 blocks can only be the longer length.
        assert_eq!(
            NodeEpoch::from_node(2, 600, 0).unwrap().epoch_blocks,
            Some(1_024)
        );
        // A reported epoch that no known length explains is refused.
        for bad in [13, 23, 25, 0] {
            let error = NodeEpoch::from_node(2, 12_321, bad)
                .unwrap_err()
                .to_string();
            assert!(error.contains("fits no known epoch length"), "{error}");
        }
        assert!(NodeEpoch::from_node(2, 1, 0).is_err());
    }

    #[test]
    fn node_epoch_refuses_near_the_epoch_end_including_when_the_length_is_open() {
        // Carrying block 12_798 leaves 3 more in the epoch: the last allowed tip.
        assert_eq!(
            NodeEpoch::from_node(2, 12_797, 24).unwrap().blocks_left(),
            3
        );
        for tip in [12_798, 12_799, 12_800, 12_801] {
            let error = NodeEpoch::from_node(2, tip, 24).unwrap_err().to_string();
            assert!(
                error.contains("too close to the end of epoch 24"),
                "{error}"
            );
        }
        // Tip 513 with the length open: carrying 514 is epoch 1 on a 512-block
        // chain and epoch 0 on a 1024-block chain, so it must not assemble.
        for tip in [510, 511, 512, 513] {
            assert!(NodeEpoch::from_node(2, tip, 0).is_err());
        }
        assert!(NodeEpoch::from_node(2, 509, 0).is_ok());
    }

    fn bond_view(deployment: &str, height: u64, activation: u64, epoch: u64) -> String {
        json!({"height":height.to_string(),"activation_height":activation.to_string(),
            "epoch":epoch.to_string(),"deployment_id":deployment,
            "status":"UNKNOWN","principal_base_units":null,"jailed":null,"registration":null})
        .to_string()
    }

    /// A 512-block chain, far past epoch 0, with the wire shapes a live node
    /// serves: the assembled Deposit must name the node's epoch + 1, the dry run
    /// must print both, a node that disagrees must be refused, and approvals
    /// over a stale epoch must be refused before any owner key is needed.
    #[test]
    fn deposit_on_a_512_block_chain_past_epoch_zero_uses_the_nodes_epoch() {
        let key = |n: u8| OperatorKey::from_seed(&[n; 32]).unwrap();
        let hexpk = |n: u8| hex::encode(key(n).public_key().as_bytes());
        let owner = key(11).address();
        let dir = std::env::temp_dir().join(format!("wrfcoin-deposit-512-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let token = dir.join("token");
        std::fs::write(&token, "synthetic-deposit-cli-bearer").unwrap(); // secret-leak-ok: fixed synthetic bearer for mock server only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let genesis = hex::encode([8u8; 32]);
        let deployment = hex::encode([10u8; 32]);
        let envelope = |data: Value| {
            json!({"ok":true,"data":data,"error":null,"meta":{"request_id":"r"}}).to_string()
        };
        let _n = mockito::mock("GET", "/network/info")
            .with_body(json!({"network_id":"testnet","chain_id":"888","genesis_hash":format!("0x{genesis}"),
                "peer_count":0,"sync_status":"synced","chain_height":12321}).to_string())
            .create();
        let _b = mockito::mock("GET", "/blockchain")
            .with_body(envelope(
                json!({"height":12321,"tip":"x","accounts":1,"nfts":0,"pending_transactions":0}),
            ))
            .create();
        let _a = mockito::mock("GET", format!("/addresses/{owner}").as_str())
            .with_body(envelope(
                json!({"address":owner.to_string(),"balance":"1","nonce":"0"}),
            ))
            .create();
        let bonds = format!("/node/bonds/{deployment}");
        let mut args = DepositArgs {
            deployment_id: deployment.clone(),
            owner_key: hexpk(11),
            payout_address: key(12).address().to_string(),
            payout_key: Some(hexpk(12)),
            payout_passkey_assertion: None,
            server_key: hexpk(13),
            consumer_key: hexpk(14),
            governance_key: hexpk(15),
            service_alias: "node-b".into(),
            genesis_hash: genesis,
            chain_id: "888".into(),
            network_id: "testnet".into(),
            activation_height: 2,
            api_url: mockito::server_url(),
            operator_token_file: token,
            key_file: None,
            approvals_file: None,
            fee: 1_000_000,
            out: None,
            dry_run: true,
        };
        let run = |args: &DepositArgs| {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(handle(args))
        };
        let plan = {
            let _r = mockito::mock("GET", bonds.as_str())
                .with_body(bond_view(&deployment, 12_321, 2, 24))
                .create();
            run(&args).unwrap()
        };
        assert_eq!(plan["node_epoch"], 24);
        assert_eq!(plan["effective_epoch"], 25);
        assert_eq!(plan["node_epoch_blocks"], 512);
        assert_eq!(plan["epoch_close_height"], 12_801);
        assert_eq!(plan["blocks_left_in_epoch"], 479);
        let tx: Transaction = serde_json::from_value(plan["transaction"].clone()).unwrap();
        let lifecycle = NodeBondLifecycle::parse(&tx).unwrap();
        assert_eq!(lifecycle.signed.authorization.effective_epoch, 25);
        // No passkey: the serialized payload has no new member.
        assert!(!tx.data.as_deref().unwrap().contains("payout_passkey"));

        // The epoch the default length would compute is not what a node says here.
        {
            let _r = mockito::mock("GET", bonds.as_str())
                .with_body(bond_view(&deployment, 12_321, 2, 13))
                .create();
            let error = run(&args).unwrap_err().to_string();
            assert!(error.contains("fits no known epoch length"), "{error}");
        }
        // Too close to the end of the epoch.
        {
            let _r = mockito::mock("GET", bonds.as_str())
                .with_body(bond_view(&deployment, 12_799, 2, 24))
                .create();
            let error = run(&args).unwrap_err().to_string();
            assert!(
                error.contains("too close to the end of epoch 24"),
                "{error}"
            );
        }
        // Approvals made in epoch 24 cannot be submitted in epoch 25; this is
        // refused before an owner key is even read.
        let approvals = dir.join("approvals.json");
        std::fs::write(&approvals, serde_json::to_vec(&tx).unwrap()).unwrap();
        args.dry_run = false;
        args.approvals_file = Some(approvals);
        args.out = Some(dir.join("signed.json"));
        {
            let _r = mockito::mock("GET", bonds.as_str())
                .with_body(bond_view(&deployment, 12_900, 2, 25))
                .create();
            let error = run(&args).unwrap_err().to_string();
            assert!(error.contains("approvals do not match"), "{error}");
        }
        assert!(!dir.join("signed.json").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Dry-run against a mock that serves the exact launch_api wire shapes:
    /// `/network/info` bare JSON with 0x genesis (routes/network.rs
    /// get_network_info), `/blockchain` + `/addresses/{a}` in the LawBook
    /// envelope `{ok,data,..}` with numbers/strings as emitted (routes/chain.rs
    /// get_blockchain_compat / get_address_compat), `/node/bonds/{id}` bare JSON
    /// with string heights and `status:"UNKNOWN"` (routes/node_bonds.rs inspect).
    #[test]
    fn dry_run_accepts_real_launch_api_shapes() {
        let key = |n: u8| OperatorKey::from_seed(&[n; 32]).unwrap();
        let hexpk = |n: u8| hex::encode(key(n).public_key().as_bytes());
        let owner = key(1).address();
        let dir = std::env::temp_dir().join(format!("wrfcoin-deposit-0x-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let token = dir.join("token");
        std::fs::write(&token, "synthetic-deposit-cli-bearer").unwrap(); // secret-leak-ok: fixed synthetic bearer for mock server only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let genesis = hex::encode([7u8; 32]);
        let deployment = hex::encode([9u8; 32]);
        let envelope = |data: Value| {
            json!({"ok":true,"data":data,"error":null,"meta":{"request_id":"r"}}).to_string()
        };
        let _n = mockito::mock("GET", "/network/info")
            .with_body(json!({"network_id":"testnet","chain_id":"777","genesis_hash":format!("0x{genesis}"),
                "peer_count":0,"sync_status":"synced","chain_height":48001}).to_string())
            .create();
        let _b = mockito::mock("GET", "/blockchain")
            .with_body(envelope(
                json!({"height":48001,"tip":"x","accounts":1,"nfts":0,"pending_transactions":0}),
            ))
            .create();
        let _r = mockito::mock("GET", format!("/node/bonds/{deployment}").as_str())
            .with_body(json!({"height":"48001","activation_height":"48000","epoch":"0","deployment_id":deployment,
                "status":"UNKNOWN","principal_base_units":null,"jailed":null,"registration":null}).to_string())
            .create();
        let _a = mockito::mock("GET", format!("/addresses/{owner}").as_str())
            .with_body(envelope(
                json!({"address":owner.to_string(),"balance":"1","nonce":"3"}),
            ))
            .create();
        let mut args = DepositArgs {
            deployment_id: deployment,
            owner_key: hexpk(1),
            payout_address: key(2).address().to_string(),
            payout_key: Some(hexpk(2)),
            payout_passkey_assertion: None,
            server_key: hexpk(3),
            consumer_key: hexpk(4),
            governance_key: hexpk(5),
            service_alias: "node-a".into(),
            genesis_hash: genesis,
            chain_id: "777".into(),
            network_id: "testnet".into(),
            activation_height: 48000,
            api_url: mockito::server_url(),
            operator_token_file: token,
            key_file: None,
            approvals_file: None,
            fee: 1_000_000,
            out: None,
            dry_run: true,
        };
        let out = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(handle(&args))
            .unwrap();
        assert_eq!(out["dry_run"], true);
        assert_eq!(out["validated_for_carrying_height"], 48002);
        assert!(out.get("payout_passkey_challenge").is_none());
        // Passkey payout: no --payout-key; the plan carries the challenge.
        let wallet =
            wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator::new(
                &[0x3c; 32],
                "wrfcoin.com",
            );
        let ed_payout = args.payout_address.clone();
        args.payout_key = None;
        args.payout_address = wallet.account().to_string();
        let out = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(handle(&args))
            .unwrap();
        let tx: Transaction = serde_json::from_value(out["transaction"].clone()).unwrap();
        let plan = NodeBondLifecycle::parse(&tx).unwrap();
        assert_eq!(plan.payout_key, None);
        assert_eq!(
            out["payout_passkey_challenge"]["challenge_hex"],
            hex::encode(node_bond_payout_passkey_challenge(
                &plan.signed.authorization,
                &ChainIdentity::new("777", "testnet")
            ))
        );
        args.payout_key = Some(hexpk(2));
        args.payout_address = ed_payout;
        // An explicit 888 selection cannot silently sign against this 777 API.
        args.chain_id = "888".into();
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(handle(&args))
            .unwrap_err();
        assert!(error.to_string().contains("API chain/genesis mismatch"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
