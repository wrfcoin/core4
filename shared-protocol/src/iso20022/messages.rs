//! ISO20022 message implementations
//!
//! This module provides the structure and conversions for ISO20022 compliant messages.

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use std::io::Cursor;
use uuid::Uuid;
// Module imports

/// ISO20022 financial institution identification
#[derive(Debug, Clone)]
pub struct FinancialInstitutionId {
    /// Bank Identifier Code (BIC)
    pub bic: String,

    /// Clearing system member ID
    pub clearing_system_id: Option<String>,

    /// Institution name
    pub name: Option<String>,
}

/// ISO20022 account identification
#[derive(Debug, Clone)]
pub struct AccountIdentification {
    /// International Bank Account Number (IBAN)
    pub iban: Option<String>,

    /// Other account identifier
    pub other: Option<String>,

    /// Account type
    pub account_type: Option<String>,
}

/// ISO20022 amount with currency
#[derive(Debug, Clone)]
pub struct Amount {
    /// Monetary amount
    pub value: f64,

    /// Currency code (ISO 4217)
    pub currency: String,
}

/// ISO20022 transaction status
#[derive(Debug, Clone)]
pub enum TransactionStatus {
    /// Transaction is pending
    Pending,

    /// Transaction completed successfully
    Completed,

    /// Transaction rejected
    Rejected,

    /// Transaction returned
    Returned,

    /// Transaction cancelled
    Cancelled,
}

/// ISO20022 credit transfer message (pacs.008)
#[derive(Debug, Clone)]
pub struct CreditTransferMessage {
    /// Message identification
    pub message_id: String,

    /// Creation date and time
    pub creation_date_time: DateTime<Utc>,

    /// Number of transactions
    pub number_of_transactions: u32,

    /// Instructing agent (sender financial institution)
    pub instructing_agent: FinancialInstitutionId,

    /// Instructed agent (receiver financial institution)
    pub instructed_agent: FinancialInstitutionId,

    /// Transaction information
    pub transaction_info: CreditTransferTransactionInfo,
}

/// ISO20022 credit transfer transaction information
#[derive(Debug, Clone)]
pub struct CreditTransferTransactionInfo {
    /// Payment identification
    pub payment_id: String,

    /// Instructed amount
    pub instructed_amount: Amount,

    /// Debtor account
    pub debtor_account: AccountIdentification,

    /// Creditor account
    pub creditor_account: AccountIdentification,

    /// Remittance information
    pub remittance_info: Option<String>,
}

/// ISO20022 payment return message (pacs.004)
#[derive(Debug, Clone)]
pub struct PaymentReturnMessage {
    /// Message identification
    pub message_id: String,

    /// Creation date and time
    pub creation_date_time: DateTime<Utc>,

    /// Original message ID
    pub original_message_id: String,

    /// Return reason code
    pub return_reason: String,

    /// Transaction information
    pub transaction_info: PaymentReturnTransactionInfo,
}

/// ISO20022 payment return transaction information
#[derive(Debug, Clone)]
pub struct PaymentReturnTransactionInfo {
    /// Return identification
    pub return_id: String,

    /// Original payment ID
    pub original_payment_id: String,

    /// Returned amount
    pub returned_amount: Amount,

    /// Original debtor account
    pub original_debtor_account: AccountIdentification,

    /// Original creditor account
    pub original_creditor_account: AccountIdentification,
}

/// ISO20022 account report message (camt.052)
#[derive(Debug, Clone)]
pub struct AccountReportMessage {
    /// Message identification
    pub message_id: String,

    /// Creation date and time
    pub creation_date_time: DateTime<Utc>,

    /// Account identification
    pub account: AccountIdentification,

    /// Report entries
    pub entries: Vec<ReportEntry>,
}

/// ISO20022 account report entry
#[derive(Debug, Clone)]
pub struct ReportEntry {
    /// Entry reference
    pub reference: String,

    /// Amount
    pub amount: Amount,

