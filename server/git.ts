import { execFile, spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import { firstFiles, ignoredPath, readable } from '../src/core/loaders.ts';
import type { CommitOrder, CompareInfo, Comparison, Commit, FileChange, GitMode, InputFile, LeftOut, Project } from '../src/core/types.ts';

const MAX_BUFFER = 1 << 30;
/** How much of a blob is looked at for a NUL byte, as git does, to tell binary from text. */
const SNIFF = 8000;

/** Runs `git` (no shell) in `repo`; its output is a string. */
function git(repo: string, args: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile('git', args, { cwd: repo, maxBuffer: MAX_BUFFER, encoding: 'utf8' }, (err, out) => (err ? reject(err) : resolve(out)));
  });
}

/** The top folder of the repository `dir` is in; fails clearly if there is none. */
export async function top(dir: string): Promise<string> {
  try {
    return (await git(dir, ['rev-parse', '--show-toplevel'])).trim();
  } catch {
    throw new Error(`${dir} is not a git repository`);
  }
}

/** The local branches, the checked-out one, the branch usually compared against (`main`, else `master`), the newest tags, and whether files have uncommitted changes. */
export async function gitBranches(repo: string): Promise<{ current: string | null; branches: string[]; defaultBase: string | null; tags: string[]; dirty: boolean }> {
  const root = await top(repo);
  const lines = async (...args: string[]) => (await git(root, args)).split('\n').filter(Boolean);
  const branches = await lines('for-each-ref', '--format=%(refname:short)', 'refs/heads');
  const tags = await lines('for-each-ref', '--format=%(refname:short)', '--sort=-creatordate', '--count=200', 'refs/tags');
  const current = (await git(root, ['symbolic-ref', '--short', '-q', 'HEAD']).catch(() => '')).trim() || null;
  const dirty = (await lines('status', '--porcelain')).length > 0;
  return { current, branches, defaultBase: ['main', 'master'].find(b => branches.includes(b)) ?? null, tags, dirty };
}

/** The commit a branch, tag or other revision names. One starting with `-` would be read by git as an option. */
async function commitOf(repo: string, ref: string, what: string): Promise<string> {
  const sha = ref.startsWith('-') ? '' : (await git(repo, ['rev-parse', '--verify', '--quiet', `${ref}^{commit}`]).catch(() => '')).trim();
  if (!sha) throw new Error(`No ${what} named ${ref} in ${repo}`);
  return sha;
}

interface Blob { path: string; size: number; sha?: string }

/** The files of a commit, as `git ls-tree` lists them: `<mode> <type> <sha> <size>\t<path>`, NUL-separated. */
async function listFiles(repo: string, sha: string): Promise<Blob[]> {
  const out: Blob[] = [];
  for (const line of (await git(repo, ['ls-tree', '-r', '-l', '-z', sha])).split('\0')) {
    const m = line.match(/^\d+ blob ([0-9a-f]+) +(\d+)\t(.*)$/s);
    if (m) out.push({ sha: m[1], size: Number(m[2]), path: m[3] });
  }
  return out;
}

/** The contents of blobs by sha, all through one `git cat-file --batch`: each answer is `<sha> blob <size>\n<content>\n`. */
async function readBlobs(repo: string, shas: string[]): Promise<Map<string, Buffer>> {
  if (!shas.length) return new Map();
  const chunks: Buffer[] = [];
  await new Promise<void>((resolve, reject) => {
    const p = spawn('git', ['cat-file', '--batch'], { cwd: repo, stdio: ['pipe', 'pipe', 'ignore'] });
    p.stdout.on('data', (c: Buffer) => chunks.push(c));
    p.on('error', reject);
    p.on('close', code => (code === 0 ? resolve() : reject(new Error('git cat-file failed'))));
    p.stdin.on('error', () => {});
    p.stdin.end(shas.join('\n') + '\n');
  });
  const all = Buffer.concat(chunks);
  const out = new Map<string, Buffer>();
  for (let at = 0; at < all.length;) {
    const eol = all.indexOf(10, at);
    const [sha, type, size] = all.toString('utf8', at, eol).split(' ');
    if (type !== 'blob') { at = eol + 1; continue; }
    out.set(sha, all.subarray(eol + 1, eol + 1 + Number(size)));
    at = eol + 1 + Number(size) + 1;
  }
  return out;
}

const LOG = '%H%x1f%h%x1f%s%x1f%an%x1f%aI%x1f%D%x1f%P';

