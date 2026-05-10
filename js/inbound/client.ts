import { invoke } from '@tauri-apps/api/core';
import type { BuildInfo, Capabilities, ErrorContext, LogPreview, ReportInput, SystemInfo } from './types';

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

export function submitReport(input: ReportInput): Promise<void> {
	return invoke('plugin:inbound|submit', { input });
}
