#![no_main]

//! Fuzz target for AccountAbstraction `UserOperation` validation (#1389).
//!
//! `UserOperation` does not derive `Deserialize` — EIP-4337-style bundler
//! inputs are parsed upstream. We fuzz the *validation* surface downstream:
//! `user_operation_hash`, `validate_time_window`, and `ValidationPipeline`'s
//! end-to-end `validate_and_apply`. Each must be total (no panics) against
//! arbitrary signer keys, signatures, call-data hashes, and time windows.
//!
//! The harness carves a `UserOperation` out of the raw fuzz input so libFuzzer
//! can explore the decision tree without needing a serialized wire format.

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::accounts::abstraction::{
    BasicSignatureVerifier, SmartAccountPolicy, SponsorshipRequest, UserOperation,
    ValidationContext, ValidationPipeline, user_operation_hash, validate_time_window,
};
use wrfcoin_shared_protocol::common::types::{
    Address, BlockNumber, ChainId, Nonce, Timestamp,
};

const MIN_LEN: usize = 20 + 8 + 32 + 16 + 9 + 9 + 2 + 8 + 8 + 1 + 16 + 20;

fn carve_userop(
    data: &[u8],
) -> Option<(UserOperation, ValidationContext, Option<SponsorshipRequest>)> {
    if data.len() < MIN_LEN {
        return None;
    }

    let mut cur = 0usize;
    let sender_bytes = &data[cur..cur + 20]; cur += 20;
    let nonce_bytes = &data[cur..cur + 8]; cur += 8;
    let call_data_hash = &data[cur..cur + 32]; cur += 32;
    let max_fee_bytes = &data[cur..cur + 16]; cur += 16;
    let valid_after_flag = data[cur]; cur += 1;
    let valid_after_bytes = &data[cur..cur + 8]; cur += 8;
    let valid_until_flag = data[cur]; cur += 1;
    let valid_until_bytes = &data[cur..cur + 8]; cur += 8;
    let chain_id_bytes = &data[cur..cur + 2]; cur += 2;
    let block_number_bytes = &data[cur..cur + 8]; cur += 8;
    let ctx_ts_bytes = &data[cur..cur + 8]; cur += 8;
    let sponsor_flag = data[cur]; cur += 1;
    let sponsor_max_fee_bytes = &data[cur..cur + 16]; cur += 16;
    let _sponsor_addr_bytes = &data[cur..cur + 20]; cur += 20;

    let remainder = &data[cur..];
    let split = remainder.len() / 2;
    let signer_public_key = remainder[..split].to_vec();
    let signature = remainder[split..].to_vec();

    let mut sender_arr = [0u8; 20];
    sender_arr.copy_from_slice(sender_bytes);

    let mut call_data_arr = [0u8; 32];
    call_data_arr.copy_from_slice(call_data_hash);

    let op = UserOperation {
        sender: Address(sender_arr),
        nonce: Nonce::new(u64::from_be_bytes(nonce_bytes.try_into().ok()?)),
        call_data_hash: call_data_arr,
        max_fee_per_gas: u128::from_be_bytes(max_fee_bytes.try_into().ok()?),
        signer_public_key,
        signature,
        valid_after: if valid_after_flag & 1 == 1 {
            Some(Timestamp::new(u64::from_be_bytes(
                valid_after_bytes.try_into().ok()?,
            )))
        } else {
            None
        },
        valid_until: if valid_until_flag & 1 == 1 {
            Some(Timestamp::new(u64::from_be_bytes(
                valid_until_bytes.try_into().ok()?,
            )))
        } else {
            None
        },
            validation_gas_limit: 1_000_000,
            call_gas_limit: 0,
            paymaster: None,
        };

    let ctx = ValidationContext {
        chain_id: ChainId::new(u16::from_be_bytes(chain_id_bytes.try_into().ok()?)),
        block_number: BlockNumber::new(u64::from_be_bytes(block_number_bytes.try_into().ok()?)),
        timestamp: Timestamp::new(u64::from_be_bytes(ctx_ts_bytes.try_into().ok()?)),
    };

    let sponsorship = if sponsor_flag & 1 == 1 {
        Some(SponsorshipRequest {
            sponsor: Address(sender_arr),
            max_fee_per_gas: u128::from_be_bytes(sponsor_max_fee_bytes.try_into().ok()?),
        })
    } else {
        None
    };

    Some((op, ctx, sponsorship))
}

fuzz_target!(|data: &[u8]| {
    let Some((op, ctx, sponsorship)) = carve_userop(data) else {
        return;
    };

    let _ = validate_time_window(&op, ctx.timestamp);
    let _ = user_operation_hash(&op, ctx);

    let mut pipeline = ValidationPipeline::new(BasicSignatureVerifier);
    pipeline.set_policy(
        op.sender,
        SmartAccountPolicy {
            allow_gas_sponsorship: true,
            max_sponsored_fee_per_op: u128::MAX,
        },
    );
    let _ = pipeline.validate_and_apply(&op, ctx, sponsorship);
});
