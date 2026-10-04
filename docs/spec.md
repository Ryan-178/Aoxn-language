# Aoxn Language Specification (v0.40.0)

Aoxn is an AI-native, statically typed, ahead-of-time compiled language with a
Python-style syntax. Design goals: minimal syntax, explicit semantics, native
(C++-class) speed via clang-compiled C, self-hosting core libraries, and
machine-friendly tooling (JSON diagnostics, dumpable C text).

The surface syntax tracks Python closely: the same operators, the same
control-flow shapes, the same indentation rules. Where Aoxn deliberately
differs (strict static typing, value semantics, no implicit numeric mixing)
the deviation is called out explicitly below.

## Layout rules (Python-style)

- Blocks are defined by **indentation**, not braces. Any consistent width works;
  a tab counts as 4 columns.
- Statements end at a line break; no semicolons. `;` is a syntax error.
- `#` starts a comment (to end of line). Blank and comment-only lines are ignored.
- Inside `(...)` a line break is ignored (implicit line joining) 鈥?call
  arguments may span lines, trailing commas allowed. Since v0.40.0 the same
  holds inside `{...}` (a dict literal may span lines).
- A single simple statement may follow `:` on the same line
  (`if n < 2: return n`). Compound statements (if/while/def) cannot.

## Types

| Type     | Meaning                  | Backend       |
|----------|--------------------------|---------------|
| `int`    | signed 64-bit integer    | i64           |
| `float`  | 64-bit IEEE float        | double        |
| `bool`   | `True` / `False`         | i1            |
| `string` | immutable byte string    | ptr to NUL-terminated bytes (heap when built, static when literal) |
| `void`   | absence of a value       | (function return only) |
| `[T; N]` | fixed-size array, N > 0  | [N x T]       |
| `dict[V]`| string-keyed map of V (v0.40.0) | heap handle (`struct ax_dict_V*`) |
| `T \| None` | nullable T; the only union form (v0.40.0) | carrier struct (value + tag) |
| `fn(A, ...) -> R` | function pointer (v0.40.0) | raw address |
| `Name`   | struct (see below)       | named %struct |

No implicit conversions. `int` and `float` never mix silently; `%` is
int-only, and so are the bitwise/shift operators (`&`, `|`, `^`, `~`, `<<`,
`>>`).
Integer division by zero is undefined (native crash, no runtime check - speed
first, matching C/C++). Signed integer overflow is undefined as well, since
`int` arithmetic lowers to plain C `long long` arithmetic.
Array indexing is **unchecked** (C-style).

## Arrays

```Aoxn
a = [10, 20, 30]              # inferred [int; 3]
xs: [int; 4] = [1, 2, 3, 4]   # annotated
m: [[int; 2]; 3] = [[1, 2], [3, 4], [5, 6]]   # nested
a[1] = 99                     # element assignment
print(len(a))                 # 3 (compile-time constant)
```

- Literals are non-empty; all elements must share one type (compounds allowed:
  arrays of structs, arrays of arrays).
- Indexing is 0-based with an `int`; **no bounds checking** (out-of-bounds is
  undefined behavior, like C).
- `len(x)` requires an array and returns its (static) length as `int`.

## Structs

```Aoxn
struct Point:
    x: int
    y: int

p = Point(x=1, y=2)     # construct: every field, by name, exactly once
p.x = 10                # field assignment
d = p.x + p.y           # field access
```

- Fields are declared in an indented block; each needs a type annotation.
- Structs may reference structs defined later (forward references) and may
  contain arrays/other structs. A struct **cannot contain itself**, directly
  or indirectly.
- Construction requires all fields, by name, in any order; types must match.
- Struct and function names share one namespace.

## Strings

```Aoxn
s = "hello" + ", " + "world"   # concatenation (runtime malloc + memcpy)
print(s)                       # hello, world
print(len(s))                  # 12 (bytes)
print("abc" == "abc")          # true
print("apple" < "banana")      # true (byte-wise lexicographic)
```

- `+` concatenates two strings (only strings can be concatenated).
- All six comparison operators work on `(string, string)` via byte-wise
  `strcmp` semantics; `len(s)` is the byte length.
