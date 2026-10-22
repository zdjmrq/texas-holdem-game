# Phase 5 服务端实现方案（poker-server）

> 状态：Phase 5 预研（2026-10-23），本轮不实现服务器。
> 关联文档：docs/specs/protocol-v2.md（协议权威规格）、docs/RUST_REWRITE_HANDOFF.md §3.4 / §4 Phase 5。
> 目标：用 tokio + axum 重写原 server/local-server.js + server/server-game.js 的行为，
> 与原 JS 客户端双向互通，并保留房间/限流/重连/TTL/Origin 等全部安全语义。

## 1. 范围

### 1.1 交付

1. crates/poker-server：可嵌入库（lib）+ 可选独立二进制（bin），提供 start/spawn/shutdown。
2. 协议 v2 全部 20 种消息的服务端实现（解析、校验、广播、错误）。
3. 房间生命周期：创建/加入/离开、准备、开局、AI 补位、行动、结算、下一手、房主迁移、
   断线重连/resume token、空房 TTL。
4. 安全与限制：消息大小上限、每连接限流、连接数/每 IP 连接数、Origin 白名单、
   代理信任开关、环境变量。
5. 测试：单元 + 集成（真实 WebSocket）+ 原 tests/server.test.js 场景等价 + 双向互操作脚本。

### 1.2 非目标

- 不做公网部署/多进程分片（本地桌面游戏，最多 5 房间 24 连接）。
- 不实现新的协议版本；仍为 v2。
- 不承担 AI 策略实现（由 poker-ai 提供适配器）；不实现 UI。
- 不做 TLS 终止（wss 由外部代理或后续再议）。

## 2. crate 结构与依赖方向

    crates/poker-server/
      src/lib.rs          # 对外 API：ServerOptions、spawn/start、ServerHandle
      src/options.rs      # 环境变量 + 默认值 + boundedNumber（复用 poker-protocol 常量）
      src/actor/
        server.rs         # ServerActor：连接注册表、房间注册表、session、创建/加入/恢复
        room.rs           # RoomActor：单个房间的全部可变状态与定时器
        connection.rs     # 每连接读/写任务、限流、心跳
      src/protocol_io.rs  # ClientMessage/ServerMessage <-> 文本帧，错误回复
      src/game.rs         # ServerPokerGame 适配层（包 poker-engine）
      src/ai.rs           # ServerAi trait + 串行 AI 队列
      src/limits.rs       # 限流/容量/Origin/代理 IP 纯函数（易测）
      src/room_state.rs   # 房间/玩家数据结构与状态机核心（不依赖 I/O）
      tests/              # 集成测试（tokio + tokio-tungstenite 客户端）
      benches/            # 可选：单房间 1k 消息吞吐

依赖方向（严格单向，不反向依赖 UI）：

    poker-core <- poker-engine <- poker-server
    poker-core <- poker-protocol <- poker-server
    poker-ai   <- poker-server（仅通过 trait 适配器；或用 feature 解耦）
    poker-server -> poker-app（poker-app 依赖 server 以嵌入进程）

建议把 AI/engine 适配定义成 trait（见 §6），Phase 5 早期用桩实现跑通框架，engine/AI 就绪后替换，
避免阻塞在其他 crate 的进度上。

## 3. 运行时与启动

- tokio（features = full 或 rt-multi-thread + net + time + sync + signal）多线程 runtime。
- axum（ws feature）提供 WebSocket upgrade；serde_json 编解码；tracing 日志。
- 每个连接：一个读任务（socket -> 解析 -> command）+ 一个写任务（outbound mpsc -> socket），
  保证同一连接消息顺序。
- 全局任务：
  - ServerActor 任务（单线程所有权，无锁处理房间注册表/session/连接准入）。
  - 每个房间一个 RoomActor 任务（房间内状态无需加锁）。
  - 心跳任务（每 15s）。
  - 可选 shutdown 信号（Ctrl-C / ServerHandle::shutdown）。
- 对外 API：

    pub struct ServerOptions { host, port, ai_delay, disconnect_grace, max_rooms, ... }
    impl ServerOptions { pub fn from_env() -> Self; pub fn sanitized(self) -> Self }
    pub struct ServerHandle { pub local_addr: SocketAddr, pub app_version: String }
    pub async fn start(options: ServerOptions) -> io::Result<ServerHandle>;
    impl ServerHandle { pub async fn shutdown(self); }

