# Wrfcoin Node Strategy

**Date Created:** May 22, 2025  
**Last Edited:** May 26, 2025

## Enhanced Hierarchical Block Structure

Wrfcoin has evolved into a sophisticated multi-chain ecosystem with advanced node types supporting
13 cross-chain bridge protocols, AI-enhanced validation, and comprehensive IoT sensor network
integration. Our hierarchical structure includes multiple specialized block types validated by
appropriate node configurations: Block Types and Their Data

    Standard Blocks:
        Data: Basic transactions, wallet addresses, timestamps, simple metadata
        Verification: Validated by all node types
        Purpose: General blockchain maintenance and transaction processing
        Mining Time: Fastest (1-3 minutes)

    Checkpoint Blocks:
        Data: Block summaries, state validation, network integrity checks
        Verification: Only full Validator Nodes and Archive Nodes
        Purpose: Create secure reference points for SPV nodes
        Mining Time: Medium (15-30 minutes)
        Frequency: Generated every 100 standard blocks

    Data-Aware Blocks:
        Data: Weather sensor data, device readings, timestamps, location proofs
        Verification: Data Collection Nodes and Validator Nodes
        Purpose: Store environmental data with location proofs
        Mining Time: Medium-Fast (5-10 minutes)
        Contains: Data hashes, IPFS/Arweave links, sensor fingerprints

    Computational Blocks:
        Data: Forecast model inputs/outputs, computation results, model metadata
        Verification: Compute Nodes and specialized Validators
        Purpose: Store computational results with provenance
        Mining Time: Slow (30-60 minutes)
        Contains: Model specifications, input parameters, result hashes

    Cross-Chain Blocks:
        Data: Bridge transactions, multi-chain proofs, validator signatures
        Verification: Validator Nodes with cross-chain capabilities
        Purpose: Enable secure data transfer between chains
        Mining Time: Slowest (60+ minutes)
        Contains: Relay data, cross-chain transaction proofs

Node Roles and Block Responsibilities

    Validator Nodes:
        Build and verify: Standard, Checkpoint blocks
        Verify only: Data-Aware, Computational blocks (conditional)
        Responsible for: Network consensus, transaction validation

    Data Collection Nodes:
        Build and verify: Data-Aware blocks
        Verify only: Standard blocks
        Responsible for: Data ingestion, sensor validation, location proofs

    Compute Nodes:
        Build and verify: Computational blocks
        Verify only: Standard blocks, Data-Aware blocks
        Responsible for: Running forecast models, computational verification

    Storage Keeper Nodes:
        Build: None
        Verify: Data-Aware blocks, Computational blocks
        Responsible for: Distributed storage across IPFS/Filecoin/Arweave

    Light Clients/SPV Wallet Nodes:
        Build: None
        Verify: Headers only (SPV verification)
        Responsible for: User interaction, simplified verification

Pruning Schedule

The pruning process depends on block type and data importance:

    Standard Blocks:
        Light Clients: Keep only the most recent 1,000 headers
        Validator Nodes: Keep full blocks for the last 10,000 blocks
        Archive Nodes: No pruning (store everything)

    Data-Aware Blocks:
        Data relevance-based pruning:
            Critical environmental data: Never pruned
            Standard readings: Pruned after 1 year
            High-frequency readings: Compressed after 30 days, pruned after 90 days
        Location: Only high-importance geographic areas stored permanently

    Computational Blocks:
        Model inputs: Pruned after 30 days
        Model outputs (forecasts): Pruned after their validity period ends + 30 days
        Historic forecast verification data: Never pruned (stored for accuracy comparisons)

    Cross-Chain Blocks:
        Transaction proofs: Retained for 1 year
        Bridge state data: Never pruned

This hierarchical approach allows Wrfcoin to efficiently manage different types of data while
maintaining appropriate security and validation for each data type. The specialized node roles
ensure that each block type receives the necessary expertise for validation while the pruning
schedule helps manage the growth of the blockchain.
