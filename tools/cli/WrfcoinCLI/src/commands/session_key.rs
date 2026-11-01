//! Session-key artifact builders (`shared-protocol/src/accounts/session_keys.rs`).
//!
//! Offline, like `station owner`: each command reads key files, signs the
//! exact preimage core4 verifies, self-verifies it, and writes a JSON artifact
//! that is the body of a node route. No node contact. Private keys are read
//! from files only (never argv) and never written into an artifact.
//!
//! - `keygen`: new session (device) key file, owner-only permissions.
//! - `register`: owner key + session key sign a grant; body of
//!   `POST /session-keys/submit`. The registration nonce comes from
//!   `GET /accounts/{address}/session-keys` (`next_registration_nonce`).
//! - `revoke`: owner key signs; body of `POST /session-keys/submit`. The
//!   record's `registration_nonce` comes from the same listing.
//! - `send`: session key signs a transfer; body of `POST /transactions/submit`.
//!   The nonce is the `from` account nonce (`GET /addresses/{address}`).

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use serde_json::{Value, json};

use wrfcoin_shared_protocol::accounts::session_keys::{
    ANY_TARGET, MethodId, SessionKeyAction, SessionKeyActionPayload, SessionKeyGrant,
    SessionScope, SpendWindow, TRANSFER_METHOD, session_key_register_message,
    session_key_revoke_message, verify_stateless,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    SessionKeyTransferAuth, session_key_transfer_authorization_message_with_domain,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};

use super::station::{atomic_artifact_write, read_key};

/// Base units per WRF (8 decimals).
pub const BASE_UNITS_PER_WRF: u128 = 100_000_000;

/// Blocks in a "day" on chain 888 (~77 s per block).
pub const DAY_BLOCKS_888: u64 = 1_122;

