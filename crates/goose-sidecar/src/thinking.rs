//! What a model's own chat template lets a client steer about reasoning, proven on the template's
//! Jinja AST with Rapid-MLX's detection rules — never guessed from a model name.
//!
//! The rules are a port of `rapid_mlx/utils/chat_template.py` (the engine pinned in
//! `ENGINE_LAUNCHER`): `detect_native_reasoning_effort_levels` / `_walk_for_validation` for the
//! effort vocabulary, `template_thinking_switch` for the on/off variable. The engine applies the
//! same rules to the same template when a request arrives, so what this record declares is what
//! the engine will honour. Two deliberate differences, both named where they live: the parser is
//! minijinja's (its `elif` chains and `not in` are re-shaped to Jinja2's node forms before the
//! walk), and the context-read analysis behind the thinking switch is reduced to "the template
//! reads the name" (see [`ThinkingCapabilities::thinking_switch`]).

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use anyhow::{anyhow, bail, Result};
use minijinja::machinery::ast::{self, CallArg, Expr, Stmt};
use minijinja::machinery::{parse, Span, WhitespaceConfig};
use serde_json::{Map, Value};

use crate::engine::{ModelProfile, ThinkingMode};

/// The reasoning controls a chat template declares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThinkingCapabilities {
    /// The template variable that switches reasoning on/off: `enable_thinking` when the template
    /// reads it, else `reasoning` when the template branches on that name as a plain boolean
    /// (Cohere North Mini Code; the engine maps an off request onto it). `None` = no switch, so
    /// neither on nor off changes what the template renders. Rapid-MLX decides the second case
    /// with a full context-read/scoping analysis; this port requires only that `reasoning` is read
    /// and tested bare (`if reasoning` / `if not reasoning`) outside macro and call bodies.
    pub thinking_switch: Option<String>,
    /// The effort vocabulary the template VALIDATES `reasoning_effort` against, in template order
    /// (Qwen3.8: `xhigh`, `medium`, `low`). Empty = the template declares none; the engine then
    /// maps a graded effort onto a `reasoning_max_tokens` cap instead.
    pub effort_levels: Vec<String>,
    /// The level the template uses when the request names none: the literal of the
    /// `reasoning_effort|default('…')` it validates, or the literal its validation block assigns
    /// when the value is undefined. `None` when neither is provable.
    pub default_effort: Option<String>,
    /// The template reads `preserve_thinking` (Qwen3.8: history rows keep their `<think>` block
    /// unless it is set false).
    pub preserve_thinking: bool,
    /// The template renders a `<think>`…`</think>` span — the one shape the engine's
    /// `reasoning_max_tokens` force-close can close. Necessary, not sufficient: the engine also
    /// requires `</think>` to be a single token and skips the force-close on tool requests.
    pub budget_forcible: bool,
}

static CACHE: LazyLock<Mutex<HashMap<u64, Result<ThinkingCapabilities, String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The capability record for the model directory's tool-use chat template (every goose agent turn
/// carries tools, so that is the template the engine renders with). Parsed once per distinct
/// template source. Errors name the missing or unparseable template.
pub fn model_thinking_capabilities(dir: &Path) -> Result<ThinkingCapabilities> {
    let source = crate::model_parsers::tool_use_chat_template(dir)?.ok_or_else(|| {
        anyhow!(
            "no chat template in {} (chat_template.jinja, additional_chat_templates/tool_use.jinja, or tokenizer_config.json chat_template)",
            dir.display()
        )
    })?;
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let key = hasher.finish();
    let mut cache = CACHE.lock().expect("thinking capability cache poisoned");
    cache
        .entry(key)
        .or_insert_with(|| template_thinking_capabilities(&source).map_err(|e| format!("{e:#}")))
        .clone()
        .map_err(|e| anyhow!("{e}"))
}

/// The capability record for one template source.
pub fn template_thinking_capabilities(source: &str) -> Result<ThinkingCapabilities> {
    let source = generation_blocks_as_with(source);
    let tree = parse(
        &source,
        "chat_template",
        Default::default(),
        WhitespaceConfig::default(),
    )
    .map_err(|e| anyhow!("chat template does not parse: {e}"))?;
    let Stmt::Template(template) = &tree else {
        return Err(anyhow!("chat template parsed to a non-template root"));
    };
    let body = &template.children;

    let effort = if binds_name(body, "raise_exception") {
        None
    } else {
        let mut forgotten = HashSet::new();
        let derived = HashMap::from([("reasoning_effort".to_string(), None)]);
        walk_for_validation(body, &derived, &mut forgotten, &source)
    };

    let mut reads = HashSet::new();
    let mut tested = HashSet::new();
    collect_reads(body, &mut reads, &mut tested, false);
    let thinking_switch = if reads.contains("enable_thinking") {
        Some("enable_thinking".to_string())
    } else if reads.contains("reasoning") && tested.contains("reasoning") {
        Some("reasoning".to_string())
    } else {
        None
    };

    let (effort_levels, default_effort) = match effort {
        Some(found) => (found.levels, found.default),
        None => (Vec::new(), None),
    };
    Ok(ThinkingCapabilities {
        thinking_switch,
        effort_levels,
        default_effort,
        preserve_thinking: reads.contains("preserve_thinking"),
        budget_forcible: source.contains("<think>") && source.contains("</think>"),
    })
}

