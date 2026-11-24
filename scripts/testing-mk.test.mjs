import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const testingMk = path.join(root, 'configs/testing.mk').replaceAll('\\', '/');

const makeVersion = spawnSync('make', ['--version'], { encoding: 'utf8' });
const makeOptions = {
	skip: /GNU Make/.test(makeVersion.stdout ?? '') ? false : 'GNU Make is not available'
};

function dryRun(t, target, extraVars = []) {
	const dir = mkdtempSync(path.join(tmpdir(), 'utils-testing-mk-'));
	t.after(() => rmSync(dir, { recursive: true, force: true }));
	writeFileSync(path.join(dir, 'Makefile'), `include ${testingMk}\n`);

	const result = spawnSync('make', ['--no-print-directory', '-n', target, ...extraVars], {
		cwd: dir,
		encoding: 'utf8',
		env: { ...process.env, MAKEFLAGS: '', MFLAGS: '' }
	});
	assert.ifError(result.error);
	assert.equal(result.status, 0, result.stdout + result.stderr);
	return result.stdout.trim();
}

test(
	'testing.mk delegates test, verify, and hyphenated test-% targets to pnpm',
	makeOptions,
	(t) => {
		assert.equal(dryRun(t, 'test'), 'toke -v pnpm test');
		assert.equal(dryRun(t, 'verify'), 'toke -v pnpm verify');
		assert.equal(dryRun(t, 'test-unit'), 'toke -v pnpm run test:unit');
		assert.equal(dryRun(t, 'test-rust-build'), 'toke -v pnpm run test:rust:build');
	}
);

test(
	'testing.mk omits toke when TESTING_TOKE is empty and respects TESTING_PNPM',
	makeOptions,
	(t) => {
		assert.equal(
			dryRun(t, 'test-unit', ['TESTING_TOKE=', 'TESTING_PNPM=custom-pnpm']),
			'custom-pnpm run test:unit'
		);
	}
);
