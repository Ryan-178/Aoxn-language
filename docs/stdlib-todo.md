# 标准库路线 TODO（stdlib roadmap）

> 工作文档，不是规范。语言规则见 `docs/spec.md`，OpenAI SDK 见
> `docs/openai-sdk.md`。本文件只回答一个问题：**接下来按什么顺序、把哪些
> Python 标准库模块搬进 Aoxn，卡在哪里。**
>
> 模块编号沿用需求清单原编号 1–31，方便对照。

---

## 0. 动手之前：四条硬约束

这四条是**已核实**的，不是猜测。它们决定了后面每个模块的可行形态；
写模块之前先读这一节，能省掉大部分返工。

### 0.1 `stdlib/stdlib.ax` 在自举关键路径上 —— 新模块必须独立成文件

`selfhost/codegen.ax`、`selfhost/driver.ax`、`selfhost/lex_demo.ax` 等
**全部** `import * from "../stdlib/stdlib.ax"`。而 v0.40.0 的
dict / `None` / fn-ptr / `raise` 在 selfhost 镜像里尚未覆盖
（`selfhost/codegen.ax` 发不出这些文本，见 AGENTS.md）。

**规则**：新模块一律写成独立文件 `stdlib/<name>.ax`，以
`import * from "stdlib/<name>"` 引入。**只有**当某个模块必须被自举编译器
自身引用时，才写进 `stdlib.ax`，并把语法约束在 selfhost 能编译的子集内。

- ✅ `stdlib/os.ax`、`stdlib/datetime.ax`、`stdlib/re.ax` … 各自成文件
- ❌ 把它们 `import` 进 `stdlib/stdlib.ax`（一步毁掉固定点测试）

### 0.2 argv 还没进语言 —— `sys` / `argparse` 拿不到命令行参数

`src/codegen_c.rs:653` 生成的是 `int main(void)`，入口签名不带参数。

- `sys.argv` / `sys.executable` / `sys.stdin` **阻塞**
- `argparse` 可以先做「解析一个传入的 `string` 数组」，只是没法自己拿到
  那份数组 —— 不算完全阻塞，优先级下调

> 编译器侧的改动很小（`main(int argc, char** argv)` + 落进一个运行时
> `Vec`），但要同步 `selfhost/codegen.ax` 才能保持固定点，属于 P3。

### 0.3 没有泛型堆容器 —— `collections` / `itertools` 无处落脚

`stdlib.ax` 的 `struct Vec` 是 **int-only**（`vec_push(v: Vec, item: int)`），
另有 `VecVec` 硬编码的嵌套版本。泛型是**单态化**的（按 (类型, 长度)），
所以 `Vec[T]` 可写但会按类型各编一份 —— 需要一个显式的堆容器基座。

**P0 必须先做 `Vec[T]`**，否则 `collections` 整个落不了地。

### 0.4 extern 只能传 int/f64/string/bool，且是定长

- 不能按值传结构体 → 想接 Win32 的 `SYSTEMTIME`、`WIN32_FIND_DATAA` 这类
  结构体，只能**自己在堆上分配 + `load_i64/store_i64` 逐字段读写**
- **变参函数不可用**：`printf`/`sprintf` 会读到未初始化的 XMM 溢出槽
  （已实测：打印出 `2.47e-323`）。格式化必须手写整数数学
- `web/sock_win.ax` 里已有 `load_i64(ts + 8)` 这种写法，直接照抄

---

## 1. 批次总览

| 批次 | 主题 | 模块 | 定位 |
|---|---|---|---|
| **P0** | 基座 + 纯 extern/math 能吃的 | `vec` 基座、`time`、`datetime`、`calendar`、`hashlib`、`hmac`、`base64`(补 decode)、`os`(核心)、`pathlib`、`glob` | 立刻能做，风险最低 |
| **P1** | 解析与序列化 | `json`(已有，需提升)、`csv`、`re`、`shutil`、`bisect`、`heapq`、`socket`(已有需上提) | 有工程量但形状清晰 |
| **P2** | 容器与并发 | `collections`、`itertools`、`urllib`、`threading`、`concurrent.futures` | 前两个依赖 P0 基座；线程有真实阻塞 |
| **P3** | 语言级阻塞 | `sys`、`argparse`、`multiprocessing`、`pickle`、`logging`、`traceback`、`typing`、`unittest` | 需要先改语言或改需求形态 |

