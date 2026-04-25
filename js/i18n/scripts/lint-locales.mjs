#!/usr/bin/env node
/**
 * Validate that every locale file under `messages/` has the same key set as
 * the base locale. Run from any consuming app:
 *
 *     pnpm exec i18n-lint               # default: ./messages, base = en
 *     pnpm exec i18n-lint ./messages
 *     pnpm exec i18n-lint ./messages ru # treat ru.json as base
 *
 * Exits 0 on parity, 1 on any missing/extra keys.
 */
import { readdir, readFile } from 'node:fs/promises';
import { resolve, basename, extname, join } from 'node:path';

const META_KEYS = new Set(['$schema']);

const args = process.argv.slice(2);
const dir = resolve(args[0] ?? './messages');
const baseLocaleArg = args[1] ?? 'en';

async function loadLocales(messagesDir) {
	let entries;
	try {
		entries = await readdir(messagesDir);
	} catch (err) {
		console.error(`i18n-lint: cannot read directory ${messagesDir}: ${err.message}`);
		process.exit(1);
	}

	const files = entries.filter((f) => extname(f) === '.json');
	if (files.length === 0) {
		console.error(`i18n-lint: no .json files found in ${messagesDir}`);
		process.exit(1);
	}

	const locales = {};
	for (const file of files) {
		const locale = basename(file, '.json');
		const content = await readFile(join(messagesDir, file), 'utf-8');
		try {
			locales[locale] = JSON.parse(content);
		} catch (err) {
			console.error(`i18n-lint: invalid JSON in ${file}: ${err.message}`);
			process.exit(1);
		}
	}
	return locales;
}

function keysOf(obj) {
	return Object.keys(obj).filter((k) => !META_KEYS.has(k));
}

const locales = await loadLocales(dir);
const localeNames = Object.keys(locales);

if (!locales[baseLocaleArg]) {
	console.error(
		`i18n-lint: base locale "${baseLocaleArg}" not found. Available: ${localeNames.join(', ')}`
	);
	process.exit(1);
}

const baseKeys = new Set(keysOf(locales[baseLocaleArg]));
let problems = 0;

for (const locale of localeNames) {
	if (locale === baseLocaleArg) continue;
	const keys = new Set(keysOf(locales[locale]));

	const missing = [...baseKeys].filter((k) => !keys.has(k));
	const extra = [...keys].filter((k) => !baseKeys.has(k));

	if (missing.length > 0) {
		console.error(`✗ ${locale}: missing ${missing.length} key(s) present in ${baseLocaleArg}:`);
		for (const k of missing) console.error(`    - ${k}`);
		problems += missing.length;
	}

	if (extra.length > 0) {
		console.error(`✗ ${locale}: ${extra.length} extra key(s) not in ${baseLocaleArg}:`);
		for (const k of extra) console.error(`    - ${k}`);
		problems += extra.length;
	}

	if (missing.length === 0 && extra.length === 0) {
		console.log(`✓ ${locale}: ${keys.size} keys, parity with ${baseLocaleArg}`);
	}
}

console.log(`✓ ${baseLocaleArg}: ${baseKeys.size} keys (base)`);

if (problems > 0) {
	console.error(`\ni18n-lint: ${problems} problem(s) across ${localeNames.length - 1} locale(s)`);
	process.exit(1);
}
