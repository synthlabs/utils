#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { appendFileSync, readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

export const SOURCE_REPOSITORY = 'synthlabs/utils';
export const SUBMODULE_URL = 'https://github.com/synthlabs/utils.git';
const REGISTRY_URL = new URL('../.github/utils-consumers.json', import.meta.url);
const SHA_PATTERN = /^[0-9a-f]{40}$/;
const MAX_ATTEMPTS = 3;

export class GitHubApiError extends Error {
  constructor(message, status) {
    super(message);
    this.name = 'GitHubApiError';
    this.status = status;
  }
}

export function createGhApi({ env = process.env, execute = spawnSync } = {}) {
  return (method, path, body) => {
    const args = [
      'api', '--hostname', 'github.com', '--method', method, path,
      '--header', 'Accept: application/vnd.github+json',
      '--header', 'X-GitHub-Api-Version: 2026-03-10',
    ];
    if (body !== undefined) args.push('--input', '-');
    const token = path.startsWith(`repos/${SOURCE_REPOSITORY}/`)
      ? env.UTILS_READ_TOKEN || env.GH_TOKEN
      : env.GH_TOKEN;
    const result = execute('gh', args, {
      encoding: 'utf8',
      input: body === undefined ? undefined : JSON.stringify(body),
      env: { ...env, ...(token ? { GH_TOKEN: token } : {}) },
      maxBuffer: 8 * 1024 * 1024,
      timeout: 60_000,
    });
    if (result.error) throw new GitHubApiError(result.error.message);
    if (result.status !== 0) {
      let response;
      try { response = JSON.parse(result.stdout); } catch {}
      const status = Number(response?.status || result.stderr?.match(/\(HTTP (\d{3})\)/)?.[1]) || undefined;
      throw new GitHubApiError(response?.message || result.stderr?.trim() || `gh exited ${result.status}`, status);
    }
    return result.stdout.trim() ? JSON.parse(result.stdout) : null;
  };
}

export function validateRegistry(repositories) {
  if (!Array.isArray(repositories) || repositories.length === 0) {
    throw new Error('The consumer registry must be a nonempty array');
  }
  for (const repository of repositories) {
    if (typeof repository !== 'string' || !/^synthlabs\/[a-zA-Z0-9_.-]+$/.test(repository) || repository === SOURCE_REPOSITORY) {
      throw new Error(`Invalid consumer repository: ${repository}`);
    }
  }
  if (new Set(repositories).size !== repositories.length) throw new Error('Duplicate consumer repositories');
  return repositories;
}

export function readRegistry() {
  return validateRegistry(JSON.parse(readFileSync(REGISTRY_URL, 'utf8')));
}

export function validateSubmoduleConfig(content) {
  const expected = { path: 'utils', url: SUBMODULE_URL, branch: 'main' };
  for (const [key, value] of Object.entries(expected)) {
    const result = spawnSync('git', ['config', '--file', '-', '--get-all', `submodule.utils.${key}`], {
      input: content, encoding: 'utf8', timeout: 10_000,
    });
    if (result.error) throw result.error;
    if (result.status !== 0 || result.stdout.replace(/\r?\n$/, '') !== value) {
      throw new Error(`Expected submodule.utils.${key} = ${value} in .gitmodules`);
    }
  }
}

function requireSha(value, label) {
  if (!SHA_PATTERN.test(value ?? '')) throw new Error(`Invalid ${label} SHA`);
  return value;
}

function readMain(api, repository) {
  const ref = api('GET', `repos/${repository}/git/ref/heads/main`);
  if (ref?.object?.type !== 'commit') throw new Error(`${repository}/main is not a commit`);
  return requireSha(ref.object.sha, `${repository}/main`);
}

export function resolveTarget(api, requestedSha) {
  const currentMain = readMain(api, SOURCE_REPOSITORY);
  if (!requestedSha || requestedSha === currentMain) return currentMain;
  requireSha(requestedSha, 'target');
  const comparison = api('GET', `repos/${SOURCE_REPOSITORY}/compare/${requestedSha}...${currentMain}`);
  if (comparison.status !== 'ahead') throw new Error('The requested target is not on utils/main');
  return requestedSha;
}

