//! `config/routes.rb`: Rails' routing DSL read the way `bin/rails routes`
//! prints it. `resources :stories` is the seven REST routes of
//! `StoriesController`, routes written inside it nest under `/:story_id`
//! (under `/:id` in `member do`), `namespace :mod` prefixes paths and
//! controllers, a singular `resource :retry` is served by
//! `RetriesController`, and an engine's routes are under its namespace.

use crate::code::syntax::*;

/// One route: `PATCH, PUT /stories/:id` → `stories#update`.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub verbs: Vec<String>,
    pub path: String,
    /// The controller as routes name it (`mod/stories`), or what a row leads
    /// to when no action serves it (`→ /upvoted/stories`, a mounted engine).
    pub controller: String,
    pub action: String,
    pub target: Option<String>,
}

impl Route {
    pub fn key(&self) -> String {
        if self.verbs.is_empty() {
            self.path.clone()
        } else {
            format!("{} {}", self.verbs.join(", "), self.path)
        }
    }

    pub fn value(&self) -> String {
        match &self.target {
            Some(t) => t.clone(),
            None => format!("{}#{}", self.controller, self.action),
        }
    }

    /// The controller class that serves it: `mod/stories` → `Mod::StoriesController`.
    pub fn controller_class(&self) -> Option<String> {
        if self.target.is_some() || self.controller.is_empty() {
            return None;
        }
        let parts: Vec<String> = self.controller.split('/').map(camelize).collect();
        Some(format!("{}Controller", parts.join("::")))
    }
}

/// The routes a block of the file draws, with the statement they come from.
pub struct Group<'p> {
    pub title: String,
    pub routes: Vec<Route>,
    pub first: &'p Stmt,
}

/// `stories` → `Stories`, `bulk_retries` → `BulkRetries`, `mission_control` → `MissionControl`.
pub fn camelize(s: &str) -> String {
    s.split('_').map(|w| {
        let mut c = w.chars();
        match c.next() {
            Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
            None => String::new(),
        }
    }).collect()
}

/// `stories` → `story`, `categories` → `category`, `statuses` → `status`.
pub fn singularize(s: &str) -> String {
    if let Some(stem) = s.strip_suffix("ies") {
        return format!("{stem}y");
    }
    for end in ["sses", "shes", "ches", "xes", "uses"] {
        if s.ends_with(end) {
            return s[..s.len() - 2].to_string();
        }
    }
    s.strip_suffix('s').filter(|t| !t.ends_with('s')).unwrap_or(s).to_string()
}

/// `retry` → `retries`, `pause` → `pauses`; a plural stays one.
pub fn pluralize(s: &str) -> String {
    if s.ends_with('s') {
        return s.to_string();
    }
    if let Some(stem) = s.strip_suffix('y').filter(|t| !t.ends_with(['a', 'e', 'i', 'o', 'u'])) {
        return format!("{stem}ies");
    }
    if s.ends_with('x') || s.ends_with("ch") || s.ends_with("sh") {
        return format!("{s}es");
    }
    format!("{s}s")
}

/// `x.routes.draw do … end`: the block's statements, and the controller
/// namespace an engine's routes are under (`MissionControl::Jobs::Engine`
/// routes to `mission_control/jobs/…`).
pub fn draw_block<'p>(s: &'p Stmt, src: &str) -> Option<(Vec<(&'p Stmt, &'p Expr)>, String)> {
    let e = match s {
        Stmt::Expr(x) => &*x.value,
        _ => return None,
    };
    let Expr::Call(c) = e else { return None };
    let Expr::Attribute(draw) = &*c.func else { return None };
    // `Rails.application.routes` (calls without parentheses) or `Engine.routes`.
    let routes = match &*draw.value {
        Expr::Call(r) if r.arguments.args.is_empty() => &*r.func,
        e => e,
    };
    let Expr::Attribute(routes) = routes else { return None };
    if draw.attr != "draw" || routes.attr != "routes" {
        return None;
    }
    let block = c.arguments.args.last()?;
    let owner = text(src, &*routes.value);
    let module = match owner.strip_suffix("::Engine") {
        Some(ns) => format!("{}/", super::underscore(ns)),
        None => String::new(),
    };
    let items = match block {
        Expr::Function(f) => f.body.iter().filter_map(|st| stmt_expr(st).map(|e| (st, e))).collect(),
        Expr::Lambda(l) => vec![(s, &*l.body)],
        _ => vec![],
    };
    Some((items, module))
}

