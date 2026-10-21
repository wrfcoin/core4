# XCMP: Cross-Consensus Message Passing (Polkadot)

## Overview

Cross-Consensus Message Passing (XCMP) is Polkadot's native cross-chain communication protocol that
enables parachains to send messages to each other directly. XCMP allows for secure, trustless
communication between different blockchain networks within the Polkadot ecosystem.

## Architecture

### Core Components

1. **Parachains**: Independent blockchains connected to the Polkadot relay chain
2. **Relay Chain**: Central chain that coordinates message passing
3. **Collators**: Nodes that produce parachain blocks and facilitate XCMP
4. **Validators**: Secure the network and validate cross-chain messages

### Message Flow

```
Parachain A → XCMP Queue → Relay Chain → XCMP Queue → Parachain B
```

## XCM Format

### Cross-Consensus Message Structure

```rust
pub enum Xcm<Call> {
    WithdrawAsset {
        assets: MultiAssets,
        effects: Vec<Order<Call>>,
    },
    ReserveAssetDeposited {
        assets: MultiAssets,
        effects: Vec<Order<Call>>,
    },
    ReceiveTeleportedAsset {
        assets: MultiAssets,
        effects: Vec<Order<Call>>,
    },
    QueryResponse {
        query_id: u64,
        response: Response,
    },
    TransferAsset {
        assets: MultiAssets,
        beneficiary: MultiLocation,
    },
    // ... other message types
}
```

## Wrfcoin Parachain Integration

### Weather Data Cross-Parachain Transfer

```javascript
// Transfer weather data tokens between parachains
const xcmMessage = {
  version: 2,
  instructions: [
    {
      WithdrawAsset: {
        assets: [
          {
            id: {
              Concrete: {
                parents: 0,
                interior: {
                  X2: [
                    { PalletInstance: 50 }, // Weather data pallet
                    { GeneralIndex: 1 }, // WRF token index
                  ],
                },
              },
            },
            fun: { Fungible: '1000000000000' }, // 1000 WRF
          },
        ],
      },
    },
    {
      BuyExecution: {
        fees: {
          id: {
            Concrete: {
              parents: 1,
              interior: 'Here',
            },
          },
          fun: { Fungible: '10000000000' }, // Execution fee
        },
        weightLimit: 'Unlimited',
      },
    },
    {
      DepositAsset: {
        assets: { Wild: 'All' },
        maxAssets: 1,
        beneficiary: {
          parents: 0,
          interior: {
            X1: {
              AccountId32: {
                network: 'Any',
                id: recipientAccountId,
              },
            },
          },
        },
      },
    },
  ],
};

await weatherParachain.sendXcm(targetParachainId, xcmMessage);
```

### Weather Oracle Network

```javascript
class WeatherOracleXCMP {
  async broadcastWeatherUpdate(weatherData) {
    const xcmMessage = {
      version: 2,
      instructions: [
        {
          Transact: {
            originType: 'SovereignAccount',
            requireWeightAtMost: '5000000000',
            call: {
              encoded: this.encodeWeatherUpdate(weatherData),
            },
          },
        },
      ],
    };

    // Broadcast to all connected parachains
    const targetChains = await this.getConnectedChains();

    for (const chainId of targetChains) {
      await this.sendXcm(chainId, xcmMessage);
    }
  }

  encodeWeatherUpdate(data) {
    return {
      WeatherOracle: {
        UpdateWeatherData: {
          location: data.location,
          temperature: data.temperature,
          humidity: data.humidity,
          pressure: data.pressure,
          timestamp: data.timestamp,
          validators: data.validators,
        },
      },
    };
  }
}
```

## Asset Transfer Patterns

### Reserve Transfer

```javascript
// Reserve-backed asset transfer
const reserveTransfer = {
  dest: {
    parents: 1,
    interior: {
      X1: { Parachain: 2000 }, // Target parachain
    },
  },
  beneficiary: {
    parents: 0,
    interior: {
      X1: {
        AccountId32: {
          network: 'Any',
          id: beneficiaryAccount,
        },
      },
    },
  },
  assets: [
    {
      id: {
        Concrete: {
          parents: 0,
          interior: {
            X2: [{ PalletInstance: 50 }, { GeneralIndex: 1 }],
          },
        },
      },
      fun: { Fungible: amount },
    },
  ],
  feeAssetItem: 0,
};

await polkadotApi.tx.xcmPallet
  .reserveTransferAssets(
    reserveTransfer.dest,
    reserveTransfer.beneficiary,
    reserveTransfer.assets,
    reserveTransfer.feeAssetItem
  )
  .signAndSend(sender);
```

