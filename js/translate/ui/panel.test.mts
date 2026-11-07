import { flushSync, mount, tick, unmount, type ComponentProps } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import ModelManagerPanel from './ModelManagerPanel.svelte';
import CustomPanel from './Panel.fixture.svelte';
import { formatModelBytes, modelInstallationProgress } from './copy';
import {
	deferred,
	EN_RU,
	fixture,
	installation,
	local,
	model,
	RU_EN,
	settle
} from '../testing/fixtures';

const components: ReturnType<typeof mount>[] = [];
const fixtures: ReturnType<typeof fixture>[] = [];

async function flush() {
	await settle();
	flushSync();
	await tick();
	await settle();
	flushSync();
}
async function setup(props: Partial<ComponentProps<typeof ModelManagerPanel>> = {}) {
	const f = fixture();
	fixtures.push(f);
	await f.controller.connect();
	await flush();
	const target = document.createElement('div');
	document.body.append(target);
	components.push(
		mount(ModelManagerPanel, { target, props: { controller: f.controller, ...props } })
	);
	await flush();
	return f;
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
	const found = document.querySelector<T>(selector);
	expect(found, selector).not.toBeNull();
	return found!;
}
async function click(selector: string) {
	element(selector).click();
	await flush();
}
afterEach(async () => {
	for (const component of components.splice(0)) await unmount(component);
	for (const f of fixtures.splice(0)) await f.controller.dispose();
	document.body.replaceChildren();
});

