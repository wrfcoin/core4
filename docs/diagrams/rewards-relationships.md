# Achievements & Economic Incentives - Schema Relationships

## Tables Overview

### `rewards.achievements`

**Primary Key:** `id`

**Relationships:**

- many to_one with `rewards.achievement_categories` (via `category_id`)
- one to_many with `rewards.user_achievements` (via `achievement_id`)
- one to_many with `rewards.achievement_progress` (via `achievement_id`)

### `rewards.achievement_categories`

**Primary Key:** `id`

**Relationships:**

- one to_many with `rewards.achievements` (via `category_id`)

### `rewards.user_achievements`

**Primary Key:** `id`

**Relationships:**

- many to_one with `rewards.achievements` (via `achievement_id`)

### `rewards.mining_rewards`

**Primary Key:** `id`

**Relationships:**

- one to_many with `rewards.mining_contributions` (via `reward_id`)
