// updater.ts
import { check } from '@tauri-apps/plugin-updater';
import { ask } from '@tauri-apps/plugin-dialog';
import { relaunch } from '@tauri-apps/plugin-process';

export async function checkForAppUpdates(release_url: string) {
	const update = await check();
	if (!update) {
		console.log('No update available');
	} else if (update) {
		console.log('Update available!', update);
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
