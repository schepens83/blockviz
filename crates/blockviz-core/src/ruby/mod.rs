//! Ruby source to syntax tree. Prism, Ruby's own parser, reads each file
//! (error-tolerant, current with the newest grammar); `lower` translates its
//! tree into the shared syntax tree.
//!
//! Ruby code names classes and modules, not files: a Rails app never
//! requires its own code (Zeitwerk loads `UsersController` from
//! `app/controllers/users_controller.rb` when it is first named), and a gem
//! requires its files once and then uses their constants anywhere. So the
//! files of a project (the directory of its Gemfile or gemspec) share one
//! namespace, as the files of a Go package do, and every class and module
//! is known by its full name (`Admin::UsersController`): a reference is
//! resolved the way Ruby looks constants up, from the innermost module
//! around it outwards, against what the project defines.

#[cfg(target_arch = "wasm32")]
mod libc;
pub mod lower;
pub mod rails;

use crate::code::source::Source;
use crate::code::syntax::{Lang, TextRange};
use crate::code::{CodeFile, Module};
use crate::hash::{Map, Set};
use crate::ir::Builder;
use ruby_prism::Node;

pub struct RbInput {
    pub file: u32,
    pub path: String,
    pub text: String,
}

/// Ruby code by its path: `.rb` files, Rake files and executables with a
/// Ruby shebang.
pub fn is_ruby(path: &str, text: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    path.ends_with(".rb") || path.ends_with(".rake") || matches!(base, "Rakefile" | "Thorfile") || (!base.contains('.') && text.starts_with("#!") && text.lines().next().is_some_and(|l| l.contains("ruby")))
}

/// Libraries that ship with Ruby: required, but not packages the project depends on.
pub fn is_stdlib(name: &str) -> bool {
    matches!(
        name,
        "abbrev" | "base64" | "benchmark" | "bigdecimal" | "cgi" | "coverage" | "csv" | "date" | "delegate" | "did_you_mean" | "digest" | "drb" | "English" | "erb" | "etc" | "expect" | "fcntl" | "fiber" | "fiddle" | "fileutils" | "find" | "forwardable" | "getoptlong" | "io" | "ipaddr" | "irb" | "json" | "logger" | "matrix" | "monitor" | "mutex_m" | "net" | "nkf" | "objspace" | "observer" | "open-uri" | "open3" | "openssl" | "optparse" | "ostruct" | "pathname" | "pp" | "prettyprint" | "prime" | "pstore" | "psych" | "racc" | "rbconfig" | "rdoc" | "readline" | "reline" | "resolv" | "ripper" | "securerandom" | "set" | "shellwords" | "singleton" | "socket" | "stringio" | "strscan" | "syslog" | "tempfile" | "time" | "timeout" | "tmpdir" | "tsort" | "un" | "uri" | "weakref" | "yaml" | "zlib" | "rubygems" | "bundler" | "thread" | "rational" | "complex" | "enumerator" | "prism" | "random" | "win32ole"
    )
}

/// A class or module the project defines.
#[derive(Default, Debug)]
pub struct Const {
    pub module: bool,
    /// The superclass as written, and resolved to a project constant.
    pub superclass: Option<String>,
    /// Modules mixed in (`include`, `extend`, `prepend`), resolved where possible.
    pub mixins: Vec<String>,
    pub methods: Set<String>,
    /// `def self.x` and the methods of `class << self`.
    pub singleton: Set<String>,
    /// Inputs that define it, in order.
    pub files: Vec<usize>,
}

/// Every class and module of the project, by full name.
#[derive(Default)]
pub struct Consts {
    pub defs: Map<String, Const>,
}

impl Consts {
    /// The constant `path` names where `nesting` (outermost first) is open:
    /// Ruby looks in each enclosing module, innermost first, then at the top.
    pub fn resolve(&self, nesting: &[String], path: &str) -> Option<String> {
        if let Some(abs) = path.strip_prefix("::") {
            return self.defs.contains_key(abs).then(|| abs.to_string());
        }
        for scope in nesting.iter().rev() {
            let full = format!("{scope}::{path}");
            if self.defs.contains_key(&full) {
                return Some(full);
            }
        }
        self.defs.contains_key(path).then(|| path.to_string())
    }

    /// Classes and modules whose methods `name` has, nearest first: itself,
    /// its mixins, its superclass and theirs.
    pub fn ancestors(&self, name: &str) -> Vec<&str> {
        let mut out: Vec<&str> = vec![];
        let mut stack = vec![name];
        while let Some(n) = stack.pop() {
            let Some((key, c)) = self.defs.get_key_value(n) else { continue };
            if out.contains(&key.as_str()) || out.len() > 64 {
                continue;
            }
            out.push(key);
            if let Some(s) = &c.superclass {
                stack.push(s);
            }
            stack.extend(c.mixins.iter().rev().map(String::as_str));
        }
        out
    }

