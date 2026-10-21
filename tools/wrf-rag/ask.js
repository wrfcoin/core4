import { searchMemory } from './utils/mem0.js';
import { askCodebaseQuestion } from './utils/gemini.js';
import { loadConfig } from './utils/chunker.js';

export async function ask(query, options = {}) {
  console.log(`🔍 Searching for: "${query}"`);

  try {
    const config = loadConfig();
    const searchLimit = options.limit || 5;

    // Search memory for relevant context
    const memories = await searchMemory(query, {
      limit: searchLimit,
      user_id: 'wrfcoin-dev',
    });

    if (memories.length === 0) {
      console.log('❌ No relevant context found in memory.');
      console.log(
        '💡 Try indexing your codebase first with: npm run rag:index'
      );
      return;
    }

    console.log(`📚 Found ${memories.length} relevant context chunks`);

    // Display context sources
    console.log('\n📖 Context Sources:');
    memories.forEach((memory, index) => {
      const metadata = memory.metadata || {};
      const filename = metadata.filename || 'Unknown file';
      const lines =
        metadata.chunk_start_line && metadata.chunk_end_line
          ? `:${metadata.chunk_start_line}-${metadata.chunk_end_line}`
          : '';
      const tags = (metadata.tags || []).join(', ');

      console.log(
        `  ${index + 1}. ${filename}${lines} ${tags ? `[${tags}]` : ''}`
      );
    });

    // Query Gemini with context
    console.log('\n🤖 Generating response...');
    const result = await askCodebaseQuestion(query, memories, {
      model: config.api.model,
      temperature: config.api.temperature,
      maxTokens: config.api.maxTokens,
    });

    // Display result
    console.log('\n🧠 Answer:');
    console.log('='.repeat(60));
    console.log(result.answer);
    console.log('='.repeat(60));

    console.log(
      `\n📊 Context used: ${result.contextUsed} chunks, ~${result.tokensEstimated} tokens`
    );
  } catch (error) {
    console.error('❌ Query failed:', error.message);

    if (error.message.includes('MEM0_API_KEY')) {
      console.log('💡 Make sure MEM0_API_KEY is set in your environment');
    }

    if (error.message.includes('GOOGLE_API_KEY')) {
      console.log('💡 Make sure GOOGLE_API_KEY is set in your environment');
    }

    throw error;
  }
}

export async function askWithFilters(query, filters = {}) {
  console.log(`🔍 Filtered search for: "${query}"`);
  console.log(`🏷️  Filters:`, filters);

  try {
    // Search with filters
    const memories = await searchMemory(query, {
      limit: filters.limit || 5,
      user_id: 'wrfcoin-dev',
      filters: filters,
    });

    if (memories.length === 0) {
      console.log('❌ No relevant context found with those filters.');
      return;
    }

    return await ask(query, { contextMemories: memories });
  } catch (error) {
    console.error('❌ Filtered query failed:', error.message);
    throw error;
  }
}

export async function explainCode(filePath, lineStart, lineEnd) {
  const query = `Explain the code in ${filePath} from lines ${lineStart} to ${lineEnd}`;

  console.log(
    `🔍 Code explanation request: ${filePath}:${lineStart}-${lineEnd}`
  );

  return await ask(query, {
    limit: 3,
    contextFocus: 'code-explanation',
  });
}

export async function findSimilarCode(description) {
  const query = `Find code that ${description}`;

  console.log(`🔍 Similar code search: ${description}`);

  return await ask(query, {
    limit: 8,
    contextFocus: 'code-search',
  });
}
