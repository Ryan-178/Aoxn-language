//! Aoxn type checker: strict, no implicit conversions, deterministic errors.
//! Generic functions are monomorphized at call sites: arguments are unified
//! against the declared param types (T / length N), a concrete instance is
//! cloned+substituted, queued for checking, and codegen receives the instance.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::ast::*;
use crate::hashing::FastBuild;
use crate::Diag;

/// resolved struct layout: ordered fields plus an O(1) name index (large
/// 40-field state structs make the linear scans measurable)
#[derive(Debug, Clone)]
pub struct StructInfo {
    pub fields: Vec<(String, Type)>,
    index: HashMap<String, usize, FastBuild>,
}

impl StructInfo {
    pub fn new(fields: Vec<(String, Type)>) -> Self {
        let index = fields
            .iter()
            .enumerate()
            .map(|(i, (n, _))| (n.clone(), i))
            .collect();
        Self { fields, index }
    }

    pub fn field_type(&self, name: &str) -> Option<&Type> {
        self.index.get(name).map(|&i| &self.fields[i].1)
    }
}

pub type StructTable = HashMap<String, StructInfo, FastBuild>;

/// internal maps keyed by short identifiers use the fast hasher
/// (see hashing.rs); their iteration order is never observable
type SigMap = HashMap<String, FnSig, FastBuild>;
type GenMap = HashMap<String, FnDecl, FastBuild>;
type Scopes = HashMap<String, Type, FastBuild>;

#[derive(Debug, Clone)]
pub struct FnSig {
    pub params: Vec<Type>,
    pub ret: Type,
}

/// mutable checking context shared by the recursive walkers
pub struct Tc<'a> {
    pub sigs: SigMap,
    pub structs: &'a StructTable,
    pub generics: &'a GenMap,
    /// file whose function body is currently being checked
    pub cur_file: u32,
    /// AST node address of a generic call -> mangled instance name
    pub call_map: HashMap<usize, String>,
    /// variables currently narrowed by `is None` (v0.40.0): name -> the
    /// DECLARED (nullable) type, kept so an assignment inside the branch can
    /// re-narrow (or unwind) the view against the real slot type
    narrowed: HashMap<String, Type>,
    /// instances pending body checking (FIFO: instance order must match the
    /// order codegen emits, so a VecDeque, never a `Vec::remove(0)`)
    queue: VecDeque<FnDecl>,
    /// instance names already created (dedup)
    done: HashSet<String>,
    /// checked instances, emitted by the codegen stage — these are the same
    /// objects that pass 4 checked, so their node addresses match call_map
    instances: Vec<FnDecl>,
    /// AOXN_TC_TRACE, read once per compile (not per function)
    trace: bool,
}

pub struct CheckOutput {
    pub structs: StructTable,
    pub sigs: SigMap,
    pub instances: Vec<FnDecl>,
    pub call_map: HashMap<usize, String>,
}

pub fn check(program: &Program) -> Result<CheckOutput, Diag> {
    let structs = collect_structs(&program.structs)?;
    let mut sigs: SigMap = HashMap::default();
    let mut generics: GenMap = HashMap::default();

    // pass 1: concrete functions (signatures first, enables mutual recursion)
    for f in &program.funcs {
        if !f.type_params.is_empty() {
            continue;
        }
        if f.is_extern && f.name == "main" {
            return Err(Diag::at("type", f.pos.file, f.pos.line, f.pos.col, "'main' cannot be declared extern"));
        }
        if sigs.contains_key(&f.name) {
            return Err(Diag::at("type", f.pos.file, f.pos.line, f.pos.col, format!("function '{}' is defined more than once", f.name)));
        }
        if structs.contains_key(&f.name) {
            return Err(Diag::at("type", f.pos.file, f.pos.line, f.pos.col, format!("'{}' is already defined as a struct", f.name)));
        }
        for (i, p) in f.params.iter().enumerate() {
            if f.params[..i].iter().any(|q| q.name == p.name) {
                return Err(Diag::at("type", p.pos.file, p.pos.line, p.pos.col, format!("duplicate parameter '{}' in function '{}'", p.name, f.name)));
            }
            resolve_ty(&p.ty, &structs).map_err(|m| Diag::at("type", p.pos.file, p.pos.line, p.pos.col, m))?;
            if p.ty == Type::Void {
                return Err(Diag::at("type", p.pos.file, p.pos.line, p.pos.col, format!("parameter '{}' cannot have type void", p.name)));
            }
            // v0.40.0: an `extern def` is a C declaration, and the surface has
            // no way to spell a C function-pointer parameter. Function
            // pointers live in Aoxn values; cross the FFI boundary through
            // `to_int` + `store_i64` instead.
            if f.is_extern {
                if let Type::FnPtr { .. } = p.ty {
                    return Err(Diag::at(
                        "type",
                        p.pos.file,
                        p.pos.line,
                        p.pos.col,
                        format!("extern function '{}' cannot take a function-pointer parameter; pass its address as an int", f.name),
                    ));
                }
            }
        }
        resolve_ty(&f.ret, &structs).map_err(|m| Diag::at("type", f.pos.file, f.pos.line, f.pos.col, m))?;
        if has_generic_len(&f.ret) {
            return Err(Diag::at("type", f.pos.file, f.pos.line, f.pos.col, "array length parameters are only valid inside generic functions"));
        }
        for p in &f.params {
            if has_generic_len(&p.ty) {
                return Err(Diag::at("type", p.pos.file, p.pos.line, p.pos.col, "array length parameters are only valid inside generic functions"));
            }
        }
        sigs.insert(
            f.name.clone(),
            FnSig {
                params: f.params.iter().map(|p| p.ty.clone()).collect(),
                ret: f.ret.clone(),
            },
        );
    }

    // pass 2: generic declarations (no body checking until instantiation)
    for f in &program.funcs {
        if f.type_params.is_empty() {
            continue;
        }
        if f.is_extern {
            return Err(Diag::at("type", f.pos.file, f.pos.line, f.pos.col, "extern functions cannot be generic"));
        }
        if f.name == "main" {
            return Err(Diag::at("type", f.pos.file, f.pos.line, f.pos.col, "'main' cannot be generic"));
        }
        if sigs.contains_key(&f.name) || generics.contains_key(&f.name) {
            return Err(Diag::at("type", f.pos.file, f.pos.line, f.pos.col, format!("function '{}' is defined more than once", f.name)));
        }
        if structs.contains_key(&f.name) {
            return Err(Diag::at("type", f.pos.file, f.pos.line, f.pos.col, format!("'{}' is already defined as a struct", f.name)));
        }
        generics.insert(f.name.clone(), f.clone());
    }

    if !sigs.contains_key("main") {
        return Err(Diag::at("type", 0, 1, 1, "program has no 'main' function"));
    }

    let mut tc = Tc {
        sigs,
        structs: &structs,
        generics: &generics,
        cur_file: 0,
        call_map: HashMap::new(),
        narrowed: HashMap::new(),
        queue: VecDeque::new(),
        done: HashSet::new(),
        instances: Vec::new(),
        trace: std::env::var("AOXN_TC_TRACE").is_ok(),
    };

    // pass 3: check concrete non-extern bodies
    for f in &program.funcs {
        if f.type_params.is_empty() && !f.is_extern {
            tc.cur_file = f.pos.file;
            tc.check_fn_body(f)?;
        }
    }

    // pass 4: drain monomorphized instances (they may enqueue more).
    // Each instance is checked as the same object that goes to codegen, so
    // call_map keys (AST node addresses) match during emission — nested
    // generic calls inside instance bodies depend on this.
    while !tc.queue.is_empty() {
        let inst = tc.queue.pop_front().unwrap();
        tc.cur_file = inst.pos.file;
        tc.check_fn_body(&inst)?;
        tc.instances.push(inst);
    }
    let Tc { sigs, call_map, instances, .. } = tc;

    Ok(CheckOutput {
        structs,
        sigs,
        instances,
        call_map,
    })
}

