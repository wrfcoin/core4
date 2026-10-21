import chokidar from 'chokidar';
import path from 'path';
import { debounce } from './utils/debounce.js';
import { indexSingleFileUpdate } from './index.js';
import { loadConfig } from './utils/chunker.js';
import {
  GracefulShutdown,
  ShutdownPriority,
} from '../../backend/shared/graceful-shutdown/index.js';

export async function startWatcher(watchDirs) {
  const config = loadConfig();
  const { debounceMs, watchExtensions } = config.watching;

  console.log(`👁️  Starting file watcher...`);
  console.log(`📁 Watching: ${watchDirs.join(', ')}`);
  console.log(`📄 Extensions: ${watchExtensions.join(', ')}`);
  console.log(`⏱️  Debounce: ${debounceMs}ms`);

  const watcher = chokidar.watch(watchDirs, {
    ignored: [
      /node_modules/,
      /\.git/,
      /\.cache/,
      /build/,
      /dist/,
      /target/,
      /\.DS_Store/,
      /\.vscode/,
      /\.idea/,
    ],
    persistent: true,
    ignoreInitial: true,
    awaitWriteFinish: {
      stabilityThreshold: 1000,
      pollInterval: 100,
    },
  });

  const pendingFiles = new Set();

  const debouncedReindex = debounce(async () => {
    if (pendingFiles.size === 0) return;

    console.log(`\n🔄 Processing ${pendingFiles.size} changed files...`);
    const filesToProcess = Array.from(pendingFiles);
    pendingFiles.clear();

    let processed = 0;
    let failed = 0;

    for (const filePath of filesToProcess) {
      try {
        // Check if file still exists and has valid extension
        const ext = path.extname(filePath);
        if (!watchExtensions.includes(ext)) {
          continue;
        }

        await indexSingleFileUpdate(filePath);
        processed++;

        const relativePath = path.relative(process.cwd(), filePath);
        console.log(`✅ Re-indexed: ${relativePath}`);
      } catch (error) {
        failed++;
        console.warn(`⚠️  Failed to re-index ${filePath}: ${error.message}`);
      }
    }

    console.log(`📊 Batch complete: ${processed} processed, ${failed} failed`);
  }, debounceMs);

  watcher.on('change', filePath => {
    const ext = path.extname(filePath);
    if (watchExtensions.includes(ext)) {
      pendingFiles.add(filePath);
      console.log(`📝 File changed: ${path.relative(process.cwd(), filePath)}`);
      debouncedReindex();
    }
  });

  watcher.on('add', filePath => {
    const ext = path.extname(filePath);
    if (watchExtensions.includes(ext)) {
      pendingFiles.add(filePath);
      console.log(`📄 File added: ${path.relative(process.cwd(), filePath)}`);
      debouncedReindex();
    }
  });

  watcher.on('unlink', filePath => {
    console.log(`🗑️  File deleted: ${path.relative(process.cwd(), filePath)}`);
    // TODO: Remove from memory when mem0 supports deletion
  });

  watcher.on('error', error => {
    console.error('❌ Watcher error:', error);
  });

  watcher.on('ready', () => {
    console.log('✅ File watcher is ready and monitoring changes');
    console.log('💡 Press Ctrl+C to stop watching');
  });

  // Register with GracefulShutdown for coordinated shutdown
  const shutdown = GracefulShutdown.getInstance({
    enableLogging: true,
    globalTimeout: 10000,
  });

  shutdown.registerHandler({
    name: 'wrf-rag-file-watcher',
    priority: ShutdownPriority.STOP_BACKGROUND_JOBS,
    timeout: 5000,
    handler: async () => {
      console.log('\n🛑 Stopping file watcher...');
      await watcher.close();
      console.log('✅ File watcher stopped');
    },
  });

  shutdown.setupSignalHandlers();

  return watcher;
}

export async function watchSingleDirectory(dirPath, options = {}) {
  const config = loadConfig();
  const watchExtensions = options.extensions || config.watching.watchExtensions;

  console.log(`👁️  Watching single directory: ${dirPath}`);

  const watcher = chokidar.watch(dirPath, {
    ignored: /node_modules|\.git/,
    persistent: true,
    ignoreInitial: true,
  });

  watcher.on('change', async filePath => {
    const ext = path.extname(filePath);
    if (watchExtensions.includes(ext)) {
      try {
        await indexSingleFileUpdate(filePath);
        console.log(`✅ Updated: ${path.relative(process.cwd(), filePath)}`);
      } catch (error) {
        console.warn(`⚠️  Failed to update ${filePath}: ${error.message}`);
      }
    }
  });

  return watcher;
}
