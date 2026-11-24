import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(new URL('./lint-locales.mjs', import.meta.url));

function withMessages(t, files) {
	const dir = mkdtempSync(path.join(tmpdir(), 'utils-i18n-lint-'));
	t.after(() => rmSync(dir, { recursive: true, force: true }));
	for (const [name, content] of Object.entries(files)) {
		const text = typeof content === 'string' ? content : JSON.stringify(content, null, 2);
		writeFileSync(path.join(dir, name), text, 'utf8');
	}
	return dir;
}

function runLint(args) {
	const result = spawnSync(process.execPath, [scriptPath, ...args], { encoding: 'utf8' });
	assert.ifError(result.error);
	return result;
}

test('lint-locales passes on matching key sets and ignores $schema', (t) => {
	const dir = withMessages(t, {
		'en.json': { $schema: 'https://inlang.com/schema/inlang-message-format', hello: 'Hello' },
		'ru.json': { hello: 'Привет' }
	});
	const result = runLint([dir]);
	assert.equal(result.status, 0, result.stderr);
	assert.match(result.stdout, /ru: 1 keys, parity with en/);
});

test('lint-locales fails when keys are missing or extra', (t) => {
	const dir = withMessages(t, {
		'en.json': { hello: 'Hello', bye: 'Bye' },
		'ru.json': { hello: 'Привет', extra: 'Лишнее' }
	});
	const result = runLint([dir]);
	assert.equal(result.status, 1);
	assert.match(result.stderr, /missing 1 key/);
	assert.match(result.stderr, /1 extra key/);
});

test('lint-locales supports a custom base locale and fails when base is missing', (t) => {
	const dir = withMessages(t, {
		'ru.json': { title: 'Заголовок' },
		'de.json': { title: 'Titel' }
	});
	assert.equal(runLint([dir, 'ru']).status, 0);
	const missingBase = runLint([dir]);
	assert.equal(missingBase.status, 1);
	assert.match(missingBase.stderr, /base locale "en" not found/);
});

test('lint-locales fails on invalid JSON or empty directories', (t) => {
	const invalidDir = withMessages(t, { 'en.json': '{not json' });
	const invalidResult = runLint([invalidDir]);
	assert.equal(invalidResult.status, 1);
	assert.match(invalidResult.stderr, /invalid JSON/);

	const emptyDir = withMessages(t, {});
	const emptyResult = runLint([emptyDir]);
	assert.equal(emptyResult.status, 1);
	assert.match(emptyResult.stderr, /no \.json files found/);
});