- 桌面端集成：poker-app 启动时先 start(port=3000)；EADDRINUSE 时用 hello 探测已有服务
  （serverId/protocolVersion/appVersion 严格匹配，超时 900ms，见 protocol-v2.md §11），
  匹配则复用，否则 start(port=0)。探测逻辑可放在 poker-server（复用 Hello::is_desktop_probe_compatible）。
- appVersion：默认 COMPAT_APP_VERSION = "1.5.8"（旧桌面端探针要求全等）；可用
  POKER_APP_VERSION 覆盖。Phase 5 末与用户确认长期策略（见 §12）。

## 4. 连接处理流水线

每个 WebSocket 连接的状态机（对应 local-server.js onConnection/handle）：

1. HTTP upgrade 阶段：
   - Origin 校验（§7.3）：不通过直接在 upgrade 前返回 401，不建立 WebSocket。
   - 容量校验（总连接数/每 IP）：JS 是升级成功后 close(1013)。为保持旧客户端“terminal 不重连”
     语义，Rust 也必须先完成 upgrade 再发 close 1013（返回 HTTP 5xx 会让客户端看到 1006 并自动重连）。
2. upgrade 成功：注册连接（IP、起始时间、消息窗口计数、alive 标志），立即发送 hello。
3. 读循环每条消息（顺序与 JS 一致）：
   a. 大小检查：> 16 KiB → close(1009)。（axum 配置 max_message_size 后由库层关闭，
      应用层保留兜底判断。）
   b. 限流：窗口过期重置；计数 +1；超限 → close(1008, "message rate exceeded")。
   c. parse_client_message(text)：失败 → ServerMessage::Error(error.to_server_error())。
   d. 分发（见 §5/§8），handler 内任何失败都转成 error 消息或 action_ack(accepted=false)，
      绝不 panic；用 catch_unwind 包住 handler 作为最后防线并记录日志。
4. 关闭：显式 leave 或 socket close → onDisconnect 语义（§5.4）。写任务退出时释放连接槽。

连接 meta：room_code、player_id、session_token、ip、rate 窗口、alive、heartbeat 时间。
写通道使用有界 mpsc（建议 256）；慢消费者（本地客户端不应发生）在队列满时关闭连接并记日志，
避免内存无限增长。

## 5. 房间状态机实现

### 5.1 数据结构

    struct Room {
        code: String,
        config: RoomConfig,
        players: Vec<Player>,          // 按 seat_id 升序
        host_id: Option<String>,
        game: Option<ServerGame>,
        state_version: i64,
        ready: HashSet<String>,
        timers: RoomTimers,            // 见 5.3
        created_at: Instant,
    }
    struct Player {
        id: String, seat_id: i64, name: String, avatar: String,
        is_human: bool, connected: bool, left: bool,
        stack: i64, hole_cards: Vec<Card>, chips_in_pot: i64, round_bet: i64,
        folded: bool, is_all_in: bool, dealt_in: bool,
        last_action: Option<PlayerActionRecord>,
        resume_token: String,
        processed: DedupeCache,        // requestId -> 已发送的 action_ack JSON，容量 64
        joined_at: Instant,
        connection: Option<ConnectionId>,
    }

房间内所有可变状态只由 RoomActor 任务访问（无 Mutex），命令通过 mpsc 排队，天然串行，
与 JS 单事件循环的语义一致。

### 5.2 命令与广播

RoomActor 输入命令（由 ServerActor 路由或连接任务直接投递）：
StartGame、PlayerAction、ReadyForNext、Leave、Resume、Snapshot、Disconnect、ConnectionClosed、
GraceExpired、NextHandDeadline、AiDeadline、EmptyRoomDeadline、Heartbeat 等。
输出：向每个玩家对应连接的 outbound mpsc 发送已序列化 JSON 字符串（广播即多次 send），
顺序与 JS 的 safeSend 循环一致（按 players 顺序）。

### 5.3 定时器

JS 用 setTimeout；Rust 用 RoomActor 内的 tokio::select! + Option<Instant> 截止时间（不是 spawn
独立任务），这样取消/重建不会产生悬挂回调：

    loop {
        tokio::select! {
            Some(command) = rx.recv() => handle(command),
            _ = sleep_until(ai_deadline), if ai_deadline.is_some() => handle_ai_turn(),
            _ = sleep_until(next_hand_deadline), if ... => start_next_hand(),
            _ = sleep_until(worker_deadline), if ... => drop_player(player_id),
            _ = sleep_until(empty_room_deadline), if ... => drop_room(),
        }
    }

