export interface BuildInfo {
	app_version: string;
	app_commit: string;
	build_time: string;
}

export interface ErrorContext {
	kind: string;
	message: string;
	target: string | null;
	timestamp: string;
}

export interface IncludeFlags {
	system: boolean;
	build: boolean;
	log: boolean;
	config: boolean;
	error: boolean;
}

export interface LogPreview {
	exists: boolean;
	byte_count: number;
	preview_text: string;
	gzipped_size: number;
}

export interface ReportInput {
	discord_user: string | null;
	message: string;
	include: IncludeFlags;
}

export interface SystemInfo {
	os: string;
	arch: string;
	locale: string | null;
}

export interface Capabilities {
	has_config: boolean;
}

export type StepId = 'username' | 'message' | 'system' | 'build' | 'log' | 'config' | 'error' | 'preview' | 'sending' | 'done' | 'failed';
