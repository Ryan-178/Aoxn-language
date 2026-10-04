# 语言能力缺口分析（language gaps）

> 这份文档回答一个问题：**为什么某些库做不了。**
>
> `docs/stdlib-todo.md` 回答「接下来做什么、按什么顺序」；这份回答
> 「缺的是语言里的哪一条、补上它要付什么代价、解锁了什么」。
> 两条清单（31 个标准库模块 + 15 个第三方库）的可行性判断全部指向这里。

---

## 0. 怎么读

每条缺口用同一个格式写：**缺什么 → 挡住了什么 → 要动哪里 → 代价 → 解锁**。
「要动哪里」给的是**实测的文件清单**，因为 Aoxn 的语言改动成本高度可预测
（见 §6）。

| 缺口 | 一句话 | 直接挡住 |
|---|---|---|
| **A1** 没有引用类型 | 全值语义，没有 `&T` | 一切「共享可变对象」的模型 |
| **A2** 没有 `sizeof`、不能取结构体地址 | 容器只能装 8 字节槽 | 所有非槽位类型的容器 |
| **A3** 没有 GC，拼接与 dict 泄漏 | 只增不减 | 长驻进程（web 框架） |
| **A4** 没有反射 | 无法在运行期问「这是什么」 | pickle、ORM、fixture 注入 |
| **B1** 没有 `enum` / 可标记联合 / `Any` | 唯一联合是 `T \| None` | pandas、所有 ORM、numpy dtype |
| **B2** `dict[V]` 单值类型 + **O(n) 查找** | 「一个设置表，不是一个数据库」 | 大规模映射 |
| **B3** 没有元组 / 多返回值 | `def f() -> int` 只能返回一个 | 大量自然的 API 形态 |
| **B4** 没有命名空间，`import` 全部合并 | 重名是**硬错误** | 46 个模块的规模化 |
| **B5** 泛型只按 (类型, 长度) 单态化，无约束 | 长度是类型的一部分 | 通用容器、numpy 分发 |
| **C1** extern 拒绝 fn-ptr 参数 | **但有逃生口**（见 §3） | ~~线程~~ ——实际不挡 |
| **C2** 没有闭包 / lambda | 函数指针只指顶层函数 | 回调生态、装饰器 |
| **C3** 没有 `yield` / 生成器 | | itertools 惰性、游标 |
| **C4** 没有类 / 方法 / 接口 | | unittest fixture、ORM 实体、多态 |
| **C5** 没有运算符重载 | | `a + b` 的形状语法、BigInt |
| **C6** 没有 `with` | | 确定性资源释放 |
| **D1** argv 缺失（`int main(void)`） | | sys、argparse、pytest、CLI |
| **D2** 没有线程/同步原语 | 见 §3，线程本身可行 | 线程池、锁、条件变量 |
| **D3** 浮点与整数除零都是 UB，无检查 | 原生崩溃 | 所有数值库 |
| **D4** 没有 `inf`/`nan` 字面量，位运算 int-only | 戳不进 float 的 IEEE 位 | 所有数值库 |
| **D5** float 只能 `%f` 6 位小数 | | 科学计数法、刻度标签 |
| **E1–E8** 语法糖 | 见 §5 | 零散但面广 |

---

## A. 内存与所有权

### A1 没有引用类型 —— 最大的单点代价

**缺什么**　`docs/spec.md:171`：「There are no references or pointers yet」。
赋值、传参、返回全部按值拷贝（memcpy）。

**挡住了什么**

- **一切「多处可见的同一对象」**。唯一的例外是**堆句柄**
  （`dict[V]`、`TableModel`、`FileTable`、`Vec` 都是 `struct X*`）。
  想让对象可共享，唯一的办法就是把它做成句柄——于是每个容器的
  「是不是同一个东西」语义都不一致。
