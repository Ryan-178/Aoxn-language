# 标准库路线 TODO（stdlib roadmap）

> 工作文档，不是规范。语言规则见 `docs/spec.md`，两套 API SDK 见
> `docs/openai-sdk.md` 与 `docs/anthropic-sdk.md`（它们共用的底层在
> `stdlib/net/`）。本文件只回答一个问题：**接下来按什么顺序、把哪些
> Python 标准库模块搬进 Aoxn，卡在哪里。**
>
> 模块编号沿用两份需求清单的原编号：§2 是标准库 1–31，§5 是第三方 1–15，
> 方便对照。

---

## 0. 动手之前：六条硬约束

这六条是**已核实**的，不是猜测。它们决定了后面每个模块的可行形态；
写模块之前先读这一节，能省掉大部分返工。

（0.1–0.4 对标准库模块适用；0.5–0.6 主要是为第二份清单里的
科学计算 / 数据分析库准备的，见 §5。）

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

### 0.5 没有 `enum`、没有可运行时标记的联合、没有 `Any` —— 异构记录做不了

v0.40.0 的唯一联合形式是 `T | None`，且 `dict[V]` 的**值类型只有一个**。
所以「一列里既有 int 又有 string」「一行数据库记录有多种类型」这类数据结构
**在语言层面无法表达**。

被这条直接判死的：`pandas` 的 DataFrame（§5 #3）、所有 ORM 的行对象
（§5 #12/#14）、`numpy` 的运行期 dtype 分发（§5 #2）。

**已有的正确范式就在仓库里**：`stdlib/net/json.ax` 的 JSON DOM 是一个
**40 字节带 tag 的 slab**（kind / i64 / f64 / ptrA / ptrB），用它绕开了「JSON
值可以是任意类型」这个问题。凡是要做动态值的库（Variant、DataFrame、ORM 行），
**照抄这个 tagged-slab 设计**，不要试图发明别的。

代价是访问要走 accessor（类型检查从静态掉到运行期），换来的是通用性。
文档里要写明这层访问开销，别让用户以为是零成本的。

### 0.6 造不出 `inf` / `nan`，浮点除零也没有保护 —— 所有数值代码都要自己兜

两条都核实过：

- **词法层没有 `inf` / `nan` 字面量**。`src/lexer.rs::scan_number` 只接受
  `数字 [. 数字]`，`codegen_c.rs:537` 那个 `contains("inf")` 是 Rust 侧
  **打印** f64 常量时的分支，不是词法支持。
- **整数的位运算戳不进 float**。v0.38.0 的 `& | ^ ~ << >>` 与 `%` 一样是
  **int-only**，不能拿 IEEE754 的位模式去造 NaN。
- **浮点除零原样发射**（`src/codegen_c.rs:2153`，`Div => "/"`），C 里
  `0.0/0.0` 是**未定义行为**，不是保证给 NaN。

**可行的绕法（都实测可写）**：

- 造 NaN：`extern def log(x: float) -> float`，调 `log(-1.0)`
- 造 ±Inf：`log(0.0)` 给 `-inf`，`-log(0.0)` 给 `+inf`
- 除法：库里统一走一个 `fdiv(a, b)` 辅助函数，自己判零并返回哨兵值

`numpy` / `scipy` / `matplotlib` 这三个库**全部**踩在这条上，它们的每一个
公式（归一化、插值、积分、缩放坐标轴）都得考虑除零与 NaN 传播。
建议在 `stdlib/math.ax` 里就把 `fdiv` / `is_nan` / `is_inf` 定下来，别让
每个库各写一遍。

---

## 1. 批次总览

| 批次 | 主题 | 模块 | 定位 |
|---|---|---|---|
| **P0** | 基座 + 纯 extern/math 能吃的 | `vec` 基座、`time`、`datetime`、`calendar`、`hashlib`、`hmac`、`base64`(补 decode)、`os`(核心)、`pathlib`、`glob` | 立刻能做，风险最低 |
| **P1** | 解析与序列化 | `json`(已有，需提升)、`csv`、`re`、`shutil`、`bisect`、`heapq`、`socket`(已有需上提) | 有工程量但形状清晰 |
| **P2** | 容器与并发 | `collections`、`itertools`、`urllib`、`threading`、`concurrent.futures` | 前两个依赖 P0 基座；线程可行（见 #18），但同步原语要从 extern 起步 |
| **P3** | 语言级阻塞 | `sys`、`argparse`、`multiprocessing`、`pickle`、`logging`、`traceback`、`typing`、`unittest` | 需要先改语言或改需求形态 |

