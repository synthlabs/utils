import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { LanguageController, type ParaglideRuntime } from './languageController.svelte';
import LanguageSwitcher from './LanguageSwitcher.svelte';

const mounted: ReturnType<typeof mount>[] = [];

afterEach(async () => {
	for (const component of mounted.splice(0)) {
		await unmount(component);
	}
	document.body.replaceChildren();
});

function createRuntime(initial = 'en') {
	let current = initial;
	const setLocale = vi.fn((next: string) => {
		current = next;
	});
	const runtime: ParaglideRuntime<string> = {
		getLocale: () => current,
		setLocale,
		locales: ['en', 'ru'] as const
	};
	return { runtime, setLocale };
}

describe('LanguageController', () => {
	it('initializes from runtime and exposes configured locales', () => {
		const { runtime } = createRuntime('ru');
		const controller = new LanguageController(runtime);
		expect(controller.current).toBe('ru');
		expect(controller.locales).toEqual(['en', 'ru']);
	});

	it('does nothing when setting the already-active locale', () => {
		const { runtime, setLocale } = createRuntime('en');
		const controller = new LanguageController(runtime);
		controller.set('en');
		expect(setLocale).not.toHaveBeenCalled();
	});

	it('persists the new locale, updates reactive state, and triggers reload when window exists', () => {
		const { runtime, setLocale } = createRuntime('en');
		const controller = new LanguageController(runtime);
		const reload = vi.fn();
		const originalWindow = globalThis.window;

		Object.defineProperty(globalThis, 'window', {
			configurable: true,
			value: { location: { reload } }
		});
		try {
			controller.set('ru');
		} finally {
			Object.defineProperty(globalThis, 'window', {
				configurable: true,
				value: originalWindow
			});
		}

		expect(setLocale).toHaveBeenCalledWith('ru');
		expect(controller.current).toBe('ru');
		expect(reload).toHaveBeenCalledTimes(1);
	});
});

describe('LanguageSwitcher', () => {
	it('renders the active locale label with fallback uppercasing and custom labels', () => {
		const { runtime } = createRuntime('en');
		const controller = new LanguageController(runtime);

		const target = document.createElement('div');
		document.body.append(target);
		mounted.push(mount(LanguageSwitcher, { target, props: { controller } }));
		flushSync();
		expect(target.querySelector('.ls-label')?.textContent).toBe('EN');

		const customTarget = document.createElement('div');
		document.body.append(customTarget);
		mounted.push(
			mount(LanguageSwitcher, {
				target: customTarget,
				props: { controller, labels: { en: 'English', ru: 'Русский' } }
			})
		);
		flushSync();
		expect(customTarget.querySelector('.ls-label')?.textContent).toBe('English');
	});
});
