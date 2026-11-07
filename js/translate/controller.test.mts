import { afterEach, describe, expect, it, vi } from 'vitest';
import { ModelManagerController } from './controller';
import {
	deferred,
	EN_RU,
	fixture,
	installation,
	local,
	model,
	RU_EN,
	service,
	settle
} from './testing/fixtures';
import type { InventoryDto } from './types';

const controllers: ModelManagerController[] = [];
async function connected() {
	const f = fixture();
	controllers.push(f.controller);
	await f.controller.connect();
	await settle();
	return f;
}
afterEach(async () => {
	await Promise.all(controllers.splice(0).map((controller) => controller.dispose()));
	vi.useRealTimers();
});

describe('model manager consent and inventory', () => {
	it('connects once, stays network-free, and separates valid, missing, and problematic local entries', async () => {
		const f = await connected();
		await f.controller.connect();
		f.rows([
			local(),
			local(EN_RU, 'incomplete'),
			{
				directory: 'C:/models/unknown',
				pair: null,
				state: 'unmanaged',
				installed: null,
				matchesCurrentCatalog: null,
				detail: 'unknown directory'
			}
		]);
		await f.controller.refreshModels();
		const snapshot = f.controller.snapshot();
		expect(snapshot.installed.map((row) => row.key)).toEqual(['rus:eng']);
		expect(snapshot.attention.map((row) => row.local?.state)).toEqual(['incomplete', 'unmanaged']);
		expect(snapshot.available).toEqual([]);
		expect(f.client.watchState).toHaveBeenCalledTimes(1);
		expect(f.client.refreshCatalog).not.toHaveBeenCalled();
		expect(f.client.verifyModel).not.toHaveBeenCalled();
		expect(f.client.startInstall).not.toHaveBeenCalled();
	});

	it.each([false, true])(
		'requires host approval (%s) and sends the reviewed pinned plan',
		async (approved) => {
			const f = await connected();
			f.confirm.mockResolvedValue(approved);
			await f.controller.install(RU_EN);
			expect(f.confirm).toHaveBeenCalledWith(
				expect.objectContaining({
					action: 'install',
					pair: RU_EN,
					interruptsTranslation: false,
					plan: expect.objectContaining({ catalogSha256: 'a'.repeat(64) })
				})
			);
			expect(f.client.startInstall).toHaveBeenCalledTimes(approved ? 1 : 0);
			if (approved)
				expect(f.client.startInstall.mock.calls[0][0]).toEqual(
					await f.client.planInstall.mock.results[0].value
				);
		}
	);

	it('isolates confirmation data from download authority and allows catalog changes without replanning', async () => {
		const f = await connected();
		f.confirm.mockImplementation(async (request) => {
			if ('plan' in request) {
				request.plan.modelId = 'tampered';
				request.plan.model.downloadBytes = '0';
			}
			f.emit({
				catalog: {
					...f.state().catalog,
					catalog: {
						...f.state().catalog.catalog!,
						sha256: 'd'.repeat(64),
						models: [model(RU_EN, 'new-model')]
					}
				},
				modelsRevision: '1'
			});
			return true;
		});
		await f.controller.install(RU_EN);
		expect(f.client.planInstall).toHaveBeenCalledTimes(1);
		expect(f.client.startInstall).toHaveBeenCalledWith(
			expect.objectContaining({
				modelId: model().id,
				catalogSha256: 'a'.repeat(64),
				model: expect.objectContaining({ downloadBytes: '1024' })
			})
		);
	});

	it('blocks duplicate actions and does not execute a late approval after disposal', async () => {
		const f = await connected();
		const approval = deferred<boolean>();
		f.confirm.mockReturnValue(approval.promise);
		const action = f.controller.install(RU_EN);
		await vi.waitFor(() => expect(f.confirm).toHaveBeenCalledTimes(1));
		expect(await f.controller.remove(RU_EN)).toBe(false);
		await f.controller.dispose();
		approval.resolve(true);
		await action;
		expect(f.client.startInstall).not.toHaveBeenCalled();
		expect(f.watchers.size).toBe(0);
	});

	it('does not act on local data changed while confirmation is open', async () => {
		const f = await connected();
		f.confirm.mockImplementation(async () => {
			f.rows([local()]);
			return true;
		});
		await f.controller.install(RU_EN);
		expect(f.client.startInstall).not.toHaveBeenCalled();
		expect(f.controller.snapshot().errors.at(-1)?.error.code).toBe('invalid_plan');
	});

	it('requires new consent when a previously unloaded model becomes resident', async () => {
		const f = await connected();
		f.rows([local()]);
		f.confirm.mockImplementation(async () => {
			f.loaded();
			return true;
		});
		await f.controller.remove(RU_EN);
		expect(f.client.release).not.toHaveBeenCalled();
		expect(f.client.removeModel).not.toHaveBeenCalled();
		expect(f.controller.snapshot().errors.at(-1)?.error.code).toBe('busy');
	});

	it('shows unmanaged data read-only and never sends a destructive request for it', async () => {
		const f = await connected();
		f.rows([local(RU_EN, 'unmanaged')]);
		await f.controller.remove(RU_EN);
		await f.controller.install(RU_EN, 'repair');
		expect(f.confirm).not.toHaveBeenCalled();
		expect(f.client.removeModel).not.toHaveBeenCalled();
		expect(f.client.startInstall).not.toHaveBeenCalled();
	});

	it('ignores obsolete inventory responses, including revisions larger than Number.MAX_SAFE_INTEGER', async () => {
		const f = await connected();
		const old = deferred<InventoryDto[]>();
		f.client.inventory.mockReturnValueOnce(old.promise);
		f.emit({ modelsRevision: '9007199254740992' });
		f.rows([local()]);
		f.emit({ modelsRevision: '9007199254740993' });
		await settle();
		old.resolve([local(RU_EN, 'invalid')]);
		await settle();
		expect(f.controller.snapshot().installed).toHaveLength(1);
	});

	it('keeps installed models visible without a catalog and reports failed explicit refresh with cached fallback', async () => {
		const f = await connected();
		f.rows([local()]);
		const failure = { code: 'catalog_http' as const, message: 'offline' };
		f.emit({ catalog: { catalog: null, error: failure, freshness: null, refreshing: false } });
		await f.controller.refreshModels();
		expect(f.controller.snapshot().installed).toHaveLength(1);
		expect(f.controller.snapshot().available).toHaveLength(0);
		f.emit({ catalog: { ...service().catalog, freshness: 'cached', error: failure } });
		await f.controller.refreshCatalog();
		expect(f.controller.snapshot().service?.catalog.catalog).not.toBeNull();
		expect(f.onerror).toHaveBeenCalledWith(expect.objectContaining(failure), {
			action: 'refresh_catalog'
		});
	});

	it('verifies only on request and preserves the unverified inventory label', async () => {
		const f = await connected();
		f.rows([local()]);
		await f.controller.verify(RU_EN);
		expect(f.client.verifyModel).toHaveBeenCalledWith(RU_EN);
		expect(f.controller.snapshot().notice?.kind).toBe('verified');
		expect(f.controller.snapshot().installed[0].local?.state).toBe('present_unverified');
		expect(f.confirm).not.toHaveBeenCalled();
	});
});

