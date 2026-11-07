import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const client = join(root, 'js/translate');
const require = createRequire(join(client, 'package.json'));
if (process.argv.length > 2) {
	console.error('Usage: node scripts/check-translate-ui.mjs');
	process.exit(2);
}
const commands = [
	['--test', 'client.test.mjs'],
	[require.resolve('typescript/bin/tsc'), '--project', 'tsconfig.json'],
	[require.resolve('typescript/bin/tsc'), '--project', 'tsconfig.ui.json'],
	[
		require.resolve('svelte-check/bin/svelte-check'),
		'--config',
		'./svelte.config.js',
		'--tsconfig',
		'./tsconfig.ui.json',
		'--fail-on-warnings'
	],
	[
		join(dirname(require.resolve('vitest/package.json')), 'vitest.mjs'),
		'run',
		'--config',
		'vitest.config.mts'
	]
];
for (const args of commands) {
	console.log(`node ${args.join(' ')}`);
	const result = spawnSync(process.execPath, args, { cwd: client, stdio: 'inherit' });
	if (result.error) console.error(result.error.message);
	if (result.status !== 0) process.exit(result.status ?? 1);
}