    /// Whether code in `class` (or its singleton, for class methods) can
    /// call `method` without a receiver because the project defines it there.
    pub fn has_method(&self, class: &str, method: &str, singleton: bool) -> bool {
        self.ancestors(class).iter().any(|a| {
            let c = &self.defs[*a];
            if singleton {
                c.singleton.contains(method) || (c.module && c.methods.contains(method) && self.extended(class, a))
            } else {
                c.methods.contains(method)
            }
        })
    }

    /// `class` extends module `m` (its methods are class methods).
    fn extended(&self, class: &str, m: &str) -> bool {
        class == m || self.defs.get(class).is_some_and(|c| c.mixins.iter().any(|x| x == m))
    }
}

/// `UsersController` → `users_controller`, `Admin::HTTPClient` → `admin/http_client`:
/// where Zeitwerk expects a constant to be defined.
pub fn underscore(name: &str) -> String {
    let mut out = String::new();
    for (i, part) in name.split("::").enumerate() {
        if i > 0 {
            out.push('/');
        }
        let chars: Vec<char> = part.chars().collect();
        for (j, &c) in chars.iter().enumerate() {
            if c.is_ascii_uppercase() {
                let prev_lower = j > 0 && (chars[j - 1].is_ascii_lowercase() || chars[j - 1].is_ascii_digit());
                let next_lower = chars.get(j + 1).is_some_and(|n| n.is_ascii_lowercase());
                if j > 0 && (prev_lower || (next_lower && chars[j - 1].is_ascii_uppercase())) {
                    out.push('_');
                }
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
            }
        }
    }
    out
}

/// The project a Ruby file belongs to: the nearest directory above it with
/// a Gemfile or a gemspec ("" for the repository root).
fn root_of<'a>(path: &str, roots: &'a [String]) -> &'a str {
    roots.iter().filter(|r| r.is_empty() || path.starts_with(&format!("{r}/"))).max_by_key(|r| r.len()).map(String::as_str).unwrap_or("")
}

/// Directories that hold a Gemfile or a gemspec.
pub fn roots(paths: &[&str]) -> Vec<String> {
    let mut roots: Vec<String> = paths
        .iter()
        .filter(|p| {
            let base = p.rsplit('/').next().unwrap_or(p);
            base == "Gemfile" || base.ends_with(".gemspec")
        })
        .map(|p| p.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// Where `require` and `require_relative` find project files.
pub struct Requires<'a> {
    files: Set<&'a str>,
    roots: Vec<String>,
}

impl<'a> Requires<'a> {
    pub fn new(paths: &[&'a str]) -> Self {
        Requires { files: paths.iter().copied().collect(), roots: roots(paths) }
    }

    /// The project file `spec` names from `from`, else `None` (a gem or the standard library).
    pub fn resolve(&self, from: &str, spec: &str, relative: bool) -> Option<String> {
        let with_ext = if spec.ends_with(".rb") { spec.to_string() } else { format!("{spec}.rb") };
        let candidates: Vec<String> = if relative {
            let dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            vec![normalize(&format!("{dir}/{with_ext}"))]
        } else {
            // A gem's `lib/` is on the load path, as is the app's (Rails adds `lib/`).
            let root = root_of(from, &self.roots);
            let join = |d: &str| normalize(&if root.is_empty() { format!("{d}/{with_ext}") } else { format!("{root}/{d}/{with_ext}") });
            vec![join("lib"), join("app/models"), join(".")]
        };
        candidates.into_iter().find(|c| self.files.contains(c.as_str()))
    }
}

/// `a/./b/../c.rb` → `a/c.rb`.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(p),
        }
    }
    parts.join("/")
}

