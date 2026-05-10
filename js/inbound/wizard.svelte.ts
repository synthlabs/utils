import type { ErrorContext, IncludeFlags, StepId } from './types';

export class InboundWizardState {
	step = $state<StepId>('username');
	discordUser = $state('');
	message = $state('');
	include = $state<IncludeFlags>({
		system: true,
		build: true,
		log: true,
		config: false,
		error: false
	});
	hasConfig = $state(false);
	prefilledError = $state<ErrorContext | null>(null);
	errorMessage = $state('');

	constructor(prefilledError: ErrorContext | null) {
		this.prefilledError = prefilledError;
		this.include.error = prefilledError !== null;
	}

	visibleSteps(): StepId[] {
		const steps: StepId[] = ['username', 'message', 'system', 'build', 'log'];
		if (this.hasConfig) steps.push('config');
		if (this.prefilledError) steps.push('error');
		steps.push('preview');
		return steps;
	}

	next() {
		const steps = this.visibleSteps();
		const index = steps.indexOf(this.step);
		if (index >= 0 && index < steps.length - 1) {
			this.step = steps[index + 1];
		}
	}

	back() {
		const steps = this.visibleSteps();
		const index = steps.indexOf(this.step);
		if (index > 0) {
			this.step = steps[index - 1];
		}
	}
}
