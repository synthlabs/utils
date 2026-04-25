import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/plugin-log', () => ({
	trace: vi.fn(),
	debug: vi.fn(),
	info: vi.fn(),
	warn: vi.fn(),
	error: vi.fn(),
}));

import { stringify } from './log';

describe('stringify', () => {
	it('passes strings through unchanged', () => {
		expect(stringify('hello', 'world')).toBe('hello world');
	});

	it('JSON.stringifies non-string values', () => {
		expect(stringify({ a: 1 }, [2, 3], 'tail')).toBe('{"a":1} [2,3] tail');
	});

	it('falls back to String() when JSON.stringify throws (e.g. circular refs)', () => {
		type Node = { self?: Node };
		const circular: Node = {};
		circular.self = circular;
		const out = stringify(circular);
		expect(out).toBe(String(circular));
	});
});
