import fs from 'fs';
import path from 'path';

export function loadConfig() {
  try {
    const configPath = path.join(process.cwd(), 'rag.config.json');
    return JSON.parse(fs.readFileSync(configPath, 'utf-8'));
  } catch (error) {
    console.error('Failed to load config:', error.message);
    return getDefaultConfig();
  }
}

function getDefaultConfig() {
  return {
    chunking: {
      rules: [
        { maxLines: 300, chunkSize: 40, overlap: 10 },
        { maxLines: 800, chunkSize: 80, overlap: 20 },
        { maxLines: 99999, chunkSize: 120, overlap: 30 },
      ],
    },
  };
}

export function adaptiveChunkFile(content, filename, config = null) {
  const conf = config || loadConfig();
  const lines = content.split('\n');
  const fileSize = lines.length;

  // Select chunking rule based on file size
  const rule =
    conf.chunking.rules.find(r => fileSize <= r.maxLines) ||
    conf.chunking.rules[conf.chunking.rules.length - 1];

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

function estimateTokens(text) {
  // Rough estimation: ~4 characters per token
  return Math.ceil(text.length / 4);
}

export function detectLanguage(filepath) {
  const ext = path.extname(filepath).toLowerCase();
  const langMap = {
    '.rs': 'rust',
    '.js': 'javascript',
    '.ts': 'typescript',
    '.tsx': 'typescript',
    '.jsx': 'javascript',
    '.md': 'markdown',
    '.json': 'json',
    '.toml': 'toml',
    '.yml': 'yaml',
    '.yaml': 'yaml',
  };
  return langMap[ext] || 'text';
}
