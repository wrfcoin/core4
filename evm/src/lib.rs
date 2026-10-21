mod runtime;
mod state;
mod transaction;

pub use runtime::EvmRuntime;
pub use state::EvmState;
pub use transaction::EvmTransaction;

use anyhow::Result;
use ethers_core::types::{H160, H256, U256};

#[derive(Debug, Default)]
pub struct EvmCompatibility {
    state: EvmState,
    runtime: EvmRuntime,
}

impl EvmCompatibility {
    pub fn new() -> Self {
        Self {
            state: EvmState::new(),
            runtime: EvmRuntime::new(),
        }
    }

    pub fn deploy_contract(
        &mut self,
        deployer: H160,
        code: Vec<u8>,
        value: U256,
        gas_limit: u64,
    ) -> Result<H160> {
        self.runtime
            .deploy_contract(&mut self.state, deployer, code, value, gas_limit)
    }

    pub fn execute_function(
        &mut self,
        caller: H160,
        contract_address: H160,
        function_selector: [u8; 4],
        arguments: Vec<u8>,
        value: U256,
        gas_limit: u64,
    ) -> Result<Vec<u8>> {
        let mut input = Vec::with_capacity(4 + arguments.len());
        input.extend_from_slice(&function_selector);
        input.extend_from_slice(&arguments);
        self.runtime
            .execute_call(&mut self.state, caller, contract_address, input, value, gas_limit)
    }

    pub fn state(&self) -> &EvmState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut EvmState {
        &mut self.state
    }

    pub fn create_transaction(
        &self,
        from: H160,
        to: Option<H160>,
        value: U256,
        data: Vec<u8>,
        gas_limit: u64,
        gas_price: U256,
    ) -> EvmTransaction {
        EvmTransaction::new(from, to, value, data, gas_limit, gas_price, self.runtime.chain_id())
    }

    pub fn apply_transaction(&mut self, transaction: EvmTransaction) -> Result<H256> {
        self.runtime.apply_transaction(&mut self.state, transaction)
    }
}

pub fn init_evm_compatibility() -> EvmCompatibility {
    EvmCompatibility::new()
}

#[cfg(test)]
mod tests {
    use super::{EvmCompatibility, init_evm_compatibility};
    use ethers_core::types::{H160, U256};

    fn alice() -> H160 {
        H160::from_low_u64_be(0xA1)
    }

    #[test]
    fn init_returns_empty_runtime() {
        let evm = init_evm_compatibility();
        assert_eq!(evm.state().all_balances().len(), 0);
    }

    #[test]
    fn deploy_and_execute_round_trips_input() {
        let mut evm = EvmCompatibility::new();
        let deployer = alice();
        evm.state_mut().create_account(deployer, U256::zero());

        let address = evm
            .deploy_contract(deployer, vec![0xde, 0xad, 0xbe, 0xef], U256::zero(), 500_000)
            .expect("deploy should succeed");

        let output = evm
            .execute_function(
                deployer,
                address,
                [0xaa, 0xbb, 0xcc, 0xdd],
                vec![1, 2, 3, 4],
                U256::zero(),
                500_000,
            )
            .expect("call should succeed");

        assert_eq!(output, vec![0xaa, 0xbb, 0xcc, 0xdd, 1, 2, 3, 4]);
    }

    #[test]
    fn apply_transaction_transfers_value_and_increments_nonce() {
        let mut evm = EvmCompatibility::new();
        let from = H160::from_low_u64_be(1);
        let to = H160::from_low_u64_be(2);
        evm.state_mut().set_balance(from, U256::from(25u64));
        evm.state_mut().create_account(to, U256::zero());

        let tx = evm.create_transaction(
            from,
            Some(to),
            U256::from(10u64),
            Vec::new(),
            10,
            U256::from(1u64),
        );

        evm.apply_transaction(tx).expect("transaction should succeed");

        // from: started with 25, sent 10 value + 10 gas fee = 5 remaining
        assert_eq!(evm.state().balance(from), U256::from(5u64));
        assert_eq!(evm.state().balance(to), U256::from(10u64));
        assert_eq!(evm.state().nonce(from), U256::from(1u64));
    }
}
