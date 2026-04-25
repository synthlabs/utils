import { paraglideVitePlugin } from '@inlang/paraglide-js';

export type ParaglideStrategy =
	| 'cookie'
	| 'url'
	| 'localStorage'
	| 'preferredLanguage'
	| 'baseLocale'
	| 'globalVariable';

export interface DefineParaglideOptions {
	/** Path to the inlang project. Defaults to `./project.inlang`. */
	project?: string;
	/** Where to emit the generated runtime. Defaults to `./src/lib/paraglide`. */
	outdir?: string;
	/**
	 * Locale resolution strategy. Defaults to the desktop-app stack:
	 * `['localStorage', 'preferredLanguage', 'baseLocale']`.
	 *
	 * For static sites with locale-prefixed URLs (e.g. /ru), pass:
	 * `['url', 'cookie', 'baseLocale']`.
	 */
	strategy?: ParaglideStrategy[];
}

const DESKTOP_DEFAULT_STRATEGY: ParaglideStrategy[] = [
	'localStorage',
	'preferredLanguage',
	'baseLocale'
];

/**
 * Wraps `paraglideVitePlugin` with Synth Labs defaults so each app's
 * `vite.config.ts` only needs one line.
 */
export function defineParaglide(opts: DefineParaglideOptions = {}) {
	return paraglideVitePlugin({
		project: opts.project ?? './project.inlang',
		outdir: opts.outdir ?? './src/lib/paraglide',
		strategy: opts.strategy ?? DESKTOP_DEFAULT_STRATEGY
	});
}
