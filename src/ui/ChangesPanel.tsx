import { ChevronLeft, ChevronRight, Crosshair, Download, ExternalLink, X } from 'lucide-react';
import { useEffect, useMemo, useRef, type CSSProperties } from 'react';
import type { Change, SceneDiff, Status } from '../canvas/diff';
import type { SceneModel } from '../canvas/model';
import type { Highlights } from '../core/highlights';
import type { CompareInfo, SceneNode } from '../core/types';
import { DiffView } from './DiffView';
import { TOUCH, useMedia } from './media';
import { KIND_LABEL, ResizeHandle, type PanelSize } from './panel';
import { filesIn, scopeName } from './scope';

/** A block's name as a list shows it: its title, or for an untitled branch what its first arm tests. */
export function blockName(model: SceneModel, n: SceneNode): string {
  if (n.title) return n.title;
  const arm = (n.children ?? []).map(c => model.node(c)).find(c => c.kind === 'lane' && c.title);
  return arm?.title ?? KIND_LABEL[n.kind] ?? n.kind;
}

/** The file a block's code is in. */
function fileOf(model: SceneModel, n: SceneNode): string | undefined {
  for (let c: SceneNode | undefined = n; c; c = c.parent === undefined ? undefined : model.node(c.parent)) if (c.span) return model.fileOf(c);
  return undefined;
}

const OUTER = new Set(['repo', 'dir', 'file', 'notebook', 'doc', 'asset', 'group']);

/** How a change is listed: its block's name, the definition it is in, its kind, and where its code is. */
export function describeChange(diff: SceneDiff, c: Change) {
  const model = c.status === 'removed' ? diff.base : diff.head;
  const n = model.node(c.id);
  // What it is in, unless that is just the repository.
  const w = OUTER.has(n.kind) ? null : model.where(c.id);
  const where = w && w.at !== model.root ? w : null;
  // A whole file starts at its first line: no line to give.
  return { name: blockName(model, n), context: where && where.name !== n.title ? where.name : null, kind: KIND_LABEL[n.kind] ?? n.kind, file: fileOf(model, n), line: OUTER.has(n.kind) ? undefined : n.span?.line };
}

const MARK: Record<Status, string> = { added: '+', changed: '~', removed: '−' };
const FILE_MARK: Record<string, string> = { added: 'A', modified: 'M', removed: 'D', renamed: 'R' };

/** Steps through the changes: which one this is, and buttons for the previous and next. */
export function ChangeSteps({ index, total, onStep }: { index: number; total: number; onStep: (delta: number) => void }) {
  if (!total) return null;
  return (
    <div className="change-steps">
      <span>{index >= 0 ? <>Change <strong>{index + 1}</strong> of {total}</> : <>{total} change{total === 1 ? '' : 's'} in all</>}</span>
      <button className="icon" aria-label="Previous change (P)" title="Previous change (P)" onClick={() => onStep(-1)}><ChevronLeft size={15} /></button>
      <button className="icon" aria-label="Next change (N)" title="Next change (N)" onClick={() => onStep(1)}><ChevronRight size={15} /></button>
    </div>
  );
}

/** A change's code, as the details panel would show it: the lines it removed and added. */
function ChangeCode({ diff, change, highlights, baseHighlights }: { diff: SceneDiff; change: Change; highlights: Highlights; baseHighlights: Highlights }) {
  if (change.status === 'removed') {
    const span = diff.base.node(change.id).span;
    return span && diff.base.fileBytes(span.file) ? <DiffView base={{ model: diff.base, highlights: baseHighlights, span }} head={null} /> : <p className="insp-doc">No code to show.</p>;
  }
  const model = diff.head;
  const node = model.node(change.id);
  const view = model.sourceView(node);
  const root = view?.root.span ?? view?.span;
  if (!view || !root || !model.fileBytes(root.file)) return <p className="insp-doc">No code to show.</p>;
  const baseRoot = diff.baseOf(view.root.id);
  const baseSpan = baseRoot !== undefined ? diff.base.node(baseRoot).span : undefined;
  return (
    <DiffView
      head={{ model, highlights, span: root }}
      base={baseSpan && diff.base.fileBytes(baseSpan.file) ? { model: diff.base, highlights: baseHighlights, span: baseSpan } : null}
      marked={view.root !== node && node.span ? [node.span.line, node.span.endLine] : null}
    />
  );
}

