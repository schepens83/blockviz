import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { gitBranches, gitComparison, gitLog, gitOrder } from './git';

let dir: string;
let repo: string;
const gitIn = (cwd: string, ...args: string[]) => execFileSync('git', args, { cwd, stdio: 'pipe' });
const git = (...args: string[]) => gitIn(repo, ...args);
const write = (name: string, text: string) => {
  fs.mkdirSync(path.dirname(path.join(repo, name)), { recursive: true });
  fs.writeFileSync(path.join(repo, name), text);
};
const commit = (message: string) => { git('add', '-A'); git('commit', '-q', '-m', message); };

const body = Array.from({ length: 20 }, (_, i) => `def step_${i}(x):\n    return x + ${i}\n`).join('\n');

beforeAll(() => {
  dir = fs.mkdtempSync(path.join(os.tmpdir(), 'blockviz-git-'));
  repo = path.join(dir, 'proj');
  fs.mkdirSync(repo);
  git('init', '-q', '-b', 'main');
  git('config', 'user.name', 'T');
  git('config', 'user.email', 't@example.com');
  git('config', 'commit.gpgsign', 'false');
  write('app.py', 'def run():\n    return 1\n');
  write('gone.py', 'def gone():\n    return 0\n');
  write('old_name.py', body);
  write('same.py', 'X = 1\n');
  write('logo.bin', 'a\0b');
  write('node_modules/dep/index.js', 'x');
  commit('base');
  git('tag', 'v1');
  git('checkout', '-q', '-b', 'feature');
  write('app.py', 'def run():\n    return 2\n');
  write('new.py', 'def new():\n    pass\n');
  git('rm', '-q', 'gone.py');
  git('mv', 'old_name.py', 'new_name.py');
  commit('feature');
  git('checkout', '-q', 'main');
  write('main_only.py', 'Y = 2\n');
  commit('main moves on');
});

afterAll(() => fs.rmSync(dir, { recursive: true, force: true }));

describe('gitBranches', () => {
  it('lists local branches, the current one and the default base', async () => {
    const b = await gitBranches(repo);
    expect(b.branches.sort()).toEqual(['feature', 'main']);
    expect(b.current).toBe('main');
    expect(b.defaultBase).toBe('main');
    expect(b.tags).toEqual(['v1']);
    expect(b.dirty).toBe(false);
  });
});

describe('gitComparison', () => {
  it('compares the head with where it branched off the base', async () => {
    const c = await gitComparison(repo, { mode: 'branch', base: 'main', head: 'feature' });
    const paths = (files: { path: string }[]) => files.map(f => f.path).sort();
    expect(paths(c.base.files)).toEqual(['app.py', 'gone.py', 'logo.bin', 'old_name.py', 'same.py']);
    expect(paths(c.head.files)).toEqual(['app.py', 'logo.bin', 'new.py', 'new_name.py', 'same.py']);
    expect(c.base.name).toBe('proj');
    expect(c.head.source).toBe('local git');
    expect(c.head.files.find(f => f.path === 'app.py')!.text).toBe('def run():\n    return 2\n');
    expect(c.base.files.find(f => f.path === 'app.py')!.text).toBe('def run():\n    return 1\n');
    expect(c.head.files.find(f => f.path === 'same.py')).toMatchObject({ text: 'X = 1\n', bytes: 6 });
    // A binary file keeps its size but not its text.
    expect(c.head.files.find(f => f.path === 'logo.bin')).toEqual({ path: 'logo.bin', bytes: 3 });
    expect(c.info.files).toEqual([
      { path: 'app.py', status: 'modified' },
      { path: 'gone.py', status: 'removed' },
      { path: 'new_name.py', status: 'renamed', previous: 'old_name.py' },
      { path: 'new.py', status: 'added' },
    ]);
    expect(c.info).toMatchObject({ kind: 'local', repo, title: 'main...feature', url: '', baseRef: 'main', headRef: 'feature' });
    expect(c.info.baseSha).toBe(git('rev-parse', 'feature~1').toString().trim());
    expect(c.info.headSha).toBe(git('rev-parse', 'feature').toString().trim());
  });

  it('rejects unknown branches, option-like refs and folders that are no repository', async () => {
    await expect(gitComparison(repo, { mode: 'branch', base: 'main', head: 'nope' })).rejects.toThrow(`No branch named nope in ${repo}`);
    await expect(gitComparison(repo, { mode: 'branch', base: '--output=x', head: 'feature' })).rejects.toThrow(/No branch named --output=x/);
    expect(fs.existsSync(path.join(repo, 'x'))).toBe(false);
    await expect(gitBranches(dir)).rejects.toThrow(`${dir} is not a git repository`);
    await expect(gitComparison(path.join(dir, 'missing'), { mode: 'branch', base: 'a', head: 'b' })).rejects.toThrow(/is not a git repository/);
  });
});

