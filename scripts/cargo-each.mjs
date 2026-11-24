import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

function quoteCmdArg(value) {
	if (!/[ \t&()^|<>"]/.test(value)) return value;
	return `"${value.replaceAll('"', '""')}"`;
}

function spawnCargo(cargo, args, cwd) {
	if (process.platform === 'win32' && /\.(cmd|bat)$/i.test(cargo)) {
		return spawnSync(
			process.env.ComSpec ?? 'cmd.exe',
			['/d', '/s', '/c', [cargo, ...args].map(quoteCmdArg).join(' ')],
			{ cwd, stdio: 'inherit' }
		);
	}
	return spawnSync(cargo, args, { cwd, stdio: 'inherit' });
}

export function discoverCrateDirs(rustRoot) {
	return readdirSync(rustRoot, { withFileTypes: true })
		.filter((entry) => entry.isDirectory())
		.map((entry) => path.join(rustRoot, entry.name))
		.filter((dir) => existsSync(path.join(dir, 'Cargo.toml')))
		.sort((a, b) => path.basename(a).localeCompare(path.basename(b)));
}

export function runCargoEach(args, { rustRoot, cargo = process.env.CARGO || 'cargo' } = {}) {
	if (args.length === 0) {
		console.error('Usage: node scripts/cargo-each.mjs <cargo-subcommand> [args...]');
		return 2;
	}

	const crates = discoverCrateDirs(rustRoot);
	if (crates.length === 0) {
		console.error(`No Cargo.toml crates found under ${rustRoot}`);
		return 1;
	}

	for (const crateDir of crates) {
		console.log(`==> [${path.basename(crateDir)}] ${cargo} ${args.join(' ')}`);
		const result = spawnCargo(cargo, args, crateDir);
		if (result.error) {
			console.error(result.error.message);
			return 1;
		}
		if (result.status !== 0) {
			return result.status ?? 1;
		}
	}

	return 0;
}

const thisFilePath = fileURLToPath(new URL(import.meta.url));
if (process.argv[1] && path.resolve(process.argv[1]) === thisFilePath) {
	const root = fileURLToPath(new URL('..', import.meta.url));
	process.exitCode = runCargoEach(process.argv.slice(2), {
		rustRoot: path.join(root, 'rust')
	});
}
