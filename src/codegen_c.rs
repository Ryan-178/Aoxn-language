//! The C-emitting backend (the compiler's only backend since v0.29.0).
//!
//! Walks the typechecked AST and emits ISO C99 text, compiled to a native
//! object by the clang toolchain (first shipped as an experimental backend in
//! v0.27.1; the LLVM backend was removed in v0.29.0 — see
//! docs/llvm-independence-report.md). The mapping rests on two observations
//! from the LLVM backend's discipline:
//!
//! * Aggregates are already "addresses + explicit copies" there, which is
//!   exactly C's struct value semantics — so structs map to C structs and
//!   arrays are wrapped in single-field structs (`typedef struct { T data[N]; }`)
//!   to become C values (copy on assignment / param / return, like LLVM's
//!   pointer+sret ABI but spelled natively).
//! * The used LLVM subset (`nsw` arithmetic, `inbounds` GEP, string ops via
//!   the C runtime) has the same UB and the same runtime as plain C.
//!
//! No C standard header is included: the runtime surface the generated code
//! touches is expressed with clang `__builtin_*` forms, and every other C
//! function is declared in the same Aoxn-`extern` shape the LLVM backend uses
//! (`int` is 8 bytes; pointers arrive as `long long`). This keeps generated
//! declarations conflict-free with Aoxn programs that declare the same
//! functions themselves (e.g. stdlib's `malloc`).
//!
//! Known divergences from the removed LLVM backend (all documented in
//! docs/llvm-independence-report.md): C leaves evaluation order between
//! operands/arguments unspecified (Aoxn's spec does not pin it either), and
//! mangled generic names (`sort.i.8` -> `sort_i_8`) can theoretically collide
//! with a user function of the same spelling.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use crate::ast::*;
use crate::hashing::FastBuild;

type Locals = HashMap<String, Type, FastBuild>;

/// C identifiers that must not be emitted as-is: keywords, the generated
/// entry point, and the C runtime names the backend may reference. User
/// names hitting this list (or starting with the generator's `ax_`/`aoxn_`
/// prefixes) get a `_ax` suffix.
const C_KEYWORDS: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else",
    "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long", "register",
    "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch", "typedef",
    "union", "unsigned", "void", "volatile", "while", "_Bool", "_Complex", "_Imaginary",
];

/// C runtime functions the backend itself may call (always reserved so user
/// locals can never shadow them), mirroring `is_len_safe_extern` + the raw
/// names stdlib declares.
const RUNTIME_NAMES: &[&str] = &[
    "malloc", "free", "realloc", "memcpy", "memset", "strlen", "strcmp", "strncmp", "snprintf",
    "printf", "puts", "_setmode", "fopen", "fread", "fwrite", "fseek", "ftell", "fclose",
    "system",
];

struct FnSig {
    c_name: String,
    ret: Type,
    /// parameter types, kept so a function used as a VALUE can be typed
    /// (`Type::FnPtr`) without re-reading the AST (v0.40.0)
    params: Vec<Type>,
}

/// Compile the typechecked program to a native object file through C text +
/// `clang -c`. The generated C text is written next to the object (kept for
/// debugging when the compile fails) and removed on success.
pub fn generate_to_object(
    program: &Program,
    obj_path: &Path,
    opt_level: u8,
    call_map: &HashMap<usize, String>,
) -> Result<(), String> {
    let text = generate_c_text(program, call_map)?;
    if std::env::var("AOXN_DUMP_C").is_ok() {
        eprintln!("{text}");
    }
    let c_path = obj_path.with_extension("c");
    std::fs::write(&c_path, &text)
        .map_err(|e| format!("internal error: cannot write {}: {e}", c_path.display()))?;

    let clang = crate::find_clang()
        .ok_or_else(|| "internal error: cannot find clang for the C backend".to_string())?;
    let mut cmd = std::process::Command::new(&clang);
    cmd.arg(format!("-O{}", opt_level.min(3))).arg("-w");
    // mirror the LLVM backend's AOXN_CPU handling; clang spells it -march on
    // x86 and -mcpu elsewhere, and unlike the LLVM-C API it does accept
    // "native" (the v0.26.3 C-API limitation does not apply here)
    if let Ok(cpu) = std::env::var("AOXN_CPU") {
        if !cpu.is_empty() {
            if cfg!(target_arch = "x86_64") || cfg!(target_arch = "x86") {
                cmd.arg(format!("-march={cpu}"));
            } else {
                cmd.arg(format!("-mcpu={cpu}"));
            }
        }
    }
    cmd.arg("-c").arg(&c_path).arg("-o").arg(obj_path);
    let out = cmd
        .output()
        .map_err(|e| format!("internal error: failed to spawn {}: {e}", clang.display()))?;
    if !out.status.success() {
        // keep the generated C around for debugging
        let stderr = String::from_utf8_lossy(&out.stderr);
        let lines: Vec<&str> = stderr.lines().collect();
        let start = lines.len().saturating_sub(12);
        return Err(format!(
            "internal error: C backend clang failed with exit code {:?}; generated C kept at {}\n{}",
            out.status.code(),
            c_path.display(),
            lines[start..].join("\n")
        ));
    }
    let _ = std::fs::remove_file(&c_path);
    Ok(())
}

pub fn generate_c_text(program: &Program, call_map: &HashMap<usize, String>) -> Result<String, String> {
    let mut g = GenC::new(program, call_map)?;
    let mut protos: Vec<String> = Vec::new();
    let mut bodies: Vec<String> = Vec::new();
    for f in &program.funcs {
        if f.type_params.is_empty() && !f.is_extern {
            let (proto, body) = g.gen_fn(f)?;
            protos.push(proto);
            bodies.push(body);
        }
    }
    g.assemble(protos, bodies)
}

struct GenC<'a> {
    call_map: &'a HashMap<usize, String>,
    /// every concrete function (user + monomorphized instances + externs)
    sigs: HashMap<String, FnSig, FastBuild>,
    struct_fields: HashMap<String, Vec<(String, Type)>, FastBuild>,
    struct_names: HashSet<String>,
    /// array wrapper typedefs: name -> (elem, len)
    typedefs: BTreeMap<String, (Type, usize)>,
    /// `T | None` carrier structs: name -> T (v0.40.0). Registered lazily by
    /// `c_type`, emitted with the rest of the type graph in `assemble`
    opt_structs: BTreeMap<String, Type>,
    /// user `extern def` declarations, deduped by C name
    extern_decls: BTreeMap<String, String>,
    /// per-replication-site fill helper: node address -> rendered text
    rep_helpers: BTreeMap<usize, String>,
    reserved: HashSet<String>,
    /// variables narrowed by `is None` (v0.40.0): name -> the type the CURRENT
    /// branch sees. Latched while a branch is emitted, popped after it.
    narrowed: HashMap<String, Type>,
    /// return type of the function being emitted (for nullable coercion)
    cur_ret: Type,
    // per-function emission state
    out: String,
    decls: Vec<String>,
    locals: Locals,
    tmpn: usize,
    // usage flags for the optional runtime helper groups
    used_concat: bool,
    used_str_i: bool,
    used_str_f: bool,
    used_raw: bool,
}

