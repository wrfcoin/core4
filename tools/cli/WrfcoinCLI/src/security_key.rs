//! Security-key wallets from the CLI (`security_key_spend`; design
//! `docs/design/security-key-wallets.md`).
//!
//! The chain verifies a WebAuthn assertion, so the CLI talks to the key the way
//! a browser would, through libfido2's command-line tools (`fido2-token`,
//! `fido2-cred`, `fido2-assert`; packaged as `fido2-tools` / `libfido2` on
//! Linux, macOS and Windows). PKCS#11 (YubiKey PIV) is deliberately not used:
//! PIV signs bare ECDSA, which is not a WebAuthn assertion.
//!
//! The CLI builds `clientDataJSON` itself (`type: webauthn.get`, the chain's
//! challenge, origin `wrfcoin-cli`), hands its SHA-256 to `fido2-assert`, and
//! assembles the assertion the chain checks. The RP ID defaults to
//! `wrfcoin.com`, so a key enrolled here also works in the web wallet.
//!
//! Nothing here reads, prints or stores the phrase; the PIN prompt belongs to
//! the libfido2 tool and goes straight to the terminal.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use sha2::{Digest, Sha256};
use wrfcoin_shared_protocol::accounts::passkey::{P256Signature, base64url_nopad};
use wrfcoin_shared_protocol::accounts::security_key::{SecurityKeyAssertionWire, SecurityKeyWire};

/// Default relying party: the web wallet's, so one enrollment serves both.
pub const DEFAULT_RP_ID: &str = "wrfcoin.com";
/// `origin` the CLI writes into `clientDataJSON` (the chain does not read it).
pub const CLI_ORIGIN: &str = "wrfcoin-cli";
/// Environment override for the libfido2 tools directory.
pub const TOOLS_DIR_ENV: &str = "WRFCOIN_FIDO2_TOOLS";

/// How to reach the key.
#[derive(Clone, Debug)]
pub struct Fido2Tools {
    /// Directory holding `fido2-*`; `None` = search `PATH`.
    pub dir: Option<PathBuf>,
    /// Device path (`/dev/hidraw0`, `ioreg://…`, `windows://hello`); `None`
    /// = the first device `fido2-token -L` lists.
    pub device: Option<String>,
    pub rp_id: String,
}

impl Fido2Tools {
    pub fn new(device: Option<String>, rp_id: Option<String>) -> Self {
        Self {
            // determinism-ok: CLI-side tool location only; never consensus input
            dir: std::env::var_os(TOOLS_DIR_ENV).map(PathBuf::from),
            device,
            rp_id: rp_id.unwrap_or_else(|| DEFAULT_RP_ID.to_string()),
        }
    }

    pub fn rp_id_hash(&self) -> [u8; 32] {
        Sha256::digest(self.rp_id.as_bytes()).into()
    }

    fn tool(&self, name: &str) -> PathBuf {
        let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
        match &self.dir {
            Some(dir) => dir.join(file),
            None => PathBuf::from(file),
        }
    }

