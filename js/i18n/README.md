# @synthlabs/i18n

Shared i18n tooling for Synth Labs apps. Thin layer over [Paraglide JS](https://inlang.com/m/gerre34r/library-inlang-paraglideJs) — provides our standard Vite plugin config, an OS-locale detector, a Svelte 5 controller for runtime locale switching, and a CLI to keep locale files in sync.

## Consume from another repo

This package is consumed via pnpm `link:` (always-symlink), no publish:

```jsonc
// in your app's package.json
{
	"dependencies": {
		"@synthlabs/i18n": "link:../utils/js/i18n"
	},
	"devDependencies": {
		"@inlang/paraglide-js": "^2.16.0"
	}
}
```

The `@inlang/paraglide-js` peerDep belongs to the consuming app — it owns the generated runtime under `src/lib/paraglide/`.

## What's inside

### Vite plugin helper

```ts
// vite.config.ts
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { defineParaglide } from '@synthlabs/i18n';

export default defineConfig({
	plugins: [
		sveltekit(),
		defineParaglide() // defaults: project ./project.inlang, outdir ./src/lib/paraglide
	]
});
```

Default strategy is `['localStorage', 'preferredLanguage', 'baseLocale']` — right for desktop apps. Override for static sites with locale-prefixed URLs:

```ts
defineParaglide({ strategy: ['url', 'cookie', 'baseLocale'] });
```

### OS-locale detection

```ts
import { detectOsLocale } from '@synthlabs/i18n';

const locale = detectOsLocale(['en', 'ru'], 'en');
// "en-US" → "en", "ru-RU" → "ru", "de-DE" → "en" (fallback)
```

Walks `navigator.languages` in priority order, matches by exact tag then primary subtag.

### LanguageController + LanguageSwitcher (Svelte 5)

`LanguageController` is a reactive wrapper around the per-app Paraglide runtime. Each app injects its own runtime (the Paraglide compiler emits different files per project):

```svelte
<script lang="ts">
	import { LanguageController, LanguageSwitcher } from '@synthlabs/i18n/svelte';
	import * as runtime from '$lib/paraglide/runtime';

	const language = new LanguageController(runtime);
</script>

<LanguageSwitcher
	controller={language}
	labels={{ en: 'English', ru: 'Русский' }}
	accentColor="hsl(var(--c-scrybe))"
/>
```

`LanguageSwitcher` is a `bits-ui` v2 dropdown styled with vanilla CSS against shadcn design tokens (`--accent`, `--popover`, `--border`, `--foreground`, `--muted-foreground`, `--radius`). It inherits the host app's theme automatically and needs no Tailwind `@source` config in the consumer. The `labels` prop is optional — locale codes are uppercased as the fallback display. The optional `accentColor` prop accepts any CSS color string and tints the active locale code tag.

Need a custom UI? Use `LanguageController` directly:

```svelte
<select value={language.current} onchange={(e) => language.set(e.currentTarget.value)}>
	{#each language.locales as locale}
		<option value={locale}>{locale.toUpperCase()}</option>
	{/each}
</select>
```

`language.set()` reloads the window so every Paraglide message function re-evaluates against the new locale (Paraglide message fns are not Svelte-reactive).

### Inlang settings template

A canonical starting point for `project.inlang/settings.json`. Copy it into your app:

```bash
cp node_modules/@synthlabs/i18n/inlang-settings.template.json project.inlang/settings.json
# then edit `locales` to add your supported locales
```

### Lint script

Validates that every `messages/<locale>.json` has the same key set as the base locale. Wire it into `pnpm check`:

```jsonc
// package.json
{
	"scripts": {
		"check": "svelte-kit sync && svelte-check && i18n-lint ./messages"
	}
}
```

Direct invocation:

```bash
pnpm exec i18n-lint                # default: ./messages, base = en
pnpm exec i18n-lint ./messages ru  # treat ru as the base
```

Exit 0 on parity, exit 1 on any missing/extra key (with a per-locale diff printed).

## Conventions

- Import messages as `import { m as msgs } from '$lib/paraglide/messages';` so the local `m` identifier (commonly used as a loop var) doesn't collide.
- Keep the actual `messages/*.json` content in each app — never commit translation strings to this package.
- Add new locales by dropping `messages/<lang>.json` and appending to `project.inlang/settings.json` `locales`. The lint script will surface any missing keys on the next run.

## Peer dependencies

- `@inlang/paraglide-js` — required; the consuming app installs it and owns the generated runtime.
- `bits-ui` ^2.0.0 — required if you import `LanguageSwitcher` from `@synthlabs/i18n/svelte`. Skip if you only use `LanguageController`.
- `svelte` ^5.0.0 — required if you use anything under `/svelte`.