每个 deadline 触发时必须复核前置条件（与 JS 定时器回调内的复核一致）：房间仍存在、object
identity 未变（用 generation/epoch 比较）、handId/turnId/current_player_idx 未变、phase 符合。

### 5.4 断线与恢复

- 连接关闭（非显式 leave）：player.connected=false、connection=None；广播 player_left；
  若有 game 广播 game_state；启动 1800ms（可配）宽限定时器；不迁移房主（等待 resume）。
  客户端在宽限内不会断开多久（本地环境通常 <1s）。动作若是当前玩家，等待宽限到期才弃牌，
  期间其 turn 保持；到期仍未回 → left=true、删 session、自动 fold/advance、迁移房主、
  stateVersion++、广播、尝试开下一手、清理空房。
- resume：token 在 sessions 表中 → 绑定新连接；若旧连接仍 OPEN 则 close(4001,
  "session resumed elsewhere")；取消宽限/TTL/host 定时器；回 room_joined(resumed=true)；
  广播 player_joined；有 game 则补 game_state；idle+lastHandResult 时尝试开下一手。
- 显式 leave：立即执行断线宽限到期的那套逻辑（fold、host 迁移、broadcast、cleanup），并回 ack。

### 5.5 stateVersion 与旧消息拒绝

与 protocol-v2.md §8.3 完全一致：join/resume/leave/disconnect/grace 到期/host 迁移/游戏
onChange 时 state_version += 1；客户端 game_state 按 stateVersion 单调过滤；
player_action 校验 roomCode/handId/turnId/expectedStateVersion。

广播去重：同一 stateVersion 的 game_state 只发送一次（JS 可能重复发送同版本，客户端会丢弃；
Rust 可选优化但不得改变单调性）。

### 5.6 房间号/TTL/容量

- 创建：1..5 中第一个空闲号；房间数上限由 ServerActor 维护。
- 空房：无 game → 所有人类断开/离开后立即删除并释放 session；有 game → 15s TTL 后删除。
- 删除时清空所有定时器、session、从注册表移除；向 ServerActor 发 RoomClosed 以更新计数。

## 6. 游戏与 AI 适配边界

poker-server 不复制 rules/engine 逻辑，通过两个适配点接入：

    trait ServerGame: Send {
        fn hand_id(&self) -> i64;
        fn turn_id(&self) -> i64;
        fn phase(&self) -> Phase;
        fn pot(&self) -> i64;
        // ... 仅暴露协议需要的快照字段
        fn legal_actions(&self, idx: usize) -> LegalActions;
        fn act(&mut self, idx: usize, action: ActionType, amount: i64) -> ActResult;
        fn start_hand(&mut self) -> bool;
        fn active_for_new_hand(&self, idx: usize) -> bool;
        fn current_player_idx(&self) -> i64;
        fn snapshot_for(&self, idx: usize) -> GameStateSnapshot;
    }

    trait ServerAi: Send + 'static {
        fn decide(&mut self, view: &AiView) -> Option<(ActionType, i64)>;
    }

- Phase 5 早期用 DeterministicBot（check/call 优先）实现 ServerAi，先把房间/协议跑通；
  poker-ai 完成后替换为 AICoreAdapter（复用其 decide 上下文构造，见 oracle/golden-ai.js 与
  ai-strategy 规格）。poker-engine 的 ServerPokerGame 等价实现完成后替换 ServerGame，
  两者都通过 trait 隔离，不阻塞框架开发。
- AI 队列：每房间一条串行队列（与 JS aiQueue+aiBusy 等价）。决策放
  tokio::task::spawn_blocking（CPU 数十毫秒，不能阻塞 Tokio worker），回到 RoomActor 后
  复核 handId/turnId/currentPlayerIdx 再 act。aiDelay 用可取消 deadline 实现，
  不要用 spawn+sleep 后直接 act（避免过期决策）。
- 行动顺序/结算/边池/退款等规则全部在 poker-engine，poker-server 只负责“按协议暴露状态”
  与“把 action 转给引擎”，保证与本地单机 AI 一致。

## 7. 限流、安全、Origin、代理

### 7.1 纯函数层（limits.rs，便于单测）

- rate_limiter: 每连接保存 window_started_at + message_count；窗口 10s 过期重置；
  每消息先 ++ 再比较，超过 maxMessagesPerWindow（默认 80）→ 关闭 1008。
  边界测试：窗口内第 80 条通过、第 81 条关闭；窗口过期后重新计数。
