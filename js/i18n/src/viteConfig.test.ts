import { describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
	paraglideVitePlugin: vi.fn((config: unknown) => ({ name: 'paraglide-mock', config }))
}));

vi.mock('@inlang/paraglide-js', () => ({
	paraglideVitePlugin: mocks.paraglideVitePlugin
}));

import { defineParaglide } from './viteConfig';

describe('defineParaglide', () => {
	it('passes Synth Labs desktop defaults to paraglideVitePlugin', () => {
		const plugin = defineParaglide();
		expect(mocks.paraglideVitePlugin).toHaveBeenCalledWith({
			project: './project.inlang',
			outdir: './src/lib/paraglide',
			strategy: ['localStorage', 'preferredLanguage', 'baseLocale']
		});
		expect(plugin).toMatchObject({ name: 'paraglide-mock' });
	});

	it('forwards custom project, outdir, and strategy overrides', () => {
		defineParaglide({
			project: './i18n/project.inlang',
			outdir: './src/generated/paraglide',
			strategy: ['url', 'cookie', 'baseLocale']
		});
		expect(mocks.paraglideVitePlugin).toHaveBeenCalledWith({
			project: './i18n/project.inlang',
			outdir: './src/generated/paraglide',
			strategy: ['url', 'cookie', 'baseLocale']
		});
	});
});