    /// Credit/debit indicator
    pub credit_debit_indicator: CreditDebitIndicator,

    /// Status
    pub status: TransactionStatus,

    /// Booking date
    pub booking_date: DateTime<Utc>,

    /// Value date
    pub value_date: DateTime<Utc>,

    /// Transaction details
    pub transaction_details: Option<String>,
}

/// Credit or debit indicator
#[derive(Debug, Clone)]
pub enum CreditDebitIndicator {
    /// Credit (incoming payment)
    Credit,

    /// Debit (outgoing payment)
    Debit,
}

impl CreditTransferMessage {
    /// Create a credit transfer message from a Wrfcoin transaction
    pub fn from_transaction(
        transaction: &crate::blockchain::transaction::Transaction,
    ) -> Result<Self> {
        // In our transaction model, extract sender from first input and recipient from first output
        if transaction.inputs.is_empty() {
            return Err(anyhow!("Transaction has no inputs"));
        }

        if transaction.outputs.is_empty() {
            return Err(anyhow!("Transaction has no outputs"));
        }

        // For the sender, we'll use the public key if available,
        // otherwise use a placeholder with the transaction ID
        let sender = match &transaction.inputs[0].public_key {
            Some(pk) => format!("{}", pk),
            None => format!(
                "Unknown-{}",
                transaction
                    .id
                    .as_ref()
                    .map_or_else(|| "no-id".to_string(), |h| format!("{}", h))
            ),
        };

        // For the recipient, use the address from the first output
        let recipient = transaction.outputs[0].address.clone();

        // Create a new credit transfer message
        Ok(Self {
            message_id: Uuid::new_v4().to_string(),
            creation_date_time: Utc::now(),
            number_of_transactions: 1,
            instructing_agent: FinancialInstitutionId {
                bic: "WRFCOINDXXX".to_string(), // Default BIC for Wrfcoin
                clearing_system_id: None,
                name: Some("Wrfcoin Network".to_string()),
            },
            instructed_agent: FinancialInstitutionId {
                bic: "WRFCOINDXXX".to_string(), // Default BIC for Wrfcoin
                clearing_system_id: None,
                name: Some("Wrfcoin Network".to_string()),
            },
            transaction_info: CreditTransferTransactionInfo {
                payment_id: match &transaction.id {
                    Some(hash) => format!("{}", hash),
                    None => Uuid::new_v4().to_string(),
                },
                instructed_amount: Amount {
                    // Get the sum of outputs for the amount
                    value: transaction.outputs.iter().map(|o| o.amount as f64).sum(),
                    currency: "WRF".to_string(), // Wrfcoin currency code
                },
                debtor_account: AccountIdentification {
                    iban: None,
                    other: Some(sender),
                    account_type: Some("BLOCKCHAIN".to_string()),
                },
                creditor_account: AccountIdentification {
                    iban: None,
                    other: Some(recipient),
                    account_type: Some("BLOCKCHAIN".to_string()),
                },
                remittance_info: transaction.data.clone(),
            },
        })
    }

