//! `Aoxn symbols <file.ax> [--json]` — the language service export.
//!
//! An editor needs to know what a program *declares*: the outline of a file,
//! where a function is defined so "go to definition" can jump to it, and
//! what the workspace contains so a symbol search has something to search.
//! All three are answers the compiler already knows exactly, because it has
//! just parsed the whole program — including every transitive import.
//!
//! The point of asking the COMPILER rather than guessing in the editor is
//! that a regex over source text cannot know where a declaration ends, which
//! name is a parameter and which is a type, or whether a function is
//! `extern` (declared in C) or Aoxn-defined. This module walks the real AST
//! [`crate::ast::Program`] that [`crate::load_program`] produced, so the
//! answer is the language's own, not a heuristic's.
//!
//! Scope is deliberately TOP-LEVEL DECLARATIONS ONLY. Locals live inside a
//! function body and are meaningless outside its scope; an editor asking
//! "where is this defined" wants the declarations a reader can see. That is
//! also what keeps the output small enough to re-derive on every keystroke
//! in a file without lag.
//!
//! Zero external dependencies, like the rest of `src/` — see SECURITY.md.
//!
//! ```text
//! Aoxn symbols <file.ax> [--json]
//!
//! human: one line per declaration
//!     function  fib        def fib(n: int) -> int          src/main.ax:7:5
//!     struct     Point     struct Point { x: int, … }      src/shapes.ax:3:1
//!
//! json:   {"symbols":[{"kind":"function","name":"fib","signature":"def fib(n: int) -> int",
//!                       "file":"src/main.ax","line":7,"column":5,"end_line":11,
//!                       "params":[{"name":"n","type":"int"}],"ret":"int","extern":false}, …]}
//! ```

use crate::ast::{Program, StructDecl, Type};
use crate::files;

/// One top-level declaration, as the editor sees it.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: &'static str,
    pub name: String,
    /// Source text of the declaration's signature, e.g. `def f(a: int) -> int`.
    /// Ready to show in a hover or an outline row.
    pub signature: String,
    /// The file the declaration lives in — an IMPORT's symbol reports the
    /// imported file, which is what makes cross-file navigation work.
    pub file: String,
    pub line: usize,
    pub column: usize,
    /// Last line of the declaration, so an outline can fold the range and a
    /// "go to definition" can select the whole thing. Equals `line` for a
    /// one-liner (`extern def`) and for a `struct` header.
    pub end_line: usize,
    /// Parameters with their types, for callers that want to render a call
    /// hint. Empty for structs.
    pub params: Vec<(String, String)>,
    /// Declared return type as a type name, INCLUDING `"void"` for a
    /// function that returns nothing. This differs from `signature` on
    /// purpose: the signature reproduces how Aoxn source spells a
    /// declaration (no arrow when there is no return type), while this is
    /// the type itself, for a caller that has to decide something about it.
    pub ret: String,
    /// True for `extern def`: declared here, implemented in C. An outline
    /// that cannot tell the two apart sends "go to definition" into a body
    /// that is not there.
    pub is_extern: bool,
    /// Struct fields, for a hover on a field. Empty for functions.
    pub fields: Vec<(String, String)>,
}

impl Symbol {
    fn to_json(&self) -> String {
        let items = |pairs: &[(String, String)]| -> String {
            pairs
                .iter()
                .map(|(n, t)| format!(
                    "{{\"name\":\"{}\",\"type\":\"{}\"}}",
                    esc(n),
                    esc(t)
                ))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            "{{\"kind\":\"{}\",\"name\":\"{}\",\"signature\":\"{}\",\"file\":\"{}\",\
             \"line\":{},\"column\":{},\"end_line\":{},\"ret\":\"{}\",\"extern\":{},\
             \"params\":[{}],\"fields\":[{}]}}",
            self.kind,
            esc(&self.name),
            esc(&self.signature),
            esc(&self.file),
            self.line,
            self.column,
            self.end_line,
            esc(&self.ret),
            self.is_extern,
            items(&self.params),
            items(&self.fields),
        )
    }
}

/// JSON-escape a string. Same rules as `Diag::to_json`: the control
/// characters below 0x20 are escaped numerically so the output is always
/// parseable, whatever a source file contained.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Every top-level declaration in a loaded program, in source order.
///
/// The program has already had its imports resolved and spliced, so this
/// returns the declarations of the entry file AND of every module it pulls
/// in — which is the entire point: "where is `fib` defined?" has to be
/// answerable when `fib` lives in an imported file.
pub fn collect(program: &Program) -> Vec<Symbol> {
    let mut out = Vec::new();
    for s in &program.structs {
        out.push(struct_symbol(s));
    }
    for f in &program.funcs {
        out.push(func_symbol(f));
    }
    out
}

