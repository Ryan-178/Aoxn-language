# Aoxn

<img src="icons/mark.svg" width="96" alt="Aoxn" align="right">

## English

### What it is

Aoxn is an AI-native, statically typed language with Python-style indentation
and ahead-of-time native compilation. The Rust compiler lowers `.ax` programs
to ISO C and invokes clang; it does not link LLVM libraries. The compiler
library in `src/` has no external Rust dependencies. The separate package
manager uses dependencies pinned in `Cargo.lock`.

**Current version: 0.54.1** (`Cargo.toml`, latest released entry in
[CHANGELOG.md](CHANGELOG.md)); package manager crate: **0.42.0**. Development
changes after that release preserve language semantics: exception reachability
uses a reverse-call-edge worklist instead of repeated whole-graph scans, and
module rewriting borrows its immutable member tables rather than cloning the
whole table for every module. These are compiler-time changes, not changes to
generated programs. Measurement details and limitations are recorded in
[docs/p2-compiler-performance.md](docs/p2-compiler-performance.md).

```python
import * from "stdlib"

def square(x: int, scale: int = 1) -> int:
    return x ** 2 * scale

def main() -> int:
    values = [2, 3, 5]
    for value in values:
        print(value, square(value))
    print("native" if "ox" in "Aoxn" else "compiled")
    return 0
```

### Install and build

**Windows is first-class; Linux is supported; macOS is not supported.**
See [platform support](docs/platform-support.md).

