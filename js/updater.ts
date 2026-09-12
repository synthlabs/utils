// updater.ts
import { check, type DownloadEvent } from '@tauri-apps/plugin-updater';
import { openUrl } from '@tauri-apps/plugin-opener';
import { relaunch } from '@tauri-apps/plugin-process';
import { toast, type ExternalToast } from 'svelte-sonner';
import Logger from '$utils/log';

type ToastId = string | number;

export type UpdateToastCopy = {
	updateAvailable: (version: string) => string;
	releaseNotes: string;
	dismiss: string;
	update: string;
	downloading: string;
	downloadingProgress: (percent: number) => string;
	installing: string;
	restarting: string;
	installFailed: (error: string) => string;
};

export type UpdateErrorStage = 'check' | 'install' | 'release_notes' | 'external_update';
export type UpdateErrorHandler = (error: unknown, stage: UpdateErrorStage) => void;

export type CheckForAppUpdatesOptions = {
	onError?: UpdateErrorHandler;
	copy?: Partial<UpdateToastCopy>;
	durationMs?: number;
	openReleaseNotes?: (url: string) => Promise<void>;
	updateAction?: UpdateAction;
};

export type UpdateAction = { kind: 'install' } | { kind: 'external'; label: string; url: string };

const UPDATE_TOAST_ID = 'app-update-available';
const DEFAULT_TOAST_DURATION_MS = 12000;
const DEFAULT_UPDATE_ACTION: UpdateAction = { kind: 'install' };

const DEFAULT_COPY: UpdateToastCopy = {
	updateAvailable: (version) => `Update ${version} available`,
	releaseNotes: 'Release notes',
	dismiss: 'Dismiss',
	update: 'Update',
	downloading: 'Downloading update...',
	downloadingProgress: (percent) => `Downloading update... ${percent}%`,
	installing: 'Installing update...',
	restarting: 'Restarting...',
	installFailed: (error) => `Update failed: ${error}`
};

function resolveCopy(copy?: Partial<UpdateToastCopy>): UpdateToastCopy {
	return {
		updateAvailable: copy?.updateAvailable ?? DEFAULT_COPY.updateAvailable,
		releaseNotes: copy?.releaseNotes ?? DEFAULT_COPY.releaseNotes,
		dismiss: copy?.dismiss ?? DEFAULT_COPY.dismiss,
		update: copy?.update ?? DEFAULT_COPY.update,
		downloading: copy?.downloading ?? DEFAULT_COPY.downloading,
		downloadingProgress: copy?.downloadingProgress ?? DEFAULT_COPY.downloadingProgress,
		installing: copy?.installing ?? DEFAULT_COPY.installing,
		restarting: copy?.restarting ?? DEFAULT_COPY.restarting,
		installFailed: copy?.installFailed ?? DEFAULT_COPY.installFailed
	};
}

async function openReleaseNotesUrl(
	releaseUrl: string,
	openReleaseNotes: (url: string) => Promise<void>,
	onError?: UpdateErrorHandler
) {
	try {
		await openReleaseNotes(releaseUrl);
	} catch (error) {
		if (onError) onError(error, 'release_notes');
		else Logger.error('Failed to open release notes', error);
	}
}

async function openExternalUpdateUrl(
	updateUrl: string,
	openExternalUrl: (url: string) => Promise<void>,
	onError?: UpdateErrorHandler
) {
	try {
		await openExternalUrl(updateUrl);
	} catch (error) {
		if (onError) onError(error, 'external_update');
		else Logger.error('Failed to open external update URL', error);
	}
}

function formatError(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}

function loadingToastOptions(id: ToastId): ExternalToast {
	return {
		id,
		duration: Number.POSITIVE_INFINITY,
		dismissable: false
	};
}