- **别名悬垂**。这不是理论问题：v0.39.2 的 `0xC0000374` 崩溃
  （`selfhost/typecheck.ax` 的 cycle detector 按值传递 DFS 路径 `Vec`，
  递归时 realloc 释放了所有副本共同指向的缓冲区）**就是这个缺口的直接
  产物**。它潜伏了很久，只因内联扩容预留 8 个槽位、头八次 push 不触发
  realloc 才没炸；换成委托形式就必炸。AGENTS.md 已把它定为「根因」。
- ORM 实体的 identity 缓存、事件监听器表、任何「同一处修改处处可见」。

**要动哪里**　`src/parser.rs`（`&` 已在位运算符里，要区分前缀/中缀）、
`src/ast.rs`、`src/typecheck.rs`（引用类型不参与拷贝语义）、
`src/codegen_c.rs`（大部分已是指针发射，可能只差赋值处的拷贝抑制）、
`selfhost/parser.ax` + `selfhost/typecheck.ax` + `selfhost/codegen.ax`、
`docs/spec.md`、`tests/pipeline.rs`。

**代价　大**。它会**削弱「无隐式共享」这个核心卖点**。spec 的设计目标里
「strict static typing, value semantics」是与 C++ 类速度并列的；
引入引用要么做成显式 `ref`（安全但啰嗦），要么做成默认引用（那本质上是
改语言的性格）。**建议：保持现状，把「共享 = 堆句柄」写成显式约定。**

### A2 没有 `sizeof`，不能取结构体地址

**缺什么**　`docs/spec.md:425`：「There is deliberately **no `sizeof`** and
no way to take the address of a struct value (`as_ptr` works on a `string`
only).」

**挡住了什么**　**任何非 8 字节值的容器。** `Vec` 只能装槽位，所以
「`Point` 的列表」必须拆成 x 槽 + y 槽（`VecVec` 干的就是这件事），
或者指向逐个分配的记录。直接后果：

- numpy 的结构化数组 / 非 8 字节 dtype
- DataFrame 的列存储
- 任何 `Vec[SomeStruct]`（`Vec[T]` 基座本身就得先解决这个）

**要动哪里**　`src/codegen_c.rs` 发射一个 `ax_sizeof_<Struct>()` 静态函数，
`selfhost/codegen.ax` 镜像它，再给 `as_ptr` 放开 struct。
**这是本文档里最便宜的一条**——发射侧几乎是现成的（C 里
`sizeof(struct X)` 编译器知道），主要成本在自举镜像和文档。

**代价　小。解锁　大** —— 这是 numpy / Pillow / 通用容器的共同前置。

### A3 没有 GC，字符串拼接与 dict 值永不释放

**缺什么**　`docs/spec.md:96`：拼接结果堆分配且**故意不释放**；
`docs/spec.md:757`：dict 值也活到程序结束。

**挡住了什么**　**长驻进程**。flask 每处理一个请求就拼若干临时字符串，
RSS 单调增长到进程结束。现有 web 服务器是靠预分配的字节缓冲区
（`web/http_buf.ax`）绕过去的，那是**为服务器专门写的纪律，不是语言给的能力**。
任何通用 web 框架都会撞上。

**要动哪里**　需要 codegen 知道谁持有谁 → 逃逸分析或保守的「永不释放 +
arena」策略（arena 只在已知边界处整块释放，比逐个 free 容易得多）。

**代价　大。** 但注意：**这是一个「已知会疼但可以晚疼」的问题**——
只有在写长驻服务时才疼。

### A4 没有反射

**缺什么**　没有任何运行期类型查询：这个值是什么类型、有哪些字段、
这个函数签名是什么。

**挡住了什么**　`pickle`（§2 #10）、所有 ORM（§5 #14/#12）、
pytest 的 fixture 按参数名注入、任何「把对象转成 JSON」或
「从 JSON 还原成对象」的通用实现。

