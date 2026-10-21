# Core Blockchain Operations - Schema Relationships

## Tables Overview

### `core.blocks`

**Primary Key:** `id`

**Relationships:**

- one to_many with `core.transactions` (via `block_id`)
- one to_many with `core.computational_work_proofs` (via `block_id`)

### `core.transactions`

**Primary Key:** `id`

**Relationships:**

- many to_one with `core.blocks` (via `block_id`)

### `core.computational_work_proofs`

**Primary Key:** `id`

**Relationships:**

- many to_one with `core.blocks` (via `block_id`)

### `core.trust_anchors`

**Primary Key:** `id`

**Relationships:**

- one to_many with `core.trust_endorsements` (via `anchor_id`)

### `core.trust_endorsements`

**Primary Key:** `id`

**Relationships:**

- many to_one with `core.trust_anchors` (via `anchor_id`)

### `core.shards`

**Primary Key:** `id`

**Relationships:**

- one to_many with `core.shard_rebalance_operations` (via `shard_id`)

### `core.shard_rebalance_operations`

**Primary Key:** `id`

**Relationships:**

- many to_one with `core.shards` (via `shard_id`)

### `core.slash_events`

**Primary Key:** `id`

**Relationships:** None (standalone table)
