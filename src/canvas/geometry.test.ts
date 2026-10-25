import { describe, expect, it } from 'vitest';
import type { SceneNode } from '../core/types';
import { childXf, edgeMarker, frame, frameAround, frameReadable, locate, markerArea, NO_INSETS, panToReveal, parentXf, scrimRects, READABLE, rectLine, smoothPath, viewOf, visibleRuns, xfOf, zoomAt } from './geometry';

const box = (over: Partial<SceneNode> = {}): SceneNode => ({
  id: 1, kind: 'function', shape: 'box', category: 'function', title: 'f',
  x: 40, y: 60, w: 200, h: 150, inner: { x: 8, y: 30, w: 184, h: 112 }, ...over,
});

describe('placement', () => {
  it('child and parent transforms are inverses', () => {
    const t = { x: 13, y: -7, s: 0.8 };
    const n = box();
    const c = childXf(t, n, 600, 300);
    const back = parentXf(c, n, 600, 300);
    expect(back.x).toBeCloseTo(t.x, 9);
    expect(back.y).toBeCloseTo(t.y, 9);
    expect(back.s).toBeCloseTo(t.s, 12);
  });

  it('fits content inside the inner rect, centered', () => {
    const n = box();
    const c = childXf({ x: 0, y: 0, s: 1 }, n, 368, 112);
    // Width-limited: 184 / 368 = 0.5, content height 56 centered in 112.
    expect(c.s).toBeCloseTo(0.5);
    expect(c.x).toBeCloseTo(48);
    expect(c.y).toBeCloseTo(60 + 30 + 28);
  });

  it('stays precise many levels deep', () => {
    let t = { x: 0, y: 0, s: 1 };
    const n = box();
    for (let i = 0; i < 30; i++) t = childXf(t, n, 600, 300);
    for (let i = 0; i < 30; i++) t = parentXf(t, n, 600, 300);
    expect(t.s).toBeCloseTo(1, 6);
    expect(Math.abs(t.x)).toBeLessThan(1e-3);
  });
});

describe('camera', () => {
  it('zooming keeps the point under the cursor fixed', () => {
    const t = { x: 100, y: 50, s: 2 };
    const z = zoomAt(t, 3, 400, 300);
    const u = (400 - t.x) / t.s;
    expect(z.x + u * z.s).toBeCloseTo(400);
  });

  it('views round-trip through transforms', () => {
    const t = { x: -120, y: 33, s: 1.7 };
    const back = xfOf(viewOf(t, 1200, 800), 1200, 800);
    expect(back.x).toBeCloseTo(t.x);
    expect(back.y).toBeCloseTo(t.y);
    expect(back.s).toBeCloseTo(t.s);
  });

  it('frames a box with padding and the screen aspect', () => {
    const v = frame({ x: 0, y: 0, w: 100, h: 100 }, 1600, 800, 0.1);
    expect(v.cx).toBe(50);
    expect(v.w).toBeCloseTo(200 * 1.2);
  });

  it('smooth paths start and end at the given views', () => {
    const a = { cx: 0, cy: 0, w: 1000 };
    const b = { cx: 5000, cy: -300, w: 20 };
    const p = smoothPath(a, b);
    const s = p.at(0);
    const e = p.at(1);
    expect(s.cx).toBeCloseTo(a.cx);
    expect(s.w).toBeCloseTo(a.w);
    expect(e.cx).toBeCloseTo(b.cx, 3);
    expect(e.w).toBeCloseTo(b.w, 3);
    // Zooms out on the way when traveling far.
    expect(Math.max(...[0.25, 0.5, 0.75].map(t => p.at(t).w))).toBeGreaterThan(a.w);
  });

  it('smooth paths stay finite between views far apart in zoom, starting exactly where they are', () => {
    // From a block about forty levels deep to a file across the project: 10²⁰ apart in zoom.
    const deep = { cx: 12.3456789, cy: 45.678, w: 1e-17 };
    const far = { cx: 300, cy: -40, w: 1e3 };
    for (const [a, b] of [[deep, far], [far, deep]]) {
      const p = smoothPath(a, b);
      expect(p.length).toBeGreaterThan(0);
      for (let i = 0; i <= 100; i++) {
        const v = p.at(i / 100);
        expect([v.cx, v.cy, v.w].every(Number.isFinite)).toBe(true);
        expect(v.w).toBeGreaterThan(0);
      }
      expect(p.at(0)).toEqual(a);
      expect(p.at(1).cx).toBeCloseTo(b.cx, 9);
      expect(p.at(1).w / b.w).toBeCloseTo(1, 9);
    }
    // Leaving a deep view, it zooms out before it moves: the first frames keep the start in view.
    const out = smoothPath(deep, far);
    for (const t of [1e-3, 1e-2]) {
      const v = out.at(t);
      expect(Math.abs(v.cx - deep.cx)).toBeLessThan(v.w / 2);
    }
    const pure = smoothPath({ cx: 0, cy: 0, w: 1e-200 }, { cx: 0, cy: 0, w: 1e200 });
    expect(pure.at(0.5).w).toBeCloseTo(1, 9);
  });

  it('pure zooms interpolate scale', () => {
    const p = smoothPath({ cx: 1, cy: 1, w: 100 }, { cx: 1, cy: 1, w: 1 });
    expect(p.at(1).w).toBeCloseTo(1);
    expect(p.at(0.5).w).toBeCloseTo(10, 0);
  });

  // Screen position of a frame point under a view.
  const screen = (v: { cx: number; cy: number; w: number }, x: number, y: number, W = 1000, H = 800) => {
    const s = W / v.w;
    return { x: W / 2 + (x - v.cx) * s, y: H / 2 + (y - v.cy) * s, s };
  };

  it('opens a box whole when its diagram is readable that way', () => {
    const b = { x: 0, y: 0, w: 400, h: 300 };
    expect(frameReadable(b, 2, 1000, 800, 0.08, NO_INSETS)).toEqual(frame(b, 1000, 800, 0.08));
  });

  it('opens a tall diagram at a readable size from its top', () => {
    // One interior unit is 0.01 frame units: fitting the whole box would show
    // its text at a fraction of a pixel.
    const b = { x: 0, y: 0, w: 4, h: 40 };
    const v = frameReadable(b, 0.01, 1000, 800, 0.08, NO_INSETS);
    const top = screen(v, 2, 0);
    expect(top.s * 0.01).toBeCloseTo(READABLE);
    expect(top.y).toBeCloseTo(16);
    expect(top.x).toBeCloseTo(500);
    // Never wider than the box's full width, even if text is then smaller.
    const wide = screen(frameReadable(b, 0.001, 1000, 800, 0.08, NO_INSETS), 0, 0);
    expect(4 * wide.s).toBeCloseTo(1000 / 1.06);
  });

  it('shows a block with nothing inside among its neighbors', () => {
    const b = { x: 100, y: 100, w: 20, h: 10 };
    const v = frameAround(b, 0.1, 1000, 800, NO_INSETS);
    const c = screen(v, 110, 105);
    expect(c.x).toBeCloseTo(500);
    expect(c.y).toBeCloseTo(400);
    // Its diagram's names read at a comfortable size; the block stays small.
    expect(c.s * 0.1).toBeCloseTo(READABLE * 1.15);
    expect(20 * c.s).toBeLessThan(1000 * 0.55);
  });
});

