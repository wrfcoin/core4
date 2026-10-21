# WRF-RAG: Enhanced RAG + Mem0 Integration for WRFCoin

A comprehensive RAG (Retrieval-Augmented Generation) system integrated with mem0 for the WRFCoin
codebase, providing semantic search and contextual AI assistance.

## Features

- **Adaptive Chunking**: Dynamic chunk sizes (40-120 lines) based on file size
- **Parallel Indexing**: Concurrent processing for large codebases (8000+ files)
- **Gemini Integration**: Uses Gemini 2.5 Flash/Pro for intelligent responses
- **Smart File Watcher**: Automatic re-indexing on file changes
- **Git Integration**: Tracks commit history and author information
- **Project-Specific Tagging**: Automatic tagging based on directory structure and content
- **Configurable Settings**: All parameters adjustable via `rag.config.json`

## Installation

```bash
cd tools/wrf-rag
npm install
```

## Environment Setup

Set the following environment variables:

```bash
export MEM0_API_KEY="your-mem0-api-key"
export GOOGLE_API_KEY="your-google-api-key"
```

## Usage

### Initialize Configuration

```bash
node wrf-rag.js config init
```

### Index Codebase

```bash
# Index specific directories
node wrf-rag.js index ./blockchain-core ./frontend ./docs

# Full parallel indexing
node wrf-rag.js index --full --parallel ./blockchain-core ./frontend ./docs
```

### Ask Questions

```bash
node wrf-rag.js ask "How does the replay log work in state.rs?"
node wrf-rag.js ask "What's the token economics implementation?"
node wrf-rag.js ask "Show me cross-chain bridge architecture"
```

### Watch for Changes

```bash
node wrf-rag.js watch ./blockchain-core ./frontend ./docs
```

### View Statistics

```bash
node wrf-rag.js stats
```

### Clean Old Memories

```bash
node wrf-rag.js clean --older-than=30d
```

## NPM Scripts

The following scripts are available in the main package.json:

- `npm run rag:init` - Initialize configuration
- `npm run rag:index` - Index with parallel processing
- `npm run rag:index:full` - Full parallel indexing
- `npm run rag:watch` - Start file watcher
- `npm run rag:ask` - Ask questions (requires quotes)
- `npm run rag:stats` - Show memory statistics
- `npm run rag:clean` - Clean old memories

## Configuration

Edit `rag.config.json` to customize:

### Chunking Rules

```json
{
  "chunking": {
    "rules": [
      { "maxLines": 300, "chunkSize": 40, "overlap": 10 },
      { "maxLines": 800, "chunkSize": 80, "overlap": 20 },
      { "maxLines": 99999, "chunkSize": 120, "overlap": 30 }
    ]
  }
}
```

### Directory Tagging

```json
{
  "tagging": {
    "directoryMappings": {
      "blockchain-core": ["blockchain", "core"],
      "smart-contracts": ["tokenomics", "contracts"],
      "frontend": ["ui", "frontend"],
      "cli": ["cli", "tools"]
    }
  }
}
```

### API Settings

```json
{
  "api": {
    "provider": "gemini",
    "model": "gemini-2.5-flash",
    "fallbackModel": "gemini-2.5-pro",
    "maxTokens": 2048,
    "temperature": 0.1
  }
}
```

## File Structure

```
tools/wrf-rag/
├── package.json              # Dependencies and scripts
├── rag.config.json          # Configuration
├── wrf-rag.js               # Main CLI entry point
├── index.js                 # Parallel indexing system
├── ask.js                   # Gemini-powered queries
├── watch.js                 # Smart file watcher
└── utils/
    ├── chunker.js           # Adaptive chunking
    ├── mem0.js              # Memory operations
    ├── gemini.js            # Gemini API integration
    ├── git.js               # Git metadata
    ├── tags.js              # Project tagging
    └── debounce.js          # Utility functions
```

## Key Features

### Adaptive Chunking

- **Small files** (< 300 lines): 40-line chunks with 10-line overlap
- **Medium files** (300-800 lines): 80-line chunks with 20-line overlap
- **Large files** (> 800 lines): 120-line chunks with 30-line overlap

### Performance Optimizations

- **Parallel processing** with configurable concurrency
- **Batch operations** for large file sets
- **Smart caching** and memory management
- **Debounced file watching** to prevent spam

### Intelligence Features

- **Context-aware responses** using Gemini 2.5
- **Git-integrated metadata** (commit, author, timestamp)
- **Project-specific tagging** based on directory structure
- **Semantic search** across the entire codebase

## Development Workflow

1. **Initial Setup**:

   ```bash
   npm run rag:init
   npm run rag:index:full
   ```

2. **Daily Use**:

   ```bash
   # Start file watcher in background
   npm run rag:watch &

   # Ask questions about codebase
   npm run rag:ask "What does this function do?"
   ```

3. **Maintenance**:
   ```bash
   npm run rag:stats    # Check memory usage
   npm run rag:clean    # Clean old entries
   ```

## Troubleshooting

### API Key Issues

- Ensure `MEM0_API_KEY` and `GOOGLE_API_KEY` are set
- Check that API keys are valid and not expired
- Verify API quotas haven't been exceeded

### Performance Issues

- Adjust `concurrency` and `batchSize` in config
- Use `--parallel` flag for large indexing operations
- Monitor memory usage during indexing

### File Watching Issues

- Check file permissions in watched directories
- Ensure `node_modules` and other build directories are excluded
- Verify file extensions are in `watchExtensions` list

## Integration with Claude Code

This RAG system is designed to work with Claude Code via the existing mem0 MCP integration. The
system provides:

- **Automated indexing** of your codebase
- **Intelligent chunking** for optimal retrieval
- **Context-aware responses** about your code
- **Real-time updates** as you modify files

For best results, use this in conjunction with Claude Code's native capabilities and the existing
mem0 MCP server.

## Architecture

The system follows a modular architecture:

1. **CLI Layer** (`wrf-rag.js`) - User interface and command routing
2. **Processing Layer** (`index.js`, `ask.js`, `watch.js`) - Core functionality
3. **Integration Layer** (`utils/`) - External API and service integrations
4. **Configuration Layer** (`rag.config.json`) - Centralized settings

This design ensures maintainability, testability, and extensibility for future enhancements.

## Note on API Integration

Currently, the system is configured to use direct API calls to mem0 and Gemini. If you encounter API
key issues, consider using the existing MCP integration for mem0 operations while keeping the Gemini
integration for intelligent responses.

Future updates will include better integration with the existing MCP infrastructure for seamless
operation within the Claude Code environment.