/** Refs as `%D` decorates a commit (`HEAD -> main, tag: v1, origin/x`): `HEAD`, then branch and tag names. */
const refNames = (decoration: string): string[] =>
  decoration.split(', ').filter(Boolean).flatMap(d => (d.startsWith('HEAD -> ') ? ['HEAD', d.slice(8)] : [d.replace(/^tag: /, '')]));

/**
 * Commits to choose from: those of `ref`, else of HEAD and every branch,
 * newest first, at most `limit` (up to 500). `q` keeps those whose subject
 * has it, and adds the commit its text starts the sha of.
 */
export async function gitLog(repo: string, { ref, limit = 100, q }: { ref?: string; limit?: number; q?: string }): Promise<{ commits: Commit[] }> {
  const root = await top(repo);
  const n = Math.min(Math.max(Math.floor(limit) || 100, 1), 500);
  const head = (await git(root, ['rev-parse', '--verify', '--quiet', 'HEAD^{commit}']).catch(() => '')).trim();
  const from = ref ? [await commitOf(root, ref, 'revision')] : [...(head ? ['HEAD'] : []), '--branches'];
  const parse = (out: string) => out.split('\0').filter(Boolean).map((r): Commit => {
    const [sha, short, subject, author, date, refs, parents] = r.split('\x1f');
    return { sha, short, subject, author, date, refs: refNames(refs), parents: parents.split(' ').filter(Boolean) };
  });
  const text = q?.trim();
  const found = parse(await git(root, ['log', '-z', `--format=${LOG}`, `-n${n}`, ...(text ? [`--grep=${text}`, '--regexp-ignore-case', '--fixed-strings'] : []), ...from, '--']));
  if (text && /^[0-9a-f]{4,40}$/i.test(text)) {
    const sha = (await git(root, ['rev-parse', '--verify', '--quiet', `${text}^{commit}`]).catch(() => '')).trim();
    if (sha && !found.some(c => c.sha === sha)) found.unshift(...parse(await git(root, ['log', '-z', `--format=${LOG}`, '-n1', sha, '--'])));
  }
  return { commits: found };
}

/** Whether `a` is `b` or in its history: `git merge-base --is-ancestor` says by exiting 0 or 1. */
async function isAncestor(root: string, a: string, b: string): Promise<boolean> {
  try {
    await git(root, ['merge-base', '--is-ancestor', a, b]);
    return true;
  } catch (e) {
    if ((e as { code?: unknown }).code === 1) return false;
    throw e;
  }
}

/** Which of two revisions is the older one, so a comparison that would run backwards can be flagged. */
export async function gitOrder(repo: string, base: string, head: string): Promise<CommitOrder> {
  const root = await top(repo);
  const [baseSha, headSha] = [await commitOf(root, base, 'revision'), await commitOf(root, head, 'revision')];
  return { baseSha, headSha, baseIsAncestorOfHead: await isAncestor(root, baseSha, headSha), headIsAncestorOfBase: await isAncestor(root, headSha, baseSha) };
}

/** A file with its text, unless it is binary (a NUL byte early on) or was not read. */
function fileOf(path: string, bytes: number, buf?: Buffer): InputFile {
  return buf && !buf.subarray(0, SNIFF).includes(0) ? { path, text: buf.toString('utf8'), bytes } : { path, bytes };
}

/** The files a project's limits keep, which of them to read, and what is left out (as the local folder loaders limit them). */
type Plan = { kept: Blob[]; read: Set<Blob>; leftOut?: LeftOut };
function plan(blobs: Blob[]): Plan {
  const { first, more } = firstFiles(blobs);
  const { read, unread } = readable(first);
  return { kept: first, read, leftOut: more || unread ? { files: more, texts: unread } : undefined };
}

/** The files of the working tree: those tracked and not deleted, and those untracked and not ignored (as `git status` shows). Symlinks and submodules are left out. */
async function diskFiles(root: string): Promise<{ blobs: Blob[]; untracked: Set<string> }> {
  const list = async (...args: string[]) => (await git(root, ['ls-files', '-z', ...args])).split('\0').filter(Boolean);
  const untracked = new Set(await list('--others', '--exclude-standard'));
  const blobs: Blob[] = [];
  await Promise.all([...new Set([...await list('--cached'), ...untracked])].map(async file => {
    const st = await fs.lstat(path.join(root, file)).catch(() => null);
    if (st?.isFile()) blobs.push({ path: file, size: st.size });
  }));
  return { blobs, untracked };
}

