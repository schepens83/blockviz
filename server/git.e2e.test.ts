import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { SceneDiff } from '../src/canvas/diff';
import { SceneModel } from '../src/canvas/model';
import type { Project } from '../src/core/types';
import { analyze_project, initSync } from '../src/wasm/blockviz_core';
import { gitComparison } from './git';

initSync({ module: readFileSync(new URL('../src/wasm/blockviz_core_bg.wasm', import.meta.url)) });

/** A project through the real analyzer, as the page does. */
function modelOf(p: Project): SceneModel {
  const r = analyze_project(new TextEncoder().encode(JSON.stringify({ name: p.name, description: '', files: p.files, shared_trunks: false })));
  const out = new Uint8Array(r.len);
  let at = 0;
  for (let x = r.next(); x; x = r.next()) { out.set(x, at); at += x.length; }
  const reply = JSON.parse(new TextDecoder().decode(out));
  if (!reply.ok) throw new Error(reply.error);
  return new SceneModel(reply.scene, p);
}

let dir: string;
let repo: string;
const git = (...args: string[]) => execFileSync('git', args, { cwd: repo, stdio: 'pipe' });

beforeAll(() => {
  dir = fs.mkdtempSync(path.join(os.tmpdir(), 'blockviz-e2e-'));
  repo = path.join(dir, 'shop');
  fs.mkdirSync(path.join(repo, 'app'), { recursive: true });
  git('init', '-q', '-b', 'main');
  git('config', 'user.name', 'T');
  git('config', 'user.email', 't@example.com');
  const file = path.join(repo, 'app', 'prices.py');
  fs.writeFileSync(file, 'def total(items):\n    return sum(i.price for i in items)\n\n\ndef tax(amount):\n    return amount * 0.2\n');
  fs.writeFileSync(path.join(repo, 'main.py'), 'from app.prices import total\n\n\ndef run(items):\n    return total(items)\n');
  git('add', '-A');
  git('commit', '-q', '-m', 'base');
  git('checkout', '-q', '-b', 'feature');
  fs.writeFileSync(file, 'def total(items):\n    return sum(i.price * i.count for i in items)\n\n\ndef tax(amount):\n    return amount * 0.2\n\n\ndef discount(amount):\n    return amount * 0.9\n');
  git('add', '-A');
  git('commit', '-q', '-m', 'feature');
});
afterAll(() => fs.rmSync(dir, { recursive: true, force: true }));

describe('a local comparison, analyzed and diffed', () => {
  it('colors what a branch changed', async () => {
    const c = await gitComparison(repo, { mode: 'branch', base: 'main', head: 'feature' });
    const s = new SceneDiff(modelOf(c.base), modelOf(c.head), c.info.files).summary;
    expect(s.changed).toBeGreaterThanOrEqual(1);
    expect(s.added).toBeGreaterThanOrEqual(1);
  });

  it('colors what the working tree changed', async () => {
    git('checkout', '-q', 'main');
    fs.writeFileSync(path.join(repo, 'app', 'prices.py'), 'def total(items):\n    return sum(i.price * i.count for i in items)\n\n\ndef tax(amount):\n    return amount * 0.2\n\n\ndef discount(amount):\n    return amount * 0.9\n');
    const c = await gitComparison(repo, { mode: 'worktree' });
    const s = new SceneDiff(modelOf(c.base), modelOf(c.head), c.info.files).summary;
    expect(s.changed).toBeGreaterThanOrEqual(1);
    expect(s.added).toBeGreaterThanOrEqual(1);
  });
});