describe('model manager workload lifecycle', () => {
	it('confirms interruption and releases, removes, and resumes in order', async () => {
		const f = await connected();
		f.loaded();
		await f.controller.remove(RU_EN);
		expect(f.confirm).toHaveBeenCalledWith(
			expect.objectContaining({ action: 'remove', interruptsTranslation: true })
		);
		expect(f.client.release.mock.invocationCallOrder[0]).toBeLessThan(
			f.client.removeModel.mock.invocationCallOrder[0]
		);
		expect(f.client.removeModel.mock.invocationCallOrder[0]).toBeLessThan(
			f.client.resume.mock.invocationCallOrder[0]
		);
		expect(f.controller.snapshot().notice?.kind).toBe('removed');
	});

	it('preserves an already-paused workload', async () => {
		const f = await connected();
		f.rows([local()]);
		f.emit({ phase: 'paused', workload: null });
		await f.controller.remove(RU_EN);
		expect(f.client.release).not.toHaveBeenCalled();
		expect(f.client.resume).not.toHaveBeenCalled();
	});

	it('awaits replacement and resumption during disposal without cancelling installation', async () => {
		const f = await connected();
		f.loaded();
		const action = f.controller.install(RU_EN, 'repair');
		await vi.waitFor(() => expect(f.client.startInstall).toHaveBeenCalled());
		let disposed = false;
		const cleanup = f.controller.dispose().then(() => {
			disposed = true;
		});
		await settle();
		expect(disposed).toBe(false);
		expect(f.client.resume).not.toHaveBeenCalled();
		f.finish();
		await action;
		await cleanup;
		expect(disposed).toBe(true);
		expect(f.client.resume).toHaveBeenCalledTimes(1);
		expect(f.client.cancelInstall).not.toHaveBeenCalled();
		expect(f.watchers.size).toBe(0);
	});

	it('preserves mutation and resumption failures together', async () => {
		const f = await connected();
		f.loaded();
		f.client.removeModel.mockRejectedValue({ code: 'io', message: 'remove failed' });
		f.client.resume.mockRejectedValue('resume IPC failed');
		await f.controller.remove(RU_EN);
		expect(f.controller.snapshot().errors.map((issue) => issue.error.message)).toEqual(
			expect.arrayContaining(['remove failed', 'resume IPC failed'])
		);
		expect(f.onerror).toHaveBeenCalledTimes(2);
	});

	it('restores the workload when release fails after pausing', async () => {
		const f = await connected();
		f.loaded();
		f.client.release.mockImplementation(async () => {
			f.emit({ phase: 'paused', workload: null });
			throw new Error('release failed');
		});
		await f.controller.remove(RU_EN);
		expect(f.client.removeModel).not.toHaveBeenCalled();
		expect(f.client.resume).toHaveBeenCalledTimes(1);
	});

	it('leaves loaded-model download cleanup Busy without confirmation or release', async () => {
		const f = await connected();
		f.loaded();
		await f.controller.clearDownloads(RU_EN);
		expect(f.controller.snapshot().errors.at(-1)?.error.code).toBe('busy');
		expect(f.confirm).not.toHaveBeenCalled();
		expect(f.client.release).not.toHaveBeenCalled();
		expect(f.client.discardDownloads).not.toHaveBeenCalled();
	});

	it('confirms cleanup and reports no files without inventing a retained size', async () => {
		const f = await connected();
		await f.controller.clearDownloads(RU_EN);
		expect(f.confirm).toHaveBeenCalledWith(expect.objectContaining({ action: 'clear_downloads' }));
		expect(f.controller.snapshot().notice?.kind).toBe('no_downloads');
	});

	it('continues restoration even if host error reporting throws', async () => {
		const f = await connected();
		f.loaded();
		f.onerror.mockImplementation(() => {
			throw new Error('toast failed');
		});
		const action = f.controller.install(RU_EN, 'repair');
		await vi.waitFor(() => expect(f.client.startInstall).toHaveBeenCalled());
		f.finish({ code: 'integrity', message: 'bad checksum' });
		await action;
		expect(f.client.resume).toHaveBeenCalledTimes(1);
		expect(
			f.controller.snapshot().errors.filter((issue) => issue.error.code === 'integrity')
		).toHaveLength(1);
	});
});

