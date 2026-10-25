//! `blockviz-app hook`: for a coding agent's hook that runs before a shell command. When the command
//! is a `git commit`, it opens blockviz on what the commit would make (reusing the running window) and
//! tells the agent to ask the user first; any other command it ignores.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// A `git commit` found in a command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Commit {
    /// `-a`/`--all`: the commit takes the working tree's changes, not just what is staged.
    pub all: bool,
    /// The first `-m` message.
    pub message: Option<String>,
    /// Where it runs, when `git -C <dir>` says.
    pub dir: Option<String>,
}

/// A command line in words, quotes and backslashes resolved; the words of each simple command
/// (split at `&&`, `||`, `;`, `|`, `&` and newlines outside quotes) on their own.
fn commands(line: &str) -> Vec<Vec<String>> {
    let mut out = vec![];
    let mut words: Vec<String> = vec![];
    let mut word = String::new();
    let mut has = false;
    let mut chars = line.chars().peekable();
    let end = |words: &mut Vec<String>, out: &mut Vec<Vec<String>>, word: &mut String, has: &mut bool| {
        if *has {
            words.push(std::mem::take(word));
            *has = false;
        }
        if !words.is_empty() {
            out.push(std::mem::take(words));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                has = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    word.push(q);
                }
            }
            '"' => {
                has = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => match chars.peek() {
                            Some(&n) if matches!(n, '"' | '\\' | '$' | '`') => {
                                word.push(n);
                                chars.next();
                            }
                            _ => word.push('\\'),
                        },
                        _ => word.push(q),
                    }
                }
            }
            '\\' => {
                has = true;
                if let Some(n) = chars.next() {
                    if n != '\n' {
                        word.push(n);
                    }
                }
            }
            ' ' | '\t' => {
                if has {
                    words.push(std::mem::take(&mut word));
                    has = false;
                }
            }
            '&' | '|' | ';' | '\n' | '(' | ')' => end(&mut words, &mut out, &mut word, &mut has),
            c => {
                has = true;
                word.push(c);
            }
        }
    }
    end(&mut words, &mut out, &mut word, &mut has);
    out
}

/// The `git commit` a command line runs, if it does (the first, in a chain). `git commit --help`, other
/// git commands and mentions of it inside quotes are not.
pub fn parse_commit(line: &str) -> Option<Commit> {
    commands(line).into_iter().find_map(|words| commit_of(&words))
}

