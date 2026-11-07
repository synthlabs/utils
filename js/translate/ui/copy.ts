import type { ModelManagerAction, ModelManagerNotice } from '../controller';
import type {
	InstallationDto,
	InstallationStateDto,
	InstallPhaseDto,
	PairDto,
	ServicePhase
} from '../types';

export type ModelManagerControl = {
	kind: ModelManagerAction;
	label: string;
	disabled: boolean;
	title?: string;
	onclick: () => void;
};

export function formatModelBytes(decimal: string): string {
	if (!/^\d+$/.test(decimal)) return decimal;
	const bytes = BigInt(decimal);
	const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB', 'EiB'];
	let unit = 0;
	let divisor = 1n;
	while (unit < units.length - 1 && bytes >= divisor * 1024n) {
		divisor *= 1024n;
		unit++;
	}
	if (!unit) return `${bytes} B`;
	const tenths = (bytes * 10n) / divisor;
	return `${tenths / 10n}.${tenths % 10n} ${units[unit]}`;
}

/** File progress is determinate only while transferring or decompressing that file. */
export function modelInstallationProgress(operation: InstallationDto): number | undefined {
	if (operation.finished) return operation.result ? 100 : undefined;
	const progress = operation.progress;
	const counters =
		progress.phase === 'downloading'
			? [progress.fileDownloadBytes, progress.fileDownloadTotal]
			: progress.phase === 'decompressing'
				? [progress.fileInstalledBytes, progress.fileInstalledTotal]
				: null;
	if (!counters || counters.some((value) => !/^\d+$/.test(value))) return undefined;
	const [current, total] = counters.map(BigInt);
	if (total === 0n) return undefined;
	return Number(((current > total ? total : current) * 10000n) / total) / 100;
}

const language = (code: string) => ({ eng: 'English', rus: 'Russian' })[code] ?? code;
export const defaultModelManagerCopy = {
	title: 'Translation models',
	description: 'Manage models stored on this device.',
	installed: 'Installed',
	available: 'Available',
	attention: 'Needs attention',
	noInstalled: 'No installed models.',
	noAvailable: 'No additional models available.',
	noCatalog: 'Model catalog unavailable. Refresh the catalog to see available models.',
	cachedCatalog: 'Showing the cached model catalog.',
	catalogFailed: 'Could not refresh the model catalog.',
	refreshCatalog: 'Refresh catalog',
	refreshModels: 'Refresh model status',
	loading: 'Reading model status…',
	notConnected: 'Waiting for the host to connect model management.',
	retryInitialize: 'Retry initialization',
	resume: 'Resume translation',
	paused: 'Translation is paused.',
	releasing: 'Waiting for translation work to finish…',
	confirming: 'Waiting for confirmation…',
	disposing: 'Finishing accepted model changes…',
	install: 'Download',
	repair: 'Repair',
	replace: 'Replace with catalog model',
	remove: 'Remove',
	verify: 'Verify',
	clearDownloads: 'Clear downloads',
	inUse: 'In use',
	cleanupBusy: 'Downloads cannot be cleared while this model is in use.',
	differentCatalog: 'Installed model differs from the catalog.',
	unmanaged: 'This data is not managed by the model store. It will be left untouched.',
	cancel: 'Cancel',
	cancelling: 'Cancelling…',
	cancelHelp: 'Cancellation keeps reusable downloaded files. Use Clear downloads to remove them.',
	progressLabel: 'Current installation phase progress',
	awaitingResult: 'Waiting for the installation result…',
	installationFailed: 'Installation failed',
	installationCancelled: 'Installation cancelled',
	installationComplete: 'Installation complete',
	details: 'Details',
	dismissError: 'Dismiss error',
	cleanupWarnings: 'Installed successfully; some temporary files could not be cleaned up.',
	recoveryWarnings: 'Model store recovery needs attention.',
	modelId: 'Model',
	version: 'Version',
	downloadSize: 'Download size',
	installedSize: 'Installed size',
	directory: 'Directory',
	filename: 'Current file',
	file: 'File',
	attempt: 'Attempt',
	fileDownload: 'Current file download',
	fileInstalled: 'Current file decompression',
	downloaded: 'Completed download bytes',
	decompressed: 'Completed decompressed bytes',
	reused: 'Reused download bytes',
	network: 'Network traffic including retries',
	language,
	bytes: formatModelBytes,
	pair: (pair: PairDto, label: (code: string) => string) =>
		`${label(pair.source)} → ${label(pair.target)}`,
	notice: (kind: ModelManagerNotice['kind']) =>
		({
			verified: 'Verification succeeded at this point in time.',
			removed: 'Model removed.',
			absent: 'The model was already absent.',
			cleared: 'Retained downloads cleared.',
			no_downloads: 'No retained downloads were found.',
			installed: 'Model installation succeeded.',
			cancelled: 'Installation cancelled; reusable downloads were retained.'
		})[kind],
	localState: (state: InstallationStateDto) =>
		({
			not_installed: 'Not installed',
			present_unverified: 'Present, not verified',
			incomplete: 'Incomplete',
			invalid: 'Invalid',
			unavailable: 'Unavailable',
			unmanaged: 'Unmanaged',
			busy: 'Busy'
		})[state],
	servicePhase: (phase: ServicePhase) =>
		({
			initializing: 'Initializing model management…',
			ready: 'Ready',
			unavailable: 'Model management is unavailable.',
			releasing: 'Waiting for translation work to finish…',
			paused: 'Translation is paused.',
			shutdown: 'Translation service has shut down.'
		})[phase],
	installPhase: (phase: InstallPhaseDto) =>
		({
			idle: 'Waiting to install',
			preparing: 'Preparing installation',
			downloading: 'Downloading current file',
			retry_waiting: 'Waiting to retry download',
			verifying: 'Verifying files',
			decompressing: 'Decompressing current file',
			publishing: 'Publishing installation',
			completed: 'Finishing installation',
			cancelled: 'Finishing cancellation',
			failed: 'Finishing failed installation'
		})[phase]
};

export type ModelManagerCopy = typeof defaultModelManagerCopy;
