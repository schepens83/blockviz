//! What Rails conventions make of a Ruby project: the role of each file by
//! where it lives (`app/controllers/` holds controllers, `config/routes.rb`
//! the routes), which classes are models, and the views an action renders.

/// The role of a file by its place in a Rails app or engine (whose `app/`
/// may be below the repository root), a gem's `bin/` or a Rake file.
pub fn file_role(path: &str) -> Option<&'static str> {
    let parts: Vec<&str> = path.split('/').collect();
    let base = *parts.last()?;
    if base == "Rakefile" || base.ends_with(".rake") {
        return Some("tasks");
    }
    if parts.len() >= 2 && parts[parts.len() - 2] == "bin" && !base.contains('.') {
        return Some("executable");
    }
    if let Some(i) = parts.iter().position(|p| *p == "app") {
        let inside = &parts[i + 1..parts.len() - 1];
        if inside.contains(&"concerns") {
            return Some("concern");
        }
        return Some(match *inside.first()? {
            "controllers" => "controller",
            "models" => "model",
            "jobs" => "job",
            "mailers" => "mailer",
            "helpers" => "helpers",
            "channels" => "channel",
            "mailboxes" => "mailbox",
            "services" => "service",
            "workers" => "worker",
            "policies" => "policy",
            "serializers" => "serializer",
            "validators" => "validator",
            "components" => "component",
            _ => return None,
        });
    }
    if let Some(i) = parts.iter().position(|p| *p == "config") {
        return Some(if parts.len() == i + 2 && base == "routes.rb" { "routes" } else { "config" });
    }
    if base == "seeds.rb" && parts.len() >= 2 && parts[parts.len() - 2] == "db" {
        return Some("script");
    }
    None
}

/// A class that descends from `ActiveRecord::Base`, through the project's
/// own classes (`ApplicationRecord`).
pub fn is_model(consts: &super::Consts, name: &str) -> bool {
    consts.ancestors(name).iter().any(|a| consts.defs[*a].superclass.as_deref().is_some_and(|s| matches!(s.trim_start_matches("::"), "ActiveRecord::Base")))
}

use crate::code::syntax::*;

/// What a class body declares with Rails' DSL, read for the class's
/// diagram: tables (associations, validations, callbacks, filters), the
/// roles the methods it names play (`before save`, `before every action`),
/// the models it belongs to, and scopes as the class methods they are.
#[derive(Default)]
pub struct Declarations {
    pub tables: Vec<(String, Vec<(String, String, String)>)>,
    /// (method, role) in the order declared.
    pub roles: Vec<(String, String)>,
    /// Classes the declarations name (`belongs_to :user` is `User`).
    pub uses: Vec<(String, TextRange)>,
    pub scopes: Vec<FunctionDef>,
}

const CALLBACKS: &[&str] = &[
    "before_validation", "after_validation", "before_save", "around_save", "after_save", "before_create", "around_create", "after_create", "before_update", "around_update", "after_update", "before_destroy", "around_destroy", "after_destroy", "after_commit", "after_rollback", "after_create_commit", "after_update_commit", "after_destroy_commit", "after_save_commit", "after_initialize", "after_find", "after_touch",
];

/// The order Active Record runs its callbacks in.
fn lifecycle(callback: &str) -> usize {
    ["after_initialize", "after_find", "before_validation", "after_validation", "before_save", "around_save", "before_create", "around_create", "before_update", "around_update", "before_destroy", "around_destroy", "after_create", "after_update", "after_destroy", "after_save", "after_touch", "after_commit", "after_create_commit", "after_update_commit", "after_destroy_commit", "after_save_commit", "after_rollback"].iter().position(|c| *c == callback).unwrap_or(usize::MAX)
}

const FILTERS: &[&str] = &["before_action", "after_action", "around_action", "prepend_before_action", "prepend_after_action", "prepend_around_action", "append_before_action", "append_after_action", "append_around_action"];

fn literal(e: &Expr) -> Option<&str> {
    match e {
        Expr::StringLiteral(s) => Some(&s.value),
        _ => None,
    }
}