impl<'a> GenC<'a> {
    fn new(program: &Program, call_map: &'a HashMap<usize, String>) -> Result<GenC<'a>, String> {
        let mut g = GenC {
            call_map,
            sigs: HashMap::default(),
            struct_fields: HashMap::default(),
            struct_names: HashSet::new(),
            typedefs: BTreeMap::new(),
            opt_structs: BTreeMap::new(),
            extern_decls: BTreeMap::new(),
            rep_helpers: BTreeMap::new(),
            narrowed: HashMap::default(),
            cur_ret: Type::Void,
            reserved: HashSet::new(),
            out: String::new(),
            decls: Vec::new(),
            locals: HashMap::default(),
            tmpn: 0,
            used_concat: false,
            used_str_i: false,
            used_str_f: false,
            used_raw: false,
        };
        for s in &program.structs {
            g.struct_names.insert(s.name.clone());
            let fields: Vec<(String, Type)> = s.fields.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
            g.struct_fields.insert(s.name.clone(), fields);
        }
        // reserved must be complete before any c_ident call renders a name
        for k in C_KEYWORDS {
            g.reserved.insert((*k).to_string());
        }
        for k in RUNTIME_NAMES {
            g.reserved.insert((*k).to_string());
        }
        g.reserved.insert("main".to_string());
        g.reserved.insert("aoxn_main".to_string());
        for f in &program.funcs {
            if f.is_extern {
                g.reserved.insert(f.name.clone());
            }
        }
        for f in &program.funcs {
            if !f.type_params.is_empty() {
                continue; // generic declarations never reach codegen
            }
            if f.is_extern {
                let decl = g.extern_decl_text(f)?;
                if let Some(prev) = g.extern_decls.get(&f.name) {
                    if prev != &decl {
                        return Err(format!(
                            "internal error: conflicting extern declarations for '{}'",
                            f.name
                        ));
                    }
                    continue;
                }
                g.extern_decls.insert(f.name.clone(), decl);
            }
            // externs keep their original spelling: that IS the C symbol they
            // link against. `reserved` still contains their names, so user
            // locals shadowing them get renamed instead (e.g. `system`).
            let c_name = if f.is_extern {
                f.name.clone()
            } else if f.name == "main" {
                "aoxn_main".to_string()
            } else {
                g.c_ident(&f.name)
            };
            g.sigs.insert(f.name.clone(), FnSig { c_name, ret: f.ret.clone(), params: f.params.iter().map(|p| p.ty.clone()).collect() });
        }
        // struct field types register their array typedefs up front so the
        // type graph is complete for the topological assembly
        let field_tys: Vec<Type> = g
            .struct_fields
            .values()
            .flat_map(|f| f.iter().map(|(_, t)| t.clone()))
            .collect();
        for ty in &field_tys {
            g.register_type(ty);
        }
        Ok(g)
    }

    // ---- naming / types ----

    /// Render an Aoxn identifier as a C identifier. Names that would collide
    /// with C keywords, the runtime surface, the entry point, or the
    /// generator's own `ax_`/`aoxn_` prefixes get a `_ax` suffix; generic
    /// instance names (`sort.i.8`) are flattened to `sort_i_8`.
    fn c_ident(&self, name: &str) -> String {
        let mut out: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
            .collect();
        if out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            out.insert(0, '_');
        }
        if self.reserved.contains(&out) || out.starts_with("ax_") || out.starts_with("aoxn_") {
            out.push_str("_ax");
        }
        out
    }

    fn type_token(t: &Type) -> String {
        match t {
            Type::Int | Type::Bool => "i".to_string(), // both are C `int`
            Type::Float => "f".to_string(),
            Type::Str => "s".to_string(),
            Type::Void => "v".to_string(),
            Type::None => "z".to_string(),
            Type::Opt(inner) => format!("o{}", Self::type_token(inner)),
            Type::Struct(n) => n.clone(),
            Type::Array { elem, len } => format!("a{}_{}", Self::type_token(elem), len),
            // the signature is part of the token: two arrays of differently
            // typed function pointers must not share a typedef
            Type::FnPtr { ret, params } => {
                let mut tok = format!("fn{}", Self::type_token(ret));
                for p in params {
                    tok.push('_');
                    tok.push_str(&Self::type_token(p));
                }
                tok
            }
        }
    }

    fn typedef_name(elem: &Type, len: usize) -> String {
        format!("ax_a_{}_{}", Self::type_token(elem), len)
    }

    fn register_type(&mut self, t: &Type) {
        if let Type::Array { elem, len } = t {
            if *len == GENERIC_LEN {
                return; // unreachable post-monomorphization
            }
            let name = Self::typedef_name(elem, *len);
            if !self.typedefs.contains_key(&name) {
                self.typedefs.insert(name, ((**elem).clone(), *len));
                self.register_type(elem);
            }
        }
    }

    /// C type token for a value position; array typedefs are registered here.
    fn c_type(&mut self, t: &Type) -> String {
        match t {
            Type::Int => "long long".to_string(),
            Type::Float => "double".to_string(),
            Type::Bool => "int".to_string(),
            Type::Str => "char*".to_string(),
            Type::Void => "void".to_string(),
            // a None VALUE is just the integer 0 on the wire; the tag lives in
            // the Opt struct that holds it (v0.40.0)
            Type::None => "long long".to_string(),
            Type::Opt(inner) => {
                let name = format!("ax_opt_{}", Self::type_token(inner));
                self.opt_structs.insert(name.clone(), (**inner).clone());
                format!("struct {name}")
            }
            Type::Struct(n) => format!("struct {}", self.c_ident(n)),
            Type::FnPtr { ret, params } => {
                // a function-pointer TYPE is spelled with the parenthesised
                // declarator, so `c_decl` renders `long long (*)(long long) cb`
                let ps: Vec<String> = params.iter().map(|p| self.c_type(p)).collect();
                let args = if ps.is_empty() { "void".to_string() } else { ps.join(", ") };
                format!("{} (*)({})", self.c_type(ret), args)
            }
            Type::Array { elem, len } => {
                if *len == GENERIC_LEN {
                    return "void*".to_string(); // unreachable post-monomorphization
                }
                let name = Self::typedef_name(elem, *len);
                if !self.typedefs.contains_key(&name) {
                    self.typedefs.insert(name.clone(), ((**elem).clone(), *len));
                    let elem = (**elem).clone();
                    self.register_type(&elem);
                }
                name
            }
        }
    }

    /// `<type> <ident>` declaration text
    fn c_decl(&mut self, t: &Type, ident: &str) -> String {
        // a function pointer needs the parenthesised NAME inside the
        // declarator: `long long (*cb)(long long)`, not `long long (*)(…) cb`
        if let Type::FnPtr { ret, params } = t {
            let ps: Vec<String> = params.iter().map(|p| self.c_type(p)).collect();
            let args = if ps.is_empty() { "void".to_string() } else { ps.join(", ") };
            return format!("{} (*{})({})", self.c_type(ret), ident, args);
        }
        format!("{} {}", self.c_type(t), ident)
    }

    fn extern_decl_text(&mut self, f: &FnDecl) -> Result<String, String> {
        let mut params: Vec<String> = f.params.iter().map(|p| self.c_decl(&p.ty, &self.c_ident(&p.name))).collect();
        if params.is_empty() {
            params.push("void".to_string());
        }
        Ok(format!("{} {}({});", self.c_type(&f.ret), f.name, params.join(", ")))
    }

    fn c_string_lit(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for b in s.bytes() {
            match b {
                b'"' => out.push_str("\\\""),
                b'\\' => out.push_str("\\\\"),
                b'\n' => out.push_str("\\n"),
                b'\t' => out.push_str("\\t"),
                b'\r' => out.push_str("\\r"),
                0x20..=0x7e => out.push(b as char),
                // 3-digit octal escapes are unambiguous even when the next
                // byte is a digit (unlike \x)
                other => out.push_str(&format!("\\{other:03o}")),
            }
        }
        out.push('"');
        out
    }

    fn float_lit(v: f64) -> String {
        let s = format!("{v}");
        if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("nan") {
            s
        } else {
            format!("{s}.0")
        }
    }

    fn fresh_tmp(&mut self) -> String {
        self.tmpn += 1;
        format!("ax_t{}", self.tmpn)
    }

    // ---- module assembly ----

