import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import {
  createGhApi, formatSummary, GitHubApiError, parseArguments, propagate,
  readRegistry, resolveTarget, SOURCE_REPOSITORY, SUBMODULE_URL,
  validateRegistry, validateSubmoduleConfig,
} from './update-utils-submodules.mjs';

const OLD = '1'.repeat(40);
const TARGET = '2'.repeat(40);
const NEWER = '3'.repeat(40);
const INITIAL_HEAD = 'a'.repeat(40);
const INITIAL_TREE = 'b'.repeat(40);
const CONFIG_BLOB = 'c'.repeat(40);
const OTHER_BLOB = 'd'.repeat(40);
const RACE_HEAD = 'f'.repeat(40);
const IDENTITY = { name: 'utils-updater[bot]', email: '123+utils-updater[bot]@users.noreply.github.com' };
const CONFIG = `[submodule "utils"]\n\tpath = utils\n\turl = ${SUBMODULE_URL}\n\tbranch = main\n`;

function fakeGitHub({ repositories = ['synthlabs/pepo'], pins = {}, config = CONFIG, sourceHead = TARGET,
  compare = () => 'ahead', intercept, beforePatch, afterPatch } = {}) {
  const calls = [];
  const states = new Map(repositories.map(repository => [repository, {
    head: INITIAL_HEAD, tree: INITIAL_TREE, pin: pins[repository] || OLD,
  }]));
  const trees = new Map();
  const commits = new Map();
  let sequence = 100;
  let successfulPatches = 0;
  const newSha = () => (sequence++).toString(16).padStart(40, '0');
  const api = (method, path, body) => {
    calls.push({ method, path, body: structuredClone(body) });
    intercept?.({ method, path, body, calls, states });
    if (method === 'GET' && path === `repos/${SOURCE_REPOSITORY}/git/ref/heads/main`) {
      return { object: { type: 'commit', sha: sourceHead } };
    }
    if (method === 'GET' && path.startsWith(`repos/${SOURCE_REPOSITORY}/compare/`)) {
      return { status: compare(path.split('/').pop()) };
    }
    const match = path.match(/^repos\/(synthlabs\/[^/]+)\/git\/(.+)$/);
    assert.ok(match, `Unexpected API path: ${path}`);
    const [, repository, operation] = match;
    const state = states.get(repository);
    assert.ok(state, `Unexpected consumer: ${repository}`);
    if (method === 'GET' && operation === 'ref/heads/main') {
      return { object: { type: 'commit', sha: state.head } };
    }
    if (method === 'GET' && operation === `commits/${state.head}`) {
      return { tree: { sha: state.tree } };
    }
    if (method === 'GET' && operation === `trees/${state.tree}`) {
      return { tree: [
        { path: '.gitmodules', type: 'blob', mode: '100644', sha: CONFIG_BLOB },
        { path: 'utils', type: 'commit', mode: '160000', sha: state.pin },
        { path: 'README.md', type: 'blob', mode: '100644', sha: OTHER_BLOB },
      ] };
    }
    if (method === 'GET' && operation === `blobs/${CONFIG_BLOB}`) {
      return { encoding: 'base64', content: Buffer.from(config).toString('base64') };
    }
    if (method === 'POST' && operation === 'trees') {
      const sha = newSha();
      trees.set(sha, structuredClone(body));
      return { sha };
    }
    if (method === 'POST' && operation === 'commits') {
      const sha = newSha();
      commits.set(sha, structuredClone(body));
      return { sha };
    }
    if (method === 'PATCH' && operation === 'refs/heads/main') {
      beforePatch?.({ repository, state, body, calls });
      const commit = commits.get(body.sha);
      assert.ok(commit);
      assert.equal(body.force, false);
      if (commit.parents[0] !== state.head) throw new GitHubApiError('Update is not a fast forward', 422);
      const tree = trees.get(commit.tree);
      assert.equal(tree.base_tree, state.tree);
      state.head = body.sha;
      state.tree = commit.tree;
      state.pin = tree.tree[0].sha;
      successfulPatches++;
      afterPatch?.({ state, body });
      return { object: { sha: body.sha } };
    }
    assert.fail(`Unexpected API call: ${method} ${path}`);
  };
  return { api, calls, states, commits, trees, get successfulPatches() { return successfulPatches; } };
}

function run(fake, options = {}) {
  return propagate(fake.api, {
    repositories: [...fake.states.keys()], dryRun: false, identity: IDENTITY, ...options,
  });
}

