//! Limiting a change's lists to the block the view is zoomed into (the web's
//! src/ui/scope.ts).

use crate::compare::FileChange;
use crate::diff::{Change, Status};
use crate::model::Model;
use blockviz_core::ir::Kind;

/// Whether block `id` is `scope` or inside it.
fn inside(m: &Model, id: u32, scope: u32) -> bool {
    let mut at = Some(id);
    while let Some(c) = at {
        if c == scope {
            return true;
        }
        at = m.node(c).parent;
    }
    false
}

/// Which of the changes (as their indexes) are in the block the view is
/// zoomed into, the repository being all. A removed block is where it was:
/// in the head block it was in.
pub fn changes_in(m: &Model, changes: &[Change], scope: u32) -> Vec<usize> {
    changes
        .iter()
        .enumerate()
        .filter(|(_, c)| scope == m.root() || inside(m, if c.status == Status::Removed { c.at } else { c.id }, scope))
        .map(|(i, _)| i)
        .collect()
}

/// Which files (by path, or the path before a rename) are in the folder or file the view is zoomed into.
pub fn files_in<'a>(m: &Model, files: &'a [FileChange], scope: u32) -> Vec<&'a FileChange> {
    let mut at = Some(scope);
    while let Some(c) = at {
        let n = m.node(c);
        if n.kind == Kind::Repo {
            break;
        }
        if let Some(file) = m.file_of(n) {
            return files.iter().filter(|f| f.path == file || f.previous.as_deref() == Some(file)).collect();
        }
        if n.kind == Kind::Dir {
            // Folders are titled by their own name (`app/`), joined down from the repository.
            let dir: String = m.canonical_path(c).iter().map(|&i| m.node(i)).filter(|x| x.kind == Kind::Dir).map(|x| x.title.as_str()).collect();
            return files.iter().filter(|f| f.path.starts_with(&dir) || f.previous.as_deref().is_some_and(|p| p.starts_with(&dir))).collect();
        }
        at = n.parent;
    }
    files.iter().collect()
}

/// Where a scope is, as the breadcrumbs name it, below the repository (empty for the repository).
pub fn scope_name(m: &Model, scope: u32) -> String {
    m.canonical_path(scope).iter().skip(1).map(|&i| m.node(i).title.as_str()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" › ")
}
