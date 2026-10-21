# Cross-Chain Operations - Schema Relationships

## Tables Overview

### `crosschain.cross_chain_bridges`

**Primary Key:** `id`

**Relationships:**

- one to_many with `crosschain.bridge_transactions` (via `bridge_id`)
- one to_many with `crosschain.cross_chain_transfers` (via `bridge_id`)

### `crosschain.axelar_chains`

**Primary Key:** `id`

**Relationships:**

- one to_many with `crosschain.axelar_transfers` (via `chain_id`)
- one to_many with `crosschain.axelar_messages` (via `chain_id`)

### `crosschain.layer_zero_chains`

**Primary Key:** `id`

**Relationships:**

- one to_many with `crosschain.layer_zero_messages` (via `chain_id`)
- one to_many with `crosschain.layer_zero_token_transfers` (via `chain_id`)
