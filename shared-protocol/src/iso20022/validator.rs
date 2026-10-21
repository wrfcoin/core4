//! ISO20022 message validators
//!
//! This module provides validation for ISO20022 compliant messages.

use anyhow::{Result, anyhow};

use crate::iso20022::{
    AccountReportMessage, CreditTransferMessage, Message, MessageValidator, PaymentReturnMessage,
};

/// Credit transfer message validator
pub(crate) struct CreditTransferValidator {
    /// Validation rules
    rules: Vec<Box<dyn Fn(&CreditTransferMessage) -> Result<()> + Send + Sync>>,
}

impl CreditTransferValidator {
    /// Create a new credit transfer validator
    pub fn new() -> Self {
        let mut validator = Self { rules: Vec::new() };

        // Add validation rules
        validator.add_rule(Box::new(|msg| {
            // Check if message ID is present
            if msg.message_id.is_empty() {
                return Err(anyhow!("Message ID is required"));
            }

            Ok(())
        }));

        validator.add_rule(Box::new(|msg| {
            // Check if BIC codes are valid
            if !Self::is_valid_bic(&msg.instructing_agent.bic) {
                return Err(anyhow!("Invalid instructing agent BIC"));
            }

            if !Self::is_valid_bic(&msg.instructed_agent.bic) {
                return Err(anyhow!("Invalid instructed agent BIC"));
            }

            Ok(())
        }));

        validator.add_rule(Box::new(|msg| {
            // Check if amount is positive
            if msg.transaction_info.instructed_amount.value <= 0.0 {
                return Err(anyhow!("Amount must be positive"));
            }

            Ok(())
        }));

        validator.add_rule(Box::new(|msg| {
            // Check if debtor and creditor accounts are present
            if msg.transaction_info.debtor_account.other.is_none()
                && msg.transaction_info.debtor_account.iban.is_none()
            {
                return Err(anyhow!("Debtor account identifier is required"));
            }

            if msg.transaction_info.creditor_account.other.is_none()
                && msg.transaction_info.creditor_account.iban.is_none()
            {
                return Err(anyhow!("Creditor account identifier is required"));
            }

            Ok(())
        }));

        validator
    }

    /// Add a validation rule
    fn add_rule(&mut self, rule: Box<dyn Fn(&CreditTransferMessage) -> Result<()> + Send + Sync>) {
        self.rules.push(rule);
    }

    /// Check if a BIC code is valid
    fn is_valid_bic(bic: &str) -> bool {
        // BIC format: 8 or 11 characters
        // AAAABBCC or AAAABBCCDDD
        // AAAA = bank code (letters only)
        // BB = country code (letters only)
        // CC = location code (letters or digits)
        // DDD = branch code (optional, letters or digits)

        if bic.len() != 8 && bic.len() != 11 {
            return false;
        }

        // Check bank code (first 4 characters, letters only)
        for c in bic[0..4].chars() {
            if !c.is_ascii_alphabetic() {
                return false;
            }
        }

        // Check country code (next 2 characters, letters only)
        for c in bic[4..6].chars() {
            if !c.is_ascii_alphabetic() {
                return false;
            }
        }

        // Check location code (next 2 characters, letters or digits)
        for c in bic[6..8].chars() {
            if !c.is_ascii_alphanumeric() {
                return false;
            }
        }

        // Check branch code if present (last 3 characters, letters or digits)
        if bic.len() == 11 {
            for c in bic[8..11].chars() {
                if !c.is_ascii_alphanumeric() {
                    return false;
                }
            }
        }

        true
    }
}

impl MessageValidator for CreditTransferValidator {
    /// Validate a credit transfer message
    fn validate(&self, message: &Message) -> Result<()> {
        match message {
            Message::CreditTransfer(credit_transfer) => {
                // Apply all validation rules
                for rule in &self.rules {
                    rule(credit_transfer)?;
                }

                Ok(())
            },
            _ => Err(anyhow!("Expected a credit transfer message")),
        }
    }
}

/// Payment return message validator
pub(crate) struct PaymentReturnValidator {
    /// Validation rules
    rules: Vec<Box<dyn Fn(&PaymentReturnMessage) -> Result<()> + Send + Sync>>,
}

impl PaymentReturnValidator {
    /// Create a new payment return validator
    pub fn new() -> Self {
        let mut validator = Self { rules: Vec::new() };

        // Add validation rules
        validator.add_rule(Box::new(|msg| {
            // Check if message ID is present
            if msg.message_id.is_empty() {
                return Err(anyhow!("Message ID is required"));
            }

            Ok(())
        }));

        validator.add_rule(Box::new(|msg| {
            // Check if original message ID is present
            if msg.original_message_id.is_empty() {
                return Err(anyhow!("Original message ID is required"));
            }

            Ok(())
        }));

        validator.add_rule(Box::new(|msg| {
            // Check if return reason code is present
            if msg.return_reason.is_empty() {
                return Err(anyhow!("Return reason code is required"));
            }

            Ok(())
        }));

        validator.add_rule(Box::new(|msg| {
            // Check if amount is positive
            if msg.transaction_info.returned_amount.value <= 0.0 {
                return Err(anyhow!("Amount must be positive"));
            }

            Ok(())
        }));

        validator
    }

    /// Add a validation rule
    fn add_rule(&mut self, rule: Box<dyn Fn(&PaymentReturnMessage) -> Result<()> + Send + Sync>) {
        self.rules.push(rule);
    }
}

impl MessageValidator for PaymentReturnValidator {
    /// Validate a payment return message
    fn validate(&self, message: &Message) -> Result<()> {
        match message {
            Message::PaymentReturn(payment_return) => {
                // Apply all validation rules
                for rule in &self.rules {
                    rule(payment_return)?;
                }

                Ok(())
            },
            _ => Err(anyhow!("Expected a payment return message")),
        }
    }
}

/// Account report message validator
pub(crate) struct AccountReportValidator {
    /// Validation rules
    rules: Vec<Box<dyn Fn(&AccountReportMessage) -> Result<()> + Send + Sync>>,
}

impl AccountReportValidator {
    /// Create a new account report validator
    pub fn new() -> Self {
        let mut validator = Self { rules: Vec::new() };

        // Add validation rules
        validator.add_rule(Box::new(|msg| {
            // Check if message ID is present
            if msg.message_id.is_empty() {
                return Err(anyhow!("Message ID is required"));
            }

            Ok(())
        }));

        validator.add_rule(Box::new(|msg| {
            // Check if account is identified
            if msg.account.iban.is_none() && msg.account.other.is_none() {
                return Err(anyhow!("Account identifier is required"));
            }

            Ok(())
        }));

        validator
    }

    /// Add a validation rule
    fn add_rule(&mut self, rule: Box<dyn Fn(&AccountReportMessage) -> Result<()> + Send + Sync>) {
        self.rules.push(rule);
    }
}

impl MessageValidator for AccountReportValidator {
    /// Validate an account report message
    fn validate(&self, message: &Message) -> Result<()> {
        match message {
            Message::AccountReport(account_report) => {
                // Apply all validation rules
                for rule in &self.rules {
                    rule(account_report)?;
                }

                Ok(())
            },
            _ => Err(anyhow!("Expected an account report message")),
        }
    }
}