/// The `chat_template_kwargs` a profile's thinking choices put on a request. `None` when both are
/// at their defaults (auto / the template's own level): the request then carries nothing new and
/// is byte-identical to one built before these choices existed.
pub fn chat_template_kwargs(profile: &ModelProfile) -> Option<Map<String, Value>> {
    let mut kwargs = Map::new();
    if let Some(mode) = profile.thinking {
        kwargs.insert(
            "enable_thinking".to_string(),
            Value::Bool(mode == ThinkingMode::On),
        );
    }
    if let Some(effort) = &profile.reasoning_effort {
        kwargs.insert(
            "reasoning_effort".to_string(),
            Value::String(effort.clone()),
        );
    }
    (!kwargs.is_empty()).then_some(kwargs)
}

/// Refuses thinking choices the model's template cannot honour: a switch on a template that has
/// none, or an effort outside the template's own vocabulary (Qwen3.8 raises on one — every turn
/// would fail). A template that could not be read refuses any non-default choice, naming why.
pub fn validate_thinking_choices(
    profile: &ModelProfile,
    capabilities: Result<ThinkingCapabilities>,
) -> Result<()> {
    if profile.thinking.is_none() && profile.reasoning_effort.is_none() {
        return Ok(());
    }
    let capabilities =
        capabilities.map_err(|e| anyhow!("cannot verify the thinking choices: {e:#}"))?;
    if profile.thinking.is_some() && capabilities.thinking_switch.is_none() {
        bail!("the chat template has no thinking switch, so thinking on/off would change nothing");
    }
    if let Some(effort) = &profile.reasoning_effort {
        if !capabilities.effort_levels.contains(effort) {
            if capabilities.effort_levels.is_empty() {
                bail!("reasoning effort '{effort}': the chat template declares no effort levels");
            }
            bail!(
                "reasoning effort '{effort}' is not one of the chat template's levels ({})",
                capabilities.effort_levels.join(", ")
            );
        }
    }
    Ok(())
}

/// Transformers' `{% generation %}` span marker, which the engine's parser accepts as a plain
/// scope, becomes an empty `{% with %}` — the minijinja statement the walk treats identically
/// (not searched, binds nothing).
fn generation_blocks_as_with(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(open) = rest.find("{%") {
        out.push_str(&rest[..open + 2]);
        rest = &rest[open + 2..];
        let modifier = rest
            .strip_prefix('-')
            .or_else(|| rest.strip_prefix('+'))
            .map(|after| (&rest[..1], after))
            .unwrap_or(("", rest));
        let after_ws = modifier.1.trim_start();
        let ws = &modifier.1[..modifier.1.len() - after_ws.len()];
        let word_len = after_ws
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after_ws.len());
        let replacement = match &after_ws[..word_len] {
            "generation" => Some("with"),
            "endgeneration" => Some("endwith"),
            _ => None,
        };
        if let Some(replacement) = replacement {
            out.push_str(modifier.0);
            out.push_str(ws);
            out.push_str(replacement);
            rest = &after_ws[word_len..];
        }
    }
    out.push_str(rest);
    out
}

struct Found {
    levels: Vec<String>,
    default: Option<String>,
}

fn expr_span(expr: &Expr<'_>) -> Span {
    match expr {
        Expr::Var(e) => e.span(),
        Expr::Const(e) => e.span(),
        Expr::Slice(e) => e.span(),
        Expr::UnaryOp(e) => e.span(),
        Expr::BinOp(e) => e.span(),
        Expr::Compare(e) => e.span(),
        Expr::IfExpr(e) => e.span(),
        Expr::Filter(e) => e.span(),
        Expr::Test(e) => e.span(),
        Expr::GetAttr(e) => e.span(),
        Expr::GetItem(e) => e.span(),
        Expr::Call(e) => e.span(),
        Expr::List(e) => e.span(),
        Expr::Map(e) => e.span(),
    }
}

fn call_arg_expr<'e, 'a>(arg: &'e CallArg<'a>) -> &'e Expr<'a> {
    match arg {
        CallArg::Pos(e) | CallArg::Kwarg(_, e) | CallArg::PosSplat(e) | CallArg::KwargSplat(e) => e,
    }
}

