export const defaultWizardCopy = {
	title: 'Send a bug report',
	description: 'Review each item before it leaves this machine.',
	username: 'Discord username',
	optional: 'optional',
	usernameHelp: 'This is only used to follow up about this report.',
	message: 'What happened?',
	messagePlaceholder: 'Add any context, expected behavior, or steps you tried.',
	system: 'System info',
	systemHelp: 'Operating system, CPU architecture, and locale.',
	build: 'Build info',
	buildHelp: 'App version, git commit, and build timestamp.',
	log: 'Log tail',
	logHelp: 'The most recent local app log lines, compressed before sending.',
	noLog: 'No log file was found.',
	config: 'Configuration snapshot',
	configHelp: 'App settings with known secrets redacted.',
	error: 'Detected error',
	errorHelp: 'The first captured error or panic from this session.',
	preview: 'Preview',
	discordLabel: 'Discord:',
	messageLabel: 'Message:',
	includedLabel: 'Included:',
	notProvided: '(not provided)',
	none: 'none',
	sending: 'Sending report…',
	done: 'Report sent',
	doneHelp: 'Thanks. A private Discord thread was created for this report.',
	failed: 'Report failed',
	back: 'Back',
	next: 'Next',
	send: 'Send',
	close: 'Done',
	retry: 'Try again',
	loading: 'Loading…',
	loadFailed: 'Could not load report details. Try again.',
	technicalDetails: 'Technical details',
	step: (current: number, total: number) => `Step ${current} of ${total}`,
	logSize: (raw: string, compressed: string) => `${raw} raw, ${compressed} compressed`
};
export type WizardCopy = typeof defaultWizardCopy;
export type WizardAction = {
	kind: 'back' | 'next' | 'send' | 'close' | 'retry';
	label: string;
	disabled: boolean;
	onclick: () => void;
};
export type WizardInclude = {
	id: string;
	title: string;
	description: string;
	checked: boolean;
	onchange: (checked: boolean) => void;
};
