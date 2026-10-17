//! TS-M1 parser (S1) tests: AST lowering shapes, slice diagnostics, and
//! end-to-end compile+run of TypeScript sources through the full pipeline.

use aoxn::ast::{Expr, Stmt, Type};
use aoxn::ts::parser::parse;

fn prog(src: &str) -> aoxn::ast::Program {
    parse(0, src).expect("parse failed")
}

#[test]
fn ts_parse_function_and_interface() {
    let p = prog(
        "interface Point { x: number; y: string }\n\
         function dist(p: Point, n: number): number {\n\
             return p.x + n;\n\
         }\n",
    );
    assert_eq!(p.structs.len(), 1);
    assert_eq!(p.structs[0].name, "Point");
    assert_eq!(p.structs[0].fields.len(), 2);
    assert_eq!(p.structs[0].fields[1].name, "y");
    assert_eq!(p.structs[0].fields[1].ty, Type::Str);

    assert_eq!(p.funcs.len(), 1);
    let f = &p.funcs[0];
    assert_eq!(f.name, "dist");
    assert_eq!(f.params.len(), 2);
    assert_eq!(f.params[0].ty, Type::Struct("Point".into()));
    assert_eq!(f.params[1].ty, Type::Float); // `number` is f64 in S2b
    assert_eq!(f.ret, Type::Float);
}

#[test]
fn ts_parse_c_style_for_desugars_to_while() {
    let p = prog("function main(): number {\n  for (let i = 0; i < 3; i++) {\n    x += i;\n  }\n  return 0;\n}");
    let body = &p.funcs[0].body.stmts;
    // for becomes: if true { let i = 0; while ... }
    let (init, loop_) = match &body[0] {
        Stmt::If { cond: Expr::Bool(true, ..), then_block, .. } => {
            (then_block.stmts.first().unwrap(), then_block.stmts.get(1).unwrap())
        }
        other => panic!("unexpected desugar shape: {other:?}"),
    };
    assert!(matches!(init, Stmt::Let { name, .. } if name == "i"));
    match loop_ {
        Stmt::While { body, .. } => {
            // body = [x += i, i++]
            assert_eq!(body.stmts.len(), 2);
            assert!(matches!(&body.stmts[0], Stmt::Assign { .. }));
            assert!(matches!(&body.stmts[1], Stmt::Assign { .. }));
        }
        other => panic!("expected while, got {other:?}"),
    }
}

#[test]
fn ts_parse_do_while_and_inc() {
    let p = prog("function main(): number {\n  let n = 0;\n  do {\n    n++;\n  } while (n < 3);\n  return n;\n}");
    let stmts = &p.funcs[0].body.stmts;
    match &stmts[1] {
        Stmt::While { cond, body, .. } => {
            assert!(matches!(cond, Expr::Bool(true, _)));
            // [n++, if !(n < 3) { break }]
            assert_eq!(body.stmts.len(), 2);
            assert!(matches!(&body.stmts[0], Stmt::Assign { target: Expr::Var { name, .. }, .. } if name == "n"));
            assert!(matches!(&body.stmts[1], Stmt::If { .. }));
        }
        other => panic!("expected do-while desugar, got {other:?}"),
    }
}

#[test]
fn ts_parse_console_log_and_length_lower() {
    let p = prog("function main(): number {\n  console.log(\"hi\");\n  const n = \"abcd\".length;\n  return 0;\n}");
    let stmts = &p.funcs[0].body.stmts;
    match &stmts[0] {
        Stmt::ExprStmt { expr: Expr::Call { name, args, .. } } => {
            assert_eq!(name, "print");
            assert_eq!(args.len(), 1);
        }
        other => panic!("console.log should lower to print: {other:?}"),
    }
    match &stmts[1] {
        Stmt::Let { expr: Expr::Cast { expr, to, .. }, .. } => {
            assert_eq!(*to, Type::Float);
            match &**expr {
                Expr::Call { name, args, .. } => {
                    assert_eq!(name, "len");
                    assert_eq!(args.len(), 1);
                }
                other => panic!(".length should lower to float(len(..)): {other:?}"),
            }
        }
        other => panic!(".length should lower to float(len(..)): {other:?}"),
    }
}

