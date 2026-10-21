#!/usr/bin/env node
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';
import fs from 'fs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// Load modules dynamically
async function loadModule(modulePath) {
  try {
    return await import(modulePath);
  } catch (error) {
    console.error(`Failed to load module ${modulePath}:`, error.message);
    process.exit(1);
  }
}

const [, , command, ...args] = process.argv;

async function main() {
  switch (command) {
    case 'config':
      if (args[0] === 'init') {
        const configPath = join(process.cwd(), 'rag.config.json');

        if (fs.existsSync(configPath)) {
          console.log('Configuration already exists at rag.config.json');
          console.log(
            'To modify settings, edit the file directly or use config commands'
          );
        } else {
          const defaultConfig = {
            user_id: 'wrfcoin-dev',
            chunking: {
              default: { size: 40, overlap: 10 },
              adaptive: [
                { maxLines: 300, size: 40, overlap: 10 },
                { maxLines: 800, size: 80, overlap: 20 },
                { maxLines: Infinity, size: 120, overlap: 30 },
              ],
            },
            exclude: ['node_modules', 'dist', 'build', '.git'],
            extensions: ['.js', '.ts', '.rs', '.md'],
            memory_tags: {
              'blockchain-core': ['blockchain', 'core'],
              cli: ['cli', 'tools'],
              frontend: ['ui', 'frontend'],
              docs: ['documentation'],
              forecast: ['forecasting'],
              'smart-contracts': ['contracts', 'tokenomics'],
            },
          };

          fs.writeFileSync(configPath, JSON.stringify(defaultConfig, null, 2));
          console.log('✅ Configuration created at rag.config.json');
        }
      } else {
        console.log('Available config commands: init');
      }
      break;

    case 'index':
      const { indexDirectory } = await loadModule('./index.js');
      const targetDirs =
        args.length > 0 ? args.filter(a => !a.startsWith('--')) : ['.'];
      const parallel = args.includes('--parallel');
      const full = args.includes('--full');

      console.log(
        `🚀 Starting ${full ? 'full' : 'incremental'} indexing${parallel ? ' (parallel)' : ''}...`
      );
      await indexDirectory(targetDirs, { parallel, full });
      break;

    case 'ask':
      const { ask } = await loadModule('./ask.js');
      const query = args.join(' ');
      if (!query) {
        console.log('❗ Usage: wrf-rag ask "your question here"');
        process.exit(1);
      }
      await ask(query);
      break;

    case 'watch':
      const { startWatcher } = await loadModule('./watch.js');
      const watchDirs = args.length > 0 ? args : ['../../'];
      console.log(`👁 Starting file watcher for: ${watchDirs.join(', ')}`);
      await startWatcher(watchDirs);
      break;

    case 'stats':
      const { getMemoryStats } = await loadModule('./utils/mem0.js');
      console.log('📊 RAG Statistics:');
      try {
        const stats = await getMemoryStats();
        console.log(`  Total memories: ${stats.totalMemories}`);
        console.log(`  Memory entries: ${stats.memorySize}`);
        console.log(`  User: wrfcoin-dev`);
      } catch (error) {
        console.log('  Unable to fetch stats:', error.message);
      }
      break;

    case 'clean':
      const { deleteOldMemories } = await loadModule('./utils/mem0.js');
      const days = args
        .find(arg => arg.startsWith('--older-than='))
        ?.split('=')[1];
      const olderThanDays = days ? parseInt(days.replace('d', '')) : 30;

      console.log('🧹 Cleaning old memories...');
      try {
        const result = await deleteOldMemories(olderThanDays);
        console.log(`✅ ${result.deleted} memories cleaned`);
        if (result.message) console.log(`💡 ${result.message}`);
      } catch (error) {
        console.log('❌ Cleanup failed:', error.message);
      }
      break;

    default:
      console.log(`Unknown command: ${command || 'none'}`);
      console.log('Usage: wrf-rag [config|index|ask|watch|stats|clean]');
      console.log('');
      console.log('Commands:');
      console.log('  config init    - Initialize configuration');
      console.log('  index [dirs]   - Index codebase directories');
      console.log('  ask "query"    - Ask questions about codebase');
      console.log('  watch [dirs]   - Watch for file changes');
      console.log('  stats          - Show indexing statistics');
      console.log('  clean          - Clean old memory entries');
      break;
  }
}

main().catch(console.error);
