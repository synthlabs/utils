import { spawnSync } from 'node:child_process';
import { realpathSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const utilsRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const crateRoot = join(utilsRoot, 'rust', 'translate');
const args = process.argv.slice(2);
let models;

for (let index = 0; index < args.length; index++) {
	if (args[index] === '--help') {
		console.log('Usage: node scripts/check-translate.mjs [--models <local-model-directory>]');
		process.exit(0);
	}
	if (
		args[index] !== '--models' ||
		models ||
		!args[index + 1] ||
		args[index + 1].startsWith('--')
	) {
		console.error(`Invalid argument: ${args[index]}`);
		process.exit(2);
	}
	try {
		models = realpathSync(resolve(args[++index]));
		if (!statSync(models).isDirectory()) throw new Error('not a directory');
	} catch (error) {
		console.error(`Invalid model directory: ${error.message}`);
		process.exit(2);
	}
}

const commands = [
	['fmt', '--all', '--', '--check'],
	['clippy', '--locked', '--all-targets', '--', '-D', 'warnings'],
	['test', '--locked'],
	['test', '--locked', '--release']
];
if (models) {
	commands.push([
		'test',
		'--locked',
		'--release',
		'--test',
		'local_models',
		'--',
		'--ignored',
		'--test-threads=1'
	]);
}

for (const command of commands) {
	console.log(`cargo ${command.join(' ')}`);
	const result = spawnSync('cargo', command, {
		cwd: crateRoot,
		stdio: 'inherit',
		env: { ...process.env, ...(models ? { SYNTH_TRANSLATE_TEST_MODELS: models } : {}) }
	});
	if (result.error) console.error(result.error.message);
	if (result.status !== 0) process.exit(result.status ?? 1);
}
