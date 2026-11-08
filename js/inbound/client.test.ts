import { beforeEach, describe, expect, it, vi } from 'vitest';
const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
import { submitReport } from './client';
import type { ReportInput } from './types';

const input: ReportInput = {
	discord_user: null,
	message: 'slow startup',
	include: { system: false, build: false, log: false, config: false, error: false }
};
describe('report submission cancellation', () => {
	beforeEach(() => {
		mocks.invoke.mockReset();
	});
	it('keeps the existing call shape for consumers without cancellation', async () => {
		mocks.invoke.mockResolvedValue(undefined);
		await submitReport(input);
		expect(mocks.invoke).toHaveBeenCalledWith('plugin:inbound|submit', { input });
	});
	it('registers before submitting and sends cancellation to the same native request', async () => {
		let cancel!: () => void;
		const pending = new Promise<void>((_, reject) => {
			cancel = () => reject('Report submission canceled.');
		});
		mocks.invoke.mockImplementation((command: string) => {
			if (command.endsWith('|submit')) return pending;
			if (command.endsWith('|cancel_submission')) cancel();
			return Promise.resolve();
		});
		const abort = new AbortController();
		const result = submitReport(input, { signal: abort.signal });
		const rejected = expect(result).rejects.toContain('canceled');
		await Promise.resolve();
		abort.abort();
		await rejected;
		const registered = mocks.invoke.mock.calls[0][1].id;
		expect(mocks.invoke).toHaveBeenCalledWith('plugin:inbound|submit', { input, id: registered });
		expect(mocks.invoke).toHaveBeenCalledWith('plugin:inbound|cancel_submission', {
			id: registered
		});
		expect(mocks.invoke.mock.calls.filter(([command]) => command.endsWith('|submit'))).toHaveLength(
			1
		);
	});
	it('does not submit if cancellation arrives while registration is pending', async () => {
		let registered!: () => void;
		mocks.invoke.mockImplementation((command: string) =>
			command.endsWith('|register_submission')
				? new Promise<void>((resolve) => {
						registered = resolve;
					})
				: Promise.resolve()
		);
		const abort = new AbortController();
		const result = submitReport(input, { signal: abort.signal });
		const rejected = expect(result).rejects.toMatchObject({ name: 'AbortError' });
		abort.abort();
		registered();
		await rejected;
		expect(mocks.invoke.mock.calls.some(([command]) => command.endsWith('|submit'))).toBe(false);
	});
});
