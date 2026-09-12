import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

function load(name, mocks = {}, globals = {}) {
	const source = readFileSync(new URL(name, import.meta.url), 'utf8');
	const javascript = ts.transpileModule(source, {
		compilerOptions: {
			module: ts.ModuleKind.CommonJS,
			target: ts.ScriptTarget.ES2022
		}
	}).outputText;
	const module = { exports: {} };
	vm.runInNewContext(javascript, {
		module,
		exports: module.exports,
		Error,
		console,
		setTimeout,
		clearTimeout,
		require: (id) => {
			if (id in mocks) return mocks[id];
			throw new Error(`Unexpected import ${id}`);
		},
		...globals
	});
	return module.exports;
}

function harness() {
	let time = 0,
		sequence = 0;
	const timers = new Map();
	const api = load(
		'./controller.ts',
		{},
		{
			setTimeout(fn, duration) {
				const id = ++sequence;
				timers.set(id, { fn, at: time + duration });
				return id;
			},
			clearTimeout(id) {
				timers.delete(id);
			}
		}
	);
	const logs = [];
	const controller = new api.ErrorController({
		now: () => time,
		log: (error) => logs.push(error)
	});
	const stop = controller.start();
	return {
		...api,
		controller,
		logs,
		stop,
		advance(ms) {
			const end = time + ms;
			while (true) {
				const next = [...timers]
					.filter(([, value]) => value.at <= end)
					.sort((a, b) => a[1].at - b[1].at)[0];
				if (!next) break;
				time = next[1].at;
				timers.delete(next[0]);
				next[1].fn();
			}
			time = end;
		}
	};
}
const presentation = {
	source: 'home.start',
	title: 'Could not start',
	recovery: 'Try again'
};
const capture = (id, message = 'failure') => ({
	id,
	error: { kind: 'error', message, target: 'native', timestamp: '1' }
});
const notices = (h) => h.controller.snapshot().notifications;

test('repeated failures share one toast and its original deadline; later failures can notify again', () => {
	const h = harness();
	h.controller.capture('failure', presentation);
	h.advance(9000);
	h.controller.capture('failure', presentation);
	assert.equal(notices(h).length, 1);
	assert.equal(notices(h)[0].count, 2);
	h.advance(1000);
	assert.equal(notices(h).length, 0);
	h.controller.capture('failure', presentation);
	assert.equal(notices(h)[0].count, 1);
	h.stop();
});

test('hover and keyboard pauses retain remaining time until both end', () => {
	const h = harness();
	const incident = h.controller.capture('failure', presentation);
	h.advance(3000);
	h.controller.pause(incident.id, 'hover');
	h.controller.pause(incident.id, 'focus');
	h.advance(20000);
	h.controller.resume(incident.id, 'hover');
	h.advance(20000);
	assert.equal(notices(h).length, 1);
	h.controller.resume(incident.id, 'focus');
	h.advance(6999);
	assert.equal(notices(h).length, 1);
	h.advance(1);
	assert.equal(notices(h).length, 0);
});

test('inline failures retain diagnostics without a toast', () => {
	const h = harness();
	const error = {
		kind: 'device_unavailable',
		details: 'device disconnected'
	};
	const incident = h.controller.capture(error, { source: 'settings.devices' }, false);
	assert.equal(notices(h).length, 0);
	assert.equal(h.logs.length, 1);
	assert.deepEqual(JSON.parse(incident.error.message), error);
	h.controller.openReport(incident);
	assert.equal(h.controller.snapshot().report.error.message, incident.error.message);
});

test('native snapshot and live event overlap is deduplicated by incident ID', () => {
	const h = harness();
	h.controller.receive(capture('2'));
	h.controller.receive(capture('1', 'another failure'));
	h.controller.receive(capture('2'));
	assert.equal(notices(h).length, 2);
	assert.equal(notices(h)[0].count, 1);
	assert.equal(h.logs.length, 0);
	h.controller.dismiss('native-2');
	h.controller.receive(capture('2'));
	assert.equal(notices(h).length, 1);
	h.controller.receive(capture('3'));
	assert.equal(notices(h).length, 2);
	h.stop();
});