**建议起手**：P0 全做完 → P1 的 `json` 提升 + `bisect`/`heapq` →
再评估 P2 并发。P3 整体挂起，等语言侧排期。

---

## 2. 模块清单

可行性图例：
✅ 现在就能写 · ⚠️ 有前置依赖（已注明）· ⛔ 当前语言不支持（已注明原因）

### 1. 文件、路径、操作系统

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 1 | `os` | P0/P1 | ✅ | 底座是 Win32：`CreateDirectoryW`、`RemoveDirectoryW`、`GetEnvironmentVariableW`、`SetEnvironmentVariableW`、`GetFileAttributesW`、`CopyFileW`、`MoveFileW`。遍历/时间戳推迟到 P1（见 `glob`）。所有路径走 **宽字符**（`W` 版本），非 ASCII 路径必须支持。 |
| 2 | `pathlib` | P0 | ✅ | `struct Path{parts}` + 纯字符串运算（`/ \` 分割、`join`、`suffix`、`stem`、`parent`、`with_suffix`…）。**推荐作为日常写法**，`os.path` 只做兼容门面。 |
| 3 | `shutil` | P1 | ✅ | 递归拷贝/删除用 `FindFirstFileW` 递归 + `CopyFileW`。**必须防目录穿越与符号链接环**（用 `FILE_ATTRIBUTE_REPARSE_POINT` 判环）。 |
| 4 | `glob` | P0 | ✅ | 建立在 `FindFirstFileW` 上，`*`/`?` 匹配自己写（不用 Win32 的 `FindFirstFile` 通配，它自己就是通配）。`[a-z]` 字符类要写。 |

### 2. 时间、日期

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 5 | `time` | P0 | ✅ | `QueryPerformanceCounter`（**注意：`timespec_get` 在 Windows 上不链接**，AGENTS.md 已记）+ `GetSystemTimeAsFileTime` / `GetTickCount64`。`sleep` → `Sleep`（毫秒，Python 是秒，要自己换算）。 |
| 6 | `datetime` | P0 | ✅ | 纯整数数学：儒略日/民用日历互转、闰年、`strftime` 的 `%Y-%m-%d %H:%M:%S` 子集。**格式串自己解析**，不要用 `wcsftime`（走 CRT 路径且依赖 locale）。 |
| 7 | `calendar` | P0 | ✅ | 月历网格、日序数、`weekday`/`monthrange`。直接建在 `datetime` 上。 |

### 3. 数据序列化 / 文本解析

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 8 | `json` | P1 | ✅ **已有** | `stdlib/openai/json.ax` 已经是一个完整 JSON DOM（40 字节 slab、parse/dumps、往返测试都过了）。**P1 的工作是把它提升为 `stdlib/json.ax` 独立模块 + 补文件 I/O**，不是从零写。 |
| 9 | `csv` | P1 | ✅ | RFC 4180：引号转义、内嵌换行、`\r\n`。**没有 `\r` 字符串转义**（AGENTS.md：只有 `\n \t \\ \"`），CRLF 要用 `store_u8` 写字节 13。 |
| 10 | `pickle` | P3 | ⛔ **无反射** | 语言没有「遍历一个值的类型/字段」。可行替代是**显式注册**：程序自己声明要序列化的字段清单，`pickle_dump(recs)`。这样能往返，但和 Python pickle 格式不兼容 —— 若目标是「读 `.pkl` 文件」，**直接不做**。 |
| 11 | `re` | P1 | ⚠️ 大工程 | 回溯匹配器（支持 `* + ? \| () [] {m,n} .`、字符类、非贪婪、锚点）+ 一个手写 DFA 编译步骤会太大。**先只做回溯版**，用 `Vec` 存回溯栈（两个 int 栈：位置、备选点）。预计 2–4k 行。锚定建议：跟一个「HTML/XML 标记提取」的真实用例，避免做成正则引擎而没人用。 |