function readConsumer(api, repository, mainSha) {
  const commit = api('GET', `repos/${repository}/git/commits/${mainSha}`);
  const treeSha = requireSha(commit.tree?.sha, `${repository} tree`);
  const tree = api('GET', `repos/${repository}/git/trees/${treeSha}`);
  if (tree.truncated || !Array.isArray(tree.tree)) throw new Error('Cannot inspect the complete consumer tree');
  const submodule = tree.tree.find(entry => entry.path === 'utils');
  if (submodule?.mode !== '160000' || submodule.type !== 'commit') {
    throw new Error('Expected utils to be an existing submodule; complete consumer onboarding first');
  }
  const gitmodules = tree.tree.find(entry => entry.path === '.gitmodules');
  if (gitmodules?.type !== 'blob' || !['100644', '100755'].includes(gitmodules.mode)) {
    throw new Error('Expected a regular .gitmodules file');
  }
  const blob = api('GET', `repos/${repository}/git/blobs/${gitmodules.sha}`);
  if (blob.encoding !== 'base64') throw new Error('Unexpected .gitmodules blob encoding');
  validateSubmoduleConfig(Buffer.from(blob.content, 'base64').toString('utf8'));
  return { treeSha, oldSha: requireSha(submodule.sha, 'utils gitlink') };
}

function shouldRetry(error) {
  return error instanceof GitHubApiError && (
    error.status === undefined || error.status === 409 || error.status === 422 ||
    error.status === 429 || error.status >= 500
  );
}

export function updateConsumer(api, { repository, targetSha, dryRun, identity }) {
  let oldSha;
  let error;
  let pendingCommit;
  for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
    let mainSha;
    try {
      mainSha = readMain(api, repository);
      const consumer = readConsumer(api, repository, mainSha);
      oldSha = consumer.oldSha;
      const result = { repository, oldSha, newSha: targetSha, attempts: attempt };
      if (oldSha === targetSha) {
        if (pendingCommit?.commitSha === mainSha) {
          return { ...result, oldSha: pendingCommit.oldSha, status: 'updated', commitSha: mainSha };
        }
        return { ...result, status: 'unchanged' };
      }
      const comparison = api('GET', `repos/${SOURCE_REPOSITORY}/compare/${oldSha}...${targetSha}`);
      if (comparison.status === 'behind') {
        const currentMain = readMain(api, SOURCE_REPOSITORY);
        if (oldSha !== currentMain) {
          const membership = api('GET', `repos/${SOURCE_REPOSITORY}/compare/${oldSha}...${currentMain}`);
          if (membership.status !== 'ahead') throw new Error('The consumer utils pin is not on utils/main');
        }
        return { ...result, status: 'ahead', newSha: oldSha };
      }
      if (comparison.status !== 'ahead') throw new Error('The consumer utils pin has divergent or unknown history');
      if (dryRun) return { ...result, status: 'would-update' };

      const tree = api('POST', `repos/${repository}/git/trees`, {
        base_tree: consumer.treeSha,
        tree: [{ path: 'utils', mode: '160000', type: 'commit', sha: targetSha }],
      });
      const commit = api('POST', `repos/${repository}/git/commits`, {
        message: `chore(deps): update utils to ${targetSha.slice(0, 7)}\n\nSource: https://github.com/${SOURCE_REPOSITORY}/commit/${targetSha}`,
        tree: requireSha(tree.sha, 'new tree'),
        parents: [mainSha],
        author: identity,
        committer: identity,
      });
      const commitSha = requireSha(commit.sha, 'new commit');
      pendingCommit = { commitSha, oldSha };
      api('PATCH', `repos/${repository}/git/refs/heads/main`, { sha: commitSha, force: false });
      return { ...result, status: 'updated', commitSha };
    } catch (caught) {
      error = caught;
      if (!shouldRetry(error) || attempt === MAX_ATTEMPTS) break;
      if (error.status === 409 || error.status === 422) {
        try {
          // An unchanged ref means rejection by validation or protection, not a race.
          if (!mainSha || readMain(api, repository) === mainSha) break;
        } catch (refError) {
          error = refError;
          break;
        }
      }
    }
  }
  return { repository, oldSha, newSha: targetSha, status: 'failed', error: error.message };
}