/**
 * What a pull request (or comparison) changed, in the order the diagram
 * reads: grouped by file, each change a row. Only what is inside the block
 * the view is zoomed into is listed (unless `scoped` is off). Clicking a row
 * marks its block in the diagram and unfolds its code diff here, without
 * moving the camera; double-clicking, or its crosshair, zooms to it. Files
 * changed without changing any block (binaries, whitespace, files not
 * analyzed) are listed after.
 */
export function ChangesPanel({ diff, info, size, current, marked, visible, scope, scoped, highlights, baseHighlights, onResize, onClose, onGo, onMark, onScoped, onPeek }: {
  diff: SceneDiff;
  info: CompareInfo;
  size: PanelSize;
  /** The change being read, as its index in `diff.changes` (-1: none). */
  current: number;
  /** The change unfolded and marked in the diagram (-1: none). */
  marked: number;
  /** The changes listed, by index, in the order read. */
  visible: number[];
  /** The block the view is zoomed into. */
  scope: number;
  scoped: boolean;
  highlights: Highlights;
  baseHighlights: Highlights;
  onResize: (size: PanelSize) => void;
  onClose: () => void;
  /** Zooms to a change. */
  onGo: (index: number) => void;
  /** Marks a change (-1: none). */
  onMark: (index: number) => void;
  onScoped: (scoped: boolean) => void;
  onPeek: (id: number | null) => void;
}) {
  const touch = useMedia(TOUCH);
  const list = useRef<HTMLDivElement>(null);
  const model = diff.head;
  const zoomed = scope !== model.root;
  const narrowed = zoomed && scoped;
  const { groups, others } = useMemo(() => {
    const groups: { file: string; mark?: string; items: { c: Change; i: number; d: ReturnType<typeof describeChange> }[] }[] = [];
    for (const i of visible) {
      const c = diff.changes[i];
      const d = describeChange(diff, c);
      const file = d.file ?? '';
      let g = groups.find(x => x.file === file);
      if (!g) groups.push((g = { file, mark: FILE_MARK[diff.fileChange(file, c.status === 'removed')?.status ?? ''], items: [] }));
      g.items.push({ c, i, d });
    }
    const listed = new Set(diff.changes.map(c => describeChange(diff, c).file));
    const rest = diff.files.filter(f => !listed.has(f.path) && !(f.previous && listed.has(f.previous)));
    return { groups, others: narrowed ? filesIn(model, rest, scope) : rest };
  }, [diff, visible, narrowed, scope, model]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>('[aria-current="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [current, marked]);
  const s = { added: 0, changed: 0, removed: 0 };
  for (const i of visible) s[diff.changes[i].status]++;
  const elsewhere = diff.changes.length - visible.length;
  return (
    <aside className="inspector" aria-label="Changes" style={{ '--panel-w': `${size.w}px`, ...(size.h ? { '--panel-h': `${size.h}px` } : {}) } as CSSProperties}>
      <ResizeHandle size={size} onResize={onResize} />
      <div className="insp-body" ref={list}>
        <div className="insp-head">
          <header>
            <span className="insp-kind">{info.kind === 'pr' ? `Pull request #${info.number}` : info.kind === 'local' ? ({ refs: 'Local refs', worktree: 'Working tree' } as Record<string, string>)[info.mode ?? ''] ?? 'Local branch' : 'Comparison'}{info.state ? <> · <span>{info.state}</span></> : null}</span>
            <button className="icon" aria-label="Close changes" onClick={onClose}><X size={15} /></button>
          </header>
          <h2>{info.title}</h2>
        </div>
        <p className="change-refs">
          <code>{info.baseRef}</code> ← <code>{info.headRef}</code>
          {info.kind === 'local' ? <span className="dim" title={info.repo}>{info.repo}</span> : <a href={info.url} target="_blank" rel="noreferrer">GitHub<ExternalLink size={11} /></a>}
        </p>
        <div className="change-summary">
          <span className="added">{s.added} added</span>
          <span className="changed">{s.changed} changed</span>
          <span className="removed">{s.removed} removed</span>
        </div>
        {zoomed && (
          <div className="change-scope">
            <span title={scopeName(model, scope)}>{scoped ? <>In <strong>{scopeName(model, scope)}</strong></> : 'Everywhere'}</span>
            <label><input type="checkbox" checked={scoped} onChange={e => onScoped(e.target.checked)} />Only in view</label>
          </div>
        )}
        {visible.length > 0
          ? <p className="insp-doc">In the order the diagram reads. {touch ? 'Tap a change to unfold it, and its crosshair to go there.' : <>Click a change to unfold it, double-click to zoom to it; <kbd>N</kbd> and <kbd>P</kbd> step through them.</>}</p>
          : narrowed && elsewhere > 0
            ? <p className="insp-doc">No changes in this part; {elsewhere} elsewhere. <button className="link" onClick={() => onScoped(false)}>Show all</button></p>
            : <p className="insp-doc">No block of the analyzed code changed{diff.files.length ? '; the files changed are listed below.' : '.'}</p>}
        {groups.map(g => (
          <div className="insp-field" key={g.file}>
            <label className="change-file">{g.mark && <span className={`file-mark ${g.mark}`}>{g.mark}</span>}{g.file || 'Repository'}</label>
            <div className="uses changes" onMouseLeave={() => onPeek(null)}>
              {g.items.map(({ c, i, d }) => (
                <div className="change-item" key={i}>
                  <div className="change-row">
                    <button
                      className="change-main"
                      aria-current={i === current}
                      aria-expanded={i === marked}
                      // The first click of a double click only marks; the second does not undo it.
                      onClick={e => { if (e.detail < 2) onMark(i === marked ? -1 : i); }}
                      onDoubleClick={() => { onMark(i); onGo(i); }}
                      onMouseEnter={() => onPeek(c.at)}
                    >
                      <span className="use-name"><span className={`change-mark ${c.status}`}>{MARK[c.status]}</span>{d.context && <span className="dim">{d.context} › </span>}{d.name}<span className="dim"> · {d.kind.toLowerCase()}</span></span>
                      {d.line !== undefined && <span className="use-at">{d.line}</span>}
                    </button>
                    <button className="icon change-zoom" aria-label={`Zoom to ${d.name}`} title="Zoom to it" onClick={() => { onMark(i); onGo(i); }}><Crosshair size={13} /></button>
                  </div>
                  {i === marked && <div className="change-code"><ChangeCode diff={diff} change={c} highlights={highlights} baseHighlights={baseHighlights} /></div>}
                </div>
              ))}
            </div>
          </div>
        ))}
        {others.length > 0 && (
          <div className="insp-field">
            <label>Also changed <span className="count">{others.length}</span></label>
            <div className="other-files">{others.map(f => <div key={f.path}><span className={`file-mark ${FILE_MARK[f.status]}`}>{FILE_MARK[f.status]}</span>{f.previous ? `${f.previous} → ` : ''}{f.path}</div>)}</div>
          </div>
        )}
      </div>
    </aside>
  );
}

/** A block the change removed: what it was, where it was, and its code as it was. */
export function RemovedDetails({ diff, change, baseHighlights, size, index, onResize, onClose, onShow, onStep, onDownload }: {
  diff: SceneDiff;
  change: Change;
  baseHighlights: Highlights;
  size: PanelSize;
  index: number;
  onResize: (size: PanelSize) => void;
  onClose: () => void;
  /** Shows the block it was in. */
  onShow: (id: number) => void;
  onStep: (delta: number) => void;
  /** Saves the block as it was, alone, as SVG. */
  onDownload: () => void;
}) {
  const model = diff.base;
  const n = model.node(change.id);
  const d = describeChange(diff, change);
  const span = n.span;
  return (
    <aside className="inspector" aria-label="Removed block" style={{ '--panel-w': `${size.w}px`, ...(size.h ? { '--panel-h': `${size.h}px` } : {}) } as CSSProperties}>
      <ResizeHandle size={size} onResize={onResize} />
      <div className="insp-body">
        <div className="insp-head">
          <header>
            <span className="insp-kind">{d.kind}{n.subtitle ? <> · <span>{n.subtitle}</span></> : null}</span>
            <button className="icon" aria-label="Close details" onClick={onClose}><X size={15} /></button>
          </header>
          <h2 className={n.kind === 'op' ? 'mono' : undefined}>{d.name}</h2>
        </div>
        <div className="change-status"><span className="change-badge removed">Removed</span><span>from <button className="link" onClick={() => onShow(change.at)}>{diff.head.qualified(change.at)}</button></span></div>
        <ChangeSteps index={index} total={diff.changes.length} onStep={onStep} />
        <div className="insp-actions">
          <button onClick={onDownload} title="Download this block as it was, alone, as SVG"><Download size={13} />SVG</button>
        </div>
        {span && model.fileBytes(span.file) && (
          <div className="insp-source">
            <label>{d.file}<span className="dim"> · {span.line === span.endLine ? `line ${span.line}` : `lines ${span.line}–${span.endLine}`} before</span></label>
            <DiffView base={{ model, highlights: baseHighlights, span }} head={null} />
          </div>
        )}
      </div>
    </aside>
  );
}
