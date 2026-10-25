import type { Change } from '../canvas/diff';

type ScopeNode = { kind: string; title: string; parent?: number; span?: unknown };

/** What scoping needs of a diagram. */
export interface ScopeModel {
  root: number;
  node(id: number): ScopeNode;
  fileOf(n: ScopeNode): string | undefined;
  canonicalPath(id: number): number[];
}

/** Whether block `id` is `scope` or inside it. */
function inside(model: ScopeModel, id: number, scope: number): boolean {
  for (let c: number | undefined = id; c !== undefined; c = model.node(c).parent) if (c === scope) return true;
  return false;
}

/**
 * Which of the changes (as their indexes) are in the block the view is zoomed
 * into, the repository being all. A removed block is where it was: in the
 * head block it was in.
 */
export function changesIn(model: ScopeModel, changes: Change[], scope: number): number[] {
  const out: number[] = [];
  changes.forEach((c, i) => { if (scope === model.root || inside(model, c.status === 'removed' ? c.at : c.id, scope)) out.push(i); });
  return out;
}

/** Which files (by path, or the path before a rename) are in the folder or file the view is zoomed into. */
export function filesIn<F extends { path: string; previous?: string }>(model: ScopeModel, files: F[], scope: number): F[] {
  for (let c: number | undefined = scope; c !== undefined; c = model.node(c).parent) {
    const n = model.node(c);
    if (n.kind === 'repo') return files;
    const file = model.fileOf(n);
    if (file !== undefined) return files.filter(f => f.path === file || f.previous === file);
    if (n.kind === 'dir') {
      // Folders are titled by their own name (`app/`), joined down from the repository.
      const dir = model.canonicalPath(c).map(id => model.node(id)).filter(x => x.kind === 'dir').map(x => x.title).join('');
      return files.filter(f => f.path.startsWith(dir) || f.previous?.startsWith(dir));
    }
  }
  return files;
}

/** Where a scope is, as breadcrumbs name it, below the repository (empty for the repository). */
export function scopeName(model: ScopeModel, scope: number): string {
  return model.canonicalPath(scope).slice(1).map(id => model.node(id).title).filter(Boolean).join(' › ');
}
