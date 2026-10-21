import path from 'path';
import { loadConfig } from './chunker.js';

export function generateTags(filePath, content = '') {
  const config = loadConfig();
  const tags = new Set();

  // Directory-based tagging
  const relativePath = path.relative(process.cwd(), filePath);
  const pathParts = relativePath.split(path.sep);

  for (const [dir, dirTags] of Object.entries(
    config.tagging.directoryMappings
  )) {
    if (pathParts.some(part => part.includes(dir))) {
      dirTags.forEach(tag => tags.add(tag));
    }
  }

  // Keyword-based tagging
  const fileContent = content.toLowerCase();
  const fileName = path.basename(filePath).toLowerCase();

  for (const [keyword, keywordTags] of Object.entries(
    config.tagging.keywordMappings
  )) {
    if (fileContent.includes(keyword) || fileName.includes(keyword)) {
      keywordTags.forEach(tag => tags.add(tag));
    }
  }

  // File extension tagging
  const ext = path.extname(filePath);
  if (ext === '.rs') tags.add('rust');
  if (ext === '.js' || ext === '.ts') tags.add('javascript');
  if (ext === '.md') tags.add('documentation');
  if (filePath.includes('test')) tags.add('testing');

  return Array.from(tags);
}