#[test]
fn ts_parse_object_literal_needs_annotation() {
    let p = prog("interface P { x: number }\nfunction main(): number {\n  const p: P = { x: 1 };\n  return p.x;\n}");
    match &p.funcs[0].body.stmts[0] {
        Stmt::Let { ty: Some(Type::Struct(name)), expr: Expr::StructLit { name: lit, fields, .. }, .. } => {
            assert_eq!(name, "P");
            assert_eq!(lit, "P");
            assert_eq!(fields.len(), 1);
        }
        other => panic!("expected struct literal, got {other:?}"),
    }
    let e = parse(0, "function main(): number {\n  const q = { x: 1 };\n  return 0;\n}");
    assert!(e.is_err(), "unannotated object literal must be rejected");
}

#[test]
fn ts_parse_slice_diagnostics() {    let cases: &[(&str, &str)] = &[
        ("import * as ns from \"y\";", "namespace"),
        ("export default a;", "default exports"),
        ("class C {}", "class"),
        ("var x = 1;", "var"),
        ("const t = 1;", "top-level"),
        ("function f(): number { const n = null; return 0; }", "null"),
        ("function f(): number { g.h(); return 0; }", "method calls"),
        ("function f(): number { g<number>(1); return 0; }", "explicit type arguments"),
        ("function f(): number { for (let i = 0; i < 2; i++) { continue; } return 0; }", "continue"),
        ("function f(): number { let x = a?.b; return 0; }", "S2 type layer"),
        ("function f(): number { let x = 1n; return 0; }", "BigInt"),
        ("function f(a: number[], b: string[]): number { return 0; }", "multiple array parameters"),
        ("function f(): number[] { return [1]; }", "array return"),
    ];
    for (src, want) in cases {
        let e = parse(0, src).expect_err(&format!("should reject: {src}"));
        assert!(e.message.contains(want), "for {src:?} want {want:?} got {:?}", e.message);
    }
}

/// the ternary was on that list until v0.51.0, when the Aoxn conditional
/// expression gave it a node to lower into: `c ? a : b` is the same shape as
/// `a if c else b` with the operands in the other order.
#[test]
fn ts_ternary_lowers_to_the_conditional_expression() {
    let p = parse(0, "function f(): number { return a ? b : c; }").expect("the ternary now parses");
    let f = &p.funcs[0];
    let Stmt::Return { expr: Some(e), .. } = &f.body.stmts[0] else {
        panic!("expected a return statement");
    };
    let Expr::IfExp { then_expr, cond, else_expr, .. } = e else {
        panic!("expected IfExp, got {e:?}");
    };
    assert!(matches!(&**cond, Expr::Var { name, .. } if name == "a"));
    assert!(matches!(&**then_expr, Expr::Var { name, .. } if name == "b"));
    assert!(matches!(&**else_expr, Expr::Var { name, .. } if name == "c"));
}

#[test]
fn ts_parse_generic_array_signature() {
    let p = prog("function maxOf<T>(xs: T[]): T {\n  return xs[0];\n}");
    let f = &p.funcs[0];
    // the array length becomes the pipeline's single generic length param
    assert_eq!(f.type_params, vec!["T".to_string(), "N".to_string()]);
    assert_eq!(f.len_param.as_deref(), Some("N"));
    assert_eq!(
        f.params[0].ty,
        Type::Array { elem: Box::new(Type::Struct("T".into())), len: aoxn::ast::GENERIC_LEN }
    );
    assert_eq!(f.ret, Type::Struct("T".into()));

    // `Array<T>` spelling and `number[]` share the same lowering
    let p = prog("function head(xs: Array<number>): number {\n  return xs[0];\n}");
    assert_eq!(p.funcs[0].type_params, vec!["N".to_string()]);
    assert_eq!(p.funcs[0].len_param.as_deref(), Some("N"));
}

