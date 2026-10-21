use anyhow::{Result, ensure};
use ethers_core::types::{H160, H256, U256};
use sha3::{Digest, Keccak256};

use crate::{EvmState, EvmTransaction};

#[derive(Debug, Clone)]
pub struct EvmRuntime {
    gas_price: U256,
    chain_id: u64,
}

impl Default for EvmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl EvmRuntime {
    pub fn new() -> Self {
        Self {
            gas_price: U256::from(1u64),
            chain_id: 2025,
        }
    }

    pub fn deploy_contract(
        &mut self,
        state: &mut EvmState,
        deployer: H160,
        code: Vec<u8>,
        value: U256,
        gas_limit: u64,
    ) -> Result<H160> {
        ensure!(gas_limit > 0, "gas_limit must be non-zero");
        let deployer_nonce = state.nonce(deployer);
        let address = derive_create_address(deployer, deployer_nonce);

        // Value transfer: debit deployer, credit new contract (#1207).
        if !value.is_zero() {
            let deployer_bal = state.balance(deployer);
            ensure!(deployer_bal >= value, "insufficient balance for deploy value");
            state.set_balance(deployer, deployer_bal - value);
        }

        state.create_account(address, value);
        state.set_code(address, code);
        Ok(address)
    }

    pub fn execute_call(
        &self,
        state: &mut EvmState,
        caller: H160,
        contract_address: H160,
        input: Vec<u8>,
        value: U256,
        gas_limit: u64,
    ) -> Result<Vec<u8>> {
        ensure!(gas_limit > 0, "gas_limit must be non-zero");
        ensure!(
            state.account_exists(contract_address),
            "contract account does not exist"
        );

        // Value transfer: debit caller, credit contract (#1207).
        if !value.is_zero() {
            let caller_bal = state.balance(caller);
            ensure!(caller_bal >= value, "insufficient balance for call value");
            state.set_balance(caller, caller_bal - value);
            let contract_bal = state.balance(contract_address);
            state.set_balance(contract_address, contract_bal + value);
        }

        let storage_key = H256::zero();
        let call_hash = H256::from_slice(&Keccak256::digest(&input));
        state.set_storage(contract_address, storage_key, call_hash);
        Ok(input)
    }

    pub fn apply_transaction(
        &mut self,
        state: &mut EvmState,
        mut transaction: EvmTransaction,
    ) -> Result<H256> {
        let from = transaction.from();
        state.create_account(from, state.balance(from));
        let current_nonce = state.nonce(from);
        transaction.set_nonce(current_nonce);

        let fee = self.estimated_fee(&transaction);
        let total_cost = transaction.value() + fee;
        ensure!(
            state.balance(from) >= total_cost,
            "insufficient balance for value plus fee"
        );

        // Deduct only the gas fee here; value transfer is handled inside
        // execute_call / deploy_contract so the accounting is centralised (#1207).
        state.set_balance(from, state.balance(from) - fee);

        if let Some(to) = transaction.to() {
            state.create_account(to, state.balance(to));
            self.execute_call(
                state,
                from,
                to,
                transaction.data().to_vec(),
                transaction.value(),
                transaction.gas_limit(),
            )?;
        } else {
            self.deploy_contract(
                state,
                from,
                transaction.data().to_vec(),
                transaction.value(),
                transaction.gas_limit(),
            )?;
        }

        state.increment_nonce(from);
        Ok(transaction.hash())
    }

    pub fn gas_price(&self) -> U256 {
        self.gas_price
    }

    pub fn set_gas_price(&mut self, gas_price: U256) {
        self.gas_price = gas_price;
    }

    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    fn estimated_fee(&self, transaction: &EvmTransaction) -> U256 {
        let gas_limit = U256::from(transaction.gas_limit());
        transaction.gas_price() * gas_limit
    }

}

/// Spec-compliant EVM CREATE address derivation (Yellow Paper §7):
///   `keccak256(RLP([sender, nonce]))[12:]`
///
/// Implements just enough RLP for the two-element list `[H160, U256]`.
fn derive_create_address(sender: H160, nonce: U256) -> H160 {
    let rlp_sender = rlp_encode_bytes(sender.as_bytes());
    let rlp_nonce = rlp_encode_u256(nonce);

    let payload_len = rlp_sender.len() + rlp_nonce.len();
    let mut encoded = Vec::with_capacity(1 + payload_len);
    // Short list prefix: 0xc0 + payload length (payload < 56 bytes for our case)
    encoded.push(0xc0 + payload_len as u8);
    encoded.extend_from_slice(&rlp_sender);
    encoded.extend_from_slice(&rlp_nonce);

    let digest = Keccak256::digest(&encoded);
    H160::from_slice(&digest[12..])
}

/// RLP-encode a fixed byte slice (for the 20-byte address).
fn rlp_encode_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + data.len());
    out.push(0x80 + data.len() as u8);
    out.extend_from_slice(data);
    out
}