**对冲手段**　**显式注册**：程序自己声明字段清单，
`dump(recs: Vec[Rec])`。这能往返，但不是 Python 的语义。
现存范式见 `stdlib/openai/json.ax`——40 字节带 tag 的 slab。

**代价　大**（需要 codegen 建类型元数据表 + 两种编译器一致）。

---

## B. 类型系统

### B1 没有 `enum`、没有可运行时标记的联合、没有 `Any`

**缺什么**　v0.40.0 的唯一联合形式是 `T | None`（`docs/spec.md:36`），
且 `dict[V]` 的**值类型只有一个**（`docs/spec.md:637`）。

**挡住了什么**　**异构记录在语言层面无法表达**：

- `pandas` 的 DataFrame —— 一列可以混合类型正是它的核心卖点
- 所有 ORM 的行对象 —— 一行数据库记录必然混合类型
- `numpy` 的运行期 dtype 分发
- 任何 `Variant` / `Any` / `object` 语义

**现成的正确范式就在仓库里**：`stdlib/openai/json.ax` 用一个
**带 tag 的 slab**（kind / i64 / f64 / ptrA / ptrB）表达「JSON 值可以是
任意类型」。它绕开了这个问题，代价是访问要走 accessor、类型检查从静态
掉到运行期。**要动态值的库都该抄这个设计**——包括 pandas 的降级版。

**要动哪里**　`src/ast.rs`（新的类型节点）、`src/typecheck.rs`（tag 分支
收窄，与 `is None` 收窄同机制）、`src/codegen_c.rs`（tag 发射）、
自举镜像三件套、`docs/spec.md`。

**代价　大**　但它是 pandas 与 ORM 的**唯一出路**。

### B2 `dict[V]` 是单值类型，且查找是**线性扫描**

**缺什么**　`src/codegen_c.rs:749` 的注释直说：查找是线性扫描；
`ax_dict_find_T`（:789）遍历 key 数组比字符串。插入序是保留的
（`for k in d` 按插入序走 key 数组，`src/codegen_c.rs:1361`）。

**挡住了什么**　任何超过几百条目的映射。spec 自己的措辞是
「a settings map, not a database」（`docs/spec.md:647`）。

**修正一处我此前的判断**　我先前认为 `OrderedDict` 需要自己维护插入序索引
——**不对，底层已经有序了**。`collections.OrderedDict` 的真正成本是
**每次查找 O(n)**，不是保序。

**代价　中**（换开放寻址或排序数组即可）。
**解锁　大**（会话状态、路由表、词表都受益）。

### B3 没有元组 / 多返回值

**缺什么**　函数返回一个值（`docs/spec.md:183`）。
`def f() -> int` 就是整数，没有 `(int, int)`。`raise`/`try` 也只能传
`string` 载荷。

**挡住了什么**

- `a, b = f()`、无临时变量的 swap
- 一次调用取多个结果——**库 API 最自然的形态**
- 「成功时给一个值，失败时给一个错误」的形状；现在只能用 out-slot
  （`docs/spec.md:448`）或 tagged slab 绕
- 科学计算的 `(值, 误差)` 这类返回对

**要动哪里**　`src/ast.rs`（Tuple 节点）、parser、typechecker、
codegen（sret 已有，AGENTS.md 说聚合 ABI 早就是指针+out 指针）。
**自举镜像同理。**

**代价　中。** 它不新造机制，只是把**已经存在的 sret ABI** 提升到源码层。

### B4 没有命名空间：所有 `import` 合并成一个命名空间

**缺什么**　`docs/spec.md:201`：「load another file and **merge it into
one namespace**」。即使写 `import { helper, Vec } from ...`，spec 也注明
「M1 merges all names」。**没有 `lib.sort(...)` 限定**（roadmap 第 4 条）。

**后果比风格问题严重**　**重名是硬错误**：
`src/typecheck.rs:94` → `function 'X' is defined more than once`。

所以**每个 stdlib 模块必须手工给每一个名字加前缀**。这不是猜测，是仓库里
既成的事实：

