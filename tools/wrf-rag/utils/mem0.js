import axios from 'axios';

const MEM0_API_KEY = process.env.MEM0_API_KEY;
const MEM0_BASE_URL = 'https://api.mem0.ai';

if (!MEM0_API_KEY) {
  console.warn(
    '⚠️  MEM0_API_KEY not found in environment. Using local file cache instead.'
  );
}

export async function addToMemory(chunk, metadata = {}) {
  if (!MEM0_API_KEY) {
    // Use local file storage as fallback
    return await saveToLocalCache(chunk, metadata);
  }

  try {
    const response = await axios.post(
      `${MEM0_BASE_URL}/v1/memories/`,
      {
        messages: [{ role: 'user', content: chunk.content }],
        user_id: 'wrfcoin-dev',
        metadata: {
          filename: chunk.source,
          language: metadata.language || 'unknown',
          chunk_start_line: chunk.startLine,
          chunk_end_line: chunk.endLine,
          chunk_size: chunk.chunkSize,
          token_count: chunk.tokenCount,
          file_size_lines: chunk.fileSizeLines,
          tags: metadata.tags || [],
          commit_hash: metadata.commitHash,
          author: metadata.author,
          commit_timestamp: metadata.commitTimestamp,
          last_indexed: new Date().toISOString(),
          ...metadata,
        },
      },
      {
        headers: {
          Authorization: `Bearer ${MEM0_API_KEY}`,
          'Content-Type': 'application/json',
        },
      }
    );

    return response.data;
  } catch (error) {
    const errorData = error.response?.data;
    if (errorData?.code === 'token_not_valid') {
      console.error(
        '❌ MEM0_API_KEY is invalid or expired. Falling back to local cache.'
      );
      return await saveToLocalCache(chunk, metadata);
    }
    console.error('Failed to add to memory:', errorData || error.message);
    return { success: false, message: error.message };
  }
}

export async function searchMemory(query, options = {}) {
  try {
    const response = await axios.post(
      `${MEM0_BASE_URL}/v1/memories/search/`,
      {
        query,
        user_id: 'wrfcoin-dev',
        limit: options.limit || 5,
        ...options,
      },
      {
        headers: {
          Authorization: `Bearer ${MEM0_API_KEY}`,
          'Content-Type': 'application/json',
        },
      }
    );

    return response.data.results || [];
  } catch (error) {
    console.error(
      'Failed to search memory:',
      error.response?.data || error.message
    );
    throw error;
  }
}

export async function getMemoryStats() {
  if (!MEM0_API_KEY) {
    return await getLocalCacheStats();
  }

  try {
    const response = await axios.get(`${MEM0_BASE_URL}/v1/memories/`, {
      headers: {
        Authorization: `Bearer ${MEM0_API_KEY}`,
      },
      params: {
        user_id: 'wrfcoin-dev',
        limit: 1,
      },
    });

    return {
      totalMemories: response.data.total || 0,
      memorySize: response.data.results?.length || 0,
    };
  } catch (error) {
    const errorData = error.response?.data;
    if (errorData?.code === 'token_not_valid') {
      console.warn(
        '❌ MEM0_API_KEY is invalid or expired. Using local cache stats.'
      );
      return await getLocalCacheStats();
    }
    console.error(
      'Failed to get memory stats:',
      error.response?.data || error.message
    );
    return { totalMemories: 0, memorySize: 0 };
  }
}

export async function deleteOldMemories(olderThanDays = 30) {
  if (!MEM0_API_KEY) {
    return await cleanLocalCache(olderThanDays);
  }

  try {
    const cutoffDate = new Date();
    cutoffDate.setDate(cutoffDate.getDate() - olderThanDays);

    console.log(
      `🧹 Cleaning memories older than ${olderThanDays} days (before ${cutoffDate.toISOString()})`
    );

    // Note: This is a placeholder - actual implementation would need mem0's delete API
    console.log(
      'Memory cleanup functionality requires mem0 delete API implementation'
    );

    return { deleted: 0, message: 'Cleanup feature pending mem0 delete API' };
  } catch (error) {
    const errorData = error.response?.data;
    if (errorData?.code === 'token_not_valid') {
      console.warn(
        '❌ MEM0_API_KEY is invalid or expired. Using local cache cleanup.'
      );
      return await cleanLocalCache(olderThanDays);
    }
    console.error('Failed to clean old memories:', error.message);
    throw error;
  }
}