Windows releases ship one `Aoxn-<version>-Setup.exe` from the
[release page](https://github.com/AlonechatWorkspace/Aoxn-language/releases).
The installer unpacks the compiler, standard library and examples into
`%LOCALAPPDATA%\aoxn`, updates PATH and runs a toolchain self-check. It
**downloads nothing by default**. clang is an external prerequisite;
`-InstallClang` explicitly opts into installation. Windows linking requires
MSVC Build Tools. Unsigned binaries can be blocked by Windows security policy;
see [installation guide](docs/install.md) and [security policy](SECURITY.md).

To build the compiler from source, install Rust. To compile Aoxn programs,
also install clang (and MSVC Build Tools on Windows). Linux UI programs require
X11/Xft/fontconfig development libraries; headless compiler use does not.

```powershell
cargo build --release
cargo run -- run examples/hello.ax
cargo run -- check examples/fib.ax
cargo run -- build examples/fib.ax -o fib.exe
cargo run -- c examples/fib.ax
cargo run -- symbols examples/ui_gallery.ax --json
cargo run -- doctor --json
cargo run -- version
```

Use a native output name on Linux, for example `-o fib`. `check`, `c` and
`symbols` do not invoke clang. `--json` provides structured diagnostics;
`symbols` exports source declarations for editor navigation. `--O0` through
`--O3` select clang's optimization level (default O3); the generated C text is
independent of that level. `--clang-arg` passes an additional clang argument,
`-g` requests debug information, and `--cc-warnings` enables clang warnings.

`run` and `build` share a content-keyed executable cache. `AOXN_NO_CACHE=1`
disables it; `AOXN_CACHE_DIR` relocates it. `AOXN_TIME=1` reports lex, parse,
**resolve**, typecheck, codegen and link timings where those stages run.
`aoxn c` isolates Rust-side compilation from external clang/link time.
`AOXN_DUMP_C=1` prints emitted C. `AOXN_CLANG` selects clang;
`AOXN_HOME` and `AOXN_STDLIB` override installed toolchain/stdlib locations.

### Language and boundaries

- Indentation blocks, `def`, `elif`, `pass`, `#` comments, type inference,
  defaults, conditional expressions, `**`, multi-value `print`, and generics.
- Strict types: no implicit int/float conversion, bool-only conditions,
  no retyping bound variables, and checked return/control-flow rules.
- Fixed-size arrays and structs copy by value. Strings are immutable,
  NUL-terminated byte sequences; `len`, indexing, slicing and iteration use
  **bytes**, not Unicode characters. String indexing/slicing clamps to `""`
  outside bounds; array indexing is unchecked. Concatenation allocates and
  intentionally does not free results; there is no garbage collector.
- Modules support star/selective imports, aliases and qualified names.
  Bare imports first probe the importing file's directory; quoted imports
  prefer packages/installed stdlib and use the sibling file as a last fallback.
- `not` has Python precedence: `not x == y` means `not (x == y)`.
  Bitwise operators retain their documented C-style precedence.
- `dict[V]` is a shared heap handle, preserving insertion order with linear
  key lookup. Function pointers, `T | None` narrowing and `raise`/`try`/`except`
  are available. Raw memory builtins and C FFI are intentionally unsafe.
- A TypeScript front end lowers its supported subset to the same AST.
  It is **not full TypeScript/JavaScript compatibility**; runtime objects,
  closures and GC remain gaps. See [TS plan](docs/ts-m1-spec.md).

The [language specification](docs/spec.md) is authoritative. Known limitations
and their consequences are in [language gaps](docs/language-gaps.md).

### Libraries, packages and tools

The core [standard library](docs/stdlib.md) is written in Aoxn. Separate modules
include math, time, datetime, calendar, pathlib, base64, hashlib, hmac, os,
glob, JSON, bisect and heapq. Import modules explicitly, for example
`import * from "stdlib/math"`. The [UI toolkit](docs/ui.md) has a portable
widget layer with Win32/GDI and Linux X11/Xft backends. The
[OpenAI](docs/openai-sdk.md) and [Anthropic](docs/anthropic-sdk.md) SDKs share
JSON/HTTP/SSE infrastructure; their WinHTTP transport is Windows-only.

The independent [package manager](docs/pkg-manager.md) uses `aoxn.json`,
`aoxn.lock` and `aox_modules/`. Commands include `aoxn pkg init`, `add`,
`install`, `update`, `list`, `freeze`, `tree`, `why`, `audit`, `trust`,
`publish`, `yank`, `cache` and `npm-import`; direct aliases are supported.
Registries can be directories, git repositories or read-only plain-HTTP mirrors.
The npm bridge accepts Aoxn packages, not arbitrary JavaScript dependencies.
Warm installs retain an existing package only after re-hashing its content
against the lockfile, excluding only the installer-owned `index.ax` shim.

The [CSS pipeline](docs/css-assets.md) supports CSS imports, CSS Modules,
minification, URL assets and `--emit-assets`. Tailwind can be supplied as
pre-generated CSS; `--tailwind` implements a documented built-in subset,
not the complete upstream toolchain.

The [IDE](docs/ide.md) under `ide/` combines Tauri 2, Next.js and Monaco and
drives the real compiler for checking/building/running and symbol navigation.
`pnpm --dir ide dev` also runs an in-memory browser fixture for UI development.

### Self-hosting and performance claims

`selfhost/` contains an Aoxn-written lexer, parser, checker, loader, C emitter
and compiler driver. The fixed-point test checks the self-hosted driver's
products across compiler stages, including C and object bytes (masking Windows
COFF timestamps). It **does not prove that the Rust and Aoxn emitters agree on
all source programs**. The self-hosted backend still lacks several newer
features, including dict/None/function-pointer/exception emission; the Rust
compiler is the complete implementation.

Generated code is compiled by clang; runtime comparisons and web benchmarks
are separate from compiler-time measurements. See [web results](docs/web-benchmark.md)
and [compiler measurements](docs/p2-compiler-performance.md). Host load,
antivirus, process startup and external clang can dominate short measurements;
synthetic scaling wins are not universal real-project speedup claims.

### Validation and repository map

Run the suites from the repository root.

```powershell
cargo test
bash run_pkg_tests.sh
pnpm --dir ide test
pnpm --dir ide typecheck
cargo test --manifest-path ide/src-tauri/Cargo.toml
```

The package test script refreshes a marker to work around Windows Smart App
Control blocking reused unsigned test binaries. IDE checks are separate from
the root workspace. Local Windows validation passed **338 root tests and 104
package-manager tests (442 total, zero failed/ignored)**. The individual suite
results and measurement conditions are recorded in the
[compiler performance report](docs/p2-compiler-performance.md), not inferred
from old README counts. Tests cover diagnostics, generated-code behavior,
C reproducibility, imports, assets, SDK loopback mocks, stdlib and self-hosting.
Windows and Linux have CI jobs; the Linux UI probe runs under Xvfb. This local
Windows validation does not replace the Linux CI run or test the IDE.

| Location | Purpose |
|---|---|
| `src/` | Rust compiler, module resolver, C backend, CLI, installer |
| `crates/aoxn-pkg/` | independent package manager |
| `stdlib/`, `examples/` | libraries and runnable examples |
| `selfhost/` | Aoxn-written compiler subset |
| `tests/` | compiler/stdlib/asset/integration regression suites |
| `ide/` | Tauri/Next.js/Monaco IDE, separate tests |
| `web/` | Windows HTTP server and benchmark comparisons |
| `dist/package.ps1` | single-file Windows installer packaging |
| `docs/` | maintained specifications, guides and measurements |
| `wiki/` | frozen historical documentation; do not use as current authority |

Contributions follow [AGENTS.md](AGENTS.md); report vulnerabilities privately
as described in [SECURITY.md](SECURITY.md). Licensed under [Apache-2.0](LICENSE).

---

## 中文

### 项目简介与当前状态

Aoxn 是面向 AI 的静态类型、提前编译语言，使用 Python 风格缩进语法。
Rust 编译器把 `.ax` 降低为 ISO C，再调用 clang 生成原生程序；编译器本身
不链接 LLVM 库。`src/` 编译器库不依赖外部 Rust crate，独立包管理器的依赖
由 `Cargo.lock` 固定。

**当前版本为 0.54.1**，来源是 `Cargo.toml` 与
[CHANGELOG.md](CHANGELOG.md) 最新已发布条目；包管理器 crate 为 **0.42.0**。
本次开发中的优化不改变语言语义：异常可达性改为反向调用边工作队列，避免
反复扫描全图；模块重写直接借用只读成员表，不再为每个模块克隆整张表。
优化针对编译器耗时，不是生成程序的运行速度。测量方法、结果和限制见
[编译器性能报告](docs/p2-compiler-performance.md)。

### 安装、构建与命令

**Windows 为一等平台，Linux 受支持，macOS 不受支持**，以
[平台说明](docs/platform-support.md) 为准。

Windows 从[发布页](https://github.com/AlonechatWorkspace/Aoxn-language/releases)
下载单文件 `Aoxn-<version>-Setup.exe`。安装器解包到 `%LOCALAPPDATA%\aoxn`，
设置 PATH 并运行工具链自检。**默认不下载任何内容**；clang 是外部前提，
只有显式指定 `-InstallClang` 才尝试安装。Windows 链接还需要 MSVC Build Tools。
未签名程序可能被 Windows 安全策略阻止，详见[安装指南](docs/install.md)。

从源码构建需要 Rust；编译 Aoxn 程序还需要 clang。Linux 图形界面程序另外
需要 X11/Xft/fontconfig 开发库，纯命令行编译不需要这些界面库。

```powershell
cargo build --release
cargo run -- run examples/hello.ax
cargo run -- check examples/fib.ax
cargo run -- build examples/fib.ax -o fib.exe
cargo run -- c examples/fib.ax
cargo run -- symbols examples/ui_gallery.ax --json
cargo run -- doctor --json
cargo run -- version
```

Linux 可使用 `-o fib`。`check`、`c`、`symbols` 不调用 clang；`--json` 提供
结构化诊断。`--O0` 到 `--O3` 选择 clang 优化级别，默认为 O3，级别不改变
生成的 C 文本。`--clang-arg` 透传参数，`-g` 请求调试信息，`--cc-warnings`
启用 clang 警告。

`run` 与 `build` 共享按内容计算的可执行文件缓存；`AOXN_NO_CACHE=1` 禁用，
`AOXN_CACHE_DIR` 改位置。`AOXN_TIME=1` 输出实际经过的 lex、parse、**resolve**、
typecheck、codegen、link 阶段耗时；测 Rust 编译器自身应使用 `aoxn c`，避免
把 clang 和链接耗时混在一起。`AOXN_DUMP_C=1` 打印 C；`AOXN_CLANG` 指定 clang；
`AOXN_HOME`、`AOXN_STDLIB` 覆盖安装根与标准库目录。

### 语言与边界

- 支持缩进块、`def`、`elif`、`pass`、`#` 注释、推断、泛型、默认参数、
  条件表达式、`**` 和多参数 `print`。
- 类型严格：int/float 不隐式转换，条件必须 bool，已绑定变量不改变类型，
  返回路径与控制流受检查。
- 定长数组与结构体按值复制。字符串不可变、以 NUL 结束，长度、索引、切片
  和遍历均按**字节**，不是 Unicode 字符。字符串越界返回空串，数组索引不检查。
  字符串拼接结果按设计不释放，目前没有垃圾回收器。
- 模块支持星号/选择性导入、别名和限定名称。裸导入先找导入者同目录；引号导入
  优先包/安装标准库，所有包探测失败后才回退同目录。
- `not` 采用 Python 优先级，`not x == y` 等于 `not (x == y)`；位运算保留
  规范记录的 C 风格优先级。
- `dict[V]` 为共享堆句柄，保留插入顺序，键查找是线性的。支持函数指针、
  `T | None` 收窄及 `raise`/`try`/`except`；原始内存操作与 C FFI 故意不安全。
- TypeScript 前端把已支持的子集降低到同一个 AST，**并非完整 TS/JS 兼容**；
  运行时对象、闭包和 GC 等仍有缺口，见 [TS 计划](docs/ts-m1-spec.md)。

规则以[语言规范](docs/spec.md)为准，限制和影响见[语言缺口](docs/language-gaps.md)。

### 标准库、包管理与工具

[标准库](docs/stdlib.md)用 Aoxn 编写，独立模块包括 math、time、datetime、
calendar、pathlib、base64、hashlib、hmac、os、glob、JSON、bisect、heapq；
例如使用 `import * from "stdlib/math"` 显式导入。
[UI 工具包](docs/ui.md)共享可移植控件层，后端为 Windows Win32/GDI 和 Linux
X11/Xft。[OpenAI](docs/openai-sdk.md) 与 [Anthropic](docs/anthropic-sdk.md)
SDK 共享 JSON/HTTP/SSE 基础设施，WinHTTP 传输为 Windows 专用。

独立[包管理器](docs/pkg-manager.md)使用 `aoxn.json`、`aoxn.lock`、`aox_modules/`，
提供 init/add/install/update/list/freeze/tree/why/audit/trust/publish/yank/cache/
npm-import 等命令和直接别名。注册表可为目录、git 仓库或只读明文 HTTP 镜像；
npm 桥仅接受 Aoxn 包，不把任意 JS 包变成本语言依赖。热安装只有在重新计算
已安装目录内容哈希并匹配锁文件后才保留，仅排除安装器自身生成的 `index.ax` shim。

[CSS 资源管线](docs/css-assets.md)支持 CSS 导入、CSS Modules、压缩、URL 资源与
`--emit-assets`。Tailwind 可预生成 CSS；内建 `--tailwind` 只是已文档化子集，
不是完整上游工具链。[IDE](docs/ide.md)位于 `ide/`，采用 Tauri 2、Next.js、Monaco，
检查/构建/运行/符号导航均调用真实编译器；`pnpm --dir ide dev` 可启动浏览器内存样例。

### 自举、性能与验证

`selfhost/` 包含本语言编写的词法器、解析器、检查器、加载器、C 发射器和驱动。
不动点测试比较自举驱动跨阶段的产物，包括 C 与目标文件字节（屏蔽 Windows
COFF 时间戳）。它**不证明 Rust 和 Aoxn 两个发射器对所有程序产生相同 C**。
自举发射器仍缺少 dict/None/函数指针/异常等新特性，完整实现是 Rust 编译器。

生成程序的运行性能、[Web 基准](docs/web-benchmark.md)和
[编译器自身耗时](docs/p2-compiler-performance.md)是不同指标。机器负载、杀毒软件、
进程启动与外部 clang 都可能主导短测量；合成长链的改善不能宣传为所有真实项目的加速。

从仓库根运行：

```powershell
cargo test
bash run_pkg_tests.sh
pnpm --dir ide test
pnpm --dir ide typecheck
cargo test --manifest-path ide/src-tauri/Cargo.toml
```

包测试脚本通过刷新标记绕过 Windows Smart App Control 对复用未签名测试程序的
阻止。IDE 测试独立于根工作区。本地 Windows 实测 **338 项根测试 + 104 项包测试，
共 442 项通过，无失败/忽略**。分套件结果记录在
[编译器性能报告](docs/p2-compiler-performance.md)，不引用历史 README 中漂移的数字。
测试涵盖诊断、运行结果、C 确定性、导入、资源、SDK 回环 mock、标准库和自举。
Windows/Linux 均有 CI，Linux UI probe 使用 Xvfb；本地 Windows 验证不替代 Linux CI，
也未顺带验证 IDE。

主要目录：`src/` 编译器/CLI/安装器，`crates/aoxn-pkg/` 包管理器，`stdlib/` 与
`examples/` 库和示例，`selfhost/` 自举子集，`tests/` 回归套件，`ide/` IDE，
`web/` Windows HTTP 服务与对比基准，`dist/package.ps1` 打包安装器，`docs/`
维护中的文档。`wiki/` 已冻结，只能作为历史资料。

贡献遵守 [AGENTS.md](AGENTS.md)，安全问题按 [SECURITY.md](SECURITY.md) 私下报告。
许可证：[Apache-2.0](LICENSE)。
