import { describe, expect, it } from 'vitest';
import { ago, place } from './RefPicker';

describe('how long ago', () => {
  const now = Date.parse('2026-06-10T12:00:00Z');
  const at = (s: number) => new Date(now - s * 1000).toISOString();
  it('is brief', () => {
    expect(ago(at(20), now)).toBe('now');
    expect(ago(at(5 * 60), now)).toBe('5m ago');
    expect(ago(at(3 * 3600), now)).toBe('3h ago');
    expect(ago(at(4 * 86400), now)).toBe('4d ago');
    expect(ago(at(90 * 86400), now)).toBe('3mo ago');
    expect(ago(at(800 * 86400), now)).toBe('2y ago');
  });
});

describe('where the list opens', () => {
  const box = (left: number, top: number, width = 200) => ({ left, right: left + width, top, bottom: top + 32, width });

  it('is below the button, at least as wide as a list needs', () => {
    expect(place(box(100, 100), 1200, 800)).toMatchObject({ left: 100, width: 360, top: 136 });
  });

  it('stays within the window', () => {
    expect(place(box(1000, 100), 1200, 800)).toMatchObject({ left: 1200 - 12 - 360 });
    expect(place(box(0, 100), 300, 800)).toMatchObject({ left: 12, width: 276 });
  });

  it('opens upward when there is more room above', () => {
    const p = place(box(100, 700), 1200, 800);
    expect(p).toMatchObject({ bottom: 800 - 700 + 4 });
    expect(p.top).toBeUndefined();
  });
});