/** `git diff --name-status -z -M`: `A`, `D`, `M`/`T`, and `R<score>` with the old path before the new. */
async function changes(repo: string, args: string[]): Promise<FileChange[]> {
  const t = (await git(repo, ['diff', '--name-status', '-z', '-M', ...args])).split('\0');
  const out: FileChange[] = [];
  for (let i = 0; i + 1 < t.length;) {
    const status = t[i][0];
    const [a, b] = status === 'R' || status === 'C' ? [t[i + 1], t[i + 2]] : [t[i + 1], undefined];
    i += status === 'R' || status === 'C' ? 3 : 2;
    const file = b ?? a;
    if (ignoredPath(file)) continue;
    if (status === 'A' || status === 'C') out.push({ path: file, status: 'added' });
    else if (status === 'D') out.push({ path: file, status: 'removed' });
    else if (status === 'R') out.push({ path: file, status: 'renamed', previous: a });
    else out.push({ path: file, status: 'modified' });
  }
  return out;
}

const byPath = (files: FileChange[]) => files.sort((x, y) => x.path.localeCompare(y.path));

/** What to compare: a branch with where it branched off a base (as on GitHub), two exact revisions, or the files on disk with HEAD. */
export type GitCompare = { mode: Exclude<GitMode, 'worktree'>; base: string; head: string } | { mode: 'worktree' };

/**
 * Two versions of the repository at `repo` and which files differ. Paths
 * start with the repository's folder name, as in a local folder.
 */
export async function gitComparison(repo: string, how: GitCompare): Promise<Comparison> {
  const root = await top(repo);
  const name = path.basename(root);
  const none = { kept: [], read: new Set<Blob>() };
  let baseSha = '';
  let headSha = '';
  let before: Plan = none;
  let after: Plan;
  let files: FileChange[];
  let info: Pick<CompareInfo, 'mode' | 'title' | 'baseRef' | 'headRef'>;
  let disk: Awaited<ReturnType<typeof diskFiles>> | undefined;
  if (how.mode === 'worktree') {
    info = { mode: 'worktree', title: 'working tree vs HEAD', baseRef: 'HEAD', headRef: 'working tree' };
    baseSha = (await git(root, ['rev-parse', '--verify', '--quiet', 'HEAD^{commit}']).catch(() => '')).trim();
    disk = await diskFiles(root);
    after = plan(disk.blobs);
    if (baseSha) {
      before = plan(await listFiles(root, baseSha));
      const untracked = [...disk.untracked].map((f): FileChange => ({ path: f, status: 'added' })).filter(c => !ignoredPath(c.path));
      files = byPath([...await changes(root, ['HEAD']), ...untracked]);
    } else {
      // No commit yet: everything is new.
      files = byPath(after.kept.map((b): FileChange => ({ path: b.path, status: 'added' })));
    }
  } else {
    const what = how.mode === 'refs' ? 'revision' : 'branch';
    const [baseTip, tip] = [await commitOf(root, how.base, what), await commitOf(root, how.head, what)];
    headSha = tip;
    if (how.mode === 'refs') baseSha = baseTip;
    else {
      baseSha = (await git(root, ['merge-base', baseTip, tip]).catch(() => '')).trim();
      if (!baseSha) throw new Error(`${how.base} and ${how.head} have no common history in ${repo}`);
    }
    info = { mode: how.mode, title: `${how.base}${how.mode === 'refs' ? '..' : '...'}${how.head}`, baseRef: how.base, headRef: how.head };
    [before, after] = [plan(await listFiles(root, baseSha)), plan(await listFiles(root, tip))];
    files = byPath(await changes(root, [baseSha, tip]));
  }
  // A file the two versions share is read once.
  const blobs = await readBlobs(root, [...new Set([...before.read, ...(disk ? [] : after.read)].map(b => b.sha!))]);
  const project = async ({ kept, read, leftOut }: Plan, text: (b: Blob) => Promise<Buffer | undefined>): Promise<Project> => ({
    name,
    source: 'local git',
    leftOut,
    files: await Promise.all(kept.map(async b => fileOf(b.path, b.size, read.has(b) ? await text(b) : undefined))),
  });
  const fromGit = async (b: Blob) => blobs.get(b.sha!);
  const fromDisk = (b: Blob) => fs.readFile(path.join(root, b.path)).catch(() => undefined);
  return {
    base: await project(before, fromGit),
    head: await project(after, disk ? fromDisk : fromGit),
    info: { kind: 'local', repo, url: '', baseSha, headSha, files, ...info },
  };
}
