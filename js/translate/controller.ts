import { normalizeTranslationError, TranslationClientError } from './client';
import type { TranslationClient } from './client';
import type {
	InstallationDto,
	InstallPlanDto,
	InventoryDto,
	ModelDto,
	PairDto,
	ServiceSnapshot
} from './types';

export type ModelManagerClient = Pick<
	TranslationClient,
	| 'watchState'
	| 'status'
	| 'inventory'
	| 'initialize'
	| 'refreshCatalog'
	| 'planInstall'
	| 'startInstall'
	| 'installation'
	| 'cancelInstall'
	| 'verifyModel'
	| 'removeModel'
	| 'discardDownloads'
	| 'release'
	| 'resume'
>;

export type ModelManagerAction =
	| 'install'
	| 'repair'
	| 'replace'
	| 'remove'
	| 'clear_downloads'
	| 'verify'
	| 'refresh_catalog'
	| 'retry_initialize'
	| 'resume'
	| 'cancel'
	| 'connect'
	| 'inventory';

export type ModelActionContext = { action: ModelManagerAction; pair?: PairDto };
export type ModelConfirmationRequest = {
	pair: PairDto;
	installed: ModelDto | null;
	interruptsTranslation: boolean;
} & (
	| { action: 'install' | 'repair' | 'replace'; plan: InstallPlanDto }
	| { action: 'remove' | 'clear_downloads' }
);
export type ModelManagerIssue = ModelActionContext & {
	id: number;
	error: TranslationClientError;
};
export type ModelManagerNotice = {
	kind: 'verified' | 'removed' | 'absent' | 'cleared' | 'no_downloads' | 'installed' | 'cancelled';
	pair: PairDto;
};
export type ModelManagerRow = {
	key: string;
	pair: PairDto | null;
	model: ModelDto | null;
	catalogModel: ModelDto | null;
	local: InventoryDto | null;
	inUse: boolean;
};
export type ModelManagerSnapshot = {
	connected: boolean;
	disposing: boolean;
	loading: boolean;
	service: ServiceSnapshot | null;
	installed: ModelManagerRow[];
	available: ModelManagerRow[];
	attention: ModelManagerRow[];
	installation: InstallationDto | null;
	pending:
		| (ModelActionContext & {
				stage: 'checking' | 'confirming' | 'releasing' | 'working' | 'resuming';
		  })
		| null;
	cancelling: boolean;
	errors: ModelManagerIssue[];
	notice: ModelManagerNotice | null;
};
export type ModelManagerOptions = {
	client: ModelManagerClient;
	confirm: (request: ModelConfirmationRequest) => Promise<boolean>;
	onerror?: (error: TranslationClientError, context: ModelActionContext) => void;
};

export function modelPairKey(pair: PairDto): string {
	return `${pair.source}:${pair.target}`;
}

function samePair(left: PairDto, right: PairDto): boolean {
	return modelPairKey(left) === modelPairKey(right);
}

function localIdentity(entry: InventoryDto | undefined): string {
	return JSON.stringify([
		entry?.state ?? 'not_installed',
		entry?.directory,
		entry?.installed?.id,
		entry?.installed?.catalogSha256
	]);
}

function usableService(service: ServiceSnapshot | null): boolean {
	return service !== null && ['ready', 'paused', 'releasing'].includes(service.phase);
}

/** The host owns the client and must keep it alive until dispose() resolves. */
export class ModelManagerController {
	private service: ServiceSnapshot | null = null;
	private inventory: InventoryDto[] = [];
	private operation: InstallationDto | null = null;
	private listeners = new Set<(snapshot: ModelManagerSnapshot) => void>();
	private waiters = new Set<() => void>();
	private unlisten?: () => void;
	private connection?: Promise<void>;
	private disposal?: Promise<void>;
	private action?: Promise<boolean>;
	private cancellation?: Promise<void>;
	private connected = false;
	private disposing = false;
	private disposed = false;
	private loading = false;
	private inventoryRequest = 0;
	private pending: ModelManagerSnapshot['pending'] = null;
	private abandonConfirmation?: () => void;
	private cancellingId: string | null = null;
	private terminalId: string | null = null;
	private errors: ModelManagerIssue[] = [];
	private notice: ModelManagerNotice | null = null;
	private sequence = 0;

	constructor(private readonly options: ModelManagerOptions) {}