fn commit_of(words: &[String]) -> Option<Commit> {
    let mut i = words.iter().position(|w| !is_assignment(w))?;
    if words.get(i).map(String::as_str) != Some("git") {
        return None;
    }
    i += 1;
    let mut dir = None;
    // git's own options, before the subcommand.
    while let Some(w) = words.get(i) {
        match w.as_str() {
            "-C" => dir = words.get(i + 1).cloned().or(dir),
            "-c" | "--git-dir" | "--work-tree" | "--namespace" => {}
            _ if w.starts_with('-') => {
                i += 1;
                continue;
            }
            _ => break,
        }
        i += 2;
    }
    if words.get(i).map(String::as_str) != Some("commit") {
        return None;
    }
    let mut c = Commit { dir, ..Default::default() };
    let mut rest = words[i + 1..].iter();
    while let Some(w) = rest.next() {
        match w.as_str() {
            "--help" | "-h" => return None,
            "--all" => c.all = true,
            "--message" => c.message = c.message.take().or(rest.next().cloned()),
            "--" => break,
            w if w.starts_with("--message=") => c.message = c.message.take().or(Some(w["--message=".len()..].to_string())),
            w if w.starts_with('-') && !w.starts_with("--") => {
                // Short flags together (`-am "msg"`), a message possibly glued on (`-m"msg"`).
                for (k, f) in w.char_indices().skip(1) {
                    match f {
                        'a' => c.all = true,
                        'm' => {
                            let glued = &w[k + 1..];
                            let message = if glued.is_empty() { rest.next().cloned() } else { Some(glued.to_string()) };
                            c.message = c.message.take().or(message);
                            break;
                        }
                        'F' | 'C' | 'c' | 'S' => {
                            if w[k + 1..].is_empty() {
                                rest.next();
                            }
                            break;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    Some(c)
}

/// `NAME=value`, which may precede a command.
fn is_assignment(w: &str) -> bool {
    w.split_once('=').is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !n.starts_with(|c: char| c.is_ascii_digit()))
}

/// What the agent is about to run, from its hook's input (Claude Code's PreToolUse).
#[derive(Deserialize)]
struct ClaudeInput {
    #[serde(default)]
    tool_name: String,
    #[serde(default)]
    tool_input: ToolInput,
    #[serde(default)]
    cwd: String,
}

#[derive(Deserialize, Default)]
struct ToolInput {
    #[serde(default)]
    command: String,
}

/// The command and folder from Claude Code's hook input, when it is a shell command.
pub fn claude_command(stdin: &str) -> Option<(String, String)> {
    let i: ClaudeInput = serde_json::from_str(stdin).ok()?;
    (i.tool_name == "Bash").then_some((i.tool_input.command, i.cwd))
}

/// What to show for a commit about to be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub folder: PathBuf,
    pub compare: &'static str,
    pub title: Option<String>,
}

/// The comparison for a command run in `cwd`: none when it is not a commit.
pub fn launch_for(command: &str, cwd: &Path) -> Option<Launch> {
    let c = parse_commit(command)?;
    let folder = match &c.dir {
        Some(d) => cwd.join(d),
        None => cwd.to_path_buf(),
    };
    Some(Launch { folder, compare: if c.all { "worktree" } else { "staged" }, title: c.message })
}

/// What Claude Code is told: ask the user, whatever mode the agent is in.
pub const CLAUDE_ASK: &str = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"Showing the commit in blockviz"}}"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(line: &str) -> Option<Commit> {
        parse_commit(line)
    }

    #[test]
    fn finds_a_commit_and_what_it_says() {
        assert_eq!(commit("git commit -m \"Fix the thing\""), Some(Commit { all: false, message: Some("Fix the thing".into()), dir: None }));
        assert_eq!(commit("git commit"), Some(Commit::default()));
        assert_eq!(commit("git commit --amend --no-edit"), Some(Commit::default()));
        assert_eq!(commit("git commit --message=Hi there").unwrap().message.as_deref(), Some("Hi"));
        assert_eq!(commit("git commit -m 'one' -m 'two'").unwrap().message.as_deref(), Some("one"));
        assert_eq!(commit("git commit -m\"glued\"").unwrap().message.as_deref(), Some("glued"));
    }

    #[test]
    fn notices_the_flag_that_commits_everything() {
        assert!(commit("git commit -a -m x").unwrap().all);
        assert!(commit("git commit --all -m x").unwrap().all);
        let am = commit("git commit -am \"msg\"").unwrap();
        assert!(am.all);
        assert_eq!(am.message.as_deref(), Some("msg"));
        assert!(!commit("git commit -m \"add everything\"").unwrap().all);
    }

    #[test]
    fn finds_it_in_a_chain_and_after_git_options() {
        assert_eq!(commit("git add -A && git commit -m done && git push").unwrap().message.as_deref(), Some("done"));
        assert!(commit("cd repo; git commit -m x").is_some());
        assert!(commit("make test || git commit -m x").is_some());
        assert_eq!(commit("git -C ../other commit -m x").unwrap().dir.as_deref(), Some("../other"));
        assert!(commit("git --no-pager -c user.name=A commit -m x").is_some());
        assert!(commit("GIT_AUTHOR_NAME=A git commit -m x").is_some());
        assert_eq!(commit("git commit -m \"it's \\\"quoted\\\"\"").unwrap().message.as_deref(), Some("it's \"quoted\""));
        assert!(commit("git commit -m \"a && b\"").is_some());
    }

    #[test]
    fn ignores_what_is_not_a_commit() {
        for not in ["git status", "git push origin main", "git log --grep commit", "git commit --help", "git commit -h", "echo \"git commit -m x\"", "git add commit.txt", "ls", "", "gitk commit", "grep 'git commit' README.md"] {
            assert_eq!(commit(not), None, "{not}");
        }
    }

    #[test]
    fn decides_what_to_show_from_the_agents_input() {
        let input = |tool: &str, cmd: &str| format!("{{\"tool_name\":\"{tool}\",\"tool_input\":{{\"command\":{}}},\"cwd\":\"/work/app\"}}", serde_json::to_string(cmd).unwrap());
        let (cmd, cwd) = claude_command(&input("Bash", "git commit -m \"Add x\"")).unwrap();
        assert_eq!(launch_for(&cmd, Path::new(&cwd)), Some(Launch { folder: "/work/app".into(), compare: "staged", title: Some("Add x".into()) }));
        let (cmd, cwd) = claude_command(&input("Bash", "git commit -am wip")).unwrap();
        assert_eq!(launch_for(&cmd, Path::new(&cwd)).unwrap().compare, "worktree");
        let (cmd, cwd) = claude_command(&input("Bash", "git -C sub commit -m x")).unwrap();
        assert_eq!(launch_for(&cmd, Path::new(&cwd)).unwrap().folder, PathBuf::from("/work/app/sub"));
        // Other tools, other commands and nonsense are none of its business.
        assert_eq!(claude_command(&input("Edit", "git commit")), None);
        assert_eq!(launch_for("git push", Path::new("/work/app")), None);
        assert_eq!(claude_command("not json"), None);
    }

    #[test]
    fn tells_the_agent_to_ask() {
        let v: serde_json::Value = serde_json::from_str(CLAUDE_ASK).unwrap();
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "ask");
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    }
}