| 模块 | 前缀 | 顶层函数数 |
|---|---|---|
| `stdlib/ui_draw.ax` | `ui_` / `plat_` | 50 |
| `stdlib/openai/client.ax` | `oa_` | 40 |

加上 `stdlib.ax` 的 `vec_` / `err_` / `out_` / `str_`。
**46 个规划模块 × 每个几十个函数**，靠手写前缀维持，是一条会越来越紧的绳子。

**要动哪里**　`src/parser.rs` 的 import 形式、`src/lib.rs`（保留模块身份）、
`src/typecheck.rs`（名字解析加命名空间层）、`src/codegen_c.rs`
（C 符号改名以避免 clang 侧撞名——AGENTS.md 已有 keyword 表，
命名空间前缀是同一机制的扩展）、`selfhost/load.ax` + typecheck + codegen 镜像。

**代价　中～大**。**解锁　最大**：它是 46 个模块的地基。
**这项排第一** —— 不是因为它解锁了某个具体的库，而是因为**不做它，
stdlib 每加一个模块都在给未来的自己挖坑**。

### B5 泛型只按 (类型, 长度) 单态化，且无约束

**缺什么**　`docs/spec.md:158`：「**At most one length parameter per
function**」。长度是**类型的一部分**（`[T; N]`），泛型按 (类型, 长度)
各编一份（`docs/spec.md:161`）。

**挡住了什么**

- **变长容器**——`Vec[T]` 基座必须做在堆上，不能是数组
- **签名里有两个不同长度的数组**——仍不支持，这直接塑造了 UI 工具包的
  设计（table/tree 因此接受堆上的 `TableModel` 而不是并行数组）
- **「要求 `T` 可比较」这种约束**——现在只能在实例化时逐个检查，
  所以 `sort[T]` 对 int/float/string 行、对 struct 不行，**写法完全一样，
  结果取决于实例化**。numpy 的 dtype 分发要落地，只能按 dtype 各一个类型
  （`f64arr` / `i64arr`），靠单态化让同一份算法为每种类型各编一遍。

**代价　大**（约束泛型要重写实例化算法和所有调用点的推断）。

---

## C. 抽象与回调

### C2 没有闭包 / lambda

**缺什么**　`docs/spec.md:601` 的函数指针是一个**裸地址**：无捕获环境。
`cb = handler` 只能指顶层函数。

**挡住了什么**

- 携带上下文的回调（「把这个字典的每项传给函数」里的那个字典）
- **装饰器**
- itertools 的惰性链式运算（`itertools.chain(map(f, xs), ys))`——两种写法都做不了：
  既没有 `map`，也没有能捕获的 `f`
- UI 事件总线只能退化成一个**整数信号**（`ui_connect(c, signal, slot)` +
  `ui_emit(c, signal, kind, a, b)`），让 app 自己 switch 分发——
  **这个设计是被 C2 逼出来的，不是选择**

**要动哪里**　全新的闭包转换（词法作用域捕获 → 环境记录）、
codegen 的环境布局、自举镜像。闭包会引入**共享可变状态**，
而 Aoxn 现在完全没有（A1）——所以闭包捕获的变量怎么算，是个真问题。

**代价　大。**

### C3 没有 `yield` / 生成器

**缺什么**　roadmap 第 9 条。

**挡住了什么**　惰性迭代器（`itertools` 的核心卖点）、数据库游标的自然写法、
惰性解析器（bs4 的流式解析）、`pipelines`。

**可做的替代**　**批量返回数组**，或者「显式状态机」——把生成器写成
「一个函数 + 一个携带位置的 state 结构」。UI 工具包的全状态机（`st` 堆块）
就是这种写法的成功范例。

**代价　大**（需要改调用约定、栈帧结构、codegen 全面）。

### C4 没有类 / 方法 / 接口（trait）

**缺什么**　只有 `struct`（纯数据，字段只能按值读写）和自由函数。