    fn run(&self, name: &str, args: &[&str], input: &str) -> Result<String> {
        let mut child = Command::new(self.tool(name))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // The tool prompts for the PIN on the terminal and reports touch
            // timeouts on stderr: let the user see both.
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| {
                format!(
                    "could not run {name}: install libfido2's tools (fido2-tools) or set {TOOLS_DIR_ENV}"
                )
            })?;
        child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("{name}: no stdin"))?
            .write_all(input.as_bytes())?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!("{name} failed ({}); is the key plugged in, touched, and the PIN right?", output.status);
        }
        String::from_utf8(output.stdout).map_err(|_| anyhow!("{name}: output is not UTF-8"))
    }

    /// The device to use: the configured one, else the first one listed.
    pub fn device(&self) -> Result<String> {
        if let Some(device) = &self.device {
            return Ok(device.clone());
        }
        let listing = self.run("fido2-token", &["-L"], "")?;
        first_device(&listing).ok_or_else(|| anyhow!("no FIDO2 security key found (fido2-token -L listed none)"))
    }

    /// Create a new credential on the key (touch + PIN). Returns the wire key
    /// to register.
    pub fn make_credential(&self, user_name: &str, user_id: &[u8]) -> Result<SecurityKeyWire> {
        let device = self.device()?;
        let mut random = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut random);
        let b64 = base64::engine::general_purpose::STANDARD;
        let input = format!(
            "{}\n{}\n{}\n{}\n",
            b64.encode(Sha256::digest(random)),
            self.rp_id,
            user_name,
            b64.encode(user_id)
        );
        let output = self.run("fido2-cred", &["-M", "-v", &device, "es256"], &input)?;
        let key = parse_make_credential_output(&output)?;
        if key.rp_id_hash != hex::encode(self.rp_id_hash()) {
            bail!("the key answered for a different relying party");
        }
        Ok(key)
    }

    /// Ask the key for an assertion over `challenge` (touch + PIN) with the
    /// credential `credential_id_hex`, for the registered public key
    /// `public_key_hex`.
    pub fn assert(&self, public_key_hex: &str, credential_id_hex: &str, challenge: &[u8; 32]) -> Result<SecurityKeyAssertionWire> {
        let device = self.device()?;
        let client_data_json = client_data_json(challenge);
        let b64 = base64::engine::general_purpose::STANDARD;
        let credential_handle = hex::decode(credential_id_hex).context("credential id is not hex")?;
        let input = format!( // secret-leak-ok: a WebAuthn credential id is a public handle (WebAuthn §5.1), not secret material
            "{}\n{}\n{}\n",
            b64.encode(Sha256::digest(&client_data_json)),
            self.rp_id,
            b64.encode(credential_handle)
        );
        let output = self.run("fido2-assert", &["-G", "-p", "-v", &device], &input)?;
        let (authenticator_data, signature) = parse_get_assertion_output(&output)?;
        assertion_wire(public_key_hex, &authenticator_data, &client_data_json, &signature)
    }
}

/// `clientDataJSON` for a `webauthn.get` over `challenge`.
pub fn client_data_json(challenge: &[u8; 32]) -> Vec<u8> {
    format!(
        "{{\"type\":\"webauthn.get\",\"challenge\":\"{}\",\"origin\":\"{CLI_ORIGIN}\",\"crossOrigin\":false}}",
        base64url_nopad(challenge)
    )
    .into_bytes()
}

/// Assemble the wire assertion, normalising the signature to low-s (the chain
/// refuses high-s; every authenticator signature is equally valid normalised).
pub fn assertion_wire(
    public_key_hex: &str,
    authenticator_data: &[u8],
    client_data_json: &[u8],
    der_signature: &[u8],
) -> Result<SecurityKeyAssertionWire> {
    let signature = P256Signature::from_der(der_signature)
        .map_err(|_| anyhow!("the key returned a malformed signature"))?
        .normalized()
        .to_der_bytes();
    Ok(SecurityKeyAssertionWire {
        public_key_p256: public_key_hex.to_string(),
        authenticator_data: hex::encode(authenticator_data),
        client_data_json: hex::encode(client_data_json),
        signature: hex::encode(signature),
    })
}

/// First device path in `fido2-token -L` output (`<path>: vendor=…`).
pub fn first_device(listing: &str) -> Option<String> {
    listing
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .and_then(|line| line.split(": ").next())
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
}

/// libfido2 prints authenticator data CBOR-wrapped (a byte string). Unwrap it
/// when it is; pass raw bytes through.
pub fn unwrap_cbor_bytes(data: &[u8]) -> Vec<u8> {
    let (header, len) = match data.first() {
        Some(&b) if (0x40..=0x57).contains(&b) => (1usize, (b - 0x40) as usize),
        Some(0x58) if data.len() >= 2 => (2, data[1] as usize),
        Some(0x59) if data.len() >= 3 => (3, u16::from_be_bytes([data[1], data[2]]) as usize),
        _ => return data.to_vec(),
    };
    if data.len() == header + len {
        data[header..].to_vec()
    } else {
        data.to_vec()
    }
}

fn b64_line(line: Option<&str>, what: &str) -> Result<Vec<u8>> {
    let line = line.ok_or_else(|| anyhow!("fido2 output is missing the {what}"))?.trim();
    base64::engine::general_purpose::STANDARD
        .decode(line)
        .map_err(|_| anyhow!("fido2 output: {what} is not base64"))
}

/// `fido2-assert -G` output: client data hash, rp id, authenticator data,
/// signature (each base64 except the rp id), then optional lines.
pub fn parse_get_assertion_output(output: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut lines = output.lines();
    let _client_data_hash = lines.next();
    let _rp_id = lines.next();
    let authenticator_data = unwrap_cbor_bytes(&b64_line(lines.next(), "authenticator data")?);
    let signature = b64_line(lines.next(), "signature")?;
    Ok((authenticator_data, signature))
}

