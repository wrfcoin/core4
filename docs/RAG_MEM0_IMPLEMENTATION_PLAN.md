# Enhanced RAG + Mem0 Integration Plan for WRFCoin

_Revised with ChatGPT 4o feedback and Gemini API integration_

## Phase 1: Enhanced RAG CLI Tool Structure

### 1.1 Core CLI Tool (`wrf-rag.js`)

- Entry point with commands: `index`, `ask`, `watch`, `config`
- Integration with existing mem0 MCP server
- **NEW**: Configuration management for adaptive chunking rules

### 1.2 Enhanced Indexing System (`index.js`)

- **File Discovery**: Recursively scan directories for `.rs`, `.js`, `.ts`, `.md` files
- **Adaptive Chunking**: Size-aware chunking strategy:
  ```
  File Size (lines) | Chunk Size | Overlap
  < 300 lines      | 40 lines   | 10 lines
  300-800 lines    | 80 lines   | 20 lines
  > 800 lines      | 120 lines  | 30 lines
  ```
- **Parallel Processing**: Concurrent indexing using `Promise.all()` for performance
- **Token-Aware**: Future-proof with token counting for Gemini context limits
- **Git Integration**: Extract commit hash, author, timestamp for each file
- **Project Tags**: Auto-tag based on directory structure with configurable mappings

### 1.3 Enhanced Memory Schema

```json
{
  "user_id": "wrfcoin-dev",
  "metadata": {
    "filename": "blockchain-core/state.rs",
    "language": "rust",
    "chunk_start_line": 81,
    "chunk_end_line": 200,
    "chunk_size": 120,
    "token_count": 450,
    "tags": ["blockchain", "state-machine", "core"],
    "commit_hash": "a1b2c3d4",
    "author": "Tony Koop",
    "commit_timestamp": "2025-07-07T21:44:00Z",
    "file_size_lines": 850,
    "last_indexed": "2025-07-09T10:30:00Z"
  }
}
```

## Phase 2: Gemini-Powered Query System (`ask.js`)

- **Semantic Search**: Use mem0 MCP to search relevant code chunks
- **Context Assembly**: Combine top 3-5 chunks with file paths and line numbers
- **Gemini Integration**: Use existing `GOOGLE_API_KEY` with Gemini 2.5 Flash/Pro
- **Smart Context Management**: Token-aware prompt assembly to stay within limits
- **Developer-Focused**: Tailored for code analysis, debugging, architecture questions

### 2.1 Gemini API Integration

