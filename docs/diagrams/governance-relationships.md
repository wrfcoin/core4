# Governance & Voting System - Schema Relationships

## Tables Overview

### `governance.governance_proposals`

**Primary Key:** `id`

**Relationships:**

- one to_many with `governance.governance_votes` (via `proposal_id`)

### `governance.governance_votes`

**Primary Key:** `id`

**Relationships:**

- many to_one with `governance.governance_proposals` (via `proposal_id`)

### `governance.governance_foundation_members`

**Primary Key:** `id`

**Relationships:**

- one to_many with `governance.governance_foundation_actions` (via `member_id`)

### `governance.governance_foundation_actions`

**Primary Key:** `id`

**Relationships:**

- many to_one with `governance.governance_foundation_members` (via `member_id`)

### `governance.governance_parameters`

**Primary Key:** `id`

**Relationships:**

- one to_many with `governance.governance_parameter_history` (via `parameter_id`)

### `governance.governance_parameter_history`

**Primary Key:** `id`

**Relationships:**

- many to_one with `governance.governance_parameters` (via `parameter_id`)