export function propagate(api, { repositories, targetSha, dryRun = true, identity }) {
  validateRegistry(repositories);
  const resolvedSha = resolveTarget(api, targetSha);
  if (!dryRun && (!identity?.name || !identity?.email)) throw new Error('Writing updates requires an App bot identity');
  const results = repositories.map(repository => updateConsumer(api, {
    repository, targetSha: resolvedSha, dryRun, identity,
  }));
  return { targetSha: resolvedSha, dryRun, results };
}

function markdownCell(value) {
  return String(value ?? '—').replaceAll('|', '\\|').replaceAll(/\r?\n/g, ' ').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
}

export function formatSummary({ targetSha, dryRun, results }) {
  const lines = [
    `## Utils submodule updates${dryRun ? ' (dry run)' : ''}`,
    '', `Target: [${targetSha.slice(0, 7)}](https://github.com/${SOURCE_REPOSITORY}/commit/${targetSha})`,
    '', '| Repository | Previous SHA | Resulting SHA | Result | Commit or error |',
    '| --- | --- | --- | --- | --- |',
  ];
  for (const result of results) {
    const detail = result.commitSha
      ? `[${result.commitSha.slice(0, 7)}](https://github.com/${result.repository}/commit/${result.commitSha})`
      : markdownCell(result.error);
    lines.push(`| ${result.repository} | ${result.oldSha?.slice(0, 7) || '—'} | ${result.status === 'failed' ? '—' : result.newSha.slice(0, 7)} | ${result.status} | ${detail} |`);
  }
  return `${lines.join('\n')}\n`;
}

export function parseArguments(args) {
  const options = { dryRun: true };
  let mode;
  for (let index = 0; index < args.length; index++) {
    const arg = args[index];
    if (arg === '--dry-run' || arg === '--write') {
      if (mode && mode !== arg) throw new Error('Choose either --dry-run or --write');
      mode = arg;
      options.dryRun = arg === '--dry-run';
    } else if (arg === '--target-sha') {
      if (options.targetSha) throw new Error('Specify --target-sha only once');
      options.targetSha = requireSha(args[++index], 'target');
    } else if (arg === '--list-repositories') {
      options.listRepositories = true;
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }
  return options;
}

function main() {
  const options = parseArguments(process.argv.slice(2));
  const repositories = readRegistry();
  if (options.listRepositories) {
    console.log(repositories.map(repository => repository.split('/')[1]).join(','));
    return;
  }
  const api = createGhApi();
  let identity;
  if (!options.dryRun) {
    const slug = process.env.UTILS_UPDATER_APP_SLUG;
    if (!process.env.GH_TOKEN || !/^[a-z0-9][a-z0-9-]*$/.test(slug ?? '')) {
      throw new Error('--write requires GH_TOKEN and UTILS_UPDATER_APP_SLUG from the updater App');
    }
    const bot = api('GET', `users/${encodeURIComponent(`${slug}[bot]`)}`);
    if (!Number.isSafeInteger(bot.id) || bot.type !== 'Bot') throw new Error('Cannot resolve the updater App bot');
    identity = { name: `${slug}[bot]`, email: `${bot.id}+${slug}[bot]@users.noreply.github.com` };
  }
  const summary = propagate(api, { ...options, repositories, identity });
  const markdown = formatSummary(summary);
  console.log(markdown);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, markdown);
  if (summary.results.some(result => result.status === 'failed')) process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try { main(); } catch (error) {
    console.error(`Utils propagation failed: ${error.message}`);
    process.exitCode = 1;
  }
}
