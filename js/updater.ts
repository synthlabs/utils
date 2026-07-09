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

export type CheckForAppUpdatesOptions = {
	copy?: Partial<UpdateToastCopy>;
	durationMs?: number;
	openReleaseNotes?: (url: string) => Promise<void>;
};

const UPDATE_TOAST_ID = 'app-update-available';
const DEFAULT_TOAST_DURATION_MS = 12000;

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
	openReleaseNotes: (url: string) => Promise<void>
) {
	try {
		await openReleaseNotes(releaseUrl);
	} catch (error) {
		Logger.error('Failed to open release notes', error);
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
	openReleaseNotes: (url: string) => Promise<void>
): ExternalToast {
	return {
		...loadingToastOptions(id),
		action: {
			label: copy.releaseNotes,
			onClick: async (event) => {
				event.preventDefault();
				await openReleaseNotesUrl(releaseUrl, openReleaseNotes);
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
	const update = await check();

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
			label: copy.update,
			onClick: async (event) => {
				event.preventDefault();
				await installUpdate(
					toastId,
					copy,
					release_url,
					openReleaseNotes,
					update.downloadAndInstall.bind(update)
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
	downloadAndInstall: (onEvent?: (event: DownloadEvent) => void) => Promise<void>
) {
	let contentLength: number | undefined;
	let downloadedBytes = 0;
	let lastPercent: number | undefined;

	toast.loading(
		copy.downloading,
		downloadingToastOptions(toastId, copy, releaseUrl, openReleaseNotes)
	);

	try {
		await downloadAndInstall((event) => {
			if (event.event === 'Started') {
				contentLength = event.data.contentLength;
				downloadedBytes = 0;
				lastPercent = undefined;
				toast.loading(
					copy.downloading,
					downloadingToastOptions(toastId, copy, releaseUrl, openReleaseNotes)
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
						downloadingToastOptions(toastId, copy, releaseUrl, openReleaseNotes)
					);
				}

				return;
			}

			toast.loading(copy.installing, loadingToastOptions(toastId));
		});

		toast.loading(copy.restarting, loadingToastOptions(toastId));
		await relaunch();
	} catch (error) {
		Logger.error('Update install failed', error);
		toast.error(copy.installFailed(formatError(error)), {
			id: toastId,
			duration: 6000
		});
	}
}