**挡住了什么**

- `unittest` 的 fixture 组织、pytest 的 fixture 参数注入
- ORM 实体与多态
- 任何「同一族类型各自实现一个行为」

**现有的对冲**　**指针 struct + get/set 函数**：`TableModel`/`TreeModel`
就是这个形状（AGENTS.md 的「Model/view without interfaces」）。
可行但笨：没有方法分派、没有继承、每个「虚调用」是一根函数指针。

**代价　大**（类型系统要加方法解析和 trait 求解）。

### C5 没有运算符重载

**缺什么**　`+` 等按**语法**固定语义，不查用户函数。

**挡住了什么**

- numpy 的 `a + b`（只能写成 `add(a, b)`，可读性损失）
- BigInt / 任意精度数
- 矩阵运算的直觉写法

**要动哪里**　typechecker 里每个运算符节点要接入函数查找；
codegen 侧把 `a + b` 改写成 `op_add(a, b)`。

**代价　中**　—— 出奇地便宜，但会**削弱「严格、无隐式转换」的可验证性**
（一个 `+` 可能意味着完全不同的东西）。要仔细限制触发条件。

### C6 没有 `with` / 确定性析构

**缺什么**　roadmap 第 9 条。**没有 RAII。**

**挡住了什么**　文件、socket、锁的确定性释放。**而且 v0.40.0 引入了
`raise` 之后这个需求变强了**——异常展开时资源不释放是真实泄漏路径。

**代价　中**　—— 主要是 codegen 里的展开期清理表，以及 selfhost 镜像。

---

## D. 运行时与平台

### D1 argv 缺失

**缺什么**　`src/codegen_c.rs:653` 发射 `int main(void)`。
入口签名不带参数，程序**无法知道自己被怎么调起来的**。

**挡住了什么**　`sys.argv`（§2 #24）、`argparse`（#25）、
`pytest` 的用例发现（#15）、所有 CLI 工具。

**要动哪里**　codegen 发射 `main(int argc, char** argv)`，
把参数落进一个运行时结构，`sys` 从那里读；
**必须同步 `selfhost/codegen.ax`**（AGENTS.md 明确：两个编译器要一致，
否则固定点破）。

**代价　小。** 这是 §2 #24/#25/#15 三项的共同前置。

### D2 没有线程与同步原语

**缺什么**　语言层没有 thread / mutex / condition / atomic。
**但这一点被普遍误解了**——见 §3。

**代价**：需要 Win32 extern（`CreateThread`、`CreateMutex`、
`WaitForSingleObject`、`SRWLock`、`Interlocked*`）。

### D3 除零与溢出都是未定义行为，没有运行时检查

**缺什么**　`docs/spec.md:43`：「Integer division by zero is undefined
(native crash, **no runtime check** — speed first, matching C/C++)」，
有符号溢出同理。浮点侧：`src/codegen_c.rs:2153` 把 `Div` 原样发射成
`"/"`，而 C 里 `0.0/0.0` 是**未定义行为**（不是保证给 NaN）。

**挡住了什么**　**所有数值库**——numpy、scipy、matplotlib 的每一个
归一化、插值、积分、坐标轴缩放都要考虑除零。数组索引也是无检查的
（`docs/spec.md:60`），所以数值代码还要自己防越界。

**要动哪里**　库层解决即可（语言不改）：在 `stdlib/math.ax` 里统一定下
`fdiv` / `is_nan` / `is_inf`，别让每个库各写一遍。

**代价　零**（这是**库层**的缺口，不是语言的）。这也是 §2/§5 里
三个数值库的**最便宜前置**。

### D4 没有 `inf` / `nan` 字面量

**缺什么**　`src/lexer.rs:403` 的 `scan_number` 只接受 `数字 [. 数字]`。
`src/codegen_c.rs:537` 那个 `s.contains("inf")` 是 Rust 侧**打印** f64
常量时的分支，**不是词法支持**。又因为 v0.38.0 的 `& | ^ ~ << >>`
与 `%` 一样**是 int-only**，也**戳不进 float 的 IEEE 位**。

