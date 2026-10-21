use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::blockchain::transaction::{Transaction, TransactionType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NFTType {
    Art,
    ComputationalResource,
    Document,
    Music,
    Video,
    Domain,
    GameItem,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NFTAttribute {
    pub trait_type: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NFTMetadata {
    pub name: String,
    pub description: String,
    pub image_uri: Option<String>,
    pub external_url: Option<String>,
    pub attributes: Vec<NFTAttribute>,
    pub nft_type: NFTType,
    
    // Fields specific to computational NFTs
    pub compute_power: Option<f64>,        // in FLOPS or similar unit
    pub memory: Option<u64>,               // in MB
    pub storage: Option<u64>,              // in MB
    pub duration: Option<u64>,             // in seconds
    pub renewable: Option<bool>,
    pub availability: Option<f64>,         // percentage of uptime
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NFT {
    pub id: String,
    pub token_id: u64,
    pub creator: String,          // Creator's wallet address
    pub owner: String,            // Current owner's wallet address
    pub metadata: NFTMetadata,
    pub creation_tx: String,      // Transaction ID that created this NFT
    pub created_at: DateTime<Utc>,
    pub last_transferred_at: Option<DateTime<Utc>>,
    pub price_history: Vec<(DateTime<Utc>, u64)>, // price in base units
    pub royalty_percentage: f64,  // Percentage that goes to original creator on resale
}

impl NFT {
    pub fn new(
        creator: String,
        token_id: u64,
        metadata: NFTMetadata,
        creation_tx: String,
        royalty_percentage: f64,
    ) -> Self {
        let now = Utc::now();
        
        // Generate NFT ID as a hash of creator address, token ID, and timestamp
        let mut hasher = Sha256::new();
        hasher.update(format!(
            "{}:{}:{}",
            creator,
            token_id,
            now.timestamp()
        ).as_bytes());
        
        let id = hex::encode(hasher.finalize());
        
        NFT {
            id,
            token_id,
            creator: creator.clone(),
            owner: creator,
            metadata,
            creation_tx,
            created_at: now,
            last_transferred_at: None,
            price_history: Vec::new(),
            royalty_percentage,
        }
    }
    
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
    
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
    
    pub fn transfer(&mut self, new_owner: String, price: u64) {
        let now = Utc::now();
        
        self.owner = new_owner;
        self.last_transferred_at = Some(now);
        self.price_history.push((now, price));
    }
    
    pub fn is_computational(&self) -> bool {
        matches!(self.metadata.nft_type, NFTType::ComputationalResource)
    }
    
    pub fn get_compute_specs(&self) -> Option<(f64, u64, u64, u64)> {
        if self.is_computational() {
            Some((
                self.metadata.compute_power.unwrap_or(0.0),
                self.metadata.memory.unwrap_or(0),
                self.metadata.storage.unwrap_or(0),
                self.metadata.duration.unwrap_or(0),
            ))
        } else {
            None
        }
    }
}

pub struct NFTRegistry {
    nfts: HashMap<String, NFT>,     // NFT ID -> NFT
    token_counter: u64,
    owner_nfts: HashMap<String, Vec<String>>, // Owner address -> List of NFT IDs
    creator_nfts: HashMap<String, Vec<String>>, // Creator address -> List of NFT IDs
}

impl NFTRegistry {
    pub fn new() -> Self {
        NFTRegistry {
            nfts: HashMap::new(),
            token_counter: 0,
            owner_nfts: HashMap::new(),
            creator_nfts: HashMap::new(),
        }
    }
    
    pub fn mint_nft(
        &mut self,
        creator: String, 
        metadata: NFTMetadata,
        creation_tx: String,
        royalty_percentage: f64,
    ) -> NFT {
        self.token_counter += 1;
        
        let nft = NFT::new(
            creator.clone(),
            self.token_counter,
            metadata,
            creation_tx,
            royalty_percentage,
        );
        
        // Register NFT
        let nft_id = nft.id.clone();
        
        // Add to creator's NFTs
        self.creator_nfts
            .entry(creator.clone())
            .or_insert_with(Vec::new)
            .push(nft_id.clone());
        
        // Add to owner's NFTs (creator is initial owner)
        self.owner_nfts
            .entry(creator)
            .or_insert_with(Vec::new)
            .push(nft_id.clone());
        
        // Add to NFT registry
        self.nfts.insert(nft_id, nft.clone());
        
        nft
    }
    
    pub fn transfer_nft(
        &mut self,
        nft_id: String,
        from: String,
        to: String,
        price: u64,
    ) -> Result<(), &'static str> {
        // Check if NFT exists
        if !self.nfts.contains_key(&nft_id) {
            return Err("NFT not found");
        }
        
        // Check if sender is the owner
        if let Some(nft) = self.nfts.get(&nft_id) {
            if nft.owner != from {
                return Err("Sender is not the owner");
            }
        } else {
            return Err("NFT not found");
        }
        
        // Update NFT owner
        if let Some(nft) = self.nfts.get_mut(&nft_id) {
            nft.transfer(to.clone(), price);
        }
        
        // Update owner mappings
        if let Some(owner_nfts) = self.owner_nfts.get_mut(&from) {
            owner_nfts.retain(|id| *id != nft_id);
        }
        
        self.owner_nfts
            .entry(to)
            .or_insert_with(Vec::new)
            .push(nft_id);
        
        Ok(())
    }
    
    pub fn get_nft(&self, nft_id: &str) -> Option<&NFT> {
        self.nfts.get(nft_id)
    }
    
    pub fn get_nfts_by_owner(&self, owner: &str) -> Vec<&NFT> {
        if let Some(nft_ids) = self.owner_nfts.get(owner) {
            nft_ids.iter()
                .filter_map(|id| self.nfts.get(id))
                .collect()
        } else {
            Vec::new()
        }
    }
    
    pub fn get_nfts_by_creator(&self, creator: &str) -> Vec<&NFT> {
        if let Some(nft_ids) = self.creator_nfts.get(creator) {
            nft_ids.iter()
                .filter_map(|id| self.nfts.get(id))
                .collect()
        } else {
            Vec::new()
        }
    }
    
    pub fn get_computational_nfts(&self) -> Vec<&NFT> {
        self.nfts.values()
            .filter(|nft| nft.is_computational())
            .collect()
    }
}
