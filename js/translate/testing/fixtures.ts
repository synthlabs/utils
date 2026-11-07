import { vi } from 'vitest';
import { ModelManagerController } from '../controller';
import type { ModelConfirmationRequest } from '../controller';
import type {
	InstallationDto,
	InventoryDto,
	ModelDto,
	PairDto,
	ServiceError,
	ServiceSnapshot
} from '../types';

export const RU_EN: PairDto = { source: 'rus', target: 'eng' };
export const EN_RU: PairDto = { source: 'eng', target: 'rus' };

export function model(pair: PairDto = RU_EN, id = `${pair.source}-${pair.target}-v1`): ModelDto {
	return {
		id,
		pair: { ...pair },
		version: '1',
		architecture: 'test',
		catalogSha256: 'a'.repeat(64),
		downloadBytes: '1024',
		installedBytes: '2048'
	};
}

export function local(
	pair: PairDto = RU_EN,
	state: InventoryDto['state'] = 'present_unverified'
): InventoryDto {
	return {
		directory: `C:/models/${pair.source}-${pair.target}`,
		pair: { ...pair },
		state,
		installed: state === 'present_unverified' ? model(pair) : null,
		matchesCurrentCatalog: state === 'present_unverified' ? true : null,
		detail: null
	};
}

export function installation(overrides: Partial<InstallationDto> = {}): InstallationDto {
	return {
		id: 'install-1',
		model: model(),
		finished: false,
		result: null,
		error: null,
		progress: {
			phase: 'downloading',
			filename: 'model.bin',
			fileIndex: 1,
			fileCount: 3,
			attempt: 1,
			maxAttempts: 3,
			fileDownloadBytes: '256',
			fileDownloadTotal: '1024',
			completedDownloadBytes: '0',
			downloadTotal: '1024',
			fileInstalledBytes: '0',
			fileInstalledTotal: '2048',
			completedInstalledBytes: '0',
			installedTotal: '2048',
			reusedBytes: '0',
			networkBytes: '256',
			detail: null
		},
		...overrides
	};
}

export function service(): ServiceSnapshot {
	return {
		revision: '1',
		modelsRevision: '0',
		phase: 'ready',
		error: null,
		catalog: {
			refreshing: false,
			freshness: 'fresh',
			error: null,
			catalog: {
				sha256: 'a'.repeat(64),
				models: [model(), model(EN_RU)],
				licenseUrl: 'https://example.invalid/license',
				licenseSha256: 'b'.repeat(64),
				recordsUrl: 'https://example.invalid/records',
				recordsSha256: 'c'.repeat(64),
				retrievedAt: '2026-11-07T00:00:00Z'
			}
		},
		installation: null,
		recovery: { rolledBack: [], cleanedUp: [], warnings: [], installerBusy: false },
		workload: { queuedRequests: 0, runningRequests: 0, cachedResults: 0, residentModels: [] }
	};
}

export function deferred<T>() {
	let resolve!: (value: T) => void;
	let reject!: (error: unknown) => void;
	const promise = new Promise<T>((yes, no) => {
		resolve = yes;
		reject = no;
	});
	return { promise, resolve, reject };
}

export async function settle() {
	for (let pass = 0; pass < 20; pass++) await Promise.resolve();
}

export function fixture() {
	let state = service();
	let entries = [local(RU_EN, 'not_installed'), local(EN_RU, 'not_installed')];
	const watchers = new Set<(snapshot: ServiceSnapshot) => void>();
	const emit = (patch: Partial<ServiceSnapshot>) => {
		state = {
			...state,
			...structuredClone(patch),
			revision: (BigInt(state.revision) + 1n).toString()
		};
		for (const watcher of watchers) watcher(structuredClone(state));
	};
	const client = {
		watchState: vi.fn(async (callback: (snapshot: ServiceSnapshot) => void) => {
			watchers.add(callback);
			callback(structuredClone(state));
			return () => {
				watchers.delete(callback);
			};
		}),
		status: vi.fn(async () => structuredClone(state)),
		inventory: vi.fn(async () => structuredClone(entries)),
		initialize: vi.fn(async () => {
			emit({ phase: 'ready', error: null });
			return structuredClone(state);
		}),
		refreshCatalog: vi.fn(async () => structuredClone(state.catalog)),
		planInstall: vi.fn(async (pair: PairDto) => ({
			pair: { ...pair },
			catalogSha256: 'a'.repeat(64),
			modelId: model(pair).id,
			model: model(pair)
		})),
		startInstall: vi.fn(async (plan: { model: ModelDto }) => {
			const operation = installation({ model: structuredClone(plan.model) });
			emit({ installation: operation });
			return operation;
		}),
		installation: vi.fn(async () => structuredClone(state.installation!)),
		cancelInstall: vi.fn(async () => {}),
		verifyModel: vi.fn(async (pair: PairDto) => model(pair)),
		removeModel: vi.fn(async (pair: PairDto) => {
			entries = entries.map((entry) =>
				entry.pair?.source === pair.source && entry.pair.target === pair.target
					? local(pair, 'not_installed')
					: entry
			);
			emit({ modelsRevision: (BigInt(state.modelsRevision) + 1n).toString() });
			return true;
		}),
		discardDownloads: vi.fn(async () => false),
		release: vi.fn(async () => {
			emit({ phase: 'paused', workload: null });
			return structuredClone(state);
		}),
		resume: vi.fn(async () => {
			emit({ phase: 'ready', workload: service().workload });
			return structuredClone(state);
		})
	};
	const confirm = vi.fn<(request: ModelConfirmationRequest) => Promise<boolean>>(async () => true);
	const onerror = vi.fn();
	const controller = new ModelManagerController({ client, confirm, onerror });
	return {
		controller,
		client,
		confirm,
		onerror,
		emit,
		watchers,
		state: () => structuredClone(state),
		rows: (rows: InventoryDto[]) => {
			entries = structuredClone(rows);
		},
		loaded: () => {
			entries = [local(), local(EN_RU, 'not_installed')];
			emit({
				workload: { ...service().workload!, residentModels: [{ pair: RU_EN, modelId: model().id }] }
			});
		},
		finish: (error: ServiceError | null = null, warnings: string[] = []) => {
			const current = state.installation!;
			if (!error)
				entries = entries.map((entry) =>
					entry.pair?.source === current.model.pair.source
						? { ...local(current.model.pair), installed: current.model }
						: entry
				);
			emit({
				installation: {
					...current,
					finished: true,
					progress: {
						...current.progress,
						phase: error ? (error.code === 'cancelled' ? 'cancelled' : 'failed') : 'completed'
					},
					result: error
						? null
						: { status: 'installed', model: current.model, cleanupWarnings: warnings },
					error
				},
				modelsRevision: (BigInt(state.modelsRevision) + 1n).toString()
			});
		}
	};
}
