import { listen } from '@tauri-apps/api/event';
import { error as logError } from '@tauri-apps/plugin-log';
import { capturedErrors } from './client';
import { ErrorController, errorDetails } from './controller';
import type { CapturedError } from './types';

export function logHandledError(error: unknown, source = 'inbound') {
	console.error(`[${source}]`, error);
	void logError(`[${source}] ${errorDetails(error)}`, {
		keyValues: { 'inbound.handled': 'true' }
	}).catch(() => {});
}

export function connectErrorCapture(controller: ErrorController, target?: Window) {
	let disposed = false;
	let unlisten: (() => void) | undefined;
	void (async () => {
		try {
			const stop = await listen<CapturedError>('inbound://error-captured', (event) => {
				if (!disposed) controller.receive(event.payload);
			});
			if (disposed) {
				stop();
				return;
			}
			unlisten = stop;
			const snapshot = await capturedErrors();
			if (!disposed) snapshot.forEach((error) => controller.receive(error));
		} catch (error) {
			logHandledError(error, 'inbound.capture');
		}
	})();

	const onError = (event: ErrorEvent) => {
		if (!event.message || controller.wasHandled(event.error)) return;
		controller.capture(event.error ?? event.message, {
			source: 'javascript.exception'
		});
	};
	const onRejection = (event: PromiseRejectionEvent) => {
		if (controller.wasHandled(event.reason)) return;
		controller.capture(event.reason, { source: 'javascript.rejection' });
	};
	target?.addEventListener('error', onError);
	target?.addEventListener('unhandledrejection', onRejection);
	return () => {
		disposed = true;
		unlisten?.();
		target?.removeEventListener('error', onError);
		target?.removeEventListener('unhandledrejection', onRejection);
	};
}
