export interface PackageManifest {
	scripts?: Record<string, string>;
	dependencies?: Record<string, string>;
	devDependencies?: Record<string, string>;
}

export interface WorkflowSuiteStep {
	job: string;
	name: string | undefined;
	suite: string;
	if: string | undefined;
}

export type WorkflowInput = string | { yaml: string; job?: string };

export interface ContractCheckOptions {
	pkg: PackageManifest;
	workflows: WorkflowInput | WorkflowInput[];
	verifyTokens?: string[];
	makefile?: string;
}

export interface ContractCheckResult {
	missingSuites: string[];
	unnamedSteps: WorkflowSuiteStep[];
	missingVerifyTokens: string[];
	missingMakefileTargets: string[];
}

export interface PinMismatch {
	name: string;
	expected: string | undefined;
	actual: string | undefined;
}

export function suitesOf(pkg: PackageManifest, script?: string): string[];

export function workflowSuiteSteps(
	yamlText: string,
	options?: { job?: string }
): WorkflowSuiteStep[];

export function checkContract(options: ContractCheckOptions): ContractCheckResult;

export function checkPins(
	appPkg: PackageManifest,
	utilsPkg: PackageManifest,
	names?: string[]
): PinMismatch[];