impl<'a> Tc<'a> {
    /// type-stage diagnostic in the file currently being checked (B1 helper)
    fn err(&self, line: usize, col: usize, message: impl Into<String>) -> Diag {
        Diag::at("type", self.cur_file, line, col, message)
    }

    fn check_fn_body(&mut self, f: &FnDecl) -> Result<(), Diag> {
        if self.trace {
            eprintln!("[tc] {}", f.name);
        }
        // Aoxn bindings are function-scoped (Python-like): blocks never pop,
        // `let` on an existing name is a re-assignment — names are unique,
        // so a map preserves the old reversed-linear-scan semantics
        let mut scopes: Scopes =
            f.params.iter().map(|p| (p.name.clone(), p.ty.clone())).collect();
        let returns_all = self.check_block(&f.body, f, &mut scopes, 0)?;
        // strict rule: non-void functions must return a value on every path
        if f.ret != Type::Void && !returns_all {
            return Err(Diag::at("type", f.pos.file, f.pos.line, f.pos.col, format!(
                    "function '{}' returns {} but does not return a value on all paths",
                    f.name, f.ret
                )));
        }
        Ok(())
    }

    fn check_block(
        &mut self,
        block: &Block,
        f: &FnDecl,
        scopes: &mut Scopes,
        loop_depth: usize,
    ) -> Result<bool, Diag> {
        let mut guarantees_return = false;
        for stmt in &block.stmts {
            if guarantees_return {
                let pos = stmt_pos(stmt);
                return Err(self.err(pos.line, pos.col, "unreachable statement after 'return'"));
            }
            self.check_stmt(stmt, f, scopes, loop_depth)?;
            guarantees_return = stmt_guarantees_return(stmt);
        }
        Ok(guarantees_return)
    }

    fn check_stmt(
        &mut self,
        stmt: &Stmt,
        f: &FnDecl,
        scopes: &mut Scopes,
        loop_depth: usize,
    ) -> Result<(), Diag> {
        match stmt {
            Stmt::Let { name, ty, expr, pos } => {
                let t = self.check_expr(expr, scopes)?;
                if t == Type::Void {
                    return Err(self.err(pos.line, pos.col, format!("cannot bind a void expression to '{name}'")));
                }
                match scopes.get(name) {
                    Some(dt) => {
                        // re-assignment: type is fixed at first binding. The
                        // comparison uses the DECLARED type when the view is
                        // narrowed (a `x: int | None = None` inside an
                        // `is not None` branch re-binds the real slot)
                        let declared = self.narrowed.get(name).cloned();
                        let real = declared.as_ref().unwrap_or(dt);
                        if let Some(ann) = ty {
                            if *ann != *real {
                                return Err(self.err(pos.line, pos.col, format!(
                                        "cannot re-declare '{name}' as {ann}: it is already {real}"
                                    )));
                            }
                        }
                        if !assignable(real, &t) {
                            return Err(self.err(pos.line, pos.col, format!("cannot assign a value of type {t} to '{name}: {real}'")));
                        }
                        // re-narrow the view (v0.40.0): `x = e` on a narrowed
                        // variable is a Let, and the branch now sees `t`
                        if self.narrowed.contains_key(name) {
                            if t == self.narrowed[name] {
                                self.narrowed.remove(name);
                            }
                            scopes.insert(name.clone(), t);
                        }
                    }
                    None => {
                        // first binding: annotation (if present) must match the initializer
                        if let Some(ann) = ty {
                            if !assignable(ann, &t) {
                                return Err(self.err(pos.line, pos.col, format!(
                                        "cannot initialize '{name}: {ann}' with an expression of type {t}"
                                    )));
                            }
                            scopes.insert(name.clone(), ann.clone());
                        } else {
                            scopes.insert(name.clone(), t);
                        }
                    }
                }
                Ok(())
            }
            Stmt::Assign { target, expr, pos } => {
                if !target.is_lvalue() {
                    return Err(self.err(pos.line, pos.col, "invalid assignment target"));
                }
                // a write goes to the DECLARED slot: a narrowed view must not
                // turn `x = None` inside `if x is not None:` into an error
                let dt = match target {
                    Expr::Var { name, .. } => self.narrowed.get(name).cloned().unwrap_or_else(|| scopes.get(name).cloned().unwrap_or(Type::Void)),
                    _ => self.lvalue_type(target, scopes)?,
                };
                let t = self.check_expr(expr, scopes)?;
                if !assignable(&dt, &t) {
                    return Err(self.err(pos.line, pos.col, format!("cannot assign a value of type {t} to a target of type {dt}")));
                }
                // re-narrow the view (v0.40.0): the variable now provably
                // holds a value of type t, so the branch sees exactly that
                if let Expr::Var { name, .. } = target {
                    if self.narrowed.contains_key(name) {
                        if t == self.narrowed[name] {
                            self.narrowed.remove(name);
                        }
                        scopes.insert(name.clone(), t);
                    }
                }
                Ok(())
            }
            Stmt::If { cond, then_block, else_block, pos } => {
                let t = self.check_expr(cond, scopes)?;
                if t != Type::Bool {
                    return Err(self.err(pos.line, pos.col, format!("'if' condition must be bool, found {t}")));
                }
                // nullable narrowing (v0.40.0): `if x is None:` gives the
                // then-branch `None` and the else-branch `T` (swapped for
                // `is not None`). Only a bare variable narrows — anything
                // else keeps its declared type and the test is just a bool.
                match narrow_target(cond, scopes) {
                    Some((var, then_ty, else_ty)) => {
                        // remember the DECLARED (nullable) slot type so an
                        // assignment inside the branch can re-narrow against it
                        self.narrowed.entry(var.clone()).or_insert_with(|| scopes.get(&var).cloned().unwrap_or(Type::Void));
                        let saved = scopes.get(&var).cloned();
                        self.narrow_scope(scopes, &var, &then_ty);
                        let r = self.check_block(then_block, f, scopes, loop_depth);
                        self.restore_scope(scopes, &var, saved.clone());
                        r?;
                        // a then-branch that always exits (return/raise) makes
                        // the code after the `if` the else-branch (v0.40.0)
                        let then_exits = block_returns_all(then_block);
                        match else_block {
                            Some(eb) => {
                                self.narrow_scope(scopes, &var, &else_ty);
                                let r = self.check_block(eb, f, scopes, loop_depth);
                                self.restore_scope(scopes, &var, saved);
                                r?;
                                if then_exits && !block_returns_all(eb) {
                                    self.narrow_scope(scopes, &var, &else_ty);
                                } else if block_returns_all(eb) && !then_exits {
                                    self.narrow_scope(scopes, &var, &then_ty);
                                }
                            }
                            None => {
                                if then_exits {
                                    self.narrow_scope(scopes, &var, &else_ty);
                                }
                            }
                        }
                    }
                    None => {
                        self.check_block(then_block, f, scopes, loop_depth)?;
                        if let Some(eb) = else_block {
                            self.check_block(eb, f, scopes, loop_depth)?;
                        }
                    }
                }
                Ok(())
            }
            Stmt::While { cond, body, pos } => {
                let t = self.check_expr(cond, scopes)?;
                if t != Type::Bool {
                    return Err(self.err(pos.line, pos.col, format!("'while' condition must be bool, found {t}")));
                }
                self.check_block(body, f, scopes, loop_depth + 1)?;
                Ok(())
            }
            Stmt::For { var, iter, body, pos } => {
                let var_ty = match iter {
                    ForIter::Range(args) => {
                        if args.is_empty() || args.len() > 3 {
                            return Err(self.err(pos.line, pos.col, format!("range expects 1 to 3 arguments, found {}", args.len())));
                        }
                        for a in args {
                            let t = self.check_expr(a, scopes)?;
                            if t != Type::Int {
                                return Err(self.err(a.pos().line, a.pos().col, format!("range arguments must be int, found {t}")));
                            }
                        }
                        Type::Int
                    }
                    ForIter::Array(e) => {
                        let t = self.check_expr(e, scopes)?;
                        match t {
                            Type::Array { elem, .. } => *elem,
                            other => {
                                return Err(self.err(pos.line, pos.col, format!("'for' can only iterate over arrays, found {other}")))
                            }
                        }
                    }
                };
                match scopes.get(var) {
                    Some(dt) => {
                        if *dt != var_ty {
                            return Err(self.err(pos.line, pos.col, format!("loop variable '{var}' is already {dt}, cannot reuse as {var_ty}")));
                        }
                    }
                    None => {
                        scopes.insert(var.clone(), var_ty);
                    }
                }
                self.check_block(body, f, scopes, loop_depth + 1)?;
                Ok(())
            }
            Stmt::Break { pos } => {
                if loop_depth == 0 {
                    return Err(self.err(pos.line, pos.col, "'break' outside of a loop"));
                }
                Ok(())
            }
            Stmt::Continue { pos } => {
                if loop_depth == 0 {
                    return Err(self.err(pos.line, pos.col, "'continue' outside of a loop"));
                }
                Ok(())
            }
            Stmt::Return { expr, pos } => {
                match (expr, &f.ret) {
                    (None, Type::Void) => Ok(()),
                    (None, ret) => Err(self.err(pos.line, pos.col, format!("'return' must return a value of type {ret}"))),
                    (Some(_), Type::Void) => Err(self.err(pos.line, pos.col, "void function cannot return a value")),
                    (Some(e), ret) => {
                        let t = self.check_expr(e, scopes)?;
                        if t != *ret {
                            return Err(self.err(pos.line, pos.col, format!("'return' type mismatch: expected {ret}, found {t}")));
                        }
                        Ok(())
                    }
                }
            }
            Stmt::Pass => Ok(()),
            Stmt::ExprStmt { expr } => {
                self.check_expr(expr, scopes)?;
                Ok(())
            }
        }
    }