describe('model manager installation observation', () => {
	it('ignores superseded background inventory failures and all late failures after disposal', async () => {
		const f = await connected();
		const old = deferred<InventoryDto[]>();
		f.client.inventory.mockReturnValueOnce(old.promise);
		f.emit({ modelsRevision: '1' });
		f.rows([local()]);
		f.emit({ modelsRevision: '2' });
		await settle();
		old.reject(new Error('obsolete inventory failure'));
		await settle();
		expect(f.onerror).not.toHaveBeenCalled();
		const late = deferred<InventoryDto[]>();
		f.client.inventory.mockReturnValueOnce(late.promise);
		f.emit({ modelsRevision: '3' });
		await f.controller.dispose();
		late.reject(new Error('late inventory failure'));
		await settle();
		expect(f.onerror).not.toHaveBeenCalled();
	});

	it('treats rejected and throwing confirmations as non-authorizing', async () => {
		const f = await connected();
		f.confirm.mockRejectedValue('confirmation failed');
		expect(await f.controller.install(RU_EN)).toBe(false);
		expect(f.client.startInstall).not.toHaveBeenCalled();
		f.confirm.mockResolvedValue(false);
		expect(await f.controller.clearDownloads(RU_EN)).toBe(false);
		expect(f.client.discardDownloads).not.toHaveBeenCalled();
	});

	it('keeps both the model and workload untouched on backend Busy after approval', async () => {
		const f = await connected();
		f.client.startInstall.mockRejectedValue({
			code: 'busy',
			message: 'installer owned by another client'
		});
		await f.controller.install(RU_EN);
		expect(f.controller.snapshot().errors.at(-1)?.error.code).toBe('busy');
		expect(f.client.release).not.toHaveBeenCalled();
		expect(f.client.startInstall).toHaveBeenCalledTimes(1);
	});

	it('awaits a cancellation request before disposing and permits retry after cancellation IPC fails', async () => {
		const f = await connected();
		f.emit({ installation: installation() });
		f.client.cancelInstall.mockRejectedValueOnce('cancel failed');
		await f.controller.cancelInstallation();
		expect(f.controller.snapshot().cancelling).toBe(false);
		const cancel = deferred<void>();
		f.client.cancelInstall.mockReturnValueOnce(cancel.promise);
		const request = f.controller.cancelInstallation();
		let disposed = false;
		const cleanup = f.controller.dispose().then(() => {
			disposed = true;
		});
		await settle();
		expect(disposed).toBe(false);
		cancel.resolve();
		await request;
		await cleanup;
		expect(disposed).toBe(true);
	});

	it('observes terminal status by polling when the event stream misses completion', async () => {
		vi.useFakeTimers();
		const f = await connected();
		f.loaded();
		const action = f.controller.install(RU_EN, 'replace');
		await settle();
		expect(f.client.startInstall).toHaveBeenCalledTimes(1);
		const terminal = installation({
			finished: true,
			result: { status: 'installed', model: model(), cleanupWarnings: [] }
		});
		f.client.installation.mockResolvedValue(terminal);
		await vi.advanceTimersByTimeAsync(1000);
		await action;
		expect(f.client.resume).toHaveBeenCalledTimes(1);
		expect(vi.getTimerCount()).toBe(0);
	});

	it('leaves an ordinary app-owned installation running when the controller is disposed', async () => {
		const f = await connected();
		await f.controller.install(RU_EN);
		await f.controller.dispose();
		expect(f.client.cancelInstall).not.toHaveBeenCalled();
		expect(f.client.resume).not.toHaveBeenCalled();
		expect(f.state().installation?.finished).toBe(false);
	});

	it('reports connection failure once and supports explicit reconnection', async () => {
		const f = fixture();
		controllers.push(f.controller);
		f.client.watchState.mockRejectedValueOnce(new Error('event subscription failed'));
		expect(await f.controller.install(RU_EN)).toBe(false);
		expect(f.onerror).toHaveBeenCalledTimes(1);
		await f.controller.connect();
		expect(f.controller.snapshot().connected).toBe(true);
	});

	it('reconnects to app-owned work, requests cancellation once, and retains files', async () => {
		const f = await connected();
		f.emit({ installation: installation() });
		await f.controller.cancelInstallation();
		await f.controller.cancelInstallation();
		expect(f.controller.snapshot().cancelling).toBe(true);
		expect(f.client.cancelInstall).toHaveBeenCalledTimes(1);
		f.finish({ code: 'cancelled', message: 'cancelled' });
		expect(f.controller.snapshot().cancelling).toBe(false);
		expect(f.controller.snapshot().notice?.kind).toBe('cancelled');
		expect(f.client.discardDownloads).not.toHaveBeenCalled();
		expect(f.onerror).not.toHaveBeenCalled();
	});

	it('does not claim cancellation when publication wins the race', async () => {
		const f = await connected();
		f.emit({ installation: installation() });
		await f.controller.cancelInstallation();
		f.finish(null, ['temporary files remain']);
		expect(f.controller.snapshot().notice?.kind).toBe('installed');
		expect(f.controller.snapshot().installation?.result?.cleanupWarnings).toEqual([
			'temporary files remain'
		]);
	});

	it('reports terminal failure once even across later service snapshots', async () => {
		const f = await connected();
		f.emit({ installation: installation() });
		f.finish({ code: 'http', message: 'download failed' });
		f.emit({ modelsRevision: '2' });
		expect(f.onerror).toHaveBeenCalledTimes(1);
		expect(f.client.startInstall).not.toHaveBeenCalled();
	});

	it('blocks resumption until installation ends and then exposes explicit recovery', async () => {
		const f = await connected();
		f.emit({ phase: 'paused', workload: null, installation: installation() });
		expect(await f.controller.resume()).toBe(false);
		f.finish();
		await f.controller.resume();
		expect(f.client.resume).toHaveBeenCalledTimes(1);
	});

	it('cleans subscriptions set up asynchronously during disposal', async () => {
		const f = fixture();
		const setup = deferred<void>();
		f.client.watchState.mockImplementation(async (callback) => {
			await setup.promise;
			f.watchers.add(callback);
			callback(f.state());
			return () => {
				f.watchers.delete(callback);
			};
		});
		const connection = f.controller.connect();
		const disposal = f.controller.dispose();
		setup.resolve();
		await connection;
		await disposal;
		expect(f.watchers.size).toBe(0);
	});
});
