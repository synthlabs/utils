import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
	check: vi.fn(),
	openUrl: vi.fn(),
	relaunch: vi.fn(),
	toastError: vi.fn(),
	toastInfo: vi.fn(),
	toastLoading: vi.fn(),
	logError: vi.fn(),
	logInfo: vi.fn()
}));

vi.mock('@tauri-apps/plugin-updater', () => ({
	check: mocks.check
}));

vi.mock('@tauri-apps/plugin-opener', () => ({
	openUrl: mocks.openUrl
}));

vi.mock('@tauri-apps/plugin-process', () => ({
	relaunch: mocks.relaunch
}));

vi.mock('svelte-sonner', () => ({
	toast: {
		error: mocks.toastError,
		info: mocks.toastInfo,
		loading: mocks.toastLoading
	}
}));

vi.mock('./log', () => ({
	default: {
		error: mocks.logError,
		info: mocks.logInfo
	}
}));

import { checkForAppUpdates } from './updater';

type ToastAction = {
	action: {
		label: string;
		onClick: (event: { preventDefault: () => void }) => Promise<void>;
	};
};

function updateToastAction(): ToastAction['action'] {
	const options = mocks.toastInfo.mock.calls[0]?.[1] as ToastAction | undefined;
	if (!options) throw new Error('Update toast was not shown');
	return options.action;
}

function availableUpdate() {
	return {
		version: '0.3.0',
		downloadAndInstall: vi.fn().mockResolvedValue(undefined)
	};
}

describe('checkForAppUpdates', () => {
	beforeEach(() => {
		vi.clearAllMocks();
		mocks.relaunch.mockResolvedValue(undefined);
	});

	it('does not show a toast when no update is available', async () => {
		mocks.check.mockResolvedValue(null);

		await checkForAppUpdates('https://example.com/releases/latest');

		expect(mocks.toastInfo).not.toHaveBeenCalled();
		expect(mocks.logInfo).toHaveBeenCalledWith('No update available');
	});

	it('downloads, installs, and relaunches with the default action', async () => {
		const update = availableUpdate();
		mocks.check.mockResolvedValue(update);

		await checkForAppUpdates('https://example.com/releases/latest');
		const action = updateToastAction();
		await action.onClick({ preventDefault: vi.fn() });

		expect(action.label).toBe('Update');
		expect(update.downloadAndInstall).toHaveBeenCalledOnce();
		expect(mocks.relaunch).toHaveBeenCalledOnce();
	});

	it('opens an external update URL without installing or relaunching', async () => {
		const update = availableUpdate();
		mocks.check.mockResolvedValue(update);
		mocks.openUrl.mockResolvedValue(undefined);

		await checkForAppUpdates('https://example.com/releases/latest', {
			updateAction: {
				kind: 'external',
				label: 'Update via package manager',
				url: 'https://aur.archlinux.org/packages/pepo-bin'
			}
		});
		const action = updateToastAction();
		await action.onClick({ preventDefault: vi.fn() });

		expect(action.label).toBe('Update via package manager');
		expect(mocks.openUrl).toHaveBeenCalledWith('https://aur.archlinux.org/packages/pepo-bin');
		expect(update.downloadAndInstall).not.toHaveBeenCalled();
		expect(mocks.relaunch).not.toHaveBeenCalled();
	});

	it('logs an external URL failure without attempting installation', async () => {
		const update = availableUpdate();
		const error = new Error('opener failed');
		mocks.check.mockResolvedValue(update);
		mocks.openUrl.mockRejectedValue(error);

		await checkForAppUpdates('https://example.com/releases/latest', {
			updateAction: {
				kind: 'external',
				label: 'Update via package manager',
				url: 'https://aur.archlinux.org/packages/pepo-bin'
			}
		});
		await updateToastAction().onClick({ preventDefault: vi.fn() });

		expect(mocks.logError).toHaveBeenCalledWith('Failed to open external update URL', error);
		expect(update.downloadAndInstall).not.toHaveBeenCalled();
		expect(mocks.relaunch).not.toHaveBeenCalled();
	});
});
