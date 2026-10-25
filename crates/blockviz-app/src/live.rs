//! Keeps a folder's analysis current. A thread watches the folder; when files
//! change it reads just those, waits for a burst of changes (a save is often
//! several events, a checkout many) to settle, and analyzes the project again,
//! reusing what the core parsed of every file that did not change.

use crate::ignores::Ignores;
use crate::model::Model;
use blockviz_core::project::{build_cached, ignored, Cache, InFile};
use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// How long the folder must be quiet before analyzing again.
const SETTLE: Duration = Duration::from_millis(120);
/// Files over this size are listed without their text (the core's own limit).
pub const MAX_TEXT: u64 = 2_000_000;

/// A file as the project holds it: its text (none when unreadable or too large) and size.
#[derive(Clone)]
pub struct File {
    pub text: Option<String>,
    pub bytes: u64,
}

/// The project's files, by path relative to its folder.
pub type Files = BTreeMap<String, File>;

/// A new analysis of the project: its first, or one made after files changed.
pub struct Update {
    pub model: Model,
    /// The files it was made from (a comparison reads them).
    pub files: Files,
    /// The files that changed since the last one (none for the first, or a new layout).
    pub changed: Vec<String>,
    pub first: bool,
    /// How long reading (the first time) and analyzing took.
    pub read: Duration,
    pub took: Duration,
    /// Files whose parse was reused.
    pub reused: usize,
}

/// What the window asks of the project's thread.
pub enum Ask {
    /// Lay the project out again, each block's values on one branching line or not.
    Layout { shared_trunks: bool },
    /// Stop: another folder is open.
    Stop,
}

enum Msg {
    Fs(notify::Result<notify::Event>),
    Ask(Ask),
}

/// The folder's files, less what its `.gitignore` files and the list of folders never read leave out.
pub fn read(root: &Path) -> Files {
    read_with(root, &Ignores::new(root))
}

fn read_with(root: &Path, ignores: &Ignores) -> Files {
    let mut out = Files::new();
    walk(root, root, ignores, &mut out);
    out
}

fn walk(dir: &Path, root: &Path, ignores: &Ignores, out: &mut Files) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let rel = relative(root, &p);
        if p.is_dir() {
            if !ignored(&format!("{rel}/x")) && !ignores.ignored(&rel, true) {
                walk(&p, root, ignores, out);
            }
        } else if !ignored(&rel) && !ignores.ignored(&rel, false) {
            out.insert(rel, read_file(&p));
        }
    }
}

fn read_file(p: &Path) -> File {
    let bytes = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let text = if bytes < MAX_TEXT { std::fs::read_to_string(p).ok() } else { None };
    File { text, bytes }
}