test('one update preserves the base tree and parent, changing only the utils gitlink', () => {
  const fake = fakeGitHub();
  const summary = run(fake);
  assert.equal(summary.results[0].status, 'updated');
  assert.equal(fake.successfulPatches, 1);
  assert.deepEqual([...fake.trees.values()][0], {
    base_tree: INITIAL_TREE,
    tree: [{ path: 'utils', mode: '160000', type: 'commit', sha: TARGET }],
  });
  const commit = [...fake.commits.values()][0];
  assert.deepEqual(commit.parents, [INITIAL_HEAD]);
  assert.deepEqual(commit.author, IDENTITY);
  assert.deepEqual(commit.committer, IDENTITY);
  assert.match(commit.message, /^chore\(deps\): update utils to 2222222\n\nSource: /);
  assert.doesNotMatch(commit.message, /Co-Authored-By|Generated with/);
  const patch = fake.calls.find(call => call.method === 'PATCH');
  assert.deepEqual(patch.body, { sha: summary.results[0].commitSha, force: false });
});

test('rerunning after success does not create another tree, commit, or ref update', () => {
  const fake = fakeGitHub();
  run(fake);
  const previousCalls = fake.calls.length;
  const summary = run(fake);
  assert.equal(summary.results[0].status, 'unchanged');
  assert.equal(fake.successfulPatches, 1);
  assert.ok(fake.calls.slice(previousCalls).every(call => call.method === 'GET'));
});

test('dry run and the default mode make no write requests', () => {
  for (const options of [{ dryRun: true }, {}]) {
    const fake = fakeGitHub();
    const summary = propagate(fake.api, { repositories: ['synthlabs/pepo'], ...options });
    assert.equal(summary.results[0].status, 'would-update');
    assert.ok(fake.calls.every(call => call.method === 'GET'));
    assert.equal(fake.commits.size, 0);
    assert.equal(fake.trees.size, 0);
  }
});

test('current pins are no-ops and pins ahead of the target never move backward', () => {
  const current = fakeGitHub({ pins: { 'synthlabs/pepo': TARGET } });
  assert.equal(run(current).results[0].status, 'unchanged');
  const newer = fakeGitHub({ pins: { 'synthlabs/pepo': NEWER }, sourceHead: NEWER,
    compare: range => range === `${TARGET}...${NEWER}` ? 'ahead' : 'behind',
  });
  const summary = run(newer, { targetSha: TARGET });
  assert.equal(summary.results[0].status, 'ahead');
  assert.equal(summary.results[0].newSha, NEWER);
  for (const fake of [current, newer]) {
    assert.ok(fake.calls.every(call => call.method === 'GET'));
  }
});

test('an off-main descendant is not accepted as an already-newer main pin', () => {
  const fake = fakeGitHub({ pins: { 'synthlabs/pepo': NEWER }, compare: () => 'behind' });
  const summary = run(fake);
  assert.equal(summary.results[0].status, 'failed');
  assert.match(summary.results[0].error, /not on utils\/main/);
  assert.ok(fake.calls.every(call => call.method === 'GET'));
});

test('divergent and unknown histories fail without writing', () => {
  for (const status of ['diverged', 'unknown', 'identical']) {
    const fake = fakeGitHub({ compare: () => status });
    const summary = run(fake);
    assert.equal(summary.results[0].status, 'failed');
    assert.match(summary.results[0].error, /divergent or unknown/);
    assert.ok(fake.calls.every(call => call.method === 'GET'));
  }
});

test('a concurrent main update is preserved by rebuilding from the new parent and tree', () => {
  let raced = false;
  const raceTree = '9'.repeat(40);
  const fake = fakeGitHub({ beforePatch: ({ state }) => {
    if (!raced) {
      raced = true;
      state.head = RACE_HEAD;
      state.tree = raceTree;
    }
  } });
  const summary = run(fake);
  assert.equal(summary.results[0].status, 'updated');
  assert.equal(summary.results[0].attempts, 2);
  assert.deepEqual([...fake.commits.values()].map(commit => commit.parents[0]), [INITIAL_HEAD, RACE_HEAD]);
  assert.deepEqual([...fake.trees.values()].map(tree => tree.base_tree), [INITIAL_TREE, raceTree]);
  assert.equal(fake.successfulPatches, 1);
});

test('repeated concurrent updates stop after three attempts', () => {
  let number = 700;
  const fake = fakeGitHub({ beforePatch: ({ state }) => {
    state.head = (number++).toString(16).padStart(40, '0');
    state.tree = (number++).toString(16).padStart(40, '0');
  } });
  const summary = run(fake);
  assert.equal(summary.results[0].status, 'failed');
  assert.equal(fake.calls.filter(call => call.method === 'PATCH').length, 3);
  assert.equal(fake.successfulPatches, 0);
});

test('a lost response after a successful ref update is reconciled without duplicate commits', () => {
  const fake = fakeGitHub({ afterPatch: () => { throw new GitHubApiError('Connection reset'); } });
  const summary = run(fake);
  assert.equal(summary.results[0].status, 'updated');
  assert.equal(summary.results[0].oldSha, OLD);
  assert.equal(summary.results[0].commitSha, fake.states.get('synthlabs/pepo').head);
  assert.equal(fake.successfulPatches, 1);
  assert.equal(fake.commits.size, 1);
});

