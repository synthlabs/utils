import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { commands, errorCodes } from './types';
import type {
	CatalogStateDto,
	DetectionDto,
	ErrorCode,
	InstallationDto,
	InstallPlanDto,
	InventoryDto,
	ModelDto,
	OutcomeDto,
	PairDto,
	RequestDto,
	ServiceSnapshot
} from './types';

export const STATE_EVENT = 'translate://state-changed';
export const MODELS_EVENT = 'translate://models-changed';

const backendCodes = new Set<string>(errorCodes);

function isDetails(value: unknown): value is Record<string, string> {
	return (
		typeof value === 'object' &&
		value !== null &&
		!Array.isArray(value) &&
		Object.values(value).every((entry) => typeof entry === 'string')
	);
}

export class TranslationClientError extends Error {
	constructor(
		public readonly code: ErrorCode | 'ipc' | 'disposed',
		message: string,
		options?: ErrorOptions,
		public readonly details: Readonly<Record<string, string>> = {}
	) {
		super(message, options);
		this.name = 'TranslationClientError';
	}
}

export function normalizeTranslationError(error: unknown): TranslationClientError {
	if (error instanceof TranslationClientError) return error;
	if (
		typeof error === 'object' &&
		error !== null &&
		'code' in error &&
		typeof error.code === 'string' &&
		backendCodes.has(error.code) &&
		'message' in error &&
		typeof error.message === 'string' &&
		(!('details' in error) || isDetails(error.details))
	) {
		return new TranslationClientError(
			error.code as ErrorCode,
			error.message,
			{ cause: error },
			'details' in error ? (error.details as Record<string, string>) : {}
		);
	}
	let message = 'Translation IPC failed';
	if (typeof error === 'string') message = error;
	else if (error instanceof Error) message = error.message;
	return new TranslationClientError('ipc', message, { cause: error });
}

async function rpc<T>(command: keyof typeof commands, args?: Record<string, unknown>): Promise<T> {
	try {
		return await invoke<T>(commands[command], args);
	} catch (error) {
		throw normalizeTranslationError(error);
	}
}

type PendingTranslation = { cancel: () => Promise<void> };

/** Own one client per consuming lifecycle and await dispose() during cleanup. */
export class TranslationClient {
	private disposed = false;
	private pending = new Set<PendingTranslation>();
	private subscriptions = new Set<UnlistenFn>();

	private call<T>(command: keyof typeof commands, args?: Record<string, unknown>): Promise<T> {
		if (this.disposed) return Promise.reject(this.disposedError());
		return rpc<T>(command, args);
	}

	private disposedError() {
		return new TranslationClientError('disposed', 'Translation client has been disposed');
	}

	status(): Promise<ServiceSnapshot> {
		return this.call('status');
	}
	initialize(): Promise<ServiceSnapshot> {
		return this.call('retry_initialize');
	}
	release(): Promise<ServiceSnapshot> {
		return this.call('release');
	}
	resume(): Promise<ServiceSnapshot> {
		return this.call('resume');
	}
	detect(text: string): Promise<DetectionDto | null> {
		return this.call('detect', { text });
	}
	catalog(): Promise<CatalogStateDto> {
		return this.call('catalog');
	}
	refreshCatalog(): Promise<CatalogStateDto> {
		return this.call('refresh_catalog');
	}
	inventory(): Promise<InventoryDto[]> {
		return this.call('inventory');
	}
	verifyModel(pair: PairDto): Promise<ModelDto> {
		return this.call('verify_model', { pair });
	}
	removeModel(pair: PairDto): Promise<boolean> {
		return this.call('remove_model', { pair });
	}
	discardDownloads(pair: PairDto): Promise<boolean> {
		return this.call('discard_downloads', { pair });
	}
	planInstall(pair: PairDto): Promise<InstallPlanDto> {
		return this.call('plan_install', { pair });
	}
	startInstall(plan: InstallPlanDto): Promise<InstallationDto> {
		return this.call('start_install', { plan });
	}
	installation(id: string): Promise<InstallationDto> {
		return this.call('installation', { id });
	}
	cancelInstall(id: string): Promise<void> {
		return this.call('cancel_install', { id });
	}

	async translate(
		request: RequestDto,
		options: { signal?: AbortSignal } = {}
	): Promise<OutcomeDto> {
		if (this.disposed) throw this.disposedError();
		const { signal } = options;
		if (signal?.aborted) throw new TranslationClientError('cancelled', 'Translation cancelled');
		const submitted = rpc<string>('submit_translation', { request });
		const pending: PendingTranslation = {
			cancel: async () => {
				const id = await submitted;
				try {
					await rpc('cancel_translation', { id });
				} catch (error) {
					if (!(error instanceof TranslationClientError && error.code === 'operation_not_found')) {
						throw error;
					}
				}
			}
		};
		this.pending.add(pending);
		const abort = () => {
			// The result request remains the owner of the operation's final error.
			void pending.cancel().catch(() => {});
		};
		signal?.addEventListener('abort', abort, { once: true });
		try {
			const id = await submitted;
			if (signal?.aborted || this.disposed) await pending.cancel();
			return await rpc<OutcomeDto>('translation_result', { id });
		} finally {
			signal?.removeEventListener('abort', abort);
			this.pending.delete(pending);
		}
	}

	/** Subscribe before querying; revision checks prevent an older query overwriting an event. */
	async watchState(onSnapshot: (snapshot: ServiceSnapshot) => void): Promise<UnlistenFn> {
		if (this.disposed) throw this.disposedError();
		let stopped = false;
		let revision = -1n;
		const deliver = (snapshot: ServiceSnapshot) => {
			if (stopped || this.disposed) return;
			if (!/^\d+$/.test(snapshot.revision)) {
				throw new TranslationClientError('ipc', 'Invalid translation snapshot revision');
			}
			const next = BigInt(snapshot.revision);
			if (next <= revision) return;
			revision = next;
			onSnapshot(snapshot);
		};
		let unlisten: UnlistenFn;
		try {
			unlisten = await listen<ServiceSnapshot>(STATE_EVENT, (event) => deliver(event.payload));
		} catch (error) {
			throw normalizeTranslationError(error);
		}
		const stop = () => {
			if (stopped) return;
			stopped = true;
			this.subscriptions.delete(stop);
			unlisten();
		};
		if (this.disposed) {
			stop();
			throw this.disposedError();
		}
		this.subscriptions.add(stop);
		try {
			deliver(await this.status());
		} catch (error) {
			stop();
			throw normalizeTranslationError(error);
		}
		return stop;
	}

	async dispose(): Promise<void> {
		this.disposed = true;
		const results = await Promise.allSettled([
			...[...this.subscriptions].map(async (stop) => stop()),
			...[...this.pending].map((pending) => pending.cancel())
		]);
		const failure = results.find((result) => result.status === 'rejected');
		if (failure?.status === 'rejected') throw normalizeTranslationError(failure.reason);
	}
}

export function createTranslationClient(): TranslationClient {
	return new TranslationClient();
}
