import { describe, expect, it, vi } from 'vitest';
import { detectOsLocale } from './osLocale';

describe('detectOsLocale', () => {
	it('matches exact tags case-insensitively before falling back to primary subtags', () => {
		vi.stubGlobal('navigator', { languages: ['pt-br', 'en-US'], language: 'en-US' });
		expect(detectOsLocale(['en', 'pt-BR'] as const, 'en')).toBe('pt-BR');
	});

	it('falls back from regional tags to their primary subtag in priority order', () => {
		vi.stubGlobal('navigator', { languages: ['fr-CA', 'ru-RU', 'en-US'], language: 'en-US' });
		expect(detectOsLocale(['en', 'ru'] as const, 'en')).toBe('ru');
	});

	it('uses navigator.language when navigator.languages is empty', () => {
		vi.stubGlobal('navigator', { languages: [], language: 'ru-RU' });
		expect(detectOsLocale(['en', 'ru'] as const, 'en')).toBe('ru');
	});

	it('returns fallback when no locale matches or navigator is unavailable', () => {
		vi.stubGlobal('navigator', { languages: ['de-DE', ''], language: 'ja-JP' });
		expect(detectOsLocale(['en', 'ru'] as const, 'en')).toBe('en');

		vi.stubGlobal('navigator', undefined);
		expect(detectOsLocale(['en', 'ru'] as const, 'ru')).toBe('ru');
	});
});