**绕法（都实测可写）**

| 目标 | 写法 |
|---|---|
| NaN | `extern def log(x: float) -> float` → `log(-1.0)` |
| `-inf` | `log(0.0)` |
| `+inf` | `-log(0.0)` |

**要动哪里**　`src/lexer.rs` 加两个字面量 + `src/parser.rs` 的 primary；
`selfhost/lexer.ax` 镜像。**也可以不加**，用上面的绕法。

**代价　小**（加）或**零**（用绕法）。

### D5 float 只能按 `%f` 打印 6 位小数

**缺什么**　`docs/spec.md:396`：「Floats print with `%f` (6 decimals)」，
f-string 同样（`docs/spec.md:136`）。**没有格式说明符、没有 `%g`、
没有有效数字控制**。

**挡住了什么**

- matplotlib 的刻度标签（`0.00005` 与 `5e-05` 要手写判断）
- 任何科学计算输出
- f-string 的 `{x:.2f}`（roadmap 第 5 条已列）

**要动哪里**　`src/codegen_c.rs` 里 float→string 的发射，
`selfhost/codegen.ax` 的 `j_fmt_f`（`stdlib/openai/json.ax` 已经有
一个手写的 `~%.15g`，**可以直接提升为通用实现**）。

**代价　小。**

---

## E. 语法缺口（便宜，但面广）

这些单条都很便宜，但它们是把 Aoxn 从「能写」推向「好写」的主要阻力。
roadmap 第 1–5 条已经列了，这里补上它们各自**挡了什么**：

| 缺口 | 挡住 | 代价 |
|---|---|---|
| **E1** 没有 `**`（幂，右结合） | scipy 的 `pow`、指数退避 | 极小（parser + 一处 codegen） |
| **E2** 没有 `in` / `not in` 运算符 | 成员判断、`"x" in s` | 极小 |
| **E3** 没有条件表达式 `a if c else b` | 默认值、紧凑的分支（bs4 里到处都是） | 小 |
| **E4** 没有默认参数值 | 默认配置、可选参数（flask 的 route 全部要吃这个） | 小 |
| **E5** 没有字符串索引 / `char` 类型 / 字符串迭代 | **文本处理库全部要退到整数**：`str_get(s,i)->int` | 中（要引入 `char` 类型并同步 selfhost） |
| **E6** 没有推导式（list/dict comprehension） | pandas、csv 的逐行转换 | 小 |
| **E7** 没有 set 字面量 / set 类型 | 去重、集合运算 | 中 |
| **E8** 数组索引无边界检查 | 与 D3 同源，安全靠纪律 | 语言层的检查会让每个循环变慢 |

---

## 3. 澄清：`extern` 拒绝 fn-ptr 参数 ≠ 线程做不了

`docs/spec.md:619` 和 `src/typecheck.rs:112` 都会告诉你
「`extern def` 拒绝函数指针参数」。

**但错误信息本身就给了逃生口**——它原话是：

> `extern function 'X' cannot take a function-pointer parameter;
> **pass its address as an int**`

所以 `CreateThread` 是**可以**调的：

```Aoxn
extern def CreateThread(attr: int, stack: int, start: int,
                        param: int, flags: int, id: int) -> int

def worker(p: int) -> int:
    # ...
    return 0

tid = to_int(worker)          # v0.40.0：函数的裸地址
h = CreateThread(0, 0, tid, 0, 0, 0)
```

x86-64 上函数指针与数据指针同宽、同寄存器传递，所以把 extern 参数
声明成 `int` 在 ABI 上是通的。**`threading` 因此不受语言阻塞**，
真正缺的是上层同步原语（mutex / condition / 线程池），那些也都能 extern。

