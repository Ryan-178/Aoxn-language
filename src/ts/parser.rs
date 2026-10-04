//! TS-M1 parser (S1): TypeScript declarations/statements/expressions lowered
//! into the EXISTING `crate::ast` nodes, so the current typecheck + codegen
//! pipeline runs unchanged (docs/ts-m1-spec.md §1-§2).
//!
//! S1 scope: `function` / `interface` declarations; `const`/`let`, if/else,
//! while, do-while, C-style `for`, for-of, break/continue, return, blocks;
//! expressions: literals, identifiers, calls, member/index access, unary,
//! binary, assignment statements. `console.log(x)` lowers to `print(x)`,
//! `x.length` to `len(x)`.
//!
//! S1 numeric profile: `number` annotations lower to `int` (i64); float
//! literals work only in unannotated positions (inferred `float`). JS-wide
//! f64 semantics land with the S2 numeric layer. Everything outside the
//! slice (modules, classes, generics, ternary, closures, null) is rejected
//! with a diagnostic pointing at the slice that covers it.

use crate::ast::*;
use crate::ts::lexer::{self, Tok, TplPart, Token};
use crate::Diag;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
enum LoopKind {
    WhileLike,
    CFor,
}

pub struct Parser {
    toks: Vec<Token>,
    i: usize,
    file: u32,
    lit_id: usize,
    loops: Vec<LoopKind>,
    /// inside a function signature: array types are allowed here (they map
    /// onto the pipeline's single generic length parameter)
    fn_sig: bool,
    /// an array type appeared in the current signature
    saw_array_len: bool,
    /// variable/parameter types seen so far (console.log formatting and
    /// literal shaping need them at parse time)
    vars: HashMap<String, Type>,
    /// function return types, for expression-type inference
    fn_rets: HashMap<String, Type>,
    /// function parameter types (generic return inference matches them)
    fn_params: HashMap<String, Vec<Type>>,
    /// interface field types (member-access inference)
    struct_fields: HashMap<String, HashMap<String, Type>>,
    /// a lowered construct referenced the TS runtime helpers
    need_runtime: bool,
    /// synthesized tuple structs to merge into the program
    extra_structs: Vec<StructDecl>,
    /// `import styles from "./page.module.css"` -> `page_module`, the
    /// zero-argument accessor returning that module's class struct. A CSS
    /// module has no namespace object (Aoxn has no function pointers), so the
    /// binding is rewritten at the use site into a call: `styles.title`
    /// becomes `page_module().title`, a typed field read (v0.36.0).
    css_modules: HashMap<String, String>,
    /// optional/default parameter fill-ins, keyed by function name
    fn_opts: HashMap<String, Vec<(usize, Expr)>>,
}

/// Parse a TS source unit into the shared AST (imports are always empty:
/// TS module resolution lands in W1-S3).
pub fn parse(file: u32, src: &str) -> Result<Program, Diag> {
    let toks = lexer::lex(file, src)?;
    let mut p = Parser {
        toks,
        i: 0,
        file,
        lit_id: 0,
        loops: Vec::new(),
        fn_sig: false,
        saw_array_len: false,
        vars: HashMap::new(),
        fn_rets: HashMap::new(),
        fn_params: HashMap::new(),
        struct_fields: HashMap::new(),
        need_runtime: false,
        extra_structs: Vec::new(),
        css_modules: HashMap::new(),
        fn_opts: HashMap::new(),
    };
    let mut imports = Vec::new();
    let mut structs = Vec::new();
    let mut funcs = Vec::new();
    loop {
        let t = p.peek().clone();
        match &t.tok {
            Tok::Eof => break,
            Tok::Kw("function") => funcs.push(p.fn_decl()?),
            Tok::Kw("interface") => structs.push(p.interface_decl()?),
            Tok::Kw("import") => {
                imports.push(p.import_decl_ts()?);
            }
            Tok::Kw("export") => {
                p.i += 1;
                if p.at_kw("default") {
                    return Err(p.err_here("default exports land with the package slice (W1-S3 follow-up)"));
                }
                if p.at_punct("{") {
                    return Err(p.err_here("re-exports (`export { x }`) land with the package slice"));
                }
                match &p.peek().tok {
                    Tok::Kw("function") => funcs.push(p.fn_decl()?),
                    Tok::Kw("interface") => structs.push(p.interface_decl()?),
                    Tok::Kw("const") | Tok::Kw("let") => {
                        return Err(p.err(t, "top-level statements are not supported; wrap code in a function"))
                    }
                    _ => return Err(p.err_here("expected a declaration after `export`")),
                }
            }
            Tok::Kw("class") => return Err(p.err(t, "class declarations land in a later TS-M1 slice")),
            Tok::Kw("const") | Tok::Kw("let") => {
                return Err(p.err(t, "top-level statements are not supported; wrap code in a function"))
            }
            Tok::Kw("var") => return Err(p.err(t, "var is not supported; use const/let (block scope)")),
            Tok::Kw("declare") => {
                p.i += 1;
                p.skip_ambient()?;
            }
            _ => return Err(p.err(t, "expected a declaration (function / interface)")),
        }
    }
    structs.extend(p.extra_structs);
    if p.need_runtime {
        funcs.extend(super::runtime::runtime_funcs()?);
    }
    for f in funcs.iter_mut() {
        fill_block(&mut f.body, &p.fn_opts, &p.fn_params);
    }
    Ok(Program { imports, structs, funcs, assets: crate::assets::AssetSet::default() })
}

impl Parser {
    fn err(&self, t: Token, msg: impl Into<String>) -> Diag {
        Diag::at("parse", self.file, t.line, t.col, msg)
    }

    fn err_here(&self, msg: impl Into<String>) -> Diag {
        let t = self.peek().clone();
        self.err(t, msg)
    }

    fn peek(&self) -> &Token {
        &self.toks[self.i.min(self.toks.len() - 1)]
    }

