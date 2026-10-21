import fs from 'fs';
import path from 'path';
import {
  adaptiveChunkFile,
  detectLanguage,
  loadConfig,
} from './utils/chunker.js';
import { getGitMetadata, isGitRepo } from './utils/git.js';
import { generateTags } from './utils/tags.js';
import { addToMemory } from './utils/mem0.js';

export async function indexDirectory(targetDirs, options = {}) {
  const config = loadConfig();
  const { parallel = false, full = false } = options;

  console.log(`📂 Indexing directories: ${targetDirs.join(', ')}`);
  console.log(
    `⚙️  Mode: ${full ? 'Full' : 'Incremental'}, Parallel: ${parallel}`
  );

  const allFiles = [];

  // Collect all files
  for (const dir of targetDirs) {
    const files = await walkDirectory(dir, config);
    allFiles.push(...files);
  }

  console.log(`📄 Found ${allFiles.length} files to process`);

  if (parallel) {
    await indexFilesParallel(allFiles, config);
  } else {
    await indexFilesSequential(allFiles, config);
  }

  console.log('✅ Indexing complete!');
}

async function walkDirectory(dirPath, config) {
  const files = [];

  const skipDirs = new Set(
    config.indexing?.skipDirectories ?? config.exclude ?? []
  );
  const watchExtensions = new Set(
    config.watching?.watchExtensions ?? config.extensions ?? []
  );

  async function walk(currentPath) {
    try {
      const stat = fs.statSync(currentPath);

      if (stat.isFile()) {
        // Handle single file
        const ext = path.extname(currentPath);
        if (watchExtensions.has(ext)) {
          files.push(currentPath);
        }
        return;
      }

      if (stat.isDirectory()) {
        const entries = fs.readdirSync(currentPath, { withFileTypes: true });

        for (const entry of entries) {
          const fullPath = path.join(currentPath, entry.name);

          if (entry.isDirectory()) {
            if (!skipDirs.has(entry.name) && !entry.name.startsWith('.')) {
              await walk(fullPath);
            }
          } else if (entry.isFile()) {
            const ext = path.extname(entry.name);
            if (watchExtensions.has(ext)) {
              files.push(fullPath);
            }
          }
        }
      }
    } catch (error) {
      console.warn(`⚠️  Skipping path ${currentPath}: ${error.message}`);
    }
  }

  await walk(path.resolve(dirPath));
  return files;
}

async function indexFilesSequential(files, config) {
  let processed = 0;

  for (const file of files) {
    try {
      await indexSingleFile(file, config);
      processed++;

      if (processed % 10 === 0) {
        console.log(`📊 Processed ${processed}/${files.length} files`);
      }
    } catch (error) {
      console.warn(`⚠️  Failed to index ${file}: ${error.message}`);
    }
  }
}

async function indexFilesParallel(files, config) {
  const batchSize = config.indexing?.batchSize ?? 50;
  const concurrency = config.indexing?.concurrency ?? 5;

  for (let i = 0; i < files.length; i += batchSize) {
    const batch = files.slice(i, i + batchSize);

    const promises = [];
    for (let j = 0; j < batch.length; j += concurrency) {
      const chunk = batch.slice(j, j + concurrency);
      const chunkPromise = Promise.all(
        chunk.map(file =>
          indexSingleFile(file, config).catch(error => {
            console.warn(`⚠️  Failed to index ${file}: ${error.message}`);
          })
        )
      );
      promises.push(chunkPromise);
    }

    await Promise.all(promises);
    console.log(
      `📊 Processed batch ${Math.floor(i / batchSize) + 1}/${Math.ceil(files.length / batchSize)}`
    );
  }
}

async function indexSingleFile(filePath, config) {
  try {
    const content = fs.readFileSync(filePath, 'utf-8');
    const language = detectLanguage(filePath);
    const tags = generateTags(filePath, content);
    const gitMetadata = isGitRepo() ? getGitMetadata(filePath) : {};
    const chunks = adaptiveChunkFile(content, filePath, config);

    let successCount = 0;
    for (const chunk of chunks) {
      const metadata = {
        language,
        tags,
        ...gitMetadata,
      };
      const result = await addToMemory(chunk, metadata);
      if (result.success !== false) {
        successCount++;
      }
    }

    const statusIcon =
      successCount === chunks.length ? '✅' : successCount > 0 ? '🟡' : '❌';
    const statusMsg =
      successCount === chunks.length
        ? 'Indexed'
        : successCount > 0
          ? `Partially indexed (${successCount}/${chunks.length})`
          : 'Failed to index';

    console.log(
      `${statusIcon} ${statusMsg} ${path.relative(process.cwd(), filePath)} (${chunks.length} chunks, ${language})`
    );
  } catch (error) {
    throw new Error(`Failed to index ${filePath}: ${error.message}`);
  }
}

export async function indexSingleFileUpdate(filePath) {
  const config = loadConfig();
  await indexSingleFile(filePath, config);
}