fn stmt_expr(s: &Stmt) -> Option<&Expr> {
    match s {
        Stmt::Expr(x) => Some(&*x.value),
        Stmt::Return(r) => r.value.as_deref(),
        _ => None,
    }
}

fn text<'s>(src: &'s str, e: &impl Ranged) -> &'s str {
    let r = e.range();
    src.get(usize::from(r.start())..usize::from(r.end())).unwrap_or("")
}

/// The expressions a block runs, in order (a one-statement block is a lambda).
fn statements(block: &Expr) -> Vec<&Expr> {
    match block {
        Expr::Function(f) => exprs(&f.body),
        Expr::Lambda(l) => vec![&*l.body],
        _ => vec![],
    }
}

fn exprs(body: &[Stmt]) -> Vec<&Expr> {
    body.iter().filter_map(stmt_expr).collect()
}

/// Where a route's controller, path and nesting come from.
#[derive(Clone, Default)]
struct Scope {
    path: String,
    module: String,
    /// The resource routes inside it belong to: its controller and the
    /// paths of its members and collection.
    controller: Option<String>,
    nested: String,
    member: String,
    collection: String,
}

fn join(prefix: &str, path: &str) -> String {
    let path = path.trim_start_matches('/');
    let joined = if path.is_empty() { prefix.to_string() } else { format!("{}/{}", prefix.trim_end_matches('/'), path) };
    let joined = joined.replace("(.:format)", "");
    if joined.starts_with('/') {
        joined
    } else {
        format!("/{joined}")
    }
}

fn literal(e: &Expr) -> Option<String> {
    match e {
        Expr::StringLiteral(s) => Some(s.value.clone()),
        _ => None,
    }
}

fn list(e: &Expr) -> Vec<String> {
    match e {
        Expr::List(l) => l.elts.iter().filter_map(literal).collect(),
        e => literal(e).into_iter().collect(),
    }
}

fn keyword<'e>(c: &'e ExprCall, name: &str) -> Option<&'e Expr> {
    c.arguments.keywords.iter().find(|k| k.arg.as_deref() == Some(name)).map(|k| &k.value)
}

fn call_name(c: &ExprCall) -> Option<&str> {
    match &*c.func {
        Expr::Name(n) => Some(n.id.as_str()),
        _ => None,
    }
}

/// The groups of routes a `draw` block holds: one per top-level resource,
/// namespace, scope or constraints block, and one per run of routes
/// written one after another.
pub fn groups<'p>(items: &[(&'p Stmt, &'p Expr)], module: &str, src: &str) -> Vec<Group<'p>> {
    let scope = Scope { module: module.to_string(), ..Scope::default() };
    let mut out: Vec<Group<'p>> = vec![];
    let mut run: Vec<Route> = vec![];
    let mut run_first: Option<&'p Stmt> = None;
    let mut last_line_end = 0usize;
    let line = |at: TextSize| src[..usize::from(at).min(src.len())].matches('\n').count();
    let flush = |run: &mut Vec<Route>, first: &mut Option<&'p Stmt>, out: &mut Vec<Group<'p>>| {
        if let (false, Some(f)) = (run.is_empty(), *first) {
            let controllers: Vec<&str> = run.iter().filter(|r| r.target.is_none()).map(|r| r.controller.as_str()).collect();
            let title = match controllers.first() {
                Some(c) if controllers.iter().all(|x| x == c) => c.rsplit('/').next().unwrap_or(c).to_string(),
                _ => "routes".to_string(),
            };
            // Runs one after another for one controller are one table.
            match out.last_mut() {
                Some(g) if g.title == title && !title.contains(' ') => g.routes.append(run),
                _ => out.push(Group { title, routes: std::mem::take(run), first: f }),
            }
        }
        *first = None;
    };
    for &(s, e) in items {
        let Expr::Call(c) = e else { continue };
        let Some(name) = call_name(c) else { continue };
        let start = line(e.range().start());
        let mut found = vec![];
        visit(e, &scope, &mut found);
        if found.is_empty() {
            continue;
        }
        if matches!(name, "resources" | "resource" | "namespace" | "scope" | "constraints" | "concern" | "defaults") {
            flush(&mut run, &mut run_first, &mut out);
            // Titled as written, up to its block: `resources :stories`, `constraints id: DOMAINS_IDENTIFIER`.
            let head = c.arguments.args.iter().find(|a| !matches!(a, Expr::Function(_) | Expr::Lambda(_))).map(|a| text(src, a)).or_else(|| c.arguments.keywords.first().map(|k| text(src, k))).unwrap_or("");
            let title = if head.is_empty() { name.to_string() } else { format!("{name} {head}") };
            out.push(Group { title, routes: found, first: s });
        } else {
            // A blank line between routes starts another run.
            if !run.is_empty() && start > last_line_end + 1 {
                flush(&mut run, &mut run_first, &mut out);
            }
            run_first.get_or_insert(s);
            run.extend(found);
        }
        last_line_end = line(e.range().end());
    }
    flush(&mut run, &mut run_first, &mut out);
    out
}