### Teleport Transfer

```javascript
// Teleport assets for trusted parachains
const teleportTransfer = {
  dest: {
    parents: 1,
    interior: { X1: { Parachain: 1000 } },
  },
  beneficiary: {
    parents: 0,
    interior: {
      X1: {
        AccountId32: {
          network: 'Any',
          id: receiverAccount,
        },
      },
    },
  },
  assets: [weatherTokenAsset],
  feeAssetItem: 0,
};

await polkadotApi.tx.xcmPallet
  .limitedTeleportAssets(
    teleportTransfer.dest,
    teleportTransfer.beneficiary,
    teleportTransfer.assets,
    teleportTransfer.feeAssetItem,
    'Unlimited'
  )
  .signAndSend(sender);
```

## Multi-Location Addressing

### Location Specification

```javascript
const locations = {
  // Relay chain
  relayChain: {
    parents: 1,
    interior: 'Here',
  },

  // Sibling parachain
  siblingChain: {
    parents: 1,
    interior: { X1: { Parachain: 2001 } },
  },

  // Account on current parachain
  localAccount: {
    parents: 0,
    interior: {
      X1: {
        AccountId32: {
          network: 'Any',
          id: accountId,
        },
      },
    },
  },

  // Pallet instance
  weatherPallet: {
    parents: 0,
    interior: {
      X1: { PalletInstance: 50 },
    },
  },
};
```

## Error Handling

### XCM Execution Errors

```javascript
class XCMErrorHandler {
  handleXcmResult(result) {
    if (result.isErr) {
      const error = result.asErr;

      switch (error.type) {
        case 'Overflow':
          console.error('Arithmetic overflow in XCM execution');
          break;
        case 'Unimplemented':
          console.error('XCM instruction not implemented');
          break;
        case 'UntrustedReserveLocation':
          console.error('Reserve location not trusted');
          break;
        case 'UntrustedTeleportLocation':
          console.error('Teleport location not trusted');
          break;
        case 'MultiLocationFull':
          console.error('MultiLocation has too many junctions');
          break;
        case 'MultiLocationNotInvertible':
          console.error('MultiLocation cannot be inverted');
          break;
        case 'BadOrigin':
          console.error('Bad origin for XCM execution');
          break;
        case 'InvalidLocation':
          console.error('Invalid location specified');
          break;
        case 'AssetNotFound':
          console.error('Asset not found');
          break;
        case 'FailedToTransactAsset':
          console.error('Failed to transact asset');
          break;
        default:
          console.error('Unknown XCM error:', error);
      }

      return false;
    }

    return true;
  }
}
```

## Security Considerations

### Trust Model

- **Reserve-based transfers**: Trust the reserve location
- **Teleport transfers**: Mutual trust between chains
- **Message validation**: Verify message authenticity
- **Weight limits**: Prevent resource exhaustion

### Best Practices

- Validate all incoming XCM messages
- Implement proper access controls
- Monitor cross-chain transaction flows
- Use appropriate transfer mechanisms for asset types

## Integration Examples

### Weather Data Marketplace

```javascript
class CrossChainWeatherMarketplace {
  async listWeatherData(data, targetChains) {
    const listingMessage = {
      version: 2,
      instructions: [
        {
          Transact: {
            originType: 'SovereignAccount',
            requireWeightAtMost: '3000000000',
            call: {
              encoded: this.encodeListingCall(data),
            },
          },
        },
      ],
    };

    // List on multiple parachains
    for (const chainId of targetChains) {
      await this.sendXcm(chainId, listingMessage);
    }
  }

  async purchaseWeatherData(listingId, sourceChain, paymentAsset) {
    const purchaseMessage = {
      version: 2,
      instructions: [
        {
          WithdrawAsset: {
            assets: [paymentAsset],
          },
        },
        {
          BuyExecution: {
            fees: paymentAsset,
            weightLimit: 'Limited(5000000000)',
          },
        },
        {
          Transact: {
            originType: 'SovereignAccount',
            requireWeightAtMost: '4000000000',
            call: {
              encoded: this.encodePurchaseCall(listingId),
            },
          },
        },
      ],
    };

    return await this.sendXcm(sourceChain, purchaseMessage);
  }
}
```

## Related Standards

- **XCM**: Cross-Consensus Message format
- **UMP**: Upward Message Passing (parachain to relay chain)
- **DMP**: Downward Message Passing (relay chain to parachain)
- **HRMP**: Horizontal Relay-routed Message Passing (parachain to parachain)