    fn assemble(mut self, protos: Vec<String>, bodies: Vec<String>) -> Result<String, String> {
        if !self.sigs.contains_key("main") {
            return Err("internal error: main missing at codegen".into());
        }
        let main_ret = self.sigs["main"].ret.clone();
        let mut s = String::new();
        s.push_str("/* Generated by the Aoxn C backend. Do not edit.\n");
        s.push_str("   No C standard headers: the runtime surface uses __builtin_* forms\n");
        s.push_str("   and every other C function is an Aoxn `extern def` declaration. */\n\n");

        for decl in self.extern_decls.values() {
            s.push_str(decl);
            s.push('\n');
        }
        if !self.extern_decls.is_empty() {
            s.push('\n');
        }

        s.push_str(&self.type_defs_text()?);
        s.push('\n');

        if self.used_concat {
            s.push_str(
                "static char* ax_concat(const char* a, const char* b) {\n    \
                 long long la = (long long)__builtin_strlen(a);\n    \
                 long long lb = (long long)__builtin_strlen(b);\n    \
                 char* buf = (char*)__builtin_malloc((unsigned long long)(la + lb + 1));\n    \
                 __builtin_memcpy(buf, a, (unsigned long long)la);\n    \
                 __builtin_memcpy(buf + la, b, (unsigned long long)lb);\n    \
                 buf[la + lb] = 0;\n    \
                 return buf;\n}\n\n",
            );
        }
        if self.used_str_i {
            s.push_str(
                "static char* ax_str_i(long long v) {\n    \
                 char* buf = (char*)__builtin_malloc(32);\n    \
                 __builtin_snprintf(buf, 32, \"%lld\", v);\n    \
                 return buf;\n}\n\n",
            );
        }
        if self.used_str_f {
            s.push_str(
                "static char* ax_str_f(double v) {\n    \
                 char* buf = (char*)__builtin_malloc(64);\n    \
                 __builtin_snprintf(buf, 64, \"%f\", v);\n    \
                 return buf;\n}\n\n",
            );
        }
        if self.used_raw {
            s.push_str(
                "static long long ax_load_i64(long long a) { long long v; __builtin_memcpy(&v, (const void*)a, 8); return v; }\n\
                 static double ax_load_f64(long long a) { double v; __builtin_memcpy(&v, (const void*)a, 8); return v; }\n\
                 static long long ax_load_u8(long long a) { return (long long)*(const unsigned char*)a; }\n\
                 static void ax_store_i64(long long a, long long v) { __builtin_memcpy((void*)a, &v, 8); }\n\
                 static void ax_store_f64(long long a, double v) { __builtin_memcpy((void*)a, &v, 8); }\n\
                 static void ax_store_u8(long long a, long long v) { *(unsigned char*)a = (unsigned char)v; }\n\n",
            );
        }
        for helper in self.rep_helpers.values() {
            s.push_str(helper);
            s.push('\n');
        }
        if !self.rep_helpers.is_empty() {
            s.push('\n');
        }

        for p in &protos {
            s.push_str(p);
            s.push_str(";\n");
        }
        s.push('\n');
        for b in &bodies {
            s.push_str(b);
            s.push('\n');
        }

        let main_call = if main_ret == Type::Int {
            "    return (int)aoxn_main();\n".to_string()
        } else {
            "    aoxn_main();\n    return 0;\n".to_string()
        };
        s.push_str(&format!(
            "#ifdef _WIN32\nlong long _setmode(long long, long long);\n#endif\n\n\
             int main(void) {{\n\
             #ifdef _WIN32\n    _setmode(1, 0x8000);\n#endif\n\
             {main_call}}}\n"
        ));
        Ok(s)
    }

    /// Topological emission of struct definitions and array typedefs: a struct
    /// needs every by-value member type complete, and an array typedef needs
    /// its element type complete. By-value cycles are impossible in a sound
    /// program (infinite size) and are reported as internal errors.
    fn type_defs_text(&mut self) -> Result<String, String> {
        let mut out = String::new();
        let mut done: HashSet<String> = HashSet::new();
        let mut visiting: HashSet<String> = HashSet::new();

        let mut roots: Vec<String> = Vec::new();
        for name in &self.struct_names {
            roots.push(format!("s:{name}"));
        }
        for name in self.typedefs.keys() {
            roots.push(format!("t:{name}"));
        }
        for name in self.opt_structs.keys() {
            roots.push(format!("o:{name}"));
        }
        roots.sort();

        for root in roots {
            self.visit_type_node(&root, &mut done, &mut visiting, &mut out)?;
        }
        Ok(out)
    }

    fn type_dep_ids(t: &Type, out: &mut Vec<String>) {
        match t {
            Type::Array { elem, len } if *len != GENERIC_LEN => {
                out.push(format!("t:{}", Self::typedef_name(elem, *len)));
                Self::type_dep_ids(elem, out);
            }
            // a nullable carries its own struct; it must exist before any
            // typedef or field that mentions it (v0.40.0)
            Type::Opt(inner) => {
                out.push(format!("o:ax_opt_{}", Self::type_token(inner)));
                Self::type_dep_ids(inner, out);
            }
            Type::Struct(n) => out.push(format!("s:{n}")),
            _ => {}
        }
    }

