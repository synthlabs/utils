import { PUBLIC_DEBUG_LOGS, PUBLIC_TRACE_LOGS } from '$env/static/public';
import {
	trace as pluginTrace,
	debug as pluginDebug,
	info as pluginInfo,
	warn as pluginWarn,
	error as pluginError,
} from '@tauri-apps/plugin-log';

const debugLogs: boolean = JSON.parse(PUBLIC_DEBUG_LOGS);
const traceLogs: boolean = JSON.parse(PUBLIC_TRACE_LOGS);

export function stringify(...args: unknown[]): string {
	return args
		.map((a) => {
			if (typeof a === 'string') return a;
			try {
				return JSON.stringify(a);
			} catch {
				return String(a);
			}
		})
		.join(' ');
}

export class Logger {
	info(...args: unknown[]) {
		const message = [...generatePrefix('INFO', '#3ABFF8'), ...args];
		console.log(...message);
		pluginInfo(stringify(...args)).catch(() => {});
	}
	warn(...args: unknown[]) {
		const message = [...generatePrefix('WARN', '#FBBD23'), ...args];
		console.log(...message);
		pluginWarn(stringify(...args)).catch(() => {});
	}
	error(...args: unknown[]) {
		const message = [...generatePrefix('ERROR', '#F87272'), ...args];
		console.error(...message);
		pluginError(stringify(...args)).catch(() => {});
	}
	debug(...args: unknown[]) {
		if (!debugLogs) return;
		const message = [...generatePrefix('DEBUG', '#D926A9'), ...args];
		console.log(...message);
		pluginDebug(stringify(...args)).catch(() => {});
	}
	trace(...args: unknown[]) {
		if (!traceLogs) return;
		const message = [...generatePrefix('TRACE', '#d95c26'), ...args];
		console.log(...message);
		pluginTrace(stringify(...args)).catch(() => {});
	}
}

export default new Logger();

function generatePrefix(namespace: string, color: string): string[] {
	return [`%c[${namespace}]:%c`, `color:${color}; font-weight:bold`, ''];
}