	snapshot(): ModelManagerSnapshot {
		const service = this.service ? structuredClone(this.service) : null;
		const catalog = service?.catalog.catalog?.models ?? [];
		const locals = structuredClone(this.inventory);
		const installed: ModelManagerRow[] = [];
		const attention: ModelManagerRow[] = [];
		const available: ModelManagerRow[] = [];
		const row = (local: InventoryDto | null, model: ModelDto | null): ModelManagerRow => {
			const pair = local?.pair ?? model?.pair ?? null;
			return {
				key: pair ? modelPairKey(pair) : local!.directory,
				pair,
				model: local?.installed ?? model,
				catalogModel: pair ? (catalog.find((item) => samePair(item.pair, pair)) ?? null) : null,
				local,
				inUse: pair ? this.inUse(pair, service) : false
			};
		};
		for (const local of locals) {
			if (local.state === 'present_unverified') installed.push(row(local, null));
			else if (local.state !== 'not_installed') attention.push(row(local, null));
		}
		for (const model of catalog) {
			if (
				!locals.some(
					(local) =>
						local.pair && samePair(local.pair, model.pair) && local.state !== 'not_installed'
				)
			) {
				available.push(row(null, model));
			}
		}
		return {
			connected: this.connected,
			disposing: this.disposing,
			loading: this.loading,
			service,
			installed,
			available,
			attention,
			installation: this.operation ? structuredClone(this.operation) : null,
			pending: this.pending ? structuredClone(this.pending) : null,
			cancelling: this.cancellingId !== null,
			errors: this.errors.map((issue) => ({
				...issue,
				pair: issue.pair ? { ...issue.pair } : undefined
			})),
			notice: this.notice ? structuredClone(this.notice) : null
		};
	}

	subscribe(listener: (snapshot: ModelManagerSnapshot) => void): () => void {
		this.listeners.add(listener);
		listener(this.snapshot());
		return () => {
			this.listeners.delete(listener);
		};
	}

	private publish(): void {
		if (this.disposed) return;
		for (const listener of this.listeners) listener(this.snapshot());
	}

	private report(error: unknown, context: ModelActionContext): void {
		if (this.disposed) return;
		const normalized = normalizeTranslationError(error);
		this.errors.push({ ...context, id: ++this.sequence, error: normalized });
		this.publish();
		try {
			this.options.onerror?.(normalized, context);
		} catch {
			// A host notification failure must not interrupt workload restoration.
		}
	}

	dismissError(id: number): void {
		this.errors = this.errors.filter((issue) => issue.id !== id);
		this.publish();
	}

	connect(): Promise<void> {
		if (this.disposing)
			return Promise.reject(
				new TranslationClientError('disposed', 'Model manager has been disposed')
			);
		if (this.connection) return this.connection;
		this.connected = true;
		this.publish();
		this.connection = this.options.client
			.watchState((snapshot) => this.accept(snapshot))
			.then((stop) => {
				this.unlisten = stop;
			})
			.catch((error: unknown) => {
				this.connected = false;
				this.connection = undefined;
				this.report(error, { action: 'connect' });
				throw error;
			});
		return this.connection;
	}

	private accept(snapshot: ServiceSnapshot): void {
		if (
			this.disposed ||
			(this.service && BigInt(snapshot.revision) <= BigInt(this.service.revision))
		)
			return;
		const previous = this.service;
		this.service = structuredClone(snapshot);
		this.acceptInstallation(snapshot.installation);
		for (const waiter of this.waiters) waiter();
		this.publish();
		if (
			usableService(snapshot) &&
			(!usableService(previous) || previous?.modelsRevision !== snapshot.modelsRevision)
		) {
			const request = this.inventoryRequest + 1;
			void this.refreshInventory().catch((error: unknown) => {
				if (request === this.inventoryRequest) this.report(error, { action: 'inventory' });
			});
		}
	}

	private acceptInstallation(operation: InstallationDto | null): void {
		this.operation = operation ? structuredClone(operation) : null;
		if (this.cancellingId && this.cancellingId !== operation?.id) this.cancellingId = null;
		if (!operation?.finished) return;
		if (this.cancellingId === operation.id) this.cancellingId = null;
		if (this.terminalId === operation.id) return;
		this.terminalId = operation.id;
		if (operation.error?.code === 'cancelled')
			this.notice = { kind: 'cancelled', pair: operation.model.pair };
		else if (operation.error)
			this.report(operation.error, { action: 'install', pair: operation.model.pair });
		else if (operation.result) this.notice = { kind: 'installed', pair: operation.model.pair };
	}

	private async refreshInventory(): Promise<InventoryDto[]> {
		const request = ++this.inventoryRequest;
		const revision = this.service?.modelsRevision;
		this.loading = true;
		this.publish();
		try {
			const entries = await this.options.client.inventory();
			if (
				!this.disposed &&
				request === this.inventoryRequest &&
				revision === this.service?.modelsRevision
			) {
				this.inventory = structuredClone(entries);
			}
			return entries;
		} finally {
			if (request === this.inventoryRequest) {
				this.loading = false;
				this.publish();
			}
		}
	}

