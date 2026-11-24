import { fileURLToPath } from 'node:url';
import { defineConfig, type PlaywrightTestConfig } from '@playwright/test';

export interface PlaywrightBaseOptions {
	root: URL | string;
	devServer: {
		command: string;
		env?: Record<string, string>;
		portPattern?: RegExp;
		timeout?: number;
	};
	testDir?: string;
	timeout?: number;
	viewport?: { width: number; height: number };
}

function resolveRootDir(root: URL | string): string {
	if (typeof root === 'string') {
		return root.startsWith('file://') ? fileURLToPath(root) : root;
	}
	return fileURLToPath(root.href);
}

export function definePlaywrightConfig(options: PlaywrightBaseOptions): PlaywrightTestConfig {
	return defineConfig({
		testDir: options.testDir ?? './tests/browser',
		fullyParallel: false,
		workers: 1,
		retries: 0,
		timeout: options.timeout ?? 180000,
		outputDir: 'target/test-results/browser',
		reporter: [['list'], ['json', { outputFile: 'target/test-results/playwright.json' }]],
		use: {
			viewport: options.viewport ?? { width: 1200, height: 800 },
			deviceScaleFactor: 1,
			reducedMotion: 'no-preference',
			screenshot: 'only-on-failure',
			trace: 'retain-on-failure',
			launchOptions: {
				args: ['--disable-gpu', '--disable-dev-shm-usage', '--disable-background-networking']
			}
		},
		projects: [{ name: 'chromium', use: { browserName: 'chromium' } }],
		webServer: {
			command: options.devServer.command,
			wait: { stdout: options.devServer.portPattern ?? /Browser port (?<browser_port>\d+)/ },
			reuseExistingServer: false,
			timeout: options.devServer.timeout ?? 90000,
			cwd: resolveRootDir(options.root),
			...(options.devServer.env ? { env: options.devServer.env } : {})
		}
	});
}