describe('two refs', () => {
  const paths = (files: { path: string }[]) => files.map(f => f.path).sort();

  it('compares exactly the two commits, so what the base gained shows', async () => {
    const c = await gitComparison(repo, { mode: 'refs', base: 'feature', head: 'main' });
    expect(c.info).toMatchObject({ kind: 'local', mode: 'refs', title: 'feature..main', baseRef: 'feature', headRef: 'main' });
    expect(c.info.baseSha).toBe(git('rev-parse', 'feature').toString().trim());
    expect(paths(c.head.files)).toContain('main_only.py');
    expect(c.info.files).toContainEqual({ path: 'main_only.py', status: 'added' });
    expect(c.info.files).toContainEqual({ path: 'gone.py', status: 'added' });
  });

  it('takes tags and relative revisions', async () => {
    const c = await gitComparison(repo, { mode: 'refs', base: 'v1', head: 'HEAD~1' });
    expect(c.info.files).toEqual([]);
    expect(c.info.baseSha).toBe(git('rev-parse', 'v1').toString().trim());
  });

  it('rejects what git would read as an option', async () => {
    await expect(gitComparison(repo, { mode: 'refs', base: 'v1', head: '--output=x' })).rejects.toThrow(/No revision named --output=x/);
    await expect(gitComparison(repo, { mode: 'refs', base: 'nope', head: 'main' })).rejects.toThrow(`No revision named nope in ${repo}`);
  });
});

describe('the working tree', () => {
  let wt: string;
  beforeAll(() => {
    wt = path.join(dir, 'wt');
    fs.mkdirSync(wt);
    const g = (...a: string[]) => gitIn(wt, ...a);
    const put = (n: string, t: string) => fs.writeFileSync(path.join(wt, n), t);
    g('init', '-q', '-b', 'main');
    g('config', 'user.name', 'T');
    g('config', 'user.email', 't@example.com');
    put('.gitignore', '*.log\n');
    put('keep.py', 'K = 1\n');
    put('edit.py', 'E = 1\n');
    put('drop.py', 'D = 1\n');
    g('add', '-A');
    g('commit', '-q', '-m', 'one');
    put('edit.py', 'E = 2\n');
    put('staged.py', 'S = 1\n');
    g('add', 'staged.py');
    put('loose.py', 'L = 1\n');
    put('debug.log', 'noise');
    fs.rmSync(path.join(wt, 'drop.py'));
  });

  it('lists branches and tags, and whether there is uncommitted work', async () => {
    expect(await gitBranches(wt)).toMatchObject({ branches: ['main'], tags: [], dirty: true });
  });

  it('compares the files on disk with HEAD', async () => {
    const c = await gitComparison(wt, { mode: 'worktree' });
    expect(c.info).toMatchObject({ kind: 'local', mode: 'worktree', baseRef: 'HEAD', headRef: 'working tree', title: 'working tree vs HEAD', headSha: '' });
    expect(c.info.baseSha).toBe(gitIn(wt, 'rev-parse', 'HEAD').toString().trim());
    expect(c.head.files.map(f => f.path).sort()).toEqual(['.gitignore', 'edit.py', 'keep.py', 'loose.py', 'staged.py']);
    expect(c.base.files.map(f => f.path).sort()).toEqual(['.gitignore', 'drop.py', 'edit.py', 'keep.py']);
    expect(c.head.files.find(f => f.path === 'edit.py')!.text).toBe('E = 2\n');
    expect(c.head.files.find(f => f.path === 'keep.py')).toEqual(c.base.files.find(f => f.path === 'keep.py'));
    expect(c.info.files).toEqual([
      { path: 'drop.py', status: 'removed' },
      { path: 'edit.py', status: 'modified' },
      { path: 'loose.py', status: 'added' },
      { path: 'staged.py', status: 'added' },
    ]);
  });

  it('reads a repository without commits as all added', async () => {
    const fresh = path.join(dir, 'fresh');
    fs.mkdirSync(fresh);
    gitIn(fresh, 'init', '-q', '-b', 'main');
    fs.writeFileSync(path.join(fresh, 'a.py'), 'A = 1\n');
    const c = await gitComparison(fresh, { mode: 'worktree' });
    expect(c.base.files).toEqual([]);
    expect(c.info.files).toEqual([{ path: 'a.py', status: 'added' }]);
  });

  it('works from a folder inside the repository, named for the repository', async () => {
    fs.mkdirSync(path.join(wt, 'sub'));
    const c = await gitComparison(path.join(wt, 'sub'), { mode: 'worktree' });
    expect(c.head.name).toBe('wt');
    fs.rmdirSync(path.join(wt, 'sub'));
  });
});