> 这一条修正了我在 `docs/stdlib-todo.md` 早前版本里的说法
> （当时写的是「未验证，可能阻塞」）。**线程可行。**

---

## 4. 补语言特性的一般成本模型

Aoxn 是**自举语言**——编译器用自己写的。这让每条语言特性的成本高度可预测。
一条特性必须同时出现在：

```
src/lexer.rs  src/parser.rs  src/ast.rs  src/typecheck.rs  src/codegen_c.rs
selfhost/lexer.ax  selfhost/parser.ax  selfhost/typecheck.ax  selfhost/codegen.ax
docs/spec.md  tests/pipeline.rs
```

**代价 ≈ 上列文件里需要动的那几个。** 两个经验：

1. **纯语法**（E1–E4、E6）只碰 lexer/parser/AST + 两处镜像 → **小**
2. **发射层的改动**会连带 `selfhost/codegen.ax` —— 而 selfhost 镜像必须与
   Rust 编译器**逐字节**产出同样的 C，否则固定点测试失败（AGENTS.md）。
   凡是**产生新 C 文本**的特性，成本自动 ×2。

固定点测试是这项语言的自设护栏，也是它的税。

---

## 5. 按「解锁数量 / 改动成本」排序的建议

| 优先级 | 特性 | 成本 | 解锁 |
|---|---|---|---|
| **1** | **命名空间 / 模块限定**（B4） | 中～大 | 46 个模块的地基；不做就一直在挖坑 |
| **2** | **`sizeof` + 结构体取址**（A2） | **小** | 所有非槽位容器 → numpy / Pillow / `Vec[T]` |
| **3** | **float 格式说明符 + `%g`**（D5） | 小 | 所有数值输出（现成实现已在 json.ax 里） |
| **4** | **元组 / 多返回值**（B3） | 中 | 大量库的 API 形态；sret ABI 已存在 |
| **5** | **argv**（D1） | 小 | sys / argparse / pytest / CLI |
| **6** | **E1–E4、E6 一批语法糖** | 小 | 整个生态的可读性 |
| **7** | `dict` 换哈希/排序数组（B2） | 中 | 会话、路由表、词表的规模 |
| **8** | **`enum` / 可标记联合**（B1） | 大 | pandas、ORM —— 唯一出路 |
| **9** | 运算符重载（C5） | 中 | numpy 语法、BigInt（**但会削弱可验证性，谨慎**） |
| **10** | 闭包（C2）/ 生成器（C3）/ 类（C4） | 大 | 回调生态、fixture、多态 |

**第 1–6 项都是「小到中」的改动，且互相独立**。第 7 项是性能。
第 8–10 项每一个都是「改变语言性格」级别的设计，不该顺手做。

---

## 6. 明确不建议补的

写在这里，免得反复讨论：

| 提议 | 为什么不做 |
|---|---|
| **隐式 int/float 转换** | 会摧毁「无隐式混合」这条确定性卖点（Aoxn 的设计核心）。要混就写 `to_float` / `to_int`。 |
| **默认引用语义** | 等于改掉 Aoxn 的性格。见 A1 的代价说明。 |
| **GPU / CUDA 绑定** | 没有任何相邻的东西。pytorch 已判定不做（stdlib-todo §5 #11）。 |
| **数组边界检查** | 每个循环都要付钱，而 Aoxn 的定位是「安全靠纪律 + 编译器可验证」。C 语义一致。 |
| **隐式转换的运算符重载** | 见 C5：可以加，但触发条件必须严格限制，否则 `+` 的语义变得不可静态判定。 |

---

## 7. 相关文档

- [`stdlib-todo.md`](stdlib-todo.md) —— 46 个库的排期与可行性；本文档是它的「为什么」
- [`spec.md`](spec.md) —— 语言规范；本文档标注的 roadmap 条目对应它的 1–9 条
- [`selfhost.md`](selfhost.md) —— 自举现状；§4 的成本模型依赖于它