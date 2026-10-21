# Wrfcoin Core Module Dependencies

## Overview

This document outlines the dependency relationships between modules in the Wrfcoin Core codebase
after the migration of all functionality from the SDK to the core library. Understanding these
dependencies is crucial for maintaining a clean architecture, avoiding circular dependencies, and
ensuring efficient code organization.

## Major Modules

### Blockchain Module

The `blockchain` module is the central component that contains the core blockchain functionality.

**Provides:**

- Block and blockchain data structures
- State management
- Account handling
- NFT functionality (Standard and Computational)
- Transaction processing
- State transitions

**Dependencies:**

- `core` - Uses core utilities and common types
- `crypto` - For cryptographic operations

### Consensus Module

The `consensus` module contains various consensus algorithms used by the blockchain.

**Provides:**

- Proof of Work (PoW)
- Proof of Stake (PoS)
- Delegated Proof of Stake (DPoS)
- Proof of Participation (PoP)
- Proof of Incentive (PoI)
- Proof of Usefulness (PoU)

**Dependencies:**

- `blockchain` - Uses block and state structures
- `core` - For utilities and common types
- `crypto` - For signature verification and hashing

### Cross-Chain Module

The `cross_chain` module handles interoperability with other blockchains.

**Provides:**

- Bridge implementations (Axelar, LayerZero, Wormhole, etc.)
- Cross-chain transaction handling
- Message passing between chains

**Dependencies:**

- `blockchain` - Uses block and transaction structures
- `consensus` - For validation of cross-chain proofs
- `core` - For utilities

### Node Module

The `node` module manages node-related functionality.

**Provides:**

- Peer discovery and management
- Network communication
- Node lifecycle management

**Dependencies:**

- `blockchain` - For blockchain state
- `consensus` - For block validation
- `core` - For utilities

### Core Module

The `core` module provides common utilities and types used throughout the codebase.

**Provides:**

- Error handling
- Common types
- Utility functions
- Logging

**Dependencies:**

- No direct dependencies on other Wrfcoin modules (to avoid circular dependencies)

## Dependency Flow

Ideally, dependencies should flow in one direction to avoid circular dependencies. The general flow
should be:

```
core <- blockchain <- consensus/cross_chain/node
```

The `core` module should never depend on other modules. Other modules can depend on each other, but
circular dependencies should be avoided.

## Potential Areas of Concern

### Circular Dependencies

Circular dependencies can lead to compilation issues and make the codebase harder to understand and
maintain. The most common potential circular dependencies are:

1. `blockchain` and `consensus` modules: The consensus module needs to access blockchain data, but
   the blockchain module might need consensus functions for validation.

   **Solution:** Define clear interfaces in the blockchain module that consensus implementations can
   use.

2. `blockchain` and `cross_chain` modules: Cross-chain functionality needs blockchain data, but
   blockchain might need to handle cross-chain events.

   **Solution:** Use an event system where blockchain can emit events that cross-chain listeners
   respond to.

### Granularity

Modules should be fine-grained enough to provide clear separation of concerns but not so granular
that they create unnecessary complexity.

## Migration Impact

After migrating code from the SDK to the core library, we've consolidated functionality in the core
components. The SDK now imports directly from the core library, simplifying the codebase and
removing duplicate implementations.

## Best Practices

1. **Explicit Imports:** Always use explicit imports to make dependencies clear.
2. **Minimal Dependencies:** Each module should depend on as few other modules as possible.
3. **Avoid Circular Dependencies:** Design interfaces to prevent circular dependencies.
4. **Hide Implementation Details:** Expose only what's necessary through public APIs.
5. **Consistent Naming:** Use consistent naming conventions for modules and submodules.

## Relationship to SDK

The SDK now directly imports from the core library, using the public APIs exposed by the core
modules. This ensures consistent behavior between direct core library usage and SDK-mediated usage.

## Future Considerations

As the codebase evolves, we should regularly review the module dependencies to ensure they remain
clean and maintainable. Tools like the dependency graph visualization script in the `tools`
directory can help identify potential issues.
