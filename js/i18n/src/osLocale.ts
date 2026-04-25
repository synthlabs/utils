/**
 * Pick the best supported locale from the user's OS preferences.
 *
 * Walks `navigator.languages` in priority order. Matches first by exact tag
 * (e.g. `en-US` against `en-US`), then by primary subtag (`en-US` → `en`).
 * Returns `fallback` if nothing matches or `navigator` is unavailable (SSR).
 */
export function detectOsLocale<T extends string>(supported: readonly T[], fallback: T): T {
	if (typeof navigator === 'undefined') return fallback;

	const candidates: string[] = [];
	if (Array.isArray(navigator.languages)) candidates.push(...navigator.languages);
	if (navigator.language) candidates.push(navigator.language);

	const lowered = supported.map((s) => s.toLowerCase()) as string[];

	for (const raw of candidates) {
		if (!raw) continue;
		const lower = raw.toLowerCase();

		const exactIdx = lowered.indexOf(lower);
		if (exactIdx !== -1) return supported[exactIdx];

		const primary = lower.split('-')[0];
		const primaryIdx = lowered.indexOf(primary);
		if (primaryIdx !== -1) return supported[primaryIdx];
	}

	return fallback;
}
