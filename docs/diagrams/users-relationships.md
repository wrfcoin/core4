# User, Wallets, and Identity - Schema Relationships

## Tables Overview

### `users.users`

**Primary Key:** `id`

**Relationships:**

- one to_many with `users.user_operations` (via `user_id`)
- one to_many with `users.smart_wallets` (via `user_id`)
- one to_many with `users.reputation_events` (via `user_id`)
- one to_one with `users.reputation_scores` (via `user_id`)

### `users.decentralized_identities`

**Primary Key:** `id`

**Relationships:**

- one to_many with `users.verifiable_credentials` (via `identity_id`)