- Strings are **immutable**. Literals live in static storage; concatenation
  results are heap-allocated and intentionally not freed (no GC yet 鈥?safe
  because strings never mutate, so sharing/aliasing is sound).
- Strings may appear in struct fields, array elements, parameters, and
  returns; copies share the underlying bytes (safe: immutability).
- No string indexing yet (returns no `char` type 鈥?roadmap).

## Loops

```Aoxn
while cond:
    ...

for i in range(n):            # 0, 1, ..., n-1
for i in range(a, b):         # a .. b-1
for i in range(a, b, step):   # step may be negative (step != 0)
for x in arr:                 # iterate array elements (each copied into x)
```

- `range(...)` takes 1..3 `int` arguments; start/end/step are evaluated once
  at loop entry (Python semantics). A zero step loops forever (undefined, not
  checked).
- `for x in arr` requires an array; the loop variable gets the element type.
  Iterating strings is not supported yet.
- The loop variable follows normal binding rules (first use declares; the
  type is fixed thereafter).
- `break` exits the innermost loop; `continue` jumps to the next iteration.
  Both are errors outside a loop.

## f-strings

```Aoxn
name = "Aoxn"
print(f"hello {name}, {1 + 2}, {True}")   # hello Aoxn, 3, true
print(f"braces: {{literal}}")             # braces: {literal}
```

- `{expr}` embeds any expression whose type is `int`, `float`, `bool`, or
  `string`; the expression may contain string literals and nested calls.
- `{{` and `}}` produce literal braces.
- f-strings desugar to string concatenation via the `str()` builtin:
  ints format as decimal, floats as `%f` (6 decimals), bools as
  `true`/`false`.

## Generic functions

```Aoxn
def sort[T, N](arr: [T; N]) -> [T; N]:
    result = arr
    for i in range(N):
        for j in range(N - 1 - i):
            if result[j] > result[j + 1]:
                t = result[j]
                result[j] = result[j + 1]
                result[j + 1] = t
    return result

sort([3, 1, 2])       # T = int,   N = 3
sort([1.5, 0.5])      # T = float, N = 2
sort(["b", "a"])      # T = string, N = 2
```

- Type parameters (`T`) and array-length parameters (`N`) are declared in
  `[...]` after the function name. At most one length parameter per function.
- Monomorphization: every distinct (type, length) combination at a call site
  produces a dedicated instance at compile time; arguments are unified
  against the declared parameter types (inferred 鈥?no explicit type
  arguments).
- The length parameter is a compile-time `int` constant inside the body
  (`range(N)`, `N - 1`).
- Operations on `T` are checked per instance: `sort` requires `<` on `T`
  (int, float, string work; structs do not).
- Generic functions cannot be `main` or `extern`.

## Value semantics (arrays and structs)

Assignment, parameter passing, and returns **copy** the whole value (lowered
to memcpy). There are no references or pointers yet:

```Aoxn
b = a          # b is an independent copy
b[0] = 99      # does not change a
data: [int; 50000] = [0] * 50000    # Python-style replication
```

## Program structure

A program is a list of `import`, `struct`, `def`, and `extern def`
declarations; execution starts at `main`.

```Aoxn
import * from "stdlib"                # the installed standard library

extern def sqrt(x: float) -> float    # C runtime function (FFI), no body

def main() -> int:
    print(sqrt(2.0))                  # 1.414214
    return 0
```

- `extern def` declares a C-runtime function: no body, resolved at link time
  from the default libraries. Extern functions cannot be named `main`, cannot
  be generic, and may only use the scalar types (`int`, `float`, `bool`,
  `string`, `void`) - aggregate parameters by value would need a struct ABI
  the `extern` surface cannot spell.