fn struct_symbol(s: &StructDecl) -> Symbol {
    let fields: Vec<(String, String)> = s
        .fields
        .iter()
        .map(|p| (p.name.clone(), p.ty.to_string()))
        .collect();
    let rendered = fields
        .iter()
        .map(|(n, t)| format!("{n}: {t}"))
        .collect::<Vec<_>>()
        .join(", ");
    Symbol {
        kind: "struct",
        name: s.name.clone(),
        signature: if rendered.is_empty() {
            format!("struct {}", s.name)
        } else {
            format!("struct {} {{ {} }}", s.name, rendered)
        },
        file: files::name(s.pos.file),
        line: s.pos.line,
        column: s.pos.col,
        end_line: s.pos.line,
        params: Vec::new(),
        ret: String::new(),
        is_extern: false,
        fields,
    }
}

fn func_symbol(f: &crate::ast::FnDecl) -> Symbol {
    let params: Vec<(String, String)> = f
        .params
        .iter()
        .map(|p| (p.name.clone(), p.ty.to_string()))
        .collect();
    let rendered = params
        .iter()
        .map(|(n, t)| format!("{n}: {t}"))
        .collect::<Vec<_>>()
        .join(", ");
    // Generic declarations carry their type parameters in the signature —
    // `def sort[T, N](arr: [T; N]) -> [T; N]` — because dropping them would
    // render two different overloads as the same text.
    let generics = if f.type_params.is_empty() {
        String::new()
    } else {
        format!("[{}]", f.type_params.join(", "))
    };
    // `void` is the type a `def` without an arrow gets, and it is omitted
    // from the rendered signature: Aoxn source spells it by leaving the
    // arrow off, and a hover that said `-> void` would read as if the
    // author had written it. Every other return type is shown, extern
    // included — an `extern def` that returns `int` is exactly the case
    // where a caller needs to know the shape.
    let returns = match f.ret {
        Type::Void => String::new(),
        ref other => format!(" -> {other}"),
    };
    let signature = if f.is_extern {
        format!("extern def {}{generics}({rendered}){returns}", f.name)
    } else {
        format!("def {}{generics}({rendered}){returns}", f.name)
    };
    Symbol {
        kind: "function",
        name: f.name.clone(),
        signature,
        file: files::name(f.pos.file),
        line: f.pos.line,
        column: f.pos.col,
        // The body spans to its last statement; an `extern def` has no body
        // and is a single line. `Block` carries no end position, so the
        // last statement's own position is the closest honest answer.
        end_line: body_end(&f.body).unwrap_or(f.pos.line).max(f.pos.line),
        params,
        ret: f.ret.to_string(),
        is_extern: f.is_extern,
        fields: Vec::new(),
    }
}

/// Last source line occupied by a block, if it has any statements.
///
/// A function whose body is `pass` still occupies its `def` line, and one
/// that is empty occupies nothing — hence the `Option`, resolved by the
/// caller against the `def` line.
fn body_end(block: &crate::ast::Block) -> Option<usize> {
    block.stmts.iter().map(stmt_end).max()
}

fn stmt_end(s: &crate::ast::Stmt) -> usize {
    use crate::ast::Stmt;
    match s {
        Stmt::Let { pos, .. }
        | Stmt::Assign { pos, .. }
        | Stmt::If { pos, .. }
        | Stmt::While { pos, .. }
        | Stmt::For { pos, .. }
        | Stmt::Break { pos }
        | Stmt::Continue { pos }
        | Stmt::Raise { pos, .. }
        | Stmt::Try { pos, .. }
        | Stmt::Return { pos, .. } => pos.line,
        // These carry no position of their own; they have no line to report
        // and the caller falls back to the previous statement's.
        Stmt::Pass | Stmt::ExprStmt { .. } => 0,
    }
}

/// The whole symbol table as one JSON document.
pub fn to_json(symbols: &[Symbol]) -> String {
    let items = symbols
        .iter()
        .map(Symbol::to_json)
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"symbols\":[{items}]}}")
}

