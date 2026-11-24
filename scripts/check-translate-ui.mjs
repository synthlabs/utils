import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const client = join(root, 'js/translate');
const require = createRequire(join(root, 'package.json'));
if (process.argv.length > 2) {
	console.error('Usage: node scripts/check-translate-ui.mjs');
	process.exit(2);
}
const commands = [
	[client, ['--test', 'client.test.mjs']],
	[client, [require.resolve('typescript/bin/tsc'), '--project', 'tsconfig.json']],
	[client, [require.resolve('typescript/bin/tsc'), '--project', 'tsconfig.ui.json']],
	[
		client,
		[
			require.resolve('svelte-check/bin/svelte-check'),
			'--config',
			'./svelte.config.js',
			'--tsconfig',
			'./tsconfig.ui.json',
			'--fail-on-warnings'
		]
	],
	[
		root,
		[
			join(dirname(require.resolve('vitest/package.json')), 'vitest.mjs'),
			'run',
			'--config',
			'vitest.config.ts',
			'--project',
			'translate'
		]
	]
];
for (const [cwd, args] of commands) {
	console.log(`node ${args.join(' ')}`);
	const result = spawnSync(process.execPath, args, { cwd, stdio: 'inherit' });
	if (result.error) console.error(result.error.message);
	if (result.status !== 0) process.exit(result.status ?? 1);
}
