import { ChevronDown, GitBranch, GitCommitHorizontal, Tag } from 'lucide-react';
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { createPortal } from 'react-dom';
import { gitLog } from '../core/loaders';
import type { Commit } from '../core/types';
import { badges, type Badge } from './refs';

/** How long ago, briefly (`3d ago`). */
export function ago(date: string, now = Date.now()): string {
  const s = Math.max(0, (now - new Date(date).getTime()) / 1000);
  const [n, unit] = s < 3600 ? [s / 60, 'm'] : s < 86400 ? [s / 3600, 'h'] : s < 86400 * 60 ? [s / 86400, 'd'] : s < 86400 * 730 ? [s / (86400 * 30), 'mo'] : [s / (86400 * 365), 'y'];
  return n < 1 && unit === 'm' ? 'now' : `${Math.floor(n)}${unit} ago`;
}

type Row = { key: string; value: string; commit?: Commit; name?: Badge; use?: boolean };

/** Branches and tags shown before "show all". */
const FEW = 6;
const MENU_W = 360;
const MARGIN = 12;

/** Where the popover goes: below its trigger, or above when there is more room there, kept within the window. */
export function place(r: { left: number; right: number; top: number; bottom: number; width: number }, vw: number, vh: number): CSSProperties {
  const width = Math.min(Math.max(r.width, MENU_W), vw - 2 * MARGIN);
  const left = Math.max(MARGIN, Math.min(r.left, vw - MARGIN - width));
  const below = vh - r.bottom - MARGIN;
  const above = r.top - MARGIN;
  return below < 260 && above > below
    ? { left, width, bottom: vh - r.top + 4, maxHeight: above - 4 }
    : { left, width, top: r.bottom + 4, maxHeight: Math.max(below - 4, 160) };
}

const Badges = ({ refs, branches, tags }: { refs: string[]; branches: string[]; tags: string[] }) => {
  const { shown, more } = badges(refs, branches, tags);
  return <>
    {shown.map(b => <span className={`ref-badge ${b.kind}`} key={b.name} title={b.name}>{b.name}</span>)}
    {more.length > 0 && <span className="ref-badge" title={more.join('\n')}>+{more.length}</span>}
  </>;
};

/**
 * A revision picked from a list, not typed: the branches and tags, then
 * recent commits, narrowed by a search. Text that matches nothing but names
 * a revision git knows (`HEAD~3`, a full sha) can still be used. The list
 * floats over the dialog, so it is neither clipped nor makes it scroll.
 */