fn names(e: &Expr) -> Vec<&str> {
    match e {
        Expr::List(l) => l.elts.iter().filter_map(literal).collect(),
        e => literal(e).into_iter().collect(),
    }
}

fn keyword<'e>(c: &'e ExprCall, name: &str) -> Option<&'e Expr> {
    c.arguments.keywords.iter().find(|k| k.arg.as_deref() == Some(name)).map(|k| &k.value)
}

fn text<'s>(src: &'s str, r: TextRange) -> &'s str {
    src.get(usize::from(r.start())..usize::from(r.end())).unwrap_or("")
}

/// `before_save` → `before save`; `after_create_commit` → `after commit on create`;
/// with `on: :create`, `if: :admin?` said after it.
fn callback_label(callback: &str, c: &ExprCall, src: &str) -> String {
    let mut label = match callback.strip_suffix("_commit").and_then(|r| r.strip_prefix("after_")) {
        Some(on) if on != "after" && !on.is_empty() => format!("after commit on {on}"),
        _ => callback.replace('_', " "),
    };
    if let Some(on) = keyword(c, "on") {
        label = format!("{label} on {}", names(on).join(", "));
    }
    for cond in ["if", "unless"] {
        if let Some(v) = keyword(c, cond) {
            label = format!("{label} · {cond} {}", text(src, v.range()).trim_start_matches(':'));
        }
    }
    label
}

/// `before_action` → `before`, with the actions it applies to.
fn filter_label(filter: &str, c: &ExprCall) -> String {
    let when = filter.trim_start_matches("prepend_").trim_start_matches("append_").trim_end_matches("_action");
    match (keyword(c, "only"), keyword(c, "except")) {
        (Some(only), _) => format!("{when} {}", names(only).join(", ")),
        (_, Some(except)) => format!("{when} every action but {}", names(except).join(", ")),
        _ => format!("{when} every action"),
    }
}

