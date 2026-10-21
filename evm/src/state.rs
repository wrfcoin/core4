use std::collections::{BTreeMap, HashMap};

use ethers_core::types::{H160, H256, U256};

#[derive(Debug, Clone, Default)]
pub struct EvmState {
    balances: HashMap<H160, U256>,
    nonces: HashMap<H160, U256>,
    code: HashMap<H160, Vec<u8>>,
    storage: HashMap<H160, BTreeMap<H256, H256>>,
}

impl EvmState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn balance(&self, address: H160) -> U256 {
        self.balances.get(&address).copied().unwrap_or_default()
    }

    pub fn set_balance(&mut self, address: H160, balance: U256) {
        self.balances.insert(address, balance);
    }

    pub fn all_balances(&self) -> &HashMap<H160, U256> {
        &self.balances
    }

    pub fn nonce(&self, address: H160) -> U256 {
        self.nonces.get(&address).copied().unwrap_or_default()
    }

    pub fn increment_nonce(&mut self, address: H160) {
        let current = self.nonce(address);
        self.nonces.insert(address, current + U256::one());
    }

    pub fn code(&self, address: H160) -> Vec<u8> {
        self.code.get(&address).cloned().unwrap_or_default()
    }

    pub fn set_code(&mut self, address: H160, code: Vec<u8>) {
        self.code.insert(address, code);
    }

    pub fn storage(&self, address: H160, index: H256) -> H256 {
        self.storage
            .get(&address)
            .and_then(|entries| entries.get(&index))
            .copied()
            .unwrap_or_default()
    }

    pub fn set_storage(&mut self, address: H160, index: H256, value: H256) {
        self.storage
            .entry(address)
            .or_default()
            .insert(index, value);
    }

    pub fn account_exists(&self, address: H160) -> bool {
        self.balances.contains_key(&address)
            || self.nonces.contains_key(&address)
            || self.code.contains_key(&address)
    }

    pub fn create_account(&mut self, address: H160, balance: U256) {
        self.balances.insert(address, balance);
        self.nonces.entry(address).or_insert(U256::zero());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(byte: u8) -> H160 {
        H160::from_low_u64_be(byte as u64)
    }

    fn slot(byte: u8) -> H256 {
        H256::from_low_u64_be(byte as u64)
    }

    #[test]
    fn balance_set_get_round_trip() {
        let mut state = EvmState::new();
        let account = addr(1);
        state.set_balance(account, U256::from(42u64));

        assert_eq!(state.balance(account), U256::from(42u64));
        assert_eq!(state.all_balances().len(), 1);
    }

    #[test]
    fn nonce_increments_from_zero() {
        let mut state = EvmState::new();
        let account = addr(2);

        assert_eq!(state.nonce(account), U256::zero());
        state.increment_nonce(account);
        state.increment_nonce(account);

        assert_eq!(state.nonce(account), U256::from(2u64));
    }

    #[test]
    fn code_set_get_round_trip() {
        let mut state = EvmState::new();
        let account = addr(3);
        let bytecode = vec![0x60, 0x00, 0x52];
        state.set_code(account, bytecode.clone());

        assert_eq!(state.code(account), bytecode);
        assert!(state.account_exists(account));
    }

    #[test]
    fn storage_slots_are_isolated_by_account() {
        let mut state = EvmState::new();
        let account_a = addr(4);
        let account_b = addr(5);
        let key = slot(9);
        let value_a = slot(11);
        let value_b = slot(12);

        state.set_storage(account_a, key, value_a);
        state.set_storage(account_b, key, value_b);

        assert_eq!(state.storage(account_a, key), value_a);
        assert_eq!(state.storage(account_b, key), value_b);
    }

    #[test]
    fn create_account_initializes_balance_and_nonce() {
        let mut state = EvmState::new();
        let account = addr(6);
        state.create_account(account, U256::from(1_000u64));

        assert!(state.account_exists(account));
        assert_eq!(state.balance(account), U256::from(1_000u64));
        assert_eq!(state.nonce(account), U256::zero());
    }
}
