import { svelte } from '@sveltejs/vite-plugin-svelte';
import { mergeConfig } from 'vitest/config';
import base from './configs/vitest.base.ts';

export default mergeConfig(base, {
	test: {
		projects: [
			{
				extends: true,
				test: {
					name: 'js',
					include: ['js/**/*.test.ts'],
					exclude: [
						'**/node_modules/**',
						'**/dist/**',
						'**/build/**',
						'**/target/**',
						'js/translate/**',
						'js/i18n/**'
					]
				}
			},
			{
				extends: true,
				plugins: [svelte({ configFile: false })],
				resolve: { conditions: ['browser'] },
				test: {
					name: 'translate',
					include: ['js/translate/**/*.test.ts']
				}
			},
			{
				extends: true,
				plugins: [svelte({ configFile: false })],
				resolve: { conditions: ['browser'], dedupe: ['svelte'] },
				test: {
					name: 'i18n',
					include: ['js/i18n/**/*.test.ts'],
					server: {
						deps: {
							inline: ['bits-ui', 'runed', 'svelte-toolbelt', 'esm-env']
						}
					}
				}
			}
		]
	}
});
