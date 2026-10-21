//! Governance voting implementation
//!
//! This module provides the voting mechanisms for governance proposals.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Vote types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Vote {
    /// Yes vote (approve)
    Yes,

    /// No vote (reject)
    No,

    /// Abstain vote (counted for quorum but not for approval)
    Abstain,

    /// Veto vote (stronger form of No)
    Veto,
}

/// Voting power
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VotingPower {
    /// The voting power value
    pub value: u64,
}

/// Voting system interface
#[async_trait]
pub trait VotingSystem: Send + Sync {
    /// Get the voting power of an address
    async fn get_voting_power(&self, address: &str) -> Result<VotingPower>;

    /// Get the total voting power in the system
    async fn get_total_voting_power(&self) -> Result<VotingPower>;

    /// Get the voting power type (token-based, NFT-based, etc.)
    fn voting_power_type(&self) -> &'static str;
}

/// Token-based voting implementation
pub struct TokenBasedVoting {
    /// Token balances for each address
    balances: HashMap<String, u64>,

    /// Stake amounts for each address
    staked: HashMap<String, u64>,

    /// Total supply
    total_supply: u64,
}

impl TokenBasedVoting {
    /// Create a new token-based voting system
    pub fn new() -> Self {
        Self {
            balances: HashMap::new(),
            staked: HashMap::new(),
            total_supply: 0,
        }
    }

    /// Update the token balance of an address
    pub fn update_balance(&mut self, address: String, balance: u64) -> anyhow::Result<()> {
        self.balances.insert(address, balance);

        // Update total supply via checked arithmetic to prevent overflow panic
        self.total_supply = self.balances.values().try_fold(0u64, |acc, &v| {
            acc.checked_add(v).ok_or_else(|| anyhow::anyhow!("total_supply overflow"))
        })?;
        Ok(())
    }

    /// Update the staked amount of an address
    pub fn update_staked(&mut self, address: String, staked: u64) {
        self.staked.insert(address, staked);
    }
}

#[async_trait]
impl VotingSystem for TokenBasedVoting {
    async fn get_voting_power(&self, address: &str) -> Result<VotingPower> {
        // Voting power is based on staked tokens
        let staked = self.staked.get(address).cloned().unwrap_or(0);
        Ok(VotingPower { value: staked })
    }

    async fn get_total_voting_power(&self) -> Result<VotingPower> {
        // Total voting power is the sum of all staked tokens via checked arithmetic
        let total_staked = self.staked.values().try_fold(0u64, |acc, &v| {
            acc.checked_add(v).ok_or_else(|| anyhow::anyhow!("total_staked overflow"))
        })?;
        Ok(VotingPower {
            value: total_staked,
        })
    }

    fn voting_power_type(&self) -> &'static str {
        "token-based"
    }
}

/// NFT-based voting implementation
pub struct NftBasedVoting {
    /// NFT holdings for each address
    nft_holdings: HashMap<String, Vec<String>>,

    /// NFT voting weights
    nft_weights: HashMap<String, u64>,

    /// Total NFTs
    total_nfts: u64,
}

impl NftBasedVoting {
    /// Create a new NFT-based voting system
    pub fn new() -> Self {
        Self {
            nft_holdings: HashMap::new(),
            nft_weights: HashMap::new(),
            total_nfts: 0,
        }
    }

    /// Update the NFT holdings of an address
    pub fn update_holdings(&mut self, address: String, nfts: Vec<String>) -> anyhow::Result<()> {
        self.nft_holdings.insert(address, nfts);

        // Update total NFTs via checked arithmetic to prevent overflow panic
        self.total_nfts = self.nft_holdings.values().try_fold(0u64, |acc, v| {
            let len = v.len() as u64;
            acc.checked_add(len)
                .ok_or_else(|| anyhow::anyhow!("total_nfts overflow"))
        })?;
        Ok(())
    }

    /// Update the weight of an NFT
    pub fn update_nft_weight(&mut self, nft_id: String, weight: u64) {
        self.nft_weights.insert(nft_id, weight);
    }
}