test('a validation or protection rejection with an unchanged ref is not retried', () => {
  for (const status of [403, 409, 422]) {
    const fake = fakeGitHub({ intercept: ({ method }) => {
      if (method === 'PATCH') throw new GitHubApiError('Protected branch', status);
    } });
    const summary = run(fake);
    assert.equal(summary.results[0].status, 'failed');
    assert.equal(fake.calls.filter(call => call.method === 'PATCH').length, 1);
  }
});

test('a failing consumer does not prevent other consumers from updating', () => {
  const fake = fakeGitHub({ repositories: ['synthlabs/pepo', 'synthlabs/scrybe'],
    intercept: ({ path }) => {
      if (path.startsWith('repos/synthlabs/pepo/')) throw new GitHubApiError('Resource not accessible', 403);
    },
  });
  const summary = run(fake);
  assert.deepEqual(summary.results.map(result => result.status), ['failed', 'updated']);
  assert.equal(fake.states.get('synthlabs/pepo').pin, OLD);
  assert.equal(fake.states.get('synthlabs/scrybe').pin, TARGET);
});

test('authentication failures are reported without repeating the consumer request', () => {
  const fake = fakeGitHub({ intercept: ({ path }) => {
    if (path.startsWith('repos/synthlabs/pepo/')) throw new GitHubApiError('Bad credentials', 401);
  } });
  assert.equal(run(fake).results[0].status, 'failed');
  assert.equal(fake.calls.filter(call => call.path.startsWith('repos/synthlabs/pepo/')).length, 1);
});

test('missing onboarding, incorrect gitlink modes, and truncated trees are rejected', () => {
  for (const tree of [
    { tree: [] },
    { tree: [{ path: 'utils', mode: '040000', type: 'tree', sha: OLD }] },
    { truncated: true, tree: [] },
  ]) {
    const fake = fakeGitHub({ intercept: ({ path }) => {
      if (path.endsWith(`/trees/${INITIAL_TREE}`)) throw new Error(JSON.stringify(tree));
    } });
    const originalApi = fake.api;
    const api = (method, path, body) => path.endsWith(`/trees/${INITIAL_TREE}`) ? tree : originalApi(method, path, body);
    const summary = propagate(api, { repositories: ['synthlabs/pepo'], dryRun: false, identity: IDENTITY });
    assert.equal(summary.results[0].status, 'failed');
    assert.ok(fake.calls.every(call => call.method === 'GET'));
  }
});

test('bad committed submodule configuration blocks updates', () => {
  for (const config of [CONFIG.replace('branch = main', 'branch = develop'), CONFIG.replace(SUBMODULE_URL, 'git@github.com:synthlabs/utils.git')]) {
    const fake = fakeGitHub({ config });
    const summary = run(fake);
    assert.equal(summary.results[0].status, 'failed');
    assert.match(summary.results[0].error, /Expected submodule.utils/);
    assert.ok(fake.calls.every(call => call.method === 'GET'));
  }
});

test('Git parses quoted configuration and rejects missing, duplicate, and foreign settings', () => {
  validateSubmoduleConfig(CONFIG);
  validateSubmoduleConfig(CONFIG.replace('branch = main', 'branch = "main" # tracking'));
  validateSubmoduleConfig(`${CONFIG}\n[submodule "other"]\n path = other\n url = https://example.com/other.git\n`);
  for (const content of [
    CONFIG.replace('branch = main', ''), CONFIG.replace('path = utils', 'path = elsewhere'),
    `${CONFIG}\tbranch = develop\n`, CONFIG.replace('[submodule "utils"]', '[submodule "foreign"]'),
    CONFIG.replace('branch = main', 'branch = "main "'),
    '[submodule invalid\n',
  ]) assert.throws(() => validateSubmoduleConfig(content), /Expected submodule.utils/);
});

test('requested snapshots must belong to utils/main, including snapshots superseded during a run', () => {
  const fake = fakeGitHub({ sourceHead: NEWER });
  assert.equal(resolveTarget(fake.api, TARGET), TARGET);
  assert.equal(resolveTarget(fake.api), NEWER);
  assert.throws(() => resolveTarget(fakeGitHub({ compare: () => 'diverged' }).api, OLD), /not on utils\/main/);
  assert.throws(() => resolveTarget(fake.api, 'bad'), /Invalid target SHA/);
});

test('stale invocations resolve the current main tip and can converge all registered consumers', () => {
  const repositories = readRegistry();
  const fake = fakeGitHub({ repositories, sourceHead: NEWER });
  const summary = run(fake);
  assert.equal(summary.targetSha, NEWER);
  assert.equal(summary.results.length, 7);
  assert.ok(summary.results.every(result => result.status === 'updated' && result.newSha === NEWER));
});