- capacity: 总连接数 >= maxConnections 或同 IP 连接数 >= maxConnectionsPerIp → 1013。
  IP 计数字符串相等（JS 用 remoteAddress 字符串比较，不规范化 IPv6-mapped 地址）。
- origin_allowed(origin, allowed_origins)：无 Origin / "null" / file:// → true；
  白名单精确匹配 → true；否则 URL 解析 hostname 属于 {localhost, 127.0.0.1, ::1} → true；
  其余 false。JS 的 URL 解析失败也返回 false。
- client_ip(headers, peer_addr, trust_proxy)：trust 时取 X-Forwarded-For 第一个逗号段 trim，
  空则回退 peer；不 trust 时直接用 peer。

### 7.2 消息大小

axum WebSocketUpgrade 设置 max_message_size(16 * 1024) 与 max_frame_size(16 * 1024)
（行为等价 ws maxPayload）。超限由库层关闭 1009。应用层仍保留长度兜底判断。
注意：axum/tungstenite 的 1009 关闭 reason 可能与 JS 不同（JS 捕获为空串），客户端只看 code，
测试只断言 code == 1009。

### 7.3 Origin 白名单

在 upgrade handler 中读取 Origin 头：
- 通过 → 继续 upgrade。
- 不通过 → 返回 401（不进入 WebSocket）。旧测试只断言 >=400。
- 不使用 tower-http CORS 中间件（语义不同），手写校验。
测试：tokio-tungstenite 客户端带自定义 Origin 头，断言握手失败/HTTP 状态。

### 7.4 心跳

心跳任务每 15s 对所有连接发 Ping 并检查 last_pong_at：
- 若上次 ping 后未收到 Pong（或从未收到且已超过 2 个周期）→ close/abort 连接。
- 收到客户端 Pong 时更新 last_pong_at。axum 的读循环需要显式处理 Message::Pong。
- 只影响僵尸连接，不参与业务逻辑。

### 7.5 恶意输入与故障隔离

- 所有 handler 只返回 Result，不使用 unwrap/expect 处理外部输入；最后用 catch_unwind
  包一层（记录日志 + error "服务端处理失败，请重试"）。
- 单连接错误不影响其他连接/房间；RoomActor panic 由 JoinHandle 监控，记录并关闭该房间
  （所有连接收到 close），ServerActor 不崩溃。桌面端嵌入场景下服务端进程不能拖垮 UI。
- 禁止在 handler 内做无界分配：名字先截断 256、消息先限 16 KiB、玩家数 <= 10、房间 <= 5。
- 慢消费者：outbound mpsc 有界（256），满则断开该连接（本地网络不应触发），避免内存膨胀。

## 8. 配置加载（ServerOptions）

优先级：显式 options > 环境变量 > 默认值；所有数值经 boundedNumber（poker-protocol 已实现
JS 语义，含范围 clamp 与整数化）。

| 字段 | 默认 | 范围 | 来源 |
| --- | --- | --- | --- |
| host | 127.0.0.1 | — | options / POKER_HOST |
| port | 3000 | 0..65535 | options / POKER_PORT |
| ai_delay | 450 | 0..10000 | options / — |
| disconnect_grace_ms | 1800 | 100..60000 | options / — |
| max_rooms | 3 | 1..5 | options / POKER_MAX_ROOMS |
| max_connections | 24 | 4..100 | options / POKER_MAX_CONNECTIONS |
| max_connections_per_ip | 20 | 2..30 | options / POKER_MAX_CONNECTIONS_PER_IP |
| rate_window_ms | 10000 | 1000..60000 | options / — |
| max_messages_per_window | 80 | 20..500 | options / — |
| trust_proxy | false | — | options / POKER_TRUST_PROXY == "1" |
| allowed_origins | [] | — | options / POKER_ALLOWED_ORIGINS 逗号分隔 |
| app_version | "1.5.8" | — | POKER_APP_VERSION（建议新增） |

独立二进制读 env；库调用方（poker-app）显式传 options，桌面端不需要读 env。

## 9. 可观测性

- tracing 结构化日志：启动地址/版本；房间创建/关闭；连接接受/拒绝（capacity/origin/rate）；
  消息解析失败（type/原因，不打印完整 payload）；action 拒绝（requestId/原因）；
  resume/顶号/宽限到期；AI 决策耗时；房间定时器触发。
- 计数指标（进程内 AtomicU64，测试可读）：connections_active、rooms_active、messages_total、
  parse_errors_total、actions_rejected_total、reconnects_total、ai_decisions_total。