export function RefPicker({ repo, label, value, onChange, branches, tags, commits }: { repo: string; label: string; value: string; onChange: (value: string) => void; branches: string[]; tags: string[]; commits: Commit[] }) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState('');
  const [active, setActive] = useState(0);
  const [all, setAll] = useState(false);
  const [style, setStyle] = useState<CSSProperties>({});
  // What the server knows beyond the commits listed: matches of the search, the revision it names, and the commit chosen.
  const [more, setMore] = useState<{ found: Commit[]; named?: Commit }>({ found: [] });
  const [picked, setPicked] = useState<Commit>();
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const q = search.trim().toLowerCase();

  const listed = (r: string) => commits.find(c => c.refs.includes(r) || c.sha.startsWith(r) || c.short === r);
  const matches = (c: Commit) => !q || c.subject.toLowerCase().includes(q) || c.sha.startsWith(q) || c.refs.some(r => r.toLowerCase().includes(q));

  const { names, rows, hidden } = useMemo(() => {
    const kinds = [...branches.map(name => ({ name, kind: 'branch' as const })), ...tags.map(name => ({ name, kind: 'tag' as const }))].filter(n => !q || n.name.toLowerCase().includes(q));
    const capped = !q && !all && kinds.length > FEW;
    const names: Row[] = (capped ? kinds.slice(0, FEW) : kinds).map(n => ({ key: `${n.kind} ${n.name}`, value: n.name, name: n, commit: commits.find(c => c.refs.includes(n.name)) }));
    const have = new Set(commits.filter(matches).map(c => c.sha));
    const list: Row[] = [...commits.filter(matches), ...more.found.filter(c => !have.has(c.sha))].map(c => ({ key: c.sha, value: c.short, commit: c }));
    const use: Row[] = q && more.named && !names.length && !list.length ? [{ key: 'use', value: search.trim(), commit: more.named, use: true }] : [];
    return { names, hidden: capped ? kinds.length - FEW : 0, rows: [...names, ...list, ...use] };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [branches, tags, commits, q, more, search, all]);

  // Beyond the list: search the whole history, and ask whether the text names a revision.
  useEffect(() => {
    if (!open || q.length < 3) { setMore({ found: [] }); return; }
    let live = true;
    const t = setTimeout(async () => {
      const [found, named] = await Promise.all([gitLog(repo, { q: search.trim(), limit: 50 }).catch(() => []), gitLog(repo, { ref: search.trim(), limit: 1 }).then(c => c[0], () => undefined)]);
      if (live) setMore({ found, named });
    }, 250);
    return () => { live = false; clearTimeout(t); };
  }, [open, q, repo, search]);

  // The subject of the choice, from the list or from the server.
  useEffect(() => {
    setPicked(undefined);
    if (!value || listed(value)) return;
    let live = true;
    gitLog(repo, { ref: value, limit: 1 }).then(c => { if (live) setPicked(c[0]); }, () => {});
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [repo, value, commits]);

  // While open: follow the trigger, and close on a click anywhere else.
  useLayoutEffect(() => {
    if (!open) return;
    const reposition = () => { if (trigger.current) setStyle(place(trigger.current.getBoundingClientRect(), innerWidth, innerHeight)); };
    const away = (e: MouseEvent) => { if (!trigger.current?.contains(e.target as Node) && !menu.current?.contains(e.target as Node)) close(); };
    reposition();
    addEventListener('resize', reposition);
    addEventListener('scroll', reposition, true);
    document.addEventListener('mousedown', away);
    return () => {
      removeEventListener('resize', reposition);
      removeEventListener('scroll', reposition, true);
      document.removeEventListener('mousedown', away);
    };
  }, [open]);

  useEffect(() => setActive(0), [q, more]);
  useEffect(() => { menu.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' }); }, [active, open]);

  const choice = value ? listed(value) ?? picked : undefined;
  const KindIcon = branches.includes(value) ? GitBranch : tags.includes(value) ? Tag : GitCommitHorizontal;
  const close = () => { setOpen(false); setSearch(''); setAll(false); };
  const choose = (r: Row) => { onChange(r.value); close(); trigger.current?.focus(); };
  const key = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') { e.stopPropagation(); close(); trigger.current?.focus(); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); setActive(a => Math.min(a + 1, rows.length - 1)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setActive(a => Math.max(a - 1, 0)); }
    else if (e.key === 'Enter') { e.preventDefault(); if (rows[active]) choose(rows[active]); }
  };
  const row = (r: Row, i: number) => (
    <div key={r.key} role="option" aria-selected={i === active} className={`ref-row${r.name ? ' named' : ''}`} onMouseEnter={() => setActive(i)} onMouseDown={e => e.preventDefault()} onClick={() => choose(r)}
      title={r.commit && !r.name ? `${r.commit.subject}\n${r.commit.author} · ${new Date(r.commit.date).toLocaleString()}` : undefined}>
      {r.use ? <span className="ref-use">Use <code>{r.value}</code></span>
        : r.name ? <><span className={`ref-badge ${r.name.kind}`}>{r.name.name}</span><code className="ref-sha">{r.commit?.short}</code></>
        : <>
          <code className="ref-sha">{r.commit?.short}</code>
          <Badges refs={r.commit?.refs ?? []} branches={branches} tags={tags} />
          <span className="ref-subject">{r.commit?.subject}</span>
          {r.commit && <small>{ago(r.commit.date)}</small>}
        </>}
      {r.use && <span className="ref-subject">{r.commit?.subject}</span>}
    </div>
  );
  const nameCount = names.length;

  return (
    <div className="ref-picker">
      <button type="button" ref={trigger} className="ref-value" aria-haspopup="listbox" aria-expanded={open} aria-label={label} onClick={() => (open ? close() : setOpen(true))}>
        <KindIcon size={14} className="ref-kind" />
        <code>{value || 'Choose…'}</code>
        <span className="ref-subject">{choice?.subject ?? ''}</span>
        <ChevronDown size={14} className="ref-chevron" />
      </button>
      {open && createPortal(
        <div className="ref-menu" ref={menu} style={style}>
          <input autoFocus value={search} onChange={e => setSearch(e.target.value)} onKeyDown={key} placeholder="Search commits, branches, tags" aria-label="Search" spellCheck={false} />
          <div role="listbox" aria-label={label}>
            {nameCount > 0 && <div className="ref-group">Branches &amp; tags</div>}
            {names.map(row)}
            {hidden > 0 && <button type="button" className="link ref-all" onMouseDown={e => e.preventDefault()} onClick={() => setAll(true)}>Show all {nameCount + hidden}</button>}
            {rows.length > nameCount && <div className="ref-group">{rows[nameCount]?.use ? 'Revision' : 'Commits'}</div>}
            {rows.slice(nameCount).map((r, i) => row(r, nameCount + i))}
            {!rows.length && <p className="ref-none">Nothing matches.</p>}
          </div>
        </div>,
        document.body,
      )}
    </div>
  );
}