test('the registry excludes utils itself, duplicates, other owners, and unsafe repository names', () => {
  assert.deepEqual(readRegistry(), [
    'synthlabs/blockbuster', 'synthlabs/pepo', 'synthlabs/pepo.chat', 'synthlabs/pointpit',
    'synthlabs/scrybe', 'synthlabs/scrybe.cc', 'synthlabs/sysmon',
  ]);
  for (const registry of [[], null, ['synthlabs/utils'], ['synthlabs/pepo', 'synthlabs/pepo'],
    ['other/pepo'], ['synthlabs/pepo/../../utils'], ['synthlabs/pepo\nmain']]) {
    assert.throws(() => validateRegistry(registry));
  }
});

test('source reads use the source token and consumer writes use the App token with structured stdin', () => {
  const invocations = [];
  const api = createGhApi({
    env: { GH_TOKEN: 'app-token', UTILS_READ_TOKEN: 'source-token' },
    execute: (command, args, options) => {
      invocations.push({ command, args, options });
      return { status: 0, stdout: '{}', stderr: '' };
    },
  });
  api('GET', `repos/${SOURCE_REPOSITORY}/git/ref/heads/main`);
  api('POST', 'repos/synthlabs/pepo/git/commits', { message: 'Literal `text` and $(text)\nNext line' });
  assert.ok(invocations.every(call => call.command === 'gh' && !call.options.shell));
  assert.equal(invocations[0].options.env.GH_TOKEN, 'source-token');
  assert.equal(invocations[1].options.env.GH_TOKEN, 'app-token');
  assert.deepEqual(invocations[1].args.slice(-2), ['--input', '-']);
  assert.deepEqual(JSON.parse(invocations[1].options.input), { message: 'Literal `text` and $(text)\nNext line' });
  assert.ok(invocations.every(call => !call.args.some(arg => arg.includes('token'))));
});

test('gh errors preserve HTTP status and native errors, and read-only local mode can use gh authentication', () => {
  const failingApi = createGhApi({ execute: () => ({ status: 1, stdout: '{"message":"Protected branch","status":"422"}', stderr: 'gh: Protected branch (HTTP 422)' }) });
  assert.throws(() => failingApi('PATCH', 'repos/synthlabs/pepo/git/refs/heads/main'), error => error instanceof GitHubApiError && error.status === 422);
  const missingApi = createGhApi({ execute: () => ({ error: new Error('gh not found') }) });
  assert.throws(() => missingApi('GET', 'anything'), /gh not found/);
  const nativeAuth = createGhApi({ env: {}, execute: (_command, _args, options) => {
    assert.equal(options.env.GH_TOKEN, undefined);
    return { status: 0, stdout: '{}', stderr: '' };
  } });
  nativeAuth('GET', 'repos/synthlabs/pepo/git/ref/heads/main');
});

test('CLI defaults to dry run and refuses ambiguous modes and malformed arguments', () => {
  assert.deepEqual(parseArguments([]), { dryRun: true });
  assert.deepEqual(parseArguments(['--write', '--target-sha', TARGET]), { dryRun: false, targetSha: TARGET });
  for (const args of [['--write', '--dry-run'], ['--target-sha'], ['--target-sha', TARGET, '--target-sha', OLD], ['--unknown']]) {
    assert.throws(() => parseArguments(args));
  }
  assert.throws(() => propagate(fakeGitHub().api, { repositories: ['synthlabs/pepo'], dryRun: false }), /App bot identity/);
});

test('the CLI exits unsuccessfully before network access if write credentials are missing', () => {
  const result = spawnSync(process.execPath, [new URL('./update-utils-submodules.mjs', import.meta.url).pathname, '--write'], {
    encoding: 'utf8', env: { ...process.env, GH_TOKEN: '', UTILS_UPDATER_APP_SLUG: '' },
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /requires GH_TOKEN and UTILS_UPDATER_APP_SLUG/);
});

test('summary records commits, failed consumers, and safe multiline errors', () => {
  const summary = formatSummary({ targetSha: TARGET, dryRun: false, results: [
    { repository: 'synthlabs/pepo', oldSha: OLD, newSha: TARGET, status: 'updated', commitSha: INITIAL_HEAD },
    { repository: 'synthlabs/scrybe', oldSha: OLD, newSha: TARGET, status: 'failed', error: 'Denied | reason\n<details>' },
  ] });
  assert.match(summary, /https:\/\/github.com\/synthlabs\/pepo\/commit\//);
  assert.match(summary, /synthlabs\/scrybe \| 1111111 \| — \| failed/);
  assert.match(summary, /Denied \\\| reason &lt;details&gt;/);
});