/// `fido2-cred -M` output: client data hash, rp id, format, authenticator data,
/// credential id, … The public key comes from the attested credential data in
/// the authenticator data (COSE EC2 / ES256).
pub fn parse_make_credential_output(output: &str) -> Result<SecurityKeyWire> {
    let mut lines = output.lines();
    let _client_data_hash = lines.next();
    let _rp_id = lines.next();
    let _format = lines.next();
    let authenticator_data = unwrap_cbor_bytes(&b64_line(lines.next(), "authenticator data")?);
    let (rp_id_hash, credential_id, xy) = parse_attested_credential(&authenticator_data)?;
    Ok(SecurityKeyWire {
        public_key_p256: hex::encode(xy),
        rp_id_hash: hex::encode(rp_id_hash),
        credential_id: hex::encode(credential_id),
    })
}

/// rpIdHash(32) flags(1) signCount(4) aaguid(16) credIdLen(2) credId COSE_Key.
pub fn parse_attested_credential(auth: &[u8]) -> Result<([u8; 32], Vec<u8>, [u8; 64])> {
    const AT: u8 = 0x40;
    if auth.len() < 37 + 16 + 2 || auth[32] & AT == 0 {
        bail!("authenticator data carries no attested credential");
    }
    let rp_id_hash: [u8; 32] = auth[..32].try_into().map_err(|_| anyhow!("rp id hash"))?;
    let id_len = u16::from_be_bytes([auth[53], auth[54]]) as usize;
    let id_end = 55 + id_len;
    if auth.len() < id_end || id_len == 0 {
        bail!("authenticator data credential id is truncated");
    }
    let credential_id = auth[55..id_end].to_vec();
    let xy = parse_cose_es256(&auth[id_end..])?;
    Ok((rp_id_hash, credential_id, xy))
}

/// Minimal COSE_Key reader for EC2 / ES256 / P-256:
/// `{1: 2, 3: -7, -1: 1, -2: x(32), -3: y(32)}` in any order.
pub fn parse_cose_es256(cose: &[u8]) -> Result<[u8; 64]> {
    let mut at = 0usize;
    let mut next = || -> Result<u8> {
        let b = *cose.get(at).ok_or_else(|| anyhow!("COSE key is truncated"))?;
        at += 1;
        Ok(b)
    };
    let head = next()?;
    if !(0xA0..=0xB7).contains(&head) {
        bail!("COSE key is not a small map");
    }
    let entries = head - 0xA0;
    let (mut kty, mut alg, mut crv) = (None, None, None);
    let (mut x, mut y) = (None, None);
    for _ in 0..entries {
        let key = read_small_int(&mut next)?;
        let first = next()?;
        match first {
            0x58 => {
                let len = next()? as usize;
                let mut bytes = Vec::with_capacity(len);
                for _ in 0..len {
                    bytes.push(next()?);
                }
                match key {
                    -2 => x = Some(bytes),
                    -3 => y = Some(bytes),
                    _ => {},
                }
            },
            _ => {
                let value = small_int_from(first, &mut next)?;
                match key {
                    1 => kty = Some(value),
                    3 => alg = Some(value),
                    -1 => crv = Some(value),
                    _ => {},
                }
            },
        }
    }
    if kty != Some(2) || alg != Some(-7) || crv != Some(1) {
        bail!("the key did not create an ES256 (P-256) credential");
    }
    let (Some(x), Some(y)) = (x, y) else { bail!("COSE key is missing x or y") };
    if x.len() != 32 || y.len() != 32 {
        bail!("COSE key coordinates are not 32 bytes");
    }
    let mut xy = [0u8; 64];
    xy[..32].copy_from_slice(&x);
    xy[32..].copy_from_slice(&y);
    Ok(xy)
}

fn read_small_int(next: &mut impl FnMut() -> Result<u8>) -> Result<i64> {
    let first = next()?;
    small_int_from(first, next)
}

