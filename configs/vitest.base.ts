import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { defineConfig } from 'vitest/config';

const here = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
	resolve: {
		alias: {
			$utils: path.resolve(here, '../js'),
			'$env/static/public': path.resolve(here, './env-static-public-stub.ts'),
		},
	},
	test: {
		environment: 'jsdom',
		globals: false,
		include: ['**/*.{test,spec}.ts'],
		exclude: ['**/node_modules/**', '**/dist/**', '**/build/**', '**/.svelte-kit/**', '**/target/**'],
	},
});