/// Every route a `draw` block draws, with paths and controllers as `bin/rails routes` prints them.
pub fn all(items: &[(&Stmt, &Expr)], module: &str) -> Vec<Route> {
    let scope = Scope { module: module.to_string(), ..Scope::default() };
    let mut out = vec![];
    for (_, e) in items {
        visit(e, &scope, &mut out);
    }
    out
}

const REST: &[&str] = &["index", "create", "new", "edit", "show", "update", "destroy"];

fn visit(e: &Expr, scope: &Scope, out: &mut Vec<Route>) {
    let Expr::Call(c) = e else { return };
    let Some(name) = call_name(c) else { return };
    let block = c.arguments.args.iter().find(|a| matches!(a, Expr::Function(_) | Expr::Lambda(_)));
    let first = c.arguments.args.first().and_then(literal);
    let inner = |s: &Scope, out: &mut Vec<Route>| {
        if let Some(b) = block {
            for x in statements(b) {
                visit(x, s, out);
            }
        }
    };
    match name {
        "resources" | "resource" => {
            let Some(res) = first else { return };
            let singular = name == "resource";
            let controller = keyword(c, "controller").and_then(literal).unwrap_or_else(|| if singular { pluralize(&res) } else { res.clone() });
            let controller = format!("{}{}", keyword(c, "module").and_then(literal).map(|m| format!("{m}/")).unwrap_or_else(|| scope.module.clone()), controller);
            let segment = keyword(c, "path").and_then(literal).unwrap_or_else(|| res.clone());
            let base = join(&current(scope), &segment);
            let param = keyword(c, "param").and_then(literal).unwrap_or_else(|| "id".into());
            let id = if singular { base.clone() } else { join(&base, &format!(":{param}")) };
            let mut actions: Vec<&str> = REST.iter().copied().filter(|a| !(singular && *a == "index")).collect();
            if let Some(only) = keyword(c, "only") {
                let only = list(only);
                actions.retain(|a| only.iter().any(|o| o == a));
            }
            if let Some(except) = keyword(c, "except") {
                let except = list(except);
                actions.retain(|a| !except.iter().any(|o| o == a));
            }
            for a in actions {
                let (verbs, path): (&[&str], String) = match a {
                    "index" => (&["GET"], base.clone()),
                    "create" => (&["POST"], base.clone()),
                    "new" => (&["GET"], join(&base, "new")),
                    "edit" => (&["GET"], join(&id, "edit")),
                    "show" => (&["GET"], id.clone()),
                    "update" => (&["PATCH", "PUT"], id.clone()),
                    _ => (&["DELETE"], id.clone()),
                };
                out.push(Route { verbs: verbs.iter().map(|v| v.to_string()).collect(), path, controller: controller.clone(), action: a.into(), target: None });
            }
            let nested = if singular { base.clone() } else { join(&base, &format!(":{}_id", singularize(&res))) };
            let s = Scope { path: scope.path.clone(), module: scope.module.clone(), controller: Some(controller), nested: nested.clone(), member: id, collection: base };
            inner(&s, out);
        }
        "namespace" => {
            let Some(ns) = first else { return };
            let path = keyword(c, "path").and_then(literal).unwrap_or_else(|| ns.clone());
            let module = keyword(c, "module").and_then(literal).unwrap_or_else(|| ns.clone());
            let s = Scope { path: join(&current(scope), &path), module: format!("{}{module}/", scope.module), ..Scope::default() };
            inner(&s, out);
        }
        "scope" => {
            let path = first.or_else(|| keyword(c, "path").and_then(literal)).unwrap_or_default();
            let mut s = scope.clone();
            if !path.is_empty() {
                let p = join(&current(scope), &path);
                if s.controller.is_some() {
                    s.nested = p;
                } else {
                    s.path = p;
                }
            }
            if let Some(m) = keyword(c, "module").and_then(literal) {
                s.module = format!("{}{m}/", scope.module);
            }
            inner(&s, out);
        }
        "member" | "collection" => {
            let mut s = scope.clone();
            s.nested = if name == "member" { scope.member.clone() } else { scope.collection.clone() };
            inner(&s, out);
        }
        "constraints" | "defaults" | "concern" | "with_options" => inner(scope, out),
        "root" => {
            let to = first.or_else(|| keyword(c, "to").and_then(literal)).unwrap_or_default();
            if let Some((controller, action)) = to.split_once('#') {
                out.push(Route { verbs: vec!["GET".into()], path: join(&current(scope), ""), controller: format!("{}{controller}", scope.module), action: action.into(), target: None });
            }
        }
        "mount" => {
            // `mount Engine, at: "/jobs"` or `mount Engine => "/jobs"`.
            let (target, at) = match (c.arguments.args.first(), keyword(c, "at").and_then(literal)) {
                (Some(e), Some(at)) => (dotted(e), at),
                _ => match c.arguments.keywords.first() {
                    Some(k) if k.arg.is_some() => (k.arg.clone().unwrap_or_default(), literal(&k.value).unwrap_or_default()),
                    _ => return,
                },
            };
            out.push(Route { verbs: vec![], path: join(&current(scope), &at), controller: String::new(), action: String::new(), target: Some(target) });
        }
        "get" | "post" | "patch" | "put" | "delete" | "match" | "options" | "head" => verb(c, name, first, scope, out),
        _ => {}
    }
}