#[async_trait]
impl VotingSystem for NftBasedVoting {
    async fn get_voting_power(&self, address: &str) -> Result<VotingPower> {
        // Voting power is based on NFT holdings and their weights via checked arithmetic
        let holdings = self.nft_holdings.get(address).cloned().unwrap_or_default();
        let power = holdings
            .iter()
            .try_fold(0u64, |acc, nft_id| {
                let weight = self.nft_weights.get(nft_id).cloned().unwrap_or(1);
                acc.checked_add(weight)
                    .ok_or_else(|| anyhow::anyhow!("voting_power overflow"))
            })?;

        Ok(VotingPower { value: power })
    }

    async fn get_total_voting_power(&self) -> Result<VotingPower> {
        // Calculate total voting power from all NFTs and their weights via checked arithmetic
        let mut total = 0u64;

        for holdings in self.nft_holdings.values() {
            for nft_id in holdings {
                let weight = self.nft_weights.get(nft_id).cloned().unwrap_or(1);
                total = total
                    .checked_add(weight)
                    .ok_or_else(|| anyhow::anyhow!("total_voting_power overflow"))?;
            }
        }

        Ok(VotingPower { value: total })
    }

    fn voting_power_type(&self) -> &'static str {
        "nft-based"
    }
}

/// Integer square root via Newton's method. Returns largest r where r*r <= n.
fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = (n as f64).sqrt() as u64;
    if x == 0 {
        x = 1;
    }
    loop {
        let x1 = (x + n / x) / 2;
        if x1 >= x {
            return x;
        }
        x = x1;
    }
}

/// Quadratic voting implementation
#[cfg(feature = "governance")]
pub struct QuadraticVoting {
    /// Token balances for each address
    balances: HashMap<String, u64>,

    /// Total supply
    total_supply: u64,
}

#[cfg(feature = "governance")]
impl QuadraticVoting {
    /// Create a new quadratic voting system
    pub fn new() -> Self {
        Self {
            balances: HashMap::new(),
            total_supply: 0,
        }
    }

    /// Update the token balance of an address
    pub fn update_balance(&mut self, address: String, balance: u64) -> anyhow::Result<()> {
        self.balances.insert(address, balance);

        // Update total supply via checked arithmetic to prevent overflow panic
        self.total_supply = self.balances.values().try_fold(0u64, |acc, &v| {
            acc.checked_add(v).ok_or_else(|| anyhow::anyhow!("total_supply overflow"))
        })?;
        Ok(())
    }
}

#[cfg(feature = "governance")]
#[async_trait]
impl VotingSystem for QuadraticVoting {
    async fn get_voting_power(&self, address: &str) -> Result<VotingPower> {
        // Voting power is the square root of the token balance
        let balance = self.balances.get(address).cloned().unwrap_or(0);
        let sqrt_balance = isqrt(balance);
        Ok(VotingPower {
            value: sqrt_balance,
        })
    }

    async fn get_total_voting_power(&self) -> Result<VotingPower> {
        // Calculate total voting power as sum of square roots of balances via checked arithmetic
        let total = self.balances.values().try_fold(0u64, |acc, &balance| {
            acc.checked_add(isqrt(balance))
                .ok_or_else(|| anyhow::anyhow!("total_voting_power overflow"))
        })?;

        Ok(VotingPower { value: total })
    }

    fn voting_power_type(&self) -> &'static str {
        "quadratic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isqrt_small_values() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(2), 1);
        assert_eq!(isqrt(3), 1);
        assert_eq!(isqrt(4), 2);
        assert_eq!(isqrt(9), 3);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
    }

    #[test]
    fn isqrt_large_values() {
        // u64::MAX = 18446744073709551615, sqrt ≈ 4294967295.999…
        assert_eq!(isqrt(u64::MAX), 4_294_967_295);
        // 2^53 + 1 — beyond f64 mantissa precision
        let big = (1u64 << 53) + 1;
        let r = isqrt(big);
        assert!(r * r <= big);
        assert!((r + 1) * (r + 1) > big);
    }

    #[test]
    fn isqrt_perfect_squares() {
        for i in 0u64..=65535 {
            let n = i * i;
            assert_eq!(isqrt(n), i, "isqrt({n}) should be {i}");
        }
    }
}
