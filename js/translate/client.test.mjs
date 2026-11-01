import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

function load(file, imports = {}) {
	const source = readFileSync(new URL(file, import.meta.url), 'utf8');
	const javascript = ts.transpileModule(source, {
		compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 }
	}).outputText;
	const module = { exports: {} };
	vm.runInNewContext(javascript, {
		module,
		exports: module.exports,
		Error,
		Set,
		Promise,
		AbortSignal,
		require: (name) => {
			if (name in imports) return imports[name];
			throw new Error(`Unexpected import: ${name}`);
		}
	});
	return module.exports;
}

const contract = load('./types.ts');
const request = { text: 'hello', target: 'eng', source: 'rus', detectionPolicy: 'reliable_only' };
const skipped = { kind: 'skipped', reason: { kind: 'blank_input' } };
const state = (revision) => ({ revision: String(revision), phase: 'ready', installation: null });

function deferred() {
	let resolve, reject;
	const promise = new Promise((yes, no) => {
		resolve = yes;
		reject = no;
	});
	return { promise, resolve, reject };
}

function harness(options = {}) {
	const calls = [];
	const listeners = new Set();
	let removed = 0;
	const api = load('./client.ts', {
		'./types': contract,
		'@tauri-apps/api/core': {
			invoke: async (command, args) => {
				calls.push({ command, args });
				return options.invoke?.(command.replace('plugin:translate|', ''), args) ?? state(0);
			}
		},
		'@tauri-apps/api/event': {
			listen: async (event, callback) => {
				assert.equal(event, 'translate://state-changed');
				await options.listen?.();
				listeners.add(callback);
				return () => {
					removed++;
					listeners.delete(callback);
					options.unlisten?.(removed);
				};
			}
		}
	});
	return {
		...api,
		client: api.createTranslationClient(),
		calls,
		emit: (snapshot) => {
			for (const callback of listeners) callback({ payload: snapshot });
		},
		get removed() {
			return removed;
		},
		get listeners() {
			return listeners.size;
		}
	};
}

test('normalizes typed backend, raw string, Error, and malformed rejections', () => {
	const h = harness();
	for (const input of [
		{ code: 'busy', message: 'model leased' },
		'invalid args `request`',
		new Error('webview disconnected'),
		{ code: 'busy', message: 123 },
		{ code: 'busy', message: 'bad details', details: { actual: 123 } },
		{ code: 'invented', message: 'not a backend error' },
		null
	]) {
		const error = h.normalizeTranslationError(input);
		assert.ok(error instanceof h.TranslationClientError);
		assert.equal(error.cause, input);
		assert.equal(
			error.code,
			input?.code === 'busy' && typeof input.message === 'string' && !input.details ? 'busy' : 'ipc'
		);
	}
	const typed = new h.TranslationClientError('capacity', 'full');
	assert.equal(h.normalizeTranslationError(typed), typed);
});

test('preserves typed diagnostic details without accepting malformed fields', () => {
	const h = harness();
	const details = { actual: '129', limit: '128' };
	const error = h.normalizeTranslationError({
		code: 'input_too_long',
		message: 'input too long',
		details
	});
	assert.equal(error.code, 'input_too_long');
	assert.equal(error.details, details);
});

test('dispose attempts every cleanup even when one listener fails', async () => {
	const h = harness({
		unlisten: (count) => {
			if (count === 1) throw new Error('cleanup failed');
		}
	});
	await h.client.watchState(() => {});
	await h.client.watchState(() => {});
	await assert.rejects(h.client.dispose(), { code: 'ipc', message: 'cleanup failed' });
	assert.equal(h.removed, 2);
	assert.equal(h.listeners, 0);
});

test('raw invalid-argument strings reject instead of masquerading as backend errors', async () => {
	const h = harness({ invoke: () => Promise.reject('invalid args `pair`: missing field target') });
	await assert.rejects(
		h.client.removeModel({ source: 'rus' }),
		(error) => error.code === 'ipc' && error.message.includes('missing field target')
	);
});

test('translation resolves expected skips and retrieves the registered result', async () => {
	const h = harness({
		invoke: (command) => (command === 'submit_translation' ? 'translation-1' : skipped)
	});
	assert.equal(await h.client.translate(request), skipped);
	assert.equal(h.calls[0].command, contract.commands.submit_translation);
	assert.equal(h.calls[0].args.request, request);
	assert.equal(h.calls[1].command, contract.commands.translation_result);
	assert.equal(h.calls[1].args.id, 'translation-1');
	await h.client.dispose();
	assert.equal(h.calls.length, 2);
});

