# CLI 使用手册

QuantNight 的 `taskctl` 用于管理 REGULAR、SUPER 和 Priority 任务。它与桌面应用共用 Rust `task-core`；不是另一个回测引擎，也不会提交平台 Alpha。

本页集中维护 **16个业务命令、调用方式和操作边界**；runtime宿主管理与任务业务命令分开，构建/登记/安全管理只见下述构建手册。构建、开发绑定和版本准入细节见 [CLI 构建与配置](../crates/taskctl/README.md)，桌面操作见[用户指南](user-guide.zh.md)。参数以当前可执行程序的 `--help` 为准；本文对应当前源码，不保证旧安装包已包含同样功能。

> 除帮助外，真实任务操作需要兼容的私有 QuantNight 后端及相应 MongoDB、脚本、账号配置。后端不包含在本仓库或桌面安装包中。示例不是模拟或数据修改授权；先替换示例路径和任务名，只执行需要且已授权的步骤。不要上传凭据、cookie、私有配置或任务回执。

## 1. 选择调用入口

### 已有后端与开发绑定：使用原 Python 入口

```bash
BACK=/absolute/path/to/quantnight
cd "$BACK"
TASKCTL=.agents/skills/manage-quantnight-frontend/scripts/taskctl.py

# 当前终端的便捷函数；不安装新命令或修改 PATH。
qn() { PYTHONPATH=. uv run python "$TASKCTL" "$@"; }

qn --help
qn generate --help
```

后文 `qn ...` 均指这个函数。Python 文件只转发到已登记的原生程序，不自行执行任务控制逻辑，不自动编译、搜索另一份 checkout 或退回旧 Python 实现。

**明确runtime登记后**，Python shim直接exec登记的独立 `runtime.executable` 作CLI client，不经GUI executable/trampoline，也不需要GUI文件、ready或窗口。GUI与CLI平级使用同一task-core Unix IPC客户端、codec、身份/错误与dispatcher，任务Runtime/TaskControl/连接slots由headless host持有。schema1未启用runtime保留旧dev GUI-trampoline或原显式standalone模式；这是启动时选择，不是enrolled错误后的fallback。帮助能显示不等于host/数据库/脚本/执行端可用。

