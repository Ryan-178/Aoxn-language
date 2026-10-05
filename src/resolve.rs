//! Module resolution: imports become bindings, and bindings become names.
//!
//! Aoxn merges every file a program imports into ONE namespace. Until v0.43.0
//! that was the whole story, and a duplicate top-level name was a hard error
//! (`src/typecheck.rs`) — which is why every stdlib module hand-prefixes all
//! fifty-odd of its functions. This module is the layer that makes the prefix
//! optional: `import ui` then `ui.button(...)` reaches another module's
//! `button` without either module renaming anything.
//!
//! # How it works, and why codegen never sees it
//!
//! Resolution happens on the AST, before typecheck, and rewrites every
//! reference to a module-level name into the *flat* name the whole program
//! shares. Typecheck and `codegen_c.rs` therefore keep seeing exactly what
//! they saw before — one flat namespace — and a program that compiles today
//! emits byte-identical C.
//!
//! That "byte-identical" is a design requirement, not a coincidence:
//!
//! * A name exported by exactly one module keeps its spelling.
//! * Only a name exported by **two or more** modules gets a prefix
//!   (`ui.button` -> `ui_button`). Such a program did not compile before, so
//!   nothing that used to build changes shape.
//! * The self-hosted stage-2 compiler merges modules flat and has no
//!   namespace layer. The fixed point is only meaningful if both stages agree
//!   on every program stage-2 can still compile — which, by the rule above, is
//!   every program that compiled before this file existed. See
//!   `selfhost/load.ax` and `docs/spec.md`.
//!
//! # Ordering
//!
//! The flatten walks the module graph in post-order — an imported file's
//! declarations land before the importer's — exactly as the pre-v0.43.0
//! loader's recursion did, so declaration order, and therefore the generated
//! C, is unchanged for every existing program.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::Diag;

/// One file of the program, with its imports already pointed at the modules
/// they resolved to (path resolution stays in `lib.rs`, which owns the
/// canonical-path include-once and the cycle stack).
pub struct Module {
    /// the import spelling this module was first reached through, with `/`
    /// normalized to `.` — `stdlib.net.json`. Only used to build a mangling
    /// prefix and to resolve `a.b.c` chains; never the file's identity, which
    /// is its canonical path.
    pub key: String,
    pub structs: Vec<StructDecl>,
    pub funcs: Vec<FnDecl>,
    pub imports: Vec<Import>,
}

#[derive(Clone)]
pub struct Import {
    pub target: usize,
    pub kind: ImportKind,
    pub pos: Pos,
}

/// The flattened program, plus the map that undoes the renaming.
///
/// `typecheck` and `codegen` want the flat names; the IDE's outline wants the
/// names the SOURCE spells. Returning the map is cheaper than teaching the
/// symbol table about modules.
pub struct Flattened {
    pub structs: Vec<StructDecl>,
    pub funcs: Vec<FnDecl>,
    /// flat global name -> the name its module declares it under
    pub source_names: HashMap<String, String>,
}

impl Flattened {
    /// Restore the source spelling of a name.
    pub fn source_name(&self, flat: &str) -> String {
        self.source_names.get(flat).cloned().unwrap_or_else(|| flat.to_string())
    }

    /// Restore the source spelling everywhere it appears in a type.
    fn source_type(&self, t: &Type) -> Type {
        match t {
            Type::Struct(n) => Type::Struct(self.source_name(n)),
            Type::Opt(i) => Type::Opt(Box::new(self.source_type(i))),
            Type::Dict(i) => Type::Dict(Box::new(self.source_type(i))),
            Type::Array { elem, len } => Type::Array { elem: Box::new(self.source_type(elem)), len: *len },
            Type::FnPtr { ret, params } => Type::FnPtr {
                ret: Box::new(self.source_type(ret)),
                params: params.iter().map(|p| self.source_type(p)).collect(),
            },
            other => other.clone(),
        }
    }

