import axios from 'axios';

const GEMINI_API_KEY = process.env.GOOGLE_API_KEY;
const GEMINI_BASE_URL =
  'https://generativelanguage.googleapis.com/v1beta/models';

if (!GEMINI_API_KEY) {
  console.warn(
    '⚠️  GOOGLE_API_KEY not found in environment. Query operations will fail.'
  );
}

export async function queryGemini(prompt, options = {}) {
  try {
    const model = options.model || 'gemini-2.5-flash';
    const temperature = options.temperature || 0.1;
    const maxTokens = options.maxTokens || 2048;

    const response = await axios.post(
      `${GEMINI_BASE_URL}/${model}:generateContent?key=${GEMINI_API_KEY}`,
      {
        contents: [
          {
            parts: [{ text: prompt }],
          },
        ],
        generationConfig: {
          temperature,
          maxOutputTokens: maxTokens,
          topK: 40,
          topP: 0.95,
        },
      },
      {
        headers: {
          'Content-Type': 'application/json',
        },
      }
    );

    const result = response.data?.candidates?.[0]?.content?.parts?.[0]?.text;

    if (!result) {
      throw new Error('No response generated from Gemini');
    }

    return result;
  } catch (error) {
    console.error('Gemini API error:', error.response?.data || error.message);

    // Fallback to simpler model if flash fails
    if (options.model !== 'gemini-2.5-pro' && error.response?.status === 400) {
      console.log('🔄 Retrying with Gemini Pro...');
      return await queryGemini(prompt, { ...options, model: 'gemini-2.5-pro' });
    }

    throw error;
  }
}

export function buildRAGPrompt(query, contextChunks) {
  const context = contextChunks
    .map((chunk, index) => {
      const metadata = chunk.metadata || {};
      return `## Context ${index + 1}: ${metadata.filename || 'Unknown file'}
**Lines:** ${metadata.chunk_start_line || '?'}-${metadata.chunk_end_line || '?'}
**Language:** ${metadata.language || 'unknown'}
**Tags:** ${(metadata.tags || []).join(', ') || 'none'}

\`\`\`${metadata.language || 'text'}
${chunk.memory || chunk.content || 'No content'}
\`\`\``;
    })
    .join('\n\n');

  return `You are an expert WRFCoin developer assistant. Use the following code context to answer the user's question accurately and concisely.

${context}

**User Question:** ${query}

**Instructions:**
- Answer based on the provided code context
- Include specific file names and line numbers when relevant
- If the context doesn't contain enough information, say so clearly
- Focus on practical, actionable information
- Use code examples from the context when helpful

**Answer:**`;
}

export async function askCodebaseQuestion(query, contextChunks, options = {}) {
  try {
    const prompt = buildRAGPrompt(query, contextChunks);
    const response = await queryGemini(prompt, options);

    return {
      answer: response,
      contextUsed: contextChunks.length,
      tokensEstimated: Math.ceil(prompt.length / 4),
    };
  } catch (error) {
    console.error('Failed to get codebase answer:', error.message);
    throw error;
  }
}
