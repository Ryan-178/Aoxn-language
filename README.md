# Aoxn

## English

### Project and current scope

Aoxn is a statically typed native language with Python-style indentation. The
Rust compiler emits ISO C99 and invokes external clang. The compiler library
`src/` has no external Rust crate dependencies or LLVM library dependency. The
independent package manager has dependencies pinned in `Cargo.lock`.

**Compiler 0.55.0; package-manager crate 0.42.0.** The latest versioned
[changelog](CHANGELOG.md) entry fixes exponentiation precedence: `-2 ** 2` is
`-4`. Unreleased work optimizes exception propagation/module/struct-cycle
handling, ports None/dict/function-pointer/exception support into the
self-hosted compiler, extends the Aoxn-written CLI (run/symbols/doctor/JSON/
cache/options), and adds the CSV, random and socket stdlib modules. This does
not introduce C source compatibility. See [C compatibility](docs/c-compatibility.md).

```python
import * from "stdlib"

def square(x: int, scale: int = 1) -> int:
    return x ** 2 * scale

def main() -> int:
    for value in [2, 3, 5]:
        print(value, square(value))
    return 0
```

### Installation, platforms and commands

Windows is first-class, Linux is supported, macOS is unsupported. The
[platform policy](docs/platform-support.md) is authoritative. Windows ships
`Aoxn-<version>-Setup.exe` from the [release page](https://github.com/AlonechatWorkspace/Aoxn-language/releases).
It unpacks into `%LOCALAPPDATA%\aoxn`, updates PATH and runs `aoxn doctor`.
**Nothing is downloaded by default**; `-InstallClang` explicitly opts in.
clang is external; Windows linking needs MSVC-compatible build tools. Unsigned
binaries can be quarantined by OS policy; see [installation](docs/install.md).

Build the compiler with Rust. Compile native Aoxn programs with clang as well.
Linux GUI programs need X11/Xft/fontconfig development libraries.

```powershell
cargo build --release
cargo run -- run examples/hello.ax
cargo run -- check examples/fib.ax
cargo run -- c examples/fib.ax
cargo run -- build examples/fib.ax -o fib.exe
cargo run -- symbols examples/ui_gallery.ax --json
cargo run -- doctor --json
cargo run -- version
```

On Linux use an output such as `-o fib`. `check`, `c` and `symbols` do not invoke
clang. `--json` provides machine-readable diagnostics. `--O0` through `--O3`
select clang optimization (default O3), not different C text. `--clang-arg`, `-g`
and `--cc-warnings` expose clang options, debug information and warnings.

`run`/`build` share a content-keyed cache. `AOXN_NO_CACHE=1` disables it;
`AOXN_CACHE_DIR` relocates it. `AOXN_CLANG` selects clang; `AOXN_HOME` and
`AOXN_STDLIB` select installed toolchain/library paths. `AOXN_DUMP_C=1` prints
C, and `AOXN_TIME=1` reports lex/parse/resolve/typecheck/codegen/link phases.

### Language, ABI and self-hosting

- Strict types, explicit int/float conversion, bool-only conditions, checked
  returns and unreachable statements. Defaults, generics, conditional expressions,
  multi-value print, namespaces and selective/star imports are available.
- Structs and fixed arrays copy by value. Strings are immutable NUL-terminated
  bytes; string bounds clamp, while fixed-array indexing is unchecked.
- Dicts share heap handles and preserve insertion order with linear lookup.
  Nullable narrowing, function pointers and string-payload exceptions are supported
  by the Rust compiler. Concatenation intentionally leaks; there is no GC.
- C FFI and raw-memory builtins require correct caller contracts. Aoxn `int`
  maps to `long long`, not arbitrary C `int`; bool maps to C `int`, not a promise
  of `_Bool` compatibility. Externs are fixed-arity, not variadic. Internal
  aggregate ABI is not arbitrary C struct-by-value extern compatibility.
- The TypeScript front end is a supported subset, not full TS/JavaScript.
  Closures, objects and GC remain gaps; see the [TS plan](docs/ts-m1-spec.md).

The self-hosted compiler now covers the v0.40.0 surface — `None`/`T | None`,
`dict[V]`, function pointers and `raise`/`try`/`except` — with C that matches
the Rust emitter byte-for-byte on the tested common subset. Programs without
those features keep their previous C. Its stage fixed point compares
self-hosted driver products across stages (masking Windows COFF timestamps) and
remains independent of the direct Rust-vs-selfhost emitter comparison; neither
finite corpus proves all-feature equivalence.

`selfhost/cli.ax` is a real Aoxn implementation of help, version, check, c,
build, run, symbols and doctor. `run` passes an exact argv vector to the child
(Windows CRT spawn, POSIX fork/exec — no shell), preserves empty/spaced/special
arguments and returns the child exit code. `-o`, `--O0..--O3`, `-l/-L`, `--cpu`,
`--backend c`, `--clang-arg`, `-g`, `--cc-warnings` and `--json` reach
codegen/link; JSON reports (success and error) go to stderr; W001, a
content-keyed run cache (`AOXN_NO_CACHE`/`AOXN_CACHE_DIR`) and
`AOXN_STDLIB`/`AOXN_HOME` resolution are implemented.

Still outside the selfhost CLI, and rejected rather than forwarded: package
commands, CSS assets/`--emit-assets`/`--tailwind`, and namespace/selective
imports. Build caching, full doctor rows, imported-symbol file attribution and
exact diagnostic spans are incomplete; Linux-native execution was not run this
round. The Rust CLI remains the production entry point. Read
[C compatibility](docs/c-compatibility.md), the [spec](docs/spec.md),
[language card](AGENTS.md) and [gaps](docs/language-gaps.md).

### Libraries and tools

Explicitly imported [stdlib modules](docs/stdlib.md) include math, time,
datetime, calendar, pathlib, base64, hashlib, hmac, os, glob, JSON, bisect,
heapq, CSV, random and socket. Core containers/I/O remain in
`stdlib/stdlib.ax`. CSV is RFC 4180 with value errors, not the full Python
dialect; random is deterministic MINSTD and explicitly NOT cryptographically
secure; socket is blocking IPv4 TCP (no DNS/UDP/IPv6/TLS) with Windows
`-l ws2_32`, and its Linux branch is implemented but not natively executed in
this round. The [UI](docs/ui.md) shares widgets across Win32/GDI and Linux
X11/Xft. The [OpenAI](docs/openai-sdk.md) and
[Anthropic](docs/anthropic-sdk.md) SDKs share JSON/HTTP/SSE; WinHTTP transport
is Windows-only.

The [package manager](docs/pkg-manager.md) uses `aoxn.json`, `aoxn.lock` and
`aox_modules/`, with init/add/remove/install/update/list/freeze/tree/why/audit/
trust/publish/yank/cache/npm-import commands and direct aliases. Registries are
directories, git or read-only plain HTTP. npm-import requires Aoxn metadata,
not arbitrary JS. Warm reuse rechecks content hashes, excluding only its own
`index.ax` shim. [CSS assets](docs/css-assets.md) include CSS Modules,
minification, URL handling and `--emit-assets`; built-in `--tailwind` is a subset.
The [IDE](docs/ide.md) is a Tauri/Next.js/Monaco application with a
ZCode-derived Zai design system, assistant, terminal, source-control and
preview panes; separately tested.

### Repository maintenance and CI scope

Main CI explicitly runs `cargo test -p aoxn --locked` and
`cargo test -p aoxn-pkg --locked` on Windows/Linux: the root non-virtual workspace
otherwise defaults to only `aoxn`. A separate named step reruns
`--test c_emitter_parity --test c_abi` with `AOXN_TEST_REQUIRE_CLANG=1`; missing
clang fails that gate instead of silently skipping the comparisons. Linux also
runs the X11 probe under Xvfb. Release packaging/install smoke runs on tags or
manual dispatch, not every push;
web benchmarks and IDE checks are separate. Windows smoke commands now check
native exit codes immediately. Release notes match the installer's opt-in clang
download behavior.

Current specifications and contributor instructions have been reconciled with
the C backend; LLVM-era reports retain historical data with explicit boundaries.
`wiki/` stays frozen. The [repository audit](docs/repository-audit.md) records
artifact provenance without deleting unexplained files. Run the dependency-free
read-only checks with Python:

```text
python tools/test_check_doc_links.py
python tools/check_doc_links.py
```

The path auditor is warning-only by default; `--strict` treats all findings as
failures. Command hints are heuristic, not confirmed bugs. It does not fetch
links, execute documented commands, validate anchors or replace a security audit.

### Validation and evidence

```powershell
$env:AOXN_CLANG = "E:/llvm/bin/clang.exe"  # choose an existing clang
$env:AOXN_TEST_REQUIRE_CLANG = "1"
cargo test -p aoxn --locked --no-fail-fast
cargo test -p aoxn --locked --test c_emitter_parity --test c_abi
cargo test -p aoxn-pkg --locked
cargo check --workspace --tests --target x86_64-unknown-linux-gnu
```

Final Windows validation for the current tree (2026-11-07, clang 23.1.2):
**357 root + 105 package = 462 passed, 0 failed/ignored**; the complete log is
`target/final-test.log`. Nineteen full-C comparison inputs, 22 rejection
fixtures and three separately pinned known gaps are distinct categories, not
parity successes. The four ABI tests (O0/O3), both parity targets, the original
fixed point, both CLI stages, real socket loopback (Rust and selfhost builds),
random cross-process vectors and the Python `csv` differential all ran.
`cargo check --workspace --tests --target x86_64-unknown-linux-gnu` passed;
Linux native linking/runtime was not executed. Reruns are not added to the
counts. IDE tests and full installer packaging were not run for this batch.

**Dependency security, 2026-11-01**: the two JS manifests were re-audited and
upgraded — `ide` to `next` 16.4.3 plus dompurify 3.4.17, `web/next-app` to
`next` 15.5.28 — closing seven advisories; `pnpm audit` now reports no known
vulnerabilities in either, with 62/62 IDE frontend tests, `pnpm typecheck`, both
`pnpm build`s and a `next start` route smoke passing. The one remaining Rust
advisory (glib, `RUSTSEC-2024-0429`) stays accepted in `ide/src-tauri/audit.toml`
with a dated re-check. No compiler suite re-ran for it: nothing in `src/`,
`tests/`, the Aoxn sources or any `Cargo.lock` changed.

**IDE rework, 2026-11-07**: the IDE moved to a ZCode v3.17.2-derived Zai
design system and gained assistant, terminal, source-control and preview
panes. Validation: `pnpm --dir ide typecheck`, **64/64 IDE frontend tests**,
`pnpm build`, **43 IDE Rust tests** and `pnpm audit` (no known
vulnerabilities) all pass; the git porcelain parser and preview MIME map
have dedicated Rust tests. No compiler suite re-ran — no compiler source
changed.

**Linux CI, 2026-11-01**: one `aoxn-pkg` test was Windows-only in its expectation.
The archive entry `demo/C:/Users/victim/…` is an absolute path on Windows and an
ordinary directory named `C:` on POSIX, where extraction keeps it inside the
destination — so the guard accepted what the test demanded it refuse. The test now
derives its absolute spelling from `temp_dir()` and both hosts assert one rule; a
second test pins the divergence (refused here, contained there, never escaping).
The guard's own code is unchanged. Verified: **105/105 `aoxn-pkg` lib tests** on
Windows, `cargo check --workspace --tests --target x86_64-unknown-linux-gnu`, and
the hosted CI run on `main` — **Windows and Linux both green**, where the Linux
`Test package manager` step no longer aborts, so `Smoke test` and the X11 probe
under Xvfb ran again for the first time since the streak turned red. The Linux
test binary was not executed on this machine (no WSL or Docker); CI supplied that.
Details: `CHANGELOG.md`.

Historical baseline **8f90aca** recorded 343 root + 104 package tests (447), with
explicit clang 23.1.2 on Windows. This is a dated baseline, not a live count of
the modified tree. The [performance report](docs/p2-compiler-performance.md)
records exact comparisons and failures: the 1600-struct synthetic chain improved
from 602.866 to 2.398 ms typecheck median, but real workloads do not establish a
general end-to-end speedup. Wrapped/array indirect struct cycles remain a known
checker limitation.

| Location | Purpose |
|---|---|
| `src/`, `tests/` | compiler, CLI, installer and regression suites |
| `selfhost/` | self-hosted compiler subset |
| `stdlib/`, `examples/` | libraries and examples |
| `crates/aoxn-pkg/` | independent package manager |
| `ide/`, `web/` | IDE and Windows web benchmarks |
| `dist/package.ps1` | Windows installer packaging |
| `tools/compiler_perf.py` | clang-free interleaved measurements |
| `docs/` | maintained specifications and evidence |
| `wiki/` | frozen historical documentation |

Contribute under [AGENTS.md](AGENTS.md); report vulnerabilities privately under
[SECURITY.md](SECURITY.md). License: [Apache-2.0](LICENSE).

---

## 中文

### 项目与当前范围

Aoxn 是使用 Python 风格缩进的静态类型原生语言。Rust 编译器发射 ISO C99，
再调用外部 clang；`src/` 编译器库没有外部 Rust crate 或 LLVM 库依赖。
独立包管理器的依赖固定在 `Cargo.lock`。

**编译器 0.55.0，包管理器 crate 0.42.0**；以清单和
[CHANGELOG](CHANGELOG.md) 最新版本条目为准，0.55.0 修复 `-2 ** 2 == -4`。
未发布工作优化异常传播/模块/结构体环检查，把 None/dict/函数指针/异常移植进自举、
扩展 Aoxn 编写的 CLI（run/symbols/doctor/JSON/缓存/选项），并新增 CSV、random、
socket 标准库模块。不新增 C 源码兼容。见 [C 兼容边界](docs/c-compatibility.md)。

```python
import * from "stdlib"

def square(x: int, scale: int = 1) -> int:
    return x ** 2 * scale

def main() -> int:
    for value in [2, 3, 5]:
        print(value, square(value))
    return 0
```

### 平台、安装和命令

Windows 为一等平台，支持 Linux，不支持 macOS，以
[平台政策](docs/platform-support.md) 为准。[Windows 发布包](https://github.com/AlonechatWorkspace/Aoxn-language/releases)
为 `Aoxn-<version>-Setup.exe`，解包至 `%LOCALAPPDATA%\aoxn`、设置 PATH、
运行 doctor。**默认不下载**；`-InstallClang` 显式启用下载。
clang 为外部前提，Windows 链接需要 MSVC 兼容工具；未签名程序可能被系统隔离。
见[安装指南](docs/install.md)。

构建编译器需要 Rust，编译 Aoxn 程序还需 clang；Linux 图形程序另需
X11/Xft/fontconfig 开发库。

```powershell
cargo build --release
cargo run -- run examples/hello.ax
cargo run -- check examples/fib.ax
cargo run -- c examples/fib.ax
cargo run -- build examples/fib.ax -o fib.exe
cargo run -- symbols examples/ui_gallery.ax --json
cargo run -- doctor --json
cargo run -- version
```

Linux 输出可写 `-o fib`。check/c/symbols 不调用 clang；`--json` 提供结构化诊断。
`--O0` 至 `--O3` 仅选择 clang 优化级别（默认 O3），不改变 C 文本。
`--clang-arg`、`-g`、`--cc-warnings` 用于透传参数、调试信息和警告。
run/build 共享内容缓存；AOXN_NO_CACHE 禁用，AOXN_CACHE_DIR 改位置。
AOXN_CLANG 指定 clang，AOXN_HOME/AOXN_STDLIB 指定安装位置；
AOXN_DUMP_C 打印 C，AOXN_TIME 输出各编译阶段耗时。

### 语言、ABI 与自举

- 类型严格，int/float 显式转换，条件必须 bool，检查返回路径和不可达代码。
  支持默认参数、泛型、条件表达式、多参数 print、命名空间和选择性/星号导入。
- 结构体与定长数组按值复制；字符串是不可变 NUL 结束字节，字符串越界 clamp，
  数组索引不检查。dict 为保序共享堆句柄，查询线性。
- Rust 编译器支持 nullable 收窄、函数指针与异常；拼接字符串故意不释放，没有 GC。
- C FFI/原始内存依赖调用方正确约定。int 对应 long long 而不是任意 C int；
  bool 对应 C int，并不保证兼容 _Bool。extern 固定参数，不支持可变参数；
  内部聚合 ABI 不等于任意 C struct-by-value extern 兼容。
- TS 前端为子集而非完整 TS/JS；对象、闭包、GC 仍有缺口，见[计划](docs/ts-m1-spec.md)。

自举现覆盖 v0.40.0 语言面：None/`T | None`、`dict[V]`、函数指针与
raise/try/except；在已测试的共同子集上 C 与 Rust 发射器逐字节相同，不使用这些
特性的程序 C 不变。跨阶段不动点与 Rust-vs-selfhost 直接比较仍是两项独立证据，
有限语料都不证明全功能等价。

selfhost/cli.ax 是真正的 Aoxn 实现，提供 help/version/check/c/build/run/
symbols/doctor。run 以精确 argv 向量启动子进程（Windows CRT spawn、POSIX
fork/exec，无 shell），保留空串/空格/特殊字符并返回子进程退出码。`-o`、
`--O0..--O3`、`-l/-L`、`--cpu`、`--backend c`、`--clang-arg`、`-g`、
`--cc-warnings`、`--json` 进入真实编译/链接；JSON 报告（成功/错误）走 stderr；
W001、内容键 run 缓存（`AOXN_NO_CACHE`/`AOXN_CACHE_DIR`）与
`AOXN_STDLIB`/`AOXN_HOME` 解析已实现。

仍不在自举 CLI 内、且明确拒绝而非转发：包管理器命令、CSS 资源/
`--emit-assets`/`--tailwind`、命名空间/选择性导入。build 缓存、完整 doctor 行、
导入符号文件归属和精确诊断跨度仍不完整；本轮未执行 Linux 原生运行。生产 CLI 仍是
Rust。见[C 边界](docs/c-compatibility.md)、[规范](docs/spec.md)、
[语言卡](AGENTS.md)和[语言缺口](docs/language-gaps.md)。

### 库与工具

[标准库](docs/stdlib.md)独立模块提供 math/time/datetime/calendar/pathlib/
base64/hashlib/hmac/os/glob/json/bisect/heapq/csv/random/socket；核心容器与 I/O
在 stdlib/stdlib.ax。CSV 是 RFC 4180 + 值错误，不等于 Python 全部 dialect；
random 是确定性 MINSTD，**明确非密码学安全**；socket 是 blocking IPv4 TCP
（无 DNS/UDP/IPv6/TLS），Windows 需 `-l ws2_32`，Linux 分支已实现但本轮未原生
运行。[UI](docs/ui.md)共享控件层，后端为 Win32/GDI 与 X11/Xft。
[OpenAI](docs/openai-sdk.md)/[Anthropic](docs/anthropic-sdk.md) SDK 共享传输基础，
WinHTTP 为 Windows-only。

[包管理器](docs/pkg-manager.md)使用 aoxn.json/aoxn.lock/aox_modules，提供
init/add/remove/install/update/list/freeze/tree/why/audit/trust/publish/yank/cache/
npm-import 与直接别名；注册表为目录/git/只读明文 HTTP。npm-import 只收 Aoxn
元数据，不支持任意 JS。热安装复用前重算内容哈希，仅排除自身 index.ax shim。
[CSS](docs/css-assets.md)支持 Modules、压缩、URL、emit-assets，内建 Tailwind 是子集。
[IDE](docs/ide.md)为独立测试的 Tauri/Next.js/Monaco 应用，界面采用
ZCode 衍生的 Zai 设计系统，并带助手、终端、Git 与预览面板。

### 仓库维护与 CI 范围

主 CI 在 Windows/Linux 显式执行 `cargo test -p aoxn --locked` 和
`cargo test -p aoxn-pkg --locked`；非虚拟工作区裸 cargo test 默认只测试根包。
独立命名步骤以 `AOXN_TEST_REQUIRE_CLANG=1` 重跑
`--test c_emitter_parity --test c_abi`，缺少 clang 会判定门禁失败，不再静默跳过比较。
Linux 另跑 Xvfb 下 X11 probe。安装打包与冒烟在 tag/手动触发，不是每次 push；
Web 基准和 IDE 独立。Windows 冒烟逐条检查原生命令退出码，发布文案与安装器
显式启用 clang 下载的行为一致。

规范/贡献指南对齐当前 C 后端；LLVM 历史报告保留数据并标清边界，wiki 不修改。
[仓库审计](docs/repository-audit.md)记录产物来源，未盲删历史文件。
只读检查命令无需外部 Python 包：

```text
python tools/test_check_doc_links.py
python tools/check_doc_links.py
```

检查器默认仅警告，--strict 对所有发现返回失败；命令提示只是启发式，不是确认缺陷。
不访问网络、不执行文档命令、不校验锚点，也不是安全审计。

### 验证与证据

```powershell
$env:AOXN_CLANG = "E:/llvm/bin/clang.exe"  # 替换为已有 clang
$env:AOXN_TEST_REQUIRE_CLANG = "1"
cargo test -p aoxn --locked --no-fail-fast
cargo test -p aoxn --locked --test c_emitter_parity --test c_abi
cargo test -p aoxn-pkg --locked
cargo check --workspace --tests --target x86_64-unknown-linux-gnu
```

当前树 Windows 最终验证（2026-11-07，clang 23.1.2）：**357根+105包=462项通过，
0失败/忽略**；完整日志 `target/final-test.log`。19个完整 C 比较输入、22个拒绝样例、
3个独立 known gaps 分别计证，不混算为 parity。4项 ABI（O0/O3）、两项 parity、
原不动点、两阶段 CLI、真实 socket 回环（Rust 与 selfhost 构建）、随机数跨进程向量
与 Python `csv` 差分均执行。`cargo check --workspace --tests --target
x86_64-unknown-linux-gnu` 通过，未执行 Linux 原生链接/运行；重跑不重复计数。
本批未运行 IDE 测试或完整安装器打包。

**依赖安全 2026-11-01**：两份 JS 清单重新审计并升级——`ide` 到 `next` 16.4.3
加 dompurify 3.4.17，`web/next-app` 到 `next` 15.5.28，共关闭 7 项公告；两份清单
`pnpm audit` 均已无已知漏洞，IDE 前端 62/62、`pnpm typecheck`、两侧 `pnpm build`
与 `next start` 路由冒烟全部通过。唯一保留的 Rust 公告（glib
`RUSTSEC-2024-0429`）连同本次复核仍记在 `ide/src-tauri/audit.toml`。
本次未重跑编译器测试：src、tests、Aoxn 源码与任何 Cargo.lock 都未改动。

**IDE 重构 2026-11-07**：IDE 迁移到 ZCode v3.17.2 衍生的 Zai 设计系统，
新增助手、终端、Git 与预览面板。验证：`pnpm --dir ide typecheck`、
**IDE 前端 64/64**、`pnpm build`、**IDE Rust 43 项**、`pnpm audit`
（无已知漏洞）全部通过；git porcelain 解析与预览 MIME 映射有独立 Rust 测试。
未重跑编译器套件——未改动编译器源码。

**Linux CI 2026-11-01**：`aoxn-pkg` 有一项测试的预期只适用于 Windows。归档条目
`demo/C:/Users/victim/…` 在 Windows 是绝对路径，在 POSIX 只是名叫 `C:` 的普通目录，
解包会留在目标目录内——于是守卫放行的正是测试要求拒绝的。该测试改为从
`temp_dir()` 导出绝对拼法，两个平台断言同一条规则；另一项测试钉住这次分歧（本机
拒绝、他机内含，但都不逃逸）。守卫代码本身一字未改。已验证：Windows
`aoxn-pkg` 库测试 **105/105**，`cargo check --workspace --tests --target
x86_64-unknown-linux-gnu`，以及 main 上的托管 CI——**Windows 与 Linux 双双通过**；
Linux 的 `Test package manager` 不再中断，`Smoke test` 与 Xvfb 下的 X11 探测因此
重新执行并通过。本机无 WSL 与 Docker，Linux 测试二进制未本地执行，由 CI 补齐。
详见 `CHANGELOG.md`。

历史基线 **8f90aca** 在 Windows 显式 clang 23.1.2 下记录 343 根+104 包=447 项，
不是修改后实时计数。[性能报告](docs/p2-compiler-performance.md)保留具体实验与失败：
1600 结构体合成链 typecheck 中位数 602.866→2.398ms，不能推为所有项目端到端加速。
数组等包装类型间接结构体环仍为已知检查器限制。

目录：src/tests 为编译器与回归，selfhost 为自举子集，stdlib/examples 为库/示例，
crates/aoxn-pkg 为包管理，ide/web 为 IDE/Windows Web 基准；dist/package.ps1 打包，
tools/compiler_perf.py 测量编译耗时；docs 为维护文档，**wiki 冻结且不作为当前权威**。

贡献遵守 [AGENTS.md](AGENTS.md)，漏洞私报见 [SECURITY.md](SECURITY.md)。
许可证 [Apache-2.0](LICENSE)。
