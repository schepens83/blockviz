//! Asking the app to show a comparison: the command line's `--compare`, and
//! a running instance receiving it from another launch (a coding agent's hook)
//! over a Unix socket, so that one window shows each.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What to compare.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Spec {
    /// The folder as it is, against HEAD.
    Worktree,
    /// What is staged (the index), against HEAD: what a commit would make.
    Staged,
    /// The branch checked out, against where it left the default branch.
    Branch,
    /// Two revisions exactly (`A..B`).
    Refs(String, String),
    /// `B` against where it left `A` (`A...B`, as GitHub compares).
    MergeBase(String, String),
    /// One commit: the change it made, against its parent.
    Commit(String),
}

impl Spec {
    /// `worktree`, `staged`, `branch`, `A..B`, `A...B` or `REV^!` (one commit). A revision starting with `-` would be read by git as an option.
    pub fn parse(s: &str) -> Result<Spec, String> {
        let pair = |sep: &str| -> Option<Result<(String, String), String>> {
            let (a, b) = s.split_once(sep)?;
            Some(if a.is_empty() || b.is_empty() { Err(format!("`{s}` needs a revision on each side of `{sep}`.")) } else if a.starts_with('-') || b.starts_with('-') { Err(format!("`{s}` names a revision starting with `-`.")) } else { Ok((a.into(), b.into())) })
        };
        if let Some(rev) = s.strip_suffix("^!") {
            return if rev.is_empty() || rev.starts_with('-') || rev.contains("..") { Err(format!("`{s}` needs a plain revision before `^!`.")) } else { Ok(Spec::Commit(rev.into())) };
        }
        match s {
            "worktree" => Ok(Spec::Worktree),
            "staged" => Ok(Spec::Staged),
            "branch" => Ok(Spec::Branch),
            _ => match pair("...") {
                Some(r) => r.map(|(a, b)| Spec::MergeBase(a, b)),
                None => match pair("..") {
                    Some(r) => r.map(|(a, b)| Spec::Refs(a, b)),
                    None => Err(format!("`{s}` is not worktree, staged, branch, A..B, A...B or REV^!.")),
                },
            },
        }
    }
}

/// A request to show a repository, perhaps in a comparison, with a title for it (a commit message).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compare: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl Request {
    /// As one line of JSON.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn decode(line: &str) -> Result<Request, String> {
        serde_json::from_str(line.trim()).map_err(|e| e.to_string())
    }
}

/// Where the running instance listens: the user's runtime folder, else `/tmp`.
pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("blockviz-app.sock"),
        None => PathBuf::from(format!("/tmp/blockviz-app-{}.sock", std::env::var("USER").unwrap_or_else(|_| "user".into()))),
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::mpsc::{channel, Receiver};
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(2);

    /// Hands `req` to the instance listening at `path`. False when none answers (nothing there, or a socket left behind).
    pub fn send(path: &Path, req: &Request) -> bool {
        let Ok(mut s) = UnixStream::connect(path) else { return false };
        let _ = s.set_read_timeout(Some(WAIT));
        let _ = s.set_write_timeout(Some(WAIT));
        if writeln!(s, "{}", req.encode()).is_err() {
            return false;
        }
        let mut ack = String::new();
        BufReader::new(s).read_line(&mut ack).is_ok() && ack.trim() == "ok"
    }

    /// Starts listening at `path`; requests arrive on the receiver. A socket file with nobody behind it is
    /// removed and replaced. Err when an instance is already listening there, or the socket cannot be made.
    pub fn serve(path: &Path) -> std::io::Result<Receiver<Request>> {
        let listener = match UnixListener::bind(path) {
            Ok(l) => l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                if UnixStream::connect(path).is_ok() {
                    return Err(e);
                }
                std::fs::remove_file(path)?;
                UnixListener::bind(path)?
            }
            Err(e) => return Err(e),
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                // One slow client does not hold up the rest.
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(WAIT));
                    let mut line = String::new();
                    if BufReader::new(&stream).read_line(&mut line).is_err() {
                        return;
                    }
                    if let Ok(req) = Request::decode(&line) {
                        if tx.send(req).is_ok() {
                            let _ = (&stream).write_all(b"ok\n");
                        }
                    }
                });
            }
        });
        Ok(rx)
    }
}

#[cfg(unix)]
pub use unix::{send, serve};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_to_compare() {
        assert_eq!(Spec::parse("worktree"), Ok(Spec::Worktree));
        assert_eq!(Spec::parse("staged"), Ok(Spec::Staged));
        assert_eq!(Spec::parse("branch"), Ok(Spec::Branch));
        assert_eq!(Spec::parse("v1..main"), Ok(Spec::Refs("v1".into(), "main".into())));
        assert_eq!(Spec::parse("main...feature/x"), Ok(Spec::MergeBase("main".into(), "feature/x".into())));
        assert_eq!(Spec::parse("HEAD~3..HEAD"), Ok(Spec::Refs("HEAD~3".into(), "HEAD".into())));
        assert_eq!(Spec::parse("abc123^!"), Ok(Spec::Commit("abc123".into())));
    }

    #[test]
    fn rejects_the_rest() {
        for bad in ["", "uncommitted", "a..", "..b", "...", "--output=x..main", "main...-x", "^!", "-x^!", "a..b^!"] {
            assert!(Spec::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_request_is_one_line_of_json_and_comes_back() {
        let r = Request { repo: "/home/me/app".into(), compare: Some("staged".into()), title: Some("Fix \"it\"\nnow".into()) };
        let line = r.encode();
        assert!(!line.contains('\n'));
        assert_eq!(Request::decode(&line), Ok(r));
        assert_eq!(Request::decode(r#"{"repo":"/x"}"#), Ok(Request { repo: "/x".into(), compare: None, title: None }));
        assert!(Request::decode("nonsense").is_err());
    }

    #[cfg(unix)]
    mod socket {
        use super::*;

        fn path(name: &str) -> PathBuf {
            std::env::temp_dir().join(format!("blockviz-{name}-{}.sock", std::process::id()))
        }

        #[test]
        fn a_running_instance_receives_the_request() {
            let p = path("live");
            let _ = std::fs::remove_file(&p);
            let rx = serve(&p).unwrap();
            let r = Request { repo: "/repo".into(), compare: Some("worktree".into()), title: None };
            assert!(send(&p, &r));
            assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(2)), Ok(r));
            // A second instance finds this one listening, and does not take its place.
            assert!(serve(&p).is_err());
            std::fs::remove_file(&p).unwrap();
        }

        #[test]
        fn nothing_answers_at_a_missing_or_stale_socket_and_a_stale_one_is_replaced() {
            let p = path("stale");
            let _ = std::fs::remove_file(&p);
            let r = Request { repo: "/repo".into(), compare: None, title: None };
            assert!(!send(&p, &r));
            // A socket file left by an instance that died.
            drop(std::os::unix::net::UnixListener::bind(&p).unwrap());
            assert!(p.exists());
            assert!(!send(&p, &r));
            let rx = serve(&p).unwrap();
            assert!(send(&p, &r));
            assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(2)), Ok(r));
            std::fs::remove_file(&p).unwrap();
        }
    }
}
