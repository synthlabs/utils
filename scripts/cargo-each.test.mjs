import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { discoverCrateDirs, runCargoEach } from './cargo-each.mjs';

function makeTempRustRoot(t, names) {
	const root = mkdtempSync(path.join(tmpdir(), 'utils-cargo-each-'));
	t.after(() => rmSync(root, { recursive: true, force: true }));
	for (const name of names) {
		const dir = path.join(root, name);
		mkdirSync(dir, { recursive: true });
		writeFileSync(path.join(dir, 'Cargo.toml'), `[package]\nname = "${name}"\n`);
	}
	mkdirSync(path.join(root, 'not-a-crate'), { recursive: true });
	return root;
}

function makeFakeCargo(t, logPath) {
	const dir = mkdtempSync(path.join(tmpdir(), 'utils-fake-cargo-'));
	t.after(() => rmSync(dir, { recursive: true, force: true }));
	const script = path.join(dir, 'fake-cargo.mjs');
	writeFileSync(
		script,
		`import { appendFileSync } from 'node:fs';
import path from 'node:path';
const crate = path.basename(process.cwd());
appendFileSync(${JSON.stringify(logPath)}, JSON.stringify({ crate, args: process.argv.slice(2) }) + '\\n');
if (process.env.FAKE_CARGO_FAIL === crate) process.exit(17);
`
	);
	if (process.platform === 'win32') {
		const cmd = path.join(dir, 'fake-cargo.cmd');
		writeFileSync(cmd, `@echo off\r\n"${process.execPath}" "${script}" %*\r\n`);
		return cmd;
	}
	const sh = path.join(dir, 'fake-cargo.sh');
	writeFileSync(sh, `#!/usr/bin/env sh\nexec "${process.execPath}" "${script}" "$@"\n`, {
		mode: 0o755
	});
	return sh;
}

test('discoverCrateDirs returns sorted crate directories containing Cargo.toml', (t) => {
	const rustRoot = makeTempRustRoot(t, ['zeta', 'alpha', 'beta']);
	assert.deepEqual(
		discoverCrateDirs(rustRoot).map((dir) => path.basename(dir)),
		['alpha', 'beta', 'zeta']
	);
});

test('runCargoEach forwards arguments per crate and stops on first failure', (t) => {
	const rustRoot = makeTempRustRoot(t, ['crate-a', 'crate-b', 'crate-c']);
	const logPath = path.join(rustRoot, 'calls.jsonl');
	const fakeCargo = makeFakeCargo(t, logPath);

	const statusOk = runCargoEach(['clippy', '--locked', '--', '-D', 'warnings'], {
		rustRoot,
		cargo: fakeCargo
	});
	assert.equal(statusOk, 0);
	const okCalls = readFileSync(logPath, 'utf8')
		.trim()
		.split('\n')
		.map((line) => JSON.parse(line));
	assert.deepEqual(okCalls, [
		{ crate: 'crate-a', args: ['clippy', '--locked', '--', '-D', 'warnings'] },
		{ crate: 'crate-b', args: ['clippy', '--locked', '--', '-D', 'warnings'] },
		{ crate: 'crate-c', args: ['clippy', '--locked', '--', '-D', 'warnings'] }
	]);

	rmSync(logPath, { force: true });
	process.env.FAKE_CARGO_FAIL = 'crate-b';
	try {
		const statusFail = runCargoEach(['test', '--locked'], { rustRoot, cargo: fakeCargo });
		assert.equal(statusFail, 17);
	} finally {
		delete process.env.FAKE_CARGO_FAIL;
	}
	const failCalls = readFileSync(logPath, 'utf8')
		.trim()
		.split('\n')
		.map((line) => JSON.parse(line));
	assert.deepEqual(
		failCalls.map((entry) => entry.crate),
		['crate-a', 'crate-b']
	);
});