```javascript
// Use Gemini instead of Claude
const GEMINI_API_KEY = process.env.GOOGLE_API_KEY;
const model = 'gemini-2.5-flash'; // or gemini-2.5-pro for complex queries

async function sendToGemini(prompt) {
  // Google AI Studio API integration
  const response = await axios.post(
    `https://generativelanguage.googleapis.com/v1beta/models/${model}:generateContent?key=${GEMINI_API_KEY}`,
    {
      contents: [{ parts: [{ text: prompt }] }],
      generationConfig: { temperature: 0.1, maxOutputTokens: 2048 },
    }
  );
  return response.data?.candidates?.[0]?.content?.parts?.[0]?.text;
}
```

## Phase 3: Smart File Watcher (`watch.js`)

- **Technology**: Use `chokidar` for cross-platform file watching
- **Smart Monitoring**: Watch `.rs`, `.js`, `.ts`, `.md` files only
- **Debounced Re-indexing**: 2.5s delay to batch multiple changes
- **Memory Refresh Strategy**: De-duplicate old entries when content changes
- **Git-Diff Integration**: Future enhancement to track only staged/committed changes

## Phase 4: Configuration System (`rag.config.json`)

### 4.1 Adaptive Configuration

```json
{
  "chunking": {
    "rules": [
      { "maxLines": 300, "chunkSize": 40, "overlap": 10 },
      { "maxLines": 800, "chunkSize": 80, "overlap": 20 },
      { "maxLines": 99999, "chunkSize": 120, "overlap": 30 }
    ],
    "tokenLimit": 1000000,
    "preserveFunctions": true
  },
  "indexing": {
    "concurrency": 5,
    "batchSize": 50,
    "skipDirectories": ["node_modules", ".git", "target"]
  },
  "tagging": {
    "directoryMappings": {
      "blockchain-core": ["blockchain", "core"],
      "smart-contracts": ["tokenomics", "contracts"],
      "frontend": ["ui", "frontend"],
      "cli": ["cli", "tools"],
      "forecasting": ["forecasting", "ml"]
    },
    "keywordMappings": {
      "forecast": ["forecasting"],
      "token": ["tokenomics"],
      "consensus": ["consensus", "blockchain"],
      "bridge": ["cross-chain", "bridge"]
    }
  },
  "api": {
    "provider": "gemini",
    "model": "gemini-2.5-flash",
    "fallbackModel": "gemini-2.5-pro",
    "maxTokens": 2048,
    "temperature": 0.1
  }
}
```

## Phase 5: Enhanced Utilities

### 5.1 Adaptive Chunker (`utils/chunker.js`)

```javascript
export function adaptiveChunkFile(content, filename, config) {
  const lines = content.split('\n');
  const fileSize = lines.length;

  // Select chunking rule based on file size
  const rule =
    config.chunking.rules.find(r => fileSize <= r.maxLines) ||
    config.chunking.rules[config.chunking.rules.length - 1];

  const chunks = [];
  const { chunkSize, overlap } = rule;

  for (let i = 0; i < lines.length; i += chunkSize - overlap) {
    const slice = lines.slice(i, i + chunkSize);
    const tokenCount = estimateTokens(slice.join('\n'));

    chunks.push({
      content: slice.join('\n'),
      source: filename,
      startLine: i + 1,
      endLine: Math.min(i + chunkSize, lines.length),
      chunkSize: slice.length,
      tokenCount,
      fileSizeLines: fileSize,
    });
  }

  return chunks;
}
```

### 5.2 Function Boundary Parser (`utils/parser.js`)

- **Future Enhancement**: Use tree-sitter or regex for function-level chunking
- Language-specific parsing for Rust, TypeScript, JavaScript
- Preserve semantic boundaries for better context

### 5.3 Performance Optimizations (`utils/performance.js`)

```javascript
export async function parallelIndex(files, config) {
  const batches = chunk(files, config.indexing.batchSize);
  const concurrency = config.indexing.concurrency;

  for (let i = 0; i < batches.length; i += concurrency) {
    const batch = batches.slice(i, i + concurrency);
    await Promise.all(batch.map(files => processBatch(files)));
  }
}
```

## Phase 6: Integration Points

### 6.1 Package.json Scripts

```json
{
  "rag:init": "node tools/wrf-rag/wrf-rag.js config init",
  "rag:index": "node tools/wrf-rag/wrf-rag.js index --parallel",
  "rag:index:full": "node tools/wrf-rag/wrf-rag.js index --full --parallel",
  "rag:watch": "node tools/wrf-rag/wrf-rag.js watch",
  "rag:ask": "node tools/wrf-rag/wrf-rag.js ask",
  "rag:stats": "node tools/wrf-rag/wrf-rag.js stats",
  "rag:clean": "node tools/wrf-rag/wrf-rag.js clean --older-than=7d"
}
```

### 6.2 Environment Setup

- Use existing `GOOGLE_API_KEY` for Gemini API
- Use existing `MEM0_API_KEY` from `.mcp.json`
- No additional API keys required

## Phase 7: Enhanced Directory Structure

```
tools/wrf-rag/
├── package.json              # Dependencies: chokidar, axios, tiktoken
├── rag.config.json          # Configuration for chunking, tagging, API
├── wrf-rag.js               # Enhanced CLI entry point
├── index.js                 # Parallel indexing logic
├── ask.js                   # Gemini-powered query logic
├── watch.js                 # Smart file watcher
├── config.js                # Configuration management
└── utils/
    ├── chunker.js           # Adaptive chunking
    ├── parser.js            # Function boundary detection
    ├── performance.js       # Parallel processing
    ├── mem0.js              # Mem0 API wrapper
    ├── gemini.js            # Gemini API integration
    ├── git.js               # Git metadata extraction
    ├── tags.js              # Configurable tagging
    ├── tokens.js            # Token counting utilities
    └── debounce.js          # Debouncing utility
