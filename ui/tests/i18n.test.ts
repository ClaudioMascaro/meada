import { expect, test } from 'vitest';
import { copy, language } from '../src/i18n';

test.each([['pt-BR', 'pt-BR'], ['pt', 'pt-BR'], ['en-US', 'en'], ['de', 'en']])(
  'browser language %s selects %s', (browser, expected) => {
    expect(language(browser)).toBe(expected);
  },
);

test('both languages provide every copy key', () => {
  expect(Object.keys(copy['pt-BR']).sort()).toEqual(Object.keys(copy.en).sort());
  expect(Object.values(copy['pt-BR']).every(Boolean)).toBe(true);
  expect(Object.values(copy.en).every(Boolean)).toBe(true);
});
