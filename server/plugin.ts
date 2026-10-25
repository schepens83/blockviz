import type { IncomingMessage, ServerResponse } from 'node:http';
import os from 'node:os';
import path from 'node:path';
import type { Plugin } from 'vite';
import { gitBranches, gitComparison, gitLog, gitOrder } from './git.ts';
import { Busy, folderPicker } from './pick.ts';

/** A repository path as typed: absolute, or under the home folder (`~/code/app`). */
function repoPath(raw: string | null): string {
  const p = raw?.startsWith('~/') ? path.join(os.homedir(), raw.slice(2)) : raw;
  if (!p || !path.isAbsolute(p)) throw new Error('Enter the repository as an absolute path, like /home/you/code/app.');
  return p;
}

const send = (res: ServerResponse, status: number, body: unknown) => {
  res.statusCode = status;
  res.setHeader('Content-Type', 'application/json');
  res.end(JSON.stringify(body));
};

/** Answers `/__git/branches`, `/__git/log`, `/__git/order` and `/__git/compare` from the repositories on this machine. No CORS headers: only the page this server serves may ask. */
const picker = folderPicker();

async function handle(req: IncomingMessage, res: ServerResponse, next: () => void) {
  const url = new URL(req.url ?? '/', 'http://localhost');
  if (req.method !== 'GET' || !url.pathname.startsWith('/__git/')) return next();
  const q = (name: string) => url.searchParams.get(name);
  try {
    if (url.pathname === '/__git/pick') return send(res, 200, { repo: await picker.pick() });
    if (url.pathname === '/__git/log') return send(res, 200, await gitLog(repoPath(q('repo')), { ref: q('ref') || undefined, limit: Number(q('limit')) || undefined, q: q('q') ?? undefined }));
    if (url.pathname === '/__git/order') return send(res, 200, await gitOrder(repoPath(q('repo')), q('base') ?? '', q('head') ?? ''));
    if (url.pathname === '/__git/branches') return send(res, 200, await gitBranches(repoPath(q('repo'))));
    if (url.pathname === '/__git/compare') {
      const mode = q('mode') ?? 'branch';
      if (mode !== 'branch' && mode !== 'refs' && mode !== 'worktree') throw new Error(`Unknown mode ${mode}: use branch, refs or worktree.`);
      return send(res, 200, await gitComparison(repoPath(q('repo')), mode === 'worktree' ? { mode } : { mode, base: q('base') ?? '', head: q('head') ?? '' }));
    }
    next();
  } catch (e) {
    send(res, e instanceof Busy ? 409 : 400, { error: e instanceof Error ? e.message : String(e) });
  }
}

/** Local git diffs for the dev and preview servers: the browser can't run git. */
export function gitPlugin(): Plugin {
  return {
    name: 'blockviz-git',
    configureServer: s => void s.middlewares.use(handle),
    configurePreviewServer: s => void s.middlewares.use(handle),
  };
}
