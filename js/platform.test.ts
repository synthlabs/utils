import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

describe('platform', () => {
	beforeEach(() => {
		vi.resetModules();
	});

	afterEach(() => {
		vi.unstubAllGlobals();
	});

	it('reports web + desktop when neither Tauri internals nor touch points are present', async () => {
		vi.stubGlobal('navigator', { maxTouchPoints: 0 });
		const mod = await import('./platform');
		expect(mod.default).toBe(false);
		expect(mod.isWeb).toBe(true);
		expect(mod.isMobile).toBe(false);
		expect(mod.isWebDesktop).toBe(true);
		expect(mod.isTauriMobile).toBe(false);
	});

	it('reports Tauri + mobile when internals are exposed and touch points are positive', async () => {
		vi.stubGlobal('__TAURI_INTERNALS__', {});
		vi.stubGlobal('navigator', { maxTouchPoints: 5 });
		const mod = await import('./platform');
		expect(mod.default).toBe(true);
		expect(mod.isWeb).toBe(false);
		expect(mod.isMobile).toBe(true);
		expect(mod.isTauriMobile).toBe(true);
		expect(mod.isWebDesktop).toBe(false);
	});
});