fn dotted(e: &Expr) -> String {
    match e {
        Expr::Name(n) => n.id.clone(),
        Expr::Attribute(a) => format!("{}::{}", dotted(&a.value), a.attr),
        _ => String::new(),
    }
}

/// Where routes written here go: inside a resource, under its id.
fn current(scope: &Scope) -> String {
    if scope.controller.is_some() {
        scope.nested.clone()
    } else {
        scope.path.clone()
    }
}

fn verb(c: &ExprCall, name: &str, first: Option<String>, scope: &Scope, out: &mut Vec<Route>) {
    // `get "/login" => "login#index"`: the path is a hash key.
    let (path, mut to) = match &first {
        Some(p) => (p.clone(), keyword(c, "to").and_then(literal)),
        None => match c.arguments.keywords.first() {
            Some(k) if k.arg.as_deref().is_some_and(|a| !matches!(a, "to" | "as" | "via" | "format" | "on" | "constraints" | "defaults" | "controller" | "action")) => (k.arg.clone().unwrap_or_default(), literal(&k.value)),
            _ => return,
        },
    };
    // `to: redirect("/x")` is a redirect, not an action.
    let redirect = keyword(c, "to").and_then(|t| match t {
        Expr::Call(r) if call_name(r) == Some("redirect") => Some(r.arguments.args.first().and_then(literal).unwrap_or_default()),
        _ => None,
    });
    let verbs: Vec<String> = if name == "match" {
        keyword(c, "via").map(list).unwrap_or_default().into_iter().map(|v| if v == "all" { "ANY".into() } else { v.to_ascii_uppercase() }).collect()
    } else {
        vec![name.to_ascii_uppercase()]
    };
    let full = join(&current(scope), &path);
    if let Some(r) = redirect {
        out.push(Route { verbs, path: full, controller: String::new(), action: String::new(), target: Some(format!("→ {r}")) });
        return;
    }
    if to.is_none() {
        if let (Some(cn), Some(a)) = (keyword(c, "controller").and_then(literal), keyword(c, "action").and_then(literal)) {
            to = Some(format!("{cn}#{a}"));
        }
    }
    let (controller, action) = match to.as_deref().and_then(|t| t.split_once('#')) {
        Some((cn, a)) => (format!("{}{cn}", scope.module), a.to_string()),
        None => {
            let segment = path.trim_matches('/');
            match (&scope.controller, keyword(c, "action").and_then(literal)) {
                // Inside a resource, `post :upvote` is its `upvote`.
                (Some(cn), action) => (cn.clone(), action.unwrap_or_else(|| segment.rsplit('/').next().unwrap_or(segment).to_string())),
                // `post "/stories/fetch_url_attributes"` is `stories#fetch_url_attributes`.
                (None, _) => match segment.rsplit_once('/') {
                    Some((cn, a)) if !a.starts_with(':') && !cn.contains(':') => (format!("{}{cn}", scope.module), a.to_string()),
                    _ => return,
                },
            }
        }
    };
    out.push(Route { verbs, path: full, controller, action, target: None });
}