function downloadingToastOptions(
	id: ToastId,
	copy: UpdateToastCopy,
	releaseUrl: string,
	openReleaseNotes: (url: string) => Promise<void>,
	onError?: UpdateErrorHandler
): ExternalToast {
	return {
		...loadingToastOptions(id),
		action: {
			label: copy.releaseNotes,
			onClick: async (event) => {
				event.preventDefault();
				await openReleaseNotesUrl(releaseUrl, openReleaseNotes, onError);
			}
		}
	};
}

function downloadProgressPercent(
	downloadedBytes: number,
	contentLength?: number
): number | undefined {
	if (!contentLength || contentLength <= 0) return undefined;
	return Math.min(100, Math.max(0, Math.floor((downloadedBytes / contentLength) * 100)));
}

export async function checkForAppUpdates(
	release_url: string,
	options: CheckForAppUpdatesOptions = {}
) {
	const copy = resolveCopy(options.copy);
	const duration = options.durationMs ?? DEFAULT_TOAST_DURATION_MS;
	const openReleaseNotes = options.openReleaseNotes ?? openUrl;
	const updateAction = options.updateAction ?? DEFAULT_UPDATE_ACTION;
	let update;
	try {
		update = await check();
	} catch (error) {
		if (!options.onError) throw error;
		options.onError(error, 'check');
		return;
	}

	if (!update) {
		Logger.info('No update available');
		return;
	}

	Logger.info('Update available!', update);

	const toastId = UPDATE_TOAST_ID;

	toast.info(copy.updateAvailable(update.version), {
		id: toastId,
		duration,
		action: {
			label: updateAction.kind === 'external' ? updateAction.label : copy.update,
			onClick: async (event) => {
				event.preventDefault();

				if (updateAction.kind === 'external') {
					await openExternalUpdateUrl(
						updateAction.url,
						openReleaseNotes,
						options.onError
					);
					return;
				}

				await installUpdate(
					toastId,
					copy,
					release_url,
					openReleaseNotes,
					update.downloadAndInstall.bind(update),
					options.onError
				);
			}
		},
		cancel: {
			label: copy.dismiss,
			onClick: () => {}
		}
	});
}

async function installUpdate(
	toastId: ToastId,
	copy: UpdateToastCopy,
	releaseUrl: string,
	openReleaseNotes: (url: string) => Promise<void>,
	downloadAndInstall: (onEvent?: (event: DownloadEvent) => void) => Promise<void>,
	onError?: UpdateErrorHandler
) {
	let contentLength: number | undefined;
	let downloadedBytes = 0;
	let lastPercent: number | undefined;

	toast.loading(
		copy.downloading,
		downloadingToastOptions(toastId, copy, releaseUrl, openReleaseNotes, onError)
	);

	try {
		await downloadAndInstall((event) => {
			if (event.event === 'Started') {
				contentLength = event.data.contentLength;
				downloadedBytes = 0;
				lastPercent = undefined;
				toast.loading(
					copy.downloading,
					downloadingToastOptions(toastId, copy, releaseUrl, openReleaseNotes, onError)
				);
				return;
			}

			if (event.event === 'Progress') {
				downloadedBytes += event.data.chunkLength;
				const percent = downloadProgressPercent(downloadedBytes, contentLength);

				if (percent !== undefined && percent !== lastPercent) {
					lastPercent = percent;
					toast.loading(
						copy.downloadingProgress(percent),
						downloadingToastOptions(
							toastId,
							copy,
							releaseUrl,
							openReleaseNotes,
							onError
						)
					);
				}

				return;
			}

			toast.loading(copy.installing, loadingToastOptions(toastId));
		});

		toast.loading(copy.restarting, loadingToastOptions(toastId));
		await relaunch();
	} catch (error) {
		if (onError) {
			toast.dismiss(toastId);
			onError(error, 'install');
			return;
		}
		Logger.error('Update install failed', error);
		toast.error(copy.installFailed(formatError(error)), {
			id: toastId,
			duration: 6000
		});
	}
}