```

## Phase 8: Usage Workflow

### Initial Setup

```bash
cd tools/wrf-rag && npm install
npm run rag:init  # Create config file
npm run rag:index:full  # Full parallel indexing
```

### Daily Development

```bash
# Start smart file watcher
npm run rag:watch

# Ask contextual questions
npm run rag:ask "How does the replay log work in state.rs?"
npm run rag:ask "What's the token economics implementation?"
npm run rag:ask "Show me cross-chain bridge architecture"

# Get memory statistics
npm run rag:stats
```

## Phase 9: Key Improvements from Feedback

### 9.1 Adaptive Chunking

- **Small files** (< 300 lines): Fine-grained 40-line chunks
- **Medium files** (300-800 lines): Balanced 80-line chunks
- **Large files** (> 800 lines): Contextual 120-line chunks
- Configurable rules in `rag.config.json`

### 9.2 Performance Optimizations

- **Parallel indexing** with configurable concurrency
- **Batch processing** to handle 8000+ files efficiently
- **Token-aware chunking** for Gemini context limits
- **Memory refresh strategy** to avoid duplicates

### 9.3 Enhanced Configuration

- **Centralized config** for all parameters
- **Directory and keyword-based tagging**
- **API provider flexibility** (Gemini focused)
- **Chunking rule customization**

### 9.4 Future-Proofing

- **Function boundary detection** ready for tree-sitter integration
- **Git diff integration** for efficient updates
- **Token budget management** for large context scenarios
- **Memory cleanup strategies** for long-term usage

## Benefits

1. **Scalable**: Handles 8000+ files with parallel processing
2. **Adaptive**: Intelligent chunking based on file size
3. **Cost-Effective**: Uses existing Gemini API key
4. **Configurable**: All parameters adjustable via config file
5. **Performance-Optimized**: Parallel indexing and smart memory management
6. **Future-Ready**: Token-aware, function-boundary ready, git-integrated

This enhanced plan addresses ChatGPT's feedback while optimizing for your large codebase and
existing infrastructure.

## Implementation Steps

### Step 1: Project Setup

1. Create directory structure: `mkdir -p tools/wrf-rag/utils`
2. Initialize npm project: `cd tools/wrf-rag && npm init -y`
3. Install dependencies: `npm install chokidar axios`

### Step 2: Core Files Implementation

1. Create main CLI entry point (`wrf-rag.js`)
2. Implement configuration system (`config.js`, `rag.config.json`)
3. Build adaptive chunking system (`utils/chunker.js`)
4. Create Gemini API integration (`utils/gemini.js`)
5. Implement mem0 wrapper (`utils/mem0.js`)
6. Build git metadata extraction (`utils/git.js`)

### Step 3: Core Functionality

1. Implement parallel indexing system (`index.js`)
2. Create Gemini-powered query system (`ask.js`)
3. Build smart file watcher (`watch.js`)
4. Add utility functions (debounce, tagging, performance)

### Step 4: Integration

1. Add npm scripts to main package.json
2. Test indexing with small subset
3. Validate query functionality
4. Set up file watcher

### Step 5: Optimization & Testing

1. Test with full codebase indexing
2. Optimize performance parameters
3. Validate memory refresh strategies
4. Document usage patterns
