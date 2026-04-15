// updater.ts
import { check } from '@tauri-apps/plugin-updater';
import { ask } from '@tauri-apps/plugin-dialog';
import { relaunch } from '@tauri-apps/plugin-process';
import Logger from '$utils/log';

export async function checkForAppUpdates(release_url: string) {
	const update = await check();
	if (!update) {
		Logger.info('No update available');
	} else if (update) {
		Logger.info('Update available!', update);
		const yes = await ask(
			`Update to ${update.version} is available!\n\nRelease notes: ${release_url}`,
			{
				title: 'Update Available',
				kind: 'info',
				okLabel: 'Update',
				cancelLabel: 'Cancel'
			}
		);
		if (yes) {
			await update.downloadAndInstall();
			await relaunch();
		}
	}
}
