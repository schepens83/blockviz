import { ArrowLeftRight, ArrowUpDown, CircleAlert, FileCode, FolderOpen, GitBranch, Link2, Sparkles, X } from 'lucide-react';
import { canPickDirectory, gitBranches, gitLog, gitOrder, gitServer, guessFilename, pickRepo, type DirectoryHandle } from '../core/loaders';
import { useEffect, useRef, useState } from 'react';
import type { Commit, CommitOrder, GitMode } from '../core/types';
import { RefPicker } from './RefPicker';
import { defaultChoice } from './refs';
import { TOUCH, useMedia } from './media';

export type OpenRequest =
  | { kind: 'example' }
  | { kind: 'github'; url: string }
  | { kind: 'git'; repo: string; mode: GitMode; base: string; head: string }
  | { kind: 'folder'; files: FileList }
  | { kind: 'directory'; handle: DirectoryHandle }
  | { kind: 'paste'; filename: string; text: string };

export function OpenDialog({ onOpen, onClose, canClose, lastUrl, recent }: { onOpen: (r: OpenRequest) => void; onClose: () => void; canClose: boolean; lastUrl: string; recent: string[] }) {
  const [url, setUrl] = useState(lastUrl);
  // The repository chosen, and its branches once the dev server has listed them.
  const [repo, setRepo] = useState('');
  const [branches, setBranches] = useState<Awaited<ReturnType<typeof gitBranches>> | null>(null);
  const [mode, setMode] = useState<GitMode>('branch');
  const [base, setBase] = useState('');
  const [head, setHead] = useState('');
  const [commits, setCommits] = useState<Commit[]>([]);
  // Which of base and head is older, once both are chosen (undefined while it is asked, null if it cannot be).
  const [order, setOrder] = useState<CommitOrder | null | undefined>();
  const [orderError, setOrderError] = useState('');
  const [gitError, setGitError] = useState('');
  // Whether a server that runs git (and opens its dialog) is there; the folder dialog is open; a path is being typed.
  const [server, setServer] = useState(true);
  const [picking, setPicking] = useState(false);
  const [typing, setTyping] = useState(false);
  const [typed, setTyped] = useState('');
  const [paste, setPaste] = useState(false);
  const [text, setText] = useState('');
  const [filename, setFilename] = useState('main.py');
  // The file name follows the pasted code's language until it is edited.
  const [named, setNamed] = useState(false);
  const folder = useRef<HTMLInputElement>(null);
  const urlInput = useRef<HTMLInputElement>(null);
  const touch = useMedia(TOUCH);
  // Chrome and Edge open a folder without listing all of it first (and
  // without asking to "upload" thousands of files); elsewhere, the file input.
  const pickFolder = async () => {
    if (!canPickDirectory() || touch) return folder.current?.click();
    try {
      const handle = await (window as unknown as { showDirectoryPicker(o?: { mode: 'read' }): Promise<DirectoryHandle> }).showDirectoryPicker({ mode: 'read' });
      onOpen({ kind: 'directory', handle });
    } catch (e) {
      // Cancelled, or not allowed here (an iframe): the file input still works.
      if ((e as Error).name !== 'AbortError') folder.current?.click();
    }
  };
  /** What a mode compares to begin with. */
  const defaults = (m: GitMode, b: NonNullable<typeof branches>, log: Commit[]) => {
    const d = defaultChoice(m, b, log);
    setBase(d.base);
    setHead(d.head);
  };
  const chooseMode = (m: GitMode) => {
    setMode(m);
    if (branches) defaults(m, branches, commits);
  };
  const choose = async (path: string) => {
    setGitError('');
    try {
      const [b, log] = await Promise.all([gitBranches(path), gitLog(path, { limit: 100 }).catch(() => [])]);
      setRepo(path);
      setBranches(b);
      setCommits(log);
      defaults(mode, b, log);
    } catch (e) {
      setBranches(null);
      setGitError(e instanceof Error ? e.message : String(e));
    }
  };
  useEffect(() => {
    setOrder(undefined);
    setOrderError('');
    if (mode !== 'refs' || !branches || !base.trim() || !head.trim()) return;
    let live = true;
    const t = setTimeout(() => {
      gitOrder(repo, base.trim(), head.trim()).then(o => { if (live) setOrder(o); }, e => { if (live) { setOrder(null); setOrderError(e instanceof Error ? e.message : String(e)); } });
    }, 300);
    return () => { live = false; clearTimeout(t); };
  }, [repo, mode, branches, base, head]);
  const backwards = !!order && order.headIsAncestorOfBase && !order.baseIsAncestorOfHead;
  const same = !!order && order.baseSha === order.headSha;
  const swap = () => { setBase(head); setHead(base); };
  const pickGit = async () => {
    setPicking(true);
    setGitError('');
    try {
      const path = await pickRepo();
      if (path) await choose(path);
    } catch (e) {
      // No dialog on that machine, or the folder is no repository: the path can still be typed.
      setGitError(e instanceof Error ? e.message : String(e));
      setTyping(true);
    } finally {
      setPicking(false);
    }
  };
  useEffect(() => {
    folder.current?.setAttribute('webkitdirectory', '');
    void gitServer().then(setServer);
    // On a phone, focusing would cover the choices with the keyboard.
    if (!window.matchMedia(TOUCH).matches) urlInput.current?.focus();
  }, []);
  return (
    <div className="scrim" onMouseDown={e => { if (e.target === e.currentTarget && canClose) onClose(); }}>
      <section className="open-dialog" role="dialog" aria-modal="true" aria-labelledby="open-title">
        <header>
          <div>
            <h1 id="open-title">Open a codebase</h1>
            <p>Code is analyzed in your browser with Rust/WebAssembly. Nothing is uploaded or executed. A pull request (…/pull/123) or comparison (…/compare/main...dev) shows what it changes.</p>
          </div>
          {canClose && <button className="icon" aria-label="Close" onClick={onClose}><X size={16} /></button>}
        </header>
        <form className="open-row" onSubmit={e => { e.preventDefault(); if (url.trim()) onOpen({ kind: 'github', url }); }}>
          <Link2 size={17} />
          <input ref={urlInput} value={url} onChange={e => setUrl(e.target.value)} placeholder="github.com/owner/repo, or a pull request" aria-label="GitHub repository, pull request or comparison" spellCheck={false} />
          <button className="primary" type="submit" disabled={!url.trim()}>Open</button>
        </form>
        <div className="open-options">
          <button onClick={() => onOpen({ kind: 'example' })}>
            <Sparkles size={17} />
            <span><strong>nanoGPT example</strong><small>karpathy/nanoGPT, bundled</small></span>
          </button>
          <button onClick={() => void pickFolder()}>
            <FolderOpen size={17} />
            <span><strong>Local folder…</strong><small>{touch ? 'or files on this device' : 'or drop a folder anywhere'}</small></span>
          </button>
          <button onClick={() => setPaste(p => !p)} aria-expanded={paste}>
            <FileCode size={17} />
            <span><strong>Paste code</strong><small>a single file</small></span>
          </button>
        </div>
        <div className="git">
          {!server ? <p className="git-hint">Local git diffs need the dev server (npm run dev).</p> : !branches ? (
            <>
              <button type="button" className="git-choose" disabled={picking} onClick={() => void pickGit()}>
                <GitBranch size={17} />
                <span><strong>{picking ? 'Choose in the dialog…' : 'Choose repository…'}</strong><small>compare a local branch, two revisions or uncommitted changes</small></span>
              </button>
              {recent.length > 0 && (
                <div className="git-recent">
                  {recent.map(r => <button type="button" key={r} title={r} onClick={() => void choose(r)}>{r.split('/').filter(Boolean).pop() ?? r}</button>)}
                </div>
              )}
              {typing ? (
                <form className="open-row" onSubmit={e => { e.preventDefault(); if (typed.trim()) void choose(typed.trim()); }}>
                  <input value={typed} onChange={e => setTyped(e.target.value)} placeholder="/path/to/repo" aria-label="Local git repository" spellCheck={false} autoFocus />
                  <button type="submit" disabled={!typed.trim()}>Load branches</button>
                </form>
              ) : <button type="button" className="link" onClick={() => setTyping(true)}>or type a path</button>}
            </>
          ) : (
            <form onSubmit={e => { e.preventDefault(); onOpen({ kind: 'git', repo, mode, base, head }); }}>
              <div className="git-repo">
                <GitBranch size={15} />
                <strong>{repo.split('/').filter(Boolean).pop() ?? repo}</strong>
                <small title={repo}><bdi>{repo}</bdi></small>
                <button type="button" className="link" onClick={() => setBranches(null)}>change</button>
              </div>
              <div className="segmented" role="group" aria-label="What to compare">
                {([['branch', 'Branch vs base'], ['refs', 'Two refs'], ['worktree', 'Uncommitted']] as const).map(([m, name]) => (
                  <button type="button" key={m} aria-pressed={mode === m} onClick={() => chooseMode(m)} title={m === 'worktree' && branches.dirty ? 'Has uncommitted changes' : undefined}>
                    {name}{m === 'worktree' && branches.dirty && <i className="dot" aria-label="has changes" />}
                  </button>
                ))}
              </div>
              {mode === 'branch' && (
                <div className="git-refs">
                  <label><span>Before (base)</span><select value={base} onChange={e => setBase(e.target.value)} aria-label="Base branch">{branches.branches.map(b => <option key={b}>{b}</option>)}</select></label>
                  <span className="git-arrow">←</span>
                  <label><span>After (head)</span><select value={head} onChange={e => setHead(e.target.value)} aria-label="Branch to compare">{branches.branches.map(b => <option key={b}>{b}</option>)}</select></label>
                </div>
              )}
              {mode === 'refs' && (
                <div className="git-refs">
                  <label><span>Before (base)</span><RefPicker repo={repo} label="Before (base)" value={base} onChange={setBase} branches={branches.branches} tags={branches.tags} commits={commits} /></label>
                  <button type="button" className="icon git-swap" aria-label="Swap base and head" title="Swap base and head" onClick={swap}><ArrowLeftRight size={15} className="wide" /><ArrowUpDown size={15} className="narrow" /></button>
                  <label><span>After (head)</span><RefPicker repo={repo} label="After (head)" value={head} onChange={setHead} branches={branches.branches} tags={branches.tags} commits={commits} /></label>
                </div>
              )}
              {mode === 'worktree' && <p className="git-note">The files on disk, untracked ones included and ignored ones not, against HEAD.</p>}
              <div className="git-actions">
                {mode === 'refs' && backwards ? <p className="git-notice" role="status"><CircleAlert size={14} /><span>After is older than Before — the diff will run backwards.</span><button type="button" className="link" onClick={swap}>Swap</button></p>
                  : mode === 'refs' && same ? <p className="git-notice" role="status"><CircleAlert size={14} /><span>Before and After are the same commit.</span></p>
                  : mode === 'refs' && orderError ? <p className="git-notice error" role="alert"><CircleAlert size={14} /><span>{orderError}</span></p> : <span />}
                <button className="primary" type="submit" disabled={mode === 'worktree' ? false : !base.trim() || !head.trim() || (mode === 'branch' ? base === head : same || !!orderError)}>Compare</button>
              </div>
            </form>
          )}
          {gitError && <p className="git-error" role="alert">{gitError}</p>}
        </div>
        {paste && (
          <div className="paste">
            <input value={filename} onChange={e => { setFilename(e.target.value); setNamed(true); }} aria-label="Filename" spellCheck={false} />
            <textarea value={text} onChange={e => { setText(e.target.value); if (!named) setFilename(guessFilename(e.target.value)); }} placeholder="Paste Python, JavaScript, TypeScript, Rust, Go or Ruby here…" spellCheck={false} aria-label="Code" />
            <button className="primary" disabled={!text.trim()} onClick={() => onOpen({ kind: 'paste', filename, text })}>Visualize</button>
          </div>
        )}
        <input ref={folder} type="file" multiple hidden onChange={e => { if (e.target.files?.length) onOpen({ kind: 'folder', files: e.target.files }); e.target.value = ''; }} />
      </section>
    </div>
  );
}