fn relative(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// The files as the core takes them.
pub fn inputs(files: &Files) -> Vec<InFile> {
    files.iter().map(|(path, f)| InFile { path: path.clone(), text: f.text.clone(), bytes: f.bytes }).collect()
}

pub fn texts(files: &Files) -> HashMap<String, String> {
    files.iter().filter_map(|(p, f)| Some((p.clone(), f.text.clone()?))).collect()
}

/// Brings `files` up to date with what is on disk at `paths` (files or
/// folders, added, changed or removed). Returns the files whose text changed.
pub fn apply(root: &Path, ignores: &Ignores, files: &mut Files, paths: impl IntoIterator<Item = PathBuf>) -> Vec<String> {
    let paths: Vec<PathBuf> = paths.into_iter().collect();
    // What is ignored changed: read the whole folder again.
    if paths.iter().any(|p| Ignores::affects(&relative(root, p))) {
        ignores.reset();
        let now = read_with(root, ignores);
        let mut changed: Vec<String> = files.keys().chain(now.keys()).filter(|k| files.get(*k).map(|f| &f.text) != now.get(*k).map(|f| &f.text)).cloned().collect();
        changed.sort();
        changed.dedup();
        *files = now;
        return changed;
    }
    let mut changed = vec![];
    for p in paths {
        let rel = relative(root, &p);
        if rel.is_empty() || ignored(&format!("{rel}/x")) || ignores.ignored(&rel, p.is_dir()) {
            continue;
        }
        let prefix = format!("{rel}/");
        if p.is_dir() {
            // A folder added, moved in or out, or changed wholesale: read it again.
            let mut now = Files::new();
            walk(&p, root, ignores, &mut now);
            let gone: Vec<String> = files.range(prefix.clone()..).take_while(|(k, _)| k.starts_with(&prefix)).map(|(k, _)| k.clone()).filter(|k| !now.contains_key(k)).collect();
            for k in gone {
                files.remove(&k);
                changed.push(k);
            }
            for (k, f) in now {
                if files.get(&k).map(|old| &old.text) != Some(&f.text) {
                    files.insert(k.clone(), f);
                    changed.push(k);
                }
            }
        } else if p.is_file() {
            if ignored(&rel) || ignores.ignored(&rel, false) {
                continue;
            }
            let f = read_file(&p);
            if files.get(&rel).map(|old| &old.text) != Some(&f.text) {
                files.insert(rel.clone(), f);
                changed.push(rel);
            }
        } else {
            // Gone: a file, or a folder and everything in it.
            if files.remove(&rel).is_some() {
                changed.push(rel.clone());
            }
            let under: Vec<String> = files.range(prefix.clone()..).take_while(|(k, _)| k.starts_with(&prefix)).map(|(k, _)| k.clone()).collect();
            for k in under {
                files.remove(&k);
                changed.push(k);
            }
        }
    }
    changed.sort();
    changed.dedup();
    changed
}

/// Opens the project at `root` on a thread of its own: reads and analyzes
/// it, sends the analysis, then watches the folder and sends a new one
/// whenever its files change (or the window asks for a new layout). Stops when
/// asked, or when `send` reports the window is gone.
pub fn open(root: PathBuf, shared_trunks: bool, send: impl Fn(Update) -> bool + Send + 'static) -> Sender<Ask> {
    let (tx, rx) = channel::<Msg>();
    let (ask_tx, ask_rx) = channel::<Ask>();
    // The window's asks join the folder's events in one queue.
    let forward = tx.clone();
    std::thread::spawn(move || {
        while let Ok(a) = ask_rx.recv() {
            if forward.send(Msg::Ask(a)).is_err() {
                return;
            }
        }
    });
    std::thread::spawn(move || {
        let name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut opts = blockviz_core::layout::Options { shared_trunks };
        let mut cache = Cache::default();
        let reading = Instant::now();
        let ignores = Ignores::new(&root);
        let mut files = read_with(&root, &ignores);
        let read_took = reading.elapsed();
        let analyze = |files: &Files, opts: blockviz_core::layout::Options, cache: &mut Cache| {
            let start = Instant::now();
            let scene = build_cached(&name, inputs(files), opts, cache);
            (Model::new(scene, texts(files)), start.elapsed())
        };
        let (model, took) = analyze(&files, opts, &mut cache);
        if !send(Update { model, files: files.clone(), changed: vec![], first: true, read: read_took, took, reused: 0 }) {
            return;
        }
        let events = tx.clone();
        let watcher = notify::recommended_watcher(move |e| {
            let _ = events.send(Msg::Fs(e));
        })
        .and_then(|mut w| w.watch(&root, RecursiveMode::Recursive).map(|_| w));
        let _watcher = match watcher {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("Not watching {}: {e}", root.display());
                None
            }
        };
        drop(tx);
        let paths_of = |e: notify::Result<notify::Event>| match e {
            // Reading a file is not changing it.
            Ok(e) if !matches!(e.kind, EventKind::Access(_)) => e.paths,
            _ => vec![],
        };
        while let Ok(first) = rx.recv() {
            let mut paths = vec![];
            let mut relayout = false;
            let mut take = |m: Msg, paths: &mut Vec<PathBuf>| -> bool {
                match m {
                    Msg::Fs(e) => paths.extend(paths_of(e)),
                    Msg::Ask(Ask::Layout { shared_trunks }) => {
                        relayout |= opts.shared_trunks != shared_trunks;
                        opts.shared_trunks = shared_trunks;
                    }
                    Msg::Ask(Ask::Stop) => return false,
                }
                true
            };
            if !take(first, &mut paths) {
                return;
            }
            // A save is often several events, a checkout many: wait until the folder is quiet.
            loop {
                match rx.recv_timeout(SETTLE) {
                    Ok(m) => {
                        if !take(m, &mut paths) {
                            return;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            paths.sort();
            paths.dedup();
            let changed = apply(&root, &ignores, &mut files, paths);
            if changed.is_empty() && !relayout {
                continue;
            }
            let (model, took) = analyze(&files, opts, &mut cache);
            let update = Update { model, files: files.clone(), changed, first: false, read: Duration::ZERO, took, reused: cache.reused() };
            if !send(update) {
                return;
            }
        }
    });
    ask_tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applying_changes_reads_only_what_changed_on_disk() {
        let dir = std::env::temp_dir().join(format!("blockviz-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("pkg")).unwrap();
        std::fs::write(dir.join("a.py"), "x = 1\n").unwrap();
        std::fs::write(dir.join("pkg/b.py"), "y = 2\n").unwrap();
        std::fs::create_dir_all(dir.join("node_modules/dep")).unwrap();
        std::fs::write(dir.join("node_modules/dep/index.js"), "").unwrap();
        let ig = Ignores::new(&dir);
        let mut files = read(&dir);
        assert_eq!(files.keys().collect::<Vec<_>>(), ["a.py", "pkg/b.py"]);

        // A file saved with the same text is no change; a new text is.
        std::fs::write(dir.join("a.py"), "x = 1\n").unwrap();
        assert!(apply(&dir, &ig, &mut files, [dir.join("a.py")]).is_empty());
        std::fs::write(dir.join("a.py"), "x = 3\n").unwrap();
        assert_eq!(apply(&dir, &ig, &mut files, [dir.join("a.py")]), ["a.py"]);
        assert_eq!(files["a.py"].text.as_deref(), Some("x = 3\n"));

        // Ignored folders stay out; a folder removed takes its files with it.
        std::fs::write(dir.join("node_modules/dep/index.js"), "changed").unwrap();
        assert!(apply(&dir, &ig, &mut files, [dir.join("node_modules/dep/index.js")]).is_empty());
        std::fs::remove_dir_all(dir.join("pkg")).unwrap();
        assert_eq!(apply(&dir, &ig, &mut files, [dir.join("pkg")]), ["pkg/b.py"]);
        assert!(!files.contains_key("pkg/b.py"));

        // A folder moved in is read whole.
        std::fs::create_dir_all(dir.join("new")).unwrap();
        std::fs::write(dir.join("new/c.py"), "z = 1\n").unwrap();
        assert_eq!(apply(&dir, &ig, &mut files, [dir.join("new")]), ["new/c.py"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn leaves_out_what_gitignore_says_at_first_and_as_files_come_and_go() {
        let dir = std::env::temp_dir().join(format!("blockviz-live-ignore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let write = |path: &str, text: &str| {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write(".gitignore", "*.log\ntmp/\n!keep.log\n");
        write("a.py", "x = 1\n");
        write("debug.log", "noise");
        write("keep.log", "kept");
        write("tmp/scratch.py", "y = 1\n");
        write("pkg/.gitignore", "gen.py\n");
        write("pkg/gen.py", "z = 1\n");
        write("pkg/real.py", "z = 2\n");
        // Dot folders were always read: they hold workflows and the like.
        write(".github/workflows/ci.yml", "on: push\n");
        let ig = Ignores::new(&dir);
        let mut files = read(&dir);
        let names = |f: &Files| f.keys().cloned().collect::<Vec<_>>();
        assert_eq!(names(&files), [".github/workflows/ci.yml", ".gitignore", "a.py", "keep.log", "pkg/.gitignore", "pkg/real.py"]);

        // New files: an ignored one stays out, another comes in.
        write("new.log", "x");
        write("tmp/more.py", "q = 1\n");
        write("pkg/gen.py", "z = 3\n");
        write("b.py", "w = 1\n");
        let paths = ["new.log", "tmp/more.py", "pkg/gen.py", "b.py"].map(|p| dir.join(p));
        assert_eq!(apply(&dir, &ig, &mut files, paths), ["b.py"]);
        // A folder moved in is read without what it ignores.
        write("other/.gitignore", "skip.py\n");
        write("other/skip.py", "s = 1\n");
        write("other/use.py", "u = 1\n");
        assert_eq!(apply(&dir, &ig, &mut files, [dir.join("other")]), ["other/.gitignore", "other/use.py"]);

        // Editing .gitignore (no more tmp/, no more !keep.log, now b.py) reads everything again.
        write(".gitignore", "*.log\nb.py\n");
        assert_eq!(apply(&dir, &ig, &mut files, [dir.join(".gitignore")]), [".gitignore", "b.py", "keep.log", "tmp/more.py", "tmp/scratch.py"]);
        assert!(!files.contains_key("b.py") && !files.contains_key("keep.log") && files.contains_key("tmp/scratch.py"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