- **Imports** (W1-S3 module forms) load another file and merge it into one
  namespace:
  ```Aoxn
  import * from "./util.ax"        # whole-module merge
  import { helper, Vec } from "./util.ax"   # named (M1 merges all names)
  import main_config from "./cfg.ax"        # default import
  ```
  Paths starting `./` or `../` resolve relative to the importing file, with
  extension completion (`.ax`/`.ts`/`.tsx`, `index.<ext>`); bare names are
  resolved first as package imports under `aox_modules/` (through the
  package's `aoxn.json` `main` / `exports`), then against the **installed
  standard library** (v0.30.0), where `stdlib` means the stdlib directory
  and `stdlib/ui` a module inside it — `$AOXN_STDLIB`, else
  `$AOXN_HOME/lib/stdlib`, else the checkout, so a program written against
  an installed toolchain compiles from any directory (see
  `docs/install.md`). A project-local package always wins. Each file is
  included exactly once (canonical path); circular imports are compile
  errors. `Aoxn run main.ax` alone is enough — imports pull in
  dependencies. The bare legacy form `import "path"` was **removed** in
  W1-S3; write `import * from "path"`.
- Multiple entry files on the command line (`Aoxn build a.ax b.ax`) are also
  merged, with import resolution applied to each.

`main` returns `int` (process exit code) or `void` (exit 0).

## CSS assets (v0.34.0)

A `.css` file reached through `import` is a **build asset, not source**. It is
diverted before any front end sees it and never reaches the lexer:

```Aoxn
import * from "./style.css"       # bundled into styles()

def main() -> int:
    print(styles())               # the whole bundle, as one string
    return 0
```

- `styles() -> string` — every plain stylesheet, `@import`s inlined in order,
  comments and redundant whitespace removed.
- `styles_fingerprint() -> string` — the `<16 hex>.css` name the bundle is
  **emitted under**, stable for identical input, for `<link>` cache-busting.
- A `*.module.css` file has its class names hashed (seeded by the file's own
  path) and is **not** joined into the bundle; it generates
  `<stem>_class(name: string) -> string` instead. An unknown name is `""`.
- `url(...)` pointing at a local file is fingerprinted and rewritten to the
  emitted name; `data:`, absolute, protocol-relative and `#fragment`
  references pass through, as does a target that does not resolve (with a
  warning).
- In TypeScript, `import styles from "./page.module.css"` binds `styles` to a
  synthesized struct, so `styles.title` is a typed string read. (Aoxn has no
  function pointers, so there is no namespace object: the binding is rewritten
  at the use site into a call on the generated accessor.)

`Aoxn build --emit-assets <dir>` writes the bundle, each stylesheet and every
`url()` target beside the executable; the stdlib's `exe_dir()` /
`asset_path(styles_fingerprint())` locate it at run time, which keeps the
executable relocatable. `--tailwind` generates a documented utility subset
into the bundle.

Minification is deliberately conservative — comments and whitespace only, no
selector merging or reordering — so the emitted text is a pure function of the
source. Class rewriting is context-aware: a `.` inside a string literal, an
`@media` prelude, or a declaration value is not a selector and is left alone.

Stylesheets, `@import`ed partials and `url()` targets all participate in the
build cache. Errors report stage `asset`. See
[`css-assets.md`](css-assets.md) for the full reference.

## Bindings

```Aoxn
x = 5               # inferred: int
x: int = 10         # annotated (Python-style); annotation must match
x = x + 5           # re-assignment keeps the declared type
```

- The first binding declares the variable; its type is fixed from the
  initializer (or checked against the annotation).
- Re-assignment must keep the declared type. Re-declaring with a different
  type is an error. No shadowing within a function (parameters included).
- `print(1)` prints `1`; `print(True)` prints `true` / `false` (readable).

## Augmented assignment

The Python assignment operators are accepted on every assignment target:

```Aoxn
x = 5
x += 3          # x = x + 3
x -= 1          # x = x - 1
x *= 4          # x = x * 4
x /= 7          # x = x / 7
x %= 3          # x = x % 3

s = "ab"
s += "cd"       # s = s + "cd"

arr = [1, 2, 3]
arr[0] += 10    # arr[0] = arr[0] + 10

p = Point(x=1, y=2)
p.x += 1        # p.x = p.x + 1
```

- `+= -= *= /= %=` desugar at parse time to the plain assignment shown on the
  right, so they never change what typecheck or codegen see.
- The rules are exactly those of the expanded form: the operand type must
  match the target, `+=` on a string concatenates, and using an augmented
  form on a name that was never bound is an error (the expansion reads the
  name before writing it).
- `x //= n` is not an operator: `//` is a division, not an assignment. Write
  `x = x // n`.

## Functions

```Aoxn
def fib(n: int) -> int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)
```

- Parameters require type annotations; the return annotation is optional
  (defaults to `void`).
- Recursion and mutual recursion are allowed; order of definition does not
  matter.

## Statements

- `if cond:` / `elif cond:` / `else:` - conditions must be `bool`.
- `while cond:` - any `bool` condition.
- `for var in range(...)` / `for var in arr:` - see Loops above;
  `break` and `continue` apply to it as well.
- `return` / `return expr`.
- `pass` - explicit empty statement.
- assignment (`x = e`, `x: t = e`, `x += e`) and expression statements.

## Expressions

```ebbnf
or    := and ("or"  | "||") and)*
and   := bitor ("and" | "&&") bitor)*
bitor := bitxor ("|" bitxor)*
bitxor:= bitand ("^" bitand)*
bitand:= eq ("&" eq)*
eq    := rel (("==" | "!=") rel)*
rel   := shift (("<" | "<=" | ">" | ">=") shift)*
shift := add (("<<" | ">>") add)*
add   := mul (("+" | "-") mul)*
mul   := unary (("*" | "/" | "//" | "%") unary)*
unary := ("not" | "!" | "~" | "-" | "+") unary | primary
primary := INT | FLOAT | STRING | True | False | FSTRING
         | IDENT ("(" args ")")? | "(" expr ")"
         | "[" expr ("," expr)* "]" | "[" "]"      # array literal / index
postfix := primary ("(" args ")" | "[" expr "]" | "." IDENT)*
```

- `and`/`or`/`not` and `&&`/`||`/`!` are synonyms. `and`/`or` short-circuit.
- `True`/`False` and `true`/`false` are synonyms.
- **Bitwise and shift operators (v0.37.0)**: `&`, `|`, `^`, `~`, `<<`, `>>`.
  They bind exactly as in C — looser than the comparisons, tighter than
  `&&`/`||` — and they are **int-only**, like `%`: there is no promotion to
  float, because Aoxn has no implicit int/float conversion at all. `<<` and
  `>>` shift by the right operand's value; a count of 64 or more, or a
  negative count, is undefined behaviour, exactly like signed overflow. There
  are no augmented bitwise forms (`&=`, `<<=`, …): write `x = x & y`.
  A single `&` or `|` is now the bitwise operator rather than a lex error;
  `&&` and `||` are unchanged.
- **Chained comparison** (Python): `a < b <= c` means `(a < b) and (b <= c)`
  and short-circuits like a plain `and`. Each link is type-checked on its
  own, so a chain mixing types fails at the offending link. The middle
  operands are evaluated twice by the desugaring; that is only observable if
  a middle operand is a call with side effects, in which case hoist it into a
  variable first.
- **Unary `+`** is the identity on a numeric operand, as in Python.
- **`//`** is Python's integer-division spelling. Aoxn's `/` already truncates
  on two `int`s, so `//` and `/` are the same operator there. **Deviation
  from Python:** both truncate toward zero, so `-7 // 2` is `-3`, whereas
  Python floors to `-4`. `//` requires two `int` operands (like `%`), while
  `/` accepts `int` or `float`.
- `[e] * n` is array replication; it is a parse-time form, not a general
  multiplication of arrays.

## Semantics rules (strict, AI-verifiable)

- Every `if` / `while` condition must be `bool`.
- A non-void function must return a value on **all** paths (`if/elif/else`
  where every branch returns satisfies this). A `raise` counts as an exit —
  except inside a try body, where the handler decides.
- No unreachable statements (rejected at compile time).
- A name may be declared only once; annotations on re-assignment must match.

## Builtins

- `print(expr)` 鈥?one `int`, `float`, `bool`, or `string` (no arrays/structs);
  prints a trailing newline. Floats print with `%f` (6 decimals).
- `len(x)` 鈥?array: static length; string: byte length; `dict[V]` (v0.40.0):
  entry count. Returns `int`.
- `str(x)` 鈥?convert `int`/`float`/`bool`/`string` to `string`.
- `to_int(x)` / `to_float(x)` 鈥?explicit scalar conversions (truncating
  toward zero / widening; `bool` converts through 0/1). These are the only
  int/float mixing the language allows, and what the TS front end's
  numeric tower lowers through.
- `dict_has(d: dict[V], k: string) -> bool` /
  `dict_del(d: dict[V], k: string) -> bool` (v0.40.0) 鈥?membership and
  removal; `dict_del` is `False` when the key was absent. Both mutate
  through the dict's handle.
- `to_int(f)` on a function value (v0.40.0) is its raw address; `f = addr as
  fn(...) -> ...` converts back. The FFI escape hatch.

## Raw memory (unsafe, for the standard library and systems code)

Addresses are plain `int` (pointer-sized). These builtins are the escape
hatch that lets the standard library implement growable containers and file
IO in Aoxn itself; they perform no checks.

- `load_i64(addr) -> int`, `store_i64(addr, v: int)`
- `load_f64(addr) -> float`, `store_f64(addr, v: float)`
- `load_u8(base, off) -> int`, `store_u8(base, off, v: int)` 鈥?`base` may be
  an `int` address or a `string` (byte access into the string)
- `as_string(p: int) -> string`, `as_ptr(s: string) -> int` 鈥?pointer
  reinterpretation

There is deliberately **no `sizeof`** and no way to take the address of a
struct value (`as_ptr` works on a `string` only). Two consequences run through
everything below: a container can only move 8-byte slots, never copy a struct
in wholesale, and nothing can be passed "by reference" implicitly.

## Error channel (v0.39.0)

v0.39.0 said "Aoxn has **no exceptions**" and shipped this channel as the
replacement. v0.40.0 added real `raise` / `try` / `except` (see
[Exceptions](#exceptions-v0400)) — this one stays, for the failures a caller
is expected to *inspect* rather than unwind past. Nothing here changed; only
its monopoly on error handling did.

A function that can fail returns an `Err`:

```
struct Err:
    code: int
    message: string
```

Because a function returns exactly one value and structs are copied on
return, a payload cannot ride along with the `Err`. It comes back through an
**out-slot** the caller owns:

```
def parse_port(s: string, out: int) -> Err:
    if not is_digits(s):
        return err_new(ERR_PARSE(), "port must be digits")
    out_set_i(out, to_port(s))
    return err_ok()

p = out_new()
e = parse_port("8080", p)
if err_is_ok(e):
    port = out_get_i(p)
out_free(p)
```

This is the only shape that survives value semantics. Mutating a struct field
inside a callee is discarded by the caller, so the out-slot, not a struct
parameter, is what makes a result visible to the caller at all.

A function with no payload just returns the `Err`; that is the whole "Result
carrying no value" case and needs no slot.

The convention is a **convention, not syntax**: the compiler does not know
`Err` exists, and a program may use any struct of the same shape. What the
stdlib provides is the vocabulary: `err_ok` / `err_new` / `err_is_ok` /
`err_is_err` / `err_code_name` / `err_or_int` / `err_or_str`, the standard
codes `ERR_NONE` ... `ERR_AGAIN` (zero-argument functions, because Aoxn has no
module-level bindings), and the out-slot helpers `out_new` / `out_set_i` /
`out_get_i` and their float and string siblings.

`Diag` remains a *compile-time* channel and is unrelated to this.

## Variable-length containers (v0.39.0)

`[T; N]` is fixed at compile time. A program that must hold "however many"
items uses the heap instead, and the stdlib ships what that needs:

- **`Vec`** - a growable array of 8-byte slots. Ints, bools (as 0/1) and
  string pointers live in a slot directly. Growth is doubling; `vec_reserve`
  grows to an explicit count and never shrinks.
- **Strings as elements** - `vec_push_str` / `vec_get_str` / `vec_set_str` /
  `vec_index_of_str`. A `string` *is* a pointer, so this stores a slot rather
  than a copy; the bytes behind it are immutable, which is what makes it
  safe.
- **`VecVec`** - a vector whose elements are themselves vectors, each stored
  as three consecutive slots (data, len, cap). This is the shape a document
  format needs: a JSON array of arrays, a table of variable-length rows.
  Nesting it again gives arbitrary depth with no new machinery.

What this deliberately does **not** give you is a container of structs. Since
there is no `sizeof` and no address-of-struct, "a list of `Point`" has to be a
list of slot-tuples (an x slot, a y slot) or a vector of pointers to
individually allocated records. That is a real cost, which is why it is
documented here rather than hidden.

Indexing into any of these is unchecked, exactly like array indexing in the
language. `vec_pop` and `vec_truncate` return a new value and leave the
allocation alone.

**Aliased handles are the sharp edge.** A `Vec` is a value, but the three
fields describe a heap block that two values can point at. `vec_truncate` and
`vec_clear` both return a new `Vec` over the *same* buffer, so after

```
short = vec_clear(v)
```

`short` and `v` are two handles on one allocation: pushing to either is fine
but invisible to the other, and **freeing both is a double free** that corrupts
the heap at some unrelated later allocation. Free exactly one of them.

The second half of the edge is **reallocation**. Growth goes through `realloc`,
which frees the old block, so if one copy grows a `Vec` that another copy still
holds, the other copy is left pointing at freed memory — a use-after-free that
may read as a plausible value for a long time before it faults. Value semantics
make this easy to write by accident, because passing a `Vec` to a function hands
over a *copy of the handle*:

```
def add_to(v: Vec, x: int) -> int:      # WRONG: returns no handle
    v = vec_push(v, x)
    return v.len

n = add_to(path, 1)      # `path` now dangles if the push reallocated
```

The rule: **whenever a callee may grow a `Vec` it was given, the caller must
get the handle back.** Return it (`-> Vec`), return it inside a result struct,
or take the `Vec` as a field of a state struct and use the write-back
discipline (`c = f(c)`). A function that pushes and then recurses cannot
return a bare `int` for exactly this reason. This is the same aliasing
discipline the rest of the raw-memory section already imposes, and it is the
price of not having references.

## Exceptions (v0.40.0)

v0.39.0 said "no exceptions, on purpose" and shipped the `Err`-value channel
below. v0.40.0 reverses that decision (user instruction, 2026-10-04): a
program can now **raise** and **catch**, and the `Err` channel stays as the
way a *library* reports a failure its caller is expected to inspect. The
division of labor: `raise` is control flow for a flow the program cannot
continue; `Err` is a value for a failure the caller decides about.

```
def risky(n: int) -> int:
    if n < 0:
        raise "negative input: " + str(n)
    return n * 2

try:
    print(risky(-3))
except as e:
    print("caught: " + e)     # e is a string
```

- The payload is a `string`. `raise` stores it and unwinds to the **nearest
  enclosing handler**; with none, the function unwinds to its caller, which
  repeats the check; an exception that leaves `main` prints one report and
  exits with status 1.
- A `raise` inside a try **body** is caught by that try (so `try: ... raise
  "x" ... except:` continues at the handler). A raise inside a **handler**
  propagates outward — re-raise is how a handler adds context.
- `except:` without `as` discards the message.
- `raise` counts as an exit for the all-paths-return rule — except in a try
  body, where the handler decides.
- The runtime is two statics (a pending flag + the message), a `goto` to the
  handler label or the frame's unwind label, and a slot check after every
  statement that called a raiser. Which calls may raise is a fixpoint over
  function names; an indirect call through a function pointer always counts.
- **Known wart**: the check runs after the statement, so
  `print(f(x))` with a raising `f` prints the unwound frame's zero return
  value before the hop. Hoisting raising arguments out of the side-effecting
  call is future work (it must be mirrored in the self-hosted emitter too).

## Nullability: `None` and `T | None` (v0.40.0)

`None` is a keyword and `T | None` is the **only** union form. It is spelled
like a type union but it is a carrier: a value of `T` plus a presence tag,
copied by value like any struct.

- Widening `T -> T | None` (and `None -> T | None`) is implicit; no other
  union, and no implicit narrowing. `print` and every operator reject a
  nullable until it is narrowed — there is no truthiness and no `??`.
- `x is None` / `x is not None` test and **narrow** a bare variable: inside
  the `is not None` branch the checker and the code generator both see `x`
  as plain `T`. A branch that always returns keeps the narrowing, and an
  assignment inside the branch re-narrows the view (`x = None` narrows to
  None, `x = x + 1` re-tags the slot).
- Array literals infer through it: `[1, None, 3]` is `[int | None; 3]`.
- `x == None` is rejected — comparison operators want non-nullables; the
  test is `is`.

## Function pointers (v0.40.0)

A function with no parentheses in value position **is** its address.

```
def twice(x: int) -> int:
    return x * 2

cb = twice          # cb: fn(int) -> int
print(cb(21))       # indirect call
p = to_int(cb)      # raw address — the FFI escape hatch
f = p as fn(int) -> int   # and back (a COM vtable slot, a Win32 callback)
```

- The annotation is `fn(A, ...) -> R`. Signatures are part of the type: two
  pointers of different signatures are different types.
- Pointers are ordinary values — they copy, and they live in arrays and
  struct fields.
- `extern def` declarations reject fn-pointer parameters (the ABI has no
  closure representation to pass through).

## Dictionaries (v0.40.0)

`dict[V]` is a string-keyed map with values of one type V.

```
d: dict[int] = {"a": 1, "b": 2}
d["a"] = 10                 # insert / overwrite
print(d["a"])               # read — a missing key RAISES
print(len(d))               # entry count
print(dict_has(d, "a"))     # membership, the guard for speculative reads
print(dict_del(d, "a"))     # remove; False when the key was absent
for k in d:                 # walks KEYS, in insertion order
    print(k)
```

- Keys are `string` only; all values share one type; `{}` cannot infer its
  value type and needs the annotation (`d: dict[int] = {}`).
- A missing key **raises** `dict key not found` — that is what the exception
  channel is for. Guard a speculative read with `dict_has`, or wrap it in
  `try`.
- The dict is a **heap handle** (`struct ax_dict_V*`), the same shape as the
  UI toolkit's `TableModel` and the web server's `FileTable`: copying the
  handle shares the dict, so a mutation made through a callee's copy is
  visible to the caller — including growth and `dict_del`. Lookup is a
  linear scan over insertion order; growth doubles, so inserts amortize to
  O(1) while the honest shape of the type stays "a settings map, not a
  database".

## Platform query

- `target_os() -> string` 鈥?compile-time platform query, folded to a
  module-internal string constant by the compiler (it is **not** a runtime
  syscall). Both compilers (Rust and self-hosted) resolve it the same way, so
  the value is stable within a build and identical between compilers. Aoxn
  targets Windows, so on a supported build it is always `"windows"` (an
  unsupported host folds to `"other"` rather than lying). The builtin stays
  because it is part of the language: source that branches on it keeps
  working, and the self-hosting fixed point depends on both compilers folding
  it identically.

The stdlib builds on these: `struct Vec` (growable 8-byte slots:
`vec_new`/`vec_push`/`vec_get`/`vec_set`/`vec_free` - write-back style,
`v = vec_push(v, x)`), byte buffers, `read_file`/`write_file`, and
`system(cmd)` for process spawning. The UI toolkit is an immediate-mode GUI
in three files (see `docs/ui.md`): `stdlib/ui.ax` (portable core) +
`stdlib/ui_draw.ax` (the platform-neutral widget layer) + the Win32/GDI
backend `stdlib/ui_win.ax`. A program reaches every widget through one import
(`import * from "stdlib/ui_win"`, linked with `-l user32 -l gdi32`); the
widget layer names no platform symbol at all.

## Tooling contract (AI-native)

- `Aoxn build file.ax [-o out] [--O0|--O1|--O2|--O3]` — native executable
  (O3 default: the level selects the clang `-O` used to compile the generated C).
- `Aoxn run file.ax [-- args...]` — compile and run.
- `Aoxn c file.ax` — print the generated C text (v0.29.0: the C-emitting
  backend is the only backend; `Aoxn ir` is kept as a deprecated alias).
- `Aoxn doctor [--json] [--no-smoke]` (v0.30.0) — report the install root,
  the stdlib, the resolved clang and its version, then compile and run a
  one-line program that imports the stdlib; exit code 0 means the toolchain
  works. `--json` is the machine-readable form.
- `Aoxn version` — the compiler version.
- Optimization levels: `--O3` (default), `--O2`, `--O1`, `--O0` select the
  clang `-O` level used to compile the generated C. The **C text itself is
  level-independent** - the level only picks compiler flags. `--O1` roughly
  halves compile time on large inputs and is recommended for iteration and
  compile-time-sensitive CI (inlining-heavy code is slower at runtime, loop
  code is unaffected).
- The C backend is the only backend (`--backend c` is accepted for
  compatibility). There is no IR, no pass pipeline, and no `nsw`/`nuw`
  refinement: `AOXN_PASSES`, `AOXN_DUMP_IR`, `AOXN_BACKEND` and `AOXN_CG_TRACE`
  are **dead** since v0.29.0 and silently ignored.
- `Aoxn run` and `Aoxn build` share one content-hash cache of the built
  executable: the key covers the content of the entry file *and all
  transitive imports*, plus the compiler binary, every codegen-affecting
  option (`--O*`, `--cpu`, `AOXN_CPU`, `-l`/`-L`, resolved clang path) and the
  output-relevant link flags. `target/cache` holds the entries
  (`AOXN_CACHE_DIR` to relocate, `AOXN_NO_CACHE=1` to disable, 64-entry
  approximate LRU). Re-running or re-building an unchanged program skips
  compile and link (`run` executes the cached exe; `build` copies it to the
  `-o` destination). Any source or option change is a miss; output is
  identical to a fresh compile either way.
- `--json` - diagnostics as `{"ok":false,"errors":[{"stage","line","col","message"}]}`.
- `AOXN_DUMP_C=1` - dump the generated C to stderr; `AOXN_TIME=1` - per-phase
  wall clock; `AOXN_TC_TRACE=1` - per-function typecheck markers;
  `AOXN_CLANG=<path>` - select the clang executable.

Diagnostics stages: `lex`, `parse`, `type`, `internal`, `link`, `io`.

## Platform support

| Platform | Status | Toolchain |
|----------|--------|-----------|
| Windows x86_64 | the only supported target | MSVC Build Tools + clang (winget LLVM provides it) |

Aoxn is a Windows-only language as of v0.30.0: one platform, one installer
(`Aoxn-<version>-Setup.exe`), one CI job, one UI backend. macOS and Linux
support, with the X11 UI backend and the POSIX web server modules, was
removed rather than left to drift — see `docs/platform-support.md` for what
went and what a port would need.

Since v0.29.0 the compiler has no LLVM dependency: codegen emits ISO C and
clang compiles it (`AOXN_CLANG`, `PATH`, or the toolchain's own
`toolchain/bin` locate the driver).

## Performance

Aoxn lowers to ISO C and compiles with clang O3 to native machine code —
measured at parity with `clang -O3` on identical algorithms (see
`examples/bench_*.ax`):
loop sum, array scan, struct copies, for-loop iteration, and string
build/compare all land within 卤15% of clang.

## Standard library

`stdlib/stdlib.ax` is written **in Aoxn itself** and compiled together with
the program: math (`abs/min/max/clamp/pow_i/gcd/lcm/isqrt/is_prime/hypot`
+ `sqrt`/`floor`/`ceil` via FFI), and generic search/sort/aggregation:
`sort`, `linear_search`, `binary_search`, `max_of`, `min_of`, `reverse`,
`sum_int`, `sum_float` 鈥?all parameterized by element type and length.

## Roadmap

Ordered by how much they cost the language's Python parity.

1. Remaining Python operators and forms: `**` (right-associative power),
   `in` / `not in` as operators (array membership, string substring).
   (`is` / `is not` shipped in v0.40.0 for the `None` narrowing.)
2. Conditional expressions (`a if c else b`) and multiple assignment /
   unpacking (`a, b = f()`), which need tuple or multi-target support in the
   AST.
3. Default parameter values (`def f(x: int = 3)`).
4. Module qualification (`lib.sort(...)`), selective imports, package layout.
5. String indexing / iteration (needs a `char` type or substring slices);
   f-string format specifiers (`{x:.2f}`) and multi-line f-strings.
6. Memory: string interning or arena freeing (currently concatenation leaks);
   dict values are never freed either (a dict lives as long as the program,
   like every other heap block in the language).
7. Standard library expansion: containers, IO, crypto — the module-by-module
   plan (all 31 Python stdlib modules, batched and feasibility-rated) is in
   `docs/stdlib-todo.md`; its §0 lists the language-level constraints each
   module runs into (the self-host critical path, argv, the int-only `Vec`).
8. Top-level statements as an implicit `main` (module-script mode).
9. Larger Python features, each of which is a real language design (not just
   syntax): `with`, generators and `yield`, closures and decorators, classes,
   set literals. (v0.40.0 took four off this list: function pointers, `None`
   optional types, `try`/`except`, and dict literals.)

