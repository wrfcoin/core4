> **NOT the testnet API.** This client is generated from the backend developer-portal OpenAPI spec, not from the routes the testnet node (`launch_api`) serves, and is not published or kept in sync with the node. Do not assume its operations exist on the testnet. The hand-derived contract for the node is `docs/api/launch-api.openapi.yaml` (core4#3245); inventory and status: `docs/api/generated-code-inventory.md` (core4#2995).

# WRFCoin SDKs

This directory contains all Software Development Kits (SDKs) for the WRFCoin blockchain platform.

## Structure

### Generated SDKs (`generated/`)

Auto-generated SDK libraries from the OpenAPI specification covering all 37 API endpoints:

- **TypeScript** (`generated/typescript/`) - Complete REST API client with TypeScript support
- **Python** (`generated/python/`) - Python SDK with comprehensive API coverage
- **Rust** (`generated/rust/`) - Rust SDK for performance-critical applications

### Extensions (`extensions/`)

Custom functionality and UI components not covered by the auto-generated APIs:

#### JavaScript Extensions (`extensions/javascript/`)

- **Weather Validation** (`weather-validation/`) - Advanced weather data quality validation
- **IoT Devices** (`iot-devices/`) - IoT device management and integration

#### UI Components (`extensions/components/`)

- **React** (`react/`) - React components for WRFCoin integration
- **Vue** (`vue/`) - Vue.js components for blockchain interactions

### Deprecated (`deprecated/`)

Archived versions of previous SDK implementations:

- `sdk-original/` - Original comprehensive manual SDK
- `sdks-manual/` - Previous manual GraphQL-focused SDK

## Quick Start

### For TypeScript/JavaScript Projects

```bash
# Use the generated TypeScript SDK
cd sdks/generated/typescript
npm install
npm run build

# Add weather validation extensions
cd ../../extensions/javascript/weather-validation
# Import WeatherService for advanced validation
```

### For Python Projects

```bash
# Use the generated Python SDK
cd sdks/generated/python
pip install -r requirements.txt
pip install .
```

### React and Vue examples

The retained component trees under `extensions/components/` are unsupported historical examples, not
installable or released SDK packages.

## API Coverage

The generated SDKs provide complete coverage of:

- Blockchain operations (blocks, transactions, wallet)
- Weather data submission and rewards
- Smart contracts and NFT marketplace
- Mining and governance
- AI/ML model integration
- Performance monitoring and analytics
- Security and compliance features

## Documentation

- Generated SDK documentation is available in each `generated/*/docs/` directory
- API specification: `generated/*/api/openapi.yaml`
- Examples: `generated/*/examples/`

## Contributing

When adding new features:

1. Auto-generated SDKs are updated via OpenAPI spec changes
2. Custom extensions go in `extensions/` directory
3. UI components go in `extensions/components/`
4. Maintain backward compatibility where possible

## Support

For SDK issues or questions:

- Check the documentation in the relevant SDK directory
- Review examples in `examples/` folders
- File issues at the main repository