fn small_int_from(first: u8, next: &mut impl FnMut() -> Result<u8>) -> Result<i64> {
    match first {
        0x00..=0x17 => Ok(first as i64),
        0x18 => Ok(next()? as i64),
        0x20..=0x37 => Ok(-1 - (first - 0x20) as i64),
        0x38 => Ok(-1 - next()? as i64),
        _ => bail!("unsupported COSE value"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
    use wrfcoin_shared_protocol::accounts::security_key::{RegisteredSecurityKey, SecurityKeyWire};

    fn cose(xy: &[u8; 64]) -> Vec<u8> {
        let mut out = vec![0xA5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20];
        out.extend_from_slice(&xy[..32]);
        out.extend_from_slice(&[0x22, 0x58, 0x20]);
        out.extend_from_slice(&xy[32..]);
        out
    }

    fn cbor_bstr(bytes: &[u8]) -> Vec<u8> {
        let mut out = if bytes.len() < 256 { vec![0x58, bytes.len() as u8] } else {
            let mut v = vec![0x59];
            v.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
            v
        };
        out.extend_from_slice(bytes);
        out
    }

    #[test]
    fn make_credential_output_yields_the_registered_key() {
        let auth = SoftwareAuthenticator::new(&[0x55; 32], DEFAULT_RP_ID);
        let xy = auth.key.public_key().to_raw_xy();
        let mut data = auth.rp_id_hash.to_vec();
        data.push(0x45); // UP | UV | AT
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&[0u8; 16]);
        let credential_id = [0xC1u8; 64];
        data.extend_from_slice(&(credential_id.len() as u16).to_be_bytes());
        data.extend_from_slice(&credential_id);
        data.extend_from_slice(&cose(&xy));
        let b64 = base64::engine::general_purpose::STANDARD;
        let text = format!( // secret-leak-ok: test fixture; a WebAuthn credential id is a public handle
            "{}\n{}\npacked\n{}\n{}\nsig\n",
            b64.encode([0u8; 32]),
            DEFAULT_RP_ID,
            b64.encode(cbor_bstr(&data)),
            b64.encode(credential_id)
        );
        let key = parse_make_credential_output(&text).unwrap();
        assert_eq!(key.public_key_p256, hex::encode(xy));
        assert_eq!(key.credential_id, hex::encode(credential_id)); // secret-leak-ok: public WebAuthn credential handle in a test
        assert_eq!(key.rp_id_hash, hex::encode(Fido2Tools::new(None, None).rp_id_hash()));
        assert!(key.decode().is_ok(), "the chain accepts the parsed key");
    }

    /// End to end against the chain's verifier: a CLI-built assertion (CLI
    /// clientDataJSON, libfido2-shaped CBOR-wrapped authenticator data, a
    /// possibly high-s DER signature) verifies for the spend challenge.
    #[test]
    fn a_cli_built_assertion_verifies_on_chain() {
        let auth = SoftwareAuthenticator::new(&[0x56; 32], DEFAULT_RP_ID);
        let xy = auth.key.public_key().to_raw_xy();
        let challenge = [0x77u8; 32];
        let cdj = client_data_json(&challenge);
        let raw = auth.assert_with(auth.authenticator_data(), cdj.clone());
        // Make the signature high-s to prove normalisation.
        let high = P256Signature::from_der(&raw.signature).unwrap().malleable_twin().unwrap();
        let b64 = base64::engine::general_purpose::STANDARD;
        let output = format!(
            "{}\n{}\n{}\n{}\n",
            b64.encode(Sha256::digest(&cdj)),
            DEFAULT_RP_ID,
            b64.encode(cbor_bstr(&raw.authenticator_data)),
            b64.encode(high.to_der_bytes())
        );
        let (authenticator_data, signature) = parse_get_assertion_output(&output).unwrap();
        assert_eq!(authenticator_data, raw.authenticator_data);
        let wire = assertion_wire(&hex::encode(xy), &authenticator_data, &cdj, &signature).unwrap();
        let registered: RegisteredSecurityKey = SecurityKeyWire {
            public_key_p256: hex::encode(xy),
            rp_id_hash: hex::encode(auth.rp_id_hash),
            credential_id: "01".into(),
        }
        .decode()
        .unwrap();
        wire.verify(&registered, &challenge).unwrap();
        assert!(wire.verify(&registered, &[0x78; 32]).is_err());
    }

    #[test]
    fn helpers_handle_edge_shapes() {
        assert_eq!(first_device("/dev/hidraw3: vendor=0x1050, product=0x0407 (Yubico YubiKey OTP+FIDO+CCID)\n"), Some("/dev/hidraw3".into()));
        assert_eq!(first_device("\n"), None);
        assert_eq!(unwrap_cbor_bytes(&[0x43, 1, 2, 3]), vec![1, 2, 3]);
        assert_eq!(unwrap_cbor_bytes(&[1, 2, 3]), vec![1, 2, 3]);
        assert!(parse_cose_es256(&[0xA1, 0x01, 0x01]).is_err());
        assert!(parse_get_assertion_output("a\nb\n").is_err());
    }
}