    /// A copy whose declarations are named the way their modules spell them.
    /// Bodies are dropped: the outline reads signatures, and keeping them
    /// would mean rewriting expressions too for no gain.
    pub fn with_source_names(&self) -> (Vec<StructDecl>, Vec<FnDecl>) {
        let structs: Vec<StructDecl> = self
            .structs
            .iter()
            .map(|s| StructDecl {
                name: self.source_name(&s.name),
                fields: s
                    .fields
                    .iter()
                    .map(|f| Param { name: f.name.clone(), ty: self.source_type(&f.ty), pos: f.pos })
                    .collect(),
                pos: s.pos,
            })
            .collect();
        let funcs: Vec<FnDecl> = self
            .funcs
            .iter()
            .map(|f| FnDecl {
                name: self.source_name(&f.name),
                type_params: f.type_params.clone(),
                len_param: f.len_param.clone(),
                params: f
                    .params
                    .iter()
                    .map(|p| Param { name: p.name.clone(), ty: self.source_type(&p.ty), pos: p.pos })
                    .collect(),
                ret: self.source_type(&f.ret),
                body: Block { stmts: Vec::new() },
                is_extern: f.is_extern,
                pos: f.pos,
            })
            .collect();
        (structs, funcs)
    }
}

/// A module-level name of one module: the flat global it compiles to, and
/// whether it is a struct (a qualified *construction* `m.Point(...)` lowers
/// to a struct literal, not a call).
#[derive(Clone)]
pub struct Member {
    pub flat: String,
    pub is_struct: bool,
}

/// What one module may spell.
struct Bindings {
    /// visible unqualified name -> flat global
    names: HashMap<String, String>,
    /// namespace prefix -> the dotted module key it stands for
    aliases: HashMap<String, String>,
    /// every dotted key that names a module -> its index
    modules: HashMap<String, usize>,
}

impl Bindings {
    /// Resolve `a.b.c` to `(module index, member name)`.
    ///
    /// An alias stands for a whole module, so `import a.b.c` — which binds
    /// the name `a`, as in Python — accepts `a.f()` and `a.b.c.f()` alike.
    /// Without an alias, every segment but the last must spell a module that
    /// was itself imported.
    fn dotted(&self, name: &str) -> Option<(usize, String)> {
        let segs: Vec<&str> = name.split('.').collect();
        if segs.len() < 2 {
            return None;
        }
        let member = segs[segs.len() - 1].to_string();
        if let Some(key) = self.aliases.get(segs[0]) {
            let mut cand = key.clone();
            for s in &segs[1..segs.len() - 1] {
                cand.push('.');
                cand.push_str(s);
            }
            return self.modules.get(&cand).map(|m| (*m, member));
        }
        let key = segs[..segs.len() - 1].join(".");
        self.modules.get(&key).map(|m| (*m, member))
    }
}

