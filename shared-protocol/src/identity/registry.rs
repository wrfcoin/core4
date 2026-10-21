use crate::common::types::{Address, Timestamp};
use rustc_hash::FxHashMap;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VerificationLevel {
    Basic,
    KycVerified,
    StationVerified,
    Institutional,
}

#[derive(Debug, Clone)]
pub struct ContributorIdentity {
    pub contributor_id: String,
    pub address: Address,
    pub verification_level: VerificationLevel,
    pub updated_at: Timestamp,
}

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("identity already exists for address {0}")]
    AlreadyRegistered(Address),
    #[error("identity not found for address {0}")]
    NotFound(Address),
}

#[derive(Debug, Default, Clone)]
pub struct IdentityRegistry {
    records: FxHashMap<Address, ContributorIdentity>,
}

impl IdentityRegistry {
    pub fn register(&mut self, identity: ContributorIdentity) -> Result<(), IdentityError> {
        if self.records.contains_key(&identity.address) {
            return Err(IdentityError::AlreadyRegistered(identity.address));
        }
        self.records.insert(identity.address, identity);
        Ok(())
    }

    pub fn get(&self, address: &Address) -> Option<&ContributorIdentity> {
        self.records.get(address)
    }

    pub fn update_verification_level(
        &mut self,
        address: Address,
        new_level: VerificationLevel,
        updated_at: Timestamp,
    ) -> Result<(), IdentityError> {
        let identity = self
            .records
            .get_mut(&address)
            .ok_or(IdentityError::NotFound(address))?;
        identity.verification_level = new_level;
        identity.updated_at = updated_at;
        Ok(())
    }

    pub fn is_station_verified(&self, address: &Address) -> bool {
        self.records
            .get(address)
            .map(|identity| identity.verification_level >= VerificationLevel::StationVerified)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    #[test]
    fn station_verification_transitions_to_true() {
        let address = addr(3);
        let mut registry = IdentityRegistry::default();
        registry
            .register(ContributorIdentity {
                contributor_id: "station-3".to_string(),
                address,
                verification_level: VerificationLevel::Basic,
                updated_at: Timestamp::new(100),
            })
            .unwrap();

        assert!(!registry.is_station_verified(&address));
        registry
            .update_verification_level(address, VerificationLevel::StationVerified, Timestamp::new(200))
            .unwrap();
        assert!(registry.is_station_verified(&address));
    }
}
