//! Prism's tree → the shared syntax tree.
//!
//! Ruby says with conventions what other languages say with syntax, and the
//! lowering reads them: a method without a receiver called in a class is a
//! method of the class (`authenticate` is `self.authenticate` when the class
//! or an ancestor defines it), `Foo.new(…)` constructs a `Foo`, `xs.each do
//! |x| … end` is a loop over `xs`, `@user` is a field of `self`, a method's
//! last expression is its result, `unless` is `if not`, and a modifier `if`
//! guards one statement. Nested classes and modules are known by their full
//! names (`Admin::UsersController`); a module that only holds other
//! definitions is a namespace, not a block.

use super::{constant_text, Const, Consts, Requires};
use crate::code::syntax::*;
use crate::hash::Map;
use ruby_prism::{CallNode, ConstantId, Location, Node, ParametersNode, StatementsNode};

fn text(id: &ConstantId<'_>) -> String {
    String::from_utf8_lossy(id.as_slice()).into_owned()
}

fn range(loc: &Location<'_>) -> TextRange {
    TextRange::new(loc.start().into(), loc.end().into())
}

fn nrange(n: &Node<'_>) -> TextRange {
    range(&n.location())
}

fn name(id: impl Into<String>, range: TextRange) -> Expr {
    Expr::Name(ExprName { id: id.into(), ctx: Ctx::Load, range })
}

fn store(e: Expr) -> Expr {
    match e {
        Expr::Name(mut n) => {
            n.ctx = Ctx::Store;
            Expr::Name(n)
        }
        Expr::Tuple(mut t) => {
            t.elts = t.elts.into_iter().map(store).collect();
            Expr::Tuple(t)
        }
        e => e,
    }
}

fn other(range: TextRange) -> Expr {
    Expr::Other(Atom { range })
}

/// `self.<name>`, written as `@name` (or as a call without a receiver) at `range`.
fn on_self(attr: &str, range: TextRange) -> Expr {
    Expr::Attribute(ExprAttribute { value: Box::new(name("self", TextRange::empty(range.start()))), attr: attr.to_string(), range })
}

/// Calls whose value is not what a method that ends with them returns:
/// they respond, print, raise or declare.
const COMMANDS: &[&str] = &[
    "render", "redirect_to", "redirect_back", "redirect_back_or_to", "head", "respond_to", "respond_with", "send_data", "send_file", "puts", "print", "p", "pp", "warn", "raise", "fail", "exit", "exit!", "abort", "sleep", "require", "require_relative", "load", "attr_accessor", "attr_reader", "attr_writer", "include", "extend", "prepend", "private", "protected", "public", "say", "say_status",
];

/// Methods that run their block once per item.
const EACH: &[&str] = &["each", "each_with_index", "each_pair", "each_key", "each_value", "find_each", "each_slice", "each_cons", "each_char", "each_line", "reverse_each", "each_entry", "each_with_object", "find_in_batches", "in_batches"];

/// Where a statement's value goes.
#[derive(Clone)]
enum Dest {
    /// Nowhere: a statement.
    None,
    /// The method's result (its last expression).
    Return,
    /// Assigned (`x = if … end`).
    Assign(Expr),
}

/// The class or module whose body the lowering is in.
struct Scope {
    name: String,
    /// In `class << self` or `def self.x`.
    singleton: bool,
}

pub struct Lower<'a> {
    src: &'a str,
    path: &'a str,
    input: usize,
    consts: &'a Consts,
    primary: &'a Map<String, usize>,
    requires: &'a Requires<'a>,
    /// Comment lines by start offset, for documentation above definitions.
    comments: Vec<(u32, u32, String)>,
    /// Lexical module nesting, outermost first.
    nesting: Vec<String>,
    scope: Vec<Scope>,
    /// The method being lowered: its name and whether it keeps its last value.
    method: Vec<String>,
    /// Comment offsets that document a definition.
    pub docs: Vec<usize>,
}

impl<'a> Lower<'a> {
    pub fn new(src: &'a str, path: &'a str, input: usize, consts: &'a Consts, primary: &'a Map<String, usize>, requires: &'a Requires<'a>, comments: &[(TextRange, String)]) -> Self {
        let comments = comments.iter().map(|(r, t)| (u32::from(r.start()), u32::from(r.end()), t.clone())).collect();
        Lower { src, path, input, consts, primary, requires, comments, nesting: vec![], scope: vec![], method: vec![], docs: vec![] }
    }

    fn of(&self, r: TextRange) -> &str {
        self.src.get(usize::from(r.start())..usize::from(r.end())).unwrap_or("")
    }

    pub fn program(&mut self, root: &Node<'_>) -> Vec<Stmt> {
        let Some(p) = root.as_program_node() else { return vec![] };
        self.statements(Some(p.statements()), &Dest::None)
    }

    // ----- statements ---------------------------------------------------------------

    fn statements(&mut self, s: Option<StatementsNode<'_>>, dest: &Dest) -> Vec<Stmt> {
        let Some(s) = s else { return vec![] };
        let nodes: Vec<Node<'_>> = s.body().iter().collect();
        let mut out = vec![];
        for (i, n) in nodes.iter().enumerate() {
            let last = i + 1 == nodes.len();
            out.extend(self.stmt(n, if last { dest } else { &Dest::None }));
        }
        out
    }

    /// A node that holds statements: a body, or `begin … rescue … end`.
    fn body(&mut self, n: Option<Node<'_>>, dest: &Dest) -> Vec<Stmt> {
        match n {
            None => vec![],
            Some(n) => match n.as_statements_node() {
                Some(s) => self.statements(Some(s), dest),
                None => self.stmt(&n, dest),
            },
        }
    }