/// RLP-encode a U256 nonce value.
fn rlp_encode_u256(val: U256) -> Vec<u8> {
    if val.is_zero() {
        return vec![0x80]; // RLP for empty byte string (nonce 0)
    }
    let mut buf = [0u8; 32];
    val.to_big_endian(&mut buf);
    // Strip leading zeros to get the minimal big-endian encoding.
    let start = buf.iter().position(|&b| b != 0).unwrap_or(31);
    let bytes = &buf[start..];
    if bytes.len() == 1 && bytes[0] < 0x80 {
        // Single byte < 0x80 encodes as itself.
        vec![bytes[0]]
    } else {
        let mut out = Vec::with_capacity(1 + bytes.len());
        out.push(0x80 + bytes.len() as u8);
        out.extend_from_slice(bytes);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EvmState;
    use ethers_core::types::{H160, U256};
    use ethers_core::utils::hex;

    fn deployer() -> H160 {
        H160::from_low_u64_be(0xA1)
    }

    #[test]
    fn deploy_contract_persists_code() {
        let mut runtime = EvmRuntime::new();
        let mut state = EvmState::new();
        let from = deployer();
        state.create_account(from, U256::from(100u64));
        let code = vec![1, 2, 3, 4];

        let address = runtime
            .deploy_contract(&mut state, from, code.clone(), U256::from(5u64), 100_000)
            .expect("deploy should succeed");

        assert_eq!(state.code(address), code);
        assert_eq!(state.balance(address), U256::from(5u64));
        // Deployer should have been debited
        assert_eq!(state.balance(from), U256::from(95u64));
    }

    #[test]
    fn zero_gas_limit_is_rejected() {
        let mut runtime = EvmRuntime::new();
        let mut state = EvmState::new();
        let err = runtime
            .deploy_contract(&mut state, deployer(), vec![1], U256::zero(), 0)
            .expect_err("deploy should fail");

        assert!(err.to_string().contains("gas_limit"));
    }

    // ── CREATE address derivation (#1207) ─────────────────────────────

    #[test]
    fn derive_create_address_matches_ethereum_spec() {
        // Reference: Ethereum Yellow Paper §7 — CREATE address is
        // keccak256(RLP([sender, nonce]))[12:].
        //
        // Known test vector: sender=0x6ac7ea33f8831ea9dcc53393aaa88b25a785dbf0 nonce=0
        // Expected: 0xcd234a471b72ba2f1ccf0a70fcaba648a5eecd8d
        let sender = H160::from_slice(
            &hex::decode("6ac7ea33f8831ea9dcc53393aaa88b25a785dbf0").unwrap(),
        );
        let nonce = U256::zero();
        let result = derive_create_address(sender, nonce);
        let expected = H160::from_slice(
            &hex::decode("cd234a471b72ba2f1ccf0a70fcaba648a5eecd8d").unwrap(),
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn derive_create_address_changes_with_nonce() {
        let sender = deployer();
        let addr0 = derive_create_address(sender, U256::zero());
        let addr1 = derive_create_address(sender, U256::from(1u64));
        assert_ne!(addr0, addr1, "different nonces must produce different addresses");
    }

    #[test]
    fn derive_create_address_changes_with_sender() {
        let addr_a = derive_create_address(H160::from_low_u64_be(1), U256::zero());
        let addr_b = derive_create_address(H160::from_low_u64_be(2), U256::zero());
        assert_ne!(addr_a, addr_b, "different senders must produce different addresses");
    }

    // ── Value transfer in execute_call (#1207) ────────────────────────

    #[test]
    fn execute_call_transfers_value_to_contract() {
        let runtime = EvmRuntime::new();
        let mut state = EvmState::new();
        let caller = H160::from_low_u64_be(0xB1);
        let contract = H160::from_low_u64_be(0xC1);
        state.create_account(caller, U256::from(100u64));
        state.create_account(contract, U256::zero());
        state.set_code(contract, vec![0x00]);

        runtime
            .execute_call(&mut state, caller, contract, vec![], U256::from(30u64), 100_000)
            .expect("call should succeed");

        assert_eq!(state.balance(caller), U256::from(70u64));
        assert_eq!(state.balance(contract), U256::from(30u64));
    }

    #[test]
    fn execute_call_rejects_insufficient_value() {
        let runtime = EvmRuntime::new();
        let mut state = EvmState::new();
        let caller = H160::from_low_u64_be(0xB2);
        let contract = H160::from_low_u64_be(0xC2);
        state.create_account(caller, U256::from(5u64));
        state.create_account(contract, U256::zero());
        state.set_code(contract, vec![0x00]);

        let err = runtime
            .execute_call(&mut state, caller, contract, vec![], U256::from(50u64), 100_000)
            .expect_err("should fail");
        assert!(err.to_string().contains("insufficient balance"));
    }
}