	private inUse(pair: PairDto, service: ServiceSnapshot | null): boolean {
		return service?.workload?.residentModels.some((model) => samePair(model.pair, pair)) ?? false;
	}

	private stage(stage: NonNullable<ModelManagerSnapshot['pending']>['stage']): void {
		if (this.pending) this.pending = { ...this.pending, stage };
		this.publish();
	}

	private run(
		context: ModelActionContext,
		work: () => Promise<void | boolean>,
		duringInstallation = false
	): Promise<boolean> {
		if (
			this.disposing ||
			this.pending ||
			(!duringInstallation && this.operation && !this.operation.finished)
		)
			return Promise.resolve(false);
		this.pending = { ...context, stage: 'checking' };
		this.notice = null;
		this.publish();
		this.action = (async () => {
			try {
				try {
					await this.connect();
				} catch {
					return false;
				}
				if (this.disposing) return false;
				return (await work()) !== false;
			} catch (error) {
				this.report(error, context);
				return false;
			} finally {
				this.pending = null;
				this.publish();
			}
		})();
		return this.action;
	}

	private async confirm(request: ModelConfirmationRequest): Promise<boolean> {
		this.stage('confirming');
		try {
			const approved = await new Promise<boolean>((resolve, reject) => {
				this.abandonConfirmation = () => resolve(false);
				Promise.resolve()
					.then(() => this.options.confirm(structuredClone(request)))
					.then(resolve, reject);
			});
			return approved === true && !this.disposing;
		} finally {
			this.abandonConfirmation = undefined;
		}
	}

	install(pair: PairDto, action: 'install' | 'repair' | 'replace' = 'install'): Promise<boolean> {
		const selected = { ...pair };
		return this.run({ action, pair: selected }, () => this.mutate(action, selected));
	}

	remove(pair: PairDto): Promise<boolean> {
		const selected = { ...pair };
		return this.run({ action: 'remove', pair: selected }, () => this.mutate('remove', selected));
	}

	clearDownloads(pair: PairDto): Promise<boolean> {
		const selected = { ...pair };
		return this.run({ action: 'clear_downloads', pair: selected }, () =>
			this.mutate('clear_downloads', selected)
		);
	}

	private async inspect(
		pair: PairDto
	): Promise<{ service: ServiceSnapshot; entry?: InventoryDto }> {
		const service = await this.options.client.status();
		this.accept(service);
		if (service.installation && !service.installation.finished)
			throw new TranslationClientError('busy', 'An installation is already active');
		if (!['ready', 'paused'].includes(service.phase))
			throw new TranslationClientError('not_ready', 'Model management is not ready');
		const entries = await this.refreshInventory();
		const entry = entries.find((item) => item.pair && samePair(item.pair, pair));
		if (entry?.state === 'unmanaged')
			throw new TranslationClientError('unmanaged', 'Unmanaged model data is read-only');
		if (entry?.state === 'busy')
			throw new TranslationClientError('busy', 'The model is being changed by another operation');
		return { service, entry };
	}

	private async mutate(
		action: 'install' | 'repair' | 'replace' | 'remove' | 'clear_downloads',
		pair: PairDto
	): Promise<boolean> {
		const before = await this.inspect(pair);
		if (this.disposing) return false;
		const interruptsTranslation =
			before.service.phase === 'ready' && this.inUse(pair, before.service);
		if (action === 'clear_downloads' && interruptsTranslation)
			throw new TranslationClientError(
				'busy',
				'Clear downloads is unavailable while this model is in use'
			);
		const base = { pair, installed: before.entry?.installed ?? null, interruptsTranslation };
		const request: ModelConfirmationRequest =
			action === 'remove' || action === 'clear_downloads'
				? { ...base, action }
				: { ...base, action, plan: await this.options.client.planInstall(pair) };
		if (this.disposing || !(await this.confirm(request))) return false;
		const current = await this.inspect(pair);
		if (this.disposing) return false;
		if (localIdentity(before.entry) !== localIdentity(current.entry))
			throw new TranslationClientError(
				'invalid_plan',
				'Local model data changed while confirmation was open. Review the action again.'
			);
		const loaded = current.service.phase === 'ready' && this.inUse(pair, current.service);
		if (loaded && (!interruptsTranslation || action === 'clear_downloads'))
			throw new TranslationClientError('busy', 'The model became in use. Review the action again.');
		let restore = false;
		try {
			if (loaded) {
				this.stage('releasing');
				restore = true;
				this.accept(await this.options.client.release());
			}
			this.stage('working');
			if ('plan' in request) {
				const initial = await this.options.client.startInstall(request.plan);
				if (this.operation?.id !== initial.id || !this.operation.finished)
					this.acceptInstallation(initial);
				this.publish();
				if (restore) this.acceptInstallation(await this.awaitInstallation(initial));
			} else if (action === 'remove') {
				const removed = await this.options.client.removeModel(pair);
				this.notice = { kind: removed ? 'removed' : 'absent', pair };
			} else {
				const removed = await this.options.client.discardDownloads(pair);
				this.notice = { kind: removed ? 'cleared' : 'no_downloads', pair };
			}
		} finally {
			if (restore) {
				this.stage('resuming');
				try {
					const latest = await this.options.client.status();
					this.accept(latest);
					if (latest.phase === 'paused') this.accept(await this.options.client.resume());
				} catch (error) {
					this.report(error, { action: 'resume', pair });
				}
			}
			try {
				await this.refreshInventory();
			} catch (error) {
				this.report(error, { action: 'inventory', pair });
			}
		}
		return true;
	}

