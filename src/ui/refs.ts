import type { Commit, GitMode } from '../core/types';

export type Badge = { name: string; kind: 'branch' | 'tag' | 'remote' | 'head' };

const ORDER = ['branch', 'tag', 'remote', 'head'];

/**
 * The refs on a commit worth a badge, local branches first, then tags,
 * remote branches and HEAD: the first two, and the names of the rest. A
 * remote's own `HEAD`, and a remote branch whose local twin is on the same
 * commit, only repeat what is there.
 */
export function badges(refs: string[], branches: string[], tags: string[]): { shown: Badge[]; more: string[] } {
  const local = new Set(refs.filter(r => branches.includes(r)));
  const all = refs
    .filter(r => !r.endsWith('/HEAD') && !(r.includes('/') && !branches.includes(r) && !tags.includes(r) && local.has(r.slice(r.indexOf('/') + 1))))
    .map((name): Badge => ({ name, kind: name === 'HEAD' ? 'head' : branches.includes(name) ? 'branch' : tags.includes(name) ? 'tag' : 'remote' }))
    .sort((a, b) => ORDER.indexOf(a.kind) - ORDER.indexOf(b.kind));
  return { shown: all.slice(0, 2), more: all.slice(2).map(b => b.name) };
}

/**
 * What a comparison starts as. A branch from its base; on the base itself
 * (nothing to compare), two refs start from the commit before, found in the
 * `commits` listed (else left to git as `HEAD~1`). The working tree needs no refs.
 */
export function defaultChoice(mode: GitMode, b: { current: string | null; branches: string[]; defaultBase: string | null }, commits: Commit[]): { base: string; head: string } {
  if (mode === 'worktree') return { base: '', head: '' };
  const from = b.defaultBase ?? b.branches[0] ?? '';
  const here = b.current ?? 'HEAD';
  if (mode === 'refs') {
    if (from && from !== here) return { base: from, head: here };
    const tip = commits.find(c => c.refs.includes('HEAD'));
    return { base: commits.find(c => c.sha === tip?.parents[0])?.short ?? 'HEAD~1', head: here };
  }
  return { base: from, head: b.current && b.current !== from ? b.current : b.branches.find(x => x !== from) ?? '' };
}