/// The human form: one aligned line per declaration.
pub fn to_text(symbols: &[Symbol]) -> String {
    if symbols.is_empty() {
        return "no top-level declarations\n".to_string();
    }
    let kindw = symbols.iter().map(|s| s.kind.len()).max().unwrap_or(0);
    let namew = symbols.iter().map(|s| s.name.len()).max().unwrap_or(0);
    let mut out = String::new();
    for s in symbols {
        out.push_str(&format!(
            "{:<kindw$}  {:<namew$}  {:<38}  {}:{}:{}\n",
            s.kind,
            s.name,
            s.signature,
            s.file,
            s.line,
            s.column,
            kindw = kindw,
            namew = namew,
        ));
    }
    out
}

/// Render a type the way a signature should read it. Thin wrapper kept so
/// the display rule lives in one place: an array length that is still the
/// generic sentinel prints as `N` (which is what `Type`'s own `Display`
/// already does), and nothing else needs to know.
pub fn type_name(t: &Type) -> String {
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program_of(src: &str) -> Program {
        // The tests here go through the real loader so the file registry is
        // populated exactly as a compile would populate it — a symbol with
        // no file name is worse than no symbol at all.
        crate::parse_sources(&[src.to_string()]).expect("parse")
    }

    fn syms(src: &str) -> Vec<Symbol> {
        collect(&program_of(src))
    }

    /// Write a real file (with its imports) and run the public entry point,
    /// so the file-name and cross-file behaviour is tested as a caller
    /// experiences it rather than through a test-only shortcut.
    fn syms_on_disk(files: &[(&str, &str)], entry: &str) -> Vec<Symbol> {
        // The directory is per CALL, not per entry name: tests run in
        // parallel threads, and two of them naming their entry `main.ax`
        // would otherwise share a directory and overwrite each other's
        // fixtures mid-run.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aoxn-symbols-{}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            entry.replace(['/', '\\'], "_")
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, text) in files {
            let real = dir.join(path);
            if let Some(parent) = real.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(real, text).unwrap();
        }
        let paths = vec![dir.join(entry).to_string_lossy().into_owned()];
        let out = crate::program_symbols(&paths).expect("symbols");
        let _ = std::fs::remove_dir_all(&dir);
        out
    }

    #[test]
    fn a_function_symbol_carries_its_signature_and_position() {
        let s = syms("def fib(n: int) -> int:\n    if n < 2:\n        return n\n    return n\n");
        let f = s.iter().find(|s| s.name == "fib").expect("fib");
        assert_eq!(f.kind, "function");
        assert_eq!(f.signature, "def fib(n: int) -> int");
        assert_eq!(f.line, 1);
        // The AST records the `def` KEYWORD's position, not the name's
        // (`def` starts at column 1). That is the right anchor for a
        // "go to definition" that selects the whole declaration, and it is
        // what the parser records — a symbol table that disagreed with the
        // AST about where a thing is would be its own bug.
        assert_eq!(f.column, 1);
        assert_eq!(f.ret, "int");
        assert!(!f.is_extern);
        assert_eq!(f.params, vec![("n".to_string(), "int".to_string())]);
        assert_eq!(f.end_line, 4, "the body runs to its last statement");
    }

    #[test]
    fn a_function_with_no_return_type_renders_without_an_arrow() {
        // `def f():` has type `void`, which the parser gets from the
        // missing arrow. The signature must not invent `-> void` — the
        // whole point is to reproduce how the source reads.
        let s = syms("def log_it(m: string):\n    print(m)\n");
        assert_eq!(s[0].signature, "def log_it(m: string)");
        assert_eq!(s[0].ret, "void", "but the machine field still says void");
    }

    #[test]
    fn a_struct_symbol_lists_its_fields() {
        let s = syms("struct Point:\n    x: int\n    y: int\n\ndef main() -> int:\n    return 0\n");
        let st = s.iter().find(|s| s.kind == "struct").expect("struct");
        assert_eq!(st.name, "Point");
        assert_eq!(st.signature, "struct Point { x: int, y: int }");
        assert_eq!(st.line, 1);
        assert_eq!(st.fields.len(), 2);
        assert_eq!(st.fields[1], ("y".to_string(), "int".to_string()));
    }

    #[test]
    fn extern_declarations_are_marked_and_have_no_body() {
        let s = syms("extern def GetTickCount() -> int\n");
        let f = &s[0];
        assert!(f.is_extern);
        // The return type is shown even though the body is not: a caller
        // reading this outline needs to know the shape of the call.
        assert_eq!(f.signature, "extern def GetTickCount() -> int");
        // No body, so the range is the declaration line itself.
        assert_eq!(f.end_line, f.line);
    }

    #[test]
    fn generic_parameters_stay_in_the_signature() {
        let s = syms("def first[T, N](a: [T; N]) -> T:\n    return a[0]\n");
        let f = &s[0];
        assert!(f.signature.contains("[T, N]"), "{}", f.signature);
    }

    #[test]
    fn symbols_carry_the_importing_files_names() {
        let s = syms("def main() -> int:\n    return 0\n");
        assert!(!s[0].file.is_empty());
        assert!(!s[0].file.contains('?'), "unresolved file: {:?}", s[0].file);
    }

    #[test]
    fn an_imported_file_contributes_its_own_symbols() {
        // The whole reason the export exists: "go to definition" for a name
        // that lives in another file. If imports were not followed, an
        // outline of `main.ax` would be missing everything it uses.
        let s = syms_on_disk(
            &[
                ("util.ax", "def clamp_i(v: int) -> int:\n    return v\n"),
                ("main.ax", "import * from \"./util.ax\"\n\ndef main() -> int:\n    return 0\n"),
            ],
            "main.ax",
        );
        let clamp = s.iter().find(|s| s.name == "clamp_i").expect("imported fn");
        assert!(clamp.file.ends_with("util.ax"), "{}", clamp.file);
        assert_eq!(clamp.line, 1);
        let main = s.iter().find(|s| s.name == "main").expect("entry fn");
        assert!(main.file.ends_with("main.ax"), "{}", main.file);
    }

    #[test]
    fn a_namespace_imported_module_is_outlined_under_its_own_names() {
        // `import util` makes the resolver prefix the module's declarations so
        // nothing can collide with them. The outline must not show those
        // prefixes: an editor listing `util_button` would send the author
        // looking for a name the file does not contain (v0.43.0).
        let s = syms_on_disk(
            &[
                ("util.ax", "def button() -> int:\n    return 1\n\nstruct Point:\n    x: int\n"),
                ("main.ax", "import util\n\ndef main() -> int:\n    return util.button()\n"),
            ],
            "main.ax",
        );
        let button = s.iter().find(|s| s.name == "button").expect("the source spelling");
        assert!(button.file.ends_with("util.ax"), "{}", button.file);
        let point = s.iter().find(|s| s.name == "Point").expect("the source spelling");
        assert!(point.file.ends_with("util.ax"), "{}", point.file);
        assert!(s.iter().all(|s| !s.name.contains("util_")), "{s:?}");
    }

    #[test]
    fn a_broken_file_reports_diagnostics_instead_of_a_half_outline() {
        // An editor that showed a truncated outline for a file that does not
        // parse would send the user to a declaration that is not there.
        // This sample lexes cleanly and fails in the parser (a `class`
        // declaration, which is not Aoxn — `class` is a perfectly good
        // identifier, so only the parser can reject it). That is the
        // interesting case: the failure is past the point where a
        // regex-over-text outline would have stopped caring.
        let dir = std::env::temp_dir().join(format!("aoxn-symbols-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.ax");
        std::fs::write(&path, "def ok() -> int:\n    return 0\n\nclass Nope:\n    pass\n").unwrap();
        let paths = vec![path.to_string_lossy().into_owned()];
        let err = crate::program_symbols(&paths).expect_err("must fail");
        assert!(!err.is_empty());
        assert_eq!(err[0].stage, "parse", "unexpected stage: {}", err[0].stage);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_is_parseable_and_escapes() {
        let s = syms("struct Q:\n    name: string\n");
        let json = to_json(&s);
        assert!(json.starts_with('{') && json.ends_with('}'));
        assert!(json.contains("\"kind\":\"struct\""));
        assert!(json.contains("\"name\":\"Q\""));
        assert!(json.contains("\"fields\":[{\"name\":\"name\",\"type\":\"string\"}]"));
        // No raw newline may leak into the JSON text.
        assert!(!json.contains('\n'));
    }

    #[test]
    fn the_text_form_names_every_declaration() {
        let s = syms("def a() -> int:\n    return 0\n\nstruct B:\n    x: int\n");
        let text = to_text(&s);
        assert!(text.contains("function"), "{text}");
        assert!(text.contains("struct"), "{text}");
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn an_empty_program_says_so_rather_than_printing_nothing() {
        assert!(to_text(&[]).contains("no top-level"));
        assert_eq!(to_json(&[]), "{\"symbols\":[]}");
    }
}
