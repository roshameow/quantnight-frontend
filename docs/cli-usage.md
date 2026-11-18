# CLI 使用手册

QuantNight 的 `taskctl` 用于管理 REGULAR、SUPER 和 Priority 任务（并内置只读数据面板查询）。它与桌面应用共用 Rust `task-core`；不是另一个回测引擎，也不会提交平台 Alpha。

本页集中维护业务任务控制、只读数据面板的调用方式和操作边界；runtime 宿主管理、构建与开发绑定见 [CLI 构建与配置](../crates/taskctl/README.md)，桌面操作见 [用户指南](user-guide.zh.md)。参数以当前可执行程序的 `--help` 为准。

> 真实任务操作需要兼容的私有 QuantNight 后端及相应 MongoDB、脚本和账号配置；使用前先替换示例路径和任务名。

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

后文 `qn ...` 均指这个函数。Python 文件仅作为薄入口转发到原生 `taskctl`：

- **统一 native `task-core` 后端**：GUI 与 CLI 平级连接到同一个 `task-core` IPC 客户端与 dispatcher；已启用常驻 runtime 时由 headless host 管理任务与连接池（不依赖 GUI 窗口打开），未启用时按原 dev / standalone 模式执行。
- **6 个核心生命周期状态**：`pending`（待生成）→ `generating`（生成中）→ `ready`（已就绪）→ `running`（运行中）→ `paused` / `completed`（含 `completed_with_errors`）→ `deactive`（已归档）。数据库里残留的旧状态（`launching`、`generation_unknown`、`syncing`、`deleting`、`generation_failed`）会按当前存活进程自动归一。

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