/// Every sub-expression of `expr`, `expr` included, in source order.
fn visit_expr<'a>(expr: &Expr<'a>, f: &mut dyn FnMut(&Expr<'a>)) {
    f(expr);
    match expr {
        Expr::Var(_) | Expr::Const(_) => {}
        Expr::Slice(e) => {
            visit_expr(&e.expr, f);
            for part in [&e.start, &e.stop, &e.step].into_iter().flatten() {
                visit_expr(part, f);
            }
        }
        Expr::UnaryOp(e) => visit_expr(&e.expr, f),
        Expr::BinOp(e) => {
            visit_expr(&e.left, f);
            visit_expr(&e.right, f);
        }
        Expr::Compare(e) => {
            visit_expr(&e.expr, f);
            for op in &e.ops {
                visit_expr(&op.expr, f);
            }
        }
        Expr::IfExpr(e) => {
            visit_expr(&e.test_expr, f);
            visit_expr(&e.true_expr, f);
            if let Some(false_expr) = &e.false_expr {
                visit_expr(false_expr, f);
            }
        }
        Expr::Filter(e) => {
            if let Some(inner) = &e.expr {
                visit_expr(inner, f);
            }
            for arg in &e.args {
                visit_expr(call_arg_expr(arg), f);
            }
        }
        Expr::Test(e) => {
            visit_expr(&e.expr, f);
            for arg in &e.args {
                visit_expr(call_arg_expr(arg), f);
            }
        }
        Expr::GetAttr(e) => visit_expr(&e.expr, f),
        Expr::GetItem(e) => {
            visit_expr(&e.expr, f);
            visit_expr(&e.subscript_expr, f);
        }
        Expr::Call(e) => {
            visit_expr(&e.expr, f);
            for arg in &e.args {
                visit_expr(call_arg_expr(arg), f);
            }
        }
        Expr::List(e) => {
            for item in &e.items {
                visit_expr(item, f);
            }
        }
        Expr::Map(e) => {
            for part in e.keys.iter().chain(e.values.iter()) {
                visit_expr(part, f);
            }
        }
    }
}

fn references_any(expr: &Expr<'_>, names: &HashSet<String>) -> bool {
    let mut hit = false;
    visit_expr(expr, &mut |e| {
        if let Expr::Var(v) = e {
            hit |= names.contains(v.id);
        }
    });
    hit
}

/// Names an assignment target binds. Jinja2's `set ns.attr = …` target is an `NSRef` holding no
/// `Name` node, so a dotted target binds nothing — the same answer the engine's walk reaches.
fn target_names<'a>(target: &Expr<'a>, out: &mut Vec<&'a str>) {
    match target {
        Expr::Var(v) => out.push(v.id),
        Expr::List(list) => {
            for item in &list.items {
                target_names(item, out);
            }
        }
        _ => {}
    }
}

/// `_bound_names`: what a statement binds in the enclosing scope, per the engine's field rule
/// (`target` / `targets` / `names`, plus a macro's own name). A call block binds nothing.
fn bound_names<'a>(stmt: &Stmt<'a>) -> Vec<&'a str> {
    let mut out = Vec::new();
    match stmt {
        Stmt::Set(s) => target_names(&s.target, &mut out),
        Stmt::SetBlock(s) => target_names(&s.target, &mut out),
        Stmt::ForLoop(s) => target_names(&s.target, &mut out),
        Stmt::WithBlock(s) => {
            for (target, _) in &s.assignments {
                target_names(target, &mut out);
            }
        }
        Stmt::Macro(s) => out.push(s.name),
        Stmt::Import(s) => target_names(&s.name, &mut out),
        Stmt::FromImport(s) => {
            for (name, alias) in &s.names {
                target_names(name, &mut out);
                if let Some(alias) = alias {
                    target_names(alias, &mut out);
                }
            }
        }
        _ => {}
    }
    out
}

/// The statement lists nested directly inside `stmt`.
fn child_bodies<'s, 'a>(stmt: &'s Stmt<'a>) -> Vec<&'s [Stmt<'a>]> {
    match stmt {
        Stmt::Template(s) => vec![&s.children],
        Stmt::ForLoop(s) => vec![&s.body, &s.else_body],
        Stmt::IfCond(s) => vec![&s.true_body, &s.false_body],
        Stmt::WithBlock(s) => vec![&s.body],
        Stmt::SetBlock(s) => vec![&s.body],
        Stmt::AutoEscape(s) => vec![&s.body],
        Stmt::FilterBlock(s) => vec![&s.body],
        Stmt::Block(s) => vec![&s.body],
        Stmt::Macro(s) => vec![&s.body],
        Stmt::CallBlock(s) => vec![&s.macro_decl.body],
        _ => Vec::new(),
    }
}