    fn stmt(&mut self, n: &Node<'_>, dest: &Dest) -> Vec<Stmt> {
        let r = nrange(n);
        match n {
            Node::ClassNode { .. } | Node::ModuleNode { .. } => self.class(n),
            // `class << Rails.application`: methods defined on one object.
            Node::SingletonClassNode { .. } => {
                let s = n.as_singleton_class_node().unwrap();
                let owner = self.of(nrange(&s.expression())).to_string();
                self.scope.push(Scope { name: owner.clone(), singleton: true });
                let nodes: Vec<Node<'_>> = match s.body() {
                    Some(b) => match b.as_statements_node() {
                        Some(st) => st.body().iter().collect(),
                        None => vec![b],
                    },
                    None => vec![],
                };
                let mut body = vec![];
                for i in &nodes {
                    match i {
                        Node::DefNode { .. } => body.push(Stmt::FunctionDef(self.def(&i.as_def_node().unwrap(), true))),
                        _ => body.extend(self.stmt(i, &Dest::None)),
                    }
                }
                self.scope.pop();
                let doc = self.doc_above(s.class_keyword_loc().start());
                vec![Stmt::ClassDef(ClassDef { name: owner, bases: vec![], keywords: vec![], body, decorators: vec![], kind: ClassKind::Object, doc, range: r })]
            }
            Node::DefNode { .. } => vec![Stmt::FunctionDef(self.def(&n.as_def_node().unwrap(), false))],
            Node::IfNode { .. } => self.if_stmt(n, dest),
            Node::UnlessNode { .. } => {
                let u = n.as_unless_node().unwrap();
                let test = self.not(u.predicate());
                let body = self.statements(u.statements(), dest);
                let orelse = u.else_clause().map(|e| self.statements(e.statements(), dest)).unwrap_or_default();
                vec![if_(test, body, orelse, r)]
            }
            Node::CaseNode { .. } => self.case(n, dest),
            Node::CaseMatchNode { .. } => {
                let c = n.as_case_match_node().unwrap();
                let subject = c.predicate().map(|p| self.expr(&p)).unwrap_or_else(|| other(r));
                let mut cases = vec![];
                for cond in c.conditions().iter() {
                    if let Some(i) = cond.as_in_node() {
                        let body = self.statements(i.statements(), dest);
                        cases.push(MatchCase { patterns: vec![nrange(&i.pattern())], binds: None, variant: None, guard: None, body, range: nrange(&cond) });
                    }
                }
                if let Some(e) = c.else_clause() {
                    let body = self.statements(e.statements(), dest);
                    cases.push(MatchCase { patterns: vec![], binds: None, variant: None, guard: None, body, range: range(&e.location()) });
                }
                vec![Stmt::Match(StmtMatch { subject: Box::new(subject), cases, range: r })]
            }
            Node::WhileNode { .. } => {
                let w = n.as_while_node().unwrap();
                let test = self.expr(&w.predicate());
                let body = self.statements(w.statements(), &Dest::None);
                vec![Stmt::While(StmtWhile { test: Box::new(test), body, orelse: vec![], range: r })]
            }
            Node::UntilNode { .. } => {
                let w = n.as_until_node().unwrap();
                let test = self.not(w.predicate());
                let body = self.statements(w.statements(), &Dest::None);
                vec![Stmt::While(StmtWhile { test: Box::new(test), body, orelse: vec![], range: r })]
            }
            Node::ForNode { .. } => {
                let f = n.as_for_node().unwrap();
                let target = store(self.target(&f.index()));
                let iter = self.expr(&f.collection());
                let body = self.statements(f.statements(), &Dest::None);
                vec![Stmt::For(StmtFor { target: Box::new(target), iter: Box::new(iter), body, orelse: vec![], kind: ForKind::In, is_async: false, range: r })]
            }
            Node::BeginNode { .. } => self.begin(n, dest),
            Node::RescueModifierNode { .. } => {
                let m = n.as_rescue_modifier_node().unwrap();
                let body = self.stmt(&m.expression(), dest);
                let handler = self.stmt(&m.rescue_expression(), dest);
                vec![Stmt::Try(StmtTry { body, handlers: vec![ExceptHandler { type_: None, name: None, body: handler, range: range(&m.keyword_loc()) }], orelse: vec![], finalbody: vec![], range: r })]
            }
            Node::ReturnNode { .. } => {
                let x = n.as_return_node().unwrap();
                let value = x.arguments().map(|a| self.values(a.arguments().iter().collect(), r));
                vec![Stmt::Return(StmtReturn { value: value.map(Box::new), range: r })]
            }
            Node::BreakNode { .. } => vec![Stmt::Break(Atom { range: r })],
            Node::NextNode { .. } => vec![Stmt::Continue(Atom { range: r })],
            Node::RetryNode { .. } | Node::RedoNode { .. } | Node::AliasMethodNode { .. } | Node::AliasGlobalVariableNode { .. } | Node::UndefNode { .. } => vec![Stmt::Other(Atom { range: r })],
            Node::ParenthesesNode { .. } => {
                let p = n.as_parentheses_node().unwrap();
                self.body(p.body(), dest)
            }
            Node::CallNode { .. } => self.call_stmt(&n.as_call_node().unwrap(), dest),
            _ if is_write(n) => self.write(n),
            _ => {
                let e = self.expr(n);
                self.value(e, dest, r)
            }
        }
    }

    /// An expression where `dest` says its value goes.
    fn value(&mut self, e: Expr, dest: &Dest, r: TextRange) -> Vec<Stmt> {
        // In a controller, a response is the action's way out, wherever it is.
        if self.responds() && super::rails::response(&e, self.src).is_some() && !matches!(dest, Dest::Assign(_)) {
            return vec![Stmt::Return(StmtReturn { value: Some(Box::new(e)), range: r })];
        }
        match dest {
            Dest::None => vec![Stmt::Expr(StmtExpr { value: Box::new(e), range: r })],
            Dest::Return if self.command(&e) => vec![Stmt::Expr(StmtExpr { value: Box::new(e), range: r })],
            Dest::Return => vec![Stmt::Return(StmtReturn { value: Some(Box::new(e)), range: r })],
            Dest::Assign(t) => vec![Stmt::Assign(StmtAssign { targets: vec![t.clone()], value: Box::new(e), range: r })],
        }
    }

    /// In a method of a controller (or a concern of one), where `render` responds.
    fn responds(&self) -> bool {
        !self.method.is_empty() && self.scope.last().is_some_and(|s| s.name.ends_with("Controller") || self.path.contains("controllers/"))
    }

    /// A call whose value the method ending with it does not return.
    fn command(&self, e: &Expr) -> bool {
        match e {
            Expr::Call(c) => match &*c.func {
                Expr::Name(n) => COMMANDS.contains(&n.id.as_str()),
                Expr::Attribute(a) => matches!(&*a.value, Expr::Name(n) if n.id == "self" && COMMANDS.contains(&a.attr.as_str())),
                _ => false,
            },
            Expr::NoneLiteral(_) => true,
            _ => false,
        }
    }

    fn if_stmt(&mut self, n: &Node<'_>, dest: &Dest) -> Vec<Stmt> {
        let x = n.as_if_node().unwrap();
        let r = nrange(n);
        // `c ? a : b` is a value.
        if x.if_keyword_loc().is_none() {
            let e = self.expr(n);
            return self.value(e, dest, r);
        }
        let test = self.expr(&x.predicate());
        let body = self.statements(x.statements(), dest);
        let mut clauses = vec![];
        let mut next = x.subsequent();
        while let Some(s) = next {
            if let Some(elsif) = s.as_if_node() {
                let test = self.expr(&elsif.predicate());
                let body = self.statements(elsif.statements(), dest);
                clauses.push(ElifElseClause { test: Some(test), body, range: nrange(&s) });
                next = elsif.subsequent();
            } else if let Some(e) = s.as_else_node() {
                let body = self.statements(e.statements(), dest);
                clauses.push(ElifElseClause { test: None, body, range: nrange(&s) });
                next = None;
            } else {
                next = None;
            }
        }
        vec![Stmt::If(StmtIf { test: Box::new(test), body, elif_else_clauses: clauses, range: r })]
    }

    fn case(&mut self, n: &Node<'_>, dest: &Dest) -> Vec<Stmt> {
        let c = n.as_case_node().unwrap();
        let r = nrange(n);
        let whens: Vec<_> = c.conditions().iter().filter_map(|w| w.as_when_node()).collect();
        let Some(subject) = c.predicate() else {
            // `case` without a subject is an `if`/`elsif` chain.
            let mut tests_bodies = vec![];
            for w in &whens {
                let conds: Vec<Expr> = w.conditions().iter().map(|c| self.expr(&c)).collect();
                let test = if conds.len() == 1 { conds.into_iter().next().unwrap() } else { Expr::BoolOp(ExprBoolOp { op: BoolOp::OrOr, values: conds, range: range(&w.location()) }) };
                tests_bodies.push((test, self.statements(w.statements(), dest), range(&w.location())));
            }
            let orelse = c.else_clause().map(|e| (self.statements(e.statements(), dest), range(&e.location())));
            let mut it = tests_bodies.into_iter();
            let Some((test, body, _)) = it.next() else { return orelse.map(|(b, _)| b).unwrap_or_default() };
            let mut clauses: Vec<ElifElseClause> = it.map(|(t, b, r)| ElifElseClause { test: Some(t), body: b, range: r }).collect();
            if let Some((b, r)) = orelse {
                clauses.push(ElifElseClause { test: None, body: b, range: r });
            }
            return vec![Stmt::If(StmtIf { test: Box::new(test), body, elif_else_clauses: clauses, range: r })];
        };
        let subject = self.expr(&subject);
        let mut cases = vec![];
        for w in whens {
            let patterns = w.conditions().iter().map(|c| nrange(&c)).collect();
            let body = self.statements(w.statements(), dest);
            cases.push(MatchCase { patterns, binds: None, variant: None, guard: None, body, range: range(&w.location()) });
        }
        if let Some(e) = c.else_clause() {
            let body = self.statements(e.statements(), dest);
            cases.push(MatchCase { patterns: vec![], binds: None, variant: None, guard: None, body, range: range(&e.location()) });
        }
        vec![Stmt::Match(StmtMatch { subject: Box::new(subject), cases, range: r })]
    }

    fn begin(&mut self, n: &Node<'_>, dest: &Dest) -> Vec<Stmt> {
        let b = n.as_begin_node().unwrap();
        let r = nrange(n);
        let body = self.statements(b.statements(), if b.else_clause().is_some() { &Dest::None } else { dest });
        if b.rescue_clause().is_none() && b.ensure_clause().is_none() {
            return body;
        }
        let mut handlers = vec![];
        let mut next = b.rescue_clause();
        while let Some(rc) = next {
            let types: Vec<Expr> = rc.exceptions().iter().map(|e| self.expr(&e)).collect();
            let type_ = match types.len() {
                0 => None,
                1 => types.into_iter().next(),
                _ => Some(Expr::Tuple(ExprSeq { elts: types, range: range(&rc.keyword_loc()) })),
            };
            let bound = rc.reference().and_then(|v| v.as_local_variable_target_node().map(|t| text(&t.name())));
            let body = self.statements(rc.statements(), dest);
            handlers.push(ExceptHandler { type_: type_.map(Box::new), name: bound, body, range: range(&rc.location()) });
            next = rc.subsequent();
        }
        let orelse = b.else_clause().map(|e| self.statements(e.statements(), dest)).unwrap_or_default();
        let finalbody = b.ensure_clause().map(|e| self.statements(e.statements(), &Dest::None)).unwrap_or_default();
        vec![Stmt::Try(StmtTry { body, handlers, orelse, finalbody, range: r })]
    }

    /// Calls that are statements of their own: requires, raises, loops
    /// written as blocks, visibility and mixin declarations.
    fn call_stmt(&mut self, c: &CallNode<'_>, dest: &Dest) -> Vec<Stmt> {
        let r = range(&c.location());
        let method = text(&c.name());
        let args: Vec<Node<'_>> = c.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        if c.receiver().is_none() {
            match method.as_str() {
                "require" | "require_relative" | "load" | "autoload" => {
                    let spec = args.iter().rev().find_map(string_value);
                    if let Some(spec) = spec {
                        let resolved = self.requires.resolve(self.path, &spec, method == "require_relative");
                        return vec![Stmt::Import(StmtImport { names: vec![Alias { name: spec, asname: Some("_".into()), range: r }], style: ModuleStyle::Path, resolved, declaration: false, range: r })];
                    }
                }
                "raise" | "fail" => {
                    let exc = match &args[..] {
                        [] => None,
                        [e] => Some(self.expr(e)),
                        // `raise Error, "message"` raises an `Error` with the message.
                        [cls, rest @ ..] => {
                            let func = self.expr(cls);
                            let args = rest.iter().map(|a| self.expr(a)).collect();
                            Some(Expr::Call(ExprCall { func: Box::new(func), arguments: Arguments { args, keywords: vec![] }, kind: CallKind::New, range: r }))
                        }
                    };
                    return vec![Stmt::Raise(StmtRaise { exc: exc.map(Box::new), cause: None, range: r })];
                }
                // Declarations that say nothing a diagram shows.
                "private" | "protected" | "public" | "module_function" | "private_constant" | "private_class_method" | "public_class_method" | "alias_method" | "ruby2_keywords" => {
                    return args.iter().filter(|a| matches!(a, Node::DefNode { .. })).flat_map(|d| self.stmt(d, &Dest::None)).collect();
                }
                "loop" if c.block().is_some() => {
                    let body = self.block_body(c.block());
                    return vec![Stmt::While(StmtWhile { test: Box::new(Expr::BooleanLiteral(ExprBooleanLiteral { value: true, range: range(&c.message_loc().unwrap_or(c.location())) })), body, orelse: vec![], range: r })];
                }
                _ => {}
            }
        }
        // `xs.each do |x| … end` loops over `xs`; `n.times do |i|` counts.
        if let (Some(recv), Some(block)) = (c.receiver(), c.block().and_then(|b| b.as_block_node())) {
            if EACH.contains(&method.as_str()) || method == "times" {
                let mut params = self.block_params(block.parameters());
                let target = match params.len() {
                    0 => name("_", TextRange::empty(range(&block.location()).start())),
                    1 => {
                        let p = params.remove(0);
                        name(p.name, p.range)
                    }
                    _ => {
                        let at = range(&block.opening_loc()).end();
                        Expr::Tuple(ExprSeq { elts: params.into_iter().map(|p| name(p.name, p.range)).collect(), range: TextRange::empty(at) })
                    }
                };
                let iter = if method == "times" { self.range_call(&recv, r) } else { self.expr(&recv) };
                let body = self.body(block.body(), &Dest::None);
                return vec![Stmt::For(StmtFor { target: Box::new(store(target)), iter: Box::new(iter), body, orelse: vec![], kind: ForKind::In, is_async: false, range: r })];
            }
        }
        // `x.y = v` and `x[k] = v` are stores.
        if c.is_attribute_write() || method == "[]=" {
            let Some(recv) = c.receiver() else { return vec![] };
            let base = self.expr(&recv);
            let mut values: Vec<Expr> = args.iter().map(|a| self.expr(a)).collect();
            let value = values.pop().unwrap_or_else(|| other(r));
            let target = if method == "[]=" {
                let slice = if values.len() == 1 { values.remove(0) } else { Expr::Tuple(ExprSeq { elts: values, range: r }) };
                Expr::Subscript(ExprSubscript { value: Box::new(base), slice: Box::new(slice), range: r })
            } else {
                let attr = method.trim_end_matches('=').to_string();
                let end = c.message_loc().map(|m| range(&m).end()).unwrap_or(r.end());
                Expr::Attribute(ExprAttribute { value: Box::new(base), attr, range: TextRange::new(r.start(), end) })
            };
            return vec![Stmt::Assign(StmtAssign { targets: vec![target], value: Box::new(value), range: r })];
        }
        let e = self.call(c);
        self.value(e, dest, r)
    }

    /// `n.times`: the numbers below `n`, written as the receiver.
    fn range_call(&mut self, n: &Node<'_>, r: TextRange) -> Expr {
        let v = self.expr(n);
        Expr::Call(ExprCall { func: Box::new(name("range", TextRange::empty(r.start()))), arguments: Arguments { args: vec![v], keywords: vec![] }, kind: CallKind::Call, range: nrange(n) })
    }

    fn block_body(&mut self, block: Option<Node<'_>>) -> Vec<Stmt> {
        match block.and_then(|b| b.as_block_node()) {
            Some(b) => self.body(b.body(), &Dest::None),
            None => vec![],
        }
    }

    // ----- assignments --------------------------------------------------------------

    fn write(&mut self, n: &Node<'_>) -> Vec<Stmt> {
        let r = nrange(n);
        macro_rules! simple {
            ($node:expr, $target:expr) => {{
                let x = $node;
                let target = $target(&x);
                self.assign(store(target), &x.value(), r)
            }};
        }
        macro_rules! op {
            ($node:expr, $target:expr) => {{
                let x = $node;
                let target = $target(&x);
                let value = self.expr(&x.value());
                match operator(&text(&x.binary_operator())) {
                    Some(op) => vec![Stmt::AugAssign(StmtAugAssign { target: Box::new(store(target)), op, value: Box::new(value), range: r })],
                    None => vec![Stmt::Assign(StmtAssign { targets: vec![store(target.clone())], value: Box::new(value), range: r })],
                }
            }};
        }
        // `@x ||= compute` keeps `x` if it is set (memoization); `&&=` changes it only if so.
        macro_rules! logical {
            ($node:expr, $target:expr, $op:expr) => {{
                let x = $node;
                let target = $target(&x);
                let value = self.expr(&x.value());
                let combined = Expr::BoolOp(ExprBoolOp { op: $op, values: vec![target.clone(), value], range: r });
                vec![Stmt::Assign(StmtAssign { targets: vec![store(target)], value: Box::new(combined), range: r })]
            }};
        }
        let local = |id: &ConstantId<'_>, loc: Location<'_>| name(text(id), range(&loc));
        let ivar = |id: &ConstantId<'_>, loc: Location<'_>| on_self(text(id).trim_start_matches('@'), range(&loc));
        let global = |id: &ConstantId<'_>, loc: Location<'_>| name(text(id), range(&loc));
        match n {
            Node::LocalVariableWriteNode { .. } => simple!(n.as_local_variable_write_node().unwrap(), |x: &ruby_prism::LocalVariableWriteNode<'_>| local(&x.name(), x.name_loc())),
            Node::InstanceVariableWriteNode { .. } => simple!(n.as_instance_variable_write_node().unwrap(), |x: &ruby_prism::InstanceVariableWriteNode<'_>| ivar(&x.name(), x.name_loc())),
            Node::ClassVariableWriteNode { .. } => simple!(n.as_class_variable_write_node().unwrap(), |x: &ruby_prism::ClassVariableWriteNode<'_>| ivar(&x.name(), x.name_loc())),
            Node::GlobalVariableWriteNode { .. } => simple!(n.as_global_variable_write_node().unwrap(), |x: &ruby_prism::GlobalVariableWriteNode<'_>| global(&x.name(), x.name_loc())),
            Node::ConstantWriteNode { .. } => simple!(n.as_constant_write_node().unwrap(), |x: &ruby_prism::ConstantWriteNode<'_>| local(&x.name(), x.name_loc())),
            Node::ConstantPathWriteNode { .. } => {
                let x = n.as_constant_path_write_node().unwrap();
                let t = x.target();
                let target = name(t.name().map(|i| text(&i)).unwrap_or_default(), range(&t.location()));
                self.assign(store(target), &x.value(), r)
            }
            Node::LocalVariableOperatorWriteNode { .. } => op!(n.as_local_variable_operator_write_node().unwrap(), |x: &ruby_prism::LocalVariableOperatorWriteNode<'_>| local(&x.name(), x.name_loc())),
            Node::InstanceVariableOperatorWriteNode { .. } => op!(n.as_instance_variable_operator_write_node().unwrap(), |x: &ruby_prism::InstanceVariableOperatorWriteNode<'_>| ivar(&x.name(), x.name_loc())),
            Node::ClassVariableOperatorWriteNode { .. } => op!(n.as_class_variable_operator_write_node().unwrap(), |x: &ruby_prism::ClassVariableOperatorWriteNode<'_>| ivar(&x.name(), x.name_loc())),
            Node::GlobalVariableOperatorWriteNode { .. } => op!(n.as_global_variable_operator_write_node().unwrap(), |x: &ruby_prism::GlobalVariableOperatorWriteNode<'_>| global(&x.name(), x.name_loc())),
            Node::LocalVariableOrWriteNode { .. } => logical!(n.as_local_variable_or_write_node().unwrap(), |x: &ruby_prism::LocalVariableOrWriteNode<'_>| local(&x.name(), x.name_loc()), BoolOp::OrOr),
            Node::InstanceVariableOrWriteNode { .. } => logical!(n.as_instance_variable_or_write_node().unwrap(), |x: &ruby_prism::InstanceVariableOrWriteNode<'_>| ivar(&x.name(), x.name_loc()), BoolOp::OrOr),
            Node::ClassVariableOrWriteNode { .. } => logical!(n.as_class_variable_or_write_node().unwrap(), |x: &ruby_prism::ClassVariableOrWriteNode<'_>| ivar(&x.name(), x.name_loc()), BoolOp::OrOr),
            Node::GlobalVariableOrWriteNode { .. } => logical!(n.as_global_variable_or_write_node().unwrap(), |x: &ruby_prism::GlobalVariableOrWriteNode<'_>| global(&x.name(), x.name_loc()), BoolOp::OrOr),
            Node::ConstantOrWriteNode { .. } => logical!(n.as_constant_or_write_node().unwrap(), |x: &ruby_prism::ConstantOrWriteNode<'_>| local(&x.name(), x.name_loc()), BoolOp::OrOr),
            Node::LocalVariableAndWriteNode { .. } => logical!(n.as_local_variable_and_write_node().unwrap(), |x: &ruby_prism::LocalVariableAndWriteNode<'_>| local(&x.name(), x.name_loc()), BoolOp::AndAnd),
            Node::InstanceVariableAndWriteNode { .. } => logical!(n.as_instance_variable_and_write_node().unwrap(), |x: &ruby_prism::InstanceVariableAndWriteNode<'_>| ivar(&x.name(), x.name_loc()), BoolOp::AndAnd),
            Node::IndexOrWriteNode { .. } | Node::IndexAndWriteNode { .. } | Node::IndexOperatorWriteNode { .. } | Node::CallOrWriteNode { .. } | Node::CallAndWriteNode { .. } | Node::CallOperatorWriteNode { .. } => self.call_write(n),
            Node::MultiWriteNode { .. } => {
                let m = n.as_multi_write_node().unwrap();
                let mut elts: Vec<Expr> = m.lefts().iter().map(|t| self.target(&t)).collect();
                if let Some(rest) = m.rest() {
                    elts.push(self.target(&rest));
                }
                elts.extend(m.rights().iter().map(|t| self.target(&t)));
                let at = elts.first().map(|e| e.range()).unwrap_or(r);
                let target = Expr::Tuple(ExprSeq { elts, range: TextRange::new(at.start(), m.operator_loc().start().into()) });
                self.assign(store(target), &m.value(), r)
            }
            _ => vec![Stmt::Expr(StmtExpr { value: Box::new(self.expr(n)), range: r })],
        }
    }

    /// `x[k] ||= v`, `x.y += 1`: a store into what the receiver holds.
    fn call_write(&mut self, n: &Node<'_>) -> Vec<Stmt> {
        let r = nrange(n);
        let (receiver, attr, args, value, op): (Option<Node<'_>>, Option<String>, Vec<Node<'_>>, Node<'_>, Option<BoolOp>) = if let Some(x) = n.as_index_or_write_node() {
            (x.receiver(), None, x.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default(), x.value(), Some(BoolOp::OrOr))
        } else if let Some(x) = n.as_index_and_write_node() {
            (x.receiver(), None, x.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default(), x.value(), Some(BoolOp::AndAnd))
        } else if let Some(x) = n.as_index_operator_write_node() {
            (x.receiver(), None, x.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default(), x.value(), None)
        } else if let Some(x) = n.as_call_or_write_node() {
            (x.receiver(), Some(text(&x.read_name())), vec![], x.value(), Some(BoolOp::OrOr))
        } else if let Some(x) = n.as_call_and_write_node() {
            (x.receiver(), Some(text(&x.read_name())), vec![], x.value(), Some(BoolOp::AndAnd))
        } else if let Some(x) = n.as_call_operator_write_node() {
            (x.receiver(), Some(text(&x.read_name())), vec![], x.value(), None)
        } else {
            return vec![];
        };
        let base = receiver.map(|b| self.expr(&b)).unwrap_or_else(|| name("self", TextRange::empty(r.start())));
        let target = match attr {
            Some(a) => Expr::Attribute(ExprAttribute { value: Box::new(base), attr: a, range: r }),
            None => {
                let mut keys: Vec<Expr> = args.iter().map(|a| self.expr(a)).collect();
                let slice = if keys.len() == 1 { keys.remove(0) } else { Expr::Tuple(ExprSeq { elts: keys, range: r }) };
                Expr::Subscript(ExprSubscript { value: Box::new(base), slice: Box::new(slice), range: r })
            }
        };
        let value = self.expr(&value);
        match op {
            Some(op) => {
                let combined = Expr::BoolOp(ExprBoolOp { op, values: vec![target.clone(), value], range: r });
                vec![Stmt::Assign(StmtAssign { targets: vec![target], value: Box::new(combined), range: r })]
            }
            None => vec![Stmt::AugAssign(StmtAugAssign { target: Box::new(target), op: Operator::Add, value: Box::new(value), range: r })],
        }
    }

    /// `target = value`; a value computed by `if`, `case` or `begin` sets the
    /// target in each of its branches.
    fn assign(&mut self, target: Expr, value: &Node<'_>, r: TextRange) -> Vec<Stmt> {
        if matches!(value, Node::CaseNode { .. } | Node::CaseMatchNode { .. } | Node::BeginNode { .. } | Node::UnlessNode { .. }) || matches!(value, Node::IfNode { .. } if value.as_if_node().is_some_and(|x| x.if_keyword_loc().is_some())) {
            return self.stmt(value, &Dest::Assign(target));
        }
        let v = self.expr(value);
        vec![Stmt::Assign(StmtAssign { targets: vec![target], value: Box::new(v), range: r })]
    }

    /// What an assignment's left side names.
    fn target(&mut self, n: &Node<'_>) -> Expr {
        let r = nrange(n);
        if let Some(t) = n.as_local_variable_target_node() {
            return name(text(&t.name()), r);
        }
        if let Some(t) = n.as_instance_variable_target_node() {
            return on_self(text(&t.name()).trim_start_matches('@'), r);
        }
        if let Some(t) = n.as_class_variable_target_node() {
            return on_self(text(&t.name()).trim_start_matches('@'), r);
        }
        if let Some(t) = n.as_global_variable_target_node() {
            return name(text(&t.name()), r);
        }
        if let Some(t) = n.as_constant_target_node() {
            return name(text(&t.name()), r);
        }
        if let Some(m) = n.as_multi_target_node() {
            let mut elts: Vec<Expr> = m.lefts().iter().map(|t| self.target(&t)).collect();
            if let Some(rest) = m.rest() {
                elts.push(self.target(&rest));
            }
            elts.extend(m.rights().iter().map(|t| self.target(&t)));
            return Expr::Tuple(ExprSeq { elts, range: r });
        }
        if let Some(s) = n.as_splat_node() {
            return match s.expression() {
                Some(e) => Expr::Starred(ExprInner { value: Box::new(self.target(&e)), range: r }),
                None => name("_", r),
            };
        }
        if let Some(t) = n.as_call_target_node() {
            let base = self.expr(&t.receiver());
            return Expr::Attribute(ExprAttribute { value: Box::new(base), attr: text(&t.name()).trim_end_matches('=').to_string(), range: r });
        }
        if let Some(t) = n.as_index_target_node() {
            let base = self.expr(&t.receiver());
            let mut keys: Vec<Expr> = t.arguments().map(|a| a.arguments().iter().map(|k| self.expr(&k)).collect()).unwrap_or_default();
            let slice = if keys.len() == 1 { keys.remove(0) } else { Expr::Tuple(ExprSeq { elts: keys, range: r }) };
            return Expr::Subscript(ExprSubscript { value: Box::new(base), slice: Box::new(slice), range: r });
        }
        if matches!(n, Node::ImplicitRestNode { .. }) {
            return name("_", r);
        }
        self.expr(n)
    }

    // ----- definitions --------------------------------------------------------------

    /// The full name a class or module definition gives.
    fn defined_name(&self, path: &Node<'_>) -> String {
        let written = constant_text(path).unwrap_or_default();
        if let Some(abs) = written.strip_prefix("::") {
            return abs.to_string();
        }
        // `class Admin::User` opens `Admin` where it is visible from here.
        if let Some((head, rest)) = written.split_once("::") {
            if let Some(h) = self.consts.resolve(&self.nesting, head) {
                return format!("{h}::{rest}");
            }
        }
        match self.nesting.last() {
            Some(outer) => format!("{outer}::{written}"),
            None => written,
        }
    }

    /// A class or module, and the classes and modules defined inside it,
    /// each at the top level under its full name.
    fn class(&mut self, n: &Node<'_>) -> Vec<Stmt> {
        let r = nrange(n);
        let (path, superclass, body, is_module, keyword) = if let Some(c) = n.as_class_node() {
            (c.constant_path(), c.superclass(), c.body(), false, c.class_keyword_loc())
        } else {
            let m = n.as_module_node().unwrap();
            (m.constant_path(), None, m.body(), true, m.module_keyword_loc())
        };
        let full = self.defined_name(&path);
        let mut bases = vec![];
        if let Some(s) = &superclass {
            bases.push(self.expr(s));
        }
        self.nesting.push(full.clone());
        self.scope.push(Scope { name: full.clone(), singleton: false });
        let mut own = vec![];
        let mut nested = vec![];
        let mut badges: Vec<&str> = vec![];
        let mut abstract_ = false;
        // Methods after `private` (or named by it), which are no controller's actions.
        let mut private = false;
        let mut hidden: Vec<String> = vec![];
        let nodes: Vec<Node<'_>> = match body {
            Some(b) => match b.as_statements_node() {
                Some(s) => s.body().iter().collect(),
                None => vec![b],
            },
            None => vec![],
        };
        for s in &nodes {
            match s {
                Node::ClassNode { .. } | Node::ModuleNode { .. } => nested.extend(self.class(s)),
                Node::DefNode { .. } => {
                    let f = self.def(&s.as_def_node().unwrap(), false);
                    if private {
                        hidden.push(f.name.clone());
                    }
                    own.push(Stmt::FunctionDef(f));
                }
                Node::SingletonClassNode { .. } => {
                    let sc = s.as_singleton_class_node().unwrap();
                    self.scope.last_mut().unwrap().singleton = true;
                    let inner: Vec<Node<'_>> = match sc.body() {
                        Some(b) => match b.as_statements_node() {
                            Some(st) => st.body().iter().collect(),
                            None => vec![b],
                        },
                        None => vec![],
                    };
                    for i in &inner {
                        match i {
                            Node::DefNode { .. } => own.push(Stmt::FunctionDef(self.def(&i.as_def_node().unwrap(), true))),
                            _ => own.extend(self.stmt(i, &Dest::None)),
                        }
                    }
                    self.scope.last_mut().unwrap().singleton = false;
                }
                Node::CallNode { .. } => {
                    let c = s.as_call_node().unwrap();
                    let method = text(&c.name());
                    if c.receiver().is_none() && matches!(method.as_str(), "private" | "protected" | "public") {
                        let args: Vec<Node<'_>> = c.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
                        if args.is_empty() {
                            private = method != "public";
                        } else if method != "public" {
                            // `private def x` and `private :x, :y`.
                            hidden.extend(args.iter().filter_map(|a| a.as_def_node().map(|d| text(&d.name())).or_else(|| string_value(a))));
                        }
                    }
                    if c.receiver().is_none() && matches!(method.as_str(), "include" | "extend" | "prepend") {
                        for a in c.arguments().map(|a| a.arguments().iter().collect::<Vec<_>>()).unwrap_or_default() {
                            match constant_text(&a).as_deref() {
                                // What a module is, not a module it uses.
                                Some("ActiveSupport::Concern") => badges.push("concern"),
                                Some("ActiveModel::Model" | "ActiveModel::API") => badges.push("form model"),
                                Some(_) => bases.push(self.expr(&a)),
                                None => {}
                            }
                        }
                        continue;
                    }
                    if c.receiver().is_none() && matches!(method.as_str(), "primary_abstract_class") {
                        abstract_ = true;
                        continue;
                    }
                    if c.is_attribute_write() && method == "abstract_class=" && c.receiver().is_some_and(|r| matches!(r, Node::SelfNode { .. })) {
                        abstract_ = true;
                        continue;
                    }
                    own.extend(self.stmt(s, &Dest::None));
                }
                _ => own.extend(self.stmt(s, &Dest::None)),
            }
        }
        // Rails' declarations: tables, the roles of the methods they name, scopes.
        let decl = super::rails::declarations(&own, self.src, |n| self.consts.resolve(&self.nesting, n).unwrap_or_else(|| n.to_string()));
        for s in &mut own {
            if let Stmt::FunctionDef(f) = s {
                if let Some((_, label)) = decl.roles.iter().find(|(m, _)| *m == f.name) {
                    f.decorators.push(super::rails::role_decorator(label, f.range));
                }
            }
        }
        // A controller's actions render the views named after them, unless they render others.
        if !is_module && full.ends_with("Controller") {
            let path = super::underscore(full.trim_end_matches("Controller"));
            for s in &mut own {
                if let Stmt::FunctionDef(f) = s {
                    if f.kind == DefKind::Method && !hidden.contains(&f.name) {
                        for t in super::rails::templates(&path, &f.name, &f.body) {
                            f.decorators.push(super::rails::template_decorator(&t, f.range));
                        }
                    }
                }
            }
        }
        let tables: Vec<Decorator> = decl.tables.iter().map(|(title, rows)| super::rails::table_decorator(title, rows, r)).collect();
        own.extend(decl.uses.into_iter().map(|(class, at)| Stmt::Expr(StmtExpr { value: Box::new(name(class, at)), range: at })));
        own.extend(decl.scopes.into_iter().map(Stmt::FunctionDef));
        self.scope.pop();
        self.nesting.pop();
        let mut out = vec![];
        // A class or module opened only to hold other definitions
        // (`class Thor; class Error < StandardError`) is their namespace.
        let primary = self.primary.get(&full).is_none_or(|&p| p == self.input);
        let namespace_only = own.is_empty() && bases.is_empty() && (is_module || !nested.is_empty() || !primary);
        if !namespace_only {
            let kind = if !primary {
                bases.insert(0, name(full.clone(), range(&path.location())));
                ClassKind::Extension
            } else if is_module && !own.iter().any(|s| matches!(s, Stmt::FunctionDef(f) if f.kind != DefKind::ClassMethod)) {
                ClassKind::Namespace
            } else {
                ClassKind::Class
            };
            let doc = self.doc_above(keyword.start());
            // Active Record models are the database's tables.
            if !is_module && kind != ClassKind::Extension && super::rails::is_model(self.consts, &full) {
                badges.push(if abstract_ { "abstract" } else { "DB table" });
            }
            let decorators = badges.iter().map(|b| badge(b, r)).chain(tables).collect();
            out.push(Stmt::ClassDef(ClassDef { name: full, bases, keywords: vec![], body: own, decorators, kind, doc, range: r }));
        }
        out.extend(nested);
        out
    }

    fn def(&mut self, d: &ruby_prism::DefNode<'_>, in_singleton_class: bool) -> FunctionDef {
        let r = range(&d.location());
        let method = text(&d.name());
        let singleton = in_singleton_class || d.receiver().is_some_and(|x| matches!(x, Node::SelfNode { .. }));
        let in_class = !self.scope.is_empty();
        let kind = if !in_class {
            DefKind::Function
        } else if singleton {
            DefKind::ClassMethod
        } else if method == "initialize" {
            DefKind::Constructor
        } else if method.ends_with('=') && !matches!(method.as_str(), "==" | "===" | "!=" | "<=" | ">=") {
            DefKind::Setter
        } else {
            DefKind::Method
        };
        let mut params = vec![];
        if in_class {
            params.push(Param { name: "self".into(), kind: ParamKind::Positional, default: None, annotation: None, range: TextRange::empty(r.start()) });
        }
        params.extend(self.params(d.parameters()));
        if let Some(s) = self.scope.last_mut() {
            s.singleton = singleton;
        }
        // An action's or a job's last value goes nowhere; a constructor's or setter's is not its result.
        let class = self.scope.last().map(|s| s.name.as_str()).unwrap_or("");
        let keeps = !matches!(kind, DefKind::Constructor | DefKind::Setter) && !(class.ends_with("Controller") || (class.ends_with("Job") && method == "perform"));
        self.method.push(method.clone());
        let body = self.body(d.body(), if keeps { &Dest::Return } else { &Dest::None });
        self.method.pop();
        if let Some(s) = self.scope.last_mut() {
            s.singleton = in_singleton_class;
        }
        FunctionDef { name: method, params, body, decorators: vec![], returns: None, is_async: false, is_generator: false, kind, doc: self.doc_above(d.def_keyword_loc().start()), range: r }
    }

    fn params(&mut self, p: Option<ParametersNode<'_>>) -> Vec<Param> {
        let Some(p) = p else { return vec![] };
        let mut out = vec![];
        let param = |n: String, kind: ParamKind, default: Option<Expr>, range: TextRange| Param { name: n, kind, default, annotation: None, range };
        for x in p.requireds().iter().chain(p.posts().iter()) {
            let r = nrange(&x);
            match x.as_required_parameter_node() {
                Some(q) => out.push(param(text(&q.name()), ParamKind::Positional, None, r)),
                None => out.push(param(self.of(r).to_string(), ParamKind::Positional, None, r)),
            }
        }
        for x in p.optionals().iter() {
            if let Some(q) = x.as_optional_parameter_node() {
                let default = self.expr(&q.value());
                out.push(param(text(&q.name()), ParamKind::Positional, Some(default), nrange(&x)));
            }
        }
        if let Some(rest) = p.rest() {
            let n = rest.as_rest_parameter_node().and_then(|q| q.name()).map(|i| text(&i)).unwrap_or_else(|| "*".into());
            out.push(param(n, ParamKind::Variadic, None, nrange(&rest)));
        }
        for x in p.keywords().iter() {
            let r = nrange(&x);
            if let Some(q) = x.as_required_keyword_parameter_node() {
                out.push(param(text(&q.name()), ParamKind::KeywordOnly, None, r));
            } else if let Some(q) = x.as_optional_keyword_parameter_node() {
                let default = self.expr(&q.value());
                out.push(param(text(&q.name()), ParamKind::KeywordOnly, Some(default), r));
            }
        }
        if let Some(k) = p.keyword_rest() {
            let n = k.as_keyword_rest_parameter_node().and_then(|q| q.name()).map(|i| text(&i)).unwrap_or_else(|| "**".into());
            out.push(param(n, ParamKind::KeywordVariadic, None, nrange(&k)));
        }
        if let Some(b) = p.block().and_then(|b| b.as_block_parameter_node()) {
            let n = b.name().map(|i| text(&i)).unwrap_or_else(|| "&".into());
            out.push(param(n, ParamKind::Positional, None, range(&b.location())));
        }
        out
    }

    /// A block's parameters (`|x, i|`; `it` and `_1` when it names none).
    fn block_params(&mut self, p: Option<Node<'_>>) -> Vec<Param> {
        let Some(p) = p else { return vec![] };
        if let Some(b) = p.as_block_parameters_node() {
            return self.params(b.parameters());
        }
        if matches!(p, Node::ItParametersNode { .. }) {
            return vec![Param { name: "it".into(), kind: ParamKind::Positional, default: None, annotation: None, range: nrange(&p) }];
        }
        if let Some(n) = p.as_numbered_parameters_node() {
            return (1..=n.maximum()).map(|i| Param { name: format!("_{i}"), kind: ParamKind::Positional, default: None, annotation: None, range: nrange(&p) }).collect();
        }
        vec![]
    }

    /// The comment block directly above `at`'s line, as documentation.
    fn doc_above(&mut self, at: u32) -> Option<String> {
        let line_start = self.src[..at as usize].rfind('\n').map(|i| i + 1).unwrap_or(0) as u32;
        let mut end = line_start;
        let mut lines = vec![];
        loop {
            // The comment that ends just before `end` (the previous line), alone on its line.
            let Some(c) = self.comments.iter().rev().find(|c| c.1 < end && end - c.1 <= 1) else { break };
            let start = c.0 as usize;
            let own_line = self.src[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
            if !self.src[own_line..start].trim().is_empty() {
                break;
            }
            lines.push((c.0, c.2.trim().to_string()));
            end = own_line as u32;
        }
        if lines.is_empty() {
            return None;
        }
        lines.reverse();
        self.docs.extend(lines.iter().map(|(o, _)| *o as usize));
        // YARD tags (`@param`, `@return`) and directives are not the description.
        let prose: Vec<&str> = lines.iter().map(|(_, t)| t.as_str()).take_while(|t| !t.starts_with('@')).filter(|t| !t.starts_with("rubocop:") && !t.starts_with(":nodoc:")).collect();
        let doc = crate::text::summary(&prose.join(" "));
        (!doc.is_empty()).then_some(doc)
    }

    // ----- expressions --------------------------------------------------------------

    /// Several values (`return a, b`) are a tuple.
    fn values(&mut self, nodes: Vec<Node<'_>>, r: TextRange) -> Expr {
        let mut elts: Vec<Expr> = nodes.iter().map(|n| self.expr(n)).collect();
        if elts.len() == 1 {
            elts.remove(0)
        } else {
            Expr::Tuple(ExprSeq { elts, range: r })
        }
    }

    fn not(&mut self, n: Node<'_>) -> Expr {
        let r = nrange(&n);
        let operand = self.expr(&n);
        Expr::UnaryOp(ExprUnaryOp { op: UnaryOp::Not, operand: Box::new(operand), range: r })
    }

    fn string(&self, value: &[u8], r: TextRange) -> Expr {
        Expr::StringLiteral(ExprStringLiteral { value: String::from_utf8_lossy(value).into_owned(), range: r })
    }

    /// A constant: a project class or module by its full name, else what it is written as.
    fn constant(&mut self, n: &Node<'_>) -> Expr {
        let r = nrange(n);
        let written = constant_text(n).unwrap_or_default();
        if let Some(full) = self.consts.resolve(&self.nesting, &written) {
            return name(full, r);
        }
        // `ActiveRecord::Base`, `Rails.logger`: members of what they are in.
        if let Some(p) = n.as_constant_path_node() {
            if let (Some(parent), Some(id)) = (p.parent(), p.name()) {
                let value = self.constant(&parent);
                return Expr::Attribute(ExprAttribute { value: Box::new(value), attr: text(&id), range: r });
            }
        }
        name(written.trim_start_matches("::"), r)
    }

    pub fn expr(&mut self, n: &Node<'_>) -> Expr {
        let r = nrange(n);
        match n {
            Node::CallNode { .. } => self.call(&n.as_call_node().unwrap()),
            Node::LocalVariableReadNode { .. } => name(text(&n.as_local_variable_read_node().unwrap().name()), r),
            Node::ItLocalVariableReadNode { .. } => name("it", r),
            Node::InstanceVariableReadNode { .. } => on_self(text(&n.as_instance_variable_read_node().unwrap().name()).trim_start_matches('@'), r),
            Node::ClassVariableReadNode { .. } => on_self(text(&n.as_class_variable_read_node().unwrap().name()).trim_start_matches('@'), r),
            Node::GlobalVariableReadNode { .. } => name(text(&n.as_global_variable_read_node().unwrap().name()), r),
            Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => self.constant(n),
            Node::SelfNode { .. } => name("self", r),
            Node::NilNode { .. } => Expr::NoneLiteral(Atom { range: r }),
            Node::TrueNode { .. } => Expr::BooleanLiteral(ExprBooleanLiteral { value: true, range: r }),
            Node::FalseNode { .. } => Expr::BooleanLiteral(ExprBooleanLiteral { value: false, range: r }),
            Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::RationalNode { .. } | Node::ImaginaryNode { .. } => Expr::NumberLiteral(Atom { range: r }),
            Node::StringNode { .. } => self.string(n.as_string_node().unwrap().unescaped(), r),
            Node::SymbolNode { .. } => self.string(n.as_symbol_node().unwrap().unescaped(), r),
            Node::XStringNode { .. } | Node::InterpolatedXStringNode { .. } => {
                // A shell command run by backticks.
                Expr::Call(ExprCall { func: Box::new(name("system", TextRange::empty(r.start()))), arguments: Arguments { args: vec![other(r)], keywords: vec![] }, kind: CallKind::Call, range: r })
            }
            Node::InterpolatedStringNode { .. } => {
                let parts = n.as_interpolated_string_node().unwrap().parts().iter().collect();
                self.template(parts, r)
            }
            Node::InterpolatedSymbolNode { .. } => {
                let parts = n.as_interpolated_symbol_node().unwrap().parts().iter().collect();
                self.template(parts, r)
            }
            Node::SourceFileNode { .. } => name("__FILE__", r),
            Node::ArrayNode { .. } => {
                let elts = n.as_array_node().unwrap().elements().iter().map(|e| self.expr(&e)).collect();
                Expr::List(ExprSeq { elts, range: r })
            }
            Node::HashNode { .. } => {
                let items = self.assocs(n.as_hash_node().unwrap().elements().iter().collect());
                Expr::Dict(ExprDict { items, range: r })
            }
            Node::KeywordHashNode { .. } => {
                let items = self.assocs(n.as_keyword_hash_node().unwrap().elements().iter().collect());
                Expr::Dict(ExprDict { items, range: r })
            }
            Node::RangeNode { .. } => {
                let x = n.as_range_node().unwrap();
                let args = x.left().into_iter().chain(x.right()).map(|e| self.expr(&e)).collect();
                Expr::Call(ExprCall { func: Box::new(name("range", TextRange::empty(r.start()))), arguments: Arguments { args, keywords: vec![] }, kind: CallKind::Call, range: r })
            }
            Node::AndNode { .. } => {
                let x = n.as_and_node().unwrap();
                let values = vec![self.expr(&x.left()), self.expr(&x.right())];
                Expr::BoolOp(ExprBoolOp { op: BoolOp::AndAnd, values, range: r })
            }
            Node::OrNode { .. } => {
                let x = n.as_or_node().unwrap();
                let values = vec![self.expr(&x.left()), self.expr(&x.right())];
                Expr::BoolOp(ExprBoolOp { op: BoolOp::OrOr, values, range: r })
            }
            Node::ParenthesesNode { .. } => match n.as_parentheses_node().unwrap().body() {
                Some(b) => match b.as_statements_node() {
                    Some(s) => match s.body().iter().last() {
                        Some(last) => self.expr(&last),
                        None => Expr::NoneLiteral(Atom { range: r }),
                    },
                    None => self.expr(&b),
                },
                None => Expr::NoneLiteral(Atom { range: r }),
            },
            Node::StatementsNode { .. } => match n.as_statements_node().unwrap().body().iter().last() {
                Some(last) => self.expr(&last),
                None => Expr::NoneLiteral(Atom { range: r }),
            },
            Node::IfNode { .. } => {
                let x = n.as_if_node().unwrap();
                let test = self.expr(&x.predicate());
                let body = x.statements().map(|s| self.expr(&s.as_node())).unwrap_or(Expr::NoneLiteral(Atom { range: r }));
                let orelse = match x.subsequent() {
                    Some(s) => match s.as_else_node() {
                        Some(e) => e.statements().map(|s| self.expr(&s.as_node())).unwrap_or(Expr::NoneLiteral(Atom { range: r })),
                        None => self.expr(&s),
                    },
                    None => Expr::NoneLiteral(Atom { range: r }),
                };
                Expr::If(ExprIf { test: Box::new(test), body: Box::new(body), orelse: Box::new(orelse), range: r })
            }
            Node::UnlessNode { .. } => {
                let x = n.as_unless_node().unwrap();
                let test = self.not(x.predicate());
                let body = x.statements().map(|s| self.expr(&s.as_node())).unwrap_or(Expr::NoneLiteral(Atom { range: r }));
                let orelse = x.else_clause().and_then(|e| e.statements()).map(|s| self.expr(&s.as_node())).unwrap_or(Expr::NoneLiteral(Atom { range: r }));
                Expr::If(ExprIf { test: Box::new(test), body: Box::new(body), orelse: Box::new(orelse), range: r })
            }
            Node::LambdaNode { .. } => {
                let l = n.as_lambda_node().unwrap();
                let params = self.block_params(l.parameters());
                self.function(params, l.body(), r)
            }
            Node::SplatNode { .. } => match n.as_splat_node().unwrap().expression() {
                Some(e) => Expr::Starred(ExprInner { value: Box::new(self.expr(&e)), range: r }),
                None => other(r),
            },
            Node::AssocSplatNode { .. } => match n.as_assoc_splat_node().unwrap().value() {
                Some(e) => Expr::Starred(ExprInner { value: Box::new(self.expr(&e)), range: r }),
                None => other(r),
            },
            Node::BlockArgumentNode { .. } => match n.as_block_argument_node().unwrap().expression() {
                Some(e) => self.expr(&e),
                None => other(r),
            },
            Node::YieldNode { .. } => {
                let args = n.as_yield_node().unwrap().arguments().map(|a| a.arguments().iter().map(|x| self.expr(&x)).collect()).unwrap_or_default();
                let kw = n.as_yield_node().unwrap().keyword_loc();
                Expr::Call(ExprCall { func: Box::new(name("yield", range(&kw))), arguments: Arguments { args, keywords: vec![] }, kind: CallKind::Call, range: r })
            }
            Node::SuperNode { .. } | Node::ForwardingSuperNode { .. } => {
                // `super(…)` is the same method of the superclass, as Python's `super().m(…)`.
                let args = n.as_super_node().and_then(|s| s.arguments()).map(|a| a.arguments().iter().map(|x| self.expr(&x)).collect()).unwrap_or_default();
                let kw = TextRange::new(r.start(), (u32::from(r.start()) + 5).into());
                let sup = Expr::Call(ExprCall { func: Box::new(name("super", kw)), arguments: Arguments::default(), kind: CallKind::Call, range: kw });
                let method = self.method.last().cloned().unwrap_or_else(|| "initialize".into());
                let func = Expr::Attribute(ExprAttribute { value: Box::new(sup), attr: if method == "initialize" { "__init__".into() } else { method }, range: kw });
                Expr::Call(ExprCall { func: Box::new(func), arguments: Arguments { args, keywords: vec![] }, kind: CallKind::Call, range: r })
            }
            Node::DefinedNode { .. } => {
                let inner = n.as_defined_node().unwrap().value();
                let v = self.expr(&inner);
                Expr::Call(ExprCall { func: Box::new(name("defined?", TextRange::empty(r.start()))), arguments: Arguments { args: vec![v], keywords: vec![] }, kind: CallKind::Call, range: r })
            }
            Node::BeginNode { .. } => match n.as_begin_node().unwrap().statements() {
                Some(s) => self.expr(&s.as_node()),
                None => Expr::NoneLiteral(Atom { range: r }),
            },
            Node::RescueModifierNode { .. } => self.expr(&n.as_rescue_modifier_node().unwrap().expression()),
            _ if is_write(n) => {
                // An assignment used as a value: `if (user = find(id))`.
                let stmts = self.write(n);
                match stmts.into_iter().next() {
                    Some(Stmt::Assign(a)) => Expr::Named(ExprNamed { target: Box::new(a.targets.into_iter().next().unwrap_or_else(|| other(r))), value: a.value, range: r }),
                    _ => other(r),
                }
            }
            _ => other(r),
        }
    }

    fn template(&mut self, parts: Vec<Node<'_>>, r: TextRange) -> Expr {
        let mut out = vec![];
        for p in parts {
            if let Some(s) = p.as_string_node() {
                out.push(TemplatePart::Literal(String::from_utf8_lossy(s.unescaped()).into_owned()));
            } else if let Some(e) = p.as_embedded_statements_node() {
                if let Some(s) = e.statements() {
                    out.push(TemplatePart::Hole { expr: self.expr(&s.as_node()), spec: vec![] });
                }
            } else if let Some(e) = p.as_embedded_variable_node() {
                out.push(TemplatePart::Hole { expr: self.expr(&e.variable()), spec: vec![] });
            } else {
                out.push(TemplatePart::Hole { expr: self.expr(&p), spec: vec![] });
            }
        }
        Expr::Template(ExprTemplate { kind: TemplateKind::Format, parts: out, tag: None, range: r })
    }

    fn assocs(&mut self, elements: Vec<Node<'_>>) -> Vec<DictItem> {
        elements
            .iter()
            .map(|e| match e.as_assoc_node() {
                Some(a) => DictItem { key: Some(self.expr(&a.key())), value: self.expr(&a.value()) },
                None => match e.as_assoc_splat_node().and_then(|s| s.value()) {
                    Some(v) => DictItem { key: None, value: self.expr(&v) },
                    None => DictItem { key: None, value: other(nrange(e)) },
                },
            })
            .collect()
    }

    /// A block or lambda: a single expression is a lambda, statements a function.
    fn function(&mut self, params: Vec<Param>, body: Option<Node<'_>>, r: TextRange) -> Expr {
        let single = body.as_ref().and_then(|b| match b.as_statements_node() {
            Some(s) if s.body().iter().count() == 1 => s.body().iter().next(),
            _ => None,
        });
        if let Some(e) = single.filter(|e| !is_write(e) && !matches!(e, Node::IfNode { .. } | Node::UnlessNode { .. } | Node::CaseNode { .. } | Node::BeginNode { .. } | Node::WhileNode { .. } | Node::ReturnNode { .. })) {
            let body = self.expr(&e);
            return Expr::Lambda(ExprLambda { params, body: Box::new(body), range: r });
        }
        let body = self.body(body, &Dest::Return);
        Expr::Function(Box::new(FunctionDef { name: String::new(), params, body, decorators: vec![], returns: None, is_async: false, is_generator: false, kind: DefKind::Function, doc: None, range: r }))
    }

    /// Whether a call without a receiver in the current class is one of the
    /// project's methods for it.
    fn own_method(&self, method: &str) -> bool {
        let Some(s) = self.scope.last() else { return false };
        self.consts.has_method(&s.name, method, s.singleton)
    }

    fn call(&mut self, c: &CallNode<'_>) -> Expr {
        let r = range(&c.location());
        let method = text(&c.name());
        let args: Vec<Node<'_>> = c.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        let recv = c.receiver();
        // `$0 == __FILE__` runs the file as a program: the main guard.
        if let (Some(left), [right], "==") = (&recv, &args[..], method.as_str()) {
            let program = |n: &Node<'_>| n.as_global_variable_read_node().is_some_and(|g| matches!(g.name().as_slice(), b"$0" | b"$PROGRAM_NAME"));
            let file = |n: &Node<'_>| matches!(n, Node::SourceFileNode { .. });
            if (program(left) && file(right)) || (file(left) && program(right)) {
                return Expr::Compare(ExprCompare { ops: vec![CmpOp::Eq], operands: vec![name("__name__", nrange(left)), Expr::StringLiteral(ExprStringLiteral { value: "__main__".into(), range: nrange(right) })], range: r });
            }
        }
        // Operators.
        if let (Some(left), [right]) = (&recv, &args[..]) {
            if let Some(op) = operator(&method) {
                let (l, rt) = (self.expr(left), self.expr(right));
                return Expr::BinOp(ExprBinOp { left: Box::new(l), op, right: Box::new(rt), range: r });
            }
            if let Some(op) = comparison(&method) {
                let (l, rt) = (self.expr(left), self.expr(right));
                return Expr::Compare(ExprCompare { ops: vec![op], operands: vec![l, rt], range: r });
            }
            if method == "[]" {
                let (v, s) = (self.expr(left), self.expr(right));
                return Expr::Subscript(ExprSubscript { value: Box::new(v), slice: Box::new(s), range: r });
            }
        }
        if let (Some(operand), []) = (&recv, &args[..]) {
            let op = match method.as_str() {
                "!" => Some(UnaryOp::Not),
                "-@" => Some(UnaryOp::USub),
                "+@" => Some(UnaryOp::UAdd),
                "~" => Some(UnaryOp::Invert),
                _ => None,
            };
            if let Some(op) = op {
                let v = self.expr(operand);
                return Expr::UnaryOp(ExprUnaryOp { op, operand: Box::new(v), range: r });
            }
        }
        let message = c.message_loc().map(|m| range(&m)).unwrap_or(r);
        let block = c.block();
        let (pos, keywords) = self.arguments(&args);
        // `Foo.new(…)` constructs a `Foo`.
        if method == "new" {
            if let Some(rv) = &recv {
                if constant_text(rv).is_some() {
                    let func = self.expr(rv);
                    let mut arguments = Arguments { args: pos, keywords };
                    if let Some(b) = block.as_ref().and_then(|b| b.as_block_node()) {
                        let params = self.block_params(b.parameters());
                        arguments.args.push(self.function(params, b.body(), range(&b.location())));
                    }
                    return Expr::Call(ExprCall { func: Box::new(func), arguments, kind: CallKind::New, range: r });
                }
            }
        }
        // `xs.map { |x| f(x) }` is a comprehension; `xs.map(&:name)` too.
        if let Some(rv) = &recv {
            if let Some(comp) = self.comprehension(&method, rv, block.as_ref(), &args, r) {
                return comp;
            }
        }
        let func = match &recv {
            Some(rv) => {
                let value = self.expr(rv);
                Expr::Attribute(ExprAttribute { value: Box::new(value), attr: method.clone(), range: TextRange::new(r.start(), message.end()) })
            }
            None if self.own_method(&method) => on_self(&method, message),
            None => {
                // A bare name that is no project method reads a value
                // (`params`, `request`, `current_user` from a library).
                if c.is_variable_call() {
                    return name(method, message);
                }
                name(method.clone(), message)
            }
        };
        let mut arguments = Arguments { args: pos, keywords };
        match block.as_ref() {
            Some(b) if b.as_block_node().is_some() => {
                let b = b.as_block_node().unwrap();
                let params = self.block_params(b.parameters());
                arguments.args.push(self.function(params, b.body(), range(&b.location())));
            }
            Some(b) => {
                if let Some(e) = b.as_block_argument_node().and_then(|a| a.expression()) {
                    arguments.args.push(self.expr(&e));
                }
            }
            None => {}
        }
        Expr::Call(ExprCall { func: Box::new(func), arguments, kind: CallKind::Call, range: r })
    }

    /// Positional arguments and keywords (`b: 1`, `**opts`).
    fn arguments(&mut self, args: &[Node<'_>]) -> (Vec<Expr>, Vec<Keyword>) {
        let mut pos = vec![];
        let mut keywords = vec![];
        for a in args {
            if let Some(h) = a.as_keyword_hash_node() {
                for e in h.elements().iter() {
                    let r = nrange(&e);
                    if let Some(assoc) = e.as_assoc_node() {
                        let key = assoc.key();
                        let arg = match key.as_symbol_node() {
                            Some(s) => String::from_utf8_lossy(s.unescaped()).into_owned(),
                            None => string_value(&key).unwrap_or_else(|| self.of(nrange(&key)).to_string()),
                        };
                        keywords.push(Keyword { arg: Some(arg), value: self.expr(&assoc.value()), range: r });
                    } else if let Some(v) = e.as_assoc_splat_node().and_then(|s| s.value()) {
                        keywords.push(Keyword { arg: None, value: self.expr(&v), range: r });
                    }
                }
            } else if matches!(a, Node::ForwardingArgumentsNode { .. }) {
                continue;
            } else {
                pos.push(self.expr(a));
            }
        }
        (pos, keywords)
    }

    fn comprehension(&mut self, method: &str, recv: &Node<'_>, block: Option<&Node<'_>>, args: &[Node<'_>], r: TextRange) -> Option<Expr> {
        let kind = match method {
            "map" | "collect" | "flat_map" | "filter_map" => 0,
            "select" | "filter" => 1,
            "reject" => 2,
            _ => return None,
        };
        let (target, body) = match block {
            Some(b) if b.as_block_node().is_some() => {
                let b = b.as_block_node().unwrap();
                let params = self.block_params(b.parameters());
                let [p] = &params[..] else { return None };
                let s = b.body()?.as_statements_node()?;
                let mut nodes = s.body().iter();
                let (Some(e), None) = (nodes.next(), nodes.next()) else { return None };
                if is_write(&e) || matches!(e, Node::IfNode { .. } | Node::CaseNode { .. } | Node::BeginNode { .. }) {
                    return None;
                }
                (name(p.name.clone(), p.range), self.expr(&e))
            }
            // `&:name`: each item's `name`.
            Some(b) => {
                let sym = b.as_block_argument_node()?.expression()?;
                let s = sym.as_symbol_node()?;
                let at = TextRange::empty(nrange(&sym).start());
                let item = name("it", at);
                (item.clone(), Expr::Attribute(ExprAttribute { value: Box::new(item), attr: String::from_utf8_lossy(s.unescaped()).into_owned(), range: nrange(&sym) }))
            }
            None => return None,
        };
        if !args.is_empty() {
            return None;
        }
        let iter = self.expr(recv);
        let (elt, ifs) = match kind {
            0 => (body, vec![]),
            1 => (target.clone(), vec![body]),
            _ => {
                let br = body.range();
                (target.clone(), vec![Expr::UnaryOp(ExprUnaryOp { op: UnaryOp::Not, operand: Box::new(body), range: br })])
            }
        };
        let generators = vec![Comprehension { target: store(target), iter, ifs, range: r }];
        Some(Expr::Comp(ExprComp { kind: CompKind::List, elt: Box::new(elt), value: None, generators, range: r }))
    }
}

/// What the front end found about a definition, as Go's front end says it.
fn badge(text: &str, range: TextRange) -> Decorator {
    let at = TextRange::empty(range.start());
    let arg = Expr::StringLiteral(ExprStringLiteral { value: text.to_string(), range: at });
    Decorator { expression: Expr::Call(ExprCall { func: Box::new(name("badge", at)), arguments: Arguments { args: vec![arg], keywords: vec![] }, kind: CallKind::Call, range: at }), range: at }
}

fn if_(test: Expr, body: Vec<Stmt>, orelse: Vec<Stmt>, range: TextRange) -> Stmt {
    let elif_else_clauses = if orelse.is_empty() { vec![] } else { vec![ElifElseClause { test: None, range: orelse[0].range(), body: orelse }] };
    Stmt::If(StmtIf { test: Box::new(test), body, elif_else_clauses, range })
}

fn is_write(n: &Node<'_>) -> bool {
    matches!(
        n,
        Node::LocalVariableWriteNode { .. }
            | Node::InstanceVariableWriteNode { .. }
            | Node::ClassVariableWriteNode { .. }
            | Node::GlobalVariableWriteNode { .. }
            | Node::ConstantWriteNode { .. }
            | Node::ConstantPathWriteNode { .. }
            | Node::LocalVariableOperatorWriteNode { .. }
            | Node::InstanceVariableOperatorWriteNode { .. }
            | Node::ClassVariableOperatorWriteNode { .. }
            | Node::GlobalVariableOperatorWriteNode { .. }
            | Node::LocalVariableOrWriteNode { .. }
            | Node::InstanceVariableOrWriteNode { .. }
            | Node::ClassVariableOrWriteNode { .. }
            | Node::GlobalVariableOrWriteNode { .. }
            | Node::ConstantOrWriteNode { .. }
            | Node::LocalVariableAndWriteNode { .. }
            | Node::InstanceVariableAndWriteNode { .. }
            | Node::IndexOrWriteNode { .. }
            | Node::IndexAndWriteNode { .. }
            | Node::IndexOperatorWriteNode { .. }
            | Node::CallOrWriteNode { .. }
            | Node::CallAndWriteNode { .. }
            | Node::CallOperatorWriteNode { .. }
            | Node::MultiWriteNode { .. }
    )
}

/// A string or symbol literal's text.
fn string_value(n: &Node<'_>) -> Option<String> {
    if let Some(s) = n.as_string_node() {
        return Some(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    n.as_symbol_node().map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
}

fn operator(m: &str) -> Option<Operator> {
    Some(match m {
        "+" => Operator::Add,
        "-" => Operator::Sub,
        "*" => Operator::Mult,
        "/" => Operator::Div,
        "%" => Operator::Mod,
        "**" => Operator::Pow,
        "<<" => Operator::LShift,
        ">>" => Operator::RShift,
        "|" => Operator::BitOr,
        "^" => Operator::BitXor,
        "&" => Operator::BitAnd,
        _ => return None,
    })
}

fn comparison(m: &str) -> Option<CmpOp> {
    Some(match m {
        "==" | "===" | "eql?" | "equal?" => CmpOp::Eq,
        "!=" => CmpOp::NotEq,
        "<" => CmpOp::Lt,
        "<=" => CmpOp::LtE,
        ">" => CmpOp::Gt,
        ">=" => CmpOp::GtE,
        _ => return None,
    })
}

// ----- the project's constants ----------------------------------------------------------

/// Records the classes and modules a file defines, with their methods,
/// superclass and mixins as written.
pub fn collect(n: &Node<'_>, nesting: &mut Vec<String>, input: usize, consts: &mut Consts) {
    let visit_body = |body: Option<Node<'_>>, nesting: &mut Vec<String>, consts: &mut Consts| {
        if let Some(b) = body {
            collect(&b, nesting, input, consts);
        }
    };
    match n {
        Node::ProgramNode { .. } => collect(&n.as_program_node().unwrap().statements().as_node(), nesting, input, consts),
        Node::StatementsNode { .. } => {
            for s in n.as_statements_node().unwrap().body().iter() {
                collect(&s, nesting, input, consts);
            }
        }
        Node::ClassNode { .. } | Node::ModuleNode { .. } => {
            let (path, superclass, body, module) = match n.as_class_node() {
                Some(c) => (c.constant_path(), c.superclass(), c.body(), false),
                None => {
                    let m = n.as_module_node().unwrap();
                    (m.constant_path(), None, m.body(), true)
                }
            };
            let written = constant_text(&path).unwrap_or_default();
            let full = match (written.strip_prefix("::"), nesting.last()) {
                (Some(abs), _) => abs.to_string(),
                (None, Some(outer)) => format!("{outer}::{written}"),
                (None, None) => written,
            };
            // Every module on the way is defined too (`class A::B` needs `A`).
            let parts: Vec<&str> = full.split("::").collect();
            for k in 1..parts.len() {
                consts.defs.entry(parts[..k].join("::")).or_insert_with(|| Const { module: true, ..Const::default() });
            }
            let c = consts.defs.entry(full.clone()).or_default();
            c.module = module;
            if !c.files.contains(&input) {
                c.files.push(input);
            }
            if let Some(s) = superclass.as_ref().and_then(constant_text) {
                c.superclass.get_or_insert(s);
            }
            nesting.push(full);
            visit_body(body, nesting, consts);
            nesting.pop();
        }
        Node::SingletonClassNode { .. } => {
            if let (Some(owner), Some(body)) = (nesting.last().cloned(), n.as_singleton_class_node().unwrap().body()) {
                let defs: Vec<String> = match body.as_statements_node() {
                    Some(s) => s.body().iter().filter_map(|d| d.as_def_node().map(|d| text(&d.name()))).collect(),
                    None => vec![],
                };
                consts.defs.entry(owner).or_default().singleton.extend(defs);
            }
        }
        Node::DefNode { .. } => {
            let d = n.as_def_node().unwrap();
            if let Some(owner) = nesting.last() {
                let c = consts.defs.entry(owner.clone()).or_default();
                if d.receiver().is_some() {
                    c.singleton.insert(text(&d.name()));
                } else {
                    c.methods.insert(text(&d.name()));
                }
            }
        }
        Node::CallNode { .. } => {
            let c = n.as_call_node().unwrap();
            let method = text(&c.name());
            let args: Vec<Node<'_>> = c.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
            if let (None, Some(owner)) = (c.receiver(), nesting.last()) {
                match method.as_str() {
                    "include" | "extend" | "prepend" => {
                        let mixins: Vec<String> = args.iter().filter_map(constant_text).collect();
                        consts.defs.entry(owner.clone()).or_default().mixins.extend(mixins);
                    }
                    // Methods these declare are the class's too.
                    "attr_accessor" | "attr_reader" | "attr_writer" | "define_method" | "delegate" => {
                        let names: Vec<String> = args.iter().filter_map(string_value).collect();
                        let c = consts.defs.entry(owner.clone()).or_default();
                        for n in names {
                            if method != "attr_writer" {
                                c.methods.insert(n.clone());
                            }
                            if matches!(method.as_str(), "attr_accessor" | "attr_writer") {
                                c.methods.insert(format!("{n}="));
                            }
                        }
                    }
                    // `private def x`.
                    _ => {
                        for a in &args {
                            if matches!(a, Node::DefNode { .. }) {
                                collect(a, nesting, input, consts);
                            }
                        }
                    }
                }
            }
        }
        // Definitions under conditions are definitions.
        Node::IfNode { .. } => {
            if let Some(s) = n.as_if_node().unwrap().statements() {
                collect(&s.as_node(), nesting, input, consts);
            }
        }
        Node::BeginNode { .. } => {
            if let Some(s) = n.as_begin_node().unwrap().statements() {
                collect(&s.as_node(), nesting, input, consts);
            }
        }
        _ => {}
    }
}