// ---- end to end: TS source -> native exe -> output ----

fn build_and_run_ts(src: &str) -> String {
    use std::process::Command;
    // Unique dir per call. The harness runs these in parallel, and the
    // wall clock alone is NOT enough: macOS' SystemTime has coarse
    // granularity, so two parallel tests could pick the same nanosecond,
    // write main.ts over each other and then run each other's binary —
    // which showed up as ts_e2e_array_param_roundtrip asserting against
    // ts_e2e_calls_strings_arrays' output. The atomic counter makes the
    // name unique regardless of clock resolution.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "aoxn-ts-{}-{}",
        std::process::id(),
        seq
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let ts = dir.join("main.ts");
    std::fs::write(&ts, src).unwrap();
    let exe = dir.join(format!("main{}", aoxn::platform::exe_ext()));
    match aoxn::build_paths_exe(&[ts.display().to_string()], &exe, true) {
        Ok(()) => {}
        Err(diags) => {
            let msgs: Vec<String> = diags.iter().map(aoxn::diag_to_string).collect();
            panic!("compile failed:\n{}", msgs.join("\n"));
        }
    }
    let out = Command::new(&exe).output().expect("run failed");
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        s.push_str(&format!("<exit {}>", out.status.code().unwrap_or(-1)));
    }
    s
}

#[test]
fn ts_e2e_hello_loop() {
    let out = build_and_run_ts(
        "function main(): number {\n\
         \x20 let total: number = 0;\n\
         \x20 for (let i = 1; i <= 10; i++) {\n\
         \x20   total += i;\n\
         \x20 }\n\
         \x20 console.log(total);\n\
         \x20 return 0;\n\
         }\n",
    );
    assert_eq!(out, "55\n");
}

#[test]
fn ts_e2e_interface_struct() {
    let out = build_and_run_ts(
        "interface Point { x: number; y: number }\n\
         function main(): number {\n\
         \x20 const p: Point = { x: 3, y: 4 };\n\
         \x20 console.log(p.x * p.x + p.y * p.y);\n\
         \x20 return 0;\n\
         }\n",
    );
    assert_eq!(out, "25\n");
}

#[test]
fn ts_e2e_calls_strings_arrays() {
    let out = build_and_run_ts(
        "function greet(name: string): string {\n\
         \x20 return \"hello, \" + name;\n\
         }\n\
         function main(): number {\n\
         \x20 const s: string = greet(\"web\");\n\
         \x20 console.log(s);\n\
         \x20 console.log(s.length);\n\
         \x20 let sum = 0;\n\
         \x20 for (const v of [1, 2, 3]) {\n\
         \x20   sum += v;\n\
         \x20 }\n\
         \x20 console.log(sum);\n\
         \x20 return 0;\n\
         }\n",
    );
    assert_eq!(out, "hello, web\n10\n6\n");
}

#[test]
fn ts_e2e_while_if_ops() {
    let out = build_and_run_ts(
        "function main(): number {\n\
         \x20 let n = 0;\n\
         \x20 let acc = 0;\n\
         \x20 while (n < 5) {\n\
         \x20   n += 1;\n\
         \x20   if (n % 2 == 0 && n != 4) {\n\
         \x20     acc += n;\n\
         \x20   }\n\
         \x20 }\n\
         \x20 console.log(acc);\n\
         \x20 return 0;\n\
         }\n",
    );
    assert_eq!(out, "2\n");
}

#[test]
fn ts_e2e_generic_functions() {
    let out = build_and_run_ts(
        "function maxOf<T>(xs: T[]): T {\n\
         \x20 let m = xs[0];\n\
         \x20 for (const x of xs) {\n\
         \x20   if (x > m) {\n\
         \x20     m = x;\n\
         \x20   }\n\
         \x20 }\n\
         \x20 return m;\n\
         }\n\
         function main(): number {\n\
         \x20 console.log(maxOf([3, 1, 4, 1, 5]));\n\
         \x20 console.log(maxOf([2, 7, 1]));\n\
         \x20 return 0;\n\
         }\n",
    );
    assert_eq!(out, "5\n7\n");
}