第二份清单（第三方 / 大型库，见 §5）**不套用这四个批次**——它们不是标准库
模块，而是**依赖链上的项目**：`numpy` 在 `scipy` 之前，`Pillow` 在 `opencv`
之前，`matplotlib` 在 `seaborn` 之前，`socket` 在 `requests` 之前。按依赖顺序
排，不按批次排。

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
| 8 | `json` | P1 | ✅ **已有** | `stdlib/net/json.ax` 已经是一个完整 JSON DOM（40 字节 slab、parse/dumps、往返测试都过了）。**P1 的工作是把它提升为 `stdlib/json.ax` 独立模块 + 补文件 I/O**，不是从零写。 |
| 9 | `csv` | P1 | ✅ | RFC 4180：引号转义、内嵌换行、`\r\n`。**没有 `\r` 字符串转义**（AGENTS.md：只有 `\n \t \\ \"`），CRLF 要用 `store_u8` 写字节 13。 |
| 10 | `pickle` | P3 | ⛔ **无反射** | 语言没有「遍历一个值的类型/字段」。可行替代是**显式注册**：程序自己声明要序列化的字段清单，`pickle_dump(recs)`。这样能往返，但和 Python pickle 格式不兼容 —— 若目标是「读 `.pkl` 文件」，**直接不做**。 |
| 11 | `re` | P1 | ⚠️ 大工程 | 回溯匹配器（支持 `* + ? \| () [] {m,n} .`、字符类、非贪婪、锚点）+ 一个手写 DFA 编译步骤会太大。**先只做回溯版**，用 `Vec` 存回溯栈（两个 int 栈：位置、备选点）。预计 2–4k 行。锚定建议：跟一个「HTML/XML 标记提取」的真实用例，避免做成正则引擎而没人用。 |

### 4. 容器、数据结构

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 12 | `collections` | P2 | ⚠️ 依赖 P0 `Vec[T]` | `deque`（环形缓冲）、`defaultdict`、`Counter`、`OrderedDict`。`dict[V]` 是**堆句柄**（v0.40.0），复制句柄=共享同一个 dict。**插入序底层已经有了**（`for k in d` 按插入序走 key 数组，`codegen_c.rs:1361`），所以 `OrderedDict` 的真正成本是**查找是 O(n) 线性扫描**（`ax_dict_find_T`，codegen_c.rs:789）——不是保序，是规模。超过几百条就要考虑换结构。 |
| 13 | `itertools` | P2 | ⚠️ **没有 `yield`** | 惰性迭代器做不了（`yield` 在 spec roadmap 第 9 条）。**形态改成"批量返回数组"**：`chain(list, list) -> Vec[T]`、`permutations(n) -> Vec[Perm]`、`product(a, b) -> Vec[Tuple]`。无限迭代器（`count`/`cycle`）改为「带 `take(n)` 的生成函数」。**这个降级要在模块头注释里写明**。 |
| 14 | `bisect` | P1 | ✅ | 便宜。`bisect_left/right` 对有序 `Vec[T]`（**注意：泛型按长度单态化，长度是类型的一部分** → 变长容器要用堆容器）。 |
| 15 | `heapq` | P1 | ✅ | 标准二叉堆，`heap_push`/`heap_pop`/`heapify`。和 `bisect` 一起做。 |

### 5. 网络、请求

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 16 | `urllib` | P2 | ✅ **部分已有** | `stdlib/net/http.ax` 已经是一套 WinHTTP 传输（open/connect/send/receive/timeouts）。**P2 是把它泛化成 `stdlib/net.ax`** + 加 URL 解析（scheme/host/port/path/query 拆分 + percent-decode）。 |
| 17 | `socket` | P1 | ✅ **已有** | `web/sock_win.ax`（ws2_32）已有 TCP/UDP 实测可用。**上提为 `stdlib/socket.ax`**，`web/` 改为 import 它。搬的时候带上两个坑：int 返回零扩展（`i32()` helper）、指针返回是全 64 位。 |

