//! Shared primitive types for the WRFCoin blockchain platform.
//!
//! This crate provides foundational types used across multiple WRFCoin crates
//! with minimal dependencies. Types are added here when they need to be shared
//! across crates that cannot depend on each other.

/// Registration guard for Cargo integration targets (core4#2888): lives at the
/// leaf of the dependency graph so every workspace crate can call it from a
/// `#[cfg(test)]` unit test.
#[cfg(feature = "integration-guard")]
pub mod integration_inventory;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Rem, RemAssign, Sub, SubAssign};
use std::str::FromStr;

macro_rules! numeric_newtype {
    ($name:ident, $inner:ty) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub $inner);

        impl $name {
            pub const fn new(value: $inner) -> Self {
                Self(value)
            }

            pub const fn into_inner(self) -> $inner {
                self.0
            }

            pub const fn to_be_bytes(self) -> [u8; std::mem::size_of::<$inner>()] {
                self.0.to_be_bytes()
            }

            pub const fn from_be_bytes(bytes: [u8; std::mem::size_of::<$inner>()]) -> Self {
                Self(<$inner>::from_be_bytes(bytes))
            }

            pub fn checked_add(self, rhs: Self) -> Option<Self> {
                self.0.checked_add(rhs.0).map(Self)
            }

            pub fn checked_sub(self, rhs: Self) -> Option<Self> {
                self.0.checked_sub(rhs.0).map(Self)
            }

            pub fn checked_mul(self, rhs: Self) -> Option<Self> {
                self.0.checked_mul(rhs.0).map(Self)
            }

            pub fn checked_div(self, rhs: Self) -> Option<Self> {
                self.0.checked_div(rhs.0).map(Self)
            }

            pub const fn saturating_add(self, rhs: Self) -> Self {
                Self(self.0.saturating_add(rhs.0))
            }

            pub const fn saturating_sub(self, rhs: Self) -> Self {
                Self(self.0.saturating_sub(rhs.0))
            }
        }

        impl From<$inner> for $name {
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $inner {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl FromStr for $name {
            type Err = <$inner as FromStr>::Err;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse::<$inner>().map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl Add for $name {
            type Output = Self;

            fn add(self, rhs: Self) -> Self::Output {
                Self(self.0 + rhs.0)
            }
        }

        impl Add<$inner> for $name {
            type Output = Self;

            fn add(self, rhs: $inner) -> Self::Output {
                Self(self.0 + rhs)
            }
        }

        impl AddAssign for $name {
            fn add_assign(&mut self, rhs: Self) {
                self.0 += rhs.0;
            }
        }

        impl AddAssign<$inner> for $name {
            fn add_assign(&mut self, rhs: $inner) {
                self.0 += rhs;
            }
        }

        impl Sub for $name {
            type Output = Self;

            fn sub(self, rhs: Self) -> Self::Output {
                Self(self.0 - rhs.0)
            }
        }

        impl Sub<$inner> for $name {
            type Output = Self;

            fn sub(self, rhs: $inner) -> Self::Output {
                Self(self.0 - rhs)
            }
        }

        impl SubAssign for $name {
            fn sub_assign(&mut self, rhs: Self) {
                self.0 -= rhs.0;
            }
        }

        impl SubAssign<$inner> for $name {
            fn sub_assign(&mut self, rhs: $inner) {
                self.0 -= rhs;
            }
        }

        impl Mul for $name {
            type Output = Self;

            fn mul(self, rhs: Self) -> Self::Output {
                Self(self.0 * rhs.0)
            }
        }

        impl Mul<$inner> for $name {
            type Output = Self;

            fn mul(self, rhs: $inner) -> Self::Output {
                Self(self.0 * rhs)
            }
        }

        impl MulAssign for $name {
            fn mul_assign(&mut self, rhs: Self) {
                self.0 *= rhs.0;
            }
        }

        impl MulAssign<$inner> for $name {
            fn mul_assign(&mut self, rhs: $inner) {
                self.0 *= rhs;
            }
        }

        impl Div for $name {
            type Output = Self;

            fn div(self, rhs: Self) -> Self::Output {
                Self(self.0 / rhs.0)
            }
        }

        impl Div<$inner> for $name {
            type Output = Self;

            fn div(self, rhs: $inner) -> Self::Output {
                Self(self.0 / rhs)
            }
        }

        impl DivAssign for $name {
            fn div_assign(&mut self, rhs: Self) {
                self.0 /= rhs.0;
            }
        }

        impl DivAssign<$inner> for $name {
            fn div_assign(&mut self, rhs: $inner) {
                self.0 /= rhs;
            }
        }

        impl Rem for $name {
            type Output = Self;

            fn rem(self, rhs: Self) -> Self::Output {
                Self(self.0 % rhs.0)
            }
        }

        impl Rem<$inner> for $name {
            type Output = Self;

            fn rem(self, rhs: $inner) -> Self::Output {
                Self(self.0 % rhs)
            }
        }

        impl RemAssign for $name {
            fn rem_assign(&mut self, rhs: Self) {
                self.0 %= rhs.0;
            }
        }

        impl RemAssign<$inner> for $name {
            fn rem_assign(&mut self, rhs: $inner) {
                self.0 %= rhs;
            }
        }

        impl PartialEq<$inner> for $name {
            fn eq(&self, other: &$inner) -> bool {
                self.0 == *other
            }
        }

        impl Sum for $name {
            fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
                iter.fold(Self::default(), |acc, item| acc + item)
            }
        }

        impl<'a> Sum<&'a $name> for $name {
            fn sum<I: Iterator<Item = &'a $name>>(iter: I) -> Self {
                iter.fold(Self::default(), |acc, item| acc + *item)
            }
        }
    };
}