#[test]
fn ts_e2e_template_strings() {
    let out = build_and_run_ts(
        "function main(): number {\n\
         \x20 const n: number = 42;\n\
         \x20 const who: string = \"web\";\n\
         \x20 console.log(`answer=${n} ok`);\n\
         \x20 console.log(`hello ${who}, n=${n * 2}!`);\n\
         \x20 return 0;\n\
         }\n",
    );
    assert_eq!(out, "answer=42 ok\nhello web, n=84!\n");
}

#[test]
fn ts_e2e_array_param_roundtrip() {
    let out = build_and_run_ts(
        "function scale(xs: number[], k: number): number[] {\n\
         \x20 for (let i = 0; i < xs.length; i++) {\n\
         \x20   xs[i] = xs[i] * k;\n\
         \x20 }\n\
         \x20 return xs;\n\
         }\n\
         function main(): number {\n\
         \x20 const a = scale([1, 2, 3], 10);\n\
         \x20 console.log(a[0] + a[1] + a[2]);\n\
         \x20 return 0;\n\
         }\n",
    );
    assert_eq!(out, "60\n");
}

#[test]
fn ts_e2e_optional_params_sentinel() {
    // `b?: number` fills with the null sentinel (0) at omitted call sites
    let out = build_and_run_ts(
        "function add(a: number, b?: number): number {
           return a + b;
         }
         function main(): number {
           console.log(add(1));
           console.log(add(1, 2));
           return 0;
         }
",
    );
    assert_eq!(out, "1
3
");
}

#[test]
fn ts_e2e_default_params() {
    let out = build_and_run_ts(
        "function greet(name: string, hi: string = \"hello\"): string {
           return hi + \" \" + name;
         }
         function main(): number {
           console.log(greet(\"ada\"));
           console.log(greet(\"ada\", \"hi\"));
           return 0;
         }
",
    );
    assert_eq!(out, "hello ada
hi ada
");
}

#[test]
fn ts_e2e_null_union_sentinel() {
    // `string | null` erases to string; `x == null` judges the "" sentinel
    let out = build_and_run_ts(
        "function f(s: string | null): number {
           if (s == null) {
             return 0;
           }
           return s.length;
         }
         function main(): number {
           const a: string | null = null;
           const b: string | null = \"hey\";
           console.log(f(a));
           console.log(f(b));
           if (b != null) {
             console.log(b);
           }
           return 0;
         }
",
    );
    assert_eq!(out, "0
3
hey
");
}

#[test]
fn ts_e2e_number_null_union() {
    // number | null: sentinel 0 (documented M1 deviation)
    let out = build_and_run_ts(
        "function f(n: number | null): number {
           if (n == null) {
             return -1;
           }
           return n;
         }
         function main(): number {
           const a: number | null = null;
           console.log(f(a));
           console.log(f(7));
           return 0;
         }
",
    );
    assert_eq!(out, "-1
7
");
}

#[test]
fn ts_e2e_tuple_values() {
    // tuples are synthesized value structs; `t[0]` reads field _0
    let out = build_and_run_ts(
        "function main(): number {
           const t: [number, string] = [1, \"one\"];
           console.log(t[0], t[1]);
           return 0;
         }
",
    );
    assert_eq!(out, "1 one
");
}

#[test]
fn ts_e2e_as_assertions() {
    // `as` lowers to checked scalar conversions
    let out = build_and_run_ts(
        "function main(): number {
           const x = 5.7;
           const i = x as int;
           console.log(i);
           console.log((i as number) + 0.5);
           const b = (x as int) as boolean;
           console.log(!b);
           return 0;
         }
",
    );
    assert_eq!(out, "5
5.5
false
");
}