- 隐私：不记录手牌/整个 payload；本地游戏但保持最小化日志。

## 10. 测试方案

### 10.1 单元测试（无 socket）

- room_state 状态机：用 tokio::time::pause（start_paused）驱动宽限/TTL/下一手 250ms/AI 延迟，
  断言 stateVersion 序列、广播消息、座位/AI 补位、房主迁移。测试 fixture 用假 ServerGame/AI。
- limits：限流窗口边界、容量、Origin 矩阵、X-Forwarded-For 解析、消息长度判断。
- dedupe cache：64 条淘汰、重复 requestId 重放、非法 requestId（空串/null）处理。
- protocol_io：直接复用 tests/golden/protocol/parse-errors.jsonl 的原始载荷，断言错误回复
  与语料逐字一致（golden 已在 poker-protocol 测试中覆盖 DTO 层，这里覆盖服务端封装层）。
- 错误文本一致性：s2c 语料包含全部错误文本；服务端 handler 文本常量可用测试与语料集合比对，
  防漂移。

### 10.2 集成测试（真实 WebSocket，tokio-tungstenite 客户端）

对应原 tests/server.test.js 的场景映射：

| 原 JS 测试 | Rust 集成测试 | 断言重点 |
| --- | --- | --- |
| room config 只接受 AI 速度档位 | options/echo 测试 | roomConfig.aiDelay 值域 |
| 默认每 IP 容量够十人房 | options 测试 | 默认 20 / 24 |
| 两客户端权威、私有、可恢复、跨手同步 | full_flow.rs | 手牌隐私、stateVersion、重放、resume、host 迁移、断线自动 fold |
| 房间上限与消息限流 | limits.rs | 房间满错误文本/码、1008 关闭 |
| 非法 Origin 拒绝 | origin.rs | 握手失败/401 |

Rust 额外补充：短牌 AI 补位、250ms 下一手、TTL（paused clock）、同 token 顶号 4001、
超长帧 1009、非法 JSON/缺 type/未知 type/版本不符错误逐字一致、get_snapshot、
hello 探针矩阵、slow consumer 处理。

测试辅助：TestClient（消息队列 + wait_for(predicate, timeout) + 发送时自动前置 protocolVersion:2），
可直接移植原 server.test.js 的 TestClient 设计。

### 10.3 双向互操作（硬验收）

1. 原 JS 客户端 → Rust 服务端：
   oracle/interop-rust-server.js（Node 脚本）。用 js/network.js 的真实实现：vm 环境 + DOM/
   sessionStorage shim + 以 ws 包为后端实现 WebSocket 接口（可参考 tests/ui-network.test.js 的
   FakeWebSocket，但网络层换成真 ws）。驱动完整流程：创建/加入/开始/自动行动/结算/下一手/
   断线重连/房主迁移，全部通过则 exit 0。Playwright/Electron 不参与（无头 Node 即可）。
2. Rust 客户端 → 原 JS 服务端：
   poker-server 的 interop 测试（#[ignore] 或 env REFERENCE_REPO 存在时启用）：spawn
   node server/local-server.js（reference repo 已 npm ci --omit=dev，有 ws），用 poker-protocol
   解析 ServerMessage，走同样流程。
3. 回归门禁：互操作脚本输出摘要写入 docs/specs/protocol-v2.md §14 的验收记录；每次协议相关
   改动必须重跑（Lead 在合并前检查）。

## 11. 与 JS 行为的差异处理清单

| # | 差异 | 处理 |
| --- | --- | --- |
| 1 | 1009 close reason 不同 | 只保证 code；测试断言 code |
| 2 | Origin 拒绝 401（axum） | 测试断言握手失败，HTTP 状态 401 或 >=400 |
| 3 | JS 的 setImmediate 串行 AI 队列 | RoomActor 内 await spawn_blocking，串行语义一致 |
| 4 | JS 可能重复广播同 stateVersion | Rust 广播前去重（客户端不可见差异） |
| 5 | 异常处理文本 | 兜底文本逐字一致：服务端处理失败，请重试 |
| 6 | 时间精度 | tokio 定时器毫秒级；宽限/TTL 允许 +-50ms 误差，测试用 paused clock 不受影响 |
| 7 | 中文文本 | 全部集中在 messages.rs 常量，golden 语料比对 |
| 8 | player_disconnected 死分支 | 不实现 |
| 9 | get_snapshot | 保留实现（调试/未来 Rust 客户端使用） |
| 10 | appVersion | 默认 1.5.8 兼容值，POKER_APP_VERSION 可覆盖 |

