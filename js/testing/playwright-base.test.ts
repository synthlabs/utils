import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { describe, expect, it, vi } from 'vitest';
import { definePlaywrightConfig } from '../../configs/playwright.base';
import { capturedBaseURL } from './browser-port';

describe('definePlaywrightConfig', () => {
	it('produces the standard Chromium serial configuration with default port pattern', () => {
		const rootDir = path.resolve('.');
		const root = pathToFileURL(rootDir);
		const config = definePlaywrightConfig({
			root,
			devServer: { command: 'node tests/support/browser/dev-server.mjs --port 0' }
		});

		expect(config.testDir).toBe('./tests/browser');
		expect(config.fullyParallel).toBe(false);
		expect(config.workers).toBe(1);
		expect(config.retries).toBe(0);
		expect(config.timeout).toBe(180000);
		expect(config.outputDir).toBe('target/test-results/browser');
		expect(config.use?.viewport).toEqual({ width: 1200, height: 800 });
		expect(config.use?.screenshot).toBe('only-on-failure');
		expect(config.use?.trace).toBe('retain-on-failure');
		expect(config.projects).toEqual([{ name: 'chromium', use: { browserName: 'chromium' } }]);
		expect(config.webServer).toMatchObject({
			command: 'node tests/support/browser/dev-server.mjs --port 0',
			wait: { stdout: /Browser port (?<browser_port>\d+)/ },
			reuseExistingServer: false,
			timeout: 90000,
			cwd: rootDir
		});
	});

	it('applies viewport, timeout, testDir, env, and custom port pattern overrides', () => {
		const root = pathToFileURL(path.resolve('.'));
		const portPattern = /App port (?<app_port>\d+)/;
		const config = definePlaywrightConfig({
			root,
			testDir: './custom/browser',
			timeout: 60000,
			viewport: { width: 1300, height: 900 },
			devServer: {
				command: 'node dev.mjs',
				env: { ENABLE_INTERNAL: '0' },
				portPattern,
				timeout: 30000
			}
		});

		expect(config.testDir).toBe('./custom/browser');
		expect(config.timeout).toBe(60000);
		expect(config.use?.viewport).toEqual({ width: 1300, height: 900 });
		expect(config.webServer).toMatchObject({
			env: { ENABLE_INTERNAL: '0' },
			wait: { stdout: portPattern },
			timeout: 30000
		});
	});
});

describe('capturedBaseURL', () => {
	it('reads BROWSER_PORT by default and supports custom env variable names', () => {
		vi.stubEnv('BROWSER_PORT', '41234');
		expect(capturedBaseURL()).toBe('http://127.0.0.1:41234');

		vi.stubEnv('CUSTOM_BROWSER_PORT', '5500');
		expect(capturedBaseURL('CUSTOM_BROWSER_PORT')).toBe('http://127.0.0.1:5500');
	});

	it('throws when the port variable is missing or non-numeric', () => {
		vi.stubEnv('BROWSER_PORT', '');
		expect(() => capturedBaseURL()).toThrow(/BROWSER_PORT/);

		vi.stubEnv('BROWSER_PORT', 'abc');
		expect(() => capturedBaseURL()).toThrow(/BROWSER_PORT/);
	});
});