/// Hash type for various blockchain elements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn into_inner(self) -> [u8; 32] {
        self.0
    }

    pub fn copy_from_slice(&mut self, src: &[u8]) {
        self.0.copy_from_slice(src);
    }
}

impl From<[u8; 32]> for Hash {
    fn from(value: [u8; 32]) -> Self {
        Self(value)
    }
}

impl From<Hash> for [u8; 32] {
    fn from(value: Hash) -> Self {
        value.0
    }
}

impl AsRef<[u8; 32]> for Hash {
    fn as_ref(&self) -> &[u8; 32] {
        &self.0
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

numeric_newtype!(Balance, u128);
numeric_newtype!(BlockNumber, u64);
numeric_newtype!(ChainId, u16);
numeric_newtype!(Timestamp, u64);
numeric_newtype!(Nonce, u64);
numeric_newtype!(Gas, u64);

/// The type for a transaction ID.
pub type TransactionId = Hash;

/// The type for a block ID (which is the same as its hash).
pub type BlockId = Hash;

/// A type representing a shard ID.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShardId(pub u32);

impl ShardId {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }
}

impl fmt::Display for ShardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Shard-{}", self.0)
    }
}

/// Verification result type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationResult {
    Verified,
    Rejected,
    Pending,
    Failed,
}

impl fmt::Display for VerificationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verified => write!(f, "Verified"),
            Self::Rejected => write!(f, "Rejected"),
            Self::Pending => write!(f, "Pending"),
            Self::Failed => write!(f, "Failed"),
        }
    }
}

/// Privacy level for confidential transactions.
///
/// Variants are ordered from least to most private, enabling comparison
/// via `PartialOrd`/`Ord` (e.g., `High > Basic`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum PrivacyLevel {
    /// Standard transaction (no privacy features).
    #[default]
    Standard,
    /// Basic privacy: stealth addresses (receiver privacy) / Pedersen commitments.
    Basic,
    /// Medium privacy: confidential transactions (amount privacy) + stealth addresses.
    Medium,
    /// High privacy: ring signatures (sender privacy) + confidential transactions + stealth addresses.
    High,
    /// Maximum privacy: all privacy features enabled.
    Maximum,
}


impl fmt::Display for PrivacyLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Standard => write!(f, "Standard"),
            Self::Basic => write!(f, "Basic"),
            Self::Medium => write!(f, "Medium"),
            Self::High => write!(f, "High"),
            Self::Maximum => write!(f, "Maximum"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_level_ordering() {
        assert!(PrivacyLevel::Maximum > PrivacyLevel::High);
        assert!(PrivacyLevel::High > PrivacyLevel::Medium);
        assert!(PrivacyLevel::Medium > PrivacyLevel::Basic);
        assert!(PrivacyLevel::Basic > PrivacyLevel::Standard);
    }

    #[test]
    fn privacy_level_default() {
        assert_eq!(PrivacyLevel::default(), PrivacyLevel::Standard);
    }

    #[test]
    fn privacy_level_display() {
        assert_eq!(PrivacyLevel::Standard.to_string(), "Standard");
        assert_eq!(PrivacyLevel::Maximum.to_string(), "Maximum");
    }

    #[test]
    fn balance_arithmetic() {
        let mut balance = Balance::new(10);
        balance += Balance::new(5);
        balance -= 3u128;
        assert_eq!(balance, Balance::new(12));
        assert_eq!(balance + 8u128, Balance::new(20));
    }

    #[test]
    fn hash_display_hex() {
        let hash = Hash::new([0xAB; 32]);
        assert_eq!(hash.to_string(), "ab".repeat(32));
    }

    #[test]
    fn verification_result_display() {
        assert_eq!(VerificationResult::Verified.to_string(), "Verified");
        assert_eq!(VerificationResult::Rejected.to_string(), "Rejected");
    }
}
