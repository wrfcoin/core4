# Wrfcoin Database Schema Diagrams

This directory contains visual representations of the database schema relationships for the Wrfcoin
blockchain platform.

## Generated Files

### Schema-Specific Diagrams

- **core**: `core-schema.mermaid`, `core-schema.dot`, `core-relationships.md`
- **governance**: `governance-schema.mermaid`, `governance-schema.dot`,
  `governance-relationships.md`
- **rewards**: `rewards-schema.mermaid`, `rewards-schema.dot`, `rewards-relationships.md`
- **iot**: `iot-schema.mermaid`, `iot-schema.dot`, `iot-relationships.md`
- **crosschain**: `crosschain-schema.mermaid`, `crosschain-schema.dot`,
  `crosschain-relationships.md`
- **users**: `users-schema.mermaid`, `users-schema.dot`, `users-relationships.md`

### Overview

- **`schema-overview.mermaid`**: Complete system overview showing all schemas and their
  relationships

## File Formats

### Mermaid Files (\*.mermaid)

Entity-Relationship diagrams that can be rendered directly in GitHub, GitLab, and many documentation
tools.

To view locally, use:

- [Mermaid Live Editor](https://mermaid.live/)
- VS Code with Mermaid extension
- GitHub/GitLab (automatic rendering)

### DOT Files (\*.dot)

Graphviz format diagrams for high-quality rendering.

To render locally:

```bash
# Install Graphviz
apt install graphviz  # Ubuntu/Debian
brew install graphviz  # macOS

# Render diagram
dot -Tpng core-schema.dot -o core-schema.png
```

### Markdown Files (\*-relationships.md)

Human-readable documentation of table relationships and foreign key constraints.

## Usage

### For Developers

- Reference relationship diagrams when writing queries that join across tables
- Use for understanding data flow in the application
- Consult when designing new features that interact with existing schemas

### For AI Agents

- Parse relationship files to generate accurate test fixtures
- Use for automated code generation that respects foreign key constraints
- Reference for building schema-aware database operations

## Schema Organization

The Wrfcoin database is organized into 9 logical schemas:

1. **Core** - Blockchain infrastructure (blocks, transactions, consensus)
2. **Governance** - Democratic governance and voting systems
3. **Rewards** - Achievement tracking and economic incentives
4. **IoT** - Environmental sensors and weather station data
5. **CrossChain** - Bridge protocols and cross-chain transfers
6. **Users** - Identity management and wallet operations
7. **Analytics** - Performance metrics and data analysis
8. **Infra** - System management and deployment
9. **Oracle** - External data integration and services

---

_Generated automatically by the Wrfcoin schema diagram generator_