## 12. 实施顺序（建议 1-2 周，与 poker-engine / poker-ai 进度交错）

| 阶段 | 内容 | 预计 | 依赖 |
| --- | --- | --- | --- |
| 5.1 骨架 | crate/options/env/upgrade/Origin/容量/限流/hello/create/join/leave/lobby 广播 + 集成测试 | 2-3 天 | poker-protocol（已完成） |
| 5.2 房间流程 | ready/start/下一手/房主迁移/TTL/断线宽限/resume/顶号 + 假 Game/AI 桩 + server.test 场景等价 | 3 天 | 5.1 |
| 5.3 接引擎/AI | ServerGameAdapter（poker-engine）、ServerAiAdapter（poker-ai）、完整手牌/结算/短牌/AI 延迟 | 3-4 天 | poker-engine、poker-ai |
| 5.4 互操作 | JS 客户端 ↔ Rust 服务端、Rust 客户端 ↔ JS 服务端，修协议差异 | 2-3 天 | 5.3 |
| 5.5 加固 | 恶意输入、容量/限流边界、慢消费者、泄漏/长稳、性能记录 | 1-2 天 | 5.4 |

Phase 5 早期先做 axum ws 行为 spike（1 天）：ping/pong 自动响应、close code/reason 控制、
max_message_size 的 1009 行为、Origin 拒绝的 HTTP 状态，确认与本方案一致后再大规模铺开。
（本轮 poker-protocol 已把协议语义锁定，服务端 spike 只需验证 axum 库行为。）

## 13. 验收标准（DoD）

1. 原 tests/server.test.js 的全部场景在 Rust 集成测试中等价通过（错误文本/关闭码一致）。
2. 双向互操作脚本通过：旧 JS 客户端 ↔ Rust 服务端全流程；Rust 客户端 ↔ 旧 JS 服务端全流程。
3. 恶意输入矩阵：非法 JSON、缺/非字符串 type、未知 type、版本不符、错误房间/手/回合/版本、
   离格加注、越权操作、超长帧、超频、超容量、非法 Origin——不崩溃且错误码/文本一致。
4. 生命周期：空房 TTL 15s、未开局房立即释放、断线宽限 1800ms、resume token、顶号 4001、
   房主迁移、AI 补位、短牌模式、跨手同步逐项通过。
5. 嵌入桌面端：3000 被占时 hello 探测复用或随机端口自建；关窗时服务端优雅关闭。
6. cargo fmt --check / cargo clippy -D warnings / cargo test 全绿；协议语料测试仍全绿。
7. 文档更新：protocol-v2.md 若协议语义有调整必须同步重生成 golden；server-plan 记录实际差异。

## 14. 风险与待拍板事项

| # | 事项 | 影响 | 建议 |
| --- | --- | --- | --- |
| 1 | hello.appVersion 长期策略 | 旧桌面端探测/未来 Rust 桌面端 | 兼容期固定 1.5.8；Rust 客户端上线后同步改探测逻辑，需要 lead/用户拍板 |
| 2 | poker-engine / poker-ai API 未定 | 5.3 依赖 | 先用 trait 桩推进 5.1/5.2；trait 由协议需求驱动，engine/ai 适配 |
| 3 | AI 决策耗时与 aiDelay 语义 | 旧实现：setTimeout(aiDelay) 到期后才计算，总延迟 = aiDelay + 计算耗时 | 保持相同语义（AI 决策时刻在 aiDelay 之后），不要预先计算 |
| 4 | axum ws 库行为差异 | ping/pong、close reason、payload 限制 | 5.1 首日 spike 验证并记录 |
| 5 | 单进程内 AI 并发抢占 | 多个 AI 房间同时思考可能占满 blocking 线程池 | spawn_blocking 并发上限 / 每房间串行 / 控制 maxRooms=5 |
| 6 | 互操作测试依赖 Node 与 reference 仓库 | CI/新机器无法跑 | 脚本用 env 跳过；Lead 验收机必须跑一次并记录 |
| 7 | Windows 防火墙/端口提示 | 首次监听 3000 弹窗 | Phase 7 打包时处理；文档提示用户 |
| 8 | hostId=null 的 UI 表现 | 全部人类离开后大厅状态 | 旧客户端已处理（isHost false / hostId null），Rust 服务端照发即可 |
