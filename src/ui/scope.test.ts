import { describe, expect, it } from 'vitest';
import { changesIn, filesIn, scopeName, type ScopeModel } from './scope';

type N = { kind: string; title: string; parent?: number; span?: { file: number } };
/** repo(0) › app/(1) › models.rb(2) › Producer(3) › save(4); lib/(5) › util.rb(6). */
const nodes: N[] = [
  { kind: 'repo', title: 'shop' },
  { kind: 'dir', title: 'app/', parent: 0 },
  { kind: 'file', title: 'models.rb', parent: 1, span: { file: 0 } },
  { kind: 'class', title: 'Producer', parent: 2, span: { file: 0 } },
  { kind: 'function', title: 'save', parent: 3, span: { file: 0 } },
  { kind: 'dir', title: 'lib/', parent: 0 },
  { kind: 'file', title: 'util.rb', parent: 5, span: { file: 1 } },
];
const model: ScopeModel = {
  root: 0,
  node: id => nodes[id],
  fileOf: n => (n.span ? ['app/models.rb', 'lib/util.rb'][(n.span as { file: number }).file] : undefined),
  canonicalPath: id => { const out = [id]; for (let p = nodes[id].parent; p !== undefined; p = nodes[p].parent) out.unshift(p); return out; },
};
const changes = [
  { status: 'changed' as const, id: 4, at: 4 },
  { status: 'added' as const, id: 6, at: 6 },
  { status: 'removed' as const, id: 99, at: 3 },
  { status: 'removed' as const, id: 98, at: 5 },
];

describe('changes in what the view is zoomed into', () => {
  it('is all of them for the whole project', () => {
    expect(changesIn(model, changes, 0)).toEqual([0, 1, 2, 3]);
  });

  it('is those inside the block, removed ones by where they were', () => {
    expect(changesIn(model, changes, 1)).toEqual([0, 2]);
    expect(changesIn(model, changes, 3)).toEqual([0, 2]);
    expect(changesIn(model, changes, 4)).toEqual([0]);
    expect(changesIn(model, changes, 5)).toEqual([1, 3]);
  });

  it('is none where nothing changed', () => {
    expect(changesIn(model, changes, 2).length).toBe(2);
    expect(changesIn(model, [], 1)).toEqual([]);
  });
});

describe('files changed in what the view is zoomed into', () => {
  const files = [{ path: 'app/models.rb' }, { path: 'app/views/x.erb' }, { path: 'lib/util.rb' }, { path: 'README.md' }];

  it('are those under a folder', () => {
    expect(filesIn(model, files, 0)).toEqual(files);
    expect(filesIn(model, files, 1).map(f => f.path)).toEqual(['app/models.rb', 'app/views/x.erb']);
    expect(filesIn(model, files, 5).map(f => f.path)).toEqual(['lib/util.rb']);
  });

  it('is the file, for a block in it', () => {
    expect(filesIn(model, files, 3).map(f => f.path)).toEqual(['app/models.rb']);
    expect(filesIn(model, files, 6).map(f => f.path)).toEqual(['lib/util.rb']);
  });

  it('counts a rename by either path', () => {
    expect(filesIn(model, [{ path: 'lib/new.rb', previous: 'app/old.rb' }], 1)).toHaveLength(1);
  });
});

describe('naming the scope', () => {
  it('is the path of blocks below the repository', () => {
    expect(scopeName(model, 0)).toBe('');
    expect(scopeName(model, 3)).toBe('app/ › models.rb › Producer');
  });
});