    /// Convert a credit transfer message to a Wrfcoin transaction
    pub fn to_transaction(&self) -> Result<crate::blockchain::transaction::Transaction> {
        // Extract the sender and recipient from the message
        let sender = self
            .transaction_info
            .debtor_account
            .other
            .clone()
            .ok_or_else(|| anyhow!("Missing debtor account identifier"))?;

        let recipient = self
            .transaction_info
            .creditor_account
            .other
            .clone()
            .ok_or_else(|| anyhow!("Missing creditor account identifier"))?;

        // Create a dummy hash for the input
        let dummy_hash = crate::crypto::hash::hash_string("dummy_transaction");

        // Create a transaction input
        let input = crate::blockchain::transaction::TransactionInput {
            txid: dummy_hash, // Reference to a previous transaction
            vout: 0,          // Output index in the previous transaction
            signature: None,  // No signature yet
            public_key: None, // No public key yet
            coinbase_data: Some(format!("ISO20022 transfer from {}", sender)),
        };

        // Create a transaction output to recipient
        let output = crate::blockchain::transaction::TransactionOutput {
            amount: self.transaction_info.instructed_amount.value as u64,
            address: recipient.clone(),
            script: "".to_string(), // Simple transfer
            nft_data: None,
        };

        // Create a new transaction with these inputs and outputs
        let mut transaction =
            crate::blockchain::transaction::Transaction::new(vec![input], vec![output]);

        // Set additional transaction fields
        // Create a proper Hash from the payment_id
        let payment_id = self.transaction_info.payment_id.clone();
        let hash = crate::crypto::hash::hash_bytes(payment_id.as_bytes());
        transaction.id = Some(hash);
        transaction.data = self.transaction_info.remittance_info.clone();
        // Create a UTC DateTime from the timestamp
        transaction.timestamp =
            chrono::DateTime::from_timestamp(self.creation_date_time.timestamp(), 0)
                .unwrap_or_else(|| chrono::Utc::now());

        Ok(transaction)
    }

