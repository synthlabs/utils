import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { parse } from 'yaml';
import { checkContract } from '../js/testing/ci-contract.mjs';

const root = fileURLToPath(new URL('..', import.meta.url));
const pkg = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8'));
const testsYml = readFileSync(path.join(root, '.github/workflows/tests.yml'), 'utf8');
const updateUtilsYml = parse(
	readFileSync(path.join(root, '.github/workflows/update-utils.yml'), 'utf8')
);
const makefile = readFileSync(path.join(root, 'Makefile'), 'utf8');

test('utils tests.yml mirrors every suite in pnpm test, verify tokens, and Makefile forwarding', () => {
	const result = checkContract({
		pkg,
		workflows: [{ yaml: testsYml, job: 'verify' }],
		verifyTokens: ['pnpm check', 'pnpm lint', 'pnpm lint:rust', 'git diff --check'],
		makefile
	});

	assert.deepEqual(result.missingSuites, []);
	assert.deepEqual(result.unnamedSteps, []);
	assert.deepEqual(result.missingVerifyTokens, []);
	assert.deepEqual(result.missingMakefileTargets, []);
});

test('update-utils.yml gates propagation on tests.yml', () => {
	assert.equal(updateUtilsYml.jobs.verify.uses, './.github/workflows/tests.yml');
});
