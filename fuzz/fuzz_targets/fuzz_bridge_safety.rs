#![no_main]

//! Fuzz target for bridge safety manager operations.
//!
//! The bridge safety module (1537 lines) processes cross-chain messages,
//! manages circuit breaker state transitions, exposure cap calculations,
//! and challenge window management.  Logic errors here could allow
//! double-spending or cap evasion.

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::bridge::safety_manager::BridgeSafetyManager;
use wrfcoin_shared_protocol::bridge::types::BridgeSafetyConfig;

/// A simple fuzzer input structure parsed from raw bytes.
/// We use a minimal hand-rolled parser to avoid serde overhead and
/// exercise more code paths with raw bytes.
fn fuzz_bridge(data: &[u8]) {
    if data.len() < 20 {
        return;
    }

    let mut mgr = BridgeSafetyManager::new(BridgeSafetyConfig::default());

    // Use first bytes to select operations, rest as parameters.
    let mut offset = 0;
    while offset + 10 <= data.len() {
        let op = data[offset];
        let amount = u64::from_le_bytes({
            let mut buf = [0u8; 8];
            let end = (offset + 1 + 8).min(data.len());
            let slice = &data[offset + 1..end];
            buf[..slice.len()].copy_from_slice(slice);
            buf
        });
        let chain_byte = data[(offset + 9) % data.len()];
        let chain_id = match chain_byte % 4 {
            0 => "ethereum",
            1 => "polygon",
            2 => "solana",
            _ => "cosmos",
        };
        let now_ts = (amount % 1_000_000_000) as i64 + 1_700_000_000;

        match op % 5 {
            0 => {
                // check + complete bridge operation
                if mgr.check_bridge_operation(chain_id, amount, now_ts).is_ok() {
                    mgr.complete_bridge_operation(chain_id, amount);
                }
            }
            1 => {
                // fail bridge operation
                mgr.fail_bridge_operation(chain_id, amount, "fuzz_failure", now_ts);
            }
            2 => {
                // open challenge
                let tx_id = format!("tx_{}", amount % 1000);
                let _ = mgr.open_challenge(&tx_id, chain_id, now_ts);
            }
            3 => {
                // submit + resolve challenge
                let tx_id = format!("tx_{}", amount % 1000);
                let _ = mgr.submit_challenge(&tx_id, "fuzz_challenger", "fuzz_reason", now_ts);
                let _ = mgr.resolve_challenge(&tx_id, chain_byte % 2 == 0, "fuzz_note", now_ts);
            }
            _ => {
                // halt
                mgr.halt(
                    "fuzz_halt",
                    wrfcoin_shared_protocol::bridge::types::HaltTrigger::ManualHalt,
                    now_ts,
                );
            }
        }

        offset += 10;
    }
}

fuzz_target!(|data: &[u8]| {
    fuzz_bridge(data);
});