#[derive(Args, Debug)]
pub struct RegisterArgs {
    /// The account's primary ed25519 private key file (hex).
    #[clap(long)]
    pub owner_key: PathBuf,
    /// The session (device) private key file (hex), e.g. from `keygen`. It
    /// proves possession by co-signing the registration.
    #[clap(long)]
    pub session_key: PathBuf,
    /// `next_registration_nonce` from `GET /accounts/{address}/session-keys`.
    #[clap(long)]
    pub registration_nonce: u64,
    /// First block height the key may be used at.
    #[clap(long)]
    pub valid_from: u64,
    /// Last block height the key may be used at.
    #[clap(long)]
    pub valid_until: u64,
    /// Allowed method(s). Default: `transfer`.
    #[clap(long = "method")]
    pub methods: Vec<String>,
    /// Allowed recipient(s). Omit and pass `--any-target` to allow anyone.
    #[clap(long = "target")]
    pub targets: Vec<String>,
    /// Allow any recipient (lists the explicit ANY_TARGET entry).
    #[clap(long)]
    pub any_target: bool,
    /// Largest single use, in base units.
    #[clap(long)]
    pub max_per_use: u128,
    /// Lifetime cap across all uses, in base units.
    #[clap(long)]
    pub max_total: u128,
    /// Recurring window length in blocks (1122 ≈ one day on chain 888).
    #[clap(long, requires = "window_max")]
    pub window_blocks: Option<u64>,
    /// Cap per window, in base units.
    #[clap(long, requires = "window_blocks")]
    pub window_max: Option<u128>,
    /// Optional cap on the number of uses.
    #[clap(long)]
    pub max_uses: Option<u32>,
    /// Chain id the signatures bind (use the value the node reports).
    #[clap(long)]
    pub chain_id: String,
    #[clap(long)]
    pub out: PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum SessionKeyCommands {
    /// Generate a session (device) key file with owner-only permissions.
    Keygen {
        #[clap(long)]
        out: PathBuf,
    },
    /// Owner + session key sign a grant; writes the `/session-keys/submit` body.
    Register(RegisterArgs),
    /// Owner signs a revocation; writes the `/session-keys/submit` body.
    Revoke {
        #[clap(long)]
        owner_key: PathBuf,
        /// The session key's address (`session_key` in the listing).
        #[clap(long)]
        session_key_address: String,
        /// The record's `registration_nonce` from the listing.
        #[clap(long)]
        registration_nonce: u64,
        #[clap(long)]
        chain_id: String,
        #[clap(long)]
        out: PathBuf,
    },
    /// Session key signs a transfer; writes the `/transactions/submit` body.
    Send {
        /// The session (device) private key file.
        #[clap(long)]
        session_key: PathBuf,
        /// The account the session key was registered under.
        #[clap(long)]
        from: String,
        #[clap(long)]
        to: String,
        /// Amount in base units.
        #[clap(long)]
        amount: u64,
        /// The `from` account's current nonce.
        #[clap(long)]
        nonce: u64,
        #[clap(long)]
        chain_id: String,
        #[clap(long, default_value = "testnet")]
        network_id: String,
        #[clap(long)]
        out: PathBuf,
    },
}

fn parse_address(raw: &str, what: &str) -> Result<Address> {
    raw.trim()
        .parse::<Address>()
        .map_err(|_| anyhow!("{what} is not a valid hex address: {raw}"))
}

fn write_json(out: &PathBuf, value: &Value) -> Result<()> {
    let text = serde_json::to_string_pretty(value)?;
    atomic_artifact_write(out, |file| file.write_all(text.as_bytes()))
}

/// Build the grant `register` signs. Pure, so it is tested without files.
pub fn build_grant(args: &RegisterArgs, session_public_key: PublicKey) -> Result<SessionKeyGrant> {
    let methods = if args.methods.is_empty() {
        vec![MethodId(TRANSFER_METHOD.to_string())]
    } else {
        args.methods.iter().map(|m| MethodId(m.clone())).collect()
    };
    let mut targets = args
        .targets
        .iter()
        .map(|t| parse_address(t, "--target"))
        .collect::<Result<Vec<_>>>()?;
    if args.any_target {
        targets.push(ANY_TARGET);
    }
    if targets.is_empty() {
        bail!("pass at least one --target, or --any-target (an empty list allows nothing)");
    }
    let window = match (args.window_blocks, args.window_max) {
        (Some(blocks), Some(max_value)) => Some(SpendWindow { blocks, max_value }),
        _ => None,
    };
    Ok(SessionKeyGrant {
        public_key: session_public_key,
        scope: SessionScope {
            allowed_methods: methods,
            allowed_targets: targets,
            max_value_per_use: args.max_per_use,
            max_total_value: args.max_total,
            window,
        },
        valid_from: args.valid_from,
        valid_until: args.valid_until,
        max_uses: args.max_uses,
    })
}

fn sign_register(
    owner_key: &PrivateKey,
    session_key: &PrivateKey,
    registration_nonce: u64,
    grant: SessionKeyGrant,
    chain_id: &str,
) -> Result<SessionKeyActionPayload> {
    let owner_pk = PublicKey::from_private_key(owner_key);
    let owner = Address::from_public_key(&owner_pk);
    let message = session_key_register_message(chain_id, &owner, registration_nonce, &grant);
    let action = SessionKeyAction::Register {
        owner,
        registration_nonce,
        grant,
        owner_public_key: owner_pk.to_hex(),
        owner_signature: owner_key.sign(&message).to_hex(),
        session_signature: session_key.sign(&message).to_hex(),
    };
    verify_stateless(&action, chain_id).map_err(|e| anyhow!("self-check failed: {e}"))?;
    Ok(SessionKeyActionPayload { action })
}

pub async fn handle_session_key_command(command: &SessionKeyCommands) -> Result<Value> {
    match command {
        SessionKeyCommands::Keygen { out } => {
            if out.exists() {
                bail!("{} already exists; refusing to overwrite a key", out.display());
            }
            let key = PrivateKey::generate(&mut rand::rngs::OsRng);
            let public_key = PublicKey::from_private_key(&key);
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(out)
                .with_context(|| format!("cannot create {}", out.display()))?;
            file.write_all(key.to_hex().as_bytes())?;
            file.sync_all()?;
            Ok(json!({
                "key_file": out.display().to_string(),
                "session_public_key": public_key.to_hex(),
                "session_key_address": Address::from_public_key(&public_key).to_string(),
            }))
        },
        SessionKeyCommands::Register(args) => {
            let owner_key = read_key(&args.owner_key)?;
            let session_key = read_key(&args.session_key)?;
            let grant = build_grant(args, PublicKey::from_private_key(&session_key))?;
            let payload = sign_register(
                &owner_key,
                &session_key,
                args.registration_nonce,
                grant,
                &args.chain_id,
            )?;
            let body = serde_json::to_value(&payload)?;
            write_json(&args.out, &body)?;
            Ok(json!({ "artifact": args.out.display().to_string(), "route": "POST /session-keys/submit" }))
        },
        SessionKeyCommands::Revoke {
            owner_key,
            session_key_address,
            registration_nonce,
            chain_id,
            out,
        } => {
            let owner_key = read_key(owner_key)?;
            let owner_pk = PublicKey::from_private_key(&owner_key);
            let owner = Address::from_public_key(&owner_pk);
            let session_key = parse_address(session_key_address, "--session-key-address")?;
            let message =
                session_key_revoke_message(chain_id, &owner, &session_key, *registration_nonce);
            let signature = owner_key.sign(&message);
            if !owner_pk.verify(&message, &signature) {
                bail!("self-check failed: revoke signature does not verify");
            }
            let payload = SessionKeyActionPayload {
                action: SessionKeyAction::Revoke {
                    owner,
                    session_key,
                    owner_public_key: owner_pk.to_hex(),
                    owner_signature: signature.to_hex(),
                },
            };
            write_json(out, &serde_json::to_value(&payload)?)?;
            Ok(json!({ "artifact": out.display().to_string(), "route": "POST /session-keys/submit" }))
        },
        SessionKeyCommands::Send {
            session_key,
            from,
            to,
            amount,
            nonce,
            chain_id,
            network_id,
            out,
        } => {
            let from = parse_address(from, "--from")?.to_string();
            let to = parse_address(to, "--to")?.to_string();
            let key = read_key(session_key)?;
            let public_key = PublicKey::from_private_key(&key);
            let message = session_key_transfer_authorization_message_with_domain(
                chain_id, network_id, &from, &to, *amount, *nonce, &public_key,
            );
            let signature: Signature = key.sign(&message);
            if !public_key.verify(&message, &signature) {
                bail!("self-check failed: transfer signature does not verify");
            }
            let auth = SessionKeyTransferAuth {
                session_public_key: public_key.to_hex(),
                signature: signature.to_hex(),
            };
            let body = json!({
                "from": from,
                "to": to,
                "amount": amount,
                "transparent_opt_out": true,
                "nonce": nonce.to_string(),
                "session_key_auth": auth,
            });
            write_json(out, &body)?;
            Ok(json!({ "artifact": out.display().to_string(), "route": "POST /transactions/submit" }))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn key(seed: u64) -> PrivateKey {
        PrivateKey::generate(&mut StdRng::seed_from_u64(seed))
    }

    fn args() -> RegisterArgs {
        RegisterArgs {
            owner_key: PathBuf::new(),
            session_key: PathBuf::new(),
            registration_nonce: 0,
            valid_from: 10,
            valid_until: 10 + 90 * DAY_BLOCKS_888,
            methods: vec![],
            targets: vec![],
            any_target: true,
            max_per_use: 10 * BASE_UNITS_PER_WRF,
            max_total: 300 * BASE_UNITS_PER_WRF,
            window_blocks: Some(DAY_BLOCKS_888),
            window_max: Some(10 * BASE_UNITS_PER_WRF),
            max_uses: None,
            chain_id: "wrfcoin-test".into(),
            out: PathBuf::new(),
        }
    }

    #[test]
    fn device_grant_defaults_to_transfer_and_any_target_with_a_daily_window() {
        let session = key(2);
        let grant = build_grant(&args(), PublicKey::from_private_key(&session)).unwrap();
        assert_eq!(grant.scope.allowed_methods, vec![MethodId("transfer".into())]);
        assert_eq!(grant.scope.allowed_targets, vec![ANY_TARGET]);
        assert_eq!(
            grant.scope.window,
            Some(SpendWindow { blocks: 1_122, max_value: 1_000_000_000 })
        );
        let payload = sign_register(&key(1), &session, 0, grant, "wrfcoin-test").unwrap();
        assert!(verify_stateless(&payload.action, "wrfcoin-test").is_ok());
        assert!(verify_stateless(&payload.action, "other-chain").is_err());
    }

    #[test]
    fn empty_target_list_is_refused_before_signing() {
        let mut a = args();
        a.any_target = false;
        let session_public_key = PublicKey::from_private_key(&key(2));
        assert!(build_grant(&a, session_public_key).is_err());
    }
}