describe('model manager panel', () => {
	it('renders available language directions and delegates download consent to the host', async () => {
		const f = await setup();
		expect(element('[data-model-section="available"]').textContent).toContain('Russian → English');
		expect(document.querySelectorAll('[data-model-action="install"]')).toHaveLength(2);
		f.confirm.mockResolvedValue(false);
		await click('[data-model-row="rus:eng"] [data-model-action="install"]');
		expect(f.confirm).toHaveBeenCalledTimes(1);
		expect(f.client.startInstall).not.toHaveBeenCalled();
	});

	it('disables duplicate actions while host confirmation is pending', async () => {
		const f = await setup();
		const approval = deferred<boolean>();
		f.confirm.mockReturnValue(approval.promise);
		await click('[data-model-action="install"]');
		expect(element<HTMLButtonElement>('[data-model-action="install"]').disabled).toBe(true);
		expect(document.body.textContent).toContain('Waiting for confirmation');
		approval.resolve(false);
		await flush();
		expect(element<HTMLButtonElement>('[data-model-action="install"]').disabled).toBe(false);
	});

	it('keeps valid, missing, and problematic data in separate sections with unmanaged entries read-only', async () => {
		const f = await setup();
		f.rows([
			local(),
			local(EN_RU, 'invalid'),
			{
				directory: 'C:/models/mystery',
				pair: null,
				state: 'unmanaged',
				installed: null,
				matchesCurrentCatalog: null,
				detail: 'ownership missing'
			}
		]);
		await f.controller.refreshModels();
		await flush();
		expect(element('[data-model-section="installed"]').textContent).toContain(
			'Present, not verified'
		);
		expect(element('[data-model-section="attention"]').textContent).toContain('Invalid');
		expect(element('[data-model-row="C:/models/mystery"]').querySelector('button')).toBeNull();
		await click('[data-model-row="rus:eng"] [data-model-action="verify"]');
		expect(document.body.textContent).toContain('Verification succeeded at this point in time');
		expect(element('[data-model-section="installed"]').textContent).toContain(
			'Present, not verified'
		);
	});

	it('offers replacement without claiming the catalog model is newer and blocks loaded cleanup', async () => {
		const f = await setup();
		f.loaded();
		f.rows([
			{ ...local(), installed: model(RU_EN, 'older-or-different'), matchesCurrentCatalog: false }
		]);
		await f.controller.refreshModels();
		await flush();
		expect(element('[data-model-action="replace"]').textContent).toContain(
			'Replace with catalog model'
		);
		expect(
			element<HTMLButtonElement>('[data-model-row="rus:eng"] [data-model-action="clear_downloads"]')
				.disabled
		).toBe(true);
		expect(document.body.textContent).not.toContain('newer');
	});

	it('uses minimal progress with closed expandable counters and cancellation pending until terminal status', async () => {
		const f = await setup();
		f.emit({ installation: installation() });
		await flush();
		expect(element<HTMLProgressElement>('progress').value).toBe(25);
		expect(element<HTMLDetailsElement>('[data-model-progress-details]').open).toBe(false);
		expect(element('progress').getAttribute('aria-label')).toBeTruthy();
		expect(element('[data-model-progress-details]').textContent).toContain(
			'Network traffic including retries'
		);
		await click('[data-model-action="cancel"]');
		expect(element<HTMLButtonElement>('[data-model-action="cancel"]').disabled).toBe(true);
		expect(document.body.textContent).toContain('Cancelling…');
		f.finish({ code: 'cancelled', message: 'cancelled' });
		await flush();
		expect(document.querySelector('[data-model-action="cancel"]')).toBeNull();
		expect(document.body.textContent).toContain('reusable downloads were retained');
	});

	it('does not report completion from the progress phase and retains successful cleanup warnings', async () => {
		const f = await setup();
		const operation = installation();
		f.emit({
			installation: { ...operation, progress: { ...operation.progress, phase: 'completed' } }
		});
		await flush();
		expect(element('progress').hasAttribute('value')).toBe(false);
		expect(document.body.textContent).toContain('Waiting for the installation result');
		f.finish(null, ['remove cache manually']);
		await flush();
		expect(element<HTMLProgressElement>('progress').value).toBe(100);
		expect(document.body.textContent).toContain('some temporary files could not be cleaned up');
		expect(document.body.textContent).toContain('remove cache manually');
	});

	it('exposes initialization retry and paused recovery with installation-aware Resume', async () => {
		const f = await setup();
		f.emit({ phase: 'unavailable', error: { code: 'io', message: 'store unavailable' } });
		await flush();
		await click('[data-model-action="retry_initialize"]');
		expect(f.client.initialize).toHaveBeenCalledTimes(1);
		f.emit({ phase: 'paused', workload: null, installation: installation() });
		await flush();
		expect(element<HTMLButtonElement>('[data-model-action="resume"]').disabled).toBe(true);
		f.finish();
		await flush();
		await click('[data-model-action="resume"]');
		expect(f.client.resume).toHaveBeenCalledTimes(1);
	});

	it('displays structured actionable errors with diagnostics and host notification', async () => {
		const f = await setup();
		f.client.startInstall.mockRejectedValue({
			code: 'integrity',
			message: 'Checksum did not match',
			details: { filename: 'model.bin' }
		});
		await click('[data-model-action="install"]');
		expect(element('[role="alert"]').textContent).toContain('Checksum did not match');
		expect(element('[role="alert"] details').textContent).toContain('model.bin');
		expect(f.onerror).toHaveBeenCalledTimes(1);
		await click('[aria-label="Dismiss error"]');
		expect(document.querySelector('[role="alert"]')).toBeNull();
	});

	it('only subscribes to the host controller and leaves app-owned installation running when unmounted', async () => {
		const f = await setup();
		const target = document.createElement('div');
		document.body.append(target);
		components.push(mount(ModelManagerPanel, { target, props: { controller: f.controller } }));
		f.emit({ installation: installation() });
		await flush();
		expect(f.client.watchState).toHaveBeenCalledTimes(1);
		for (const component of components.splice(0)) await unmount(component);
		expect(f.watchers.size).toBe(1);
		expect(f.client.cancelInstall).not.toHaveBeenCalled();
		expect(f.client.release).not.toHaveBeenCalled();
	});

	it('supports typed header, row, action, and progress snippets without Pepo imports', async () => {
		const f = fixture();
		fixtures.push(f);
		await f.controller.connect();
		const target = document.createElement('div');
		document.body.append(target);
		components.push(mount(CustomPanel, { target, props: { controller: f.controller } }));
		await flush();
		expect(element('[data-custom-header]').textContent).toContain('Custom models');
		expect(element('[data-custom-row]').textContent).toContain('rus / eng');
		expect(document.querySelector('.custom-panel')).not.toBeNull();
		f.confirm.mockResolvedValue(false);
		await click('[data-custom-row-action="install"]');
		expect(f.confirm).toHaveBeenCalledTimes(1);
		f.emit({ installation: installation() });
		await flush();
		expect(element('[data-custom-progress]').textContent).toBe('downloading');
		await click('[data-custom-cancel]');
		expect(f.client.cancelInstall).toHaveBeenCalledTimes(1);
	});

	it('allows copy and language/byte formatters to be replaced', async () => {
		await setup({
			copy: {
				title: 'Local engines',
				language: (code: string) => code.toUpperCase(),
				bytes: (value: string) => `${value} octets`
			}
		});
		expect(document.body.textContent).toContain('Local engines');
		expect(document.body.textContent).toContain('RUS → ENG');
		expect(document.body.textContent).toContain('1024 octets');
	});

	it('uses native keyboard-focusable buttons and disclosure controls', async () => {
		await setup();
		const button = element<HTMLButtonElement>('[data-model-action="install"]');
		button.focus();
		expect(document.activeElement).toBe(button);
		expect(button.type).toBe('button');
		const summary = element<HTMLElement>('[data-model-row] summary');
		expect(summary.parentElement?.tagName).toBe('DETAILS');
	});
});

describe('exact byte formatting and progress', () => {
	it('formats u64 counts without first rounding to Number', () => {
		expect(formatModelBytes('18446744073709551615')).toBe('15.9 EiB');
		expect(formatModelBytes('9007199254740993')).toBe('8.0 PiB');
		expect(formatModelBytes('1024')).toBe('1.0 KiB');
		expect(formatModelBytes('0')).toBe('0 B');
	});
	it('handles huge progress totals, zero totals, and retry resets without invented completion', () => {
		const operation = installation();
		operation.progress.fileDownloadBytes = '9007199254740993';
		operation.progress.fileDownloadTotal = '18014398509481986';
		expect(modelInstallationProgress(operation)).toBe(50);
		operation.progress.fileDownloadTotal = '0';
		expect(modelInstallationProgress(operation)).toBeUndefined();
		operation.progress.phase = 'retry_waiting';
		expect(modelInstallationProgress(operation)).toBeUndefined();
		operation.progress.phase = 'decompressing';
		operation.progress.fileInstalledBytes = '1024';
		expect(modelInstallationProgress(operation)).toBe(50);
	});
});