macOS用户级 `gui/<uid>` LaunchAgent在该用户已登录会话中由socket首次需求启动host，不经MCP；这里不依赖GUI仅指QuantNight窗口/进程，不扩称未登录系统headless；关闭GUI不关闭host。runtime登记/编译/安全drain采用与loaded identity只按[构建与采用手册](../crates/taskctl/README.md#runtime-enrollment-and-safe-adoption)，不以GUI重载或磁盘新binary推断host更新。业务旧源代拒新但已接操作继续，UI-only变更不强制host重启；GUI非任务Alpha/data/PNL原独立池保留。

### 需要独立 CLI：直接调用编译产物

在本前端仓库中构建：

```bash
cargo build --locked --manifest-path crates/Cargo.toml \
  -p quantnight-taskctl --bin taskctl
# 已有完整依赖缓存且要求离线时，可加 --offline。

BACK=/absolute/path/to/quantnight
CLI="$PWD/crates/target/debug/taskctl"
"$CLI" --backend-root "$BACK" list
```

如果设置了 `CARGO_TARGET_DIR` 或目标平台参数，二进制路径以实际 Cargo 输出为准。上述构建不自动安装到后端 `bin/`，也不改变原 Python 入口的登记。

直接原生调用的配置选择器放在 **子命令之前**，二选一：

```text
taskctl --backend-root /absolute/backend COMMAND [参数]
taskctl --app-config /explicit/existing/config.toml COMMAND [参数]
```

- `--backend-root`：沿原MongoConfig配置owner；Python薄壳注入后端根。runtime登记后必须匹配原selection，不能用参数切换host/数据库。
- `--app-config`：**业务global**原AppConfig选择器，放在业务子命令前。启用runtime须匹配登记的canonical app_config selector，不是临时连接override；原loader→resolve_gui_binding→MongoConfig仍是endpoint权威。GUI任务诊断需host保留原check_task_status脚本合同，backend_root纯CLI模式不捏造GUI脚本默认值。
- `runtime-plan`后面的候选 `--app-config` 与上面的业务global不是同一解析位置；Node原部署入口对应 `--runtime-app-config`。schema2只升exe无新cfg须保留selector，改selector前先旧binding安全stop/drain再publish。准确管理/部署调用只维护在[唯一手册](../crates/taskctl/README.md#management-and-deployment-command-reference)，不要把runtime管理加进16业务命令数。
- `start --config JSON`：是**任务启动配置**，不是 TOML 文件路径，也不是上述全局选择器。
- 独立 CLI 不依赖 GUI 常驻，但仍需要实际业务所需的后端和配置。不要把“独立 CLI”理解为纯前端演示模式。

## 2. 全部功能速查

先设置自己要管理的任务和已有模板：

```bash
NAME=my_task
TEMPLATE=my_existing_template
```

`templatefile` 是原模板目录／目录描述器可解析的裸名称，不是将任意路径作为可执行脚本上传。

| 命令 | 最小调用示例 | 功能与副作用 |
|---|---|---|
| `list` | `qn list` | 列出本地控制任务；符合条件的残留生成会沿原 core 核验收尾，**不再一概是纯读**。 |
| `verify-registration` | `qn verify-registration --name "$NAME"` | 对有研究绑定的任务核对生成与来源证据，可回写登记结果；不是启动。 |
| `archive-research` | `qn archive-research --name "$NAME"` | 经原研究 owner 保存研究统计／归档证据；不等于删除任务。 |
| `create` | `qn create --name "$NAME" --templatefile "$TEMPLATE"` | 创建任务控制记录；默认 regular/user1，不生成候选、不启动模拟。 |
| `generate` | `qn generate --name "$NAME"` | 认领本轮生成并调用原 writer，候选写入本地集合；不运行平台模拟。 |
| `sync` | `qn sync --name "$NAME"` | 原初始同步流程：复制并核验本地／远端数据及身份，确认后更新执行端标记。 |
| `start` | `qn start --name "$NAME"` | 启动或核验原消费者；**可能发起真实平台模拟**。 |
| `observe-accepted` | `qn observe-accepted --name "$NAME"` | 仅用于原已确认停止的 paused SUPER：继续观察已接受请求并保存结果，不发新模拟 POST。 |
| `watch` | `qn watch --name "$NAME" --notify-session-id "$SESSION_ID"` | 挂接原通知观察器，修改通知元数据；不启动模拟。SESSION_ID 必须是实际接收者。 |
| `status` | `qn status --name "$NAME"` | 查询控制／执行汇总，可按原生命周期条件写回任务终态；**不是 GUI 的只读“运行诊断”按钮**。 |
| `pause` | `qn pause --name "$NAME"` | 停止原本地／远端消费者并核验结果；不等于取消平台已接受的模拟。 |
| `resume` | `qn resume --name "$NAME"` | 按原回执和类型规则恢复允许重试的项目；**不会自动 start**。 |
| `priority-prune` | `qn priority-prune --name "$NAME" --sharpe "$SHARPE" --fitness "$FITNESS" --confirm "$NAME" --dry-run` | 仅 paused Priority 的阈值清理；先设置本轮阈值。去掉 `--dry-run` 会实际执行清理。 |
| `delete` | `qn delete --name "$NAME"` | 原安全归档／退出活动列表：保留任务与候选回执、正式结果；不是直接 drop 数据库。 |
| `results` | `qn results --name "$NAME" [--sort fitness] [--limit 20]` | 服务端实时聚合回测结果（默认 `source=live`，按指标排序/过滤，返回 Top-K、表达式与 checks 失败项）；亦可用 `--source canonical` 检查正式库入库状态。 |
| `collect-results` | `qn collect-results --name "$NAME"` | 显式完整导出已有任务结果，默认保存到后端 `reports/missions/<任务名>.json`。 |

GUI 列表会合并本地与 remote-only 展示；CLI `list` 按所选数据库角色读取，不做同样的合并。`list --remote` 读取远端控制记录，不代表“筛选本地登记中所有远程执行的任务”，也不对 remote-only 记录做本机生成收尾。

## 3. 参数速查

`--name` 除 `list` 外均必填。下表列其他当前参数；省略项由原 owner 处理，不因本文另设默认值。

| 命令 | 参数 |
|---|---|
| `list` | `--include-deactive`、`--remote`；均默认关闭。输出包含顶层 `concurrency`（`regular` / `super` / `total`）总账表达式，以及所有 running 任务的 `max_concurrent`。 |
| `create` | `--templatefile` 必填；`--type regular\|super\|priority`（默认 regular）、`--auth-profile`（默认 user1）、`--remote`、`--force`、`--expected-task-id`。 |
| `generate` | `--replace`、`--timeout`（默认 **900 秒**）、`--expected-task-id`。 |
| `sync` | `--expected-task-id`；`--timeout` 保留兼容解析，但原 sync 不使用它来设置新的执行时限。 |
| `start` | `--config '<JSON>'`、`--remote`／`--no-remote`、`--expected-task-id`、`--expected-launch-attempt-id`，以及下节通知参数。 |
| `status` | `--remote`／`--no-remote`、`--inspect-items`；`--watch-observation '<JSON>'` 是原 watcher 的绑定上下文，常规调用不需要手工构造。 |
| `pause` | `--expected-task-id`、`--expected-launch-attempt-id`。 |
| `resume` | `--include-timeout`、`--recover-processing`；它们不是未决回执的重投许可，详见下方边界。 |
| `watch` | `--replace-watcher` 及通知参数；挂接需要有效的通知接收者。 |
| `observe-accepted` | `--expected-task-id`。 |
| `verify-registration` / `archive-research` | `--expected-task-id`、`--expected-remote true\|false`。 |
| `priority-prune` | `--sharpe`、`--fitness`、`--confirm` 必填；阈值须有限且非负；`--dry-run` 默认关闭。 |
| `delete` | `--confirm`（可选原任务名）、`--expected-task-id`、`--force`（兼容参数，不能绕过原停止／回执保护）。 |
| `results` | `--source <live\|canonical>`（默认 live）、`--sort <fitness\|sharpe\|returns\|drawdown\|turnover\|margin>`（默认 fitness）、`--min-fitness <float>`、`--min-sharpe <float>`、`--min-margin <float>`、`--min-turnover <float>`、`--query <string>`（代码搜索或高级Mongo语法）、`--status <string>`（默认 success，可选 error 等）、`--limit`（默认20）、`--offset`（默认0）、`--output <路径>`。 |
| `collect-results` | `--output <路径>`，不传时使用上表默认导出位置。 |

### 身份、执行端与重试

- `--expected-task-id` 是原控制记录的 ObjectId 字符串；`--expected-launch-attempt-id` 是原启动轮次，不是平台 simulation ID。需要核验同一任务时使用原返回值，不凭同名猜身份。
- `create --remote` 只保存远端执行意向，不代表已经同步或已经远端运行。
- `start` 不传执行端参数时沿原记录选择；显式覆盖不是迁移已有任务、清空远端集合或覆盖结果的许可。
- `--replace` 具有替换集合语义，仅在明确需要且已获准时使用；不因为生成卡住就加它重试。
- `resume` 默认针对原允许的 transient/quota 状态。`--include-timeout`／`--recover-processing` 即使语法可解析，仍需原停止与接受回执规则允许；不能把已接受、未知或可能仍在执行的请求重置为未发送。普通 OLD 恢复由原消费者负责，不要求日常一律先 resume。
- `create --force`、`delete --force` 都不是通用“解锁”参数。没有通用的 `reconcile` 子命令，也没有 `generate --async` 或 `start --get-only` 参数。

### 通知参数（start / watch）

接收方式互斥：`--notify-session-id <主会话ID>`、`--notify-task-id <worker ID>`、`--no-notify`。`watch` 的作用就是挂观察器，不能用 `--no-notify` 代替所需接收者。

其他参数：`--notify-item-key <已登记的任务域>`、`--review-seconds <秒>`、`--watch-timeout <秒>`。`watch` 另支持显式 `--replace-watcher`，只更换原通知观察器，不重新 start 模拟。

```bash
# 使用实际存在的接收者；不要复制他人的会话ID。
qn start --name "$NAME" --notify-session-id "$SESSION_ID"

# 已经在运行，只修复或重新挂接通知：
qn watch --name "$NAME" --notify-session-id "$SESSION_ID"
```

worker 应按返回的原 producer／marker 完成现有等待租约；主会话使用精确 session ID。发送／入队、观察器存活、业务完成是不同事实。`watch-timeout` 到期只表示观察结束，不表示生成或模拟已被取消。

## 4. 常用流程

### 普通 / SUPER：创建、生成，然后按授权启动

以下是分步模板，不要整段当作自动执行脚本：

```bash
qn create --name "$NAME" --templatefile "$TEMPLATE" --type regular --auth-profile user1
# SUPER 使用 --type super 和对应已有模板。
qn generate --name "$NAME"
qn list
qn status --name "$NAME"

# 仅当允许远程执行且符合原初始同步条件时：
qn sync --name "$NAME"

# 仅在已授权启动时；沿用原保存配置，或使用本轮批准的完整JSON：
qn start --name "$NAME" --config "$CONFIG_JSON"

# 1. 实时聚合查看 Top-K 回测结果（默认按 fitness 倒序，包含各维度指标与 failedChecks）：
qn results --name "$NAME" --limit 20
# 2. 按自定义指标与门槛筛选：
qn results --name "$NAME" --sort sharpe --min-fitness 0.5 --min-sharpe 1.0
# 3. 按表达式代码关键词/正则搜索：
qn results --name "$NAME" --query "ts_decay_linear"
# 4. 查看报错失败的 item：
qn results --name "$NAME" --status error --limit 10
# 5. 原 canonical 模式（检查是否已同步至本地 alpha_results 正式库）：
qn results --name "$NAME" --source canonical --limit 20 --offset 0
```

`CONFIG_JSON` 必须由用户／任务 owner 设置；也可省略 `--config` 沿原保存配置规则执行。不要把示例当通用并发、区域或候选数量策略。Priority 由原专用来源／控制器驱动，不机械套用普通任务的 generate/sync 序列。

**生成数分钟是正常的。** 原 `generate` 默认900秒；Agent 应独立后台执行一次原命令、保存进程身份与 stdout/stderr 回执，不能用20秒等外层工具截止包住整条 create/checkpoint/generate。普通人工终端可以正常等待命令结束。后台启动成功不等于生成成功；失响应先查看原回执和身份，不盲重发。

### 生成失败与刷新

- 正常从 pending/waiting 发起生成，确认失败后回到该前态，`generation.phase=generation_failed`、原因和结束时间保留；失败不是 ready。
- GUI 标题旁的刷新图标与原自动列表刷新共用入口；本地 `qn list` 使用同一个原核心生成核验。
- 只检查相关的 `generating/writing` 记录。原锁忙不等待、不报警；活 writer 或缺证不猜失败；确认原 owner/writer 已停止后再按原集合／历史事实与同代CAS收尾。
- 不自动 generate/start/delete，不扫描所有候选，不把非空或部分结果猜成完整生成。相关进程无法检查或历史事实不明时会返回具体原因，不能承诺所有旧任务都回 pending。
- GUI“运行诊断（只读，可选）”检查回测进程／派发，与列表刷新及 CLI `status` 的职责不同。

### 暂停、按需恢复、最终归档

```bash
qn pause --name "$NAME"
qn status --name "$NAME"

# 仅在原重试规则允许且确实需要时；resume不会启动消费者：
qn resume --name "$NAME"
# 后续需要继续执行，再显式start。

# 本轮结束且原停止/生成/观察核验允许安全归档时：
qn delete --name "$NAME"
```

`delete` 保留归档候选、未决回执和正式 Alpha/PnL 数据；生成未决或停止未确认时不能用删除绕过。`archive-research` 是研究证据归档，不等同于任务退出活动列表。

## 5. 怎么判断命令是否成功

- `--help` 输出文本，退出码0；语法错误退出码2，发生在业务配置／客户端初始化之前。
- 业务调用通常 stdout 为一个 versioned JSON，诊断走 stderr。检查 `command`、`task`、**`ok`** 和对应动作回执，不能仅凭“命令结束”或进程退出码判断完成。
- 部分兼容路径可能 `ok=false` 但退出码0（例如 Priority 状态失败）；不能只检查 shell 的 `$?`。
- `status`／`list` 可能发生上述状态核验写回；`results` 不抓平台，但传 `--output` 会写文件；`collect-results` 本身就是导出。
- `start` 返回成功不等于所有模拟完成；`pause` 成功不等于平台请求取消；`generation_unknown` 或 `ok=false` 不是重新发送的依据。
- 源码、磁盘binary、host与GUI各自loaded image/ready、真实业务结果分别核；不同image的UUID不要求相等。GUI重载不是host升级，不能修改任务状态掩盖版本问题。
- 从**首次尝试写Call**起，半帧/未收到Accepted/断连都不授权重接重投、退直连或重放list/status。Accepted只是host当前内存接管，不是持久业务回执；host签发连接绑定Call ID，只保留内存inflight/JoinHandle并在收尾删除，不累计永久seen ID。无transport receipt/outcome/journal/operations结果库，重启不加载任何调用。结果未确认时核原task/generation/launch/业务回执，不能猜未执行。
- 客户端退出/GUI关闭只结束等待或订阅，不取消host已接的core future。只在写Call前允许同一selection的有界connect/Hello重接；10秒是启动预算，不覆盖generate900秒等业务。短读仍每原阶段30秒、write/CAS/rename8秒，**不是status/全命令总30秒deadline**；这不是Mongo根因根治声明。
- 过程ProgressGap只表示stage更新合并/滞后，不证明业务失败；最终结果/错误及原完整ConfirmedTaskStop独立传回，不能以丢stage或GUI时间代替停止回执。core已开始执行的错误不能标成not_sent。
- 原操作context仅经公共OperationalEnvironment有限透传必要PATH/proxy/SSH字段给本operation子进程，不复制适配层白名单、全量环境或host全局env，不含凭据/配置override；详见共享源码和唯一host手册。
- GUI只读运行诊断原总12秒（从caller计时并包含connect）、任务登记读取链2秒、UI observer15秒与TaskStore普通短读30秒/写8秒不是同一层；host不再重起12秒。machine stage identity中文为任务登记/目标匹配，不是任务授权或账号认证；查看与控制归属分开，remote-only原保护不扩，检查失败不隐藏已有详情/结果，未知不等任务停止。
- 完整collect-results由原writer导出，无新增结果/总量cap；IPC chunk有界不等于总体响应cap。相对 `--output` 在公共client按caller cwd转绝对路径，不改变host cwd。

## 6. 继续查哪里

- [构建、配置选择与开发绑定](../crates/taskctl/README.md)
- [桌面使用指南](user-guide.zh.md)
- [CLI 参数定义](../crates/task-core/src/cli_args.rs)
- [参数到请求映射](../crates/taskctl/src/invocation.rs)
- [共享task请求](../crates/task-core/src/task_request.rs) / [唯一业务dispatcher](../crates/task-core/src/task_dispatch.rs)
- [公共runtime/IPC owner](../crates/task-core/src/runtime/)：GUI与CLI共用 `RuntimeClient` / `TaskEndpoint.execute(TaskRequest, observer)`，不各复制schema或transport
- [唯一binding/runtime schema owner](../crates/runtime_registration.rs) / [共享原生image与process identity](../crates/native_identity.rs)
- [taskctl管理parser适配](../crates/taskctl/src/runtime_entry.rs)：管理命令与16项任务业务分开；准确管理调用/安装步骤只归构建手册，源码可读不等于help已验
- 兼容后端中的 `docs/task_control_lifecycle.md`：详细状态图、执行归属、原调用链和验收边界；不是本公开前端仓库附带的后端依赖。

本文的参数核对只需要执行各命令的 `--help`，不需要为了验证文档创建、生成、启动、暂停或删除真实任务。
