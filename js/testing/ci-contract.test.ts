import { describe, expect, it } from 'vitest';
import { checkContract, checkPins, suitesOf, workflowSuiteSteps } from './ci-contract.mjs';

const samplePkg = {
	scripts: {
		test: 'pnpm test:unit && pnpm run test:scripts && pnpm test:rust',
		verify: 'pnpm check && pnpm lint && pnpm test && git diff --check'
	},
	devDependencies: {
		vitest: '5.1.1',
		'@playwright/test': '1.65.1'
	}
};

const validWorkflow = `
name: tests
jobs:
  verify:
    steps:
      - name: Check
        run: pnpm check
      - name: Lint
        run: pnpm lint
      - name: Unit tests
        run: pnpm test:unit
      - name: Script tests
        run: pnpm run test:scripts
      - name: Rust tests
        if: runner.os != 'Windows'
        run: pnpm test:rust
      - name: Whitespace
        run: git diff --check
`;

describe('ci-contract', () => {
	it('extracts suites from pnpm test scripts', () => {
		expect(suitesOf(samplePkg)).toEqual(['test:unit', 'test:scripts', 'test:rust']);
		expect(suitesOf(samplePkg, 'missing')).toEqual([]);
	});

	it('extracts named suite steps and conditions from YAML workflows', () => {
		expect(workflowSuiteSteps(validWorkflow)).toEqual([
			{ job: 'verify', name: 'Unit tests', suite: 'test:unit', if: undefined },
			{ job: 'verify', name: 'Script tests', suite: 'test:scripts', if: undefined },
			{ job: 'verify', name: 'Rust tests', suite: 'test:rust', if: "runner.os != 'Windows'" }
		]);
	});

	it('passes when suites, verify tokens, and Makefile forwarding match', () => {
		const result = checkContract({
			pkg: samplePkg,
			workflows: validWorkflow,
			verifyTokens: ['pnpm check', 'pnpm lint', 'git diff --check'],
			makefile: 'include utils/configs/testing.mk\n'
		});
		expect(result).toEqual({
			missingSuites: [],
			unnamedSteps: [],
			missingVerifyTokens: [],
			missingMakefileTargets: []
		});
	});

	it('reports missing suites, unnamed steps, missing verify tokens, and missing Make targets', () => {
		const brokenWorkflow = `
name: tests
jobs:
  verify:
    steps:
      - run: pnpm test:unit
      - name: Check
        run: pnpm check
`;
		const result = checkContract({
			pkg: samplePkg,
			workflows: brokenWorkflow,
			verifyTokens: ['pnpm check', 'pnpm lint'],
			makefile: '.PHONY: dev\ndev:\n\tpnpm dev\n'
		});
		expect(result.missingSuites).toEqual(['test:scripts', 'test:rust']);
		expect(result.unnamedSteps).toHaveLength(1);
		expect(result.unnamedSteps[0].suite).toBe('test:unit');
		expect(result.missingVerifyTokens).toEqual(['pnpm lint']);
		expect(result.missingMakefileTargets).toEqual(['test', 'verify', 'test-%']);
	});

	it('checks shared dependency pins across package manifests', () => {
		expect(checkPins(samplePkg, samplePkg)).toEqual([]);
		expect(
			checkPins({ devDependencies: { vitest: '5.0.4', '@playwright/test': '1.65.1' } }, samplePkg)
		).toEqual([{ name: 'vitest', expected: '5.1.1', actual: '5.0.4' }]);
	});
});
