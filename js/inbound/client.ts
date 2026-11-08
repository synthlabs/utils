import { invoke } from '@tauri-apps/api/core';
import type {
	BuildInfo,
	CapturedError,
	Capabilities,
	ErrorContext,
	LogPreview,
	ReportInput,
	SystemInfo
} from './types';

export function inboundCapabilities(): Promise<Capabilities> {
	return invoke('plugin:inbound|capabilities');
}

export function previewSystem(): Promise<SystemInfo> {
	return invoke('plugin:inbound|preview_system');
}

export function previewBuild(): Promise<BuildInfo> {
	return invoke('plugin:inbound|preview_build');
}

export function previewLogTail(maxBytes = 2 * 1024 * 1024): Promise<LogPreview> {
	return invoke('plugin:inbound|preview_log_tail', { maxBytes });
}

export function previewConfig(): Promise<string | null> {
	return invoke('plugin:inbound|preview_config');
}

export function previewError(): Promise<ErrorContext | null> {
	return invoke('plugin:inbound|preview_error');
}

export function capturedErrors(): Promise<CapturedError[]> {
	return invoke('plugin:inbound|captured_errors');
}

export async function submitReport(
	input: ReportInput,
	options: { signal?: AbortSignal } = {}
): Promise<void> {
	if (!options.signal) return invoke('plugin:inbound|submit', { input });
	const id = crypto.randomUUID();
	await invoke('plugin:inbound|register_submission', { id });
	const cancel = () => invoke<void>('plugin:inbound|cancel_submission', { id });
	if (options.signal.aborted) {
		await cancel();
		throw new DOMException(
			'Report submission canceled. Delivery could not be confirmed; check before sending again.',
			'AbortError'
		);
	}
	const onAbort = () => {
		void cancel().catch(() => {});
	};
	options.signal.addEventListener('abort', onAbort, { once: true });
	try {
		await invoke('plugin:inbound|submit', { input, id });
	} finally {
		options.signal.removeEventListener('abort', onAbort);
	}
}