// Local file storage fallback functions
import fs from 'fs';
import path from 'path';

const CACHE_DIR = path.join(process.cwd(), '.rag-cache');

async function saveToLocalCache(chunk, metadata = {}) {
  try {
    // Create cache directory if it doesn't exist
    if (!fs.existsSync(CACHE_DIR)) {
      fs.mkdirSync(CACHE_DIR, { recursive: true });
    }

    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const fileName = `chunk_${timestamp}_${Math.random().toString(36).substr(2, 8)}.json`;
    const filePath = path.join(CACHE_DIR, fileName);

    const chunkData = {
      chunk: {
        content: chunk.content,
        source: chunk.source,
        startLine: chunk.startLine,
        endLine: chunk.endLine,
        chunkSize: chunk.chunkSize,
        tokenCount: chunk.tokenCount,
        fileSizeLines: chunk.fileSizeLines,
      },
      metadata: {
        user_id: 'wrfcoin-dev',
        filename: chunk.source,
        language: metadata.language || 'unknown',
        chunk_start_line: chunk.startLine,
        chunk_end_line: chunk.endLine,
        chunk_size: chunk.chunkSize,
        token_count: chunk.tokenCount,
        file_size_lines: chunk.fileSizeLines,
        tags: metadata.tags || [],
        commit_hash: metadata.commitHash,
        author: metadata.author,
        commit_timestamp: metadata.commitTimestamp,
        last_indexed: new Date().toISOString(),
        ...metadata,
      },
      timestamp: new Date().toISOString(),
      storage_type: 'local_cache',
    };

    fs.writeFileSync(filePath, JSON.stringify(chunkData, null, 2));
    console.log(
      `💾 Saved to local cache: ${path.relative(process.cwd(), chunk.source)}:${chunk.startLine}-${chunk.endLine}`
    );

    return { success: true, message: 'Saved to local cache', file: filePath };
  } catch (error) {
    console.error('Failed to save to local cache:', error.message);
    return { success: false, message: error.message };
  }
}

async function getLocalCacheStats() {
  try {
    if (!fs.existsSync(CACHE_DIR)) {
      return { totalMemories: 0, memorySize: 0 };
    }

    const files = fs.readdirSync(CACHE_DIR).filter(f => f.endsWith('.json'));
    const totalFiles = files.length;

    // Calculate total size
    let totalSize = 0;
    for (const file of files) {
      const filePath = path.join(CACHE_DIR, file);
      const stats = fs.statSync(filePath);
      totalSize += stats.size;
    }

    return {
      totalMemories: totalFiles,
      memorySize: Math.round(totalSize / 1024), // Size in KB
      cacheLocation: CACHE_DIR,
    };
  } catch (error) {
    console.error('Failed to get local cache stats:', error.message);
    return { totalMemories: 0, memorySize: 0 };
  }
}

async function cleanLocalCache(olderThanDays = 30) {
  try {
    if (!fs.existsSync(CACHE_DIR)) {
      return { deleted: 0, message: 'No cache directory found' };
    }

    const cutoffDate = new Date();
    cutoffDate.setDate(cutoffDate.getDate() - olderThanDays);

    const files = fs.readdirSync(CACHE_DIR).filter(f => f.endsWith('.json'));
    let deleted = 0;

    for (const file of files) {
      const filePath = path.join(CACHE_DIR, file);
      const stats = fs.statSync(filePath);

      if (stats.mtime < cutoffDate) {
        fs.unlinkSync(filePath);
        deleted++;
      }
    }

    return { deleted, message: `Cleaned ${deleted} files from local cache` };
  } catch (error) {
    console.error('Failed to clean local cache:', error.message);
    throw error;
  }
}