### 6. 多线程 / 多进程

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 18 | `threading` | P2 | ✅ **可行** | `extern def` 确实拒绝 fn-ptr 参数（typecheck.rs:112），**但错误信息自己给了逃生口：把函数地址当 `int` 传**。`CreateThread(0, 0, to_int(worker), 0, 0, 0)` 在 x86-64 上 ABI 是通的（函数指针与数据指针同宽同寄存器）。真正缺的是上层同步原语：`CreateMutex` / `WaitForSingleObject` / `SRWLock` / `Interlocked*`，都能 extern。**详见 `docs/language-gaps.md` §3。** |
| 19 | `multiprocessing` | P3 | ⛔ | 线程入口不再是问题，但**共享内存 / 管道 / 句柄继承**要写，语言没有共享可变状态的一切原语（没有类、没有闭包、没有 `with`）。**整体推到最后，甚至考虑不做** —— 替代方案是本地 socket（`web/sock_win.ax`）或子进程 + stdin/stdout。 |
| 20 | `concurrent.futures` | P2 | ⚠️ 依赖 18 | 线程池建在 18 之上，现在可行了；进程池仍卡在 19。 |

### 7. 编码、哈希加密

| # | 模块 | 批次 | 可行性 | 备注 |
|---|---|---|---|---|
| 21 | `base64` | P0 | ✅ **半有** | `stdlib/net/codec.ax` 有 `net_b64_encode_str`（**没有 decode**）。P0：抽出通用 encode + **补 decode**（`+`/`/` 变体、padding 校验）+ 标准/URL-safe 两套 alphabet。 |
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
| 测试框架 | `unittest`、`pytest` | `cargo test`（`tests/*.rs`） |
| 深度学习 | `pytorch` | numpy + scipy 那一层；要 GPU 就不在 Aoxn 里做 |
| 计算机视觉 | `opencv-python` 全家桶 | 明确只做子集（灰度/模糊/Sobel/阈值/缩放） |
| 数据库对象映射 | ORM（sqlalchemy / django 的那一半） | `stdlib/db.ax` 的参数化查询 + 按表生成 struct |

---

## 5. 第三方 / 大型库（第二份清单）

这 15 个不是标准库模块，是**依赖链上的项目**。编号沿用需求清单原编号 1–15。
图例同 §2。

### 5.1 依赖顺序

```
stdlib/socket.ax ──► requests(1)
stdlib/os.ax ──────► Pillow(7) ──► opencv(8)
stdlib/math.ax ────► numpy(2) ──► scipy(6)
                      │           pandas(3, 需先解决 §0.5)
                      └───────► matplotlib(4) ──► seaborn(5)
stdlib/net.ax ─────► flask(13) ──► django(12)
stdlib/re.ax ──────► lxml(10), beautifulsoup4(9)
```

### 5.2 清单