	private awaitInstallation(initial: InstallationDto): Promise<InstallationDto> {
		if (initial.finished) return Promise.resolve(initial);
		return new Promise((resolve, reject) => {
			let timer: ReturnType<typeof setTimeout> | undefined;
			let done = false;
			const finish = (operation?: InstallationDto, error?: unknown) => {
				if (done) return;
				done = true;
				clearTimeout(timer);
				this.waiters.delete(check);
				if (error) reject(error);
				else resolve(operation!);
			};
			const check = () => {
				if (this.service?.phase === 'shutdown')
					finish(
						undefined,
						new TranslationClientError('shutdown', 'Translation service shut down')
					);
				else if (this.operation?.id === initial.id && this.operation.finished)
					finish(this.operation);
			};
			const poll = async () => {
				try {
					const operation = await this.options.client.installation(initial.id);
					if (done) return;
					if (operation.finished) finish(operation);
					else
						timer = setTimeout(() => {
							void poll();
						}, 1000);
				} catch (error) {
					finish(undefined, error);
				}
			};
			this.waiters.add(check);
			check();
			if (!done)
				timer = setTimeout(() => {
					void poll();
				}, 1000);
		});
	}

	cancelInstallation(): Promise<void> {
		const operation = this.operation;
		if (this.disposing || !operation || operation.finished || this.cancellingId)
			return Promise.resolve();
		this.cancellingId = operation.id;
		this.publish();
		this.cancellation = this.options.client.cancelInstall(operation.id).catch((error: unknown) => {
			this.cancellingId = null;
			this.report(error, { action: 'cancel', pair: operation.model.pair });
		});
		return this.cancellation;
	}

	verify(pair: PairDto): Promise<boolean> {
		const selected = { ...pair };
		return this.run({ action: 'verify', pair: selected }, async () => {
			await this.options.client.verifyModel(selected);
			this.notice = { kind: 'verified', pair: selected };
			await this.refreshInventory();
		});
	}

	refreshCatalog(): Promise<boolean> {
		return this.run(
			{ action: 'refresh_catalog' },
			async () => {
				const result = await this.options.client.refreshCatalog();
				this.accept(await this.options.client.status());
				if (result.error) this.report(result.error, { action: 'refresh_catalog' });
				await this.refreshInventory();
			},
			true
		);
	}

	retryInitialize(): Promise<boolean> {
		return this.run({ action: 'retry_initialize' }, async () => {
			this.accept(await this.options.client.initialize());
			await this.refreshInventory();
		});
	}

	resume(): Promise<boolean> {
		return this.run({ action: 'resume' }, async () => {
			const current = await this.options.client.status();
			if (current.installation && !current.installation.finished)
				throw new TranslationClientError('busy', 'Wait for installation to finish before resuming');
			this.accept(await this.options.client.resume());
		});
	}

	refreshModels(): Promise<boolean> {
		return this.run(
			{ action: 'inventory' },
			async () => {
				this.accept(await this.options.client.status());
				await this.refreshInventory();
			},
			true
		);
	}

	dispose(): Promise<void> {
		if (this.disposal) return this.disposal;
		this.disposing = true;
		this.abandonConfirmation?.();
		this.publish();
		this.disposal = (async () => {
			await this.action;
			await this.cancellation;
			try {
				await this.connection;
			} catch {
				/* Connection errors are already reported. */
			}
			try {
				this.unlisten?.();
			} catch (error) {
				this.report(error, { action: 'connect' });
			}
			this.inventoryRequest++;
			this.connected = false;
			this.loading = false;
			this.disposed = true;
			this.listeners.clear();
		})();
		return this.disposal;
	}
}
