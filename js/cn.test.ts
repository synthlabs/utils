import { describe, expect, it } from 'vitest';
import { cn } from './cn';

describe('cn', () => {
	it('joins multiple classes', () => {
		expect(cn('px-2', 'py-1')).toBe('px-2 py-1');
	});

	it('lets later tailwind utilities win over earlier conflicting ones', () => {
		expect(cn('px-2', 'px-4')).toBe('px-4');
	});

	it('drops falsy entries from conditional class lists', () => {
		const isActive = false;
		expect(cn('base', isActive && 'active', undefined, null, '')).toBe('base');
	});
});
