//! Aoxn abstract syntax tree.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Int,
    Float,
    Bool,
    Str,
    Void,
    /// the type of the `None` literal (v0.40.0)
    None,
    /// `T | None` — a value that may be absent (v0.40.0). Carries a tag at
    /// runtime; `is None` / `is not None` narrow it to `None` or `T`.
    Opt(Box<Type>),
    /// `dict[V]` — a string-keyed map with values of type V (v0.40.0).
    /// Mutating a copy mutates the original: the backing buffers are shared.
    Dict(Box<Type>),
    Array { elem: Box<Type>, len: usize },
    Struct(String),
    /// A function type (v0.40.0): the address of a top-level `def`/`extern
    /// def` used as a value, or the type of a variable holding one. Written
    /// `fn(int, string) -> int` in diagnostics — the surface grammar has no
    /// `fn` type annotation yet, so the type is inferred, never spelled.
    FnPtr { ret: Box<Type>, params: Vec<Type> },
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Int => write!(f, "int"),
            Type::Float => write!(f, "float"),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "string"),
            Type::Void => write!(f, "void"),
            Type::None => write!(f, "None"),
            Type::Opt(inner) => write!(f, "{inner} | None"),
            Type::Dict(inner) => write!(f, "dict[{inner}]"),
            Type::Array { elem, len } if *len == GENERIC_LEN => write!(f, "[{elem}; N]"),
            Type::Array { elem, len } => write!(f, "[{elem}; {len}]"),
            Type::Struct(name) => write!(f, "{name}"),
            Type::FnPtr { ret, params } => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
        }
    }
}

impl Type {
    /// primitives that `print` accepts
    pub fn is_printable(&self) -> bool {
        matches!(self, Type::Int | Type::Float | Type::Bool | Type::Str)
    }

