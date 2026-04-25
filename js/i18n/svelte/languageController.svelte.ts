/**
 * Headless controller around a Paraglide runtime. Each app instantiates this
 * with its own per-app generated runtime (the Paraglide compiler emits
 * different files per project, so we inject rather than import directly).
 *
 * Why a controller and not just direct calls to setLocale? Two reasons:
 *
 * 1. Paraglide message functions read the locale at call time, but Svelte
 *    components only re-evaluate when their reactive deps change. A locale
 *    change therefore needs a full re-render. We do that with
 *    `window.location.reload()` so every component picks up the new strings.
 *
 * 2. The reactive `current` rune lets UI bind to the active locale without
 *    each app re-implementing the wrapper.
 */
export interface ParaglideRuntime<L extends string = string> {
	getLocale: () => L;
	setLocale: (locale: L) => void;
	locales: readonly L[];
}

export class LanguageController<L extends string = string> {
	#runtime: ParaglideRuntime<L>;
	current = $state<L>('' as L);

	constructor(runtime: ParaglideRuntime<L>) {
		this.#runtime = runtime;
		this.current = runtime.getLocale();
	}

	get locales(): readonly L[] {
		return this.#runtime.locales;
	}

	/**
	 * Switch the active locale. Persists via Paraglide's configured strategy
	 * (typically localStorage for desktop apps), then reloads the window so
	 * every message function re-evaluates.
	 */
	set(locale: L): void {
		if (locale === this.current) return;
		this.#runtime.setLocale(locale);
		this.current = locale;
		if (typeof window !== 'undefined') window.location.reload();
	}
}