/// The calls a class body makes, and those inside `included do … end`.
fn calls(body: &[Stmt]) -> Vec<&ExprCall> {
    let mut out = vec![];
    for s in body {
        let Stmt::Expr(x) = s else { continue };
        let Expr::Call(c) = &*x.value else { continue };
        if matches!(&*c.func, Expr::Name(n) if matches!(n.id.as_str(), "included" | "class_methods")) {
            for a in &c.arguments.args {
                match a {
                    Expr::Function(f) => out.extend(calls(&f.body)),
                    Expr::Lambda(l) => {
                        if let Expr::Call(inner) = &*l.body {
                            out.push(inner);
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Reads a class body's declarations. `resolve` finds the class a model's
/// association names (`comments` is `Comment`, unless `class_name:` says).
pub fn declarations(body: &[Stmt], src: &str, resolve: impl Fn(&str) -> String) -> Declarations {
    let mut d = Declarations::default();
    let mut associations = vec![];
    let mut validations: Vec<(String, Vec<String>)> = vec![];
    let mut callbacks: Vec<(usize, String, String)> = vec![];
    let mut filters = vec![];
    for c in calls(body) {
        let Expr::Name(f) = &*c.func else { continue };
        let dsl = f.id.as_str();
        let args: Vec<&str> = c.arguments.args.iter().filter_map(literal).collect();
        match dsl {
            "belongs_to" | "has_many" | "has_one" | "has_and_belongs_to_many" => {
                let Some(name) = args.first() else { continue };
                let polymorphic = keyword(c, "polymorphic").is_some();
                let class = match keyword(c, "class_name").and_then(literal) {
                    Some(n) => resolve(n),
                    None if dsl == "has_many" || dsl == "has_and_belongs_to_many" => resolve(&super::routes::camelize(&super::routes::singularize(name))),
                    None => resolve(&super::routes::camelize(name)),
                };
                let mut notes = vec![];
                for k in ["through", "as", "dependent", "inverse_of", "foreign_key"] {
                    if let Some(v) = keyword(c, k).and_then(literal) {
                        notes.push(format!("{k}: {v}"));
                    }
                }
                if polymorphic {
                    notes.push("polymorphic".into());
                }
                if keyword(c, "as").is_some() {
                    notes.push("polymorphic".into());
                }
                // The model holding the foreign key depends on the one it belongs to.
                if dsl == "belongs_to" && !polymorphic {
                    d.uses.push((class.clone(), c.range));
                }
                let value = if polymorphic { dsl.to_string() } else { format!("{dsl} {}", class.rsplit("::").next().unwrap_or(&class)) };
                notes.dedup();
                associations.push((name.to_string(), value, notes.join(", ")));
            }
            "validates" | "validates_presence_of" | "validates_uniqueness_of" | "validates_length_of" | "validates_format_of" | "validates_inclusion_of" | "validates_numericality_of" | "validates_acceptance_of" | "validates_confirmation_of" => {
                let rules: Vec<String> = match dsl.strip_prefix("validates_").and_then(|r| r.strip_suffix("_of")) {
                    Some(rule) => vec![rule.to_string()],
                    None => c.arguments.keywords.iter().filter_map(|k| k.arg.clone()).filter(|k| !matches!(k.as_str(), "if" | "unless" | "on" | "allow_nil" | "allow_blank" | "message")).collect(),
                };
                for a in &args {
                    match validations.iter_mut().find(|(k, _)| k == a) {
                        Some((_, rs)) => rs.extend(rules.iter().filter(|r| !rs.contains(r)).cloned().collect::<Vec<_>>()),
                        None => validations.push((a.to_string(), rules.clone())),
                    }
                }
            }
            "validate" => {
                for a in &args {
                    d.roles.push((a.to_string(), "validation".into()));
                }
            }
            "scope" => {
                let (Some(name), Some(body)) = (args.first(), c.arguments.args.get(1)) else { continue };
                let (params, body) = match body {
                    Expr::Lambda(l) => (l.params.clone(), vec![Stmt::Return(StmtReturn { value: Some(l.body.clone()), range: l.body.range() })]),
                    Expr::Function(f) => (f.params.clone(), f.body.clone()),
                    _ => continue,
                };
                let mut all = vec![Param { name: "self".into(), kind: ParamKind::Positional, default: None, annotation: None, range: TextRange::empty(c.range.start()) }];
                all.extend(params);
                let at = TextRange::empty(c.range.start());
                let badge = Decorator { expression: Expr::Call(ExprCall { func: Box::new(Expr::Name(ExprName { id: "badge".into(), ctx: Ctx::Load, range: at })), arguments: Arguments { args: vec![Expr::StringLiteral(ExprStringLiteral { value: "scope".into(), range: at })], keywords: vec![] }, kind: CallKind::Call, range: at }), range: at };
                d.scopes.push(FunctionDef { name: name.to_string(), params: all, body, decorators: vec![badge], returns: None, is_async: false, is_generator: false, kind: DefKind::ClassMethod, doc: None, range: c.range });
            }
            _ if CALLBACKS.contains(&dsl) => {
                let label = callback_label(dsl, c, src);
                for a in &args {
                    d.roles.push((a.to_string(), label.clone()));
                    callbacks.push((lifecycle(dsl), label.clone(), a.to_string()));
                }
            }
            _ if FILTERS.contains(&dsl) => {
                let label = filter_label(dsl, c);
                for a in &args {
                    d.roles.push((a.to_string(), label.clone()));
                    filters.push((label.split(' ').next().unwrap_or("").to_string() + " " + a, label.split_once(' ').map(|(_, r)| r.to_string()).unwrap_or_default(), String::new()));
                }
            }
            "skip_before_action" | "skip_after_action" | "skip_around_action" => {
                let label = filter_label(&dsl.replacen("skip_", "", 1), c);
                for a in &args {
                    filters.push((format!("skip {a}"), label.split_once(' ').map(|(_, r)| r.to_string()).unwrap_or_default(), String::new()));
                }
            }
            _ => {}
        }
    }
    if !associations.is_empty() {
        d.tables.push(("associations".into(), associations));
    }
    if !validations.is_empty() {
        d.tables.push(("validations".into(), validations.into_iter().map(|(k, rs)| (k, rs.join(" · "), String::new())).collect()));
    }
    if !callbacks.is_empty() {
        callbacks.sort_by_key(|c| c.0);
        d.tables.push(("callbacks".into(), callbacks.into_iter().map(|(_, label, m)| (label, m, String::new())).collect()));
    }
    if !filters.is_empty() {
        d.tables.push(("filters".into(), filters));
    }
    // A method registered more than once plays a general role.
    let mut counted: Vec<(String, String)> = vec![];
    for (m, label) in std::mem::take(&mut d.roles) {
        match counted.iter_mut().find(|(n, _)| *n == m) {
            Some((_, l)) if *l != label => *l = label.split(' ').next().map(|w| if w == "validation" { "validation".to_string() } else { format!("{w} action") }).unwrap_or_default(),
            Some(_) => {}
            None => counted.push((m, label)),
        }
    }
    d.roles = counted;
    d
}

/// A role the front end gave a method (`role("before save")`).
pub fn role(d: &Expr) -> Option<&str> {
    let Expr::Call(c) = d else { return None };
    match (&*c.func, c.arguments.args.first()) {
        (Expr::Name(n), Some(Expr::StringLiteral(s))) if n.id == "role" => Some(&s.value),
        _ => None,
    }
}

/// A table the front end found in a class body (`table("associations", [(k, v, note)…])`).
pub fn table(d: &Expr) -> Option<(&str, Vec<(String, String, String)>)> {
    let Expr::Call(c) = d else { return None };
    let (Expr::Name(n), [Expr::StringLiteral(title), Expr::List(rows)]) = (&*c.func, &c.arguments.args[..]) else { return None };
    if n.id != "table" {
        return None;
    }
    let rows = rows
        .elts
        .iter()
        .filter_map(|r| match r {
            Expr::Tuple(t) => {
                let s: Vec<String> = t.elts.iter().filter_map(|e| literal(e).map(str::to_string)).collect();
                Some((s.first()?.clone(), s.get(1).cloned().unwrap_or_default(), s.get(2).cloned().unwrap_or_default()))
            }
            _ => None,
        })
        .collect();
    Some((&title.value, rows))
}

/// `role(label)` and `table(title, rows)` as decorators, for `role` and `table` to read.
pub fn role_decorator(label: &str, at: TextRange) -> Decorator {
    let at = TextRange::empty(at.start());
    Decorator { expression: Expr::Call(ExprCall { func: Box::new(Expr::Name(ExprName { id: "role".into(), ctx: Ctx::Load, range: at })), arguments: Arguments { args: vec![string(label, at)], keywords: vec![] }, kind: CallKind::Call, range: at }), range: at }
}

pub fn table_decorator(title: &str, rows: &[(String, String, String)], at: TextRange) -> Decorator {
    let at = TextRange::empty(at.start());
    let rows = rows.iter().map(|(k, v, n)| Expr::Tuple(ExprSeq { elts: vec![string(k, at), string(v, at), string(n, at)], range: at })).collect();
    let args = vec![string(title, at), Expr::List(ExprSeq { elts: rows, range: at })];
    Decorator { expression: Expr::Call(ExprCall { func: Box::new(Expr::Name(ExprName { id: "table".into(), ctx: Ctx::Load, range: at })), arguments: Arguments { args, keywords: vec![] }, kind: CallKind::Call, range: at }), range: at }
}

fn string(s: &str, at: TextRange) -> Expr {
    Expr::StringLiteral(ExprStringLiteral { value: s.to_string(), range: at })
}

/// The views an action of the controller at `path` (`mod/stories`) renders:
/// `render :new`, `render action: "new"`, `render "stories/form"`,
/// `render template: …`, and its own (`mod/stories/edit`), as paths under `app/views/`.
pub fn templates(path: &str, action: &str, body: &[Stmt]) -> Vec<String> {
    let mut out = vec![format!("{path}/{action}")];
    let mut exprs = vec![];
    crate::code::walk::stmt_exprs(body, true, &mut |e| exprs.push(e));
    for e in exprs {
        crate::code::walk::walk(e, &mut |x| {
            let Expr::Call(c) = x else { return };
            let render = match &*c.func {
                Expr::Name(n) => n.id == "render",
                Expr::Attribute(a) => a.attr == "render" && matches!(&*a.value, Expr::Name(n) if n.id == "self"),
                _ => false,
            };
            if !render {
                return;
            }
            let named = c.arguments.args.first().and_then(literal).map(str::to_string);
            let t = match (named, keyword(c, "action").and_then(literal), keyword(c, "template").and_then(literal)) {
                (_, _, Some(t)) => t.to_string(),
                (_, Some(a), _) => format!("{path}/{a}"),
                (Some(n), _, _) if n.contains('/') => n,
                (Some(n), _, _) => format!("{path}/{n}"),
                _ => return,
            };
            if !out.contains(&t) {
                out.push(t);
            }
        });
    }
    out
}

pub fn template_decorator(view: &str, at: TextRange) -> Decorator {
    let at = TextRange::empty(at.start());
    Decorator { expression: Expr::Call(ExprCall { func: Box::new(Expr::Name(ExprName { id: "template".into(), ctx: Ctx::Load, range: at })), arguments: Arguments { args: vec![string(view, at)], keywords: vec![] }, kind: CallKind::Call, range: at }), range: at }
}

/// A view the front end found an action renders (`template("stories/edit")`).
pub fn template(d: &Expr) -> Option<&str> {
    let Expr::Call(c) = d else { return None };
    match (&*c.func, c.arguments.args.first()) {
        (Expr::Name(n), Some(Expr::StringLiteral(s))) if n.id == "template" => Some(&s.value),
        _ => None,
    }
}

/// A view by its place in an app: a layout, a partial or a view.
pub fn view_role(path: &str) -> Option<&'static str> {
    let i = path.find("app/views/").map(|i| i + "app/views/".len()).or_else(|| path.starts_with("views/").then_some(6))?;
    let rest = &path[i..];
    let base = rest.rsplit('/').next().unwrap_or(rest);
    let ext = base.split_once('.').map(|(_, e)| e).unwrap_or("");
    if !["erb", "haml", "slim", "builder", "jbuilder", "rabl"].iter().any(|x| ext.ends_with(x)) {
        return None;
    }
    Some(if rest.starts_with("layouts/") {
        "layout"
    } else if base.starts_with('_') {
        "partial"
    } else {
        "view"
    })
}

/// A controller's response, titled by what the client gets: `render plain`,
/// `render new`, `redirect_to`, `head 404`, with what it says of it (the
/// status, where a redirect goes).
pub fn response(e: &Expr, src: &str) -> Option<(String, String)> {
    let Expr::Call(c) = e else { return None };
    let verb = match &*c.func {
        Expr::Name(n) => n.id.as_str(),
        Expr::Attribute(a) if matches!(&*a.value, Expr::Name(n) if n.id == "self") => a.attr.as_str(),
        _ => return None,
    };
    let code = |e: &Expr| crate::text::clip(text(src, e.range()).trim_start_matches(':'), 40);
    let status = keyword(c, "status").map(|s| format!("status {}", code(s))).unwrap_or_default();
    Some(match verb {
        "render" => {
            const FORMATS: &[&str] = &["plain", "json", "html", "xml", "js", "body", "inline", "turbo_stream", "file"];
            let what = match c.arguments.args.first().and_then(literal) {
                Some(view) => view.to_string(),
                None => match (keyword(c, "action").or_else(|| keyword(c, "template")).and_then(literal), keyword(c, "partial").and_then(literal)) {
                    (Some(v), _) => v.to_string(),
                    (None, Some(p)) => format!("partial {p}"),
                    _ => c.arguments.keywords.iter().filter_map(|k| k.arg.as_deref()).find(|k| FORMATS.contains(k)).unwrap_or("").to_string(),
                },
            };
            (if what.is_empty() { "render".into() } else { format!("render {what}") }, status)
        }
        "redirect_to" | "redirect_back" | "redirect_back_or_to" => {
            let to = c.arguments.args.first().map(code).or_else(|| keyword(c, "fallback_location").map(code)).unwrap_or_default();
            (verb.to_string(), [to, status].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "))
        }
        "head" => (c.arguments.args.first().map(|s| format!("head {}", code(s))).unwrap_or_else(|| "head".into()), String::new()),
        "send_data" | "send_file" | "respond_with" => (verb.to_string(), status),
        _ => return None,
    })
}
