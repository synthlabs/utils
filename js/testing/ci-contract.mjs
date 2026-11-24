import { parse } from 'yaml';

/** @typedef {import('./ci-contract.d.ts').PackageManifest} PackageManifest */
/** @typedef {import('./ci-contract.d.ts').WorkflowSuiteStep} WorkflowSuiteStep */
/** @typedef {import('./ci-contract.d.ts').WorkflowInput} WorkflowInput */
/** @typedef {import('./ci-contract.d.ts').ContractCheckOptions} ContractCheckOptions */
/** @typedef {import('./ci-contract.d.ts').ContractCheckResult} ContractCheckResult */
/** @typedef {import('./ci-contract.d.ts').PinMismatch} PinMismatch */

const SUITE_TOKEN_RE = /\bpnpm\s+(?:run\s+)?(test:[\w:]+)/g;

/**
 * @param {PackageManifest} pkg
 * @param {string} [script='test']
 * @returns {string[]}
 */
export function suitesOf(pkg, script = 'test') {
	const value = pkg?.scripts?.[script] ?? '';
	/** @type {Set<string>} */
	const seen = new Set();
	for (const match of value.matchAll(SUITE_TOKEN_RE)) {
		seen.add(match[1]);
	}
	return [...seen];
}

/**
 * @param {string} yamlText
 * @param {{ job?: string }} [options={}]
 * @returns {WorkflowSuiteStep[]}
 */
export function workflowSuiteSteps(yamlText, options = {}) {
	const { job } = options;
	const doc = parse(yamlText);
	const jobs = doc?.jobs ?? {};
	const entries = job ? (jobs[job] ? [[job, jobs[job]]] : []) : Object.entries(jobs);
	/** @type {WorkflowSuiteStep[]} */
	const result = [];

	for (const [jobName, jobDef] of entries) {
		const steps = Array.isArray(jobDef?.steps) ? jobDef.steps : [];
		for (const step of steps) {
			if (typeof step?.run !== 'string') continue;
			for (const match of step.run.matchAll(SUITE_TOKEN_RE)) {
				result.push({
					job: jobName,
					name: step.name,
					suite: match[1],
					if: step.if
				});
			}
		}
	}

	return result;
}

/**
 * @param {string} yamlText
 * @param {string | undefined} job
 * @returns {string}
 */
function collectRuns(yamlText, job) {
	const doc = parse(yamlText);
	const jobs = doc?.jobs ?? {};
	const entries = job ? (jobs[job] ? [[job, jobs[job]]] : []) : Object.entries(jobs);
	/** @type {string[]} */
	const runs = [];
	for (const [, jobDef] of entries) {
		const steps = Array.isArray(jobDef?.steps) ? jobDef.steps : [];
		for (const step of steps) {
			if (typeof step?.run === 'string') runs.push(step.run);
		}
	}
	return runs.join('\n');
}

/**
 * @param {ContractCheckOptions} options
 * @returns {ContractCheckResult}
 */
export function checkContract({ pkg, workflows, verifyTokens = [], makefile }) {
	const list = Array.isArray(workflows) ? workflows : [workflows];
	const normalized = list.map((entry) =>
		typeof entry === 'string' ? { yaml: entry, job: undefined } : entry
	);

	const suiteSteps = normalized.flatMap(({ yaml, job }) => workflowSuiteSteps(yaml, { job }));
	const suites = suitesOf(pkg, 'test');

	const missingSuites = suites.filter((suite) => !suiteSteps.some((step) => step.suite === suite));
	const unnamedSteps = suiteSteps.filter(
		(step) => typeof step.name !== 'string' || step.name.trim().length === 0
	);

	const verifyScript = pkg?.scripts?.verify ?? '';
	const allRuns = normalized.map(({ yaml, job }) => collectRuns(yaml, job)).join('\n');
	const missingVerifyTokens = verifyTokens.filter(
		(token) => !verifyScript.includes(token) || !allRuns.includes(token)
	);

	/** @type {string[]} */
	const missingMakefileTargets = [];
	if (typeof makefile === 'string') {
		const usesSharedFragment = /^include\s+\S*testing\.mk\s*$/m.test(makefile);
		if (!usesSharedFragment) {
			if (!/^test:\s*/m.test(makefile)) missingMakefileTargets.push('test');
			if (!/^verify:\s*/m.test(makefile)) missingMakefileTargets.push('verify');
			if (!/^test-%:\s*/m.test(makefile) || !makefile.includes('run test:$(subst -,:,$*)')) {
				missingMakefileTargets.push('test-%');
			}
		}
	}

	return {
		missingSuites,
		unnamedSteps,
		missingVerifyTokens,
		missingMakefileTargets
	};
}

/**
 * @param {PackageManifest} pkg
 * @param {string} name
 * @returns {string | undefined}
 */
function dependencyVersion(pkg, name) {
	return pkg?.devDependencies?.[name] ?? pkg?.dependencies?.[name];
}

/**
 * @param {PackageManifest} appPkg
 * @param {PackageManifest} utilsPkg
 * @param {string[]} [names=['vitest', '@playwright/test']]
 * @returns {PinMismatch[]}
 */
export function checkPins(appPkg, utilsPkg, names = ['vitest', '@playwright/test']) {
	/** @type {PinMismatch[]} */
	const mismatches = [];
	for (const name of names) {
		const expected = dependencyVersion(utilsPkg, name);
		const actual = dependencyVersion(appPkg, name);
		if (expected === undefined || actual !== expected) {
			mismatches.push({ name, expected, actual });
		}
	}
	return mismatches;
}