- `--backend-root`：指定后端根目录（沿用 `MongoConfig`）；启用常驻 runtime 后须与已登记配置一致。
- `--app-config`：放在子命令前的全局 `AppConfig` TOML 路径。
- `runtime-plan` 等管理命令使用 `--runtime-app-config`（或跟在管理子命令后的选择器），维护与部署命令见 [构建与部署手册](../crates/taskctl/README.md#management-and-deployment-command-reference)。
- `start --config JSON`：是**任务启动配置** JSON，不是全局 TOML 配置。
- 独立 CLI 不依赖 GUI 窗口打开，但仍需实际的后端仓库与数据库配置。

## 2. 全部功能速查

先设置自己要管理的任务和已有模板：

```bash
NAME=my_task
TEMPLATE=my_existing_template
```

`templatefile` 是原模板目录／目录描述器可解析的裸名称，不是将任意路径作为可执行脚本上传。

| 命令 | 最小调用示例 | 功能与副作用 |
|---|---|---|
| `list` | `qn list` | 列出控制任务（默认含并发账本）；对无存活 writer 的 `generating`/`generation_unknown` 自动收敛回 `pending`。 |
| `verify-registration` | `qn verify-registration --name "$NAME"` | 对有研究绑定的任务核对生成与来源证据，回写登记结果；不启动模拟。 |
| `archive-research` | `qn archive-research --name "$NAME"` | 显式将研究单元（`desc/...`）结果统计与短名单归档落盘；如需保留研究统计请在 `delete` 前执行。 |
| `create` | `qn create --name "$NAME" --templatefile "$TEMPLATE"` | 创建任务控制记录；默认 `regular`/`user1`，不生成候选、不启动模拟。 |
| `generate` | `qn generate --name "$NAME"` | 检查无存活 writer 后调用生成脚本写入候选并置为 `ready`；不运行平台模拟。 |
| `sync` | `qn sync --name "$NAME"` | 同步本地与远端任务数据及身份标记。 |
| `start` | `qn start --name "$NAME"` | 检查无存活消费者且数据就绪后启动消费者（或对已在运行的同任务幂等返回）；会发起平台模拟。 |
| `observe-accepted` | `qn observe-accepted --name "$NAME"` | 仅用于非运行中的 `paused` SUPER：补观察已接受请求并保存结果，不发新模拟 POST。 |
| `watch` | `qn watch --name "$NAME" --notify-session-id "$SESSION_ID"` | 挂接或更新通知观察器（不启动模拟）；`SESSION_ID` 必须是实际接收者。 |
| `status` | `qn status --name "$NAME"` | 查询任务控制与执行汇总，并在需要时自动将无存活进程的中间态收敛为真实终态或 `pending`/`paused`。 |
| `pause` | `qn pause --name "$NAME"` | 任意状态下均可调用：持锁终止本地／远端存活消费者并确认退出，收敛为 `paused`。 |
| `resume` | `qn resume --name "$NAME"` | 按规则恢复允许重试的项目为待运行状态；**不会自动 start**。 |
| `priority-prune` | `qn priority-prune --name "$NAME" --sharpe "$SHARPE" --fitness "$FITNESS" --confirm "$NAME" --dry-run` | 仅 `paused` Priority 的阈值清理；去掉 `--dry-run` 会实际执行清理。 |
| `delete` | `qn delete --name "$NAME"` | 任意状态下均可调用：先终止存活进程，将本地和远端集合重命名为 `<name>__archive_<id>` 并置为 `deactive`；不删除正式 Alpha/PnL 数据，也不阻塞于 `archive-research`。 |
| `results` | `qn results --name "$NAME" [--sort fitness] [--limit 20]` | 服务端实时聚合回测结果（默认 `source=live`，按指标排序/筛选，返回 Top-K、表达式与 checks 失败项）；亦可用 `--source canonical` 检查正式库入库状态。 |
| `collect-results` | `qn collect-results --name "$NAME"` | 显式完整导出已有任务结果，默认保存到后端 `reports/missions/<任务名>.json`。 |
| `alphas` | `qn alphas --region USA --delay 1 --include-checks --include-pnl-scores` | 只读查询 `alpha_results` 或 `alpha_submitted` 面板（支持指标过滤、代码搜索、`RA_PARENT` 子聚合、`prod` 相关性、`checks` 与 `pnl_scores` 补齐）。 |
| `alpha-pnl` | `qn alpha-pnl --id <ALPHA_ID>` | 只读查询单条 Alpha 的 PnL 时间序列与 `pnl_scores`（支持 `RA_PARENT` 子序列聚合）。 |
| `datasets` | `qn datasets --region USA --delay 1` | 只读查询数据集面板（支持 region/delay/universe/category/关键词过滤与覆盖率统计）。 |
| `datafields` | `qn datafields --dataset-id anl4 --region USA --delay 1` | 只读查询数据字段面板（支持按 dataset、type、关键词、scope 覆盖率过滤）。 |
| `submission-stats` | `qn submission-stats --recent-days 14` | 只读查询提交统计面板（全部时间／近3个月／按月及 `region×delay` 汇总、各地区 OWN SUPER 存量与距6缺口、逐日提交统计、已提交 Pyramid 分布）。 |

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
| `resume` | `--include-timeout`、`--recover-processing`。 |
| `watch` | `--replace-watcher` 及通知参数；挂接需要有效的通知接收者。 |
| `observe-accepted` | `--expected-task-id`。 |
| `verify-registration` / `archive-research` | `--expected-task-id`、`--expected-remote true\|false`。 |
| `priority-prune` | `--sharpe`、`--fitness`、`--confirm` 必填；阈值须有限且非负；`--dry-run` 默认关闭。 |
| `delete` | `--confirm`（可选原任务名）、`--expected-task-id`、`--force`（兼容参数；`delete` 在任意状态下均会先终止存活进程并归档集合为 `<name>__archive_<id>`）。 |
| `results` | `--source <live\|canonical>`（默认 live）、`--sort <fitness\|sharpe\|returns\|drawdown\|turnover\|margin>`（默认 fitness）、`--min-fitness <float>`、`--min-sharpe <float>`、`--min-margin <float>`、`--min-turnover <float>`、`--query <string>`（代码搜索或高级Mongo语法）、`--status <string>`（默认 success，可选 error 等）、`--limit`（默认20）、`--offset`（默认0）、`--output <路径>`。 |
| `collect-results` | `--output <路径>`，不传时使用上表默认导出位置。 |
| `alphas` | 无需 `--name`。`--collection`（默认 `alpha_results`）、`--id`、`--region`、`--universe`、`--delay`、`--alpha-type`、`--days-within`、`--min-sharpe`、`--min-fitness`、`--min-margin`、`--min-returns`、`--min-turnover`、`--max-turnover`、`--query`、`--exclude-query`、`--category`、`--messages-in`、`--messages-nin`、`--sort`、`--sort-order`、`--page`、`--page-size`、`--include-checks`、`--include-pnl-scores`、`--no-total`、`--output`。 |
| `alpha-pnl` | 无需 `--name`。`--id` 必填；可选 `--output`。 |
| `datasets` | 无需 `--name`（`--name` 作为数据集名称过滤）。`--id`、`--search`、`--field-id`、`--category`、`--region`、`--universe`、`--delay`、`--sort`、`--sort-order`、`--page`、`--page-size`、`--output`。 |
| `datafields` | 无需 `--name`。`--dataset-id`、`--search`、`--data-type`、`--region`、`--universe`、`--delay`、`--sort`、`--sort-order`、`--page`、`--page-size`、`--output`。 |
| `submission-stats` | 无需 `--name`。`--collection`（默认 `alpha_submitted`）、`--query`、`--region`、`--delay`、`--since`、`--window-months`（默认 3）、`--recent-days`（默认 14）、`--output`。 |

### 身份、执行端与重试

- `--expected-task-id` 是任务控制记录的 ObjectId 字符串；`--expected-launch-attempt-id` 是启动轮次 ID（不是平台 simulation ID）。需要严格核对身份时传入它们。
- `create --remote` 仅记录远端执行意向，实际同步由 `sync` 完成。
- `start` 不传执行端参数时沿用任务当前配置；传 `--remote` / `--no-remote` 时只允许在对应执行端数据就绪时启动。
- `--replace` 具有清空重建集合语义，仅在需要重新生成候选时使用。
- `resume` 默认将允许重试的错误/配额项恢复为 `pending`；`--include-timeout` / `--recover-processing` 可按参数显式扩大恢复范围，只把未确认为已接受的项恢复待跑，不会自动启动消费者（仍需再执行 `start`）。

### 通知参数（start / watch）

接收方式互斥：`--notify-session-id <主会话ID>`、`--notify-task-id <worker ID>`、`--no-notify`。`watch` 用于挂接或更换通知观察器，不能与 `--no-notify` 同用。

其他参数：`--notify-item-key <任务通知域>`、`--review-seconds <秒>`、`--watch-timeout <秒>`。`watch --replace-watcher` 只更换观察器，不重启模拟。

```bash
# 使用实际存在的接收者。
qn start --name "$NAME" --notify-session-id "$SESSION_ID"

# 任务已经在运行，只修复或重新挂接通知：
qn watch --name "$NAME" --notify-session-id "$SESSION_ID"
```

worker 按返回的 `producer` / `marker` 遵守等待租约；`watch-timeout` 到期只表示通知观察结束，不停止后台模拟或生成进程。

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

### 生成中断与状态收敛

- 正常 `generate` 在候选写入完成、集合非空后将任务置为 `ready`；若明确失败则回退到 `pending` 并记录 `generation.phase=generation_failed` 与错误信息。
- 若生成进程半途退出而任务停留在 `generating`（或旧版 `generation_unknown`）：调用 `qn status --name "$NAME"` 或 `qn list`（含 GUI 列表刷新）时，若确认已无存活 writer 进程，会通过最小 5 字段身份 CAS（`_id, name, status, isRemote, updatedAt`）自动收敛回 `pending`（`generation.phase=generation_failed`）。
- 收敛后可直接重新执行 `qn generate --name "$NAME"`，或在任意状态下直接用 `qn delete --name "$NAME"` 清理归档。

### 暂停、按需恢复、最终归档

```bash
qn pause --name "$NAME"
qn status --name "$NAME"

# 仅在需要重跑失败/超时项时；resume 不会启动消费者：
qn resume --name "$NAME"
# 后续需要继续执行，再显式 start：
qn start --name "$NAME"

# 若为 analyze-dataset 任务且需把统计回写到研究单元，先显式执行：
qn archive-research --name "$NAME"

# 本轮结束归档移出活动列表（任意状态下均可直接执行）：
qn delete --name "$NAME"
```

`delete` 会先终止该任务的存活进程（若有），再将本地和远端集合重命名为 `<name>__archive_<id>` 并将控制记录置为 `deactive`，保留正式 Alpha/PnL 数据。`delete` 不会因 `sync`/`generating`/`archive-research` 未完成而阻塞，也不会隐式触发 `archive-research`。

## 5. 怎么判断命令是否成功

- `--help` 输出文本，退出码 0；CLI 参数语法错误退出码 2。
- 业务调用在 stdout 输出一个 JSON 对象，诊断信息输出到 stderr。以 stdout JSON 中的 `ok` 字段和对应动作字段（如 `status`、`archived`、`count`）为准，不要仅凭退出码 `$?` 判断（部分兼容路径可能 `ok=false` 且退出码为 0）。
- `start` / `generate` 返回 `ok=true` 表示消费者或生成流程已正确完成本阶段启动/写入；`results` 读当前库中已有结果（支持 `live` / `canonical`），`collect-results` 显式导出全量 JSON。
- 所有修改类操作通过本地 `MissionLocks` 文件锁 + 极简身份 CAS 防并发踩踏，不依赖额外调用级重放或历史状态门禁；如命令失败，先查看返回的 `error` 与 `qn status --name "$NAME"` 收敛后的实际状态再决策。

## 6. 继续查哪里

- [构建、配置选择与开发绑定](../crates/taskctl/README.md)
- [桌面使用指南](user-guide.zh.md)
- [CLI 参数定义](../crates/task-core/src/cli_args.rs)
- [参数到请求映射](../crates/taskctl/src/invocation.rs)
- [共享 task 请求](../crates/task-core/src/task_request.rs) / [唯一业务 dispatcher](../crates/task-core/src/task_dispatch.rs)
- [公共 runtime/IPC owner](../crates/task-core/src/runtime/)
- [taskctl 管理 parser 适配](../crates/taskctl/src/runtime_entry.rs)（runtime 管理命令见构建手册）
- 兼容后端中的 `docs/task_control_lifecycle.md`：详细 6 状态图与并发设计说明。
