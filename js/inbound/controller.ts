import type { CapturedError, ErrorContext } from './types';

export type ErrorAction = {
	label: string | (() => string);
	run: () => void | Promise<void>;
};
export type ErrorPresentation = {
	source: string;
	title?: string | (() => string);
	recovery?: string | (() => string);
	action?: ErrorAction;
};
export type ErrorIncident = ErrorPresentation & {
	id: string;
	error: ErrorContext;
};
export type ErrorNotification = ErrorIncident & { count: number };
export type ErrorReport = { id: string; error: ErrorContext | null };
export type ErrorSnapshot = {
	notifications: ErrorNotification[];
	report: ErrorReport | null;
};

type Notice = {
	notification: ErrorNotification;
	key: string;
	remaining: number;
	started: number;
	timer?: ReturnType<typeof setTimeout>;
	pauses: Set<string>;
};

export function errorDetails(error: unknown): string {
	if (error instanceof Error) return error.stack || error.message;
	if (typeof error === 'string') return error;
	try {
		return JSON.stringify(error, null, 2) ?? String(error);
	} catch {
		return String(error);
	}
}

export class ErrorController {
	private sequence = 0;
	private notices = new Map<string, Notice>();
	private seen = new Set<string>();
	private handled = new WeakSet<object>();
	private listeners = new Set<(snapshot: ErrorSnapshot) => void>();
	private active = false;
	private report: ErrorReport | null = null;

	constructor(
		private readonly options: {
			durationMs?: number;
			now?: () => number;
			log?: (error: ErrorContext) => void;
		} = {}
	) {}

	private now() {
		return this.options.now?.() ?? Date.now();
	}
	private id() {
		return `frontend-${++this.sequence}`;
	}

	subscribe(listener: (snapshot: ErrorSnapshot) => void) {
		this.listeners.add(listener);
		listener(this.snapshot());
		return () => {
			this.listeners.delete(listener);
		};
	}

	snapshot(): ErrorSnapshot {
		return {
			notifications: this.report
				? []
				: [...this.notices.values()].map((n) => ({
						...n.notification
					})),
			report: this.report
		};
	}

	private publish() {
		const snapshot = this.snapshot();
		for (const listener of this.listeners) listener(snapshot);
	}

	start() {
		this.active = true;
		for (const notice of this.notices.values()) this.schedule(notice);
		return () => {
			this.active = false;
			for (const notice of this.notices.values()) this.stopTimer(notice);
		};
	}

	capture(error: unknown, presentation: ErrorPresentation, notify = true): ErrorIncident {
		if (error && typeof error === 'object') this.handled.add(error);
		const incident: ErrorIncident = {
			...presentation,
			id: this.id(),
			error: {
				kind: 'error',
				message: errorDetails(error),
				target: presentation.source,
				timestamp: String(Math.floor(this.now() / 1000))
			}
		};
		this.options.log?.(incident.error);
		if (notify)
			this.enqueue(incident, error instanceof Error ? error.message : incident.error.message);
		return incident;
	}

	wasHandled(error: unknown): boolean {
		return !!error && typeof error === 'object' && this.handled.has(error);
	}

	receive(captured: CapturedError) {
		if (this.seen.has(captured.id)) return;
		this.seen.add(captured.id);
		if (this.seen.size > 1024) this.seen.delete(this.seen.values().next().value!);
		this.enqueue({
			id: `native-${captured.id}`,
			error: { ...captured.error },
			source: captured.error.target ?? 'native'
		});
	}

	private enqueue(incident: ErrorIncident, message = incident.error.message) {
		const key = JSON.stringify([incident.source, incident.error.kind, message]);
		const existing = [...this.notices.values()].find((n) => n.key === key);
		if (existing) {
			existing.notification.count++;
		} else {
			const notice: Notice = {
				notification: { ...incident, count: 1 },
				key,
				remaining: this.options.durationMs ?? 10000,
				started: 0,
				pauses: new Set()
			};
			this.notices.set(incident.id, notice);
			this.schedule(notice);
		}
		this.publish();
	}

	private schedule(notice: Notice) {
		if (!this.active || this.report || notice.pauses.size || notice.timer !== undefined) return;
		notice.started = this.now();
		notice.timer = setTimeout(() => this.dismiss(notice.notification.id), notice.remaining);
	}

	private stopTimer(notice: Notice) {
		if (notice.timer === undefined) return;
		clearTimeout(notice.timer);
		notice.timer = undefined;
		notice.remaining = Math.max(0, notice.remaining - (this.now() - notice.started));
	}

	pause(id: string, reason: string) {
		const notice = this.notices.get(id);
		if (!notice) return;
		notice.pauses.add(reason);
		this.stopTimer(notice);
	}

	resume(id: string, reason: string) {
		const notice = this.notices.get(id);
		if (!notice) return;
		notice.pauses.delete(reason);
		this.schedule(notice);
	}

	dismiss(id: string) {
		const notice = this.notices.get(id);
		if (!notice) return;
		this.stopTimer(notice);
		this.notices.delete(id);
		this.publish();
	}

	openReport(incident: ErrorIncident | null = null) {
		if (this.report) return;
		for (const notice of this.notices.values()) {
			this.stopTimer(notice);
			notice.pauses.clear();
		}
		if (incident) this.notices.delete(incident.id);
		this.report = {
			id: this.id(),
			error: incident ? { ...incident.error } : null
		};
		this.publish();
	}

	closeReport() {
		this.report = null;
		for (const notice of this.notices.values()) this.schedule(notice);
		this.publish();
	}
}