    /// type of an assignment target (already validated as lvalue by the parser)
    fn narrow_scope(&mut self, scopes: &mut Scopes, var: &str, ty: &Type) {
        scopes.insert(var.to_string(), ty.clone());
    }

    fn restore_scope(&mut self, scopes: &mut Scopes, var: &str, saved: Option<Type>) {
        match saved {
            Some(t) => {
                scopes.insert(var.to_string(), t);
            }
            None => {
                scopes.remove(var);
            }
        }
    }

    fn lvalue_type(&mut self, target: &Expr, scopes: &mut Scopes) -> Result<Type, Diag> {
        match target {
            Expr::Var { name, pos } => scopes.get(name).cloned().ok_or_else(|| self.err(pos.line, pos.col, format!("assignment to undeclared variable '{name}'"))),
            Expr::Index { arr, idx, pos } => {
                let at = self.check_expr(arr, scopes)?;
                let it = self.check_expr(idx, scopes)?;
                if it != Type::Int {
                    return Err(self.err(pos.line, pos.col, format!("array index must be int, found {it}")));
                }
                match at {
                    Type::Array { elem, .. } => Ok(*elem),
                    other => Err(self.err(pos.line, pos.col, format!("cannot index a value of type {other}"))),
                }
            }
            Expr::Field { obj, name, pos } => {
                let ot = self.check_expr(obj, scopes)?;
                match field_type(&ot, name, self.structs) {
                    Some(fty) => Ok(fty.clone()),
                    None => Err(self.err(pos.line, pos.col, format!("type {ot} has no field '{name}'"))),
                }
            }
            other => {
                let pos = other.pos();
                Err(self.err(pos.line, pos.col, "invalid assignment target"))
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr, scopes: &mut Scopes) -> Result<Type, Diag> {
        match expr {
            Expr::Cast { expr: inner, to, pos } => {
                let from = self.check_expr(inner, scopes)?;
                let ok = matches!(
                    (&from, to),
                    (Type::Int, Type::Float)
                        | (Type::Float, Type::Int)
                        | (Type::Int, Type::Int)
                        | (Type::Float, Type::Float)
                        | (Type::Bool, Type::Bool)
                        | (Type::Str, Type::Str)
                        | (Type::Bool, Type::Int)
                        | (Type::Int, Type::Bool)
                        // v0.40.0: `as` is also the re-interpretation escape
                        // hatch — an int address becomes a callable function
                        // pointer and back (a COM vtable slot, a Win32 thunk)
                        | (Type::Int, Type::FnPtr { .. })
                        | (Type::FnPtr { .. }, Type::Int)
                );
                if !ok {
                    return Err(self.err(pos.line, pos.col, format!("invalid cast from {from} to {to}")));
                }
                Ok(to.clone())
            }
            Expr::Int(..) => Ok(Type::Int),
            Expr::Float(..) => Ok(Type::Float),
            Expr::Str(..) => Ok(Type::Str),
            Expr::Bool(..) => Ok(Type::Bool),
            Expr::NoneLit(..) => Ok(Type::None),
            Expr::Var { name, pos } => match scopes.get(name).cloned() {
                Some(t) => Ok(t),
                // a bare function name in value position is its address
                // (v0.40.0): `cb = handler` yields a function pointer typed by
                // the declaration. Locals shadow globals because `scopes` is
                // consulted first.
                None => match self.sigs.get(name) {
                    Some(sig) => Ok(Type::FnPtr {
                        ret: Box::new(sig.ret.clone()),
                        params: sig.params.clone(),
                    }),
                    None => Err(self.err(pos.line, pos.col, format!("unknown variable '{name}'"))),
                },
            },
            Expr::Index { arr, idx, pos } => {
                let at = self.check_expr(arr, scopes)?;
                let it = self.check_expr(idx, scopes)?;
                if it != Type::Int {
                    return Err(self.err(pos.line, pos.col, format!("array index must be int, found {it}")));
                }
                match at {
                    Type::Array { elem, .. } => Ok(*elem),
                    other => Err(self.err(pos.line, pos.col, format!("cannot index a value of type {other} (only [T; N] arrays are indexable)"))),
                }
            }
            Expr::Field { obj, name, pos } => {
                let ot = self.check_expr(obj, scopes)?;
                match field_type(&ot, name, self.structs) {
                    Some(fty) => Ok(fty.clone()),
                    None => Err(self.err(pos.line, pos.col, format!("type {ot} has no field '{name}'"))),
                }
            }
            Expr::ArrayLit { elems, pos, .. } => {
                if elems.is_empty() {
                    return Err(self.err(pos.line, pos.col, "empty array literals are not allowed"));
                }
                let first = self.check_expr(&elems[0], scopes)?;
                let mut elem = first.clone();
                // v0.40.0: `[1, None, 3]` infers `[int | None; 3]` — the one
                // nullable join the language makes implicitly, because a
                // literal is where absence shows up in practice
                if elems.len() > 1 {
                    let mut saw_none = first == Type::None;
                    let mut inner: Option<Type> = if first == Type::None { None } else { Some(first) };
                    for e in &elems[1..] {
                        let t = self.check_expr(e, scopes)?;
                        if t == Type::None {
                            saw_none = true;
                        } else if let Some(prev) = &inner {
                            if *prev != t {
                                return Err(self.err(e.pos().line, e.pos().col, format!("array literal elements must share one type: found {prev} and {t}")));
                            }
                        } else {
                            inner = Some(t);
                        }
                    }
                    elem = match (saw_none, inner) {
                        (true, Some(t)) => Type::Opt(Box::new(t)),
                        (true, None) => Type::None,
                        (false, Some(t)) => t,
                        (false, None) => unreachable!(),
                    };
                }
                Ok(Type::Array { elem: Box::new(elem), len: elems.len() })
            }
            Expr::ArrayRep { elem, count, pos, .. } => {
                if *count == 0 {
                    return Err(self.err(pos.line, pos.col, "array replication count must be positive"));
                }
                let t = self.check_expr(elem, scopes)?;
                Ok(Type::Array { elem: Box::new(t), len: *count })
            }
            Expr::StructLit { name, fields, pos, .. } => {
                let layout = self.structs.get(name).ok_or_else(|| self.err(pos.line, pos.col, format!("unknown struct '{name}'")))?;
                // every field exactly once (any order), types must match
                for (fname, fexpr) in fields {
                    let fty = layout
                        .field_type(fname)
                        .ok_or_else(|| self.err(fexpr.pos().line, fexpr.pos().col, format!("struct '{name}' has no field '{fname}'")))?;
                    let t = self.check_expr(fexpr, scopes)?;
                    if !assignable(fty, &t) {
                        return Err(self.err(fexpr.pos().line, fexpr.pos().col, format!(
                                "field '{fname}' of '{name}' must be {fty}, found {t}"
                            )));
                    }
                }
                if fields.len() != layout.fields.len() {
                    let provided: HashSet<&str> = fields.iter().map(|f| f.0.as_str()).collect();
                    let missing: Vec<String> = layout
                        .fields
                        .iter()
                        .filter(|(n, _)| !provided.contains(n.as_str()))
                        .map(|(n, _)| n.clone())
                        .collect();
                    return Err(self.err(pos.line, pos.col, format!(
                            "struct literal '{}' is missing field(s): {}",
                            name,
                            missing.join(", ")
                        )));
                }
                // duplicate field names in the literal (the error points at
                // the first occurrence, as before)
                let mut seen: HashMap<&str, usize> = HashMap::new();
                for (i, (fname, _)) in fields.iter().enumerate() {
                    if let Some(&first) = seen.get(fname.as_str()) {
                        let fpos = fields[first].1.pos();
                        return Err(self.err(fpos.line, fpos.col, format!("field '{fname}' given more than once in struct literal")));
                    }
                    seen.insert(fname.as_str(), i);
                }
                Ok(Type::Struct(name.clone()))
            }
            Expr::Call { name, args, pos, .. } => {
                // builtins first (they are not in the function table)
                if name == "print" {
                    if args.len() != 1 || args[0].name.is_some() {
                        return Err(self.err(pos.line, pos.col, "print expects exactly 1 positional argument"));
                    }
                    let t = self.check_expr(&args[0].value, scopes)?;
                    if !t.is_printable() {
                        return Err(self.err(pos.line, pos.col, format!("print requires int, float, bool, or string, found {t}")));
                    }
                    return Ok(Type::Void);
                }
                if name == "len" {
                    if args.len() != 1 || args[0].name.is_some() {
                        return Err(self.err(pos.line, pos.col, "len expects exactly 1 positional argument"));
                    }
                    let t = self.check_expr(&args[0].value, scopes)?;
                    if !matches!(t, Type::Array { .. } | Type::Str) {
                        return Err(self.err(pos.line, pos.col, format!("len requires an array or string, found {t}")));
                    }
                    return Ok(Type::Int);
                }
                if name == "str" {
                    if args.len() != 1 || args[0].name.is_some() {
                        return Err(self.err(pos.line, pos.col, "str expects exactly 1 positional argument"));
                    }
                    let t = self.check_expr(&args[0].value, scopes)?;
                    if !t.is_printable() {
                        return Err(self.err(pos.line, pos.col, format!("cannot convert {t} to string")));
                    }
                    return Ok(Type::Str);
                }
                // explicit scalar conversions (lowered to Expr::Cast semantics
                // at codegen): to_int(x) truncates toward zero, to_float(x)
                // widens (int/float are type keywords, so the builtins carry
                // a to_ prefix)
                if name == "to_int" || name == "to_float" {
                    if args.len() != 1 || args[0].name.is_some() {
                        return Err(self.err(pos.line, pos.col, format!("{name} expects exactly 1 positional argument")));
                    }
                    let t = self.check_expr(&args[0].value, scopes)?;
                    // to_int on a function pointer yields its raw address
                    // (v0.40.0) — the escape hatch that feeds a COM vtable or
                    // any other runtime-resolved call slot.
                    if name == "to_int" && matches!(t, Type::FnPtr { .. }) {
                        return Ok(Type::Int);
                    }
                    if !matches!(t, Type::Int | Type::Float | Type::Bool) {
                        return Err(self.err(pos.line, pos.col, format!("{name}() requires int, float, or bool, found {t}")));
                    }
                    return Ok(if name == "to_int" { Type::Int } else { Type::Float });
                }
                // raw memory primitives (the self-hosting escape hatch):
                // addresses are plain `int` (pointer-sized); use with care
                if name == "load_i64" || name == "load_f64" {
                    if args.len() != 1 || args[0].name.is_some() {
                        return Err(self.err(pos.line, pos.col, format!("{name} expects exactly 1 positional argument (address: int)")));
                    }
                    let t = self.check_expr(&args[0].value, scopes)?;
                    if t != Type::Int {
                        return Err(self.err(pos.line, pos.col, format!("{name} address must be int, found {t}")));
                    }
                    if name == "load_f64" {
                        return Ok(Type::Float);
                    }
                    return Ok(Type::Int);
                }
                if name == "load_u8" || name == "store_u8" {
                    // byte access with offset; the base may be an `int`
                    // address or a `string` (bytes of the string)
                    let want_args = if name == "load_u8" { 2 } else { 3 };
                    if args.len() != want_args || args.iter().any(|a| a.name.is_some()) {
                        return Err(self.err(pos.line, pos.col, format!(
                                "{name} expects ({}, offset: int{})",
                                if name == "load_u8" { "base: int|string)" } else { "base: int|string, value: int)" },
                                if name == "store_u8" { ")" } else { "" }
                            )));
                    }
                    let bt = self.check_expr(&args[0].value, scopes)?;
                    let ot = self.check_expr(&args[1].value, scopes)?;
                    if (bt != Type::Int && bt != Type::Str) || ot != Type::Int {
                        return Err(self.err(pos.line, pos.col, format!("{name} base/offset must be (int|string, int), found ({bt}, {ot})")));
                    }
                    if name == "store_u8" {
                        let vt = self.check_expr(&args[2].value, scopes)?;
                        if vt != Type::Int {
                            return Err(self.err(pos.line, pos.col, format!("store_u8 value must be int, found {vt}")));
                        }
                        return Ok(Type::Void);
                    }
                    return Ok(Type::Int);
                }
                if name == "store_i64" || name == "store_f64" {
                    if args.len() != 2 || args.iter().any(|a| a.name.is_some()) {
                        return Err(self.err(pos.line, pos.col, format!("{name} expects (address: int, value)")));
                    }
                    let at = self.check_expr(&args[0].value, scopes)?;
                    let vt = self.check_expr(&args[1].value, scopes)?;
                    let want = if name == "store_f64" { Type::Float } else { Type::Int };
                    if at != Type::Int || vt != want {
                        return Err(self.err(pos.line, pos.col, format!("{name} expects (int, {}), found ({at}, {vt})", want)));
                    }
                    return Ok(Type::Void);
                }
                if name == "target_os" {
                    // compile-time platform query: "windows" | "linux" | "macos" | "other"
                    if !args.is_empty() {
                        return Err(self.err(pos.line, pos.col, "target_os expects no arguments"));
                    }
                    return Ok(Type::Str);
                }
                if name == "as_string" {
                    if args.len() != 1 || args[0].name.is_some() {
                        return Err(self.err(pos.line, pos.col, "as_string expects exactly 1 positional argument (ptr: int)"));
                    }
                    let t = self.check_expr(&args[0].value, scopes)?;
                    if t != Type::Int {
                        return Err(self.err(pos.line, pos.col, format!("as_string expects int, found {t}")));
                    }
                    return Ok(Type::Str);
                }
                if name == "as_ptr" {
                    if args.len() != 1 || args[0].name.is_some() {
                        return Err(self.err(pos.line, pos.col, "as_ptr expects exactly 1 positional argument (s: string)"));
                    }
                    let t = self.check_expr(&args[0].value, scopes)?;
                    if t != Type::Str {
                        return Err(self.err(pos.line, pos.col, format!("as_ptr expects string, found {t}")));
                    }
                    return Ok(Type::Int);
                }
                // generic call: unify arguments, monomorphize
                if self.sigs.get(name).is_none() {
                    if self.generics.contains_key(name) {
                        return self.instantiate_call(name, args, *pos, expr, scopes);
                    }
                    if let Some(layout) = self.structs.get(name) {
                        // `structs` is a shared reference with the checker's
                        // lifetime — the layout borrow is independent of `self`
                        return self.check_struct_construction(name, layout, args, *pos, scopes);
                    }
                    // indirect call through a function pointer (v0.40.0)
                    if let Some(Type::FnPtr { ret, params }) = scopes.get(name).cloned() {
                        if args.iter().any(|a| a.name.is_some()) {
                            return Err(self.err(pos.line, pos.col, format!("function pointer '{name}' takes positional arguments only")));
                        }
                        if args.len() != params.len() {
                            return Err(self.err(pos.line, pos.col, format!(
                                    "function pointer '{name}' expects {} argument(s), found {}",
                                    params.len(),
                                    args.len()
                                )));
                        }
                        for (i, a) in args.iter().enumerate() {
                            let t = self.check_expr(&a.value, scopes)?;
                            if !assignable(&params[i], &t) {
                                return Err(self.err(a.value.pos().line, a.value.pos().col, format!(
                                        "argument {} of '{}' must be {}, found {}",
                                        i + 1,
                                        name,
                                        params[i],
                                        t
                                    )));
                            }
                        }
                        return Ok(*ret);
                    }
                    return Err(self.err(pos.line, pos.col, format!("call to undefined function or struct '{name}'")));
                }
                // borrow the signature piecewise: params/ret are read between
                // argument checks, never across a `&mut self` call
                let nparams = self.sigs[name].params.len();
                if args.iter().any(|a| a.name.is_some()) {
                    return Err(self.err(pos.line, pos.col, format!("function '{}' takes positional arguments only", name)));
                }
                if args.len() != nparams {
                    return Err(self.err(pos.line, pos.col, format!(
                            "function '{}' expects {} argument(s), found {}",
                            name,
                            nparams,
                            args.len()
                        )));
                }
                for (i, a) in args.iter().enumerate() {
                    let t = self.check_expr(&a.value, scopes)?;
                    if !assignable(&self.sigs[name].params[i], &t) {
                        return Err(self.err(a.value.pos().line, a.value.pos().col, format!(
                                "argument {} of '{}' must be {}, found {}",
                                i + 1,
                                name,
                                self.sigs[name].params[i],
                                t
                            )));
                    }
                }
                Ok(self.sigs[name].ret.clone())
            }
            Expr::Unary { op, expr, pos } => {
                let t = self.check_expr(expr, scopes)?;
                match (op, t) {
                    (UnOp::Not, Type::Bool) => Ok(Type::Bool),
                    (UnOp::Not, other) => Err(self.err(pos.line, pos.col, format!("'!' requires bool, found {other}"))),
                    (UnOp::Neg, Type::Int) => Ok(Type::Int),
                    (UnOp::Neg, Type::Float) => Ok(Type::Float),
                    (UnOp::Neg, other) => Err(self.err(pos.line, pos.col, format!("unary '-' requires int or float, found {other}"))),
                    (UnOp::BitNot, Type::Int) => Ok(Type::Int),
                    (UnOp::BitNot, other) => Err(self.err(pos.line, pos.col, format!("unary '~' requires int, found {other}"))),
                }
            }
            Expr::Binary { op, lhs, rhs, pos } => {
                let lt = self.check_expr(lhs, scopes)?;
                let rt = self.check_expr(rhs, scopes)?;
                // a nullable value has no operators: point at the narrowing
                // that unlocks it instead of "cannot compare X with Y"
                if matches!(lt, Type::Opt(_)) || matches!(rt, Type::Opt(_)) {
                    let none_test = matches!(op, Is | IsNot) && (lt == Type::None || rt == Type::None);
                    if !none_test {
                        let side = if matches!(lt, Type::Opt(_)) { "left" } else { "right" };
                        let t = if matches!(lt, Type::Opt(_)) { &lt } else { &rt };
                        return Err(self.err(pos.line, pos.col, format!(
                                "the {side} operand is {t}: a nullable value cannot be used here — test it with `is None` / `is not None` first"
                            )));
                    }
                }
                use BinOp::*;
                match op {
                    // `is` / `is not` exist for the nullable test (v0.40.0):
                    // one side must be the `None` literal. Anything else is a
                    // type error rather than a silent `==`, because identity on
                    // a value type has no meaning worth guessing at.
                    Is | IsNot => {
                        let nullable = matches!(lt, Type::None | Type::Opt(_)) || matches!(rt, Type::None | Type::Opt(_));
                        if !nullable {
                            return Err(self.err(pos.line, pos.col, format!(
                                    "'{}' compares against None; found ({lt}, {rt})",
                                    op_str(*op)
                                )));
                        }
                        Ok(Type::Bool)
                    }
                    And | Or => {
                        if lt == Type::Bool && rt == Type::Bool {
                            Ok(Type::Bool)
                        } else {
                            Err(self.err(pos.line, pos.col, format!("'{}' requires bool operands, found ({lt}, {rt})", op_str(*op))))
                        }
                    }
                    Eq | Ne => {
                        if lt.is_compound() || rt.is_compound() {
                            Err(self.err(pos.line, pos.col, format!("cannot compare compound type {lt} with {rt}")))
                        } else if lt == rt && lt != Type::Void {
                            Ok(Type::Bool)
                        } else {
                            Err(self.err(pos.line, pos.col, format!("cannot compare {lt} with {rt}")))
                        }
                    }
                    Lt | Le | Gt | Ge => {
                        if lt == Type::Str && rt == Type::Str {
                            Ok(Type::Bool)
                        } else {
                            let numeric = (lt == Type::Int || lt == Type::Float) && lt == rt;
                            if numeric {
                                Ok(Type::Bool)
                            } else {
                                Err(self.err(pos.line, pos.col, format!(
                                        "'{}' requires two int, two float, or two string operands, found ({lt}, {rt})",
                                        op_str(*op)
                                    )))
                            }
                        }
                    }
                    Add => {
                        if lt == Type::Str || rt == Type::Str {
                            if lt == Type::Str && rt == Type::Str {
                                Ok(Type::Str)
                            } else {
                                Err(self.err(pos.line, pos.col, format!("cannot concatenate string with {rt}")))
                            }
                        } else if (lt == Type::Int || lt == Type::Float) && lt == rt {
                            Ok(lt)
                        } else {
                            Err(self.err(pos.line, pos.col, format!(
                                    "'+' requires two int or two float operands, found ({lt}, {rt})"
                                )))
                        }
                    }
                    Sub | Mul | Div => {
                        if (lt == Type::Int || lt == Type::Float) && lt == rt {
                            Ok(lt)
                        } else {
                            Err(self.err(pos.line, pos.col, format!(
                                    "'{}' requires two int or two float operands, found ({lt}, {rt})",
                                    op_str(*op)
                                )))
                        }
                    }
                    Mod => {
                        if lt == Type::Int && rt == Type::Int {
                            Ok(Type::Int)
                        } else {
                            Err(self.err(pos.line, pos.col, format!("'%' requires two int operands, found ({lt}, {rt})")))
                        }
                    }
                    // Bitwise and shift operators are int-only, exactly like
                    // `%`: there is no implicit promotion to float, because
                    // Aoxn has no implicit int/float conversion at all.
                    Shl | Shr | BitAnd | BitOr | BitXor => {
                        if lt == Type::Int && rt == Type::Int {
                            Ok(Type::Int)
                        } else {
                            Err(self.err(pos.line, pos.col, format!(
                                "'{}' requires two int operands, found ({lt}, {rt})",
                                op_str(*op)
                            )))
                        }
                    }
                }
            }
        }
    }

    /// unify arguments against the generic declaration, create/reuse the
    /// monomorphized instance, and route codegen to it via call_map
    fn instantiate_call(
        &mut self,
        name: &str,
        args: &[Arg],
        pos: Pos,
        call_expr: &Expr,
        scopes: &mut Scopes,
    ) -> Result<Type, Diag> {
        if args.iter().any(|a| a.name.is_some()) {
            return Err(self.err(pos.line, pos.col, format!("function '{name}' takes positional arguments only")));
        }
        // arity check from a borrow — the signature pieces are only cloned
        // when a new instance actually needs to be created
        if args.len() != self.generics[name].params.len() {
            return Err(self.err(pos.line, pos.col, format!(
                    "function '{name}' expects {} argument(s), found {}",
                    self.generics[name].params.len(),
                    args.len()
                )));
        }
        // check arguments, then unify against declared param types
        let mut arg_types: Vec<Type> = Vec::with_capacity(args.len());
        for a in args {
            arg_types.push(self.check_expr(&a.value, scopes)?);
        }
        let mut subst_t: HashMap<String, Type> = HashMap::new();
        let mut n: Option<usize> = None;
        for (i, at) in arg_types.iter().enumerate() {
            let g = &self.generics[name];
            if !unify(&g.params[i].ty, at, &g.type_params, &mut subst_t, &mut n) {
                return Err(self.err(args[i].value.pos().line, args[i].value.pos().col, format!(
                        "argument {} of '{}' must be {}, found {}",
                        i + 1,
                        name,
                        g.params[i].ty,
                        at
                    )));
            }
        }
        let ret = subst_type(&self.generics[name].ret, &subst_t, n).map_err(|m| self.err(pos.line, pos.col, m))?;
        let key = mangle(name, &self.generics[name].type_params, &subst_t, n);
        self.call_map.insert(call_expr as *const Expr as usize, key.clone());
        if !self.done.contains(&key) {
            self.done.insert(key.clone());
            let mut inst = self.generics[name].clone();
            inst.name = key;
            inst.type_params = Vec::new();
            for p in &mut inst.params {
                p.ty = subst_type(&p.ty, &subst_t, n).map_err(|m| Diag::at("type", p.pos.file, p.pos.line, p.pos.col, m))?;
            }
            inst.ret = ret.clone();
            let lp = self.generics[name].len_param.clone();
            subst_block_types(&mut inst.body, &subst_t, n, lp.as_deref()).map_err(|m| self.err(pos.line, pos.col, m))?;
            // checked in pass 4 as the same object codegen will emit
            self.queue.push_back(inst);
        }
        Ok(ret)
    }

    #[allow(clippy::too_many_arguments)]
    fn check_struct_construction(
        &mut self,
        name: &str,
        layout: &StructInfo,
        args: &[Arg],
        pos: Pos,
        scopes: &mut Scopes,
    ) -> Result<Type, Diag> {
        // all arguments must be named
        for a in args {
            if a.name.is_none() {
                return Err(self.err(a.value.pos().line, a.value.pos().col, format!("struct '{name}' must be constructed with named fields: {name}(field=value, ...)")));
            }
        }
        for a in args {
            let fname = a.name.as_ref().unwrap();
            let fty = layout
                .field_type(fname)
                .ok_or_else(|| self.err(a.value.pos().line, a.value.pos().col, format!("struct '{name}' has no field '{fname}'")))?;
            let t = self.check_expr(&a.value, scopes)?;
            if !assignable(fty, &t) {
                return Err(self.err(a.value.pos().line, a.value.pos().col, format!("field '{fname}' of '{name}' must be {fty}, found {t}")));
            }
        }
        // duplicates (error points at the duplicate argument itself)
        let mut seen: HashMap<&str, ()> = HashMap::new();
        for a in args.iter() {
            let an = a.name.as_ref().unwrap();
            if seen.insert(an.as_str(), ()).is_some() {
                return Err(self.err(a.value.pos().line, a.value.pos().col, format!("field '{an}' given more than once in '{name}'")));
            }
        }
        // completeness
        if args.len() != layout.fields.len() {
            let provided: HashSet<&str> = args
                .iter()
                .map(|a| a.name.as_deref().unwrap_or_default())
                .collect();
            let missing: Vec<String> = layout
                .fields
                .iter()
                .filter(|(n, _)| !provided.contains(n.as_str()))
                .map(|(n, _)| n.clone())
                .collect();
            return Err(self.err(pos.line, pos.col, format!(
                    "struct literal '{name}' is missing field(s): {}",
                    missing.join(", ")
                )));
        }
        Ok(Type::Struct(name.to_string()))
    }
}

// ---- generic helpers ----

/// unify a declared (possibly generic) type against a concrete type;
/// T-params fill subst_t, the length param fills n. Returns false on mismatch.
fn unify(
    declared: &Type,
    actual: &Type,
    params: &[String],
    subst_t: &mut HashMap<String, Type>,
    n: &mut Option<usize>,
) -> bool {
    match declared {
        Type::Struct(d) if params.contains(d) => match subst_t.get(d) {
            Some(prev) => prev == actual,
            None => {
                subst_t.insert(d.clone(), actual.clone());
                true
            }
        },
        Type::Array { elem: de, len: GENERIC_LEN } => match actual {
            Type::Array { elem: ae, len: al } => {
                if let Some(prev) = *n {
                    if prev != *al {
                        return false;
                    }
                } else {
                    *n = Some(*al);
                }
                unify(de, ae, params, subst_t, n)
            }
            _ => false,
        },
        Type::Array { elem: de, len: dl } => match actual {
            Type::Array { elem: ae, len: al } if dl == al => unify(de, ae, params, subst_t, n),
            _ => false,
        },
        _ => declared == actual,
    }
}

/// substitute type params (T) and the length param (GENERIC_LEN sentinel)
fn subst_type(t: &Type, subst_t: &HashMap<String, Type>, n: Option<usize>) -> Result<Type, String> {
    match t {
        Type::Struct(d) => Ok(subst_t.get(d).cloned().unwrap_or_else(|| t.clone())),
        Type::Array { elem, len } => {
            let e = subst_type(elem, subst_t, n)?;
            let l = if *len == GENERIC_LEN {
                match n {
                    Some(v) => v,
                    None => return Err("cannot infer array length N (use it in a parameter)".into()),
                }
            } else {
                *len
            };
            Ok(Type::Array { elem: Box::new(e), len: l })
        }
        other => Ok(other.clone()),
    }
}

/// may a value of type `actual` be stored where `declared` is expected?
///
/// v0.40.0 added exactly one widening: a `T` (or `None`) goes into a
/// `T | None` slot. Nothing else is implicit — in particular an `int` never
/// becomes a `float`, and a `T | None` never becomes a bare `T` (narrow it
/// with `is None` first).
fn assignable(declared: &Type, actual: &Type) -> bool {
    if declared == actual {
        return true;
    }
    match declared {
        Type::Opt(inner) => matches!(actual, Type::None) || **inner == *actual,
        _ => false,
    }
}

/// `None` on one side, a nullable variable on the other: yields the name and
/// the type each branch sees. `None` for either side of the tuple means "no
/// narrowing here" is impossible — the caller only uses the Some case.
fn narrow_target(cond: &Expr, scopes: &Scopes) -> Option<(String, Type, Type)> {
    let Expr::Binary { op, lhs, rhs, .. } = cond else { return None };
    let is_not = match op {
        crate::ast::BinOp::Is => false,
        crate::ast::BinOp::IsNot => true,
        _ => return None,
    };
    let (var_expr, other) = (lhs.as_ref(), rhs.as_ref());
    let (var_expr, _other) = if matches!(other, Expr::NoneLit(_)) {
        (var_expr, other)
    } else if matches!(var_expr, Expr::NoneLit(_)) {
        (other, var_expr)
    } else {
        return None;
    };
    let Expr::Var { name, .. } = var_expr else { return None };
    let Type::Opt(inner) = scopes.get(name)?.clone() else { return None };
    let (then_ty, else_ty) = if is_not { ((*inner).clone(), Type::None) } else { (Type::None, (*inner).clone()) };
    Some((name.clone(), then_ty, else_ty))
}

fn has_generic_len(t: &Type) -> bool {
    match t {
        Type::Array { elem, len } => *len == GENERIC_LEN || has_generic_len(elem),
        _ => false,
    }
}

fn subst_block_types(block: &mut Block, subst_t: &HashMap<String, Type>, n: Option<usize>, len_param: Option<&str>) -> Result<(), String> {
    for stmt in &mut block.stmts {
        subst_stmt_types(stmt, subst_t, n, len_param)?;
    }
    Ok(())
}

fn subst_stmt_types(stmt: &mut Stmt, subst_t: &HashMap<String, Type>, n: Option<usize>, len_param: Option<&str>) -> Result<(), String> {
    match stmt {
        Stmt::Let { ty: Some(t), expr, .. } => {
            *t = subst_type(t, subst_t, n)?;
            subst_expr(expr, n, len_param);
            Ok(())
        }
        Stmt::Let { expr, .. } => {
            subst_expr(expr, n, len_param);
            Ok(())
        }
        Stmt::Assign { target, expr, .. } => {
            subst_expr(target, n, len_param);
            subst_expr(expr, n, len_param);
            Ok(())
        }
        Stmt::If { cond, then_block, else_block, .. } => {
            subst_expr(cond, n, len_param);
            subst_block_types(then_block, subst_t, n, len_param)?;
            if let Some(eb) = else_block {
                subst_block_types(eb, subst_t, n, len_param)?;
            }
            Ok(())
        }
        Stmt::While { cond, body, .. } => {
            subst_expr(cond, n, len_param);
            subst_block_types(body, subst_t, n, len_param)
        }
        Stmt::For { iter, body, .. } => {
            match iter {
                ForIter::Range(args) => {
                    for a in args {
                        subst_expr(a, n, len_param);
                    }
                }
                ForIter::Array(e) => subst_expr(e, n, len_param),
            }
            subst_block_types(body, subst_t, n, len_param)
        }
        Stmt::Return { expr: Some(e), .. } => {
            subst_expr(e, n, len_param);
            Ok(())
        }
        Stmt::ExprStmt { expr } => {
            subst_expr(expr, n, len_param);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// replace the length parameter with its concrete int constant in expressions
fn subst_expr(e: &mut Expr, n: Option<usize>, len_param: Option<&str>) {
    let (lp, v) = match (len_param, n) {
        (Some(lp), Some(v)) => (lp, v),
        _ => return,
    };
    match e {
        Expr::Var { name, .. } if name == lp => {
            *e = Expr::Int(v as i64, Pos { line: 0, col: 0, file: u32::MAX });
        }
        Expr::Unary { expr, .. } => subst_expr(expr, n, len_param),
        Expr::Binary { lhs, rhs, .. } => {
            subst_expr(lhs, n, len_param);
            subst_expr(rhs, n, len_param);
        }
        Expr::Index { arr, idx, .. } => {
            subst_expr(arr, n, len_param);
            subst_expr(idx, n, len_param);
        }
        Expr::Field { obj, .. } => subst_expr(obj, n, len_param),
        Expr::ArrayLit { elems, .. } => {
            for x in elems {
                subst_expr(x, n, len_param);
            }
        }
        Expr::ArrayRep { elem, .. } => subst_expr(elem, n, len_param),
        Expr::Call { args, .. } => {
            for a in args {
                subst_expr(&mut a.value, n, len_param);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, fe) in fields {
                subst_expr(fe, n, len_param);
            }
        }
        Expr::Cast { expr, .. } => subst_expr(expr, n, len_param),
        _ => {}
    }
}

/// deterministic mangled name for an instance: `name.<slug>.<len>`
fn mangle(name: &str, params: &[String], subst_t: &HashMap<String, Type>, n: Option<usize>) -> String {
    let mut key = String::from(name);
    for p in params {
        match subst_t.get(p) {
            Some(t) => {
                key.push('.');
                key.push_str(&type_slug(t));
            }
            None => {
                // param unused in the body — still part of the key
                key.push_str(".?");
            }
        }
    }
    if let Some(len) = n {
        key.push('.');
        key.push_str(&len.to_string());
    }
    key
}

fn type_slug(t: &Type) -> String {
    match t {
        Type::Int => "i".into(),
        Type::Float => "f".into(),
        Type::Bool => "b".into(),
        Type::Str => "s".into(),
        Type::Void => "v".into(),
        Type::None => "z".into(),
        Type::Opt(inner) => format!("n{}", type_slug(inner)),
        Type::Array { elem, len } => format!("a{}x{}", type_slug(elem), len),
        Type::Struct(n) => format!("s_{n}"),
        // a function pointer is part of a generic instance's identity: two
        // instances differing only in a callback's signature are different
        // functions (v0.40.0)
        Type::FnPtr { ret, params } => {
            let mut slug = format!("fn_{}", type_slug(ret));
            for p in params {
                slug.push('_');
                slug.push_str(&type_slug(p));
            }
            slug
        }
    }
}

// ---- struct table construction ----

fn collect_structs(decls: &[StructDecl]) -> Result<StructTable, Diag> {
    let mut table: StructTable = HashMap::default();

    // pass 1: reserve all names (allows forward references between structs)
    for s in decls {
        if table.contains_key(&s.name) {
            return Err(Diag::at("type", s.pos.file, s.pos.line, s.pos.col, format!("struct '{}' is defined more than once", s.name)));
        }
        table.insert(s.name.clone(), StructInfo::new(Vec::new()));
    }

    // pass 2: resolve field types
    for s in decls {
        let mut fields: Vec<(String, Type)> = Vec::new();
        for (i, f) in s.fields.iter().enumerate() {
            if s.fields[..i].iter().any(|q| q.name == f.name) {
                return Err(Diag::at("type", f.pos.file, f.pos.line, f.pos.col, format!("duplicate field '{}' in struct '{}'", f.name, s.name)));
            }
            resolve_ty(&f.ty, &table).map_err(|m| Diag::at("type", f.pos.file, f.pos.line, f.pos.col, m))?;
            if f.ty == Type::Void {
                return Err(Diag::at("type", f.pos.file, f.pos.line, f.pos.col, format!("field '{}' cannot have type void", f.name)));
            }
            fields.push((f.name.clone(), f.ty.clone()));
        }
        table.insert(s.name.clone(), StructInfo::new(fields));
    }

    // pass 3: reject recursive structs (they would have infinite size).
    // Proper DFS with gray/black marking: a struct appearing in multiple
    // sibling fields is fine; only a struct on the current path is a cycle.
    fn cycles(name: &str, table: &StructTable, gray: &mut HashSet<String>, black: &mut HashSet<String>) -> Option<String> {
        if black.contains(name) {
            return None;
        }
        if gray.contains(name) {
            return Some(name.to_string());
        }
        gray.insert(name.to_string());
        if let Some(info) = table.get(name) {
            for (_, t) in &info.fields {
                if let Type::Struct(inner) = t {
                    if let Some(c) = cycles(inner, table, gray, black) {
                        return Some(c);
                    }
                }
            }
        }
        gray.remove(name);
        black.insert(name.to_string());
        None
    }

    for s in decls {
        let mut gray: HashSet<String> = HashSet::new();
        let mut black: HashSet<String> = HashSet::new();
        if cycles(&s.name, &table, &mut gray, &mut black).is_some() {
            return Err(Diag::at("type", s.pos.file, s.pos.line, s.pos.col, format!(
                    "recursive struct '{}' (a struct cannot contain itself, directly or indirectly)",
                    s.name
                )));
        }
    }

    Ok(table)
}

/// validate a type annotation: struct names must exist, arrays recurse
fn resolve_ty(ty: &Type, structs: &StructTable) -> Result<(), String> {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::Str | Type::Void | Type::None => Ok(()),
        Type::Array { elem, .. } => resolve_ty(elem, structs),
        Type::Opt(inner) => {
            if **inner == Type::Void {
                Err("void cannot be nullable".into())
            } else {
                resolve_ty(inner, structs)
            }
        }
        Type::Struct(name) => {
            if structs.contains_key(name) {
                Ok(())
            } else {
                Err(format!("unknown type '{name}'"))
            }
        }
        // function-pointer types are never spelled in an annotation (the
        // surface grammar has no `fn` type yet), so they can only arrive here
        // synthesized — and there is nothing to resolve.
        Type::FnPtr { .. } => Ok(()),
    }
}

/// True if control flow cannot continue past this statement.
fn stmt_guarantees_return(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return { .. } => true,
        Stmt::If { then_block, else_block, .. } => {
            block_returns_all(then_block)
                && else_block.as_ref().map(|e| block_returns_all(e)).unwrap_or(false)
        }
        _ => false,
    }
}

fn block_returns_all(block: &Block) -> bool {
    block.stmts.iter().any(stmt_guarantees_return)
}

fn stmt_pos(s: &Stmt) -> Pos {
    match s {
        Stmt::Let { pos, .. }
        | Stmt::Assign { pos, .. }
        | Stmt::If { pos, .. }
        | Stmt::While { pos, .. }
        | Stmt::For { pos, .. }
        | Stmt::Break { pos }
        | Stmt::Continue { pos }
        | Stmt::Return { pos, .. } => *pos,
        Stmt::Pass => Pos { line: 0, col: 0, file: u32::MAX },
        Stmt::ExprStmt { expr } => expr.pos(),
    }
}

fn field_type<'t>(t: &'t Type, name: &str, structs: &'t StructTable) -> Option<&'t Type> {
    if let Type::Struct(sname) = t {
        if let Some(info) = structs.get(sname) {
            return info.field_type(name);
        }
    }
    None
}

fn op_str(op: BinOp) -> &'static str {
    use BinOp::*;
    match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Mod => "%",
        Shl => "<<",
        Shr => ">>",
        BitAnd => "&",
        BitOr => "|",
        BitXor => "^",
        Eq => "==",
        Ne => "!=",
        Is => "is",
        IsNot => "is not",
        Lt => "<",
        Le => "<=",
        Gt => ">",
        Ge => ">=",
        And => "&&",
        Or => "||",
    }
}






