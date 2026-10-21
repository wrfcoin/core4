use crate::blockchain::transaction::{Operation, Transaction, TransactionType};

use super::types::{BridgeMessage, BridgeMessageType, CrossChainDirection, CrossChainNetwork};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CrossChainAdapterError {
    #[error("operation is not a cross-chain transfer")]
    NotCrossChainOperation,
    #[error("transaction type is not bridge-compatible")]
    UnsupportedTransactionType,
    #[error("missing recipient address for bridge transaction")]
    MissingRecipient,
}

pub fn operation_to_bridge_message(
    operation: &Operation,
    source_network: CrossChainNetwork,
    target_network: CrossChainNetwork,
    sender: String,
    nonce: u64,
    timestamp: i64,
) -> Result<BridgeMessage, CrossChainAdapterError> {
    match operation {
        Operation::CrossChainTransfer {
            target_chain,
            target_address,
            amount,
        } => {
            let mut message = BridgeMessage::new(
                source_network,
                target_network,
                CrossChainDirection::Outbound,
                BridgeMessageType::Lock,
                sender,
                target_address.clone(),
                *amount,
                nonce,
                timestamp,
            );
            message.payload = Some(serde_json::json!({
                "target_chain": target_chain,
            }));
            Ok(message)
        },
        _ => Err(CrossChainAdapterError::NotCrossChainOperation),
    }
}

pub fn bridge_message_to_operation(
    message: &BridgeMessage,
) -> Result<Operation, CrossChainAdapterError> {
    Ok(Operation::CrossChainTransfer {
        target_chain: message.target_network.to_string(),
        target_address: message.recipient.clone(),
        amount: message.amount,
    })
}

pub fn transaction_to_bridge_message(
    tx: &Transaction,
    source_network: CrossChainNetwork,
    target_network: CrossChainNetwork,
    nonce: u64,
) -> Result<BridgeMessage, CrossChainAdapterError> {
    if let Ok(message) = operation_to_bridge_message(
        &tx.operation,
        source_network.clone(),
        target_network.clone(),
        tx.sender_address.to_string(),
        nonce,
        tx.timestamp.timestamp(),
    ) {
        return Ok(message);
    }

    let recipient = tx
        .recipient_address
        .map(|addr| addr.to_string())
        .or_else(|| tx.outputs.first().map(|output| output.address.clone()))
        .ok_or(CrossChainAdapterError::MissingRecipient)?;
    let message_type = match tx.tx_type {
        TransactionType::BridgeLock => BridgeMessageType::Lock,
        TransactionType::BridgeUnlock => BridgeMessageType::Unlock,
        _ => return Err(CrossChainAdapterError::UnsupportedTransactionType),
    };
    let mut message = BridgeMessage::new(
        source_network,
        target_network,
        CrossChainDirection::Outbound,
        message_type,
        tx.sender_address.to_string(),
        recipient,
        tx.amount,
        nonce,
        tx.timestamp.timestamp(),
    );
    message.tx_hash = tx.id.as_ref().map(ToString::to_string);
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::transaction::{TransactionInput, TransactionOutput};

    #[test]
    fn cross_chain_operation_roundtrip() {
        let op = Operation::CrossChainTransfer {
            target_chain: "base".to_string(),
            target_address: "0xabc".to_string(),
            amount: 15,
        };
        let message = operation_to_bridge_message(
            &op,
            CrossChainNetwork::Ethereum,
            CrossChainNetwork::Base,
            "0xsender".to_string(),
            1,
            100,
        )
        .unwrap();
        let rebuilt = bridge_message_to_operation(&message).unwrap();
        assert_eq!(rebuilt, op);
    }

    #[test]
    fn bridge_lock_transaction_adapts_to_message() {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: Default::default(),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 100,
                address: "0xreceiver".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::BridgeLock;
        tx.amount = 100;

        let message = transaction_to_bridge_message(
            &tx,
            CrossChainNetwork::Ethereum,
            CrossChainNetwork::Base,
            9,
        )
        .unwrap();
        assert_eq!(message.message_type, BridgeMessageType::Lock);
        assert_eq!(message.recipient, "0xreceiver");
    }
}
