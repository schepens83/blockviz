//! Which paths of a folder are not part of the project because its `.gitignore` files say so: the ones
//! in it and in its subfolders, `.git/info/exclude`, the user's global excludes and `.ignore`, with their
//! negations and folder-only patterns. A plain folder's `.gitignore` counts too, with no repository.

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub struct Ignores {
    root: PathBuf,
    /// The patterns of each folder's own files, read when first needed.
    dirs: RefCell<HashMap<PathBuf, Rc<Gitignore>>>,
    /// The repository's `.git/info/exclude` and the user's global excludes: the weakest.
    extra: RefCell<Option<Rc<Vec<Gitignore>>>>,
}

impl Ignores {
    pub fn new(root: &Path) -> Self {
        Ignores { root: root.to_path_buf(), dirs: RefCell::default(), extra: RefCell::default() }
    }

    /// Forgets what was read: ignore files changed.
    pub fn reset(&self) {
        self.dirs.borrow_mut().clear();
        *self.extra.borrow_mut() = None;
    }

    /// Whether a change at `rel` (a path in the folder) changes what is ignored.
    pub fn affects(rel: &str) -> bool {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        name == ".gitignore" || name == ".ignore" || rel == ".git/info/exclude" || rel.ends_with("/.git/info/exclude")
    }

    fn own(&self, dir: &Path) -> Rc<Gitignore> {
        if let Some(g) = self.dirs.borrow().get(dir) {
            return g.clone();
        }
        let mut b = GitignoreBuilder::new(dir);
        // `.ignore` outranks `.gitignore`, as in ripgrep.
        for name in [".gitignore", ".ignore"] {
            let f = dir.join(name);
            if f.is_file() {
                let _ = b.add(f);
            }
        }
        let g = Rc::new(b.build().unwrap_or_else(|_| Gitignore::empty()));
        self.dirs.borrow_mut().insert(dir.to_path_buf(), g.clone());
        g
    }

    fn extras(&self) -> Rc<Vec<Gitignore>> {
        if let Some(e) = self.extra.borrow().as_ref() {
            return e.clone();
        }
        let mut list = vec![];
        let exclude = self.root.join(".git/info/exclude");
        if exclude.is_file() {
            let mut b = GitignoreBuilder::new(&self.root);
            let _ = b.add(exclude);
            if let Ok(g) = b.build() {
                list.push(g);
            }
        }
        list.push(Gitignore::global().0);
        let rc = Rc::new(list);
        *self.extra.borrow_mut() = Some(rc.clone());
        rc
    }

    /// Whether `rel` (a path in the folder, `/`-separated) is ignored, or is inside a folder that is.
    pub fn ignored(&self, rel: &str, is_dir: bool) -> bool {
        let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
        let mut path = self.root.clone();
        for (k, part) in parts.iter().enumerate() {
            let parent = path.clone();
            path.push(part);
            let dir = k + 1 < parts.len() || is_dir;
            if self.verdict(&parent, &path, dir) {
                return true;
            }
        }
        false
    }

    /// What the patterns of the folders around `path` say of it: the deepest to speak decides.
    fn verdict(&self, parent: &Path, path: &Path, is_dir: bool) -> bool {
        let mut d = Some(parent);
        while let Some(dir) = d {
            if !dir.starts_with(&self.root) {
                break;
            }
            match self.own(dir).matched(path, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
            d = dir.parent();
        }
        for g in self.extras().iter() {
            match g.matched(path, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("blockviz-ignores-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, path: &str, text: &str) {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn follows_a_gitignore_with_its_negations_and_folder_only_patterns() {
        let dir = folder("root");
        write(&dir, ".gitignore", "*.log\ntmp/\n!keep.log\nbuild-notes\n/only-here.txt\n");
        let ig = Ignores::new(&dir);
        assert!(ig.ignored("debug.log", false));
        assert!(ig.ignored("src/deep/debug.log", false));
        assert!(!ig.ignored("keep.log", false));
        assert!(!ig.ignored("a.py", false));
        // A folder-only pattern ignores the folder and what is in it, not a file of that name.
        assert!(ig.ignored("tmp", true));
        assert!(ig.ignored("tmp/x.py", false));
        assert!(ig.ignored("src/tmp/x.py", false));
        assert!(!ig.ignored("tmp", false));
        // A pattern with a slash in front is the folder's own.
        assert!(ig.ignored("only-here.txt", false));
        assert!(!ig.ignored("sub/only-here.txt", false));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_nested_gitignore_speaks_for_its_folder_and_can_undo_the_one_above() {
        let dir = folder("nested");
        write(&dir, ".gitignore", "*.gen.py\n");
        write(&dir, "pkg/.gitignore", "local.py\n!special.gen.py\n");
        let ig = Ignores::new(&dir);
        assert!(ig.ignored("pkg/local.py", false));
        assert!(!ig.ignored("local.py", false));
        assert!(!ig.ignored("other/local.py", false));
        assert!(ig.ignored("x.gen.py", false));
        assert!(ig.ignored("pkg/x.gen.py", false));
        assert!(!ig.ignored("pkg/special.gen.py", false));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn follows_the_repositorys_exclude_file_and_dot_ignore() {
        let dir = folder("exclude");
        write(&dir, ".git/info/exclude", "scratch.py\n");
        write(&dir, ".ignore", "notes/\n");
        let ig = Ignores::new(&dir);
        assert!(ig.ignored("scratch.py", false));
        assert!(ig.ignored("a/scratch.py", false));
        assert!(ig.ignored("notes/x.md", false));
        assert!(!ig.ignored("a.py", false));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_that_is_no_repository_counts_its_gitignore_too() {
        let dir = folder("plain");
        write(&dir, ".gitignore", "secret.py\n");
        assert!(!dir.join(".git").exists());
        assert!(Ignores::new(&dir).ignored("secret.py", false));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn what_changed_in_an_ignore_file_is_known() {
        for rel in [".gitignore", "pkg/.gitignore", ".ignore", ".git/info/exclude"] {
            assert!(Ignores::affects(rel), "{rel}");
        }
        assert!(!Ignores::affects("a.py") && !Ignores::affects("docs/gitignore.md"));
    }

    #[test]
    fn forgets_what_it_read_when_told() {
        let dir = folder("reset");
        let ig = Ignores::new(&dir);
        assert!(!ig.ignored("a.py", false));
        write(&dir, ".gitignore", "a.py\n");
        assert!(!ig.ignored("a.py", false));
        ig.reset();
        assert!(ig.ignored("a.py", false));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
