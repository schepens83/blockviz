import { describe, expect, it } from 'vitest';
import { addRecent, readRecent } from './recent';

describe('recent repositories', () => {
  it('survives what storage may hold', () => {
    expect(readRecent('')).toEqual([]);
    expect(readRecent('{not json')).toEqual([]);
    expect(readRecent('{"a":1}')).toEqual([]);
    expect(readRecent('["/a", 3, "", "/b"]')).toEqual(['/a', '/b']);
  });

  it('puts the latest first, once, and keeps eight', () => {
    expect(addRecent(['/a', '/b'], '/b')).toEqual(['/b', '/a']);
    const many = Array.from({ length: 8 }, (_, i) => `/r${i}`);
    expect(addRecent(many, '/new')).toEqual(['/new', ...many.slice(0, 7)]);
  });
});