/// Parses and lowers every input; the files of a project share one namespace.
pub fn front_end(_b: &mut Builder, inputs: Vec<RbInput>, all_paths: &[&str], diagnostics: &mut Vec<String>) -> (Vec<Module>, Vec<String>, Vec<CodeFile>) {
    let requires = Requires::new(all_paths);
    let parsed: Vec<ruby_prism::ParseResult<'_>> = inputs.iter().map(|i| ruby_prism::parse(i.text.as_bytes())).collect();
    // Every class and module, with its methods and ancestors, first.
    let mut consts = Consts::default();
    for (i, p) in parsed.iter().enumerate() {
        lower::collect(&p.node(), &mut vec![], i, &mut consts);
    }
    resolve_ancestors(&mut consts);
    // Where each constant is defined first: the file Zeitwerk would load it
    // from, else the first to name its superclass, else the first.
    let mut primary: Map<String, usize> = Map::default();
    for (name, c) in &consts.defs {
        let zeitwerk = format!("{}.rb", underscore(name));
        // (`A` in `class A::B` may be defined nowhere.)
        if let Some(pick) = c.files.iter().copied().find(|&f| inputs[f].path.ends_with(&zeitwerk)).or(c.files.first().copied()) {
            primary.insert(name.clone(), pick);
        }
    }
    let mut lowered = vec![];
    for (i, (input, p)) in inputs.iter().zip(&parsed).enumerate() {
        let comments: Vec<(TextRange, String)> = p
            .comments()
            .flat_map(|c| {
                let loc = c.location();
                let text = String::from_utf8_lossy(c.text()).into_owned();
                // `=begin … =end` contributes one entry per line.
                let mut out = vec![];
                let mut offset = loc.start();
                for line in text.split('\n') {
                    let len = line.len() as u32;
                    let words = line.strip_prefix('#').unwrap_or(if line.starts_with("=begin") || line.starts_with("=end") { "" } else { line });
                    out.push((TextRange::new(offset.into(), (offset + len).into()), words.to_string()));
                    offset += len + 1;
                }
                out
            })
            .collect();
        let error = p.errors().next().map(|e| {
            let offset = (e.location().start() as usize).min(input.text.len());
            let line = input.text[..offset].bytes().filter(|c| *c == b'\n').count() + 1;
            format!("{}:{}: {}", input.path, line, String::from_utf8_lossy(e.message()))
        });
        if let Some(e) = &error {
            diagnostics.push(format!("Syntax error in {e}; the rest of the file is still shown."));
        }
        let mut l = lower::Lower::new(&input.text, &input.path, i, &consts, &primary, &requires, &comments);
        let body = l.program(&p.node());
        let docs = std::mem::take(&mut l.docs);
        lowered.push((body, comments, error, docs));
    }
    drop(parsed);
    let roots = roots(all_paths);
    let mut modules = vec![];
    let mut headers = vec![];
    for (input, (body, comments, error, docs)) in inputs.into_iter().zip(lowered) {
        let mut src = Source::new(input.text, input.file, &comments);
        src.lang = Lang::Ruby;
        src.mark_docs(&docs);
        headers.push(header(&src));
        let root = root_of(&input.path, &roots);
        let dotted = input.path.trim_end_matches(".rb").replace('/', ".");
        let package = input.path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
        let namespace = Some(format!("ruby\u{0}{root}"));
        modules.push(Module { file: input.file, path: input.path, dotted, package, src, ast: body, notebook: false, lang: Lang::Ruby, tags: vec![], error, namespace, asserts: vec![] });
    }
    (modules, headers, vec![])
}

/// Superclasses and mixins as written become full names where the project defines them.
fn resolve_ancestors(consts: &mut Consts) {
    let names: Vec<String> = consts.defs.keys().cloned().collect();
    for name in names {
        // A constant's nesting is its own path (`A::B::C` is written inside `A::B`).
        let parts: Vec<&str> = name.split("::").collect();
        let nesting: Vec<String> = (1..parts.len()).map(|k| parts[..k].join("::")).collect();
        let c = &consts.defs[&name];
        let superclass = c.superclass.as_ref().map(|s| consts.resolve(&nesting, s).unwrap_or_else(|| s.clone()));
        let mixins: Vec<String> = c.mixins.iter().map(|m| consts.resolve(&nesting, m).unwrap_or_else(|| m.clone())).collect();
        let c = consts.defs.get_mut(&name).unwrap();
        // A class is not its own superclass (`class Thor::Error < Error` at the top).
        c.superclass = superclass.filter(|s| *s != name);
        c.mixins = mixins;
    }
}

/// The comment block that opens a file, without magic comments and tool directives.
fn header(src: &Source) -> String {
    let mut lines = vec![];
    for line in src.text.lines() {
        let t = line.trim();
        if let Some(c) = t.strip_prefix('#') {
            let c = c.trim();
            let lower = c.to_ascii_lowercase();
            if t.starts_with("#!") || lower.starts_with("frozen_string_literal") || lower.starts_with("encoding:") || lower.starts_with("-*-") || lower.starts_with("typed:") || lower.starts_with("rubocop:") || lower.contains("copyright") || lower.contains("license") {
                continue;
            }
            lines.push(c.to_string());
        } else if t.is_empty() && lines.is_empty() {
            continue;
        } else {
            break;
        }
    }
    crate::text::summary(&lines.join(" "))
}

/// The name a constant node is written as: `Foo`, `A::B`, `::C`.
pub(crate) fn constant_text(n: &Node<'_>) -> Option<String> {
    if let Some(c) = n.as_constant_read_node() {
        return Some(String::from_utf8_lossy(c.name().as_slice()).into_owned());
    }
    let p = n.as_constant_path_node()?;
    let name = String::from_utf8_lossy(p.name()?.as_slice()).into_owned();
    match p.parent() {
        None => Some(format!("::{name}")),
        Some(parent) => Some(format!("{}::{name}", constant_text(&parent)?)),
    }
}