describe('lines cut to the screen', () => {
  it('keeps a line on screen whole', () => {
    expect(visibleRuns([10, 10, 50, 10, 50, 80], [0, 6, 0], 0, 0, 100, 100)).toEqual([{ pts: [10, 10, 50, 10, 50, 80], radii: [0, 6, 0], at: 0 }]);
  });

  it('starts a long line where it comes on screen, as far along as it is there', () => {
    const [run, ...rest] = visibleRuns([-1000, 0, 50, 0, 50, 300], [0, 10, 0], 0, -10, 100, 200);
    expect(rest).toEqual([]);
    expect(run.pts).toEqual([0, 0, 50, 0, 50, 200]);
    expect(run.radii).toEqual([0, 10, 0]);
    expect(run.at).toBe(1000);
  });

  it('counts the corners passed off screen as drawn, rounded', () => {
    // Down 500, round a right angle of radius 10 (an arc of 5π for 20 of tangents), across 500.
    const [run] = visibleRuns([-500, -500, -500, 0, 1000, 0], [0, 10, 0], 0, -10, 100, 10);
    expect(run.pts).toEqual([0, 0, 100, 0]);
    expect(run.at).toBeCloseTo(1000 - (20 - 5 * Math.PI), 9);
  });

  it('leaves the screen and comes back as two runs', () => {
    const runs = visibleRuns([10, 5, 200, 5, 200, 50, 10, 50], [0, 0, 0, 0], 0, 0, 100, 100);
    expect(runs.map(r => r.pts)).toEqual([[10, 5, 100, 5], [100, 50, 10, 50]]);
    expect(runs[1].at).toBe(190 + 45 + 100);
  });

  it('draws nothing of a line off screen', () => {
    expect(visibleRuns([200, 0, 300, 0, 300, 400], [0, 10, 0], 0, 0, 100, 100)).toEqual([]);
  });

  it('draws nothing of a line running beside the screen, and all of one along its edge', () => {
    expect(visibleRuns([-20, -5, 120, -5], [0, 0], 0, 0, 100, 100)).toEqual([]);
    expect(visibleRuns([10, 0, 90, 0], [0, 0], 0, 0, 100, 100)).toEqual([{ pts: [10, 0, 90, 0], radii: [0, 0], at: 0 }]);
  });

  it('goes round a rectangle as roundRect does, its corners no rounder than it fits', () => {
    expect(rectLine({ x: 0, y: 0, w: 40, h: 10 }, 8)).toEqual({ pts: [5, 0, 40, 0, 40, 10, 0, 10, 0, 0, 5, 0], radii: [0, 5, 5, 5, 5, 0] });
  });
});