    fn peek_at(&self, n: usize) -> &Token {
        &self.toks[(self.i + n).min(self.toks.len() - 1)]
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.i.min(self.toks.len() - 1)].clone();
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
        t
    }

    fn at_punct(&self, p: &str) -> bool {
        matches!(&self.peek().tok, Tok::Punct(q) if *q == p)
    }

    fn at_kw(&self, k: &str) -> bool {
        matches!(&self.peek().tok, Tok::Kw(q) if *q == k)
    }

    fn eat_punct(&mut self, p: &str) -> bool {
        if self.at_punct(p) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, k: &str) -> bool {
        if self.at_kw(k) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, p: &str) -> Result<Token, Diag> {
        if self.at_punct(p) {
            Ok(self.bump())
        } else {
            Err(self.err_here(format!("expected '{p}'")))
        }
    }

    fn expect_kw(&mut self, k: &str) -> Result<Token, Diag> {
        if self.at_kw(k) {
            Ok(self.bump())
        } else {
            Err(self.err_here(format!("expected '{k}'")))
        }
    }

    fn expect_ident(&mut self) -> Result<(String, Token), Diag> {
        let t = self.peek().clone();
        match &t.tok {
            Tok::Ident(name) => {
                self.i += 1;
                Ok((name.clone(), t))
            }
            _ => Err(self.err(t, "expected an identifier")),
        }
    }

    /// end of a simple statement: `;`, or ASI (line break / `}` / EOF)
    fn end_stmt(&mut self) -> Result<(), Diag> {
        if self.eat_punct(";") {
            return Ok(());
        }
        let t = self.peek();
        if t.tok == Tok::Eof || t.nl_before || self.at_punct("}") {
            return Ok(());
        }
        Err(self.err_here("expected ';' or a line break"))
    }

    /// `declare ...` ambient forms are parsed loosely and dropped
    fn skip_ambient(&mut self) -> Result<(), Diag> {
        let mut depth = 0usize;
        loop {
            let t = self.peek().clone();
            match &t.tok {
                Tok::Eof => return Ok(()),
                Tok::Punct(p) if *p == "{" || *p == "(" || *p == "[" => depth += 1,
                Tok::Punct(p) if *p == "}" || *p == ")" || *p == "]" => {
                    if depth == 0 {
                        return Ok(());
                    }
                    depth -= 1;
                }
                Tok::Punct(";") if depth == 0 => {
                    self.i += 1;
                    return Ok(());
                }
                _ => {}
            }
            self.i += 1;
        }
    }

    // ---- declarations ----

    fn fn_decl(&mut self) -> Result<FnDecl, Diag> {
        let start = self.expect_kw("function")?;
        let (name, _) = self.expect_ident()?;
        let mut type_params: Vec<String> = Vec::new();
        if self.eat_punct("<") {
            while !self.at_punct(">") {
                let (tp, _) = self.expect_ident()?;
                type_params.push(tp);
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct(">")?;
        }
        self.fn_sig = true;
        self.saw_array_len = false;
        self.expect_punct("(")?;
        let mut params = Vec::new();
        let mut opts: Vec<(usize, Expr)> = Vec::new();
        while !self.at_punct(")") {
            let (pname, pt) = self.expect_ident()?;
            let optional = self.eat_punct("?");
            self.expect_punct(":")?;
            let ty = self.map_type()?;
            // `x?: T` fills with the type's null sentinel at omitted call
            // sites; `x: T = e` fills with `e` (literal-shape checked)
            if self.eat_punct("=") {
                let d = self.expr()?;
                let d = self.shape_num(d, &ty);
                opts.push((params.len(), d));
            } else if optional {
                let d = self.sentinel(&ty, self.pos_of(&pt));
                opts.push((params.len(), d));
            }
            params.push(Param { name: pname, ty, pos: self.pos_of(&pt) });
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_punct(")")?;
        let ret = if self.eat_punct(":") {
            self.map_type()?
        } else {
            Type::Void
        };
        self.fn_sig = false;
        self.fn_rets.insert(name.clone(), ret.clone());
        self.fn_params.insert(name.clone(), params.iter().map(|p| p.ty.clone()).collect());
        if !opts.is_empty() {
            self.fn_opts.insert(name.clone(), opts);
        }
        for p in &params {
            self.vars.insert(p.name.clone(), p.ty.clone());
        }
        // the pipeline's generic mechanism carries ONE array length
        // parameter per function; synthesize it for `T[]` signatures
        let array_params = params.iter().filter(|p| matches!(p.ty, Type::Array { .. })).count();
        let len_param = if self.saw_array_len {
            if array_params > 1 {
                return Err(self.err_here(
                    "multiple array parameters share one length parameter in TS-M1; independent lengths land with the S2b slice",
                ));
            }
            if array_params == 0 {
                return Err(self.err_here(
                    "an array return type needs an array parameter to pin its length in TS-M1",
                ));
            }
            let mut n = "N".to_string();
            if type_params.contains(&n) {
                n = "__N".to_string();
            }
            type_params.push(n.clone());
            self.saw_array_len = false;
            Some(n)
        } else {
            None
        };
        let body = self.block()?;
        Ok(FnDecl {
            name,
            type_params,
            len_param,
            params,
            ret,
            body,
            is_extern: false,
            pos: self.pos_of(&start),
        })
    }

    fn interface_decl(&mut self) -> Result<StructDecl, Diag> {
        let start = self.expect_kw("interface")?;
        let (name, _) = self.expect_ident()?;
        self.expect_punct("{")?;
        let mut fields = Vec::new();
        while !self.at_punct("}") {
            let (fname, ft) = self.expect_ident()?;
            if self.at_punct("(") {
                return Err(self.err_here("method signatures land with the S2 type layer"));
            }
            self.expect_punct(":")?;
            let ty = self.map_type()?;
            fields.push(Param { name: fname, ty, pos: self.pos_of(&ft) });
            // fields may be separated by `;`, `,`, or a line break
            self.eat_punct(";");
            self.eat_punct(",");
        }
        self.expect_punct("}")?;
        let ft: HashMap<String, Type> = fields.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
        self.struct_fields.insert(name.clone(), ft);
        Ok(StructDecl { name, fields, pos: self.pos_of(&start) })
    }

    /// map a TS type annotation onto pipeline types. Array forms
    /// (`T[]` / `Array<T>`) are allowed inside function signatures only and
    /// map to `[T; N]` with the pipeline's generic length sentinel.
    /// `import { a, b } from "./m"` / `import d from "./m"` /
    /// `import * from "./m"` / `import "./m"` -> shared AST ImportDecl.
    /// M1 merges the whole module regardless of the name list.
    fn import_decl_ts(&mut self) -> Result<ImportDecl, Diag> {
        let start = self.expect_kw("import")?;
        let pos = self.pos_of(&start);
        // `import type { ... }` is erased with the rest of the type layer;
        // it still merges the module (its functions may be called)
        if matches!(&self.peek().tok, Tok::Ident(n) if n == "type") {
            self.i += 1;
        }
        if let Tok::Str(pth) = &self.peek().tok {
            let path = pth.clone();
            self.i += 1;
            self.end_stmt()?;
            return Ok(ImportDecl { path, names: None, pos });
        }
        let names = if self.at_punct("*") {
            // `import * from "p"` is the whole-module merge every Aoxn source
            // uses, and it has never needed a namespace object. Only
            // `import * as ns` does — that stays a TS-M2 feature. Peeking one
            // token further distinguishes them; before v0.36.0 this arm
            // rejected both, so the documented form did not compile in a .ts
            // file even though its own error message recommended it.
            if matches!(&self.peek_at(1).tok, Tok::Ident(n) if n == "as") {
                return Err(self.err_here(
                    "`import * as ns` needs namespace objects (TS-M2); use `import * from` or named imports",
                ));
            }
            self.i += 1; // consume the `*`; `from` follows
            None
        } else if self.at_punct("{") {
            self.i += 1;
            let mut ns = Vec::new();
            while !self.at_punct("}") {
                // `type X` members are type-only: erased in M1
                if matches!(&self.peek().tok, Tok::Ident(n) if n == "type") {
                    self.i += 1;
                    self.expect_ident()?;
                } else {
                    let (n, _) = self.expect_ident()?;
                    ns.push(n);
                }
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct("}")?;
            Some(ns)
        } else {
            let (n, _) = self.expect_ident()?;
            Some(vec![n])
        };
        if !(matches!(&self.peek().tok, Tok::Ident(n) if n == "from")) {
            return Err(self.err_here("expected `from` in import"));
        }
        self.i += 1;
        let path = match &self.peek().tok {
            Tok::Str(pth) => pth.clone(),
            _ => return Err(self.err_here("expected a string module path")),
        };
        self.i += 1;
        self.end_stmt()?;
        // `import styles from "./page.module.css"` binds `styles` to the
        // module's class struct. The asset pipeline (src/assets.rs) generates
        // the accessor from the same stem, so recording the binding here is
        // enough for `styles.title` to become a typed field read at the use
        // site. Only the default-import form binds one name; `import * from`
        // and bare side-effect imports keep merging the module as before.
        if let (Some(bound), true) = (&names, is_css_module_path(&path)) {
            if bound.len() == 1 {
                self.css_modules
                    .insert(bound[0].clone(), css_module_accessor(&path));
            }
        }
        Ok(ImportDecl { path, names, pos })
    }

    fn map_type(&mut self) -> Result<Type, Diag> {
        if self.at_punct("[") {
            // tuple: `[A, B]` -> a synthesized value struct with _0.. fields
            self.i += 1;
            let mut elems = Vec::new();
            while !self.at_punct("]") {
                elems.push(self.map_type()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct("]")?;
            return self.tuple_type(&elems);
        }
        let ty = if matches!(&self.peek().tok, Tok::Ident(n) if n == "Array") {
            self.i += 1;
            self.expect_punct("<")?;
            let inner = self.map_type()?;
            self.expect_punct(">")?;
            self.array_type(inner)?
        } else {
            self.map_type_base()?
        };
        if self.at_punct("[") {
            self.expect_punct("[")?;
            self.expect_punct("]")?;
            return self.array_type(ty);
        }
        if self.at_punct("|") {
            // unions: `T | null | undefined` erases to T with sentinel null
            // semantics; a multi-value union keeps its first member in M1
            // (runtime tags for real discriminated unions are TS-M2)
            let mut members = vec![ty];
            while self.at_punct("|") {
                self.i += 1;
                if matches!(&self.peek().tok, Tok::Kw("null") | Tok::Kw("undefined")) {
                    self.i += 1;
                    continue;
                }
                members.push(self.map_type_base()?);
            }
            return Ok(members.into_iter().next().unwrap());
        }
        Ok(ty)
    }

    /// synthesize (once per shape) a tuple struct type: fields _0, _1, ...
    fn tuple_type(&mut self, elems: &[Type]) -> Result<Type, Diag> {
        let mut name = format!("__tuple{}", elems.len());
        for e in elems {
            name.push('_');
            name.push_str(&e.to_string().replace(['[', ']', ';', ' '], ""));
        }
        if !self.struct_fields.contains_key(&name) {
            let mut fields = Vec::new();
            let mut ft = HashMap::new();
            for (i, e) in elems.iter().enumerate() {
                let fname = format!("_{i}");
                fields.push(Param { name: fname.clone(), ty: e.clone(), pos: Pos { line: 0, col: 0, file: self.file } });
                ft.insert(fname, e.clone());
            }
            self.struct_fields.insert(name.clone(), ft);
            self.extra_structs.push(StructDecl {
                name: name.clone(),
                fields,
                pos: Pos { line: 0, col: 0, file: self.file },
            });
        }
        Ok(Type::Struct(name))
    }

    /// scalar or named type (no array suffix)
    fn map_type_base(&mut self) -> Result<Type, Diag> {
        let t = self.peek().clone();
        let kind: Option<String> = match &t.tok {
            Tok::Ident(name) => Some(name.clone()),
            Tok::Kw("void") => Some("void".to_string()),
            Tok::Kw("null") | Tok::Kw("undefined") => {
                return Err(self.err(t, "'null'/'undefined' are only union members (`T | null`); they are not standalone types in TS-M1"))
            }
            _ => None,
        };
        let kind = match kind {
            Some(k) => k,
            None => return Err(self.err(t, "expected a type")),
        };
        let base = match kind.as_str() {
            // S2b numeric tower: `number` is f64 everywhere (JS semantics);
            // explicit `int`/`float` annotations keep their exact meaning
            "number" | "float" => Type::Float,
            "int" => Type::Int,
            "string" => Type::Str,
            "boolean" | "bool" => Type::Bool,
            "void" => Type::Void,
            // any/unknown: an untyped 64-bit box in M1 (values live in the
            // int slot, narrowed at use sites with `as`); never: no value
            "any" | "unknown" => Type::Int,
            "never" => Type::Void,
            other => Type::Struct(other.to_string()),
        };
        self.i += 1;
        Ok(base)
    }

    /// `[T; N]` with the generic length sentinel; registers that this
    /// signature needs the (single) synthesized length parameter
    fn array_type(&mut self, elem: Type) -> Result<Type, Diag> {
        if !self.fn_sig {
            return Err(self.err_here(
                "array fields need a fixed length in TS-M1 (interfaces cannot hold unbounded arrays yet)",
            ));
        }
        self.saw_array_len = true;
        Ok(Type::Array { elem: Box::new(elem), len: GENERIC_LEN })
    }

    /// type annotation that may be an array/tuple type: such annotations are
    /// returned as `None` (the initializer infers element/length)
    fn map_type_loose(&mut self) -> Result<Option<Type>, Diag> {
        // bindings: tuple annotations keep their synthesized struct type;
        // `T[]`/`Array<T>` infer the length from the literal; unions erase
        // null/undefined members and keep the first concrete type
        if self.at_punct("[") {
            return Ok(Some(self.map_type()?));
        }
        if matches!(&self.peek().tok, Tok::Ident(n) if n == "Array") {
            self.i += 1;
            self.expect_punct("<")?;
            self.map_type_loose()?;
            self.expect_punct(">")?;
            if self.at_punct("[") {
                self.i += 1;
                self.expect_punct("]")?;
            }
            return Ok(None);
        }
        let ty = self.map_type_base()?;
        if self.at_punct("[") {
            // `T[]` — length inferred from the literal
            self.i += 1;
            self.expect_punct("]")?;
            return Ok(None);
        }
        if self.at_punct("|") {
            while self.at_punct("|") {
                self.i += 1;
                if matches!(&self.peek().tok, Tok::Kw("null") | Tok::Kw("undefined")) {
                    self.i += 1;
                    continue;
                }
                self.map_type_base()?;
            }
        }
        Ok(Some(ty))
    }

    // ---- statements ----

    fn block(&mut self) -> Result<Block, Diag> {
        self.expect_punct("{")?;
        let mut stmts = Vec::new();
        while !self.at_punct("}") {
            if self.peek().tok == Tok::Eof {
                return Err(self.err_here("unterminated block"));
            }
            stmts.push(self.stmt()?);
        }
        self.expect_punct("}")?;
        Ok(Block { stmts })
    }

    fn block_or_stmt(&mut self) -> Result<Block, Diag> {
        if self.at_punct("{") {
            self.block()
        } else {
            let s = self.stmt()?;
            Ok(Block { stmts: vec![s] })
        }
    }

    fn stmt(&mut self) -> Result<Stmt, Diag> {
        let t = self.peek().clone();
        match &t.tok {
            Tok::Punct(";") => {
                self.i += 1;
                Ok(Stmt::Pass)
            }
            Tok::Punct("{") => {
                // a bare block scopes like an always-taken branch
                let b = self.block()?;
                let pos = self.pos_of(&t);
                Ok(Stmt::If { cond: Expr::Bool(true, pos), then_block: b, else_block: None, pos })
            }
            Tok::Kw("const") | Tok::Kw("let") => self.var_stmt(true),
            Tok::Kw("var") => Err(self.err(t, "var is not supported; use const/let (block scope)")),
            Tok::Kw("if") => self.if_stmt(),
            Tok::Kw("while") => self.while_stmt(),
            Tok::Kw("do") => self.do_stmt(),
            Tok::Kw("for") => self.for_stmt(),
            Tok::Kw("return") => {
                self.i += 1;
                let expr = if self.stmt_ends() { None } else { Some(self.expr()?) };
                self.end_stmt()?;
                Ok(Stmt::Return { expr, pos: self.pos_of(&t) })
            }
            Tok::Kw("break") => {
                self.i += 1;
                self.end_stmt()?;
                Ok(Stmt::Break { pos: self.pos_of(&t) })
            }
            Tok::Kw("continue") => {
                self.i += 1;
                self.end_stmt()?;
                if self.loops.last() == Some(&LoopKind::CFor) {
                    return Err(self.err(t, "continue in a C-style for loop would skip the step; use while (lands with the S2 statement slice)"));
                }
                Ok(Stmt::Continue { pos: self.pos_of(&t) })
            }
            Tok::Kw("switch") => Err(self.err(t, "switch lands in a later TS-M1 slice")),
            Tok::Kw("try") | Tok::Kw("throw") => Err(self.err(t, "try/throw land in a later TS-M1 slice")),
            Tok::Kw("function") | Tok::Kw("class") | Tok::Kw("interface") => {
                Err(self.err(t, "nested declarations are not supported in TS-M1-S1"))
            }
            Tok::Kw("import") | Tok::Kw("export") => {
                Err(self.err(t, "TS modules land in W1-S3; import/export is not accepted yet"))
            }
            _ => self.expr_stmt(),
        }
    }

    fn stmt_ends(&self) -> bool {
        self.at_punct(";") || self.at_punct("}") || self.peek().tok == Tok::Eof || self.peek().nl_before
    }

    fn pos_of(&self, t: &Token) -> Pos {
        Pos { line: t.line, col: t.col, file: self.file }
    }

    fn var_stmt(&mut self, eat_end: bool) -> Result<Stmt, Diag> {
        let start = self.bump(); // const | let
        let (name, _) = self.expect_ident()?;
        self.eat_punct("!"); // definite-assignment marker, dropped
        let ty = if self.eat_punct(":") {
            self.map_type_loose()?
        } else {
            None
        };
        self.expect_punct("=")?;
        let mut expr = if self.at_punct("{") {
            // object literal -> struct literal; needs the interface annotation
            let sname = match &ty {
                Some(Type::Struct(s)) => s.clone(),
                _ => return Err(self.err_here("object literals need an interface annotation in TS-M1-S1")),
            };
            self.object_lit(&sname)?
        } else if matches!(&self.peek().tok, Tok::Kw("null") | Tok::Kw("undefined")) {
            // a null initializer needs the annotated type to pick a sentinel
            let nt = self.peek().clone();
            self.i += 1;
            match &ty {
                Some(t) => self.sentinel(t, self.pos_of(&nt)),
                None => {
                    return Err(self.err(nt, "null/undefined literals need a `T | null` annotation in TS-M1"))
                }
            }
        } else {
            self.expr()?
        };
        // a tuple annotation turns a bracket literal into the synthesized
        // struct literal (`[1, "a"]` -> `__tuple2_..(_0=1, _1="a")`)
        if let Some(Type::Struct(tname)) = &ty {
            if tname.starts_with("__tuple") {
                if let Expr::ArrayLit { elems, lit_id, pos } = expr {
                    let fields = elems
                        .into_iter()
                        .enumerate()
                        .map(|(i, v)| (format!("_{i}"), v))
                        .collect();
                    expr = Expr::StructLit { name: tname.clone(), fields, lit_id, pos };
                }
            }
        }
        // shape numeric literals against the annotation (JS `number` is f64;
        // an explicit `int` annotation keeps integer literals integer)
        if let Some(t) = &ty {
            expr = self.shape_num(expr, t);
            self.vars.insert(name.clone(), t.clone());
        } else {
            let it = self.infer_type(&expr);
            if let Some(Type::Int) = &it {
                expr = self.shape_num(expr, &Type::Float);
            }
            self.vars.insert(name.clone(), it.unwrap_or(Type::Int));
        }
        if eat_end {
            self.end_stmt()?;
        }
        Ok(Stmt::Let { name, ty, expr, pos: self.pos_of(&start) })
    }

    /// `{ x: 1, y: 2 }` -> `StructLit` for the annotated interface
    fn object_lit(&mut self, type_name: &str) -> Result<Expr, Diag> {
        let start = self.expect_punct("{")?;
        let mut fields = Vec::new();
        while !self.at_punct("}") {
            let (fname, _) = self.expect_ident()?;
            self.expect_punct(":")?;
            let v = self.expr()?;
            fields.push((fname, v));
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_punct("}")?;
        Ok(Expr::StructLit {
            name: type_name.to_string(),
            fields,
            lit_id: self.next_lit(),
            pos: self.pos_of(&start),
        })
    }

    fn if_stmt(&mut self) -> Result<Stmt, Diag> {
        let start = self.expect_kw("if")?;
        self.expect_punct("(")?;
        let cond = self.expr()?;
        self.expect_punct(")")?;
        let then_block = self.block_or_stmt()?;
        let else_block = if self.eat_kw("else") {
            Some(self.block_or_stmt()?)
        } else {
            None
        };
        Ok(Stmt::If { cond, then_block, else_block, pos: self.pos_of(&start) })
    }

    fn while_stmt(&mut self) -> Result<Stmt, Diag> {
        let start = self.expect_kw("while")?;
        self.expect_punct("(")?;
        let cond = self.expr()?;
        self.expect_punct(")")?;
        self.loops.push(LoopKind::WhileLike);
        let body = self.block_or_stmt()?;
        self.loops.pop();
        Ok(Stmt::While { cond, body, pos: self.pos_of(&start) })
    }

    /// `do B while (c);` -> `while true { B if !c { break } }` (no cloning)
    fn do_stmt(&mut self) -> Result<Stmt, Diag> {
        let start = self.expect_kw("do")?;
        self.loops.push(LoopKind::WhileLike);
        let mut body = self.block_or_stmt()?;
        self.loops.pop();
        self.expect_kw("while")?;
        self.expect_punct("(")?;
        let cond = self.expr()?;
        self.expect_punct(")")?;
        self.end_stmt()?;
        let pos = self.pos_of(&start);
        let brk = Stmt::If {
            cond: Expr::Unary { op: UnOp::Not, expr: Box::new(cond), pos },
            then_block: Block { stmts: vec![Stmt::Break { pos }] },
            else_block: None,
            pos,
        };
        body.stmts.push(brk);
        Ok(Stmt::While { cond: Expr::Bool(true, pos), body, pos })
    }

    fn for_stmt(&mut self) -> Result<Stmt, Diag> {
        let start = self.expect_kw("for")?;
        self.expect_punct("(")?;
        // for-of: `for (const x of arr)`
        if (self.at_kw("const") || self.at_kw("let") || self.at_kw("var"))
            && matches!(&self.peek_at(2).tok, Tok::Ident(n) if n == "of")
        {
            if self.at_kw("var") {
                return Err(self.err_here("var is not supported; use const/let"));
            }
            self.i += 1;
            let (var, _) = self.expect_ident()?;
            self.i += 1; // `of`
            let iter = self.expr()?;
            self.expect_punct(")")?;
            self.loops.push(LoopKind::WhileLike);
            let body = self.block_or_stmt()?;
            self.loops.pop();
            return Ok(Stmt::For { var, iter: ForIter::Array(iter), body, pos: self.pos_of(&start) });
        }
        // C-style: init ; cond ; step  (desugars to init + while, no cloning)
        let init = if self.eat_punct(";") {
            None
        } else {
            let s = if self.at_kw("const") || self.at_kw("let") {
                self.var_stmt(false)?
            } else {
                self.simple_assign_stmt(false)?
            };
            self.expect_punct(";")?;
            Some(s)
        };
        let cond = if self.at_punct(";") {
            Expr::Bool(true, self.pos_of(&start))
        } else {
            self.expr()?
        };
        self.expect_punct(";")?;
        let step = if self.at_punct(")") {
            None
        } else {
            Some(self.simple_assign_stmt(false)?)
        };
        self.expect_punct(")")?;
        self.loops.push(LoopKind::CFor);
        let mut body = self.block_or_stmt()?;
        self.loops.pop();
        if let Some(step) = step {
            body.stmts.push(step);
        }
        let w = Stmt::While { cond, body, pos: self.pos_of(&start) };
        let mut stmts = Vec::new();
        if let Some(init) = init {
            stmts.push(init);
        }
        stmts.push(w);
        // one statement out: an always-taken branch wrapping init + loop
        Ok(Stmt::If {
            cond: Expr::Bool(true, self.pos_of(&start)),
            then_block: Block { stmts },
            else_block: None,
            pos: self.pos_of(&start),
        })
    }

    /// assignment-shaped statement (`x = e`, `x += e`, `i++`, `a[i] = e`)
    fn simple_assign_stmt(&mut self, eat_end: bool) -> Result<Stmt, Diag> {
        // fast path: `i++` / `i--` (postfix increments are not expressions)
        if matches!(&self.peek().tok, Tok::Ident(_))
            && matches!(&self.peek_at(1).tok, Tok::Punct(p) if *p == "++" || *p == "--")
        {
            let (name, nt) = self.expect_ident()?;
            let pos = self.pos_of(&nt);
            let target = Expr::Var { name, pos };
            let rhs_target = target.clone();
            let up = self.at_punct("++");
            self.i += 1;
            if eat_end {
                self.end_stmt()?;
            }
            let bin = if up { BinOp::Add } else { BinOp::Sub };
            return Ok(Stmt::Assign {
                target,
                expr: self.mk_bin(bin, rhs_target, Expr::Int(1, pos), pos),
                pos,
            });
        }
        let target = self.expr()?;
        let pos = target.pos();
        let op = self.peek().clone();
        let rhs_target = target.clone();
        match &op.tok {
            Tok::Punct("=") => {
                self.i += 1;
                let mut expr = self.expr()?;
                if !target.is_lvalue() {
                    return Err(self.err(op, "invalid assignment target"));
                }
                // keep the target's numeric width (`let x = 5; x = 7` must
                // stay float, `let n: int = 1; n = 2` must stay int)
                if let Expr::Var { name, .. } = &target {
                    if let Some(t) = self.vars.get(name).cloned() {
                        expr = self.shape_num(expr, &t);
                    }
                }
                if eat_end {
                    self.end_stmt()?;
                }
                Ok(Stmt::Assign { target, expr, pos })
            }
            Tok::Punct("+=") | Tok::Punct("-=") | Tok::Punct("*=") | Tok::Punct("/=") | Tok::Punct("%=") => {
                self.i += 1;
                let expr = self.expr()?;
                if !target.is_lvalue() {
                    return Err(self.err(op, "invalid assignment target"));
                }
                let bin = match &op.tok {
                    Tok::Punct("+=") => BinOp::Add,
                    Tok::Punct("-=") => BinOp::Sub,
                    Tok::Punct("*=") => BinOp::Mul,
                    Tok::Punct("/=") => BinOp::Div,
                    _ => BinOp::Mod,
                };
                if eat_end {
                    self.end_stmt()?;
                }
                // note: a compound assignment re-evaluates the target
                // (`a[f()] += 1` calls f twice), matching neither JS nor
                // ideal semantics; documented S1 limitation
                Ok(Stmt::Assign {
                    target,
                    expr: self.mk_bin(bin, rhs_target, expr, pos),
                    pos,
                })
            }
            Tok::Punct("++") | Tok::Punct("--") => {
                self.i += 1;
                if !target.is_lvalue() {
                    return Err(self.err(op, "invalid increment target"));
                }
                if eat_end {
                    self.end_stmt()?;
                }
                let bin = if matches!(&op.tok, Tok::Punct("++")) { BinOp::Add } else { BinOp::Sub };
                Ok(Stmt::Assign {
                    target,
                    expr: self.mk_bin(bin, rhs_target, Expr::Int(1, pos), pos),
                    pos,
                })
            }
            _ => Err(self.err(op, "expected an assignment")),
        }
    }

    fn expr_stmt(&mut self) -> Result<Stmt, Diag> {
        let save = self.i;
        // fast path: `x++` / `x--` statement
        if matches!(&self.peek().tok, Tok::Ident(_))
            && matches!(&self.peek_at(1).tok, Tok::Punct(p) if *p == "++" || *p == "--")
        {
            return self.simple_assign_stmt(true);
        }
        self.i = save;
        let target = self.expr()?;
        let op = self.peek().clone();
        match &op.tok {
            Tok::Punct("=") | Tok::Punct("+=") | Tok::Punct("-=") | Tok::Punct("*=") | Tok::Punct("/=")
            | Tok::Punct("%=") | Tok::Punct("++") | Tok::Punct("--") => {
                self.i = save;
                self.simple_assign_stmt(true)
            }
            _ => {
                self.end_stmt()?;
                Ok(Stmt::ExprStmt { expr: target })
            }
        }
    }

    // ---- expressions ----

    fn expr(&mut self) -> Result<Expr, Diag> {
        self.assign_expr()
    }

    fn assign_expr(&mut self) -> Result<Expr, Diag> {
        let lhs = self.or_expr()?;
        // assignment operators are consumed by the statement layer
        // (simple_assign_stmt); only the ternary is rejected here
        let t = self.peek().clone();
        if matches!(&t.tok, Tok::Punct("?")) {
            return Err(self.err(t, "the ternary operator lands with the S2 expression slice"));
        }
        Ok(lhs)
    }

    fn or_expr(&mut self) -> Result<Expr, Diag> {
        let mut lhs = self.and_expr()?;
        while self.at_punct("||") || self.at_punct("??") {
            let op = self.bump();
            let rhs = self.and_expr()?;
            let pos = self.pos_of(&op);
            lhs = self.mk_bin(BinOp::Or, lhs, rhs, pos);
        }
        Ok(lhs)
    }

    fn and_expr(&mut self) -> Result<Expr, Diag> {
        let mut lhs = self.bit_or_expr()?;
        while self.at_punct("&&") {
            let op = self.bump();
            let rhs = self.bit_or_expr()?;
            let pos = self.pos_of(&op);
            lhs = self.mk_bin(BinOp::And, lhs, rhs, pos);
        }
        Ok(lhs)
    }

    /// bitwise `|` (JS int32 semantics via the __ts_ helpers)
    fn bit_or_expr(&mut self) -> Result<Expr, Diag> {
        let mut lhs = self.bit_xor_expr()?;
        while self.at_punct("|") {
            let op = self.bump();
            let rhs = self.bit_xor_expr()?;
            lhs = self.mk_bitop("__ts_bor", lhs, rhs, self.pos_of(&op));
        }
        Ok(lhs)
    }

    fn bit_xor_expr(&mut self) -> Result<Expr, Diag> {
        let mut lhs = self.bit_and_expr()?;
        while self.at_punct("^") {
            let op = self.bump();
            let rhs = self.bit_and_expr()?;
            lhs = self.mk_bitop("__ts_bxor", lhs, rhs, self.pos_of(&op));
        }
        Ok(lhs)
    }

    fn bit_and_expr(&mut self) -> Result<Expr, Diag> {
        let mut lhs = self.equality_expr()?;
        while self.at_punct("&") {
            let op = self.bump();
            let rhs = self.equality_expr()?;
            lhs = self.mk_bitop("__ts_band", lhs, rhs, self.pos_of(&op));
        }
        Ok(lhs)
    }

    fn equality_expr(&mut self) -> Result<Expr, Diag> {
        let mut lhs = self.relational_expr()?;
        loop {
            let op = self.peek().clone();
            let bop = match &op.tok {
                Tok::Punct("==") | Tok::Punct("===") => BinOp::Eq,
                Tok::Punct("!=") | Tok::Punct("!==") => BinOp::Ne,
                _ => break,
            };
            self.i += 1;
            // `x == null` / `x != undefined` — null narrowing compares
            // against the type's sentinel (M1 has no nullable values)
            if matches!(&self.peek().tok, Tok::Kw("null") | Tok::Kw("undefined")) {
                let nt = self.peek().clone();
                self.i += 1;
                let pos = self.pos_of(&op);
                let t = self.infer_type(&lhs).ok_or_else(|| {
                    self.err(nt.clone(), "cannot judge null on an expression of unknown type (annotate it)")
                })?;
                let s = self.sentinel(&t, self.pos_of(&nt));
                lhs = Expr::Binary { op: bop, lhs: Box::new(lhs), rhs: Box::new(s), pos };
                continue;
            }
            let rhs = self.relational_expr()?;
            let pos = self.pos_of(&op);
            lhs = self.mk_bin(bop, lhs, rhs, pos);
        }
        Ok(lhs)
    }

    fn relational_expr(&mut self) -> Result<Expr, Diag> {
        let mut lhs = self.shift_expr()?;
        loop {
            let op = self.peek().clone();
            let bop = match &op.tok {
                Tok::Punct("<") => BinOp::Lt,
                Tok::Punct("<=") => BinOp::Le,
                Tok::Punct(">") => BinOp::Gt,
                Tok::Punct(">=") => BinOp::Ge,
                Tok::Kw("instanceof") => return Err(self.err(op, "instanceof lands with the S2 type layer")),
                Tok::Kw("in") => return Err(self.err(op, "the `in` operator lands with the S2 type layer")),
                _ => break,
            };
            self.i += 1;
            let rhs = self.shift_expr()?;
            let pos = self.pos_of(&op);
            lhs = self.mk_bin(bop, lhs, rhs, pos);
        }
        Ok(lhs)
    }

    /// `<<` / `>>` / `>>>` (JS int32 semantics via the __ts_ helpers)
    fn shift_expr(&mut self) -> Result<Expr, Diag> {
        let mut lhs = self.additive_expr()?;
        loop {
            let name = match &self.peek().tok {
                Tok::Punct("<<") => "__ts_shl",
                Tok::Punct(">>") => "__ts_sar",
                Tok::Punct(">>>") => "__ts_shr",
                _ => break,
            };
            let op = self.bump();
            let rhs = self.additive_expr()?;
            lhs = self.mk_bitop(name, lhs, rhs, self.pos_of(&op));
        }
        Ok(lhs)
    }

    fn additive_expr(&mut self) -> Result<Expr, Diag> {
        let mut lhs = self.multiplicative_expr()?;
        loop {
            let op = self.peek().clone();
            let bop = match &op.tok {
                Tok::Punct("+") => BinOp::Add,
                Tok::Punct("-") => BinOp::Sub,
                _ => break,
            };
            self.i += 1;
            let rhs = self.multiplicative_expr()?;
            let pos = self.pos_of(&op);
            lhs = self.mk_bin(bop, lhs, rhs, pos);
        }
        Ok(lhs)
    }

    fn multiplicative_expr(&mut self) -> Result<Expr, Diag> {
        let mut lhs = self.unary_expr()?;
        loop {
            let op = self.peek().clone();
            let bop = match &op.tok {
                Tok::Punct("*") => BinOp::Mul,
                Tok::Punct("/") => BinOp::Div,
                Tok::Punct("%") => BinOp::Mod,
                _ => break,
            };
            self.i += 1;
            let rhs = self.unary_expr()?;
            let pos = self.pos_of(&op);
            lhs = self.mk_bin(bop, lhs, rhs, pos);
        }
        Ok(lhs)
    }

    fn unary_expr(&mut self) -> Result<Expr, Diag> {
        let t = self.peek().clone();
        match &t.tok {
            Tok::Punct("!") => {
                self.i += 1;
                let e = self.unary_expr()?;
                Ok(Expr::Unary { op: UnOp::Not, expr: Box::new(e), pos: self.pos_of(&t) })
            }
            Tok::Punct("-") => {
                self.i += 1;
                let e = self.unary_expr()?;
                Ok(Expr::Unary { op: UnOp::Neg, expr: Box::new(e), pos: self.pos_of(&t) })
            }
            Tok::Punct("+") => {
                self.i += 1;
                self.unary_expr()
            }
            Tok::Punct("~") => {
                self.i += 1;
                let e = self.unary_expr()?;
                Ok(self.mk_bitop_un("__ts_bnot", e, self.pos_of(&t)))
            }
            Tok::Kw("typeof") | Tok::Kw("void") | Tok::Kw("delete") => {
                Err(self.err(t, "this operator lands with the S2 type layer"))
            }
            Tok::Kw("new") => Err(self.err(t, "new/class construction lands in a later TS-M1 slice")),
            Tok::Kw("await") | Tok::Kw("yield") => Err(self.err(t, "async lands in TS-M2")),
            _ => self.postfix_expr(),
        }
    }

    fn postfix_expr(&mut self) -> Result<Expr, Diag> {
        let mut e = self.primary()?;
        loop {
            let t = self.peek().clone();
            match &t.tok {
                Tok::Punct("<") => {
                    // `f<T>(x)` type arguments vs `a < b` comparison: when
                    // the shape is unambiguous type args, reject explicitly
                    if self.looks_like_type_args() {
                        return Err(self.reject_explicit_type_args(&e));
                    }
                    break;
                }
                Tok::Punct("(") => {
                    self.i += 1;
                    let mut args = Vec::new();
                    while !self.at_punct(")") {
                        let v = self.expr()?;
                        args.push(Arg { name: None, value: v });
                        if !self.eat_punct(",") {
                            break;
                        }
                    }
                    self.expect_punct(")")?;
                    e = match e {
                        // `console.log(x)` -> `print(x)`
                        Expr::Field { obj, name, .. }
                            if matches!(&*obj, Expr::Var { name: n, .. } if n == "console") =>
                        {
                            if name == "log" {
                                if args.is_empty() {
                                    return Err(self.err(t, "console.log needs at least one argument"));
                                }
                                // JS prints values space-separated on one
                                // line; numbers render in JS number form
                                let mut formatted = Vec::new();
                                for a in args {
                                    formatted.push(self.log_arg(a.value));
                                }
                                let pos = self.pos_of(&t);
                                let mut out = formatted.remove(0);
                                for f in formatted {
                                    out = Expr::Binary {
                                        op: BinOp::Add,
                                        lhs: Box::new(Expr::Binary {
                                            op: BinOp::Add,
                                            lhs: Box::new(out),
                                            rhs: Box::new(Expr::Str(" ".into(), pos)),
                                            pos,
                                        }),
                                        rhs: Box::new(f),
                                        pos,
                                    };
                                }
                                Expr::Call {
                                    name: "print".into(),
                                    args: vec![Arg { name: None, value: out }],
                                    lit_id: self.next_lit(),
                                    pos,
                                }
                            } else {
                                return Err(self.err(t, format!("console.{name} is not supported in TS-M1-S1")));
                            }
                        }
                        Expr::Var { name, pos } => Expr::Call { name, args, lit_id: self.next_lit(), pos },
                        _ => return Err(self.err(t, "method calls land with the S2 type layer")),
                    };
                }
                Tok::Punct(".") => {
                    self.i += 1;
                    let (name, _) = self.expect_ident()?;
                    e = if name == "length" {
                        // `x.length` -> `float(len(x))` (JS numbers are f64)
                        let pos = self.pos_of(&t);
                        Expr::Cast {
                            expr: Box::new(Expr::Call {
                                name: "len".into(),
                                args: vec![Arg { name: None, value: e }],
                                lit_id: self.next_lit(),
                                pos,
                            }),
                            to: Type::Float,
                            pos,
                        }
                    } else {
                        Expr::Field { obj: Box::new(e), name, pos: self.pos_of(&t) }
                    };
                }
                Tok::Punct("[") => {
                    self.i += 1;
                    let idx = self.expr()?;
                    self.expect_punct("]")?;
                    // tuple structs read their fields through `t[0]`
                    let tuple_idx: Option<i64> = match &idx {
                        Expr::Float(v, _) if v.fract() == 0.0 && *v >= 0.0 => Some(*v as i64),
                        Expr::Int(v, _) if *v >= 0 => Some(*v),
                        _ => None,
                    };
                    if let Some(iv) = tuple_idx {
                        if let Some(Type::Struct(sname)) = self.infer_type(&e) {
                            let fname = format!("_{iv}");
                            if self.struct_fields.get(&sname).map(|m| m.contains_key(&fname)).unwrap_or(false) {
                                e = Expr::Field { obj: Box::new(e), name: fname, pos: self.pos_of(&t) };
                                continue;
                            }
                        }
                    }
                    // the numeric tower is f64; indexes are ints — convert
                    // on use (identity casts fold away at codegen)
                    let idx = Expr::Cast { expr: Box::new(idx), to: Type::Int, pos: self.pos_of(&t) };
                    e = Expr::Index { arr: Box::new(e), idx: Box::new(idx), pos: self.pos_of(&t) };
                }
                Tok::Ident(n) if n == "as" => {
                    // `x as T` — a checked scalar conversion where the tower
                    // has one (int/float/bool), a trust-me widening otherwise
                    self.i += 1;
                    let to = self.map_type()?;
                    let pos = self.pos_of(&t);
                    e = match to {
                        Type::Int | Type::Float | Type::Bool => Expr::Cast { expr: Box::new(e), to, pos },
                        _ => e,
                    };
                }
                Tok::Punct("!") => {
                    // `x!` non-null assertion: null/undefined do not exist in
                    // the M1 value model, so the assertion is an identity
                    self.i += 1;
                }
                Tok::Punct("?.") => return Err(self.err(t, "optional chaining lands with the S2 type layer")),
                Tok::Punct("++") | Tok::Punct("--") => {
                    return Err(self.err(t, "increment/decrement as a value lands with S2; use a statement"))
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, Diag> {
        let t = self.peek().clone();
        match &t.tok {
            Tok::Number(raw) => {
                self.i += 1;
                let n = parse_number(raw).ok_or_else(|| self.err(t.clone(), format!("invalid number literal '{raw}'")))?;
                match n {
                    // every numeric literal is a JS `number` (f64); integer
                    // positions (indexes, length params) convert on use
                    NumLit::Int(v) => Ok(Expr::Float(v as f64, self.pos_of(&t))),
                    NumLit::Float(v) => Ok(Expr::Float(v, self.pos_of(&t))),
                }
            }
            Tok::Str(s) => {
                self.i += 1;
                Ok(Expr::Str(s.clone(), self.pos_of(&t)))
            }
            Tok::Template(parts) => {
                let parts = parts.clone();
                self.i += 1;
                self.template_chain(parts, &t)
            }
            Tok::Kw("true") => {
                self.i += 1;
                Ok(Expr::Bool(true, self.pos_of(&t)))
            }
            Tok::Kw("false") => {
                self.i += 1;
                Ok(Expr::Bool(false, self.pos_of(&t)))
            }
            Tok::Kw("null") | Tok::Kw("undefined") => {
                Err(self.err(t, "null/undefined land with the S2 value layer (TS-M1 has no nullable values)"))
            }
            Tok::Ident(name) => {
                self.i += 1;
                // A CSS-module binding is not a variable: it names the module's
                // class struct, reached through the generated accessor. The
                // rewrite happens here so `styles.title` parses as a field read
                // on that call and typechecks like any other struct field.
                if let Some(accessor) = self.css_modules.get(name) {
                    return Ok(Expr::Call {
                        name: accessor.clone(),
                        args: Vec::new(),
                        pos: self.pos_of(&t),
                        lit_id: self.next_lit(),
                    });
                }
                Ok(Expr::Var { name: name.clone(), pos: self.pos_of(&t) })
            }
            Tok::Punct("(") => {
                self.i += 1;
                let e = self.expr()?;
                self.expect_punct(")")?;
                Ok(e)
            }
            Tok::Punct("[") => {
                self.i += 1;
                let mut elems = Vec::new();
                while !self.at_punct("]") {
                    elems.push(self.expr()?);
                    if !self.eat_punct(",") {
                        break;
                    }
                }
                self.expect_punct("]")?;
                if elems.is_empty() {
                    return Err(self.err(t, "empty array literals land with the S2 type layer"));
                }
                Ok(Expr::ArrayLit { elems, lit_id: self.next_lit(), pos: self.pos_of(&t) })
            }
            Tok::Punct("{") => Err(self.err(t, "object literals need an interface annotation in TS-M1-S1")),
            Tok::Kw("function") => Err(self.err(t, "function expressions/arrow functions land with TS-M2 closures")),
            _ => Err(self.err(t, "expected an expression")),
        }
    }

    /// `...${e}...` desugars to a `"lit" + str(e) + ...` chain (the same
    /// lowering the Aoxn f-string parser performs); substitutions parse
    /// through a sub-parser over their own token stream
    fn template_chain(&mut self, parts: Vec<TplPart>, t: &Token) -> Result<Expr, Diag> {
        let pos = self.pos_of(t);
        let mut acc: Option<Expr> = None;
        for part in parts {
            let e = match part {
                TplPart::Lit(s) => Expr::Str(s, pos),
                TplPart::Expr(toks) => {
                    let mut sub = Parser {
                        toks,
                        i: 0,
                        file: self.file,
                        lit_id: self.next_lit(),
                        loops: Vec::new(),
                        fn_sig: false,
                        saw_array_len: false,
                        vars: self.vars.clone(),
                        fn_rets: self.fn_rets.clone(),
                        fn_params: self.fn_params.clone(),
                        struct_fields: self.struct_fields.clone(),
                        need_runtime: false,
                        extra_structs: Vec::new(),
                        css_modules: HashMap::new(),
                        fn_opts: HashMap::new(),
                    };
                    let e = sub.expr()?;
                    if sub.need_runtime {
                        self.need_runtime = true;
                    }
                    if sub.peek().tok != Tok::Eof {
                        return Err(sub.err_here("unexpected token in template substitution"));
                    }
                    let e = self.log_arg(e);
                    match e {
                        Expr::Str(..) => e,
                        other => Expr::Call {
                            name: "str".into(),
                            args: vec![Arg { name: None, value: other }],
                            lit_id: self.next_lit(),
                            pos,
                        },
                    }
                }
            };
            acc = Some(match acc {
                None => e,
                Some(a) => self.mk_bin(BinOp::Add, a, e, pos),
            });
        }
        Ok(acc.unwrap_or(Expr::Str(String::new(), pos)))
    }

    /// `<T, U>` after a callee: explicit type arguments. The pipeline infers
    /// generics at call sites, so these could be dropped — but `a < b > (c)`
    /// is a valid comparison chain with the same shape, and silently
    /// reinterpreting it would be wrong code; reject with a clear message.
    fn reject_explicit_type_args(&mut self, callee: &Expr) -> Diag {
        let _ = callee;
        self.err_here("explicit type arguments are not supported in TS-M1 (generic calls infer them; remove the <...>)")
    }

    /// does the `<...>` ahead look like type arguments (idents/commas/dots/
    /// brackets, then `>` immediately followed by `(`)?
    fn looks_like_type_args(&self) -> bool {
        let mut j = self.i + 1;
        let mut depth: i32 = 0;
        loop {
            let t = &self.toks[j.min(self.toks.len() - 1)];
            match &t.tok {
                Tok::Ident(_) | Tok::Punct(",") | Tok::Punct(".") | Tok::Punct("[") | Tok::Punct("]")
                | Tok::Kw("void") => {}
                Tok::Punct("<") => depth += 1,
                Tok::Punct(">") if depth == 0 => {
                    return matches!(
                        self.toks[(j + 1).min(self.toks.len() - 1)].tok,
                        Tok::Punct("(")
                    )
                }
                Tok::Punct(">") => depth -= 1,
                Tok::Eof => return false,
                _ => return false,
            }
            j += 1;
        }
    }

    fn next_lit(&mut self) -> usize {
        self.lit_id += 1;
        self.lit_id
    }

    /// parse-time expression-type inference (enough for literal shaping and
    /// console.log formatting; constructs it cannot see through infer None)
    fn infer_type(&self, e: &Expr) -> Option<Type> {
        match e {
            Expr::Int(..) => Some(Type::Int),
            Expr::Float(..) => Some(Type::Float),
            Expr::Str(..) => Some(Type::Str),
            Expr::Bool(..) => Some(Type::Bool),
            Expr::NoneLit(_) => Some(Type::None),
            Expr::DictLit { .. } => None,
            Expr::Var { name, .. } => self.vars.get(name).cloned(),
            Expr::Cast { to, .. } => Some(to.clone()),
            Expr::Unary { op, expr, .. } => match op {
                UnOp::Not => Some(Type::Bool),
                // TypeScript's `~` is a bitwise NOT over `number`, which is
                // f64 on the Aoxn side unless both operands are integers
                UnOp::BitNot => Some(Type::Int),
                UnOp::Neg => self.infer_type(expr),
            },
            Expr::Binary { op, lhs, .. } => match op {
                BinOp::And | BinOp::Or | BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Some(Type::Bool),
                _ => self.infer_type(lhs),
            },
            Expr::Index { arr, .. } => match self.infer_type(arr)? {
                Type::Array { elem, .. } => Some(*elem),
                _ => None,
            },
            Expr::Field { obj, name, .. } => {
                let ot = self.infer_type(obj)?;
                match ot {
                    Type::Struct(sname) => self.struct_fields.get(&sname).and_then(|m| m.get(name)).cloned(),
                    _ => None,
                }
            }
            Expr::Call { name, args, .. } => match name.as_str() {
                "print" => None,
                "len" | "to_int" => Some(Type::Int),
                "str" | "__ts_num" => Some(Type::Str),
                "to_float" => Some(Type::Float),
                _ => self.ret_of_call(name, args),
            },
            Expr::ArrayLit { elems, .. } => {
                let elem = elems.first().and_then(|x| self.infer_type(x))?;
                Some(Type::Array { elem: Box::new(elem), len: elems.len() })
            }
            Expr::ArrayRep { elem, count, .. } => {
                let et = self.infer_type(elem)?;
                Some(Type::Array { elem: Box::new(et), len: *count })
            }
            Expr::StructLit { name, .. } => Some(Type::Struct(name.clone())),
        }
    }

    /// the null/undefined value for a type: "" / 0 / false / 0-slot.
    /// Documented M1 deviation: the sentinel is indistinguishable from a
    /// legitimate zero-ish value (real nullability is TS-M2).
    fn sentinel(&self, t: &Type, pos: Pos) -> Expr {
        match t {
            Type::Str => Expr::Str(String::new(), pos),
            Type::Float => Expr::Float(0.0, pos),
            Type::Bool => Expr::Bool(false, pos),
            _ => Expr::Int(0, pos),
        }
    }

    /// reshape a numeric literal to `want` (int <-> float). Non-literals and
    /// non-numeric wants pass through unchanged.
    fn shape_num(&self, e: Expr, want: &Type) -> Expr {
        match (&e, want) {
            (Expr::Int(v, _), Type::Float) => Expr::Float(*v as f64, e.pos()),
            (Expr::Float(v, _), Type::Int) if v.fract() == 0.0 && *v >= i64::MIN as f64 && *v <= i64::MAX as f64 => {
                Expr::Int(*v as i64, e.pos())
            }
            _ => e,
        }
    }

    /// build a binary op, reconciling mixed int/float numeric literals the
    /// way JS numbers do (a lone literal yields to the other side's width)
    fn mk_bin(&mut self, op: BinOp, lhs: Expr, rhs: Expr, pos: Pos) -> Expr {
        let (l, r) = match (self.infer_type(&lhs), self.infer_type(&rhs)) {
            (Some(Type::Float), Some(Type::Int)) => (lhs, self.shape_num(rhs, &Type::Float)),
            (Some(Type::Int), Some(Type::Float)) => (self.shape_num(lhs, &Type::Float), rhs),
            _ => (lhs, rhs),
        };
        if op == BinOp::Mod {
            // JS `%` is fmod on the f64 tower (Aoxn's % is int-only)
            return self.mk_bitop("__ts_mod", l, r, pos);
        }
        Expr::Binary { op, lhs: Box::new(l), rhs: Box::new(r), pos }
    }

    /// lower a binary bitwise op to its __ts_ runtime helper; operands go
    /// through float() so int/bool values carry their numeric value across
    fn mk_bitop(&mut self, name: &str, lhs: Expr, rhs: Expr, pos: Pos) -> Expr {
        self.need_runtime = true;
        Expr::Call {
            name: name.to_string(),
            args: vec![Arg { name: None, value: self.wrap_f(lhs) }, Arg { name: None, value: self.wrap_f(rhs) }],
            pos,
            lit_id: 0,
        }
    }

    /// unary bitwise op (`~`)
    fn mk_bitop_un(&mut self, name: &str, e: Expr, pos: Pos) -> Expr {
        self.need_runtime = true;
        Expr::Call {
            name: name.to_string(),
            args: vec![Arg { name: None, value: self.wrap_f(e) }],
            pos,
            lit_id: 0,
        }
    }

    /// `float(e)` — carries any scalar across to the f64 tower
    fn wrap_f(&self, e: Expr) -> Expr {
        let p = e.pos();
        Expr::Call { name: "to_float".to_string(), args: vec![Arg { name: None, value: e }], pos: p, lit_id: 0 }
    }

    /// return type of a user call, with lightweight generic inference: a
    /// declared return of `T` resolves through type-parameter bindings
    /// learned from the argument types (`maxOf([3,1])` -> element type)
    fn ret_of_call(&self, name: &str, args: &[Arg]) -> Option<Type> {
        let ret = self.fn_rets.get(name)?;
        let mut bind: HashMap<String, Type> = HashMap::new();
        if let Some(params) = self.fn_params.get(name) {
            for (i, pt) in params.iter().enumerate() {
                if let Some(arg) = args.get(i) {
                    if let Some(at) = self.infer_type(&arg.value) {
                        self.bind_param(pt, &at, &mut bind);
                    }
                }
            }
        }
        Some(self.resolve_t(ret, &bind))
    }

    /// learn T = concrete from a declared param shape vs the argument type
    fn bind_param(&self, declared: &Type, actual: &Type, bind: &mut HashMap<String, Type>) {
        match (declared, actual) {
            (Type::Struct(tp), at) if !self.struct_fields.contains_key(tp) => {
                bind.entry(tp.clone()).or_insert_with(|| at.clone());
            }
            (Type::Array { elem: de, .. }, Type::Array { elem: ae, .. }) => {
                self.bind_param(de, ae, bind);
            }
            _ => {}
        }
    }

    /// substitute bound type parameters inside a declared type
    fn resolve_t(&self, t: &Type, bind: &HashMap<String, Type>) -> Type {
        match t {
            Type::Struct(tp) => match bind.get(tp) {
                Some(v) => v.clone(),
                None => t.clone(),
            },
            Type::Array { elem, len } => {
                Type::Array { elem: Box::new(self.resolve_t(elem, bind)), len: *len }
            }
            other => other.clone(),
        }
    }

    /// `console.log` value formatting: numbers go through __ts_num (JS
    /// number-to-string), everything else becomes a string for joining
    fn log_arg(&mut self, e: Expr) -> Expr {
        let pos = e.pos();
        let t = self.infer_type(&e);
        match t {
            Some(Type::Float) => {
                self.need_runtime = true;
                Expr::Call {
                    name: "__ts_num".to_string(),
                    args: vec![Arg { name: None, value: e }],
                    pos,
                    lit_id: 0,
                }
            }
            Some(Type::Str) => e,
            _ => Expr::Call {
                name: "str".to_string(),
                args: vec![Arg { name: None, value: e }],
                pos,
                lit_id: 0,
            },
        }
    }
}

/// fill omitted optional/default arguments at every call site (a post-pass:
/// hoisted calls may precede the declaration the defaults come from)
fn fill_block(b: &mut Block, opts: &HashMap<String, Vec<(usize, Expr)>>, totals: &HashMap<String, Vec<Type>>) {
    for st in b.stmts.iter_mut() {
        fill_stmt(st, opts, totals);
    }
}

fn fill_stmt(st: &mut Stmt, opts: &HashMap<String, Vec<(usize, Expr)>>, totals: &HashMap<String, Vec<Type>>) {
    match st {
        Stmt::Let { expr, .. } | Stmt::Assign { expr, .. } | Stmt::Return { expr: Some(expr), .. } => {
            fill_expr(expr, opts, totals)
        }
        Stmt::Return { expr: None, .. } | Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Raise { .. } | Stmt::Try { .. } => {}
        Stmt::ExprStmt { expr } => fill_expr(expr, opts, totals),
        Stmt::Pass => {}
        Stmt::If { cond, then_block, else_block, .. } => {
            fill_expr(cond, opts, totals);
            fill_block(then_block, opts, totals);
            if let Some(e) = else_block {
                fill_block(e, opts, totals);
            }
        }
        Stmt::While { cond, body, .. } => {
            fill_expr(cond, opts, totals);
            fill_block(body, opts, totals);
        }
        Stmt::For { iter, body, .. } => {
            match iter {
                ForIter::Range(args) => {
                    for a in args {
                        fill_expr(a, opts, totals);
                    }
                }
                ForIter::Array(a) | ForIter::Dict(a) => fill_expr(a, opts, totals),
            }
            fill_block(body, opts, totals);
        }
    }
}

fn fill_expr(e: &mut Expr, opts: &HashMap<String, Vec<(usize, Expr)>>, totals: &HashMap<String, Vec<Type>>) {
    match e {
        Expr::Call { name, args, .. } => {
            for a in args.iter_mut() {
                fill_expr(&mut a.value, opts, totals);
            }
            if let Some(fills) = opts.get(name) {
                let total = totals.get(name).map(|t| t.len()).unwrap_or(0);
                while args.len() < total {
                    let idx = args.len();
                    match fills.iter().find(|(i, _)| *i == idx) {
                        Some((_, d)) => args.push(Arg { name: None, value: d.clone() }),
                        None => break, // a missing required arg: typecheck reports it
                    }
                }
            }
        }
        Expr::Unary { expr, .. } => fill_expr(expr, opts, totals),
        Expr::Binary { lhs, rhs, .. } => {
            fill_expr(lhs, opts, totals);
            fill_expr(rhs, opts, totals);
        }
        Expr::Cast { expr, .. } => fill_expr(expr, opts, totals),
        Expr::Index { arr, idx, .. } => {
            fill_expr(arr, opts, totals);
            fill_expr(idx, opts, totals);
        }
        Expr::Field { obj, .. } => fill_expr(obj, opts, totals),
        Expr::ArrayLit { elems, .. } => {
            for x in elems.iter_mut() {
                fill_expr(x, opts, totals);
            }
        }
        Expr::ArrayRep { elem, .. } => fill_expr(elem, opts, totals),
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields.iter_mut() {
                fill_expr(v, opts, totals);
            }
        }
        Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) | Expr::NoneLit(_) | Expr::DictLit { .. } | Expr::Var { .. } => {}
    }
}

enum NumLit {
    Int(i64),
    Float(f64),
}

/// decode a TS number literal (radix prefixes, `_` separators)
/// Is this module specifier a CSS Modules file? Only the `*.module.css`
/// suffix scopes class names, so a plain `.css` import gets no binding — its
/// text is the global bundle either way.
fn is_css_module_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".module.css")
}

/// The accessor `src/assets.rs` generates for this module specifier:
/// `./page.module.css` -> `page_module`.
///
/// This MUST stay identical to `assets::inject_all`'s naming, or the binding
/// would point at a function the compiler never emitted. The rule is the file
/// stem minus `.module`, with any character that is not alphanumeric or `_`
/// replaced by `_` — the same rule `assets::module_stem` applies. A stem that
/// collides across two directories is disambiguated there (`page_module_2`);
/// a collision therefore binds to the first module, which is why the loader
/// reports it rather than silently picking one.
fn css_module_accessor(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let raw = file.strip_suffix(".module.css").or_else(|| file.strip_suffix(".MODULE.CSS")).unwrap_or(file);
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        out.insert(0, '_');
    }
    format!("{out}_module")
}

fn parse_number(raw: &str) -> Option<NumLit> {
    let clean: String = raw.chars().filter(|c| *c != '_').collect();
    if let Some(rest) = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")) {
        return i64::from_str_radix(rest, 16).ok().map(NumLit::Int);
    }
    if let Some(rest) = clean.strip_prefix("0b").or_else(|| clean.strip_prefix("0B")) {
        return i64::from_str_radix(rest, 2).ok().map(NumLit::Int);
    }
    if let Some(rest) = clean.strip_prefix("0o").or_else(|| clean.strip_prefix("0O")) {
        return i64::from_str_radix(rest, 8).ok().map(NumLit::Int);
    }
    if clean.contains('.') || clean.contains('e') || clean.contains('E') {
        clean.parse::<f64>().ok().map(NumLit::Float)
    } else {
        clean.parse::<i64>().ok().map(NumLit::Int)
    }
}