### 4. 容器、数据结构

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 12 | `collections` | P2 | ⚠️ 依赖 P0 `Vec[T]` | `deque`（环形缓冲）、`defaultdict`、`Counter`、`OrderedDict`。注意 `dict[V]` 是**堆句柄**（v0.40.0），复制句柄=共享同一个 dict —— `OrderedDict` 的插入序要自己维护一个索引 `Vec`，不能指望底层有序。 |
| 13 | `itertools` | P2 | ⚠️ **没有 `yield`** | 惰性迭代器做不了（`yield` 在 spec roadmap 第 9 条）。**形态改成"批量返回数组"**：`chain(list, list) -> Vec[T]`、`permutations(n) -> Vec[Perm]`、`product(a, b) -> Vec[Tuple]`。无限迭代器（`count`/`cycle`）改为「带 `take(n)` 的生成函数」。**这个降级要在模块头注释里写明**。 |
| 14 | `bisect` | P1 | ✅ | 便宜。`bisect_left/right` 对有序 `Vec[T]`（**注意：泛型按长度单态化，长度是类型的一部分** → 变长容器要用堆容器）。 |
| 15 | `heapq` | P1 | ✅ | 标准二叉堆，`heap_push`/`heap_pop`/`heapify`。和 `bisect` 一起做。 |

### 5. 网络、请求

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 16 | `urllib` | P2 | ✅ **部分已有** | `stdlib/openai/http.ax` 已经是一套 WinHTTP 传输（open/connect/send/receive/timeouts）。**P2 是把它泛化成 `stdlib/net.ax`** + 加 URL 解析（scheme/host/port/path/query 拆分 + percent-decode）。 |
| 17 | `socket` | P1 | ✅ **已有** | `web/sock_win.ax`（ws2_32）已有 TCP/UDP 实测可用。**上提为 `stdlib/socket.ax`**，`web/` 改为 import 它。搬的时候带上两个坑：int 返回零扩展（`i32()` helper）、指针返回是全 64 位。 |

### 6. 多线程 / 多进程

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 18 | `threading` | P2 | ⛔ **语言阻塞** | Win32 `CreateThread` 本身能 extern，但**线程入口必须是一个函数**。v0.40.0 的 fn-ptr 在**值位置**可用，`extern def` 的参数能不能收 fn-ptr **未验证**。若不能，`CreateThread` 就传不进去 —— 需要编译器侧给 extern 参数加 fn-ptr 类型。**先做这个最小验证实验再排期。** |
| 19 | `multiprocessing` | P3 | ⛔ | 依赖 `threading` 的入口问题，外加共享内存 / 管道 / 句柄继承。语言没有共享可变状态的一切原语（没有类、没有闭包、没有 `with`）。**整体推到最后，甚至考虑不做。** |
| 20 | `concurrent.futures` | P2 | ⛔ 依赖 18/19 | 线程池 = 18；进程池 = 19。**没有线程就没有这个模块。** |

### 7. 编码、哈希加密

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 21 | `base64` | P0 | ✅ **半有** | `stdlib/openai/codec.ax` 有 `oa_b64_encode_str`（**没有 decode**）。P0：抽出通用 encode + **补 decode**（`+`/`/` 变体、padding 校验）+ 标准/URL-safe 两套 alphabet。 |
| 22 | `hashlib` | P0 | ✅ | SHA-256 在 TLS/HTTP2 工作里已经用位运算跑过（v0.38.0 加 `&\|^~<<>>` 就是为此）。补 SHA-1 / MD5，以及 `IncrementalHash` 式分块喂入。 |
| 23 | `hmac` | P0 | ✅ | 建在 22 上，标准两趟 HMAC。 |

### 8. 命令行、工具

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 24 | `sys` | P3 | ⛔ **argv 阻塞** | 见 §0.2。子集可先做 `sys.platform`（= `target_os()`）、`sys.executable`（`exe_path()` 已有）、`sys.exit`（已有 `system_exit_code` 模式）。 |
| 25 | `argparse` | P3 | ⚠️ 部分 | 能解析传入的 `string` 数组，但拿不到 `sys.argv` → **实际不可用**。等 24。 |
| 26 | `logging` | P3 | ⚠️ 需先有 `time` | 本身不难（级别、handler、时间戳），但**要求 P0 的 `datetime`**。建议放 P2 尾巴而不是 P3。 |
| 27 | `traceback` | P3 | ⛔ 需编译器支持 | `raise`/`try` 只给了控制流，**没有运行时调用栈对象**。要真做，需要 codegen 在每个 raise 点发一条位置常量并在 unwind 时收集 —— **这是语言功能，不是库功能**，且要同步 selfhost 镜像。 |