test('report selection is frozen and new notifications wait until the report closes', () => {
	const h = harness();
	const first = h.controller.capture('first error', presentation);
	h.controller.openReport(first);
	first.error.message = 'caller mutation';
	h.controller.capture('second error', presentation);
	h.controller.capture('second error', presentation);
	h.advance(30000);
	assert.equal(notices(h).length, 0);
	assert.equal(h.controller.snapshot().report.error.message, 'first error');
	h.controller.openReport(null);
	assert.equal(h.controller.snapshot().report.error.message, 'first error');
	h.controller.closeReport();
	assert.equal(notices(h).length, 1);
	assert.equal(notices(h)[0].count, 2);
	h.advance(9999);
	assert.equal(notices(h).length, 1);
	h.advance(1);
	assert.equal(notices(h).length, 0);
});

test('manual reports start without selecting an unrelated error', () => {
	const h = harness();
	h.controller.capture('unrelated', presentation);
	h.controller.openReport();
	assert.equal(h.controller.snapshot().report.error, null);
	h.stop();
});

test('stopping the presentation cancels timers and can safely resume', () => {
	const h = harness();
	h.controller.capture('failure', presentation);
	h.advance(2000);
	h.stop();
	h.advance(30000);
	assert.equal(notices(h).length, 1);
	h.controller.start();
	h.advance(8000);
	assert.equal(notices(h).length, 0);
});

function bridge(controller, listen, capturedErrors) {
	const listeners = new Map(),
		logs = [];
	const target = {
		addEventListener(name, callback) {
			listeners.set(name, callback);
		},
		removeEventListener(name) {
			listeners.delete(name);
		}
	};
	const api = load(
		'./capture.ts',
		{
			'@tauri-apps/api/event': { listen },
			'@tauri-apps/plugin-log': {
				error: async (message, options) => logs.push({ message, options })
			},
			'./client': { capturedErrors },
			'./controller': load('./controller.ts')
		},
		{ console: { error() {} } }
	);
	return {
		disconnect: api.connectErrorCapture(controller, target),
		listeners,
		logs
	};
}
const flush = () => new Promise((resolve) => setImmediate(resolve));

test('global exceptions and rejections use the shared controller; handled exceptions are not recaptured', async () => {
	const h = harness();
	const b = bridge(
		h.controller,
		async () => () => {},
		async () => []
	);
	const error = new Error('unexpected');
	b.listeners.get('error')({ error, message: error.message });
	b.listeners.get('unhandledrejection')({ reason: error });
	assert.equal(notices(h).length, 1);
	b.listeners.get('unhandledrejection')({ reason: 'another failure' });
	assert.equal(notices(h).length, 2);
	await flush();
	b.disconnect();
	assert.equal(b.listeners.size, 0);
	h.stop();
});

test('native subscription is installed before replay and cleaned up when mounting is interrupted', async () => {
	const h = harness();
	let resolveListen,
		stopped = 0,
		snapshots = 0;
	const b = bridge(
		h.controller,
		() => new Promise((resolve) => (resolveListen = resolve)),
		async () => {
			snapshots++;
			return [];
		}
	);
	b.disconnect();
	resolveListen(() => stopped++);
	await flush();
	assert.equal(stopped, 1);
	assert.equal(snapshots, 0);
});

test('capture bridge failures are logged as handled without a recursive error toast', async () => {
	const h = harness();
	const b = bridge(
		h.controller,
		async () => {
			throw new Error('bridge unavailable');
		},
		async () => []
	);
	await flush();
	assert.equal(notices(h).length, 0);
	assert.equal(b.logs[0].options.keyValues['inbound.handled'], 'true');
	b.disconnect();
});
