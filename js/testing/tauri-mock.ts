export interface TauriMockContext {
	emit(event: string, payload?: unknown): void;
}

export type TauriMockHandler = (
	args: Record<string, unknown> | undefined,
	ctx: TauriMockContext
) => unknown;

export interface TauriMockOptions {
	handlers?: Record<string, TauriMockHandler>;
	onUnknown?: 'null' | 'throw';
	windowLabel?: string;
}

export interface TauriMockCall {
	command: string;
	args: unknown;
}

export interface TauriMock {
	calls: TauriMockCall[];
	handle(command: string, handler: TauriMockHandler): void;
	invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
	listen<T = unknown>(
		event: string,
		callback: (event: { event: string; id: number; payload: T }) => void
	): Promise<() => Promise<void>>;
	emit(event: string, payload?: unknown): void;
	reset(): void;
}

export interface TauriInitScriptOptions {
	onUnknown?: 'null' | 'throw';
	windowLabel?: string;
	setup?: string;
}

type TauriMockTarget = Record<string, unknown>;

/*
 * Keep installTauriMock completely self-contained and closure-free so
 * installTauriMock.toString() can be serialized into Playwright addInitScript().
 */
export function installTauriMock(target: unknown, options: TauriMockOptions = {}): TauriMock {
	const host = target as TauriMockTarget;
	const opts = options || {};
	const windowLabel = opts.windowLabel || 'main';
	const onUnknown = opts.onUnknown || 'null';
	const handlers = new Map<string, TauriMockHandler>(Object.entries(opts.handlers || {}));
	const callbacks = new Map<number, { fn: (payload: unknown) => void; once: boolean }>();
	const listeners = new Map<number, { event: string; handler: number }>();
	const calls: TauriMockCall[] = [];
	let nextId = 0;

	function transformCallback(callback?: (payload: unknown) => void, once = false): number {
		const id = ++nextId;
		if (typeof callback === 'function') {
			callbacks.set(id, { fn: callback, once: Boolean(once) });
		}
		return id;
	}

	function unregisterCallback(id: number): void {
		callbacks.delete(id);
	}

	function removeListener(eventId: number): void {
		const entry = listeners.get(eventId);
		if (entry) {
			listeners.delete(eventId);
			callbacks.delete(entry.handler);
		}
	}

	function emit(event: string, payload?: unknown): void {
		const entries = Array.from(listeners.entries());
		for (let i = 0; i < entries.length; i++) {
			const [eventId, entry] = entries[i];
			if (entry.event !== event) continue;
			const cb = callbacks.get(entry.handler);
			if (!cb) continue;
			if (cb.once) callbacks.delete(entry.handler);
			cb.fn({ event, id: eventId, payload });
		}
	}

	const ctx: TauriMockContext = { emit };

	async function invoke(command: string, args?: Record<string, unknown>): Promise<unknown> {
		calls.push({ command, args });
		const custom = handlers.get(command);
		if (custom) return await custom(args, ctx);

		if (command === 'plugin:event|listen') {
			const params = args || {};
			const handlerId = typeof params.handler === 'number' ? params.handler : ++nextId;
			const eventId = handlerId;
			listeners.set(eventId, { event: String(params.event), handler: handlerId });
			return eventId;
		}
		if (command === 'plugin:event|unlisten') {
			const params = args || {};
			removeListener(Number(params.eventId));
			return null;
		}
		if (command === 'plugin:event|emit' || command === 'plugin:event|emit_to') {
			const params = args || {};
			emit(String(params.event), params.payload);
			return null;
		}
		if (onUnknown === 'throw') {
			throw new Error(`Unexpected Tauri command: ${command}`);
		}
		return null;
	}

	async function listen<T = unknown>(
		event: string,
		callback: (event: { event: string; id: number; payload: T }) => void
	): Promise<() => Promise<void>> {
		const handler = transformCallback(callback as (payload: unknown) => void);
		const eventId = (await invoke('plugin:event|listen', {
			event,
			target: { kind: 'Any' },
			handler
		})) as number;
		return async () => {
			removeListener(eventId);
			await invoke('plugin:event|unlisten', { event, eventId });
		};
	}

	host.isTauri = true;
	host.__TAURI_INTERNALS__ = {
		metadata: {
			currentWindow: { label: windowLabel },
			currentWebview: { label: windowLabel }
		},
		transformCallback,
		unregisterCallback,
		convertFileSrc(filePath: string, protocol = 'asset'): string {
			return `${protocol}://localhost/${encodeURIComponent(filePath)}`;
		},
		invoke
	};
	host.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
		unregisterListener(_event: string, eventId: number): void {
			removeListener(eventId);
		}
	};

	const mock: TauriMock = {
		calls,
		handle(command, handler) {
			handlers.set(command, handler);
		},
		invoke,
		listen,
		emit,
		reset() {
			calls.length = 0;
			listeners.clear();
			callbacks.clear();
		}
	};

	host.__tauriMock = mock;
	return mock;
}

export function createTauriMock(options: TauriMockOptions = {}): TauriMock {
	return installTauriMock({}, options);
}

export function tauriInitScript(scenario: string | TauriInitScriptOptions = {}): string {
	const opts: TauriInitScriptOptions =
		typeof scenario === 'string' ? { setup: scenario } : scenario;
	const initOptions = JSON.stringify({
		...(opts.windowLabel !== undefined ? { windowLabel: opts.windowLabel } : {}),
		...(opts.onUnknown !== undefined ? { onUnknown: opts.onUnknown } : {})
	});
	const setup = opts.setup ? `\n${opts.setup}\n` : '';
	return `(() => {\nconst installTauriMock = ${installTauriMock.toString()};\nconst mock = installTauriMock(window, ${initOptions});${setup}})();`;
}

export class MockChannel<T = unknown> {
	static #nextId = 0;
	readonly id: number;
	onmessage: (message: T) => void;

	constructor(onmessage?: (message: T) => void) {
		this.id = ++MockChannel.#nextId;
		this.onmessage = onmessage ?? (() => {});
	}

	send(message: T): void {
		this.onmessage(message);
	}

	__TAURI_TO_IPC_KEY__(): string {
		return `__CHANNEL__:${this.id}`;
	}

	toJSON(): string {
		return this.__TAURI_TO_IPC_KEY__();
	}
}

export function tauriModuleMocks(mock: TauriMock) {
	return {
		core: {
			invoke: <T = unknown>(command: string, args?: Record<string, unknown>) =>
				mock.invoke(command, args) as Promise<T>,
			Channel: MockChannel,
			isTauri: () => true
		},
		event: {
			listen: <T = unknown>(
				event: string,
				callback: (event: { event: string; id: number; payload: T }) => void
			) => mock.listen<T>(event, callback),
			once: async <T = unknown>(
				event: string,
				callback: (event: { event: string; id: number; payload: T }) => void
			) => {
				const stop = await mock.listen<T>(event, (eventData) => {
					void stop();
					callback(eventData);
				});
				return stop;
			},
			emit: async (event: string, payload?: unknown) => {
				await mock.invoke('plugin:event|emit', { event, payload });
			},
			emitTo: async (target: unknown, event: string, payload?: unknown) => {
				await mock.invoke('plugin:event|emit_to', { target, event, payload });
			}
		}
	};
}
