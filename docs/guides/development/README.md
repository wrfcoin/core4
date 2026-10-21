# Wrfcoin Development Guide

Welcome to the Wrfcoin Development Guide! This comprehensive document will help you understand the
architecture, components, and development workflows for contributing to the Wrfcoin project.

## Table of Contents

1. [Getting Started](#getting-started)
2. [Architecture Overview](#architecture-overview)
3. [Core Components](#core-components)
4. [Database Integration](#database-integration)
5. [Frontend Development](#frontend-development)
6. [WebSocket Communication](#websocket-communication)
7. [Core Feature Development](#core-feature-development)
8. [Testing](#testing)
9. [Best Practices](#best-practices)
10. [Troubleshooting](#troubleshooting)

## Getting Started

### Prerequisites

To develop for Wrfcoin, you'll need:

- Node.js 20.x or later
- Rust 1.70 or later (for Tauri and blockchain core)
- PostgreSQL 15 or later
- Git

### Setup Instructions

1. **Clone the repository:**

```bash
git clone https://github.com/wrfcoin/wrfcoin-core.git
cd wrfcoin-core
```

2. **Install dependencies:**

```bash
npm install
```

3. **Set up the database:**

```bash
# Create a .env file with your PostgreSQL connection details
echo "DATABASE_URL=postgres://username:password@localhost:5432/wrfcoin" > .env

# Push the database schema
npm run db:push
```

4. **Start the development environment:**

```bash
# Start all services (recommended)
npm run start:testnet

# Or start individual components
npm run start:api        # API server
npm run start:ws         # WebSocket server
npm run start:frontend   # Frontend development server
```

## Architecture Overview

Wrfcoin Core is built with a modular architecture consisting of:

1. **Blockchain Core** (Rust): The foundational blockchain implementation with consensus mechanisms.
2. **API Server** (Node.js): RESTful and GraphQL APIs for interacting with the blockchain.
3. **WebSocket Server** (Node.js): Real-time updates and event notifications.
4. **Frontend** (React/Tauri): Desktop and web user interface.
5. **Database** (PostgreSQL/Drizzle): Persistent storage with ORM.

### Data Flow Architecture

```
┌────────────────┐      ┌────────────────┐      ┌────────────────┐
│  Blockchain    │◄────►│  API Server    │◄────►│  WebSocket     │
│  Core (Rust)   │      │  (Node.js)     │      │  Server        │
└───────┬────────┘      └───────┬────────┘      └───────┬────────┘
        │                       │                       │
        │                       ▼                       │
        │              ┌────────────────┐               │
        └─────────────►│  PostgreSQL    │◄──────────────┘
                       │  Database      │
                       └───────┬────────┘
                               │
                               ▼
                       ┌────────────────┐
                       │  Frontend      │
                       │  (React/Tauri) │
                       └────────────────┘
```

## Core Components

### Blockchain Core

The blockchain core implements:

- **Proof of Publishing (PoP)**: Consensus mechanism rewarding environmental data contributions.
- **Block Structure**: Includes transaction, validation, and timestamp data.
- **Transaction Types**: Regular, Computational, NFT, and Governance.
- **Validation Engine**: Confirms transaction and block validity.

### API Server

The API server provides:

- **RESTful Endpoints**: Standard HTTP endpoints for blockchain operations.
- **GraphQL API**: Flexible querying of blockchain data.
- **Authentication**: Secure access to protected endpoints.
- **Rate Limiting**: Protection against abuse.

### WebSocket Server

The WebSocket server delivers:

- **Real-Time Updates**: Block and transaction notifications.
- **State Changes**: Network health and statistics.
- **Subscription System**: Topic-based message delivery.

## Database Integration

Wrfcoin uses a PostgreSQL database with Drizzle ORM for structured data storage.

### Schema Overview

Key database tables include:

- `blocks`: Blockchain blocks
- `transactions`: All blockchain transactions
- `environmental_submissions`: Environmental data contributions
- `computational_nfts`: NFT metadata for computational models
- `computational_tasks`: Tasks associated with NFTs
- `users`: User accounts and preferences
- `achievements`: User achievement tracking

### Field Naming Convention

Wrfcoin uses a convention where:

- Application code uses **camelCase** for property names
- Database uses **snake_case** for column names

The `fieldNameConverter` utility is provided to convert between these formats:

```javascript
// In application code
import { toDbFieldNames, toAppFieldNames } from '@utils/fieldNameConverter';

// Convert to database format
const dbObject = toDbFieldNames({
  submissionId: '123',
  dataType: 'WEATHER',
  sensorAccuracy: 0.95,
});
// Result: { submission_id: '123', data_type: 'WEATHER', sensor_accuracy: 0.95 }

// Convert back to application format
const appObject = toAppFieldNames(dbResult);
```

### Database Operations

Use the provided database adapter in `server/db.js` for all database operations:

```javascript
import { db } from '@server/db';
import { submissions } from '@shared/schema';
import { eq } from 'drizzle-orm';

// Query example
const result = await db.select().from(submissions).where(eq(submissions.id, 1));

// Insert example
const newRecord = await db
  .insert(submissions)
  .values({
    submissionId: 'sub123',
    dataType: 'WEATHER',
    // other fields...
  })
  .returning();
```

## Frontend Development

### Component Structure

The frontend is organized into logical sections:

- `src/components/blockchain`: Blockchain visualizations and controls
- `src/components/environmental`: Environmental data submission forms
- `src/components/computational`: NFT creation and management
- `src/components/common`: Reusable UI elements

### State Management

Wrfcoin uses React Context API for state management:

- `BlockchainContext`: Blockchain state and operations
- `UserContext`: User preferences and settings
- `NotificationContext`: System notifications

### WebSocket Integration

Connect to the WebSocket server for real-time updates:

```javascript
import { useEffect, useState } from 'react';

function BlockchainMonitor() {
  const [blocks, setBlocks] = useState([]);

  useEffect(() => {
    // Establish WebSocket connection
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws`;
    const socket = new WebSocket(wsUrl);

    socket.onmessage = event => {
      const data = JSON.parse(event.data);
      if (data.event === 'BLOCK_CREATED') {
        setBlocks(prevBlocks => [...prevBlocks, data.data.block]);
      }
    };

    return () => socket.close();
  }, []);

  return (
    <div>
      <h2>Latest Blocks</h2>
      {/* Render blocks */}
    </div>
  );
}
```

## WebSocket Communication

### Event Types

The WebSocket server emits several event types:

| Event                 | Description              | Sample Payload                                         |
| --------------------- | ------------------------ | ------------------------------------------------------ |
| `BLOCK_CREATED`       | New block added to chain | `{ blockHeight: 1024, transactions: 5, ... }`          |
| `TRANSACTION_CREATED` | New transaction added    | `{ txId: "tx123", type: "Regular", amount: 100, ... }` |
| `STATE_UPDATE`        | Network state changed    | `{ blockHeight: 1024, hashrate: 1200, ... }`           |
| `PING`                | Heartbeat message        | `{ timestamp: "2025-05-14T01:40:45.250Z" }`            |

### Subscribing to Channels

WebSocket clients can subscribe to specific channels:

```javascript
// Subscribe to blockchain events
socket.send(
  JSON.stringify({
    action: 'SUBSCRIBE',
    channel: 'blockchain',
  })
);

// Subscribe to user-specific events
socket.send(
  JSON.stringify({
    action: 'SUBSCRIBE',
    channel: 'user',
    userId: 'user123',
  })
);
```

## Core Feature Development

### Adding a New Transaction Type

To add a new transaction type:

1. Define the transaction structure in `shared/types.ts`
2. Add validation rules in `server/validation/transactions.js`
3. Implement processing logic in `server/controllers/transactions.js`
4. Update the database schema in `shared/schema.ts`
5. Add frontend support in `src/components/transactions`

### Extending the Environmental Data System

To support a new environmental data type:

1. Add the new type to `shared/constants.js` in `ENVIRONMENTAL_DATA_TYPES`
2. Define measurement fields in `shared/schema.ts` under the `environmental_submissions` table
3. Create validation rules in `server/validation/environmental.js`
4. Add UI components in `src/components/environmental`
5. Update the GraphQL schema in `server/graphql/schema.js`

### Implementing a New Achievement

To create a new achievement:

1. Define the achievement in `shared/constants.js` in `ACHIEVEMENTS`
2. Implement tracking logic in `server/controllers/achievements.js`
3. Add database operations in `server/storage.js`
4. Create UI representation in `src/components/achievements`

## Testing

### Running Tests

To run tests:

```bash
# Run all tests
npm test

# Run specific test categories
npm run test:privacy
npm run test:pop
npm run test:computational
```

### Writing Tests

Follow these best practices for tests:

1. Place test files in the same directory as the module being tested
2. Name test files with a `.test.js` suffix
3. Use descriptive test names that explain the expected behavior
4. Include both positive and negative test cases
5. Mock external dependencies

Example test:

```javascript
// In computational.test.js
describe('Computational NFT Creation', () => {
  beforeEach(() => {
    // Setup test environment
  });

  it('should create a valid computational NFT with correct fields', async () => {
    const nft = {
      name: 'Test Model',
      description: 'A test computational model',
      computationType: 'WEATHER_FORECAST',
      // other fields...
    };

    const result = await createComputationalNFT(nft);

    expect(result).toHaveProperty('nftId');
    expect(result.computationType).toBe('WEATHER_FORECAST');
    // additional assertions...
  });

  it('should reject invalid computational type', async () => {
    const nft = {
      // ...fields with invalid type
      computationType: 'INVALID_TYPE',
    };

    await expect(createComputationalNFT(nft)).rejects.toThrow();
  });
});
```

## Best Practices

### Coding Standards

- Follow the Airbnb JavaScript style guide
- Use TypeScript for new components
- Maintain consistent naming conventions
- Document public APIs with JSDoc comments

### Git Workflow

- Create feature branches from `develop`
- Use conventional commit messages (`feat:`, `fix:`, `docs:`, etc.)
- Submit pull requests with comprehensive descriptions
- Include test cases for new features

### Performance Considerations

- Optimize database queries with appropriate indexes
- Use pagination for large data sets
- Implement caching for frequently accessed data
- Batch WebSocket updates when possible

## Troubleshooting

### Common Issues

#### Database Connectivity Issues

If you encounter database connection errors:

1. Verify your PostgreSQL service is running
2. Check your `.env` file for correct credentials
3. Ensure your database schema is up to date with `npm run db:push`

#### WebSocket Connection Problems

If WebSocket connections are failing:

1. Check the WebSocket server is running (`npm run start:ws`)
2. Verify your client is using the correct WebSocket URL and protocol
3. Look for CORS issues if connecting from a different domain

#### Field Name Conversion Errors

If you see errors related to field names:

1. Ensure you're using `toDbFieldNames` when writing to the database
2. Use `toAppFieldNames` when reading from the database
3. Check that all required fields are included in your objects

### Debugging Tips

- Use the debug logging with `DEBUG=wrfcoin:* npm run start:testnet`
- Examine database queries with `DEBUG=drizzle:* npm run start:testnet`
- For WebSocket issues, check browser developer tools Network tab
- Review logs in the `logs/` directory for historical issues

## Additional Resources

- [API Documentation](../api/README.md)
- [Database Schema Documentation](../database/README.md)
- [Frontend Component Library](../frontend/README.md)
- [Blockchain Specification](../blockchain/README.md)

---

For additional help or to report bugs, please join our [Discord channel](https://discord.gg/wrfcoin)
or create an issue on our [GitHub repository](https://github.com/wrfcoin/wrfcoin-core).