test('abort before submission performs no IPC', async () => {
	const h = harness();
	const abort = new AbortController();
	abort.abort();
	await assert.rejects(h.client.translate(request, { signal: abort.signal }), {
		code: 'cancelled'
	});
	assert.equal(h.calls.length, 0);
});

test('abort during registration cancels once the operation is acknowledged', async () => {
	const submitted = deferred();
	const h = harness({
		invoke: (command) => {
			if (command === 'submit_translation') return submitted.promise;
			if (command === 'translation_result')
				return Promise.reject({ code: 'cancelled', message: 'cancelled' });
			return null;
		}
	});
	const abort = new AbortController();
	const result = h.client.translate(request, { signal: abort.signal });
	const rejected = assert.rejects(result, { code: 'cancelled' });
	abort.abort();
	submitted.resolve('translation-acknowledged');
	await rejected;
	const cancellations = h.calls.filter(
		(call) => call.command === contract.commands.cancel_translation
	);
	assert.ok(cancellations.length >= 1);
	assert.ok(cancellations.every((call) => call.args.id === 'translation-acknowledged'));
});

test('dispose cancels pending registration but leaves app-owned installation running', async () => {
	const submitted = deferred();
	const operation = { id: 'install-1', finished: false };
	const h = harness({
		invoke: (command) => {
			if (command === 'start_install') return operation;
			if (command === 'submit_translation') return submitted.promise;
			if (command === 'translation_result')
				return Promise.reject({ code: 'cancelled', message: 'cancelled' });
			return null;
		}
	});
	assert.equal(await h.client.startInstall({ modelId: 'pinned-model' }), operation);
	const result = h.client.translate(request);
	const rejected = assert.rejects(result, { code: 'cancelled' });
	const disposed = h.client.dispose();
	submitted.resolve('translation-1');
	await disposed;
	await rejected;
	assert.ok(!h.calls.some((call) => call.command === contract.commands.cancel_install));
	await assert.rejects(h.client.status(), { code: 'disposed' });
});

test('subscribes before snapshot and prevents older snapshots from replacing newer events', async () => {
	const snapshot = deferred();
	const queried = deferred();
	const h = harness({
		invoke: () => {
			queried.resolve();
			return snapshot.promise;
		}
	});
	const received = [];
	const subscription = h.client.watchState((value) => received.push(value.revision));
	await queried.promise;
	h.emit(state('9007199254740993'));
	snapshot.resolve(state(1));
	const stop = await subscription;
	h.emit(state('9007199254740992'));
	h.emit(state('9007199254740994'));
	assert.deepEqual(received, ['9007199254740993', '9007199254740994']);
	stop();
	stop();
	assert.equal(h.removed, 1);
	assert.equal(h.listeners, 0);
});

test('dispose during async listener registration cleans up its eventual listener', async () => {
	const registration = deferred();
	const h = harness({ listen: () => registration.promise });
	const subscription = h.client.watchState(() => assert.fail('disposed listener must stay silent'));
	const rejected = assert.rejects(subscription, { code: 'disposed' });
	await h.client.dispose();
	registration.resolve();
	await rejected;
	assert.equal(h.removed, 1);
	assert.equal(h.listeners, 0);
	assert.equal(h.calls.length, 0);
});

test('initial query failure cleans up listeners and preserves the normalized error', async () => {
	const h = harness({ invoke: () => Promise.reject('permission denied') });
	await assert.rejects(
		h.client.watchState(() => {}),
		{ code: 'ipc', message: 'permission denied' }
	);
	assert.equal(h.removed, 1);
});

test('listener failure and disposal during snapshot collection are handled', async () => {
	const failed = harness({ listen: () => Promise.reject('webview closed') });
	await assert.rejects(
		failed.client.watchState(() => {}),
		{ code: 'ipc' }
	);
	const snapshot = deferred();
	const queried = deferred();
	const h = harness({
		invoke: () => {
			queried.resolve();
			return snapshot.promise;
		}
	});
	const watch = h.client.watchState(() => assert.fail('late snapshot must be ignored'));
	await queried.promise;
	await h.client.dispose();
	snapshot.resolve(state(2));
	const stop = await watch;
	stop();
	assert.equal(h.removed, 1);
});

test('a new client can inspect an installation started by a disposed client', async () => {
	const operation = { id: 'install-1', finished: true, result: { status: 'installed' } };
	const h = harness({ invoke: () => operation });
	await h.client.startInstall({});
	await h.client.dispose();
	const next = h.createTranslationClient();
	assert.equal(await next.installation(operation.id), operation);
	await next.dispose();
});