/// `stdlib.net.json` -> `stdlib_net_json`. A leading `./` is dropped first, so
/// a relative import does not yield a prefix that starts with `_`.
fn prefix_of(key: &str) -> String {
    let k = key.strip_prefix("./").unwrap_or(key);
    let mut out = String::with_capacity(k.len());
    for c in k.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

/// Every top-level name a module exports, functions before structs (the order
/// `m.f()` and `m.Foo` are looked up in; neither depends on it).
fn exports(m: &Module) -> Vec<(String, bool)> {
    let mut v = Vec::with_capacity(m.funcs.len() + m.structs.len());
    for f in &m.funcs {
        v.push((f.name.clone(), false));
    }
    for s in &m.structs {
        v.push((s.name.clone(), true));
    }
    v
}

/// Resolve every module and flatten the program.
///
/// `order` is the post-order module traversal (dependencies first) that the
/// loader recorded while it recursed; the import graph is a DAG (cycles are
/// rejected during loading), so walking it in that order means a module's
/// imports are always resolved before the module itself.
///
/// `entries` are the modules the compiler was pointed at. Their own top-level
/// names are program-visible by definition — the entry file is not something
/// anybody imports — which is what keeps every pre-v0.43.0 program's `main`
/// spelled exactly as it was.
pub fn resolve(mut mods: Vec<Module>, order: &[usize], entries: &[usize]) -> Result<Flattened, Vec<Diag>> {
    let mut errs: Vec<Diag> = Vec::new();
    let n = mods.len();

    // --- 1. what each module declares ------------------------------------
    let mut providers: HashMap<String, Vec<usize>> = HashMap::new();
    let mut structness: HashSet<(usize, String)> = HashSet::new();
    for (i, m) in mods.iter().enumerate() {
        for (name, is_struct) in exports(m) {
            if is_struct {
                structness.insert((i, name.clone()));
            }
            let e = providers.entry(name.clone()).or_default();
            if !e.contains(&i) {
                e.push(i);
            }
        }
    }

    // --- 2. bindings, in dependency order --------------------------------
    // `names` maps a visible spelling to (the module that declares it, the
    // name it has THERE). Keeping the pair rather than a finished flat name is
    // what lets the flat names be decided globally, in pass 3.
    let mut names: Vec<HashMap<String, (usize, String)>> = vec![HashMap::new(); n];
    let mut exports_of: Vec<HashMap<String, (usize, String)>> = vec![HashMap::new(); n];
    let mut aliases: Vec<HashMap<String, String>> = vec![HashMap::new(); n];
    // every dotted key that names a module, and every proper prefix of it
    let mut module_index: HashMap<String, usize> = HashMap::new();
    for (j, m) in mods.iter().enumerate() {
        let segs: Vec<&str> = m.key.split('.').collect();
        for k in 1..=segs.len() {
            module_index.entry(segs[..k].join(".")).or_insert(j);
        }
    }
    // a module can only qualify through a module it imported, so start from
    // the subset it did and add to it below
    let mut reachable: Vec<HashMap<String, usize>> = vec![HashMap::new(); n];

    for &i in order {
        for (name, _) in exports(&mods[i]) {
            names[i].insert(name.clone(), (i, name.clone()));
            exports_of[i].insert(name.clone(), (i, name));
        }
        let imports: Vec<(usize, ImportKind, Pos)> =
            mods[i].imports.iter().map(|im| (im.target, im.kind.clone(), im.pos)).collect();
        for (target, kind, ipos) in imports {
            let imp = Import { target, kind: kind.clone(), pos: ipos };
            let target_key = mods[imp.target].key.clone();
            match &imp.kind {
                ImportKind::Star => {
                    // A star import binds the target's EXPORT set — its own
                    // declarations plus everything it star-imported itself.
                    // That re-export is what keeps a diamond working: `d`'s
                    // names reach the program through both `a` and `b`. The
                    // ORIGIN travels with the name, so a stdlib function that
                    // arrives through two paths is recognised as one function
                    // rather than as two modules disagreeing.
                    for (name, src) in exports_of[imp.target].clone() {
                        add_binding(&mut names[i], name.clone(), src.clone(), imp.pos, &mut errs);
                        exports_of[i].insert(name, src);
                    }
                }
                ImportKind::Names(items) => {
                    for it in items {
                        if providers.get(&it.name).map(|v| v.contains(&imp.target)).unwrap_or(false) {
                            add_binding(
                                &mut names[i],
                                it.local().to_string(),
                                (imp.target, it.name.clone()),
                                imp.pos,
                                &mut errs,
                            );
                        } else {
                            errs.push(Diag::at(
                                "type",
                                imp.pos.file,
                                imp.pos.line,
                                imp.pos.col,
                                format!("module '{target_key}' has no top-level name '{}'", it.name),
                            ));
                        }
                    }
                }
                ImportKind::Module { alias } => {
                    // Python binds the FIRST segment of a dotted path; that is
                    // also what makes `a.b.c.f()` work after `import a.b.c`.
                    let first = target_key.split('.').next().unwrap_or(&target_key).to_string();
                    let bound = alias.clone().unwrap_or(first);
                    match aliases[i].get(&bound) {
                        Some(prev) if prev != &target_key => errs.push(Diag::at(
                            "type",
                            imp.pos.file,
                            imp.pos.line,
                            imp.pos.col,
                            format!("module name '{bound}' is bound twice (to '{prev}' and to '{target_key}')"),
                        )),
                        Some(_) => {}
                        None => {
                            aliases[i].insert(bound, target_key.clone());
                        }
                    }
                }
            }
        }
        let imported: Vec<usize> = mods[i].imports.iter().map(|im| im.target).collect();
        for j in 0..n {
            if j == i || !imported.contains(&j) {
                continue;
            }
            let segs: Vec<&str> = mods[j].key.split('.').collect();
            for k in 1..=segs.len() {
                reachable[i].entry(segs[..k].join(".")).or_insert(j);
            }
        }
        // a module's own key is resolvable inside it (a self-import is
        // include-once, but the dotted chain still has to land somewhere)
        let segs: Vec<&str> = mods[i].key.split('.').collect();
        for k in 1..=segs.len() {
            reachable[i].entry(segs[..k].join(".")).or_insert(i);
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }

    // --- 3. flat names ----------------------------------------------------
    // A name keeps its bare spelling exactly when the program can spell it
    // unqualified somewhere AND only one module declares it. Everything else
    // is prefixed. Every pre-v0.43.0 program merges whole modules, so every
    // one of its names is spellable unqualified — which is why this rule
    // leaves the generated C of every existing program untouched, and why the
    // self-hosted fixed point still holds.
    let mut visible: HashSet<String> = HashSet::new();
    for &i in order {
        for imp in &mods[i].imports {
            match &imp.kind {
                // a star import makes the target's whole export set spellable
                ImportKind::Star => visible.extend(exports_of[imp.target].keys().cloned()),
                // a named import makes ONLY what it lists spellable — that is
                // the whole point of listing it
                ImportKind::Names(items) => visible.extend(items.iter().map(|it| it.local().to_string())),
                // binding a module as a namespace never makes a bare name
                // spellable; `m.f` reaches it, `f` does not
                ImportKind::Module { .. } => {}
            }
        }
    }
    for &e in entries {
        visible.extend(exports_of[e].keys().cloned());
    }

    let mut prefixes: Vec<String> = mods.iter().map(|m| prefix_of(&m.key)).collect();
    // Two modules can sanitize to the same prefix (`a/b.ax` and `a.b.ax`),
    // and a prefix can collide with a real top-level name. Both are settled
    // before any name is handed out, so the outcome cannot depend on the
    // order the fixups happen to run in.
    let taken: HashSet<String> = providers.keys().cloned().collect();
    let mut used_prefix: HashSet<String> = HashSet::new();
    for p in prefixes.iter_mut() {
        let base = p.clone();
        let mut cand = base.clone();
        let mut n = 1;
        while taken.contains(&cand) || !used_prefix.insert(cand.clone()) {
            n += 1;
            cand = format!("{base}_{n}");
        }
        *p = cand;
    }

    let mut members: Vec<HashMap<String, Member>> = vec![HashMap::new(); n];
    for (name, owners) in &providers {
        for &mi in owners {
            let bare = visible.contains(name) && owners.len() == 1;
            let flat = if bare { name.clone() } else { format!("{}_{name}", prefixes[mi]) };
            members[mi].insert(name.clone(), Member { flat, is_struct: structness.contains(&(mi, name.clone())) });
        }
    }
    // the flat name a (module, name) pair compiles to
    let flat_of = |mi: usize, name: &str| -> Option<String> { members[mi].get(name).map(|m| m.flat.clone()) };

    let binds: Vec<Bindings> = (0..n)
        .map(|i| Bindings {
            names: names[i]
                .iter()
                .filter_map(|(local, (src, orig))| flat_of(*src, orig).map(|f| (local.clone(), f)))
                .collect(),
            aliases: aliases[i].clone(),
            modules: reachable[i].clone(),

        })
        .collect();

    // --- 4. rewrite references --------------------------------------------
    for (i, m) in mods.iter_mut().enumerate() {
        let members_ref: Vec<HashMap<String, Member>> = members.iter().map(|t| t.clone()).collect();
        for f in &mut m.funcs {
            rewrite_fn(f, &binds[i], &members_ref);
        }
        for s in &mut m.structs {
            rewrite_struct(s, &binds[i], &members_ref);
        }
    }

    // --- 4. flatten ------------------------------------------------------
    // Each module is emitted exactly once, in the order the loader reached it
    // depth-first: an import's declarations land before the importer's.
    let mut slots: Vec<Option<Module>> = mods.into_iter().map(Some).collect();
    let mut structs = Vec::new();
    let mut funcs = Vec::new();
    for &i in order {
        if let Some(m) = slots[i].take() {
            structs.extend(m.structs);
            funcs.extend(m.funcs);
        }
    }
    // Every flat name mapped back to what its module calls it. Identity
    // entries are included, so a caller can map unconditionally.
    let mut source_names: HashMap<String, String> = HashMap::new();
    for table in &members {
        for (orig, mem) in table {
            source_names.insert(mem.flat.clone(), orig.clone());
        }
    }
    Ok(Flattened { structs, funcs, source_names })
}

fn add_binding(map: &mut HashMap<String, (usize, String)>, local: String, src: (usize, String), pos: Pos, errs: &mut Vec<Diag>) {
    match map.get(&local) {
        Some(prev) if *prev != src => errs.push(Diag::at(
            "type",
            pos.file,
            pos.line,
            pos.col,
            format!(
                "name '{local}' comes from two modules; import the modules (`import ...`) and qualify the use, \
                 or rename one side with `as`"
            ),
        )),
        Some(_) => {}
        None => {
            map.insert(local, src);
        }
    }
}

// ---- the AST walk -------------------------------------------------------

struct Ctx<'a> {
    b: &'a Bindings,
    /// per-module export tables, shared by every module's context: a
    /// qualified reference resolves its member in the TARGET module.
    members: &'a [HashMap<String, Member>],
    locals: HashSet<String>,
}

/// Names bound inside a function: parameters plus every `let` / `for` /
/// `except` binding in the body.
///
/// Collecting them up front rather than as the walk goes matches Python and
/// this language's own "first use declares the variable" rule: a name that is
/// local anywhere in a function is local throughout it, so a parameter that
/// shadows an import can never leak a reference through.
fn collect_locals(blk: &Block, out: &mut HashSet<String>) {
    for s in &blk.stmts {
        match s {
            Stmt::Let { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::If { then_block, else_block, .. } => {
                collect_locals(then_block, out);
                if let Some(b) = else_block {
                    collect_locals(b, out);
                }
            }
            Stmt::While { body, .. } => collect_locals(body, out),
            Stmt::For { var, body, .. } => {
                out.insert(var.clone());
                collect_locals(body, out);
            }
            Stmt::Try { body, err_name, handler, .. } => {
                collect_locals(body, out);
                if let Some(n) = err_name {
                    out.insert(n.clone());
                }
                collect_locals(handler, out);
            }
            _ => {}
        }
    }
}

fn rewrite_fn(f: &mut FnDecl, b: &Bindings, members: &[HashMap<String, Member>]) {
    let mut locals = HashSet::new();
    for p in &f.params {
        locals.insert(p.name.clone());
    }
    collect_locals(&f.body, &mut locals);
    let ctx = Ctx { b, members, locals };
    // the DECLARATION takes the flat name too — that is the whole point: two
    // modules may both define `helper`, and only the mangled pair survives
    f.name = ctx.plain(&f.name);
    for p in &mut f.params {
        p.ty = ctx.ty(&p.ty);
    }
    f.ret = ctx.ty(&f.ret);
    ctx.block(&mut f.body);
}

fn rewrite_struct(s: &mut StructDecl, b: &Bindings, members: &[HashMap<String, Member>]) {
    let ctx = Ctx { b, members, locals: HashSet::new() };
    s.name = ctx.plain(&s.name);
    for f in &mut s.fields {
        f.ty = ctx.ty(&f.ty);
    }
}

impl<'a> Ctx<'a> {
    /// An unqualified name: a binding if there is one, otherwise left alone so
    /// typecheck reports it exactly as it always has.
    fn plain(&self, name: &str) -> String {
        self.b.names.get(name).cloned().unwrap_or_else(|| name.to_string())
    }

    /// `m.f` in value position. `None` when the leading name is not a module,
    /// which is how a struct field chain and a module path stay apart without
    /// the parser needing to know what a module is.
    fn qualified(&self, name: &str) -> Option<&Member> {
        let (mi, member) = self.b.dotted(name)?;
        self.members.get(mi)?.get(&member)
    }

    /// The flat global an unqualified value position names, or `None` when the
    /// name is local or unknown.
    fn value(&self, name: &str) -> Option<String> {
        if name.contains('.') || self.locals.contains(name) {
            return None;
        }
        self.b.names.get(name).cloned()
    }

    fn ty(&self, t: &Type) -> Type {
        match t {
            Type::Struct(name) => Type::Struct(self.name_in(name)),
            Type::Opt(i) => Type::Opt(Box::new(self.ty(i))),
            Type::Dict(i) => Type::Dict(Box::new(self.ty(i))),
            Type::Array { elem, len } => Type::Array { elem: Box::new(self.ty(elem)), len: *len },
            Type::FnPtr { ret, params } => Type::FnPtr {
                ret: Box::new(self.ty(ret)),
                params: params.iter().map(|p| self.ty(p)).collect(),
            },
            other => other.clone(),
        }
    }

    /// A name in a type or construction position, where a local can never
    /// appear.
    fn name_in(&self, name: &str) -> String {
        if name.contains('.') {
            if let Some(mem) = self.qualified(name) {
                return mem.flat.clone();
            }
        }
        self.plain(name)
    }

    fn block(&self, blk: &mut Block) {
        for s in &mut blk.stmts {
            self.stmt(s);
        }
    }

    fn stmt(&self, s: &mut Stmt) {
        match s {
            Stmt::Let { ty, expr, .. } => {
                if let Some(t) = ty {
                    *t = self.ty(t);
                }
                self.expr(expr);
            }
            Stmt::Assign { target, expr, .. } => {
                self.expr(target);
                self.expr(expr);
            }
            Stmt::If { cond, then_block, else_block, .. } => {
                self.expr(cond);
                self.block(then_block);
                if let Some(b) = else_block {
                    self.block(b);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond);
                self.block(body);
            }
            Stmt::For { iter, body, .. } => {
                match iter {
                    ForIter::Range(args) => {
                        for a in args {
                            self.expr(a);
                        }
                    }
                    ForIter::Array(e) | ForIter::Dict(e) => self.expr(e),
                }
                self.block(body);
            }
            Stmt::Return { expr, .. } => {
                if let Some(e) = expr {
                    self.expr(e);
                }
            }
            Stmt::Raise { expr, .. } => self.expr(expr),
            Stmt::Try { body, handler, .. } => {
                self.block(body);
                self.block(handler);
            }
            Stmt::ExprStmt { expr } => self.expr(expr),
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Pass => {}
        }
    }

    fn expr(&self, e: &mut Expr) {
        match e {
            Expr::Var { name, .. } => {
                if let Some(f) = self.value(name) {
                    *name = f;
                }
            }
            Expr::Call { name, args, pos, lit_id } => {
                if let Some(flat) = self.value(name) {
                    *name = flat;
                } else if let Some(mem) = self.qualified(name) {
                    if mem.is_struct {
                        // `m.Point(x=1, y=2)` is a construction that only looks
                        // like a call. Every field is named in this language,
                        // so the mapping back is total.
                        let fields = args.iter().filter_map(|a| a.name.clone().map(|n| (n, a.value.clone()))).collect();
                        *e = Expr::StructLit { name: mem.flat.clone(), fields, lit_id: *lit_id, pos: *pos };
                        return;
                    }
                    *name = mem.flat.clone();
                }
                for a in args.iter_mut() {
                    self.expr(&mut a.value);
                }
            }
            Expr::Unary { expr, .. } => self.expr(expr),
            Expr::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            Expr::Index { arr, idx, .. } => {
                self.expr(arr);
                self.expr(idx);
            }
            Expr::Field { obj, name, pos } => {
                // `m.f` read as a value is a module member; a struct field
                // chain is rooted at a local, so it never reaches this arm.
                let member = match obj.as_ref() {
                    Expr::Var { name: base, .. } if !self.locals.contains(base.as_str()) => {
                        self.qualified(&format!("{base}.{name}")).map(|m| m.flat.clone())
                    }
                    _ => None,
                };
                match member {
                    Some(flat) => *e = Expr::Var { name: flat, pos: *pos },
                    None => self.expr(obj),
                }
            }
            Expr::ArrayLit { elems, .. } => {
                for el in elems.iter_mut() {
                    self.expr(el);
                }
            }
            Expr::ArrayRep { elem, .. } => self.expr(elem),
            Expr::DictLit { entries, .. } => {
                for (k, v) in entries.iter_mut() {
                    self.expr(k);
                    self.expr(v);
                }
            }
            Expr::StructLit { name, fields, .. } => {
                *name = self.name_in(name);
                for (_, v) in fields.iter_mut() {
                    self.expr(v);
                }
            }
            Expr::Cast { expr, to, .. } => {
                *to = self.ty(to);
                self.expr(expr);
            }
            Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) | Expr::NoneLit(..) => {}
        }
    }
}
