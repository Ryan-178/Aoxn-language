# Aoxn

## English

### Current project

Aoxn is a statically typed, AI-native compiled language with Python-style
indentation. The Rust compiler emits ISO C, then invokes external clang to
produce native programs. It has no LLVM library dependency. The compiler
library (`src/`) has zero external Rust crate dependencies; the independent
package manager has its own dependencies pinned in `Cargo.lock`.

**Compiler 0.55.0; package-manager crate 0.42.0.** Versions come from the
current manifests and the latest released [changelog](CHANGELOG.md) entry.
v0.55.0 fixes Python exponentiation precedence (`-2 ** 2 == -4`) and adds
an agent-oriented language card in [AGENTS.md](AGENTS.md). Unreleased compiler
performance work preserves language semantics and generated C: reverse-edge
exception propagation, borrowed module tables, and now reuse of completed
acyclic-struct proofs across declaration roots.

The latest optimization removes repeated DFS of shared struct tails. A release
comparison against `82cffcd` reduced typecheck median for a synthetic 1600-struct
chain from **602.866 ms to 2.398 ms**, and `aoxn c` process wall median from
**672.730 ms to 71.759 ms**. Real selfhost/UI/stdlib workloads do not demonstrate
a general end-to-end speedup; small changes remain noise-sensitive. Method,
raw samples, limitations and reproduction instructions are in the
[compiler performance report](docs/p2-compiler-performance.md).

```python
import * from "stdlib"

def square(x: int, scale: int = 1) -> int:
    return x ** 2 * scale

def main() -> int:
    values = [2, 3, 5]
    for value in values:
        print(value, square(value))
    return 0
```

### Platforms and installation