    /// Convert the message to XML format
    pub fn to_xml(&self) -> Result<String> {
        let mut writer = Writer::new(Cursor::new(Vec::new()));

        // Document root
        let mut root = BytesStart::new("Document");
        root.push_attribute(("xmlns", "urn:iso:std:iso:20022:tech:xsd:pacs.008.001.09"));
        root.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
        writer.write_event(Event::Start(root))?;

        // FIToFICstmrCdtTrf element
        writer.write_event(Event::Start(BytesStart::new("FIToFICstmrCdtTrf")))?;

        // GrpHdr element
        writer.write_event(Event::Start(BytesStart::new("GrpHdr")))?;

        // Message ID
        writer.write_event(Event::Start(BytesStart::new("MsgId")))?;
        writer.write_event(Event::Text(BytesText::new(&self.message_id)))?;
        writer.write_event(Event::End(BytesEnd::new("MsgId")))?;

        // Creation date time
        writer.write_event(Event::Start(BytesStart::new("CreDtTm")))?;
        writer.write_event(Event::Text(BytesText::new(
            &self
                .creation_date_time
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string(),
        )))?;
        writer.write_event(Event::End(BytesEnd::new("CreDtTm")))?;

        // Number of transactions
        writer.write_event(Event::Start(BytesStart::new("NbOfTxs")))?;
        writer.write_event(Event::Text(BytesText::new(
            &self.number_of_transactions.to_string(),
        )))?;
        writer.write_event(Event::End(BytesEnd::new("NbOfTxs")))?;

        // Instructing agent
        writer.write_event(Event::Start(BytesStart::new("InstgAgt")))?;
        writer.write_event(Event::Start(BytesStart::new("FinInstnId")))?;
        writer.write_event(Event::Start(BytesStart::new("BICFI")))?;
        writer.write_event(Event::Text(BytesText::new(&self.instructing_agent.bic)))?;
        writer.write_event(Event::End(BytesEnd::new("BICFI")))?;
        writer.write_event(Event::End(BytesEnd::new("FinInstnId")))?;
        writer.write_event(Event::End(BytesEnd::new("InstgAgt")))?;

        // Instructed agent
        writer.write_event(Event::Start(BytesStart::new("InstdAgt")))?;
        writer.write_event(Event::Start(BytesStart::new("FinInstnId")))?;
        writer.write_event(Event::Start(BytesStart::new("BICFI")))?;
        writer.write_event(Event::Text(BytesText::new(&self.instructed_agent.bic)))?;
        writer.write_event(Event::End(BytesEnd::new("BICFI")))?;
        writer.write_event(Event::End(BytesEnd::new("FinInstnId")))?;
        writer.write_event(Event::End(BytesEnd::new("InstdAgt")))?;

        // End GrpHdr
        writer.write_event(Event::End(BytesEnd::new("GrpHdr")))?;

        // Credit Transfer Transaction Information
        writer.write_event(Event::Start(BytesStart::new("CdtTrfTxInf")))?;

        // Payment ID
        writer.write_event(Event::Start(BytesStart::new("PmtId")))?;
        writer.write_event(Event::Start(BytesStart::new("InstrId")))?;
        writer.write_event(Event::Text(BytesText::new(
            &self.transaction_info.payment_id,
        )))?;
        writer.write_event(Event::End(BytesEnd::new("InstrId")))?;
        writer.write_event(Event::End(BytesEnd::new("PmtId")))?;

        // Amount
        let mut amt_tag = BytesStart::new("IntrBkSttlmAmt");

        // Use tuple of (&str, &str) for attribute, which is the correct type
        amt_tag.push_attribute((
            "Ccy",
            self.transaction_info.instructed_amount.currency.as_ref(),
        ));

        writer.write_event(Event::Start(amt_tag))?;
        writer.write_event(Event::Text(BytesText::new(
            &self.transaction_info.instructed_amount.value.to_string(),
        )))?;
        writer.write_event(Event::End(BytesEnd::new("IntrBkSttlmAmt")))?;

        // Debtor (sender)
        writer.write_event(Event::Start(BytesStart::new("Dbtr")))?;
        writer.write_event(Event::Start(BytesStart::new("Id")))?;
        writer.write_event(Event::Start(BytesStart::new("OrgId")))?;
        writer.write_event(Event::Start(BytesStart::new("Othr")))?;
        writer.write_event(Event::Start(BytesStart::new("Id")))?;

        if let Some(ref other) = self.transaction_info.debtor_account.other {
            writer.write_event(Event::Text(BytesText::new(other)))?;
        }

        writer.write_event(Event::End(BytesEnd::new("Id")))?;
        writer.write_event(Event::End(BytesEnd::new("Othr")))?;
        writer.write_event(Event::End(BytesEnd::new("OrgId")))?;
        writer.write_event(Event::End(BytesEnd::new("Id")))?;
        writer.write_event(Event::End(BytesEnd::new("Dbtr")))?;

        // Creditor (recipient)
        writer.write_event(Event::Start(BytesStart::new("Cdtr")))?;
        writer.write_event(Event::Start(BytesStart::new("Id")))?;
        writer.write_event(Event::Start(BytesStart::new("OrgId")))?;
        writer.write_event(Event::Start(BytesStart::new("Othr")))?;
        writer.write_event(Event::Start(BytesStart::new("Id")))?;

        if let Some(ref other) = self.transaction_info.creditor_account.other {
            writer.write_event(Event::Text(BytesText::new(other)))?;
        }

        writer.write_event(Event::End(BytesEnd::new("Id")))?;
        writer.write_event(Event::End(BytesEnd::new("Othr")))?;
        writer.write_event(Event::End(BytesEnd::new("OrgId")))?;
        writer.write_event(Event::End(BytesEnd::new("Id")))?;
        writer.write_event(Event::End(BytesEnd::new("Cdtr")))?;

        // Remittance info
        if let Some(ref remittance_info) = self.transaction_info.remittance_info {
            writer.write_event(Event::Start(BytesStart::new("RmtInf")))?;
            writer.write_event(Event::Start(BytesStart::new("Ustrd")))?;
            writer.write_event(Event::Text(BytesText::new(remittance_info)))?;
            writer.write_event(Event::End(BytesEnd::new("Ustrd")))?;
            writer.write_event(Event::End(BytesEnd::new("RmtInf")))?;
        }

        // End CdtTrfTxInf
        writer.write_event(Event::End(BytesEnd::new("CdtTrfTxInf")))?;

        // End FIToFICstmrCdtTrf
        writer.write_event(Event::End(BytesEnd::new("FIToFICstmrCdtTrf")))?;

        // End Document
        writer.write_event(Event::End(BytesEnd::new("Document")))?;

        // Get the result
        let result = writer.into_inner().into_inner();
        let xml_string = String::from_utf8(result)?;

        Ok(xml_string)
    }
}
