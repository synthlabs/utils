import { spawnSync } from 'node:child_process';
import { realpathSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const utilsRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const crateRoot = join(utilsRoot, 'rust', 'translate');
const args = process.argv.slice(2);
let models;
let hostedModels;
let liveCatalog = false;
let downloadModel;

for (let index = 0; index < args.length; index++) {
	if (args[index] === '--help') {
		console.log(
			'Usage: node scripts/check-translate.mjs [--models <directory>] [--hosted-models <directory>] [--live-catalog] [--download-model <rus-eng|eng-rus>]'
		);
		process.exit(0);
	}
	const option = args[index];
	if (option === '--download-model') {
		if (downloadModel || !['rus-eng', 'eng-rus'].includes(args[index + 1])) {
			console.error('--download-model requires exactly one direction: rus-eng or eng-rus');
			process.exit(2);
		}
		downloadModel = args[++index];
		continue;
	}
	if (option === '--live-catalog' && !liveCatalog) {
		liveCatalog = true;
		continue;
	}
	if (
		!['--models', '--hosted-models'].includes(option) ||
		(option === '--models' ? models : hostedModels) ||
		!args[index + 1] ||
		args[index + 1].startsWith('--')
	) {
		console.error(`Invalid argument: ${args[index]}`);
		process.exit(2);
	}
	try {
		const directory = realpathSync(resolve(args[++index]));
		if (!statSync(directory).isDirectory()) throw new Error('not a directory');
		if (option === '--models') models = directory;
		else hostedModels = directory;
	} catch (error) {
		console.error(`Invalid model directory: ${error.message}`);
		process.exit(2);
	}
}

for (const features of [[], ['--features', 'catalog-http,model-downloads,workload']]) {
	const tree = spawnSync(
		'cargo',
		['tree', '--locked', '--edges', 'normal', '--prefix', 'none', ...features],
		{
			cwd: crateRoot,
			encoding: 'utf8'
		}
	);
	if (tree.error || tree.status !== 0) {
		console.error(tree.error?.message ?? tree.stderr);
		process.exit(tree.status ?? 1);
	}
	if (/^tauri(?:-[\w-]+)? v/m.test(tree.stdout)) {
		console.error('Unexpected Tauri dependency in the core-only feature configuration');
		process.exit(1);
	}
}

const commands = [['fmt', '--all', '--', '--check']];
for (const features of [
	[],
	['--features', 'catalog-http'],
	['--features', 'model-downloads'],
	['--features', 'workload'],
	['--features', 'catalog-http,model-downloads,workload']
]) {
	commands.push(
		['clippy', '--locked', '--all-targets', ...features, '--', '-D', 'warnings'],
		['test', '--locked', ...features],
		['test', '--locked', '--release', ...features]
	);
}
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
if (hostedModels) {
	commands.push([
		'test',
		'--locked',
		'--release',
		'--test',
		'hosted_models',
		'--',
		'--ignored',
		'--test-threads=1'
	]);
}
if (liveCatalog) {
	commands.push([
		'test',
		'--locked',
		'--release',
		'--features',
		'catalog-http,model-downloads,workload',
		'--test',
		'catalog_http',
		'--',
		'--ignored',
		'--nocapture'
	]);
}
if (downloadModel) {
	commands.push([
		'test',
		'--locked',
		'--release',
		'--features',
		'catalog-http,model-downloads,workload',
		'--test',
		'download_models',
		'--',
		'--ignored',
		'--nocapture',
		'--test-threads=1'
	]);
}

for (const command of commands) {
	console.log(`cargo ${command.join(' ')}`);
	const result = spawnSync('cargo', command, {
		cwd: crateRoot,
		stdio: 'inherit',
		env: {
			...process.env,
			...(models ? { SYNTH_TRANSLATE_TEST_MODELS: models } : {}),
			...(hostedModels ? { SYNTH_TRANSLATE_HOSTED_MODELS: hostedModels } : {}),
			SYNTH_TRANSLATE_DOWNLOAD_PAIR: downloadModel ?? ''
		}
	});
	if (result.error) console.error(result.error.message);
	if (result.status !== 0) process.exit(result.status ?? 1);
}