/// `_binds_name`: whether ANY statement in the tree can shadow `name` (a local macro, import or
/// assignment named `raise_exception` turns an apparent rejection into a successful render).
fn binds_name(stmts: &[Stmt<'_>], name: &str) -> bool {
    stmts.iter().any(|stmt| {
        bound_names(stmt).contains(&name)
            || child_bodies(stmt)
                .into_iter()
                .any(|body| binds_name(body, name))
    })
}

/// `_forget_assignments_in`: every name any nested `set` may have overwritten.
fn forget_assignments_in(stmts: &[Stmt<'_>], forgotten: &mut HashSet<String>) {
    for stmt in stmts {
        let mut names = Vec::new();
        match stmt {
            Stmt::Set(s) => target_names(&s.target, &mut names),
            Stmt::SetBlock(s) => target_names(&s.target, &mut names),
            _ => {}
        }
        forgotten.extend(names.into_iter().map(str::to_string));
        for body in child_bodies(stmt) {
            forget_assignments_in(body, forgotten);
        }
    }
}

/// `_VALUE_PRESERVING_FILTERS`: filters that hand an effort name through unchanged.
const VALUE_PRESERVING_FILTERS: [&str; 4] = ["default", "trim", "lower", "string"];

/// `_value_preserving_source`: the variable `expr` carries through unchanged, with the literal a
/// `default('…')` in the chain supplies when that variable is undefined.
fn value_preserving_source<'a>(expr: &Expr<'a>) -> Option<(&'a str, Option<String>)> {
    let mut expr = expr;
    let mut default = None;
    loop {
        match expr {
            Expr::Filter(filter) if VALUE_PRESERVING_FILTERS.contains(&filter.name) => {
                if filter.name == "default" {
                    if let Some(CallArg::Pos(Expr::Const(c))) = filter.args.first() {
                        if let Some(literal) = c.value.as_str() {
                            default = Some(literal.to_string());
                        }
                    }
                }
                expr = filter.expr.as_ref()?;
            }
            Expr::Var(v) => return Some((v.id, default)),
            _ => return None,
        }
    }
}

fn is_named_test(expr: &Expr<'_>, variable: &str, test: &str) -> bool {
    matches!(expr, Expr::Test(t) if t.name == test && matches!(&t.expr, Expr::Var(v) if v.id == variable))
}

/// `_is_definedness_guard`: a disjunct true only when there is no value to validate.
fn is_definedness_guard(expr: &Expr<'_>, tested: &str) -> bool {
    match expr {
        Expr::UnaryOp(op) if matches!(op.op, ast::UnaryOpKind::Not) => match &op.expr {
            Expr::Var(v) => v.id == tested,
            inner => is_named_test(inner, tested, "defined"),
        },
        _ => is_named_test(expr, tested, "undefined") || is_named_test(expr, tested, "none"),
    }
}

fn disjuncts<'e, 'a>(expr: &'e Expr<'a>, out: &mut Vec<&'e Expr<'a>>) {
    match expr {
        Expr::BinOp(op) if matches!(op.op, ast::BinOpKind::ScOr) => {
            disjuncts(&op.left, out);
            disjuncts(&op.right, out);
        }
        _ => out.push(expr),
    }
}

/// Jinja2's `x not in y` Compare. minijinja parses both `x not in y` and `not x in y` into
/// `Not(In(x, y))`; only the first carries `not` between the operands, and only the first is
/// the engine's membership test.
fn as_not_in<'e, 'a>(expr: &'e Expr<'a>, source: &str) -> Option<(&'e Expr<'a>, &'e Expr<'a>)> {
    let Expr::UnaryOp(op) = expr else {
        return None;
    };
    if !matches!(op.op, ast::UnaryOpKind::Not) {
        return None;
    }
    let Expr::BinOp(bin) = &op.expr else {
        return None;
    };
    if !matches!(bin.op, ast::BinOpKind::In) {
        return None;
    }
    let between = source.get(
        expr_span(&bin.left).end_offset as usize..expr_span(&bin.right).start_offset as usize,
    )?;
    between
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|word| word == "not")
        .then_some((&bin.left, &bin.right))
}

struct Membership<'e, 'a> {
    levels_expr: &'e Expr<'a>,
    tested: &'a str,
    has_definedness_guard: bool,
}

/// `_guaranteed_membership`: the single `<x> not in <y>` whose failure alone enters the block.
fn guaranteed_membership<'e, 'a>(test: &'e Expr<'a>, source: &str) -> Option<Membership<'e, 'a>> {
    let mut parts = Vec::new();
    disjuncts(test, &mut parts);
    let compares: Vec<_> = parts
        .iter()
        .filter_map(|part| as_not_in(part, source).map(|pair| (*part, pair)))
        .collect();
    let [(compare, (left, right))] = compares.as_slice() else {
        return None;
    };
    let (tested, _) = value_preserving_source(left)?;
    let mut has_definedness_guard = false;
    for part in &parts {
        if std::ptr::eq(*part, *compare) {
            continue;
        }
        if !is_definedness_guard(part, tested) {
            return None;
        }
        has_definedness_guard = true;
    }
    Some(Membership {
        levels_expr: right,
        tested,
        has_definedness_guard,
    })
}

fn literal_levels(expr: &Expr<'_>) -> Option<Vec<String>> {
    let Expr::List(list) = expr else {
        return None;
    };
    let mut levels: Vec<String> = Vec::new();
    for item in &list.items {
        let Expr::Const(c) = item else {
            return None;
        };
        let level = c.value.as_str()?;
        if !levels.iter().any(|l| l == level) {
            levels.push(level.to_string());
        }
    }
    (!levels.is_empty()).then_some(levels)
}