### 9. 其他

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 28 | `math` | P0 | ✅ | `stdlib.ax` 已有 `sqrt/floor/ceil/abs/min/max/clamp/gcd/lcm/isqrt/is_prime/hypot`。补 `sin/cos/exp/log/log2/pow/fmod/trunc`（FFI）与 `pi/e` 常量。**顺手把名字改成 Python 风格**（`abs_i` → `iabs` 之类会破坏兼容，建议**加别名**不改旧名）。 |
| 29 | `random` | P1 | ✅ | **不要用 `rand()`**：MSVC 的 `rand()` 实现固定，同一个种子在任何机器上给同一序列。要跨机器可复现就自己实现 **PCG32 / xoshiro**，外部熵从 `BCryptGenRandom` 或 `SystemFunction036` 取。 |
| 30 | `typing` | P3 | ⚠️ 多数是 no-op | Aoxn **已经是静态类型 + 单态化**，`typing` 的运行时部分（`TypeVar`/`Generic`）在这里没有对应物。真正缺的是**别名与协议**：需要 `type X = Vec[int]` 形式的类型别名（spec roadmap 第 4 条「模块限定」的近邻）。**先别做**，文档里说明等价物即可。 |
| 31 | `unittest` | P3 | ⚠️ 形态要改 | 需要「类」来组织 fixture、`with` 来管资源 —— 两者都还没有。可行形态：`test_suite` 注册表 + 一个 `run_tests()` 入口 + `assert_eq/assert_true/assert_raises` 自由函数。**当前阶段的真正测试基建是 `cargo test`（`tests/*.rs`）+ `run_pkg_tests.sh`**，这个模块的收益要重新评估。 |

---

## 3. 每批次的通用任务清单

任何一个模块落地，下面这几件**都要做完**（AGENTS.md 的硬要求）：

- [ ] 模块本体 `stdlib/<name>.ax`，头部注释写清：**依赖哪些 extern、
      需要哪个 `-l` 链接库**（如 `ws2_32` / `winhttp` / `bcrypt`）
- [ ] **不放进 `stdlib/stdlib.ax`**（§0.1），除非 selfhost 真的要用
- [ ] `examples/<name>_demo.ax` 一个最小可跑示例
- [ ] `tests/<name>.rs` 测试；纯计算模块可无外部依赖，网络/文件模块
      用 **mock 服务**（照抄 `tests/openai_sdk.rs` 里那套 Aoxn 写的 mock server）
- [ ] `docs/<name>.md` 文档；`CHANGELOG.md` 记版本
- [ ] `docs/spec.md` 的 Roadmap 第 7 条「Standard library expansion」打勾
- [ ] 若用了 v0.40.0 新特性（dict/None/fn-ptr/raise），在文件头注明
      「**selfhost 尚不可编译**」，并确认固定点测试没把它纳入范围

---

## 4. 明确不做 / 用别的替代

写在这里，免得每次都重新讨论：

| 需求 | 不做的东西 | 用这个 |
|---|---|---|
| 进程间通信 | `multiprocessing` | `web/sock_win.ax` 的本地 socket；或起子进程 + stdin/stdout |
| 对象持久化 | `pickle`（无反射） | **JSON**（`stdlib/json.ax` 已有） |
| 调用栈回溯 | `traceback` | 编译器 `Diag`（诊断是编译期的，够用） |
| 正则引擎的极致性能 | `re` 的 NFA/DFA 优化 | 回溯版够用；真要快就换 `str.find` 手写扫描 |
| 类型体操 | `typing` | 现有静态类型 + 泛型单态化 |
| 随机可复现 | `rand()` | 自实现 PCG32（见 #29） |
| 测试框架 | `unittest` | `cargo test`（`tests/*.rs`） |

---

## 5. 建议的起手顺序（可直接开工）

1. **`stdlib/time.ax` + `stdlib/datetime.ax`** —— 最纯粹的 extern + 整数数学，
   零语言风险，且 `logging`/`calendar` 都等它。
2. **`stdlib/pathlib.ax`** —— 纯字符串运算，立刻能用，`os` 的门面基础。
3. **`stdlib/hashlib.ax` + `hmac` + `base64` decode** —— 算法验证密集、
   测试好写（对拍已知向量）。
4. **`stdlib/glob.ax` + `os` 遍历** —— 第一次碰 Win32 目录枚举。
5. **`Vec[T]` 堆容器基座** —— P2 的入场券。
6. **把 `stdlib/openai/json.ax` 提升为 `stdlib/json.ax`** —— 已有实现，
   是最快的一块 P1。