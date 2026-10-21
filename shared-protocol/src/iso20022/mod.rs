//! ISO20022 compliance module
//!
//! This module provides ISO20022 compliance for financial messaging
//! within the Wrfcoin blockchain ecosystem.

mod messages;
mod parser;
mod validator;

pub use messages::*;

use anyhow::Result;
use parser::MessageParser;
// Time handling
use std::collections::HashMap;
use validator::{CreditTransferValidator, PaymentReturnValidator};

/// ISO20022 message types supported by Wrfcoin
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum MessageType {
    /// Payment Initiation (pacs.008)
    CreditTransfer,

    /// Payment Return (pacs.004)
    PaymentReturn,

    /// Payment Cancellation Request (camt.056)
    PaymentCancellationRequest,

    /// Account Report (camt.052)
    AccountReport,

    /// Account Statement (camt.053)
    AccountStatement,

    /// Real-time notification (seev.031)
    TransactionNotification,
}

/// ISO20022 message format
pub enum MessageFormat {
    /// XML format
    Xml,

    /// JSON format
    Json,
}

/// ISO20022 message handler
pub struct Iso20022Handler {
    /// Registered message validators
    validators: HashMap<MessageType, Box<dyn MessageValidator>>,

    /// Message parser
    parser: MessageParser,
}

impl Iso20022Handler {
    /// Create a new ISO20022 handler
    pub fn new() -> Self {
        Self {
            validators: HashMap::new(),
            parser: MessageParser::new(),
        }
    }

    /// Register a validator for a specific message type
    pub fn register_validator<V: MessageValidator + 'static>(
        &mut self,
        message_type: MessageType,
        validator: V,
    ) {
        self.validators.insert(message_type, Box::new(validator));
    }

    /// Parse and validate an ISO20022 message
    pub fn process_message(
        &self,
        message_type: MessageType,
        message: &str,
        format: MessageFormat,
    ) -> Result<Message> {
        // Parse the message
        let message = self.parser.parse(message, format)?;

        // Validate the message if we have a validator for this type
        if let Some(validator) = self.validators.get(&message_type) {
            validator.validate(&message)?;
        }

        Ok(message)
    }

    /// Convert a Wrfcoin transaction to an ISO20022 message
    pub fn transaction_to_message(
        &self,
        transaction: &crate::blockchain::transaction::Transaction,
    ) -> Result<String> {
        // Create a credit transfer message from the transaction
        let message = CreditTransferMessage::from_transaction(transaction)?;

        // Generate XML
        message.to_xml()
    }

    /// Extract transaction data from an ISO20022 message
    pub fn message_to_transaction(
        &self,
        message: &str,
        format: MessageFormat,
    ) -> Result<crate::blockchain::transaction::Transaction> {
        // Parse the message
        let message = self.parser.parse(message, format)?;

        match message {
            Message::CreditTransfer(credit_transfer) => credit_transfer.to_transaction(),
            _ => {
                anyhow::bail!("Message type not supported for conversion to transaction")
            },
        }
    }
}

/// ISO20022 compliant message
#[derive(Debug, Clone)]
pub enum Message {
    /// Credit Transfer (pacs.008)
    CreditTransfer(CreditTransferMessage),

    /// Payment Return (pacs.004)
    PaymentReturn(PaymentReturnMessage),

    /// Account Report (camt.052)
    AccountReport(AccountReportMessage),
}

/// Base trait for all ISO20022 message validators
pub trait MessageValidator: Send + Sync {
    /// Validate a message according to ISO20022 schemas and rules
    fn validate(&self, message: &Message) -> Result<()>;
}

/// ISO20022 compliance manager for the Wrfcoin ecosystem
pub struct Iso20022ComplianceManager {
    /// Message handler
    handler: Iso20022Handler,
}

impl Iso20022ComplianceManager {
    /// Create a new ISO20022 compliance manager
    pub fn new() -> Self {
        // Create handler
        let mut handler = Iso20022Handler::new();

        // Register validators
        handler.register_validator(MessageType::CreditTransfer, CreditTransferValidator::new());

        handler.register_validator(MessageType::PaymentReturn, PaymentReturnValidator::new());

        Self { handler }
    }

    /// Process an incoming ISO20022 message and convert it to a blockchain transaction
    pub fn process_incoming_message(
        &self,
        message: &str,
        format: MessageFormat,
    ) -> Result<crate::blockchain::transaction::Transaction> {
        self.handler.message_to_transaction(message, format)
    }

    /// Generate an ISO20022 compliant message from a blockchain transaction
    pub fn generate_message(
        &self,
        transaction: &crate::blockchain::transaction::Transaction,
    ) -> Result<String> {
        self.handler.transaction_to_message(transaction)
    }

    /// Validate if a transaction is compliant with ISO20022 standards
    pub fn validate_transaction_compliance(
        &self,
        transaction: &crate::blockchain::transaction::Transaction,
    ) -> Result<()> {
        // Convert transaction to message
        let message = self.handler.transaction_to_message(transaction)?;

        // Parse and validate the message
        self.handler
            .process_message(MessageType::CreditTransfer, &message, MessageFormat::Xml)?;

        Ok(())
    }
}

/// Initialize the ISO20022 compliance subsystem
pub fn init_iso20022_compliance() -> Iso20022ComplianceManager {
    Iso20022ComplianceManager::new()
}
