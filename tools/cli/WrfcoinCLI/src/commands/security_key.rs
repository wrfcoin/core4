//! `wrfcoin wallet security-key …`: require a FIDO2 security key (YubiKey)
//! to send from a 12-word wallet (`security_key_spend`; design
//! `docs/design/security-key-wallets.md`).
//!
//! Every action is signed by the wallet's phrase key AND (except `escape`)
//! co-signed by a registered security key through libfido2's tools; see
//! `wrfcoin_cli::security_key`. The phrase never leaves the key store and is
//! never printed.

use anyhow::{Result, anyhow, bail};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use wrfcoin_cli::security_key::{DEFAULT_RP_ID, Fido2Tools};
use wrfcoin_cli::{ApiClient, ApiConfig};
use wrfcoin_shared_protocol::accounts::security_key::{
    ESCAPE_DELAY_DEFAULT_BLOCKS, SecurityKeyActionBody, SecurityKeyActionPayload,
    SecurityKeyAssertionWire, SecurityKeyWire, action_challenge, action_message, spend_challenge,
};
use wrfcoin_shared_protocol::blockchain::transaction::transparent_transfer_authorization_message;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::domain_signing::{signing_chain_id, signing_network_id};

use crate::cli::commands::Wallet;

/// Which key, and under which relying party.
#[derive(Args, Clone, Debug)]
pub struct KeyArgs {
    /// FIDO2 device path (default: the first key `fido2-token -L` lists)
    #[clap(long, env = "WRFCOIN_FIDO2_DEVICE")]
    pub device: Option<String>,
    /// WebAuthn relying party ID the key is enrolled under (the web wallet uses wrfcoin.com)
    #[clap(long, env = "WRFCOIN_SECURITY_KEY_RP_ID", default_value = DEFAULT_RP_ID)]
    pub rp_id: String,
}

impl KeyArgs {
    fn tools(&self) -> Fido2Tools {
        Fido2Tools::new(self.device.clone(), Some(self.rp_id.clone()))
    }
}

/// Security-key commands.
#[derive(Subcommand, Clone, Debug)]
pub enum SecurityKeyCommands {
    /// Show whether sends need the security key, the registered keys and any pending escape
    Status {
        /// Address to show (default: primary address)
        #[clap(long)]
        address: Option<String>,
    },
    /// Require a security key to send: enroll a key (touch + PIN twice)
    Enable {
        #[clap(flatten)]
        key: KeyArgs,
        /// Blocks a phrase-only escape must wait (1123..=33663; default ~7 days)
        #[clap(long, default_value_t = ESCAPE_DELAY_DEFAULT_BLOCKS)]
        escape_delay_blocks: u64,
    },
    /// Add a spare key (needs a registered key and the new key)
    Add {
        #[clap(flatten)]
        key: KeyArgs,
    },
    /// Remove a registered key (needs any registered key); removing the last turns protection off
    Remove {
        /// Hex P-256 public key of the key to remove (see `status`)
        #[clap(long)]
        public_key: String,
        #[clap(flatten)]
        key: KeyArgs,
    },
    /// Turn protection off now (needs a registered key)
    Disable {
        #[clap(flatten)]
        key: KeyArgs,
    },
    /// Lost every key: start the time-locked escape with the phrase alone
    Escape {
        /// Confirm: after the delay, the phrase alone can send again
        #[clap(long)]
        yes: bool,
    },
    /// Cancel a pending escape (needs a registered key)
    CancelEscape {
        #[clap(flatten)]
        key: KeyArgs,
    },
}

fn registered_keys(status: &Value) -> Vec<SecurityKeyWire> {
    status
        .get("keys")
        .cloned()
        .and_then(|keys| serde_json::from_value(keys).ok())
        .unwrap_or_default()
}

