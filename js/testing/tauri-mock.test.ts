import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import { emit, emitTo, listen, once } from '@tauri-apps/api/event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createTauriMock, installTauriMock, tauriInitScript, tauriModuleMocks } from './tauri-mock';

const testWindow = window as unknown as Record<string, unknown>;

afterEach(() => {
	delete testWindow.__TAURI_INTERNALS__;
	delete testWindow.__TAURI_EVENT_PLUGIN_INTERNALS__;
	delete testWindow.__tauriMock;
	delete testWindow.isTauri;
});

describe('installTauriMock with real @tauri-apps/api', () => {
	it('handles invoke, built-in event listen/unlisten/once/emit, isTauri, and Channel', async () => {
		const mock = installTauriMock(window, {
			windowLabel: 'settings',
			handlers: {
				greet: (args, ctx) => {
					ctx.emit('greeted', { who: args?.name });
					return `hello ${args?.name}`;
				}
			}
		});

		expect(isTauri()).toBe(true);
		expect(
			(testWindow.__TAURI_INTERNALS__ as { metadata: { currentWindow: { label: string } } })
				.metadata.currentWindow.label
		).toBe('settings');

		const received: unknown[] = [];
		const onceReceived: unknown[] = [];
		const unlisten = await listen('greeted', (event) => received.push(event.payload));
		await once('greeted', (event) => onceReceived.push(event.payload));

		expect(await invoke('greet', { name: 'synth' })).toBe('hello synth');
		await emit('greeted', { who: 'second' });
		await emitTo('settings', 'greeted', { who: 'third' });

		expect(received).toEqual([{ who: 'synth' }, { who: 'second' }, { who: 'third' }]);
		expect(onceReceived).toEqual([{ who: 'synth' }]);

		await unlisten();
		mock.emit('greeted', { who: 'after-unlisten' });
		expect(received).toHaveLength(3);

		const channel = new Channel<string>();
		expect(channel.toJSON()).toBe(`__CHANNEL__:${channel.id}`);

		expect(mock.calls.map((c) => c.command)).toContain('greet');
		mock.reset();
		expect(mock.calls).toEqual([]);
	});

	it('respects onUnknown null vs throw and runtime handle() overrides', async () => {
		const lenient = installTauriMock(window);
		expect(await invoke('missing_cmd')).toBeNull();
		lenient.handle('missing_cmd', () => 42);
		expect(await invoke('missing_cmd')).toBe(42);

		installTauriMock(window, { onUnknown: 'throw' });
		await expect(invoke('missing_cmd')).rejects.toThrow(/missing_cmd/);
	});
});

describe('tauriInitScript', () => {
	it('evaluates cleanly in an isolated window context and executes scenario setup', async () => {
		const script = tauriInitScript({
			windowLabel: 'overlay',
			onUnknown: 'throw',
			setup: `mock.handle('ping', (args) => 'pong:' + args.id);`
		});
		const fakeWindow: Record<string, unknown> = {};
		const run = new Function('window', script);
		run(fakeWindow);

		const internals = fakeWindow.__TAURI_INTERNALS__ as {
			metadata: { currentWindow: { label: string } };
			invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
		};
		expect(fakeWindow.isTauri).toBe(true);
		expect(internals.metadata.currentWindow.label).toBe('overlay');
		expect(await internals.invoke('ping', { id: 7 })).toBe('pong:7');
		await expect(internals.invoke('unknown')).rejects.toThrow(/unknown/);
	});
});

describe('tauriModuleMocks', () => {
	it('provides drop-in core and event mocks without touching window', async () => {
		const mock = createTauriMock({
			handlers: { load_state: () => ({ ready: true }) }
		});
		const mods = tauriModuleMocks(mock);

		expect(mods.core.isTauri()).toBe(true);
		expect(await mods.core.invoke('load_state')).toEqual({ ready: true });

		const seen = vi.fn();
		const onceSeen = vi.fn();
		const stop = await mods.event.listen('tick', seen);
		await mods.event.once('tick', onceSeen);

		await mods.event.emit('tick', 1);
		await mods.event.emitTo('main', 'tick', 2);
		await stop();
		mock.emit('tick', 3);

		expect(seen).toHaveBeenCalledTimes(2);
		expect(onceSeen).toHaveBeenCalledTimes(1);

		const received: string[] = [];
		const channel = new mods.core.Channel<string>((msg) => received.push(msg));
		channel.send('delta');
		expect(received).toEqual(['delta']);
		expect(channel.toJSON()).toBe(`__CHANNEL__:${channel.id}`);
	});
});