describe('the log', () => {
  it('lists the commits of every branch, newest first, with the refs on them', async () => {
    const { commits } = await gitLog(repo, {});
    expect(commits.map(c => c.subject).sort()).toEqual(['base', 'feature', 'main moves on']);
    const tip = commits.find(c => c.subject === 'main moves on')!;
    expect(tip).toMatchObject({ sha: git('rev-parse', 'main').toString().trim(), author: 'T' });
    expect(tip.short).toBe(tip.sha.slice(0, 7));
    expect(new Date(tip.date).getTime()).not.toBeNaN();
    expect(tip.refs).toEqual(['HEAD', 'main']);
    expect(tip.parents).toEqual([commits.find(c => c.subject === 'base')!.sha]);
    expect(commits.find(c => c.subject === 'base')!.refs).toEqual(['v1']);
    expect(commits.find(c => c.subject === 'feature')!.refs).toEqual(['feature']);
  });

  it('follows one ref, within a limit', async () => {
    expect((await gitLog(repo, { ref: 'feature' })).commits.map(c => c.subject)).toEqual(['feature', 'base']);
    expect((await gitLog(repo, { limit: 1 })).commits).toHaveLength(1);
    expect((await gitLog(repo, { ref: 'HEAD~1' })).commits.map(c => c.subject)).toEqual(['base']);
  });

  it('searches subjects, and finds a commit by the start of its sha', async () => {
    expect((await gitLog(repo, { q: 'MOVES' })).commits.map(c => c.subject)).toEqual(['main moves on']);
    const sha = git('rev-parse', 'feature').toString().trim();
    expect((await gitLog(repo, { q: sha.slice(0, 6) })).commits.map(c => c.subject)).toEqual(['feature']);
    expect((await gitLog(repo, { q: 'nothing like it' })).commits).toEqual([]);
  });

  it('rejects an unknown ref or one git would read as an option', async () => {
    await expect(gitLog(repo, { ref: '--output=x' })).rejects.toThrow(/No revision named --output=x/);
    await expect(gitLog(repo, { ref: 'nope' })).rejects.toThrow(/No revision named nope/);
  });

  it('is empty for a repository without commits', async () => {
    const fresh = path.join(dir, 'empty');
    fs.mkdirSync(fresh);
    gitIn(fresh, 'init', '-q', '-b', 'main');
    expect((await gitLog(fresh, {})).commits).toEqual([]);
  });
});

describe('the order of two revisions', () => {
  it('says which is the ancestor of the other', async () => {
    const sha = (r: string) => git('rev-parse', r).toString().trim();
    expect(await gitOrder(repo, 'v1', 'main')).toEqual({ baseSha: sha('v1'), headSha: sha('main'), baseIsAncestorOfHead: true, headIsAncestorOfBase: false });
    expect(await gitOrder(repo, 'main', 'v1')).toMatchObject({ baseIsAncestorOfHead: false, headIsAncestorOfBase: true });
    expect(await gitOrder(repo, 'feature', 'main')).toMatchObject({ baseIsAncestorOfHead: false, headIsAncestorOfBase: false });
    expect(await gitOrder(repo, 'main', 'main')).toMatchObject({ baseIsAncestorOfHead: true, headIsAncestorOfBase: true });
    await expect(gitOrder(repo, '-x', 'main')).rejects.toThrow(/No revision named -x/);
  });
});