    fn visit_type_node(
        &mut self,
        id: &str,
        done: &mut HashSet<String>,
        visiting: &mut HashSet<String>,
        out: &mut String,
    ) -> Result<(), String> {
        if done.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.to_string()) {
            return Err("internal error: recursive aggregate type at codegen".into());
        }
        if let Some(rest) = id.strip_prefix("s:") {
            let fields: Vec<(String, Type)> = self.struct_fields.get(rest).cloned().unwrap_or_default();
            let mut deps = Vec::new();
            for (_, ty) in &fields {
                Self::type_dep_ids(ty, &mut deps);
            }
            for d in deps {
                self.visit_type_node(&d, done, visiting, out)?;
            }
            let name = self.c_ident(rest);
            out.push_str(&format!("struct {name} {{\n"));
            for (fname, fty) in &fields {
                let decl = self.c_decl(fty, &self.c_ident(fname));
                out.push_str(&format!("    {decl};\n"));
            }
            out.push_str("};\n");
        } else if let Some(tname) = id.strip_prefix("t:") {
            let (elem, len) = self.typedefs.get(tname).cloned().ok_or_else(|| {
                format!("internal error: typedef '{tname}' referenced but never registered")
            })?;
            let mut deps = Vec::new();
            Self::type_dep_ids(&elem, &mut deps);
            for d in deps {
                self.visit_type_node(&d, done, visiting, out)?;
            }
            let decl = self.c_decl(&elem, &format!("data[{len}]"));
            out.push_str(&format!("typedef struct {{\n    {decl};\n}} {tname};\n"));
        } else if let Some(oname) = id.strip_prefix("o:") {
            // `T | None` (v0.40.0): the value plus a presence tag. Structs
            // are laid out the same way the language copies them — by value.
            let inner = self
                .opt_structs
                .get(oname)
                .cloned()
                .ok_or_else(|| format!("internal error: nullable '{oname}' referenced but never registered"))?;
            let mut deps = Vec::new();
            Self::type_dep_ids(&inner, &mut deps);
            for d in deps {
                self.visit_type_node(&d, done, visiting, out)?;
            }
            let decl = self.c_decl(&inner, "value");
            out.push_str(&format!("struct {oname} {{\n    {decl};\n    int has;\n}};\n"));
        }
        visiting.remove(id);
        done.insert(id.to_string());
        Ok(())
    }

    // ---- function emission ----

    fn gen_fn(&mut self, f: &FnDecl) -> Result<(String, String), String> {
        let sig = match self.sigs.get(&f.name) {
            Some(s) => (s.c_name.clone(), s.ret.clone()),
            None => return Err(format!("internal error: unknown function '{}' at codegen", f.name)),
        };
        self.out.clear();
        self.decls.clear();
        self.locals.clear();
        self.narrowed.clear();
        self.tmpn = 0;
        for p in &f.params {
            self.locals.insert(p.name.clone(), p.ty.clone());
        }
        self.cur_ret = f.ret.clone();
        self.emit_block(&f.body, 1)?;
        let decls = std::mem::take(&mut self.decls);
        let body = std::mem::take(&mut self.out);

        let mut params: Vec<String> = f.params.iter().map(|p| self.c_decl(&p.ty, &self.c_ident(&p.name))).collect();
        if params.is_empty() {
            params.push("void".to_string());
        }
        let header = format!("{} {}({})", self.c_type(&sig.1), sig.0, params.join(", "));
        let proto = header.clone();
        let mut text = format!("{header} {{\n");
        for d in &decls {
            text.push_str(&format!("    {d}\n"));
        }
        text.push_str(&body);
        text.push_str("}\n");
        Ok((proto, text))
    }

    fn line(&mut self, indent: usize, text: &str) {
        for _ in 0..indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    /// declare a binding in the function's declaration block on first use
    fn declare(&mut self, name: &str, ty: &Type) {
        if !self.locals.contains_key(name) {
            let decl = self.c_decl(ty, &self.c_ident(name));
            self.decls.push(format!("{decl};"));
            self.locals.insert(name.to_string(), ty.clone());
        }
    }

    /// emit `e` so it initializes a slot of type `want` (v0.40.0): a plain
    /// `T` widens into `T | None` (tag set), a `None` literal into the same
    /// struct (tag clear). Everything else passes through untouched.
    fn emit_coerced(&mut self, e: &Expr, want: &Type) -> Result<Type, String> {
        let t = self.hint(e)?;
        if let Type::Opt(inner) = want {
            if t == **inner {
                let name = format!("ax_opt_{}", Self::type_token(inner));
                self.out.push_str(&format!("(struct {name}){{ .value = "));
                self.emit_expr_inner(e)?;
                self.out.push_str(", .has = 1 }");
                return Ok(want.clone());
            }
            if t == Type::None {
                let name = format!("ax_opt_{}", Self::type_token(inner));
                let z = self.zero_value(inner);
                self.out.push_str(&format!("(struct {name}){{ .value = {z}, .has = 0 }}"));
                return Ok(want.clone());
            }
        }
        self.emit_expr_inner(e)
    }

    /// `emit_coerced` with the emitted text split off (the `emit_expr` shape)
    fn emit_coerced_expr(&mut self, e: &Expr, want: &Type) -> Result<(String, Type), String> {
        let start = self.out.len();
        let t = self.emit_coerced(e, want)?;
        let text = self.out.split_off(start);
        Ok((text, t))
    }

    /// zero value of a type, for the `.value` slot of a `None` carrier
    fn zero_value(&mut self, t: &Type) -> String {
        match t {
            Type::Struct(n) => format!("(struct {}){{0}}", self.c_ident(n)),
            Type::Array { .. } => format!("({}){{0}}", self.c_type(t)),
            _ => "0".to_string(),
        }
    }

/// the element type of an array literal, mirroring the checker's join:
/// `[1, None, 3]` is `[int | None; 3]` (v0.40.0)
    fn array_elem_type(g: &mut GenC, elems: &[Expr]) -> Result<Type, String> {
        let first = g.hint(&elems[0])?;
        if elems.len() == 1 {
            return Ok(first);
        }
        let mut saw_none = first == Type::None;
        let mut inner: Option<Type> = if first == Type::None { None } else { Some(first) };
        for e in &elems[1..] {
            let t = g.hint(e)?;
            if t == Type::None {
                saw_none = true;
            } else if let Some(prev) = &inner {
                if *prev != t {
                    return Err("internal error: mixed element types in array literal".into());
                }
            } else {
                inner = Some(t);
            }
        }
        Ok(match (saw_none, inner) {
            (true, Some(t)) => Type::Opt(Box::new(t)),
            (true, None) => Type::None,
            (false, Some(t)) => t,
            (false, None) => unreachable!(),
        })
    }

    /// the codegen mirror of the checker's `block_returns_all`: did every path
/// through this block leave it? (Only `return` matters for the nullable
/// narrowing's exit rule; `raise` joins later, in v0.40.0's exception work.)
fn block_returns_all(block: &Block) -> bool {
    block.stmts.iter().any(|s| matches!(s, Stmt::Return { .. }))
}

/// the codegen mirror of the checker's `narrow_target`: does this
/// condition narrow a variable, and what does each branch see?
    fn narrow_target(&self, cond: &Expr) -> Option<(String, Type, Type)> {
        let Expr::Binary { op, lhs, rhs, .. } = cond else { return None };
        let is_not = match op {
            BinOp::Is => false,
            BinOp::IsNot => true,
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
        let Type::Opt(inner) = self.locals.get(name)?.clone() else { return None };
        let (then_ty, else_ty) = if is_not {
            ((*inner).clone(), Type::None)
        } else {
            (Type::None, (*inner).clone())
        };
        Some((name.clone(), then_ty, else_ty))
    }

    fn emit_block(&mut self, block: &Block, indent: usize) -> Result<(), String> {
        for stmt in &block.stmts {
            self.emit_stmt(stmt, indent)?;
        }
        Ok(())
    }

    fn emit_stmt(&mut self, stmt: &Stmt, indent: usize) -> Result<(), String> {
        match stmt {
            Stmt::Let { name, ty, expr, .. } => {
                let hint = self.hint(expr)?;
                let bind_ty = match self.locals.get(name) {
                    Some(t) => t.clone(),
                    None => ty.clone().unwrap_or_else(|| hint.clone()),
                };
                self.declare(name, &bind_ty);
                let (rhs, t) = self.emit_coerced_expr(expr, &bind_ty)?;
                debug_assert_eq!(t, bind_ty, "let binding type drift");
                self.line(indent, &format!("{} = {rhs};", self.c_ident(name)));
                // re-narrow the view after a (re-)binding (mirror of the
                // checker): `b = None` is a Let, and the branch now sees None
                if self.narrowed.contains_key(name) {
                    if hint == Type::None || matches!(&bind_ty, Type::Opt(inner) if **inner == hint) {
                        self.narrowed.insert(name.clone(), hint);
                    } else {
                        self.narrowed.remove(name);
                    }
                }
            }
            Stmt::Assign { target, expr, .. } => {
                // lvalue text first, then the RHS (same order the LLVM
                // backend evaluates them in)
                let assigned = self.hint(expr)?;
                let (lhs, t) = self.emit_lvalue_text(target)?;
                let (rhs, t2) = self.emit_coerced_expr(expr, &t)?;
                debug_assert_eq!(t, t2, "assignment type drift");
                self.line(indent, &format!("{lhs} = {rhs};"));
                // re-narrow the view after a write (mirror of the checker):
                // the variable now provably holds a value of type `assigned`
                if let Expr::Var { name, .. } = target {
                    if self.narrowed.contains_key(name) {
                        if assigned == Type::None || matches!(&t, Type::Opt(inner) if **inner == assigned) {
                            self.narrowed.insert(name.clone(), assigned);
                        } else {
                            self.narrowed.remove(name);
                        }
                    }
                }
            }
            Stmt::If { cond, then_block, else_block, .. } => {
                // nullable narrowing (v0.40.0), mirroring the checker's
                // narrow_target: latch the branch type while each block is
                // emitted so reads yield `.value`, then restore. A then-branch
                // that always returns keeps the else narrowing latched after
                // the `if`, exactly like the checker.
                let narrow = self.narrow_target(cond);
                let (c, _) = self.emit_expr(cond)?;
                self.line(indent, &format!("if ({c}) {{"));
                match &narrow {
                    Some((var, then_ty, _)) => {
                        let saved = self.narrowed.get(var).cloned();
                        self.narrowed.insert(var.clone(), then_ty.clone());
                        self.emit_block(then_block, indent + 1)?;
                        match saved {
                            Some(t) => {
                                self.narrowed.insert(var.clone(), t);
                            }
                            None => {
                                self.narrowed.remove(var);
                            }
                        }
                    }
                    None => self.emit_block(then_block, indent + 1)?,
                }
                let then_exits = Self::block_returns_all(then_block);
                match else_block {
                    Some(eb) => {
                        self.line(indent, "} else {");
                        match &narrow {
                            Some((var, _, else_ty)) => {
                                let saved = self.narrowed.get(var).cloned();
                                self.narrowed.insert(var.clone(), else_ty.clone());
                                self.emit_block(eb, indent + 1)?;
                                match saved {
                                    Some(t) => {
                                        self.narrowed.insert(var.clone(), t);
                                    }
                                    None => {
                                        self.narrowed.remove(var);
                                    }
                                }
                                if then_exits {
                                    self.narrowed.insert(var.clone(), else_ty.clone());
                                }
                            }
                            None => self.emit_block(eb, indent + 1)?,
                        }
                        self.line(indent, "}");
                    }
                    None => {
                        self.line(indent, "}");
                        if let Some((var, _, else_ty)) = &narrow {
                            if then_exits {
                                self.narrowed.insert(var.clone(), else_ty.clone());
                            }
                        }
                    }
                }
            }
            Stmt::While { cond, body, .. } => {
                let (c, _) = self.emit_expr(cond)?;
                self.line(indent, &format!("while ({c}) {{"));
                self.emit_block(body, indent + 1)?;
                self.line(indent, "}");
            }
            Stmt::For { var, iter, body, .. } => self.emit_for(var, iter, body, indent)?,
            Stmt::Break { .. } => self.line(indent, "break;"),
            Stmt::Continue { .. } => self.line(indent, "continue;"),
            Stmt::Return { expr, .. } => match expr {
                None => self.line(indent, "return;"),
                Some(e) => {
                    // the function's return type decides the coercion
                    let want = self.cur_ret.clone();
                    let (r, _) = self.emit_coerced_expr(e, &want)?;
                    self.line(indent, &format!("return {r};"));
                }
            },
            Stmt::Pass => {}
            Stmt::ExprStmt { expr } => {
                let (e, _) = self.emit_expr(expr)?;
                self.line(indent, &format!("{e};"));
            }
        }
        Ok(())
    }

    /// `for var in range(...)` / `for var in array:` — start/end/step and the
    /// array source are evaluated once at loop entry (Python semantics); the
    /// loop variable is a function-scoped binding (same flat slot discipline
    /// as the LLVM backend), and array elements are copied into it per
    /// iteration.
    fn emit_for(&mut self, var: &str, iter: &ForIter, body: &Block, indent: usize) -> Result<(), String> {
        match iter {
            ForIter::Range(args) => {
                // range(n) -> 0..n, range(a, b) -> a..b, range(a, b, step)
                let (s_text, e_text, st_text) = match args.len() {
                    1 => {
                        let (e, _) = self.emit_expr(&args[0])?;
                        ("0LL".to_string(), e, "1LL".to_string())
                    }
                    2 => {
                        let (s, _) = self.emit_expr(&args[0])?;
                        let (e, _) = self.emit_expr(&args[1])?;
                        (s, e, "1LL".to_string())
                    }
                    _ => {
                        let (s, _) = self.emit_expr(&args[0])?;
                        let (e, _) = self.emit_expr(&args[1])?;
                        let (st, _) = self.emit_expr(&args[2])?;
                        (s, e, st)
                    }
                };
                self.declare(var, &Type::Int);
                let tv = self.fresh_tmp();
                let te = self.fresh_tmp();
                let ts = self.fresh_tmp();
                let vi = self.c_ident(var);
                self.line(indent, "{");
                self.line(indent + 1, &format!("long long {tv} = {s_text};"));
                self.line(indent + 1, &format!("long long {te} = {e_text};"));
                self.line(indent + 1, &format!("long long {ts} = {st_text};"));
                self.line(
                    indent + 1,
                    &format!("for ({vi} = {tv}; ({ts} > 0) ? ({vi} < {te}) : ({vi} > {te}); {vi} = {vi} + {ts}) {{"),
                );
                self.emit_block(body, indent + 2)?;
                self.line(indent + 1, "}");
                self.line(indent, "}");
            }
            ForIter::Array(e) => {
                let ty = self.hint(e)?;
                let (elem, len) = match &ty {
                    Type::Array { elem, len } => ((**elem).clone(), *len),
                    other => return Err(format!("internal error: iterating {other} at codegen")),
                };
                let base = if e.is_lvalue() {
                    let (text, _) = self.emit_lvalue_text(e)?;
                    text
                } else {
                    // non-lvalue source: materialize once (evaluated exactly
                    // once, like the LLVM backend's entry-hoisted temp)
                    let tmp = self.fresh_tmp();
                    let decl = self.c_decl(&ty, &tmp);
                    let (val, _) = self.emit_expr(e)?;
                    self.line(indent, &format!("{decl} = {val};"));
                    tmp
                };
                self.declare(var, &elem);
                let idx = self.fresh_tmp();
                let vi = self.c_ident(var);
                self.line(indent, "{");
                self.line(
                    indent + 1,
                    &format!("for (long long {idx} = 0; {idx} < {len}LL; {idx}++) {{"),
                );
                self.line(indent + 2, &format!("{vi} = {base}.data[{idx}];"));
                self.emit_block(body, indent + 2)?;
                self.line(indent + 1, "}");
                self.line(indent, "}");
            }
        }
        Ok(())
    }

    // ---- expression emission (writes C text into self.out, returns the type) ----

    /// capturing form of [`Self::emit_lvalue`]: returns the rendered text
    /// together with the type (mirrors [`Self::emit_expr`])
    fn emit_lvalue_text(&mut self, target: &Expr) -> Result<(String, Type), String> {
        let start = self.out.len();
        let t = self.emit_lvalue(target)?;
        let text = self.out.split_off(start);
        Ok((text, t))
    }

    fn emit_lvalue(&mut self, target: &Expr) -> Result<Type, String> {        match target {
            Expr::Var { name, .. } => {
                let t = self
                    .locals
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("internal error: unknown variable '{name}' at codegen"))?;
                let n = self.c_ident(name);
                self.out.push_str(&n);
                Ok(t)
            }
            Expr::Index { arr, idx, .. } => {
                let base_ty = self.emit_lvalue(arr)?;
                let elem = match &base_ty {
                    Type::Array { elem, .. } => (**elem).clone(),
                    other => return Err(format!("internal error: cannot index {other} at codegen")),
                };
                self.out.push_str(".data[");
                self.emit_expr_inner(idx)?;
                self.out.push(']');
                Ok(elem)
            }
            Expr::Field { obj, name, .. } => {
                let base_ty = self.emit_lvalue(obj)?;
                let sname = match &base_ty {
                    Type::Struct(s) => s.clone(),
                    other => return Err(format!("internal error: cannot access field of {other} at codegen")),
                };
                let fty = self
                    .struct_fields
                    .get(&sname)
                    .and_then(|f| f.iter().find(|(n, _)| n == name))
                    .map(|(_, t)| t.clone())
                    .ok_or_else(|| format!("internal error: unknown field '{name}' of '{sname}' at codegen"))?;
                let n = self.c_ident(name);
                self.out.push('.');
                self.out.push_str(&n);
                Ok(fty)
            }
            other => Err(format!(
                "internal error: invalid lvalue '{}' at codegen",
                other.pos().line
            )),
        }
    }

    fn emit_expr(&mut self, expr: &Expr) -> Result<(String, Type), String> {
        let start = self.out.len();
        let t = self.emit_expr_inner(expr)?;
        let text = self.out.split_off(start);
        Ok((text, t))
    }

    fn emit_expr_inner(&mut self, expr: &Expr) -> Result<Type, String> {
        match expr {
            Expr::Cast { expr: inner, to, .. } => {
                let from = self.hint(inner)?;
                if from == *to {
                    return self.emit_expr_inner(inner);
                }
                match (&from, to) {
                    (Type::Int, Type::Float) => {
                        self.out.push_str("(double)(");
                        self.emit_expr_inner(inner)?;
                        self.out.push(')');
                    }
                    (Type::Float, Type::Int) => {
                        self.out.push_str("(long long)(");
                        self.emit_expr_inner(inner)?;
                        self.out.push(')');
                    }
                    (Type::Int, Type::Bool) => {
                        self.out.push_str("((");
                        self.emit_expr_inner(inner)?;
                        self.out.push_str(") != 0)");
                    }
                    (Type::Bool, Type::Int) => {
                        self.out.push_str("(long long)(");
                        self.emit_expr_inner(inner)?;
                        self.out.push(')');
                    }
                    (Type::Float, Type::Bool) => {
                        // ordered not-equal semantics (JS `Boolean(x)`):
                        // true iff nonzero AND not NaN. Known experimental-C
                        // deviation: the operand text is emitted twice, so
                        // effectful operands evaluate twice here (the LLVM
                        // backend evaluates once).
                        self.out.push_str("((");
                        self.emit_expr_inner(inner)?;
                        self.out.push_str(") != 0 && (");
                        self.emit_expr_inner(inner)?;
                        self.out.push_str(") == (");
                        self.emit_expr_inner(inner)?;
                        self.out.push_str("))");
                    }
                    // v0.40.0: re-interpret an address as a callable function
                    // pointer (`addr as fn(int, int) -> int`) and back. The
                    // cast spells the exact signature, so the C is valid and
                    // the call is checked.
                    (Type::Int, Type::FnPtr { ret, params }) => {
                        let ps: Vec<String> = params.iter().map(|p| self.c_type(p)).collect();
                        let args = if ps.is_empty() { "void".to_string() } else { ps.join(", ") };
                        let rt = self.c_type(ret);
                        self.out.push_str(&format!("(({rt} (*)({args}))("));
                        self.emit_expr_inner(inner)?;
                        self.out.push_str("))");
                    }
                    (Type::FnPtr { .. }, Type::Int) => {
                        self.out.push_str("((long long)(");
                        self.emit_expr_inner(inner)?;
                        self.out.push_str("))");
                    }
                    _ => return Err(format!("internal error: invalid cast from {from} to {to}")),
                }
                Ok(to.clone())
            }
            Expr::Int(v, _) => {
                self.out.push_str(&format!("{v}LL"));
                Ok(Type::Int)
            }
            Expr::Float(v, _) => {
                self.out.push_str(&Self::float_lit(*v));
                Ok(Type::Float)
            }
            Expr::Bool(v, _) => {
                self.out.push_str(if *v { "1" } else { "0" });
                Ok(Type::Bool)
            }
            Expr::NoneLit(_) => {
                self.out.push_str("0LL");
                Ok(Type::None)
            }
            Expr::Str(s, _) => {
                let lit = Self::c_string_lit(s);
                self.out.push_str(&lit);
                Ok(Type::Str)
            }
            Expr::Var { name, .. } => match self.locals.get(name).cloned() {
                Some(t) => {
                    // narrowed by `is None` (v0.40.0): the read yields the
                    // inner value (or the absence), never the tagged struct.
                    // The map stores the type the branch SEES (None or T).
                    match self.narrowed.get(name).cloned() {
                        Some(nt) if matches!(t, Type::Opt(_)) => {
                            if nt == Type::None {
                                self.out.push_str("0LL");
                                Ok(Type::None)
                            } else {
                                let n = self.c_ident(name);
                                self.out.push_str(&format!("{n}.value"));
                                Ok(nt)
                            }
                        }
                        _ => {
                            let n = self.c_ident(name);
                            self.out.push_str(&n);
                            Ok(t)
                        }
                    }
                }
                None => match self.sigs.get(name) {
                    Some(sig) => {
                        self.out.push_str(&sig.c_name);
                        Ok(Type::FnPtr {
                            ret: Box::new(sig.ret.clone()),
                            params: sig.params.clone(),
                        })
                    }
                    None => Err(format!("internal error: unknown variable '{name}' at codegen")),
                },
            },
            Expr::Index { arr, idx, .. } => {
                let base_ty = if arr.is_lvalue() {
                    self.emit_lvalue(arr)?
                } else {
                    self.emit_expr_inner(arr)?
                };
                let elem = match &base_ty {
                    Type::Array { elem, .. } => (**elem).clone(),
                    other => return Err(format!("internal error: cannot index {other} at codegen")),
                };
                self.out.push_str(".data[");
                self.emit_expr_inner(idx)?;
                self.out.push(']');
                Ok(elem)
            }
            Expr::Field { obj, name, .. } => {
                let base_ty = if obj.is_lvalue() {
                    self.emit_lvalue(obj)?
                } else {
                    self.emit_expr_inner(obj)?
                };
                let sname = match &base_ty {
                    Type::Struct(s) => s.clone(),
                    other => return Err(format!("internal error: cannot access field of {other} at codegen")),
                };
                let fty = self
                    .struct_fields
                    .get(&sname)
                    .and_then(|f| f.iter().find(|(n, _)| n == name))
                    .map(|(_, t)| t.clone())
                    .ok_or_else(|| format!("internal error: unknown field '{name}' of '{sname}' at codegen"))?;
                let n = self.c_ident(name);
                self.out.push('.');
                self.out.push_str(&n);
                Ok(fty)
            }
            Expr::ArrayLit { elems, .. } => {
                if elems.is_empty() {
                    return Err("internal error: empty array literal at codegen".into());
                }
                let elem_ty = Self::array_elem_type(self, elems)?;
                let arr_ty = Type::Array { elem: Box::new(elem_ty.clone()), len: elems.len() };
                let ct = self.c_type(&arr_ty);
                self.out.push_str(&format!("({ct}){{.data = {{"));
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    let t = self.emit_coerced(e, &elem_ty)?;
                    if t != elem_ty {
                        return Err("internal error: mixed element types in array literal".into());
                    }
                }
                // close the .data array initializer and the compound literal
                // (plain push_str: braces are literal here, no format escapes)
                self.out.push_str("}}");
                Ok(arr_ty)
            }
            Expr::ArrayRep { elem, count, pos: _, lit_id: _ } => {
                let elem_ty = self.hint(elem)?;
                let arr_ty = Type::Array { elem: Box::new(elem_ty.clone()), len: *count };
                let ct = self.c_type(&arr_ty);
                let key = expr as *const Expr as usize;
                if !self.rep_helpers.contains_key(&key) {
                    let ed = self.c_decl(&elem_ty, "ax_v");
                    let helper = format!(
                        "static {ct} ax_rep_{key}({ed}) {{\n    \
                         {ct} ax_r;\n    \
                         for (long long ax_i = 0; ax_i < {count}LL; ax_i++) {{\n        \
                         ax_r.data[ax_i] = ax_v;\n    \
                         }}\n    \
                         return ax_r;\n}}"
                    );
                    self.rep_helpers.insert(key, helper);
                }
                self.out.push_str(&format!("ax_rep_{key}("));
                self.emit_expr_inner(elem)?;
                self.out.push(')');
                Ok(arr_ty)
            }
            Expr::StructLit { name, fields, .. } => {
                let iter = fields.iter().map(|(n, e)| (n, e));
                self.emit_struct_construction(name, iter)
            }
            Expr::Call { name, args, pos, .. } => self.emit_call(name, args, pos, expr),
            Expr::Unary { op, expr, .. } => match op {
                UnOp::Not => {
                    self.out.push_str("!(");
                    self.emit_expr_inner(expr)?;
                    self.out.push(')');
                    Ok(Type::Bool)
                }
                UnOp::Neg => {
                    self.out.push_str("-(");
                    let t = self.emit_expr_inner(expr)?;
                    self.out.push(')');
                    Ok(t)
                }
                UnOp::BitNot => {
                    self.out.push_str("~(");
                    let t = self.emit_expr_inner(expr)?;
                    self.out.push(')');
                    Ok(t)
                }
            },
            Expr::Binary { op, lhs, rhs, .. } => self.emit_binary(*op, lhs, rhs),
        }
    }

    /// borrow the field expressions (never clone: expression text is emitted
    /// in place, and cloned nodes would alias any address-keyed caches)
    fn emit_struct_construction<'f, I>(&mut self, name: &str, fields: I) -> Result<Type, String>
    where
        I: Iterator<Item = (&'f String, &'f Expr)>,
    {
        if !self.struct_fields.contains_key(name) {
            return Err(format!("internal error: unknown struct '{name}' at codegen"));
        }
        let ct = self.c_type(&Type::Struct(name.to_string()));
        self.out.push_str(&format!("({ct}){{"));
        for (i, (fname, fexpr)) in fields.enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            let n = self.c_ident(fname);
            self.out.push_str(&format!(".{n} = "));
            // a nullable field takes a plain T (or None) and tags it
            let want = self
                .struct_fields
                .get(name)
                .and_then(|fs| fs.iter().find(|(n2, _)| n2 == fname))
                .map(|(_, t)| t.clone());
            match want {
                Some(w) => {
                    self.emit_coerced(fexpr, &w)?;
                }
                None => {
                    self.emit_expr_inner(fexpr)?;
                }
            }
        }
        self.out.push('}');
        Ok(Type::Struct(name.to_string()))
    }

    fn emit_call(&mut self, name: &str, args: &[Arg], pos: &Pos, expr: &Expr) -> Result<Type, String> {
        // generic calls are routed to their monomorphized instance
        let node = expr as *const Expr as usize;
        let routed = self.call_map.get(&node).map(|s| s.as_str());
        let name: &str = routed.unwrap_or(name);
        let _ = pos;

        if name == "print" {
            if args.len() != 1 {
                return Err("internal error: print expects 1 argument".into());
            }
            let t = self.hint(&args[0].value)?;
            match t {
                Type::Bool => {
                    self.out.push_str("__builtin_printf(\"%s\\n\", (");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push_str(") ? \"true\" : \"false\")");
                }
                Type::Int => {
                    self.out.push_str("__builtin_printf(\"%lld\\n\", ");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push(')');
                }
                Type::Float => {
                    self.out.push_str("__builtin_printf(\"%f\\n\", ");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push(')');
                }
                Type::Str => {
                    self.out.push_str("__builtin_printf(\"%s\\n\", ");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push(')');
                }
                other => return Err(format!("internal error: unexpected print type {other}")),
            }
            return Ok(Type::Void);
        }
        if name == "to_int" || name == "to_float" {
            // explicit scalar conversions (same semantics as Expr::Cast)
            if args.len() != 1 {
                return Err(format!("internal error: {name} expects 1 argument"));
            }
            let t = self.hint(&args[0].value)?;
            let want = if name == "to_int" { Type::Int } else { Type::Float };
            if t == want {
                return self.emit_expr_inner(&args[0].value);
            }
            if matches!(t, Type::FnPtr { .. }) {
                // the raw address of the function (v0.40.0); the explicit
                // cast is what keeps clang quiet about pointer-to-integer
                self.out.push_str("((long long)(");
                self.emit_expr_inner(&args[0].value)?;
                self.out.push_str("))");
                return Ok(Type::Int);
            }
            match (&t, &want) {
                (Type::Int, Type::Float) => {
                    self.out.push_str("(double)(");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push(')');
                }
                (Type::Float, Type::Int) => {
                    self.out.push_str("(long long)(");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push(')');
                }
                (Type::Int, Type::Bool) => {
                    self.out.push_str("((");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push_str(") != 0)");
                }
                (Type::Bool, Type::Int) => {
                    self.out.push_str("(long long)(");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push(')');
                }
                (Type::Float, Type::Bool) => {
                    self.out.push_str("((");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push_str(") != 0 && (");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push_str(") == (");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push_str("))");
                }
                _ => return Err(format!("internal error: invalid {name}() from {t}")),
            }
            return Ok(want);
        }
        if name == "len" {
            if args.len() != 1 {
                return Err("internal error: len expects 1 argument".into());
            }
            let t = self.hint(&args[0].value)?;
            return match t {
                Type::Array { len, .. } => {
                    self.out.push_str(&format!("{len}LL"));
                    Ok(Type::Int)
                }
                Type::Str => {
                    self.out.push_str("(long long)__builtin_strlen(");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push(')');
                    Ok(Type::Int)
                }
                other => Err(format!("internal error: len on {other} at codegen")),
            };
        }
        if name == "str" {
            if args.len() != 1 {
                return Err("internal error: str expects 1 argument".into());
            }
            let t = self.hint(&args[0].value)?;
            return match t {
                Type::Str => self.emit_expr_inner(&args[0].value),
                Type::Int => {
                    self.used_str_i = true;
                    self.out.push_str("ax_str_i(");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push(')');
                    Ok(Type::Str)
                }
                Type::Float => {
                    self.used_str_f = true;
                    self.out.push_str("ax_str_f(");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push(')');
                    Ok(Type::Str)
                }
                Type::Bool => {
                    self.out.push_str("((");
                    self.emit_expr_inner(&args[0].value)?;
                    self.out.push_str(") ? \"true\" : \"false\")");
                    Ok(Type::Str)
                }
                other => Err(format!("internal error: str on {other} at codegen")),
            };
        }
        // raw memory primitives
        if name == "load_i64" || name == "load_f64" || name == "load_u8" {
            self.used_raw = true;
            if name == "load_u8" {
                if args.len() != 2 {
                    return Err("internal error: load_u8 expects 2 arguments".into());
                }
                self.out.push_str("ax_load_u8((long long)(");
                self.emit_expr_inner(&args[0].value)?;
                self.out.push_str(") + (");
                self.emit_expr_inner(&args[1].value)?;
                self.out.push_str("))");
                return Ok(Type::Int);
            }
            if args.len() != 1 {
                return Err(format!("internal error: {name} expects 1 argument"));
            }
            let helper = if name == "load_f64" { "ax_load_f64" } else { "ax_load_i64" };
            self.out.push_str(&format!("{helper}("));
            self.emit_expr_inner(&args[0].value)?;
            self.out.push(')');
            return Ok(if name == "load_f64" { Type::Float } else { Type::Int });
        }
        if name == "store_i64" || name == "store_f64" {
            self.used_raw = true;
            if args.len() != 2 {
                return Err(format!("internal error: {name} expects 2 arguments"));
            }
            let helper = if name == "store_f64" { "ax_store_f64" } else { "ax_store_i64" };
            self.out.push_str(&format!("{helper}("));
            self.emit_expr_inner(&args[0].value)?;
            self.out.push_str(", ");
            self.emit_expr_inner(&args[1].value)?;
            self.out.push(')');
            return Ok(Type::Void);
        }
        if name == "store_u8" {
            self.used_raw = true;
            if args.len() != 3 {
                return Err("internal error: store_u8 expects 3 arguments".into());
            }
            self.out.push_str("ax_store_u8((long long)(");
            self.emit_expr_inner(&args[0].value)?;
            self.out.push_str(") + (");
            self.emit_expr_inner(&args[1].value)?;
            self.out.push_str("), ");
            self.emit_expr_inner(&args[2].value)?;
            self.out.push(')');
            return Ok(Type::Void);
        }
        if name == "target_os" {
            let lit = Self::c_string_lit(crate::platform::target_os_name());
            self.out.push_str(&lit);
            return Ok(Type::Str);
        }
        if name == "as_string" {
            if args.len() != 1 {
                return Err("internal error: as_string expects 1 argument".into());
            }
            self.out.push_str("((char*)(");
            self.emit_expr_inner(&args[0].value)?;
            self.out.push_str("))");
            return Ok(Type::Str);
        }
        if name == "as_ptr" {
            if args.len() != 1 {
                return Err("internal error: as_ptr expects 1 argument".into());
            }
            self.out.push_str("((long long)(");
            self.emit_expr_inner(&args[0].value)?;
            self.out.push_str("))");
            return Ok(Type::Int);
        }
        // struct construction through call syntax: Name(field = value, ...)
        if !self.sigs.contains_key(name) {
            if self.struct_names.contains(name) {
                let mut kws: Vec<(&String, &Expr)> = Vec::with_capacity(args.len());
                for a in args {
                    match &a.name {
                        Some(n) => kws.push((n, &a.value)),
                        None => {
                            return Err(format!(
                                "internal error: unnamed argument at line {}",
                                pos.line
                            ))
                        }
                    }
                }
                return self.emit_struct_construction(name, kws.into_iter());
            }
            // indirect call through a function pointer (v0.40.0): the callee
            // is always an identifier (call syntax carries a name, not an
            // arbitrary expression), so the cast is the whole trick
            if let Some(Type::FnPtr { ret, params }) = self.locals.get(name).cloned() {
                let ps: Vec<String> = params.iter().map(|p| self.c_type(p)).collect();
                let cast_args = if ps.is_empty() { "void".to_string() } else { ps.join(", ") };
                let rt = self.c_type(&ret);
                // `((T)callee)(args...)`: the cast parentheses are the whole trick
                self.out.push_str(&format!("(({rt} (*)({cast_args})){})(", self.c_ident(name)));
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.emit_expr_inner(&a.value)?;
                }
                self.out.push(')');
                return Ok(*ret);
            }
            return Err(format!("internal error: unknown callable '{name}' at codegen"));
        }
        let (c_name, ret, params) = {
            let sig = &self.sigs[name];
            (sig.c_name.clone(), sig.ret.clone(), sig.params.clone())
        };
        self.out.push_str(&c_name);
        self.out.push('(');
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            let want = params.get(i).cloned();
            match want {
                Some(w) => {
                    self.emit_coerced(&a.value, &w)?;
                }
                None => {
                    self.emit_expr_inner(&a.value)?;
                }
            }
        }
        self.out.push(')');
        Ok(ret)
    }

    fn emit_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Type, String> {
        use BinOp::*;
if matches!(op, Is | IsNot) {
            // nullable identity (v0.40.0). The checker guarantees exactly one
            // side is the `None` literal, so the test reduces to the presence
            // tag of the other side.
            let lt = self.hint(lhs)?;
            let rt = self.hint(rhs)?;
            let (probe, absent) = if lt == Type::None { (rhs, true) } else { (lhs, false) };
            let cmp = if matches!(op, Is) { "==" } else { "!=" };
            self.out.push('(');
            // the probe is the None literal on both sides of `None is None`,
            // which degenerates to `0 == 0`
            let probe_is_none = matches!(probe, Expr::NoneLit(_));
            let absent_side_first = absent;
            for side in 0..2 {
                if side == 1 {
                    self.out.push_str(&format!(" {cmp} "));
                }
                let this_is_absent = if side == 0 { absent_side_first } else { !absent_side_first };
                if this_is_absent {
                    self.out.push_str("0LL");
                } else if probe_is_none {
                    self.out.push_str("0LL");
                } else {
                    self.emit_expr_inner(probe)?;
                    self.out.push_str(".has");
                }
            }
            self.out.push(')');
            let _ = rt;
            return Ok(Type::Bool);
        }
        if matches!(op, And | Or) {
            self.out.push('(');
            let t = self.emit_expr_inner(lhs)?;
            if t != Type::Bool {
                return Err("internal error: boolean operator on non-bool".into());
            }
            self.out.push_str(if op == And { ") && (" } else { ") || (" });
            let t2 = self.emit_expr_inner(rhs)?;
            if t2 != Type::Bool {
                return Err("internal error: boolean operator on non-bool".into());
            }
            self.out.push(')');
            return Ok(Type::Bool);
        }
        let t = self.hint(lhs)?;
        if t == Type::Str {
            match op {
                Add => {
                    self.used_concat = true;
                    self.out.push_str("ax_concat(");
                    self.emit_expr_inner(lhs)?;
                    self.out.push_str(", ");
                    self.emit_expr_inner(rhs)?;
                    self.out.push(')');
                    return Ok(Type::Str);
                }
                Eq | Ne | Lt | Le | Gt | Ge => {
                    let cmp = match op {
                        Eq => "==",
                        Ne => "!=",
                        Lt => "<",
                        Le => "<=",
                        Gt => ">",
                        Ge => ">=",
                        _ => unreachable!(),
                    };
                    self.out.push_str("((__builtin_strcmp(");
                    self.emit_expr_inner(lhs)?;
                    self.out.push_str(", ");
                    self.emit_expr_inner(rhs)?;
                    self.out.push_str(&format!(") {cmp} 0))"));
                    return Ok(Type::Bool);
                }
                _ => return Err("internal error: invalid string operator".into()),
            }
        }
        let arith = match op {
            // `is`/`is not` return before this point (nullable identity)
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Mod => {
                if t == Type::Float {
                    return Err("internal error: float mod".into());
                }
                "%"
            }
            Shl => "<<",
            Shr => ">>",
            BitAnd => "&",
            BitOr => "|",
            BitXor => "^",
            Eq => "==",
            Ne => "!=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            And | Or => return Err("internal error: boolean operator reached arithmetic codegen".into()),
            Is | IsNot => return Err("internal error: nullable identity reached arithmetic codegen".into()),
        };
        self.out.push_str("((");
        self.emit_expr_inner(lhs)?;
        self.out.push_str(&format!(") {arith} ("));
        self.emit_expr_inner(rhs)?;
        self.out.push_str("))");
        Ok(match op {
            Eq | Ne | Lt | Le | Gt | Ge => Type::Bool,
            _ => t,
        })
    }

    // ---- static type hint (mirror of codegen::Gen::type_hint) ----

    fn hint(&self, expr: &Expr) -> Result<Type, String> {
        match expr {
            Expr::Cast { to, .. } => Ok(to.clone()),
            Expr::Int(..) => Ok(Type::Int),
            Expr::Float(..) => Ok(Type::Float),
            Expr::Bool(..) => Ok(Type::Bool),
            Expr::Str(..) => Ok(Type::Str),
            Expr::Var { name, .. } => match self.locals.get(name).cloned() {
                Some(t) => {
                    // a narrowed read is the inner type (v0.40.0)
                    match self.narrowed.get(name) {
                        Some(nt) if matches!(t, Type::Opt(_)) => Ok(nt.clone()),
                        _ => Ok(t),
                    }
                }
                None => match self.sigs.get(name) {
                    Some(sig) => Ok(Type::FnPtr {
                        ret: Box::new(sig.ret.clone()),
                        params: sig.params.clone(),
                    }),
                    None => Err(format!("internal error: unknown variable '{name}' in type hint")),
                },
            },
            Expr::NoneLit(_) => Ok(Type::None),
            Expr::Call { name, .. } => {
                let node = expr as *const Expr as usize;
                let eff: &str = self
                    .call_map
                    .get(&node)
                    .map(|s| s.as_str())
                    .unwrap_or(name.as_str());
                if let Some(sig) = self.sigs.get(eff) {
                    return Ok(sig.ret.clone());
                }
                // indirect call through a local function pointer (v0.40.0)
                if let Some(Type::FnPtr { ret, .. }) = self.locals.get(eff) {
                    return Ok((**ret).clone());
                }
                if self.struct_names.contains(eff) {
                    return Ok(Type::Struct(eff.to_string()));
                }
                if eff == "print" {
                    return Ok(Type::Void);
                }
                if eff == "len" {
                    return Ok(Type::Int);
                }
                if eff == "str" {
                    return Ok(Type::Str);
                }
                if eff == "to_int" {
                    return Ok(Type::Int);
                }
                if eff == "to_float" {
                    return Ok(Type::Float);
                }
                if eff == "target_os" || eff == "as_string" {
                    return Ok(Type::Str);
                }
                if eff == "as_ptr" || eff == "load_i64" || eff == "load_u8" {
                    return Ok(Type::Int);
                }
                if eff == "load_f64" {
                    return Ok(Type::Float);
                }
                Err(format!("internal error: unknown call '{eff}' in type hint"))
            }
            Expr::Unary { expr, .. } => self.hint(expr),
            Expr::Binary { op, lhs, rhs, .. } => match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or | BinOp::Is | BinOp::IsNot => {
                    Ok(Type::Bool)
                }
                _ => self.hint(lhs).or_else(|_| self.hint(rhs)),
            },
            Expr::Index { arr, .. } => match self.hint(arr)? {
                Type::Array { elem, .. } => Ok(*elem),
                other => Err(format!("internal error: index on {other} in type hint")),
            },
            Expr::Field { obj, name, .. } => match self.hint(obj)? {
                Type::Struct(sname) => self
                    .struct_fields
                    .get(&sname)
                    .and_then(|f| f.iter().find(|(n, _)| n == name))
                    .map(|(_, t)| t.clone())
                    .ok_or_else(|| format!("internal error: field '{name}' in type hint")),
                other => Err(format!("internal error: field access on {other} in type hint")),
            },
            Expr::ArrayLit { elems, .. } => {
                let elem = self.hint(&elems[0])?;
                Ok(Type::Array { elem: Box::new(elem), len: elems.len() })
            }
            Expr::ArrayRep { elem, count, .. } => {
                let elem = self.hint(elem)?;
                Ok(Type::Array { elem: Box::new(elem), len: *count })
            }
            Expr::StructLit { name, .. } => Ok(Type::Struct(name.clone())),
        }
    }
}
