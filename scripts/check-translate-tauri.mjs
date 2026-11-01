import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const crate = join(root, 'rust/translate');
const client = join(root, 'js/translate');
const args = process.argv.slice(2);
if (args.includes('--help')) {
	console.log('Usage: node scripts/check-translate-tauri.mjs [--quick] [--rust-only]');
	process.exit(0);
}
if (args.some((arg) => !['--quick', '--rust-only'].includes(arg))) {
	console.error('Invalid argument; use --help');
	process.exit(2);
}
const quick = args.includes('--quick');
const rustOnly = args.includes('--rust-only');
const commands = [];
if (!quick) {
	commands.push(['cargo', ['fmt', '--all', '--', '--check'], crate]);
	commands.push(['cargo', ['fmt', '--all', '--', '--check'], join(root, 'rust/translate-build')]);
	commands.push(['cargo', ['fmt', '--all', '--', '--check'], join(crate, 'tests/consumer')]);
	commands.push([
		'cargo',
		['clippy', '--locked', '--all-targets', '--features', 'tauri-test', '--', '-D', 'warnings'],
		crate
	]);
	commands.push([
		'cargo',
		['clippy', '--locked', '--all-targets', '--', '-D', 'warnings'],
		join(crate, 'tests/consumer')
	]);
}
commands.push([
	'cargo',
	['test', '--locked', '--features', 'tauri-test', ...(quick ? ['--lib', 'plugin::'] : [])],
	crate
]);
if (!quick) {
	commands.push(['cargo', ['test', '--locked', '--release', '--features', 'tauri-test'], crate]);
	commands.push(['cargo', ['test', '--locked'], join(crate, 'tests/consumer')]);
	commands.push(['cargo', ['test', '--locked', '--release'], join(crate, 'tests/consumer')]);
}
if (!rustOnly) {
	commands.push([
		'cargo',
		[
			'run',
			'--locked',
			'--quiet',
			'--features',
			'tauri-plugin',
			'--example',
			'export_translate_types',
			'--',
			'--check'
		],
		crate
	]);
	commands.push([process.execPath, ['--test', 'client.test.mjs'], client]);
	const require = createRequire(join(client, 'package.json'));
	commands.push([
		process.execPath,
		[require.resolve('typescript/bin/tsc'), '--project', 'tsconfig.json'],
		client
	]);
}
for (const [command, commandArgs, cwd] of commands) {
	console.log(`${command} ${commandArgs.join(' ')}`);
	const result = spawnSync(command, commandArgs, {
		cwd,
		stdio: 'inherit',
		env: {
			...process.env,
			CARGO_TARGET_DIR:
				cwd === join(crate, 'tests/consumer')
					? join(client, 'node_modules/.cache/tauri-consumer-target')
					: join(crate, 'target')
		}
	});
	if (result.error) console.error(result.error.message);
	if (result.status !== 0) process.exit(result.status ?? 1);
}