**Windows is first-class, Linux is supported, macOS is unsupported.** The
[platform policy](docs/platform-support.md) is authoritative. Windows releases
ship one `Aoxn-<version>-Setup.exe` from the
[release page](https://github.com/AlonechatWorkspace/Aoxn-language/releases).
It unpacks into `%LOCALAPPDATA%\aoxn`, updates PATH and runs `aoxn doctor`.
It downloads **nothing by default**; `-InstallClang` explicitly opts in.
clang is external, and Windows linking requires MSVC-compatible build tools.
Unsigned binaries may be quarantined by Windows policy; see
[installation](docs/install.md) and [security](SECURITY.md).

Building the compiler requires Rust. Compiling Aoxn programs also requires
clang. Linux GUI programs additionally require X11/Xft/fontconfig development
libraries; headless compiler commands do not.

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

Use `-o fib` on Linux. `check`, `c` and `symbols` do not invoke clang.
`--json` supplies structured diagnostics. `--O0` through `--O3` select clang's
optimization level (default O3), not a different C text. `--clang-arg`, `-g`
and `--cc-warnings` expose clang arguments, debug information and warnings.

`run` and `build` share a content-keyed executable cache; `AOXN_NO_CACHE=1`
disables it and `AOXN_CACHE_DIR` relocates it. `AOXN_TIME=1` reports lex,
parse, resolve, typecheck, codegen and link phases where applicable. Use
`aoxn c` to measure Rust compilation without clang/cache work. `AOXN_CLANG`
selects clang; `AOXN_HOME`/`AOXN_STDLIB` select toolchain/library locations;
`AOXN_DUMP_C=1` prints emitted C.

### Language and boundaries

- Indentation blocks, `def`/`elif`/`pass`, `#` comments, inference, generics,
  trailing parameter defaults, conditional expressions and multi-value print.
- Strict types: no implicit int/float conversion, bool-only conditions, no
  retyping variables, checked returns and unreachable statements.
- `**` associates right; `-2 ** 2` is `-4`. `not` uses Python comparison
  precedence, while bitwise operators retain the documented C-style rules.
- Structs and fixed arrays copy by value. Strings are immutable NUL-terminated
  **bytes**, not Unicode characters. Indexing/slicing/iteration use bytes;
  string bounds clamp to `""`, but array indexing remains unchecked.
- `dict[V]` is a shared heap handle with insertion order and linear lookup.
  Function pointers, nullable `T | None` narrowing and string-payload exceptions
  (`raise`/`try`/`except`) are supported.
- Raw-memory builtins and C FFI are unsafe. Concatenation results intentionally
  leak; there is no GC or universal runtime memory-safety guarantee.
- Modules provide namespaces, aliases and selective/star imports. Bare imports
  probe siblings first; quoted imports prefer packages/stdlib, then siblings.
- The TypeScript front end lowers a supported subset to the same AST; it is
  **not full TypeScript/JavaScript compatibility**. Objects/closures/GC remain
  gaps. See [TS plan](docs/ts-m1-spec.md).

The [specification](docs/spec.md), [language card](AGENTS.md) and
[language gaps](docs/language-gaps.md) describe the actual surface and limits.

### Libraries and tools

The Aoxn-written [stdlib](docs/stdlib.md) includes core containers/I/O and
separate math, time, datetime, calendar, pathlib, base64, hashlib, hmac, os,
glob, JSON, bisect and heapq modules. Import explicitly, for example
`import * from "stdlib/math"`. The [UI toolkit](docs/ui.md) shares a portable
widget layer between Win32/GDI and Linux X11/Xft. The [OpenAI](docs/openai-sdk.md)
and [Anthropic](docs/anthropic-sdk.md) SDKs share JSON/HTTP/SSE infrastructure;
their WinHTTP transport is Windows-only.

The [package manager](docs/pkg-manager.md) uses `aoxn.json`, `aoxn.lock` and
`aox_modules/`. Commands include `aoxn pkg init|add|remove|install|update|list|freeze|tree|why|audit|trust|publish|yank|cache|npm-import`;
direct aliases are available. Registries can be directories, git repos or
read-only plain-HTTP mirrors. npm-import accepts Aoxn metadata, not arbitrary
JavaScript packages. Warm installs retain trees only after verifying their
content hash, excluding only the installer-owned `index.ax` shim.

[CSS assets](docs/css-assets.md) support CSS imports/Modules, minification,
URLs and `--emit-assets`. Tailwind may be pre-generated CSS; `--tailwind` is
only a documented built-in subset. The [IDE](docs/ide.md) (`ide/`) is a
Tauri 2/Next.js/Monaco workbench driving the real compiler; `pnpm --dir ide dev`
can run its in-memory browser fixture.

`selfhost/` contains an Aoxn-written lexer/parser/checker/loader/C emitter/driver.
The fixed point compares self-hosted driver products across compiler stages,
including C/object bytes (masking Windows COFF timestamps). It does **not**
prove equivalence of every Rust and Aoxn emitter feature. The self-hosted
backend still lacks several newer features, including dict/None/fn-pointer/
exception emission. Runtime benchmarks such as [web results](docs/web-benchmark.md)
are distinct from compiler-time measurements.

### Validation of this tree

On Windows, using Rust 1.94.1 and explicitly selecting existing clang 23.1.2:

```powershell
$env:AOXN_CLANG = "E:/llvm/bin/clang.exe"  # substitute your installed clang
cargo test --no-fail-fast
bash run_pkg_tests.sh
cargo check --workspace --tests --target x86_64-unknown-linux-gnu
```

**343 root tests + 104 package-manager tests = 447 passed, zero failed or
ignored** in the final runs. Root coverage includes 24 library, 3 setup,
198 pipeline, 39 assets, 35 TS, 14 UI, 8 symbol and 22 other integration tests.
The Linux Rust cross-check passed; it is not a Linux link/runtime test.
Initially clang was absent from PATH: the complete attempt had **202 passed,
140 failed**, before configuring `AOXN_CLANG`. That failed attempt and the
successful rerun are both documented, rather than hiding the environment issue.
The release baseline and modified compiler produced identical C, diagnostics
and exit status for **42 successful and 5 invalid inputs**.

IDE tests are separate and were not run for this compiler-only change:
`pnpm --dir ide test`, `pnpm --dir ide typecheck`, and
`cargo test --manifest-path ide/src-tauri/Cargo.toml`. Windows/Linux CI remains
necessary; Linux UI tests use Xvfb. See the performance report for logs and
exact scope, including pre-existing unchecked wrapped struct-cycle edges.

| Location | Purpose |
|---|---|
| `src/`, `tests/` | compiler, CLI, installer and regression suites |
| `stdlib/`, `examples/` | libraries and examples |
| `selfhost/` | self-hosted compiler subset |
| `crates/aoxn-pkg/` | independent package manager |
| `ide/`, `web/` | IDE and Windows web benchmark suite |
| `dist/package.ps1` | single-file Windows installer packaging |
| `tools/compiler_perf.py` | clang-free interleaved compiler benchmark |
| `docs/` | maintained specs, guides and measurements |
| `wiki/` | frozen historical documentation, not current authority |

Contributions follow [AGENTS.md](AGENTS.md). Report vulnerabilities privately
under [SECURITY.md](SECURITY.md). License: [Apache-2.0](LICENSE).

---

## 中文

### 当前项目

Aoxn 是面向 AI 的静态类型原生编译语言，使用 Python 风格缩进。Rust 编译器
输出 ISO C，再调用外部 clang，已无 LLVM 库依赖。`src/` 编译器库不依赖
外部 Rust crate；独立包管理器的依赖由 `Cargo.lock` 固定。

**编译器 0.55.0，包管理器 crate 0.42.0**，以当前清单与
[CHANGELOG](CHANGELOG.md) 最新已发布条目为准。v0.55.0 修复 Python 幂运算
优先级（`-2 ** 2 == -4`），并在 [AGENTS.md](AGENTS.md) 加入代理语言卡。
未发布的编译器优化保留语言语义和 C 文本：反向调用边异常传播、借用模块成员表，
以及本次跨声明根复用已证明无环的结构体节点。

对 `82cffcd` 的 release 交错实测，1600 个结构体共享尾链的 typecheck 中位数
**602.866 ms → 2.398 ms**，`aoxn c` 进程墙钟中位数
**672.730 ms → 71.759 ms**。这不是所有项目的加速承诺：真实 selfhost/UI/stdlib
的端到端小幅变化仍受噪声影响。方法、原始数据与复现命令见
[编译器性能报告](docs/p2-compiler-performance.md)。

```python
import * from "stdlib"

def square(x: int, scale: int = 1) -> int:
    return x ** 2 * scale

def main() -> int:
    values = [2, 3, 5]
    for value in values:
        print(value, square(value))
    return 0
```

### 平台、安装与命令

**Windows 为一等平台，支持 Linux，不支持 macOS**，以
[平台政策](docs/platform-support.md) 为准。[Windows 发布包](https://github.com/AlonechatWorkspace/Aoxn-language/releases)
是单文件 `Aoxn-<version>-Setup.exe`，解包到 `%LOCALAPPDATA%\aoxn`，设置 PATH
并运行 `aoxn doctor`。**默认不下载任何东西**，只有 `-InstallClang` 显式启用
安装 clang。clang 是外部前提，Windows 链接需要 MSVC 兼容构建工具。
未签名程序可能被系统隔离，详见[安装指南](docs/install.md)与[安全政策](SECURITY.md)。

构建编译器需要 Rust；编译 Aoxn 程序还需 clang。Linux 图形程序另需
X11/Xft/fontconfig 开发库，纯命令行编译器不需要图形库。

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

Linux 可使用 `-o fib`。`check`、`c`、`symbols` 不调用 clang，`--json` 提供
结构化诊断。`--O0` 至 `--O3` 选择 clang 优化级别，默认 O3，不改变 C 文本。
`--clang-arg`、`-g`、`--cc-warnings` 分别透传参数、请求调试信息与启用警告。

`run`/`build` 共享内容缓存；`AOXN_NO_CACHE=1` 禁用，`AOXN_CACHE_DIR` 改位置。
`AOXN_TIME=1` 输出 lex/parse/resolve/typecheck/codegen/link 实际阶段耗时；
用 `aoxn c` 排除 clang 和可执行文件缓存。`AOXN_CLANG` 指定 clang，
`AOXN_HOME`/`AOXN_STDLIB` 指定工具链/标准库，`AOXN_DUMP_C=1` 打印 C。

### 语言和边界

- 支持缩进块、def/elif/pass、# 注释、推断、泛型、尾部默认参数、条件表达式、
  多参数 print。类型严格，不隐式转换 int/float，条件必须 bool，变量不改类型，
  返回路径与不可达语句受检查。
- `**` 右结合，`-2 ** 2` 为 `-4`；`not` 使用 Python 比较优先级，位运算保留
  文档记录的 C 规则。
- 数组和结构体按值复制；字符串是不可变、NUL 结束的**字节**，长度/索引/切片/
  遍历不是 Unicode 字符操作。字符串越界返回空串，数组索引仍不检查。
- `dict[V]` 共享堆句柄，保留插入顺序但键查询线性；支持函数指针、`T | None`
  收窄与字符串载荷 raise/try/except。
- 原始内存与 C FFI 不安全；字符串拼接结果按设计不释放，没有 GC 或通用运行时
  内存安全保证。
- 模块支持命名空间、别名、选择性/星号导入。裸导入先同目录，引号导入优先包与
  标准库，失败后回退同目录。
- TypeScript 前端是同一 AST 上的已支持子集，**不是完整 TS/JS 兼容**，对象/
  闭包/GC 仍缺失，见 [TS 计划](docs/ts-m1-spec.md)。

实际规则见[规范](docs/spec.md)、[语言卡](AGENTS.md)及[语言缺口](docs/language-gaps.md)。

### 库与工具

Aoxn 编写的[标准库](docs/stdlib.md)提供核心容器/I/O 与独立 math、time、datetime、
calendar、pathlib、base64、hashlib、hmac、os、glob、JSON、bisect、heapq 模块，
例如 `import * from "stdlib/math"`。[UI](docs/ui.md)共享可移植控件层，后端为
Win32/GDI 和 Linux X11/Xft。[OpenAI](docs/openai-sdk.md)/[Anthropic](docs/anthropic-sdk.md)
SDK 共享 JSON/HTTP/SSE，WinHTTP 传输仅 Windows。

[包管理器](docs/pkg-manager.md)使用 aoxn.json/aoxn.lock/aox_modules，提供
`aoxn pkg init|add|remove|install|update|list|freeze|tree|why|audit|trust|publish|yank|cache|npm-import`
及直接别名。注册表可为目录、git 或只读明文 HTTP。npm-import 只接受 Aoxn
元数据，不接受任意 JS 包。热安装复用前仍计算真实内容哈希，仅排除自身生成的
index.ax shim。[CSS](docs/css-assets.md)支持导入/Modules、压缩、URL 与
--emit-assets；Tailwind 可预生成，内建 --tailwind 仅为已记录子集。
[IDE](docs/ide.md)采用 Tauri 2/Next.js/Monaco 并调用真实编译器；
`pnpm --dir ide dev` 可运行浏览器内存样例。

selfhost/ 包含本语言实现的词法器/解析器/检查器/加载器/C 发射器/驱动。
不动点比较驱动跨阶段产物（C/目标文件，屏蔽 Windows COFF 时间戳），
**不证明 Rust 与 Aoxn 发射器所有功能等价**。自举后端仍缺 dict/None/函数指针/
异常等新特性。[Web 运行基准](docs/web-benchmark.md)与编译器自身耗时不是同一指标。

### 当前树验证

本地 Windows、Rust 1.94.1、显式选择已有 clang 23.1.2：

```powershell
$env:AOXN_CLANG = "E:/llvm/bin/clang.exe"  # 替换为本机 clang
cargo test --no-fail-fast
bash run_pkg_tests.sh
cargo check --workspace --tests --target x86_64-unknown-linux-gnu
```

最终 **343 项根测试 + 104 项包测试 = 447 项通过，0 失败/忽略**。
根套件包含 library 24、setup 3、pipeline 198、assets 39、TS 35、UI 14、
symbols 8、其他集成 22。Linux Rust cross-check 通过，但不等于 Linux 链接或运行。
首轮 PATH 缺 clang，全量尝试为 **202 通过、140 失败**；设置 AOXN_CLANG 后重跑
全过，报告保留失败与成功日志。release 基线和修改版的 **42 个成功输入与5个
非法输入**保持 C、诊断和退出状态逐字节一致。

IDE 未在本次编译器修改中运行，其独立命令为 `pnpm --dir ide test`、
`pnpm --dir ide typecheck`、`cargo test --manifest-path ide/src-tauri/Cargo.toml`。
仍需 Windows/Linux CI，Linux UI 在 Xvfb 下运行；性能报告说明日志与既有的
包装类型结构体环检测缺口。

目录：src/tests 为编译器/CLI/安装器与回归，stdlib/examples 为库/示例，
selfhost 为自举子集，crates/aoxn-pkg 为独立包管理器，ide/web 为 IDE/Windows
Web 基准，dist/package.ps1 打包，tools/compiler_perf.py 为无 clang 交错基准，
docs 为当前文档。**wiki 已冻结，不作为当前权威**。

贡献遵守 [AGENTS.md](AGENTS.md)，漏洞按 [SECURITY.md](SECURITY.md) 私报。
许可证 [Apache-2.0](LICENSE)。