    pub fn is_compound(&self) -> bool {
        matches!(self, Type::Array { .. } | Type::Struct(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
    /// index into the compilation's file registry (u32::MAX = synthetic)
    pub file: u32,
}

#[derive(Debug)]
pub struct Program {
    pub imports: Vec<ImportDecl>,
    pub structs: Vec<StructDecl>,
    pub funcs: Vec<FnDecl>,
    /// CSS assets reached through `import`, bundled at load time and injected
    /// into `funcs` as the `styles()` accessors (see `crate::assets`).
    /// Empty for the string-based entry points, which cannot resolve imports.
    pub assets: crate::assets::AssetSet,
}

#[derive(Debug)]
pub struct ImportDecl {
    pub path: String,
    pub kind: ImportKind,
    /// the specifier was written in quotes (`"stdlib/net/json"`). A BARE
    /// Python-style specifier additionally probes the importing file's own
    /// directory, which is how `import util` finds the `util.ax` next to it;
    /// the quoted spelling keeps the package/stdlib resolution it has always
    /// had, untouched (v0.43.0).
    pub path_quoted: bool,
    pub pos: Pos,
}

/// What an import brings into the importing module (v0.43.0).
///
/// The Python spellings and the Aoxn-native ones lower to the same three
/// shapes, so a program written either way resolves identically:
///
/// | source                     | shape                       |
/// |----------------------------|-----------------------------|
/// | `from p import *`          | `Star`                      |
/// | `import * from "p"`        | `Star`                      |
/// | `import "p"`               | `Star` (revived in v0.43.0) |
/// | `from p import a, b as c`  | `Names`                     |
/// | `import { a, b } from "p"` | `Names`                     |
/// | `import d from "p"`        | `Names([d])`                |
/// | `import p` / `import p as q` | `Module`                  |
#[derive(Debug, Clone)]
pub enum ImportKind {
    /// every top-level name of the module, bound unqualified
    Star,
    /// only the listed names, each optionally renamed
    Names(Vec<ImportedName>),
    /// the module itself, reachable as `alias.member` (or `path.member` when
    /// unaliased). The binding is a namespace, not a value: using it as one is
    /// an error, exactly like a struct with no runtime representation.
    Module { alias: Option<String> },
}

#[derive(Debug, Clone)]
pub struct ImportedName {
    /// the name as the exporting module spells it
    pub name: String,
    /// the name it is bound to locally, when `as` renamed it
    pub alias: Option<String>,
}

impl ImportedName {
    /// the spelling the importing module uses.
    pub fn local(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

/// sentinel array length inside a generic declaration: `[T; N]` parses to
/// this; the monomorphizer substitutes the concrete length per instance.
pub const GENERIC_LEN: usize = usize::MAX;

#[derive(Debug)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<Param>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub name: String,
    /// type/length parameters: `def sort[T, N](arr: [T; N]) -> [T; N]`
    /// (empty for concrete functions; monomorphized at call sites)
    pub type_params: Vec<String>,
    /// which type param is the array length (usable as an int constant), if any
    pub len_param: Option<String>,
    pub params: Vec<Param>,
    pub ret: Type,
    pub body: Block,
    /// `extern def`: declared, body provided by the C runtime
    pub is_extern: bool,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum ForIter {
    /// `range(start, end, step)` — 1..3 int args
    Range(Vec<Expr>),
    /// iterate array elements (each copied into the loop variable)
    Array(Expr),
    /// iterate a dict's KEYS in insertion order (v0.40.0)
    Dict(Expr),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    /// `x = e` (inferred) or `x: t = e` (annotated). First use declares the
    /// variable; subsequent uses are re-assignments with the same type.
    Let {
        name: String,
        ty: Option<Type>,
        expr: Expr,
        pos: Pos,
    },
    /// assignment to a compound lvalue: `a[i] = e` / `p.f = e`
    Assign {
        target: Expr,
        expr: Expr,
        pos: Pos,
    },
    If {
        cond: Expr,
        then_block: Block,
        else_block: Option<Block>,
        pos: Pos,
    },
    While {
        cond: Expr,
        body: Block,
        pos: Pos,
    },
    For {
        var: String,
        iter: ForIter,
        body: Block,
        pos: Pos,
    },
    Break {
        pos: Pos,
    },
    Continue {
        pos: Pos,
    },
    Return {
        expr: Option<Expr>,
        pos: Pos,
    },
    /// `raise <string>` (v0.40.0): store the message in the error slot and
    /// unwind to the nearest enclosing `try` handler, or out of the function
    Raise {
        expr: Expr,
        pos: Pos,
    },
    /// `try: ... except as e: ...` (v0.40.0). One catch-all handler; the
    /// binding (when spelled) is a `string` holding the raised message.
    Try {
        body: Block,
        err_name: Option<String>,
        handler: Block,
        pos: Pos,
    },
    Pass,
    ExprStmt {
        expr: Expr,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Shl,
    Shr,
    BitAnd,
    BitOr,
    BitXor,
    Eq,
    Ne,
    /// `is` / `is not` (v0.40.0) — identity against `None`, which also drives
    /// the nullable narrowing. Outside `None` comparisons they mean `==`/`!=`.
    Is,
    IsNot,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64, Pos),
    Float(f64, Pos),
    Str(String, Pos),
    Bool(bool, Pos),
    /// the `None` literal (v0.40.0)
    NoneLit(Pos),
    Var { name: String, pos: Pos },
    Call { name: String, args: Vec<Arg>, pos: Pos, lit_id: usize },
    Unary { op: UnOp, expr: Box<Expr>, pos: Pos },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr>, pos: Pos },
    Index { arr: Box<Expr>, idx: Box<Expr>, pos: Pos },
    Field { obj: Box<Expr>, name: String, pos: Pos },
    ArrayLit { elems: Vec<Expr>, lit_id: usize, pos: Pos },
    /// `[elem] * N` — single-element array replication (Python-style)
    ArrayRep { elem: Box<Expr>, count: usize, lit_id: usize, pos: Pos },
    /// `{"k": v, ...}` — a dict literal (v0.40.0). Keys must be strings.
    DictLit { entries: Vec<(Expr, Expr)>, lit_id: usize, pos: Pos },
    StructLit { name: String, fields: Vec<(String, Expr)>, lit_id: usize, pos: Pos },
    /// explicit scalar conversion (int <-> float) — the TS front end lowers
    /// `as`/numeric-tower promotions here; the Aoxn surface syntax never
    /// produces it, so the self-hosted compiler never sees this node
    Cast { expr: Box<Expr>, to: Type, pos: Pos },
}

impl Expr {
    pub fn pos(&self) -> Pos {
        match self {
            Expr::Int(_, p) | Expr::Float(_, p) | Expr::Str(_, p) | Expr::Bool(_, p) => *p,
            Expr::NoneLit(p) => *p,
            Expr::Var { pos, .. }
            | Expr::Call { pos, .. }
            | Expr::Unary { pos, .. }
            | Expr::Binary { pos, .. }
            | Expr::Index { pos, .. }
            | Expr::Field { pos, .. }
            | Expr::ArrayLit { pos, .. }
            | Expr::ArrayRep { pos, .. }
            | Expr::DictLit { pos, .. }
            | Expr::StructLit { pos, .. }
            | Expr::Cast { pos, .. } => *pos,
        }
    }

    /// A valid assignment target: variable, index, or field access.
    pub fn is_lvalue(&self) -> bool {
        matches!(self, Expr::Var { .. } | Expr::Index { .. } | Expr::Field { .. })
    }
}

