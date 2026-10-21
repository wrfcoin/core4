#![no_main]

//! Fuzz target for ISO 20022 message parsing.
//!
//! The ISO 20022 handler accepts arbitrary XML/JSON strings from external
//! financial systems.  XML parsers are a classic fuzz target — any panic or
//! unbounded allocation in the parser is a DoS vector.

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::iso20022::{Iso20022Handler, MessageFormat, MessageType};

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let handler = Iso20022Handler::new();

        // Fuzz both XML and JSON parse paths with every message type.
        for msg_type in [
            MessageType::CreditTransfer,
            MessageType::PaymentReturn,
            MessageType::PaymentCancellationRequest,
            MessageType::AccountReport,
        ] {
            let _ = handler.process_message(msg_type.clone(), input, MessageFormat::Xml);
            let _ = handler.process_message(msg_type, input, MessageFormat::Json);
        }
    }
});
