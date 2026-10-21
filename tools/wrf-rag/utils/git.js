import { execSync } from 'child_process';
import path from 'path';

export function getGitMetadata(filePath) {
  try {
    const absolutePath = path.resolve(filePath);
    const commitHash = execSync(
      `git log -n 1 --pretty=format:%H -- "${absolutePath}"`,
      { encoding: 'utf-8' }
    ).trim();
    const author = execSync(
      `git log -n 1 --pretty=format:%an -- "${absolutePath}"`,
      { encoding: 'utf-8' }
    ).trim();
    const date = execSync(
      `git log -n 1 --pretty=format:%cI -- "${absolutePath}"`,
      { encoding: 'utf-8' }
    ).trim();

    return {
      commitHash: commitHash || null,
      author: author || null,
      commitTimestamp: date || null,
    };
  } catch (err) {
    return {
      commitHash: null,
      author: null,
      commitTimestamp: null,
    };
  }
}

export function isGitRepo() {
  try {
    execSync('git rev-parse --git-dir', { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}