| # | 库 | 可行性 | 备注 |
|---|---|---|---|
| 1 | `requests` | ✅ **P1，高回报** | `stdlib/net/http.ax` 的 WinHTTP 传输已经在了，`requests` 就是它上面一层：session/cookie 持久化、重定向策略、`params`/`json=` 便捷参数、`.raise_for_status()`。**接口层一天能写完**，是第三方清单里最便宜的高价值项。 |
| 13 | `flask` | ✅✅ **最高 ROI，建议排第一** | 零件基本齐了：`web/serve.ax` 是一个能跑的 HTTP/1.1 服务器（静态文件 / ETag / Range / `/metrics` 都有，`docs/web-benchmark.md` 里有实测数据）。**v0.40.0 的 fn-ptr 让路由表第一次可表达**——`struct Route{pattern, handler: fn(Request) -> Response}`。剩下的是路由匹配、`Request`/`Response`、Jinja 子集模板、session cookie。**把 `web/serve.ax` 泛化成 `stdlib/wsgi.ax`**，flask 建在上面。 |
| 9 | `beautifulsoup4` | ✅ **P1，高回报** | HTML 分词 + 树构建，**和 `selfhost/parser.ax` 是同一类活**，有现成参照可抄结构。要补的是：几百个 HTML 实体的解码表（`&amp;` `&#x4e2d;` …）、HTML5 的容错规则（隐式闭合、错误嵌套）、CSS 选择器求值。**和 `re`(§2 #11) 一起做最划算**，两者互为用例。 |
| 10 | `lxml` | ✅ P1 | XML 比 HTML 简单得多（**没有容错要求**，解析器可以短一半），XPath 求值也直接。⚠️ 但 lxml 的核心卖点是「比 bs4 快」——在 Aoxn 里 bs4 还不存在，比较没有意义。**按「XML + XPath」定位就行**，别承诺性能。 |
| 2 | `numpy` | ⚠️ **P2，最大工程** | 需要 `struct NDArray{data, shape, stride, ndim}` + 步进索引 + 广播 + 花式索引 + 归约轴。**但 dtype 的运行期分发被 §0.5 判死**——现实做法是**按 dtype 各一个单态化类型**（`f64arr` / `i64arr`），靠泛型让同一份算法为每种类型各编一份（§0.3 的泛型机制支持这个）。程序自己选类型，编译器保证没有混合运算。**这个降级必须在模块头写明**，它和 numpy 的语义不同。 |
| 6 | `scipy` | ⚠️ **P2/P3，最机械** | 纯数值、无 I/O，所以**语言约束最少**：积分（自适应辛普森）、优化（BFGS/牛顿）、线性代数（LU / QR / 特征值）、信号（FFT / 滤波）。每个算法都是循环和算术，没有语言层面的坎。**但每一个都踩 §0.6**（除零、NaN 传播）和**没有 `**` 运算符**（`pow_f` 自己写，spec roadmap 第 1 条）。建议在 `numpy` 之后做。 |
| 4 | `matplotlib` | ✅ **P2** | **现成的路已经铺好了**：绘图后端直接建在 UI 工具包的 `plat_fill_rect` / `plat_text` / `plat_measure` 上（`stdlib/ui_draw.ax`，已经是 GDI 实测可用），再包一层坐标变换、刻度、图例。⚠️ **但 `plat_*` 目前没有画线/多边形的图元**（只有矩形、文字、裁剪、pump），要先给 `ui_draw.ax` 加 `plat_line` / `plat_polyline`，并**在 `ui_win.ax` 里实现**——`tests/ui.rs` 那三个契约测试（widget 层不得出现平台符号、两个后端实现同一 `plat_*` 集、每个被调的 `plat_*` 两边都存在）会强制你两边都做，**不要只改一边的后端**。 |
| 5 | `seaborn` | ⚠️ 依赖 4 | 统计默认值（分箱、置信带）、KDE、配色板。`matplotlib` 落地后成本很低，**跟着 4 一起做，不要单独立项**。 |
| 7 | `Pillow(PIL)` | ⚠️ P2，分档 | BMP 读写很容易（头 + 像素）。**PNG 需要自己实现 zlib inflate**（可行，~600 行，还要配 deflate 才能写）。**JPEG 解码是大工程**（基线解码器 ~1500 行，含霍夫曼、IDCT、YCbCr）。另一条路是 WIC（Windows 自带的图像组件），但它是 COM 接口，定长 `extern def` 调 COM 会很别扭——**优先自己写**。图像算子（裁剪/缩放/灰度/旋转）都建立在解码之上，所以格式支持决定了这个库的规模。 |
| 3 | `pandas` | ⛔ **需要重新设计** | DataFrame 的核心卖点就是**一列可以混合类型**，正好撞在 §0.5 上。两条出路：(a) 每列固定类型 → 那就是 numpy 的列式视图，不是 pandas；(b) 走 `json.ax` 那种 **tagged slab** → 通用性有了，访问要经 accessor、运行期判类型、性能大幅下降。**建议 (b) 作为「够用就好」的版本，明确写进文档它不是 pandas。** 依赖 `numpy`(2) 与 `csv`(§2 #9)。 |
| 14 | `sqlalchemy` | ⛔ ORM 层阻塞 / ✅ 底层可行 | 一行数据库记录必然混合类型 → §0.5，ORM 的行对象建不出来。**分解做法**：先做 `stdlib/db.ax`（ODBC 或 WinSQL 的参数化查询、连接池、事务、结果集游标），**把 ORM 砍掉**；真要对象映射，改成「按表生成 struct + 代码生成」，而不是运行期反射。这是有价值的部分，也是唯一可行的部分。 |
| 12 | `django` | ⚠️ **拆开看，一半可行** | 模板引擎、请求/响应、中间件、认证 session —— 在 flask 那套之上都能做。**但 admin / forms / ORM 全部依赖 ORM 与反射**（§0.5）。**结论：不要做「django」，做 flask + 模板引擎。** django 的价值主要在 ORM 和 admin，正是这里做不了的部分。 |
| 8 | `opencv-python` | ⛔ **远期，只做子集** | 依赖 `numpy`(2) + `Pillow`(7)，而「opencv」的本体是几百个精心优化的算子。现实可行的子集：灰度化、高斯模糊、Sobel、二值化、缩放、形态学开闭运算。**性能不可能接近 C++ 版本**（Aoxn 无 SIMD、无内联汇编、逃逸分析有限）。**要张量运算就做 numpy 那一层，不要追 opencv 的性能。** |
| 11 | `pytorch` | ⛔ **建议不做** | GPU 要 CUDA，语言层没有相邻的任何东西。CPU 反向自动微分的 tape 引擎理论上可行（显式栈代替闭包），但真实代价远超收益。**要做数值计算就做 numpy + scipy 那一层**，深度学习框架是另一个量级的工程。 |
| 15 | `pytest` | ⚠️ **优先级低** | 发现用例要 argv（§0.2）；fixture 需要 `yield`（没有）→ 只能改成 setup/teardown 显式配对；`parametrize` 可做。v0.40.0 的 `raise`/`try` 让 `assert_raises` 能写。**当前阶段的真正答案仍是 `cargo test` + `tests/*.rs`**，`pytest` 只有在语言模块本身需要自测时才值得做。 |

### 5.3 第三方库通用约定

- [ ] **先确认它依赖的标准库模块已落地**（见 §5.1 依赖图），不要在缺基座时开工
- [ ] 若绕过 §0.5 / §0.6，**在模块头注释写明降级形态**（同 §3 最后一条）
- [ ] 每个数值库都要跑一遍**边界用例**：除零、空数组、单元素、`NaN` 输入
- [ ] `docs/<name>.md` 里**明确写「这不等于 Python 的同名库，差异在哪」**——
      降级过的库（numpy 的 dtype、pandas 的列类型、db 的无 ORM）尤其要写
- [ ] 若扩展了 UI 工具包的 `plat_*`，**两个后端都要实现**，`tests/ui.rs` 会查

---

## 6. 建议的起手顺序（可直接开工）

1. **`stdlib/time.ax` + `stdlib/datetime.ax`** —— 最纯粹的 extern + 整数数学，
   零语言风险，且 `logging`/`calendar` 都等它。
2. **`stdlib/pathlib.ax`** —— 纯字符串运算，立刻能用，`os` 的门面基础。
3. **`stdlib/hashlib.ax` + `hmac` + `base64` decode** —— 算法验证密集、
   测试好写（对拍已知向量）。
4. **`stdlib/glob.ax` + `os` 遍历** —— 第一次碰 Win32 目录枚举。
5. **`Vec[T]` 堆容器基座** —— P2 的入场券。
6. **把 `stdlib/net/json.ax` 提升为 `stdlib/json.ax`** —— 已有实现，
   是最快的一块 P1。

第三方清单接在后面（依赖齐了才动）：

7. **`stdlib/socket.ax` 上提 + `stdlib/net.ax`** —— `requests`(1) 和
   `flask`(13) 都等它。
8. **`stdlib/re.ax`** —— 和 `lxml`(10)、`beautifulsoup4`(9) 共用分词基础。
9. **`flask`(13)** —— 第三方清单里 ROI 最高，零件已经齐了大半。
10. **`stdlib/math.ax` 补 `fdiv` / `is_nan` / `is_inf`** —— §0.6 说这是
    三个数值库的公共前置，**先做它，比做 numpy 更划算**。