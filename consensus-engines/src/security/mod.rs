//! Consensus Security Infrastructure
//!
//! This module provides enterprise-grade security mechanisms for distributed
//! consensus protocols, including threat detection, attack prevention, audit
//! logging, cryptographic integrity validation, and Byzantine fault tolerance.
//!
//! ## Module Organization
//!
//! - `manager` - Core security manager with cryptographic validation and threat detection
//! - `audit` - Security audit framework with vulnerability detection
//! - `attack_prevention` - Protection against Sybil, DoS, double-spend, and eclipse attacks
//! - `correlation_penalties` - Quadratic/correlated slashing penalties for validator misbehavior
//! - `bridge` - Security-enhanced consensus bridge coordinating security and consensus operations
//! - `coordinator` - Secure consensus coordinator wrapping dynamic engine with security validation

pub mod attack_prevention;
pub mod audit;
pub mod bridge;
pub mod coordinator;
pub mod correlation_penalties;
pub mod manager;
