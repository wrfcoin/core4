use ethers_core::types::{H160, H256, U256};
use sha3::{Digest, Keccak256};

#[derive(Debug, Clone)]
pub struct EvmTransaction {
    from: H160,
    to: Option<H160>,
    value: U256,
    data: Vec<u8>,
    gas_limit: u64,
    gas_price: U256,
    nonce: U256,
    chain_id: u64,
}

impl EvmTransaction {
    pub fn new(
        from: H160,
        to: Option<H160>,
        value: U256,
        data: Vec<u8>,
        gas_limit: u64,
        gas_price: U256,
        chain_id: u64,
    ) -> Self {
        Self {
            from,
            to,
            value,
            data,
            gas_limit,
            gas_price,
            nonce: U256::zero(),
            chain_id,
        }
    }

    pub fn from(&self) -> H160 {
        self.from
    }

    pub fn to(&self) -> Option<H160> {
        self.to
    }

    pub fn value(&self) -> U256 {
        self.value
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    pub fn gas_price(&self) -> U256 {
        self.gas_price
    }

    pub fn nonce(&self) -> U256 {
        self.nonce
    }

    pub fn set_nonce(&mut self, nonce: U256) {
        self.nonce = nonce;
    }

    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    pub fn is_contract_creation(&self) -> bool {
        self.to.is_none()
    }

    pub fn hash(&self) -> H256 {
        let mut hasher = Keccak256::new();
        hasher.update(self.from.as_bytes());
        if let Some(to) = self.to {
            hasher.update(to.as_bytes());
        }
        let mut buf = [0u8; 32];
        self.value.to_big_endian(&mut buf);
        hasher.update(buf);
        let mut gas_price = [0u8; 32];
        self.gas_price.to_big_endian(&mut gas_price);
        hasher.update(gas_price);
        hasher.update(self.gas_limit.to_be_bytes());
        let mut nonce = [0u8; 32];
        self.nonce.to_big_endian(&mut nonce);
        hasher.update(nonce);
        hasher.update(self.chain_id.to_be_bytes());
        hasher.update(&self.data);
        H256::from_slice(&hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::EvmTransaction;
    use ethers_core::types::{H160, U256};

    #[test]
    fn hash_is_deterministic() {
        let tx = EvmTransaction::new(
            H160::from_low_u64_be(1),
            Some(H160::from_low_u64_be(2)),
            U256::from(7u64),
            vec![1, 2, 3],
            21_000,
            U256::from(1u64),
            2025,
        );

        assert_eq!(tx.hash(), tx.hash());
    }
}