/// Ask whichever registered key is plugged in for an assertion.
fn assert_with_any_registered(
    tools: &Fido2Tools,
    keys: &[SecurityKeyWire],
    challenge: &[u8; 32],
) -> Result<SecurityKeyAssertionWire> {
    let rp = hex::encode(tools.rp_id_hash());
    let mut last = anyhow!(
        "no registered key is enrolled under relying party {:?}; pass --rp-id (or WRFCOIN_SECURITY_KEY_RP_ID)",
        tools.rp_id
    );
    for key in keys.iter().filter(|k| k.rp_id_hash == rp) {
        match tools.assert(&key.public_key_p256, &key.credential_id, challenge) {
            Ok(assertion) => return Ok(assertion),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// For `wallet send`: the security-key assertion a protected account needs,
/// or `None` when the account is not protected (or the node predates the
/// route, which means the chain cannot have protected it either).
pub async fn spend_assertion_if_required(
    client: &ApiClient,
    from: &str,
    to: &str,
    amount: u64,
    nonce: u64,
) -> Result<Option<SecurityKeyAssertionWire>> {
    let Ok(status) = client.get_security_key_status(from).await else {
        return Ok(None);
    };
    if !status.get("required_to_spend").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(None);
    }
    let account: Address = from.parse().map_err(|_| anyhow!("from is not an address"))?;
    let message = transparent_transfer_authorization_message(from, to, amount, nonce);
    let challenge = spend_challenge(&account, &message);
    // determinism-ok: CLI-side device selection only
    let tools = Fido2Tools::new(
        std::env::var("WRFCOIN_FIDO2_DEVICE").ok(),
        std::env::var("WRFCOIN_SECURITY_KEY_RP_ID").ok(),
    );
    eprintln!("This wallet requires its security key to send: touch the key and enter its PIN.");
    assert_with_any_registered(&tools, &registered_keys(&status), &challenge).map(Some)
}

pub async fn handle_security_key_command(command: &SecurityKeyCommands) -> Result<Value> {
    let wallet = Wallet::try_default()?;
    let client = ApiClient::new(ApiConfig::default())?;
    if let SecurityKeyCommands::Status { address } = command {
        let address = match address {
            Some(address) => address.clone(),
            None => wallet.primary_signing_address()?,
        };
        let status = client.get_security_key_status(&address).await?;
        let required = status.get("required_to_spend").and_then(Value::as_bool).unwrap_or(false);
        return Ok(json!({
            "summary": if required {
                format!("{address}: sends need the security key")
            } else {
                format!("{address}: sends need only the phrase")
            },
            "status": status,
        }));
    }

    let address = wallet.primary_signing_address()?;
    let account: Address = address.parse().map_err(|_| anyhow!("primary address is not an address"))?;
    let status = client.get_security_key_status(&address).await?;
    if !status.get("feature_active_at_next_height").and_then(Value::as_bool).unwrap_or(false) {
        bail!("security_key_spend is not active on this chain yet");
    }
    let nonce = status
        .get("next_nonce")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("node did not return next_nonce"))?;
    let keys = registered_keys(&status);

    // Credential creation does not depend on the challenge, so it happens
    // before the action bytes are fixed.
    let (body, tools, new_key) = match command {
        SecurityKeyCommands::Status { .. } => unreachable!("handled above"),
        SecurityKeyCommands::Enable { key, escape_delay_blocks } => {
            let tools = key.tools();
            eprintln!("Insert the security key to enroll, touch it and enter its PIN.");
            let new_key = tools.make_credential(&format!("wrfcoin {address}"), &account.0)?;
            let body = SecurityKeyActionBody::Enable {
                key: new_key.clone(),
                escape_delay_blocks: *escape_delay_blocks,
            };
            (body, Some(tools), Some(new_key))
        },
        SecurityKeyCommands::Add { key } => {
            let tools = key.tools();
            eprintln!("Insert the NEW key to enroll, touch it and enter its PIN.");
            let new_key = tools.make_credential(&format!("wrfcoin {address}"), &account.0)?;
            (SecurityKeyActionBody::AddKey { key: new_key.clone() }, Some(tools), Some(new_key))
        },
        SecurityKeyCommands::Remove { public_key, key } => (
            SecurityKeyActionBody::RemoveKey { public_key_p256: public_key.trim().to_lowercase() },
            Some(key.tools()),
            None,
        ),
        SecurityKeyCommands::Disable { key } => (SecurityKeyActionBody::Disable, Some(key.tools()), None),
        SecurityKeyCommands::CancelEscape { key } => {
            (SecurityKeyActionBody::CancelEscape, Some(key.tools()), None)
        },
        SecurityKeyCommands::Escape { yes } => {
            if !yes {
                bail!(
                    "`escape` lets the phrase alone send again after the escape delay; \
                     re-run with --yes to start it (a registered key can cancel it)"
                );
            }
            (SecurityKeyActionBody::BeginEscape, None, None)
        },
    };

    let message = action_message(&signing_chain_id(), &signing_network_id(), &account, nonce, &body)?;
    let challenge = action_challenge(&message);
    let mut key_assertions = Vec::new();
    if let Some(tools) = &tools {
        match &body {
            SecurityKeyActionBody::Enable { .. } => {},
            _ => {
                eprintln!("Insert a REGISTERED key, touch it and enter its PIN.");
                key_assertions.push(assert_with_any_registered(tools, &keys, &challenge)?);
            },
        }
        if let Some(new_key) = &new_key {
            eprintln!("Now the NEW key again: touch it and enter its PIN.");
            key_assertions.push(tools.assert(&new_key.public_key_p256, &new_key.credential_id, &challenge)?);
        }
    }

    let (signer, owner_public_key, owner_signature) = wallet.sign_with_primary(&message)?;
    if signer != address {
        bail!("primary address changed while signing");
    }
    let payload = SecurityKeyActionPayload {
        account: address.clone(),
        nonce,
        action: body,
        owner_public_key,
        owner_signature,
        key_assertions,
    };
    let accepted = client.submit_security_key_action(serde_json::to_value(&payload)?).await?;
    Ok(json!({
        "summary": format!("Submitted security-key action for {address} (nonce {nonce}) — not yet confirmed"),
        "account": address,
        "nonce": nonce,
        "result": accepted,
    }))
}