fn is_bare_raise(expr: &Expr<'_>) -> bool {
    matches!(expr, Expr::Call(call) if matches!(&call.expr, Expr::Var(v) if v.id == "raise_exception"))
}

enum Rejection {
    Raises,
    Assigns(String),
}

/// `_body_unconditionally_rejects_or_defaults`: a top-level bare `raise_exception(...)` or a
/// re-assignment of the tested name to a literal from the same set.
fn body_rejects_or_defaults(
    body: &[Stmt<'_>],
    tested: &str,
    levels: &[String],
) -> Option<Rejection> {
    for stmt in body {
        match stmt {
            Stmt::EmitExpr(emit) if is_bare_raise(&emit.expr) => return Some(Rejection::Raises),
            Stmt::Set(set) => {
                if let (Expr::Var(target), Expr::Const(c)) = (&set.target, &set.expr) {
                    if target.id == tested {
                        if let Some(literal) = c.value.as_str() {
                            if levels.iter().any(|l| l == literal) {
                                return Some(Rejection::Assigns(literal.to_string()));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

type Derived = HashMap<String, Option<String>>;

fn validation_levels(
    test: &Expr<'_>,
    body: &[Stmt<'_>],
    derived: &Derived,
    forgotten: &HashSet<String>,
    source: &str,
) -> Option<Found> {
    let membership = guaranteed_membership(test, source)?;
    let tested = membership.tested;
    let carried_default = derived.get(tested)?.clone();
    if forgotten.contains(tested) {
        return None;
    }
    let levels = literal_levels(membership.levels_expr)?;
    let rejection = body_rejects_or_defaults(body, tested, &levels)?;
    let default = match (carried_default, rejection) {
        (Some(d), _) if levels.contains(&d) => Some(d),
        (_, Rejection::Assigns(literal)) if membership.has_definedness_guard => Some(literal),
        _ => None,
    };
    Some(Found { levels, default })
}

/// `_is_thinking_enabled_guard`: `enable_thinking is undefined or enable_thinking is true`.
fn is_thinking_enabled_guard(expr: &Expr<'_>) -> bool {
    let Expr::BinOp(op) = expr else {
        return false;
    };
    if !matches!(op.op, ast::BinOpKind::ScOr) {
        return false;
    }
    let parts = [&op.left, &op.right];
    parts
        .iter()
        .any(|p| is_named_test(p, "enable_thinking", "undefined"))
        && parts
            .iter()
            .any(|p| is_named_test(p, "enable_thinking", "true"))
}

fn is_thinking_disabled_guard(expr: &Expr<'_>) -> bool {
    is_named_test(expr, "enable_thinking", "false")
}

/// An `if` / `elif` … / `else` chain in Jinja2's shape. minijinja nests each `elif` as the sole
/// `IfCond` of its predecessor's else-body; one that starts at the `elif` keyword is a chain link,
/// an `else` holding a nested `if` is not.
fn if_chain<'s, 'a>(
    first: &'s ast::Spanned<ast::IfCond<'a>>,
    source: &str,
) -> (Vec<(&'s Expr<'a>, &'s [Stmt<'a>])>, &'s [Stmt<'a>]) {
    let mut branches = vec![(&first.expr, first.true_body.as_slice())];
    let mut current = first;
    loop {
        if let [Stmt::IfCond(next)] = current.false_body.as_slice() {
            let at = next.span().start_offset as usize;
            if source
                .get(at..)
                .is_some_and(|rest| rest.starts_with("elif"))
            {
                branches.push((&next.expr, next.true_body.as_slice()));
                current = next;
                continue;
            }
        }
        return (branches, current.false_body.as_slice());
    }
}

/// `_walk_for_validation`: the forward, scope-aware walk along the render path.
fn walk_for_validation(
    stmts: &[Stmt<'_>],
    derived: &Derived,
    forgotten: &mut HashSet<String>,
    source: &str,
) -> Option<Found> {
    let mut derived = derived.clone();
    for stmt in stmts {
        match stmt {
            Stmt::Set(set) => {
                if let Expr::Var(target) = &set.target {
                    let carried = value_preserving_source(&set.expr).and_then(|(src, default)| {
                        let inherited = derived.get(src)?;
                        (!forgotten.contains(src)).then(|| default.or_else(|| inherited.clone()))
                    });
                    match carried {
                        Some(default) => {
                            derived.insert(target.id.to_string(), default);
                        }
                        None => {
                            derived.remove(target.id);
                            forgotten.insert(target.id.to_string());
                        }
                    }
                } else {
                    let mut names = Vec::new();
                    target_names(&set.target, &mut names);
                    for name in names {
                        derived.remove(name);
                        forgotten.insert(name.to_string());
                    }
                }
            }
            Stmt::SetBlock(set) => {
                if let Expr::Var(target) = &set.target {
                    derived.remove(target.id);
                    forgotten.insert(target.id.to_string());
                }
            }
            Stmt::IfCond(first) => {
                let (branches, else_body) = if_chain(first, source);
                let mut effort_dependent = false;
                let mut branch_path_is_safe = true;
                for (test, body) in &branches {
                    if !effort_dependent && branch_path_is_safe {
                        if let Some(found) =
                            validation_levels(test, body, &derived, forgotten, source)
                        {
                            return Some(found);
                        }
                    }
                    let live: HashSet<String> = derived
                        .keys()
                        .filter(|name| !forgotten.contains(*name))
                        .cloned()
                        .collect();
                    if references_any(test, &live) {
                        effort_dependent = true;
                    }
                    branch_path_is_safe = branch_path_is_safe && is_thinking_disabled_guard(test);
                }
                if effort_dependent {
                    for (_, body) in &branches {
                        forget_assignments_in(body, forgotten);
                    }
                    forget_assignments_in(else_body, forgotten);
                    continue;
                }
                let mut searched = vec![false; branches.len()];
                let mut prior_only_disable = true;
                for (index, (test, body)) in branches.iter().enumerate() {
                    if prior_only_disable && is_thinking_enabled_guard(test) {
                        let found = walk_for_validation(body, &derived, forgotten, source);
                        searched[index] = true;
                        if found.is_some() {
                            return found;
                        }
                    }
                    prior_only_disable = prior_only_disable && is_thinking_disabled_guard(test);
                }
                let mut else_searched = false;
                if prior_only_disable {
                    let found = walk_for_validation(else_body, &derived, forgotten, source);
                    else_searched = true;
                    if found.is_some() {
                        return found;
                    }
                }
                for (index, (_, body)) in branches.iter().enumerate() {
                    if !searched[index] {
                        forget_assignments_in(body, forgotten);
                    }
                }
                if !else_searched {
                    forget_assignments_in(else_body, forgotten);
                }
            }
            other => {
                for name in bound_names(other) {
                    derived.remove(name);
                    forgotten.insert(name.to_string());
                }
            }
        }
    }
    None
}

fn truthiness_tested_name<'a>(test: &Expr<'a>) -> Option<&'a str> {
    let test = match test {
        Expr::UnaryOp(op) if matches!(op.op, ast::UnaryOpKind::Not) => &op.expr,
        other => other,
    };
    match test {
        Expr::Var(v) => Some(v.id),
        _ => None,
    }
}

/// Names the template reads (every variable load outside an assignment target), and the names
/// it branches on as a bare boolean outside deferred macro / call bodies.
fn collect_reads(
    stmts: &[Stmt<'_>],
    reads: &mut HashSet<String>,
    tested: &mut HashSet<String>,
    deferred: bool,
) {
    let read = |expr: &Expr<'_>, reads: &mut HashSet<String>, tested: &mut HashSet<String>| {
        visit_expr(expr, &mut |e| match e {
            Expr::Var(v) => {
                reads.insert(v.id.to_string());
            }
            Expr::IfExpr(if_expr) if !deferred => {
                if let Some(name) = truthiness_tested_name(&if_expr.test_expr) {
                    tested.insert(name.to_string());
                }
            }
            _ => {}
        })
    };
    for stmt in stmts {
        match stmt {
            Stmt::EmitExpr(s) => read(&s.expr, reads, tested),
            Stmt::ForLoop(s) => {
                read(&s.iter, reads, tested);
                if let Some(filter) = &s.filter_expr {
                    read(filter, reads, tested);
                }
            }
            Stmt::IfCond(s) => {
                if !deferred {
                    if let Some(name) = truthiness_tested_name(&s.expr) {
                        tested.insert(name.to_string());
                    }
                }
                read(&s.expr, reads, tested);
            }
            Stmt::WithBlock(s) => {
                for (_, value) in &s.assignments {
                    read(value, reads, tested);
                }
            }
            Stmt::Set(s) => read(&s.expr, reads, tested),
            Stmt::SetBlock(s) => {
                if let Some(filter) = &s.filter {
                    read(filter, reads, tested);
                }
            }
            Stmt::AutoEscape(s) => read(&s.enabled, reads, tested),
            Stmt::FilterBlock(s) => read(&s.filter, reads, tested),
            Stmt::Import(s) => read(&s.expr, reads, tested),
            Stmt::FromImport(s) => read(&s.expr, reads, tested),
            Stmt::Extends(s) => read(&s.name, reads, tested),
            Stmt::Include(s) => read(&s.name, reads, tested),
            Stmt::Macro(s) => {
                for default in &s.defaults {
                    read(default, reads, tested);
                }
                collect_reads(&s.body, reads, tested, true);
                continue;
            }
            Stmt::CallBlock(s) => {
                visit_expr(&s.call.expr, &mut |e| {
                    if let Expr::Var(v) = e {
                        reads.insert(v.id.to_string());
                    }
                });
                for arg in &s.call.args {
                    read(call_arg_expr(arg), reads, tested);
                }
                collect_reads(&s.macro_decl.body, reads, tested, true);
                continue;
            }
            Stmt::Do(s) => {
                read(&s.call.expr, reads, tested);
                for arg in &s.call.args {
                    read(call_arg_expr(arg), reads, tested);
                }
            }
            _ => {}
        }
        for body in child_bodies(stmt) {
            collect_reads(body, reads, tested, deferred);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QWEN38: &str = include_str!("../tests/fixtures/chat_templates/qwen3.8.jinja");

    fn caps(source: &str) -> ThinkingCapabilities {
        template_thinking_capabilities(source).unwrap()
    }

    fn levels(source: &str) -> Vec<String> {
        caps(source).effort_levels
    }

    #[test]
    fn qwen38_declares_its_switch_levels_default_and_preserve() {
        assert_eq!(
            caps(QWEN38),
            ThinkingCapabilities {
                thinking_switch: Some("enable_thinking".into()),
                effort_levels: vec!["xhigh".into(), "medium".into(), "low".into()],
                default_effort: Some("xhigh".into()),
                preserve_thinking: true,
                budget_forcible: true,
            }
        );
    }

    #[test]
    fn the_engines_bundled_gemma4_template_has_a_switch_and_no_levels() {
        let gemma = include_str!("../tests/fixtures/chat_templates/gemma4_full.jinja");
        assert_eq!(
            caps(gemma),
            ThinkingCapabilities {
                thinking_switch: Some("enable_thinking".into()),
                effort_levels: vec![],
                default_effort: None,
                preserve_thinking: true,
                budget_forcible: false,
            }
        );
    }

    #[test]
    fn a_template_without_reasoning_declares_nothing() {
        let plain = "{%- for m in messages %}{{ '<|im_start|>' + m.role + '\\n' + m.content + '<|im_end|>\\n' }}{%- endfor %}{%- if add_generation_prompt %}{{ '<|im_start|>assistant\\n' }}{%- endif %}";
        assert_eq!(caps(plain), ThinkingCapabilities::default());
    }

    #[test]
    fn an_on_off_switch_without_validation_has_no_levels() {
        let qwen35 = "{%- if add_generation_prompt %}{{- '<|im_start|>assistant\\n' }}{%- if enable_thinking is defined and enable_thinking is false %}{{- '<think>\\n\\n</think>\\n\\n' }}{%- else %}{{- '<think>\\n' }}{%- endif %}{%- endif %}";
        let c = caps(qwen35);
        assert_eq!(c.thinking_switch.as_deref(), Some("enable_thinking"));
        assert!(c.effort_levels.is_empty());
        assert!(c.budget_forcible);
        assert!(!c.preserve_thinking);
    }

    #[test]
    fn hy3_definedness_guard_with_a_literal_default() {
        let hy3 = "{%- if not reasoning_effort is defined or reasoning_effort not in ['no_think', 'low', 'high'] %}{%- set reasoning_effort = 'no_think' %}{%- endif %}{{ reasoning_effort }}";
        let c = caps(hy3);
        assert_eq!(c.effort_levels, vec!["no_think", "low", "high"]);
        assert_eq!(c.default_effort.as_deref(), Some("no_think"));
    }

    #[test]
    fn interpolation_branching_and_sentinels_are_not_validation() {
        for source in [
            "Reasoning: {{ reasoning_effort }}",
            "{% if reasoning_effort in ('high', 'xhigh') %}deep{% endif %}",
            "{% if reasoning_effort != 'none' %}{{ raise_exception('x') }}{% endif %}",
            "{% if not reasoning_effort in ('a', 'b') %}{{ raise_exception('x') }}{% endif %}",
            "{% if reasoning_effort not in ('a', 'b') and x %}{{ raise_exception('x') }}{% endif %}",
            "{% if reasoning_effort not in ('a', 'b') or x %}{{ raise_exception('x') }}{% endif %}",
            "{% if reasoning_effort not in ('a', 'b') %}{% if y %}{{ raise_exception('x') }}{% endif %}{% endif %}",
            "{% for m in messages %}{% if reasoning_effort not in ('a', 'b') %}{{ raise_exception('x') }}{% endif %}{% endfor %}",
            "{% macro raise_exception(m) %}{% endmacro %}{% if reasoning_effort not in ('a', 'b') %}{{ raise_exception('x') }}{% endif %}",
            "{% set e = reasoning_effort == 'a' %}{% if e not in (true, false) %}{{ raise_exception('x') }}{% endif %}",
            "{% if x %}{% set e = 1 %}{% endif %}{% set e = reasoning_effort %}{% if e not in ('a', 'b') %}{{ raise_exception('x') }}{% endif %}",
        ] {
            assert!(levels(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn an_elif_after_an_effort_test_is_path_constrained() {
        let chain = "{% if reasoning_effort == 'a' %}x{% elif reasoning_effort not in ('a', 'b') %}{{ raise_exception('x') }}{% endif %}";
        assert!(levels(chain).is_empty());
        let nested_else = "{% if enable_thinking is false %}x{% else %}{% if reasoning_effort not in ('a', 'b') %}{{ raise_exception('x') }}{% endif %}{% endif %}";
        assert_eq!(levels(nested_else), vec!["a", "b"]);
    }

    #[test]
    fn generation_spans_parse_and_are_not_searched() {
        let source = "{% generation %}{% if reasoning_effort not in ('a', 'b') %}{{ raise_exception('x') }}{% endif %}{% endgeneration %}";
        assert!(levels(source).is_empty());
        let after = "{%- generation -%}x{%- endgeneration -%}{% if reasoning_effort not in ('a', 'b') %}{{ raise_exception('x') }}{% endif %}";
        assert_eq!(levels(after), vec!["a", "b"]);
    }

    #[test]
    fn a_cohere_style_boolean_switch_is_reasoning() {
        let north = "{% if reasoning %}<think>{% else %}<think></think>{% endif %}";
        assert_eq!(caps(north).thinking_switch.as_deref(), Some("reasoning"));
        let data_only = "{{ reasoning }}";
        assert_eq!(caps(data_only).thinking_switch, None);
    }

    #[test]
    fn default_choices_send_nothing_and_explicit_ones_send_the_kwargs() {
        assert_eq!(chat_template_kwargs(&ModelProfile::default()), None);
        let sampling_only = ModelProfile {
            temperature: Some(0.6),
            ..Default::default()
        };
        assert_eq!(chat_template_kwargs(&sampling_only), None);
        let on_low = ModelProfile {
            thinking: Some(ThinkingMode::On),
            reasoning_effort: Some("low".into()),
            ..Default::default()
        };
        assert_eq!(
            Value::Object(chat_template_kwargs(&on_low).unwrap()),
            serde_json::json!({"enable_thinking": true, "reasoning_effort": "low"})
        );
        let off = ModelProfile {
            thinking: Some(ThinkingMode::Off),
            ..Default::default()
        };
        assert_eq!(
            Value::Object(chat_template_kwargs(&off).unwrap()),
            serde_json::json!({"enable_thinking": false})
        );
    }

    #[test]
    fn choices_the_template_cannot_honour_are_refused() {
        let qwen = || template_thinking_capabilities(QWEN38);
        let plain = || template_thinking_capabilities("{{ messages }}");
        let with = |thinking, effort: Option<&str>| ModelProfile {
            thinking,
            reasoning_effort: effort.map(str::to_string),
            ..Default::default()
        };
        validate_thinking_choices(&with(None, None), Err(anyhow!("unreadable"))).unwrap();
        validate_thinking_choices(&with(Some(ThinkingMode::On), Some("medium")), qwen()).unwrap();
        let err = validate_thinking_choices(&with(None, Some("high")), qwen()).unwrap_err();
        assert!(format!("{err:#}").contains("xhigh, medium, low"), "{err:#}");
        let err =
            validate_thinking_choices(&with(Some(ThinkingMode::Off), None), plain()).unwrap_err();
        assert!(format!("{err:#}").contains("no thinking switch"), "{err:#}");
        let err = validate_thinking_choices(&with(None, Some("low")), plain()).unwrap_err();
        assert!(
            format!("{err:#}").contains("declares no effort levels"),
            "{err:#}"
        );
        let err = validate_thinking_choices(
            &with(Some(ThinkingMode::On), None),
            Err(anyhow!("no chat template in /x")),
        )
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("no chat template in /x"),
            "{err:#}"
        );
    }

    #[test]
    fn an_unparseable_template_is_a_named_error() {
        let err = template_thinking_capabilities("{% if %}").unwrap_err();
        assert!(format!("{err:#}").contains("does not parse"), "{err:#}");
    }

    #[test]
    fn model_dir_capabilities_read_the_tool_use_template_and_name_its_absence() {
        let dir = tempfile::tempdir().unwrap();
        let err = model_thinking_capabilities(dir.path()).unwrap_err();
        assert!(format!("{err:#}").contains("no chat template"), "{err:#}");
        std::fs::write(dir.path().join("chat_template.jinja"), QWEN38).unwrap();
        assert_eq!(
            model_thinking_capabilities(dir.path())
                .unwrap()
                .effort_levels,
            vec!["xhigh", "medium", "low"]
        );
        std::fs::write(
            dir.path().join("tokenizer_config.json"),
            serde_json::json!({"chat_template": "{{ messages }}"}).to_string(),
        )
        .unwrap();
        assert_eq!(
            model_thinking_capabilities(dir.path())
                .unwrap()
                .thinking_switch
                .as_deref(),
            Some("enable_thinking"),
            "a standalone chat_template.jinja outranks the tokenizer_config copy"
        );
    }
}