describe('panning a box into view', () => {
  const i = { top: 50, right: 300, bottom: 40, left: 0 };
  // The visible part of a 1000×600 screen is 700×510 from (0, 50), less the margin.
  it('leaves a box that is in view where it is', () => {
    expect(panToReveal({ x: 100, y: 100, w: 50, h: 50 }, 1000, 600, i, 10)).toEqual({ dx: 0, dy: 0 });
  });

  it('moves a box that sticks out just far enough', () => {
    expect(panToReveal({ x: 650, y: 100, w: 100, h: 50 }, 1000, 600, i, 10)).toEqual({ dx: -60, dy: 0 });
    expect(panToReveal({ x: 100, y: 20, w: 50, h: 50 }, 1000, 600, i, 10)).toEqual({ dx: 0, dy: 40 });
  });

  it('centers a box that is off screen', () => {
    const p = panToReveal({ x: 2000, y: 1000, w: 100, h: 100 }, 1000, 600, i, 10);
    expect(2000 + p.dx + 50).toBe(350);
    expect(1000 + p.dy + 50).toBe(305);
  });

  it('shows the start of a box too big to fit, and leaves one that fills the view', () => {
    expect(panToReveal({ x: -500, y: 100, w: 3000, h: 50 }, 1000, 600, i, 10)).toEqual({ dx: 0, dy: 0 });
    expect(panToReveal({ x: 900, y: 100, w: 3000, h: 50 }, 1000, 600, i, 10).dx).toBe(-890);
  });
});

describe('pointing out where a change is', () => {
  const view = { x: 0, y: 50, w: 800, h: 500 };
  const hit = (id: number, box: { x: number; y: number; w: number; h: number }, clear: boolean) => ({ id, box, clear });

  it('is the block itself when it can be made out', () => {
    const b = { x: 100, y: 100, w: 200, h: 80 };
    expect(locate(7, [3, 1], [hit(7, b, true), hit(3, view, true)], b, view)).toEqual({ kind: 'itself', box: b });
  });

  it('is the nearest block around it when it is too small or not opened', () => {
    const tiny = { x: 120, y: 120, w: 3, h: 3 };
    const room = { x: 100, y: 100, w: 300, h: 200 };
    expect(locate(7, [3, 1], [hit(7, tiny, false), hit(3, room, true), hit(1, view, true)], tiny, view)).toEqual({ kind: 'within', id: 3, box: room });
    expect(locate(7, [3, 1], [hit(3, room, false), hit(1, view, true)], tiny, view)).toEqual({ kind: 'within', id: 1, box: view });
    expect(locate(7, [3, 1], [hit(1, view, true)], undefined, view)).toEqual({ kind: 'within', id: 1, box: view });
  });

  it('is away when it is outside the view, even with its container in view', () => {
    const far = { x: 2000, y: 100, w: 100, h: 50 };
    expect(locate(7, [1], [hit(1, view, true)], far, view)).toEqual({ kind: 'away', box: far });
    expect(locate(7, [], [], undefined, view)).toBeNull();
  });

  it('puts a marker where the line to it leaves the view', () => {
    const m = edgeMarker(view, { x: 1900, y: 275, w: 100, h: 50 }, 10)!;
    expect([m.x, m.y, m.angle]).toEqual([790, 300, 0]);
    const up = edgeMarker(view, { x: 350, y: -900, w: 100, h: 50 }, 10)!;
    expect(up.y).toBe(60);
    expect(up.x).toBe(400);
    expect(up.angle).toBeCloseTo(-Math.PI / 2, 9);
    const d = edgeMarker(view, { x: 1800, y: 1800, w: 100, h: 100 }, 10)!;
    expect(d.y).toBe(540);
    expect(d.x).toBeLessThan(790);
    expect(edgeMarker(view, { x: 700, y: 100, w: 300, h: 50 }, 10)).toBeNull();
  });

  it('holds the marker and its label toward the middle of the view', () => {
    const right = markerArea({ x: 790, y: 300, angle: 0 }, 80);
    expect(right.x).toBeLessThan(700);
    expect(right.x + right.w).toBeGreaterThan(790);
    const left = markerArea({ x: 10, y: 300, angle: Math.PI }, 80);
    expect(left.x + left.w).toBeGreaterThan(100);
  });
});

describe('the spotlight scrim', () => {
  const view = { x: 0, y: 50, w: 800, h: 500 };
  const area = (bs: { w: number; h: number }[]) => bs.reduce((a, b) => a + Math.max(0, b.w) * Math.max(0, b.h), 0);

  it('covers the view but for the hole', () => {
    expect(area(scrimRects(view, { x: 100, y: 150, w: 200, h: 80 }))).toBe(800 * 500 - 200 * 80);
  });

  it('cuts a hole that hangs out of the view to it', () => {
    expect(area(scrimRects(view, { x: -50, y: 50, w: 100, h: 100 }))).toBe(800 * 500 - 50 * 100);
  });
});
