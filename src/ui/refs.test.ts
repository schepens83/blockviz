import { describe, expect, it } from 'vitest';
import type { Commit } from '../core/types';
import { badges, defaultChoice } from './refs';

const branches = ['master', 'feature'];
const tags = ['v1'];

describe('badges for the refs on a commit', () => {
  it('prefers local branches, then tags, then HEAD, and shows two', () => {
    expect(badges(['HEAD', 'v1', 'feature', 'master'], branches, tags)).toEqual({
      shown: [{ name: 'feature', kind: 'branch' }, { name: 'master', kind: 'branch' }],
      more: ['v1', 'HEAD'],
    });
    expect(badges(['HEAD', 'v1'], branches, tags).shown.map(b => b.name)).toEqual(['v1', 'HEAD']);
  });

  it('hides origin/HEAD and a remote branch its local twin is on', () => {
    const b = badges(['HEAD', 'master', 'origin/master', 'origin/HEAD'], branches, tags);
    expect([...b.shown.map(x => x.name), ...b.more]).toEqual(['master', 'HEAD']);
  });

  it('keeps a remote branch that has no local twin', () => {
    expect(badges(['origin/topic'], branches, tags).shown).toEqual([{ name: 'origin/topic', kind: 'remote' }]);
  });
});

const commit = (sha: string, refs: string[], parents: string[]): Commit => ({ sha, short: sha.slice(0, 3), subject: sha, author: 'a', date: '2026-01-01T00:00:00Z', refs, parents });

describe('what two refs start as', () => {
  const log = [commit('ccc111', ['HEAD', 'master'], ['bbb222']), commit('ddd333', ['topic'], ['bbb222']), commit('bbb222', [], ['aaa000'])];

  it('is the current branch from its base, when they differ', () => {
    expect(defaultChoice('refs', { current: 'feature', branches, defaultBase: 'master' }, log)).toEqual({ base: 'master', head: 'feature' });
    expect(defaultChoice('branch', { current: 'feature', branches, defaultBase: 'master' }, log)).toEqual({ base: 'master', head: 'feature' });
  });

  it('is the commit before, on the base branch itself', () => {
    expect(defaultChoice('refs', { current: 'master', branches, defaultBase: 'master' }, log)).toEqual({ base: 'bbb', head: 'master' });
    // The parent is not in the list: git resolves HEAD~1.
    expect(defaultChoice('refs', { current: 'master', branches, defaultBase: 'master' }, [log[0]])).toEqual({ base: 'HEAD~1', head: 'master' });
  });

  it('compares the other branch on the base branch in branch mode, and nothing for the working tree', () => {
    expect(defaultChoice('branch', { current: 'master', branches, defaultBase: 'master' }, log)).toEqual({ base: 'master', head: 'feature' });
    expect(defaultChoice('worktree', { current: 'master', branches, defaultBase: 'master' }, log)).toEqual({ base: '', head: '' });
  });
});
