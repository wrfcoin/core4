use crate::common::types::{Address, Balance};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LiquidStakingError {
    #[error("amount must be greater than zero")]
    ZeroAmount,
    #[error("insufficient pool liquidity")]
    InsufficientLiquidity,
    #[error("math overflow during staking operation")]
    MathOverflow,
}

#[derive(Debug, Clone)]
pub struct StakingPosition {
    pub owner: Address,
    pub wrf_staked: Balance,
    pub stwrf_minted: Balance,
    pub role_weight_bps: u16,
}

pub trait RoleWeightProvider: Send + Sync {
    fn role_weight_bps(&self, _owner: &Address) -> u16 {
        10_000
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultRoleWeightProvider;

impl RoleWeightProvider for DefaultRoleWeightProvider {}

#[derive(Debug, Default, Clone)]
pub struct LiquidStakingPool {
    pub total_wrf_staked: Balance,
    pub total_stwrf_supply: Balance,
}

impl LiquidStakingPool {
    pub fn exchange_rate(&self) -> (Balance, Balance) {
        if self.total_wrf_staked == 0 || self.total_stwrf_supply == 0 {
            (Balance::new(1), Balance::new(1))
        } else {
            (self.total_wrf_staked, self.total_stwrf_supply)
        }
    }

    pub fn preview_mint(&self, wrf_amount: Balance) -> Result<Balance, LiquidStakingError> {
        if wrf_amount == 0 {
            return Err(LiquidStakingError::ZeroAmount);
        }

        if self.total_wrf_staked == 0 || self.total_stwrf_supply == 0 {
            return Ok(wrf_amount);
        }

        wrf_amount
            .checked_mul(self.total_stwrf_supply)
            .and_then(|v| v.checked_div(self.total_wrf_staked))
            .ok_or(LiquidStakingError::MathOverflow)
    }

    pub fn deposit(
        &mut self,
        owner: Address,
        wrf_amount: Balance,
        role_weight_bps: u16,
    ) -> Result<StakingPosition, LiquidStakingError> {
        let stwrf_minted = self.preview_mint(wrf_amount)?;
        self.total_wrf_staked = self
            .total_wrf_staked
            .checked_add(wrf_amount)
            .ok_or(LiquidStakingError::MathOverflow)?;
        self.total_stwrf_supply = self
            .total_stwrf_supply
            .checked_add(stwrf_minted)
            .ok_or(LiquidStakingError::MathOverflow)?;

        Ok(StakingPosition {
            owner,
            wrf_staked: wrf_amount,
            stwrf_minted,
            role_weight_bps,
        })
    }

    pub fn preview_redeem(&self, stwrf_amount: Balance) -> Result<Balance, LiquidStakingError> {
        if stwrf_amount == 0 {
            return Err(LiquidStakingError::ZeroAmount);
        }

        if stwrf_amount > self.total_stwrf_supply {
            return Err(LiquidStakingError::InsufficientLiquidity);
        }

        stwrf_amount
            .checked_mul(self.total_wrf_staked)
            .and_then(|v| v.checked_div(self.total_stwrf_supply.max(Balance::new(1))))
            .ok_or(LiquidStakingError::MathOverflow)
    }

    pub fn redeem(&mut self, stwrf_amount: Balance) -> Result<Balance, LiquidStakingError> {
        let wrf_out = self.preview_redeem(stwrf_amount)?;
        self.total_stwrf_supply = self
            .total_stwrf_supply
            .checked_sub(stwrf_amount)
            .ok_or(LiquidStakingError::InsufficientLiquidity)?;
        self.total_wrf_staked = self
            .total_wrf_staked
            .checked_sub(wrf_out)
            .ok_or(LiquidStakingError::InsufficientLiquidity)?;
        Ok(wrf_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    #[test]
    fn initial_deposit_mints_one_to_one() {
        let mut pool = LiquidStakingPool::default();
        let position = pool.deposit(addr(1), Balance::new(1_000), 10_000).unwrap();
        assert_eq!(position.stwrf_minted, Balance::new(1_000));
        assert_eq!(pool.total_wrf_staked, Balance::new(1_000));
        assert_eq!(pool.total_stwrf_supply, Balance::new(1_000));
    }

    #[test]
    fn redeem_reduces_pool_balances() {
        let mut pool = LiquidStakingPool::default();
        pool.deposit(addr(2), Balance::new(2_000), 10_000).unwrap();
        let wrf_out = pool.redeem(Balance::new(500)).unwrap();
        assert_eq!(wrf_out, Balance::new(500));
        assert_eq!(pool.total_wrf_staked, Balance::new(1_500));
        assert_eq!(pool.total_stwrf_supply, Balance::new(1_500));
    }

    #[test]
    fn preview_mint_overflow_returns_math_overflow() {
        let pool = LiquidStakingPool {
            total_wrf_staked: Balance::new(1),
            total_stwrf_supply: Balance::new(u128::MAX),
        };
        let err = pool.preview_mint(Balance::new(2)).unwrap_err();
        assert!(matches!(err, LiquidStakingError::MathOverflow));
    }

    #[test]
    fn preview_redeem_overflow_returns_math_overflow() {
        let pool = LiquidStakingPool {
            total_wrf_staked: Balance::new(u128::MAX),
            total_stwrf_supply: Balance::new(2),
        };
        let err = pool.preview_redeem(Balance::new(2)).unwrap_err();
        assert!(matches!(err, LiquidStakingError::MathOverflow));
    }
}
