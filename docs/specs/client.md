# poker-client：WebSocket 客户端规格与实现说明

状态：Phase 6 前置完成（2026-10-23，task-12）。
权威来源：原仓库 js/network.js（commit e0c6c6f8，v1.5.8）、docs/specs/protocol-v2.md、tests/golden/protocol/、docs/specs/ui.md §5（联机 UI 行为与文案）。
范围：只做网络传输层（连接/握手/重连/resume/消息流/错误）。不含 UI 状态、不含牌局规则、不含协议 DTO（DTO 在 poker-protocol）。
交付：crates/poker-client/** 与本文档；未修改其他 crate，未 commit。

---

## 1. 交付物与结构

~~~text
crates/poker-client/
├── Cargo.toml                 # 依赖：poker-protocol, serde, serde_json, tokio, tokio-tungstenite, futures-util
├── src/lib.rs                 # crate 文档与重导出
├── src/client.rs              # Client：connect/send/next_event/重连/断开/便利请求
├── src/transport.rs           # 底层 WebSocket 打开、hello 读取、关闭（私有模块）
├── src/config.rs              # ClientConfig / HelloPolicy / SessionCredentials / ClientHooks
├── src/event.rs               # ClientEvent
├── src/error.rs               # ClientError
├── src/request_id.rs          # RequestKind / base36 / RequestIdGenerator
├── src/probe.rs               # 桌面端 hello 服务发现（等价 main.js probePokerServer）
└── tests/
    ├── support/mod.rs         # tokio-tungstenite mock server + 黄金语料加载器
    ├── connection.rs          # 握手、终端关闭码、退避序列、resume、手动重连、断开
    ├── streaming.rs           # s2c 黄金语料逐条解析、协议错误、未知类型、顺序
    ├── requests.rs            # 请求报文形状、requestId、帧上限、发送失败
    ├── golden_client.rs       # client-behaviour.json / hello-probe-cases.jsonl 常量与探测矩阵
    └── interop.rs             # 可选：对真实协议服务端的冒烟测试（默认 ignored）
~~~

行数：src 1,506 行、tests 1,388 行、Cargo.toml 24 行（合计 2,918 行，2026-10-23）。

---

## 2. API 清单

### 2.1 入口与生命周期

| API | 签名（简） | 行为 |
| --- | --- | --- |
| Client::connect | async fn connect(config: ClientConfig) -> Result<Client, ClientError> | TCP/WebSocket 握手 + 等待 hello + 策略校验；若已存 session 则自动发 resume_session；成功后事件队列为 Connected(false)、Hello |
| is_connected | fn is_connected(&self) -> bool | 连接打开且握手完成 |
| hello | fn hello(&self) -> Option<&Hello> | 最近一次 hello（每次新连接都会更新） |
| config | fn config(&self) -> &ClientConfig | 只读配置 |
| next_event | async fn next_event(&mut self) -> Option<ClientEvent> | 返回下一个事件；None 表示永久关闭（终端关闭/主动断开/重连放弃） |
| reconnect | async fn reconnect(&mut self) -> Result<(), ClientError> | 手动重连（等价 network.reconnectToServer）：静默关闭旧 socket、保留 session、立即重连；失败时返回错误并按退避继续重试 |
| disconnect | async fn disconnect(&mut self) | 主动断开：发 Close(1000) 后关闭，保留 session；随后事件 Disconnected{1000}、Closed |
| send | async fn send(&mut self, message: ClientMessage) -> Result<(), ClientError> | 写一条协议消息（调用顺序即写顺序）；未连接返回 NotConnected |
| new_request_id | fn new_request_id(&mut self, kind: RequestKind) -> RequestId | 生成 reference 格式的 requestId |

### 2.2 发送（请求）便利方法

| 方法 | 消息 | requestId 前缀 |
| --- | --- | --- |
| create_room(name, options) | create_room | create |
| join_room(room_code, name, options) | join_room | join |
| leave_room(room_code) | leave_room | leave |
| start_game(room_code) | start_game | start |
| player_action(ActionRequest) | player_action | action |
| ready_for_next(room_code, hand_id) | ready_for_next | ready |
| resume_session() | resume_session（用已存 session；无则 NoSession） | resume |
| get_snapshot() | get_snapshot（无 requestId，与 JS 一致） | — |

ActionRequest 字段：room_code、hand_id、turn_id、expected_state_version、action（poker_protocol::ActionType）、amount。ActionRequest::new(...).with_amount(x) 构造；amount 对 fold/check 为 0，call 用实际跟注额，allin/raise 用实际筹码（与 js/network.js sendAction 一致）。

### 2.3 事件（ClientEvent）

| 变体 | 字段 | 含义 |
| --- | --- | --- |
| Connected | reconnecting: bool | 握手成功；true = 自动重连或手动 reconnect 建立的连接 |
| Hello | Hello | 服务器 hello（每次连接一次） |
| Message | ServerMessage | 已解析的协议消息（按到达顺序，不丢弃旧 stateVersion；去重由上层 UI 做，见 §5.4） |
| Disconnected | code: Option<u16>, reason, will_reconnect | 连接断开；code=None 表示传输失败/异常关闭（浏览器语义 1006）；will_reconnect 表示已安排自动重连 |
| Reconnecting | attempt: u32, delay_ms: u64 | 已安排第 attempt 次（0 基）重试，延迟 delay_ms（含抖动） |
| ReconnectFailed | attempts, last_error | 达到重试上限，放弃 |
| ProtocolError | raw, error | 单帧解析失败（原始文本保留），连接继续 |
| Closed | — | 终止事件，之后 next_event 返回 None |

### 2.4 配置、hello 策略与确定性钩子

ClientConfig 字段（全部 pub，另有链式 builder）：

| 字段 | 默认 | 说明 |
| --- | --- | --- |
| url | （必填） | ws:// 地址；wss:// 显式拒绝（见 §10） |
| hello | HelloPolicy::default() | hello 校验策略；默认不校验（JS 行为） |
| handshake_timeout | Some(5s) | TCP+hello 超时；None 关闭（JS 无超时） |
| auto_reconnect | true | 异常断开后是否自动重连 |
| max_reconnect_attempts | None（无限） | 重试延迟上限；达到后 ReconnectFailed |
| hooks | SystemHooks | requestId 时钟与重连抖动来源 |
| max_frame_bytes | 16384 | 出站帧上限（服务端 16 KiB 限制） |
| send_timeout | Some(5s) | 单帧写超时；None 关闭 |

HelloPolicy 构造器：
- js_client()：不校验（默认，等价 network.js handleMessage 忽略 hello 内容）。
- compatible_server()：要求 protocolVersion=2 且 serverId=texas-holdem-game，appVersion 任意（兼容期）。
- desktop_probe(app_version)：等价 main.js probePokerServer（serverId、protocolVersion=2、appVersion 精确相等）。
- validate(&Hello) -> Result<(), HelloMismatch>：返回第一个不匹配字段。

确定性钩子（trait ClientHooks: Send + Sync + Debug）：
- now_millis()：requestId 里的 Date.now() 等价物。
- jitter_fraction()：0..1 抖动；重连公式 = min(10000, 650 * 2^min(attempt,4)) + floor(jitter*350)。
- SystemHooks（默认）：系统时钟 + 计数器混合伪随机。
- FixedHooks(now_millis, jitter)：测试用；FixedHooks::new(1_700_000_000_000, 0.5) 逐字复现 client-behaviour.json 的退避序列。

### 2.5 服务发现（probe）

| API | 说明 |
| --- | --- |
| probe_server(url, expected_app_version) | 默认 900ms 超时（PROBE_TIMEOUT），按 desktop_probe 策略探测；等价 main.js probePokerServer |
| probe_server_with_timeout(url, timeout, expected_app_version) | 自定义超时；超时/连接失败/hello 不匹配都返回 ClientError |

### 2.6 错误（ClientError）

| 变体 | 触发 | 是否可重试 |
| --- | --- | --- |
| InvalidUrl | 非 ws:// / wss:// | 否（UI 提示格式错误） |
| UnsupportedScheme | wss://（本 build 无 TLS） | 否 |
| WebSocket(String) | TCP/WS 握手或读失败 | 是（重连循环内） |
| HandshakeTimeout{timeout_ms} | hello 超时 | 是（重连循环内） |
| Hello(HelloMismatch) | hello 策略不匹配 | 否（服务不对） |
| NotConnected | 未连接时 send | 否 |
| SendFailed(String) | 写帧失败 | 是（随后产生 Disconnected 并进入重连） |
| MessageTooLarge{len,max} | 出站帧超过上限 | 否 |
| NoSession | resume_session 无 session | 否 |
| Protocol{raw,error} | hello 首帧无法解析 | 否（握手失败） |
| Closed | 已关闭状态的操作 | 否 |

所有错误实现 Display + std::error::Error；source() 暴露 HelloMismatch/ServerParseError。

### 2.7 重导出

poker_client 根重导出：Client、ActionRequest、ClientConfig、ClientHooks、FixedHooks、SystemHooks、HelloPolicy、HelloMismatch、SessionCredentials、ClientEvent、ClientError、RequestIdGenerator、RequestKind、probe_server、probe_server_with_timeout、PROBE_TIMEOUT、DEFAULT_HANDSHAKE_TIMEOUT、DEFAULT_SEND_TIMEOUT、PROTOCOL_VERSION，以及 poker_protocol 的 ActionType、RequestId、RoomOptionsInput、ServerMessage（调用方无需直接依赖 poker-protocol 的这两类）。

### 2.8 最小用法

~~~rust
use poker_client::{Client, ClientConfig, ClientEvent, RoomOptionsInput};

let mut client = Client::connect(ClientConfig::new("ws://127.0.0.1:3000")).await?;
client.create_room("玩家", RoomOptionsInput::default()).await?;
loop {
    match client.next_event().await {
        Some(ClientEvent::Message(message)) => handle(message),
        Some(ClientEvent::Disconnected { code, reason, will_reconnect }) => {
            eprintln!("disconnected {code:?}: {reason} (retry={will_reconnect})");
        }
        Some(ClientEvent::Closed) | None => break,
        _ => {}
    }
}
~~~

并发/UI 集成：Client 用 &mut self 串行化发送与事件读取（等价 JS 单 socket）。桌面 UI 若需要“一边等事件一边读输入”，建议由专用任务持有 Client，用 tokio mpsc 把 UI 命令送进来、把 ClientEvent 送出去；不要在多任务间共享 Client（无需 Arc/Mutex，天然无乱序）。

---

## 3. 与 js/network.js 对照

### 3.1 函数映射

| js/network.js | poker-client | 说明 |
| --- | --- | --- |
| connectToServer(url, options) | Client::connect(ClientConfig) | 含 hello 握手（JS 不等 hello） |
| reconnectToServer(url) | Client::reconnect() | JS 有 150ms 延迟，Rust 立即重连 |
| disconnect(options) | Client::disconnect() / clear_session() | JS 的 preserveSession 语义由调用方选择是否 clear_session |
| send(data) | Client::send / 便利方法 | 出站统一带 protocolVersion 2 |
| nextRequestId(prefix) | Client::new_request_id(RequestKind) / RequestIdGenerator | 格式 prefix-base36(now)-base36(seq) |
| handleMessage(msg)（去重/sessionStorage） | 本 crate 解析后以 Message 事件交给上层 | stateVersion/handId 去重属 UI 层（ui.md §5） |
| isTerminalCloseCode(code) | poker_protocol::constants::is_terminal_close_code | 同一张表 |
| （重连延迟公式） | poker_protocol::constants::reconnect_delay_ms | 同一公式，jitter 可注入 |
| createRoom/joinRoom/leaveRoom/startGame | create_room/join_room/leave_room/start_game | 少一个 createRoom UI 入口问题属 UI 层 |
| sendAction(action, amount) | player_action(ActionRequest) | amount 语义相同 |
| sendReadyForNext | ready_for_next | |
| roomOptions(options) | poker_protocol::config（DTO 层） | 本 crate 不重复实现 |
| loadNetworkSession/persistNetworkSession | SessionCredentials + 调用方持久化 | crate 不做 sessionStorage 等价物 |
| onConnected/onDisconnected/onGameState/... 回调 | ClientEvent 流 | 回调改为单一有序事件流 |
| main.js probePokerServer | probe_server / probe_server_with_timeout | 900ms、三字段精确匹配 |

### 3.2 行为差异（实现时已确认）

| # | 行为 | 原 JS | 本 crate | 说明 |
| --- | --- | --- | --- | --- |
| 1 | hello 处理 | network.js 忽略内容；桌面 probe 单独校验 | 默认只等待并上报 Hello（不校验）；可配置策略；probe 单独 API | 兼容期两种语义都能用 |
| 2 | 握手超时 | 无 | 默认 5s，可关闭 | 避免连到静默端口时永久挂起；测试可关 |
| 3 | 重连必要条件 | 有 resumeToken + roomCode + 非终端码 | 有 SessionCredentials + auto_reconnect + 非终端码 | 语义等价 |
| 4 | 重连延迟 | min(10000, 650*2^min(attempt,4)) + floor(rand*350) | 同公式，jitter 由 ClientHooks 注入 | FixedHooks(0.5) 复现黄金序列 |
| 5 | attempts 计数 | 首次关闭用 0，成功后在 room_created/room_joined/game_state 清零 | 同 | 连接成功但立刻掉线不会清零（JS 同样） |
| 6 | resume 时机 | onopen 时若有 token/room 则发 | 每次 establish 完成后若有 session 则发（含首次 connect，等价页面刷新恢复） | |
| 7 | 手动重连 | disconnect(preserveSession) + setTimeout 150ms | 静默关闭 + 立即重连；失败返回错误并安排退避 | 无跨进程 setTimeout 开销 |
| 8 | 解析失败 | console.error 后继续 | ClientEvent::ProtocolError（保留 raw）后继续 | 可观测 |
| 9 | 未知 s2c 类型 | switch default 忽略 | 忽略 | 一致（含 player_disconnected 死分支） |
| 10 | protocolVersion（s2c） | 不校验 | 不校验（DTO 保留原值） | 一致 |
| 11 | sessionStorage | sessionStorage key texas-holdem-network-session-v2 | 不实现；调用方保存 SessionCredentials | 桌面端建议 serde_json 落盘，键名沿用 |
| 12 | wss:// | 支持 | 拒绝（UnsupportedScheme） | 本 build 无 TLS；Phase 7 再补 |
| 13 | 出站帧上限 | 服务端 16 KiB 入站限制 | 客户端先拒绝 >16 KiB 出站帧 | 提前失败而不是被 1009 砸掉 |
| 14 | get_snapshot | 从不发送 | 提供 API，按协议发无 requestId 的 get_snapshot | 调试/重同步用 |
| 15 | 关闭原因文本 | 只给 code 给 UI，reason 多数不展示 | Disconnected 同时给 code 与 reason | 便于诊断与本地化 |

---

## 4. 关闭码矩阵

来源：docs/specs/protocol-v2.md §7 + tests/golden/protocol/close-events.jsonl + client-behaviour.json terminalCloseCodes。

| 关闭码 | 服务端 reason | 触发 | 本 crate 行为 |
| --- | --- | --- | --- |
| 1000 | （客户端自定） | 客户端主动关闭 | 终止：Disconnected{Some(1000)} → Closed，不重连 |
| 1008 | message rate exceeded | 每连接每 10s 消息数超限 | 终止：不重连（策略违规） |
| 1009 | （空字符串） | 帧超过 maxPayload（16 KiB） | 终止：不重连 |
| 1013 | server capacity reached | 连接数/每 IP 连接数超限（hello 之前） | 终止：不重连 |
| 4001 | session resumed elsewhere | 同一 resumeToken 在别处恢复，旧连接被踢 | 终止：不重连（顶号） |
| 1005 / 1006 / 1011 等 | 传输异常 | 异常关闭 | 可重连（若 auto_reconnect 且存有 session） |
| code=None | 传输失败/EOF | IO 错误、对端 RST | 同上（浏览器把这归为 1006） |

判定入口：poker_protocol::constants::is_terminal_close_code（{1000,1008,1009,1013,4001}）。
事件字段映射：Close 帧带 code/reason → Disconnected 原样携带；传输失败 → code=None，reason 为错误文本（如 WebSocket protocol error: Connection reset without closing handshake）。
黄金语料覆盖：close-events.jsonl 的 5 条（4 条 ws-close + 1 条握手 401，后者不进入本 crate）。

---

## 5. 消息流与重连状态机

### 5.1 事件生命周期

~~~text
connect()
  └─ Connected{reconnecting:false}
     Hello
     ┌─ Message(...)          零或多次（含 hello 之前不可能出现的消息：hello 必须首帧）
     ├─ ProtocolError{raw,..} 零或多次（连接保持）
     └─ Disconnected{code,reason,will_reconnect}
          ├─ will_reconnect=false → Closed（之后 next_event()=None）
          └─ will_reconnect=true
               Reconnecting{attempt:0,delay_ms}
                 (next_event 内部 sleep delay)
                 ├─ 成功 → Connected{reconnecting:true} → Hello → [自动 resume_session] → Message...
                 └─ 失败 → Disconnected{code:None,...} → Reconnecting{attempt+1,...} 或
                            ReconnectFailed{attempts} → Closed
~~~

### 5.2 内部状态

| 状态 | 含义 |
| --- | --- |
| Connected | socket 打开，hello 握手完成 |
| ReconnectPending{attempt, delay_ms} | 已排程下一次重连；下次 next_event 先 sleep 再尝试 |
| Closed | 终止；队列清空后 next_event 返回 None |

Client 还维护：hello（最近一次）、attempts（下一个延迟索引，见 §5.3）、ids（requestId 序号）、session（resume 凭据）、queue（待交付事件，按产生顺序）。

### 5.3 重连算法（逐字对齐 js/network.js）

~~~text
on_connection_lost(code, reason):
    terminal = is_terminal_close_code(code)
    can_resume = config.auto_reconnect && session.is_some() && !terminal
    emit Disconnected{code, reason, will_reconnect: can_resume}
    if !can_resume: state = Closed; emit Closed; return
    schedule_retry(reason)

on_reconnect_failure(error):
    emit Disconnected{code: None, reason: error, will_reconnect: true}
    schedule_retry(error)

schedule_retry(last_error):
    attempt = attempts                       // 首次关闭用 0
    if let Some(max) = config.max_reconnect_attempts && attempt >= max:
        state = Closed
        emit ReconnectFailed{attempts: attempt, last_error}
        emit Closed
        return
    delay = min(10000, 650 * 2^min(attempt,4)) + floor(jitter * 350)
    attempts = attempt + 1
    state = ReconnectPending{attempt, delay}
    emit Reconnecting{attempt, delay}

next_event() -> ReconnectPending:
    sleep(delay)
    try establish(reconnecting = true):
        ok  -> state = Connected（事件已入队 Connected/Hello/[resume]）
        err -> on_reconnect_failure(err)

收到 room_created / room_joined / game_state 时: attempts = 0   // 与 JS 相同
~~~

要点：
- 只有在“有 session 且非终端码”时才重连；否则直接 Closed（与 JS canResume 条件逐项相同）。
- 连接成功本身不清零 attempts；必须收到 room_created/room_joined/game_state（JS 在相同位置清零）。
- 重连成功后自动发送 resume_session，requestId 前缀 resume，roomCode/resumeToken 取自 SessionCredentials。
- 没有任何真实等待之外的时间假设：sleep 走 tokio 时间，测试用 tokio::time::pause 自动推进。

### 5.4 顺序与并发保证

- 事件按帧到达顺序产生；next_event 每次只交付一个事件，不丢、不重排。
- 单 Client 单所有者（&mut self）：send 与 next_event 互斥，出站帧严格按调用顺序写出。
- 本 crate 不做 stateVersion/handId 过滤：协议要求“客户端按 <= 丢弃旧 stateVersion、handId 不回退”，该逻辑在 UI 层（见 ui.md §5.3）；transport 层丢事件会掩盖服务端问题，故全部交付。
- 主动 disconnect/reconnect 会清空底层 socket，但不会清空已入队事件（调用方仍可看到断线原因）。
- 未知 s2c 类型静默忽略；解析失败产生 ProtocolError 且连接继续（与 JS try/catch 行为一致）。

### 5.5 手动重连与断开

| 操作 | 事件 | 说明 |
| --- | --- | --- |
| reconnect() | Connected{reconnecting:true} → Hello → [resume] | 旧 socket 静默关闭（不产生 Disconnected）；失败则返回 Err，同时按退避安排后续重试（等价 JS onclose 后续行为） |
| disconnect() | Disconnected{code:Some(1000), reason:"client disconnect", will_reconnect:false} → Closed | 保留 session；需要彻底退出时随后 clear_session() |
| 写失败 | send 返回 Err(SendFailed) 且入队 Disconnected/后续重连事件 | 写失败与断线同路径处理 |

---

## 6. 会话与 resume

### 6.1 归属

- 本 crate 只存/取：SessionCredentials{server_url, room_code, resume_token, player_id?}。
- 调用方负责持久化（桌面端建议 serde_json 文件，键名/字段与浏览器 sessionStorage 对应；sessionStorage 键仍为 texas-holdem-network-session-v2）。
- remember_room(&RoomInfo) 可从 room_created/room_joined 自动填充并保存；clear_session() 返回被清除的凭据。
- set_session(...) 允许“应用重启后恢复会话”：connect() 成功后立即自动 resume（等价浏览器 onopen + 页面刷新）。

### 6.2 自动 resume 触发点

每次 establish（首次 connect / 自动重连 / 手动 reconnect）在 hello 校验通过后、队列 Connected/Hello 之前执行：
1. 若 session 存在：生成 resume-... requestId，send resume_session(roomCode, resumeToken)。
2. 发送失败视为本次连接失败（进入重连/返回 Err），避免“已连上但未恢复座位”的静默状态。

### 6.3 RESUME_EXPIRED / 顶号处理建议

- RESUME_EXPIRED：服务端以 error 消息返回（code=RESUME_EXPIRED）。JS network.js 会清 session 并退出房间；本 crate 不自动清 session（避免隐藏副作用）。建议 UI：收到该 error 后 clear_session() 并回连接页（文案见 ui.md §5.2）。
- 4001 session resumed elsewhere：终端关闭码，本 crate 不重连；UI 提示“账号在别处恢复”并回连接页（不要自动重试，否则两个客户端互踢）。

---

## 7. 错误模型与 UI 文案映射

| ClientError | 建议 UI 行为（对应 ui.md §5.2） |
| --- | --- |
| InvalidUrl | 状态行“服务器地址必须以 ws:// 或 wss:// 开头”（与 JS 文案一致） |
| UnsupportedScheme | 状态行提示当前版本仅支持 ws://（新增文案） |
| WebSocket / HandshakeTimeout | 首连失败：状态行“无法连接: …/连接错误，正在检查本地服务…”，停留连接页 |
| Hello(HelloMismatch) | 状态行提示服务不是 poker v2（如“连接的服务器不是德州扑克服务”） |
| NotConnected | 静默忽略（对应 JS send 返回 false，不弹错） |
| SendFailed | 进入“等待服务器确认”后收到 Disconnected，按断线重连文案处理 |
| MessageTooLarge | 程序错误，记录日志；正常 UI 不可能触发 |
| NoSession | 程序错误（UI 在无 session 时不应调用 resume_session） |
| Protocol（hello 解析失败） | 连接页提示服务不可识别；不要重试 |
| ReconnectFailed 事件 | 回连接页 + 提示“连接中断，重试已停止，请手动重连”；UI 可提供“重连”按钮调用 reconnect() |

---

## 8. 互通测试计划（与 poker-server / 原 JS 服务端）

### 8.1 Rust client ↔ 原 JS 服务端（立即可跑）

~~~bash
# 原仓库（只读参考）里启动协议 v2 服务端
cd ../reference/texas-holdem-game && node server/local-server.js
# 本仓库
CARGO_TARGET_DIR=/tmp/target-client POKER_INTEROP_URL=ws://127.0.0.1:3000   cargo test -p poker-client --offline --test interop -- --ignored --nocapture
~~~

当前已实现两个 ignored 冒烟：
1. create_join_start_smoke_against_real_server：host 建房（room_created，roomCode=1、isHost、resumeToken 非空）→ guest 加入（room_joined）→ host start_game → 双方收到 game_starting → guest 收到 game_state（2 名真人座位、roomCode 一致）。
2. resume_smoke_against_real_server：建房 → remember_room → 手动 reconnect → 收到 resumed=true 的 room_joined 且 player_id 相同。

### 8.2 Rust client ↔ Rust poker-server（协议层已具备，待服务端可独立启动）

poker-server 已提供库入口 poker_server::spawn_server(ServerOptions) 与 ServerHandle::local_addr()/shutdown()（见其 crate 文档），但没有 bin/example。联调路径（任选其一，需 Lead/protocol-dev 决定）：
1. 由 protocol-dev 增加 example 或最小 bin（推荐：crates/poker-server/examples/serve.rs，监听 127.0.0.1:0 并打印地址），client 侧用 POKER_INTEROP_URL 指向该地址跑同一组 ignored 测试。
2. 在 poker-server 内写 dev-dependency 反方向集成测试（server 依赖 client 作为 dev-dep，不会产生循环：client 不依赖 server）。
3. 由 poker-cli 提供 serve 命令。
在这些就绪前，本 crate 的 mock server 测试覆盖协议帧形状与生命周期；真实互通留待上述入口落地。

### 8.3 原 JS 客户端 ↔ Rust 服务端（属 protocol-dev 任务，client 侧提示）

- 用原仓库 npm start 启动桌面客户端，连接 ws://127.0.0.1:3000（Rust server 兼容期必须报告 appVersion=1.5.8，见 protocol-v2.md §11）。
- 验收项按 protocol-v2.md §14 清单；poker-client 能提供的帮助：c2s 黄金语料 62 条（tests/golden/protocol/c2s-messages.jsonl）可被客户端侧原样写出（requests.rs 已验证字段不丢）。

### 8.4 互通待办清单

| # | 待办 | 阻塞点 |
| --- | --- | --- |
| 1 | Rust↔Rust 自动化互通测试 | poker-server 无可执行入口（见 8.2） |
| 2 | wss:// / TLS | tokio-tungstenite 未启用 rustls/native-tls；Phase 7 公共部署再议 |
| 3 | 真机双客户端同房（含 AI 补齐） | 需真实服务端 + 桌面 UI 完成 |
| 4 | 行动/结算/下一手 ready 全流程互通 | 当前冒烟止于首次 game_state；扩展 scenarios 需真实服务端 |
| 5 | 断线宽限（1800ms）内 resume、宽限外 token 失效 | 需真实服务端 + 计时；client 的 resume 自动发送已就绪 |
| 6 | 房主迁移 host_changed 后新 host 能 start_game | 需真实服务端 |
| 7 | 限流 1008 / 容量 1013 / 超大帧 1009 端到端 | mock 已覆盖关闭码分类；真实端到端需服务端配置 |
| 8 | Origin 拒绝 401 | tokio-tungstenite 默认不发送 Origin 头，需自定义请求构造（未实现） |
| 9 | RESUME_EXPIRED 自动清 session | 建议 UI 层处理（§6.3），是否下沉到 crate 待拍板 |
| 10 | appVersion 兼容策略（Rust↔Rust 后是否还要求 1.5.8） | lead/protocol-dev 拍板（protocol-v2.md §11） |

---

## 9. 测试与证据

### 9.1 命令

~~~bash
export CARGO_TARGET_DIR=/tmp/target-client
cd texas-holdem-rs
cargo test -p poker-client --offline
cargo clippy -p poker-client --offline --all-targets --no-deps -- -D warnings
cargo fmt -p poker-client -- --check
~~~

（--no-deps 只 lint 本 crate；-p poker-client 不会改写其他 crate 或 Cargo.lock 语义。）

### 9.2 测试清单（33 项通过 + 2 项 ignored）

| 文件 | 数量 | 覆盖 |
| --- | --- | --- |
| src/request_id.rs（单元） | 3 | base36 与 JS 一致、requestId 格式、前缀唯一 |
| tests/connection.rs | 12 | 连接/hello、策略不匹配、握手超时、5 个终端关闭码不重连、退避序列匹配黄金、重试上限、无 session 不重连、禁用自动重连、自动 resume、手动重连、断开事件、非法 URL/wss |
| tests/streaming.rs | 5 | s2c 黄金 152 条逐条解析且顺序一致、协议错误后连接继续、未知类型忽略、hello 必须首帧、error 文案保真、stateVersion 去重留给调用方 |
| tests/requests.rs | 6 | 8 种请求报文形状与字段、requestId 序列、get_snapshot 无 id、帧上限、终端关闭后发送返回 NotConnected、c2s 黄金 62 条字段不丢、session 保留/清除、id 生成器 |
| tests/golden_client.rs | 6 | terminalCloseCodes、退避常量、退避序列（jitter=0.5）、requestIdPrefixes+sessionStorageKey、hello-probe 6 例矩阵、probe 字段 |
| doc-test（lib.rs 示例） | 1 | 最小用法可编译 |
| tests/interop.rs | 2 ignored | 真实服务端建房/加入/开局/rel resume 冒烟（需 POKER_INTEROP_URL） |

### 9.3 黄金语料

| 文件 | 本 crate 用法 |
| --- | --- |
| client-behaviour.json | 终端关闭码表、退避常量/序列、requestId 前缀、sessionStorage key |
| close-events.jsonl | 4 个 ws 关闭码及 reason 传播 |
| hello-probe-cases.jsonl | 6 组桌面探测接受/拒绝矩阵 |
| s2c-messages.jsonl | 153 条服务端消息（去掉 hello 后 152 条）全部经 mock server 注入 → 类型化解析 → 顺序比较 |
| c2s-messages.jsonl | 62 条客户端消息全部解析后经 Client::send 写出 → 服务端收到字段与黄金逐字段一致（键序无关） |
| meta.json / server-constants.json | 由 poker-protocol 测试锁定；client 侧引用同源常量 |

### 9.4 确定性

- 时钟：requestId 使用 ClientHooks.now_millis（测试固定 1_700_000_000_000 → action-loyw3v28-N）。
- 抖动：FixedHooks jitter=0.5 时，退避序列与 client-behaviour.json 的 reconnectDelaysMs 完全相等（825/1475/2775/5375/10175/10175）。
- 时间：重连等待走 tokio::time::sleep，测试用 #[tokio::test(start_paused = true)] 自动推进；不依赖真实墙钟。
- 网络：全部用 tokio-tungstenite mock server（127.0.0.1:0 随机端口），不依赖真实服务端。
- 重复性：同日连续运行两次，结果一致（33 passed / 0 failed / 2 ignored；见 §9.5 原始摘要）。

### 9.5 本次结果（原始摘要）

~~~text
RUN 1: unittests 3 ok; connection 12 ok; golden_client 6 ok; interop 0 ok/2 ignored;
       requests 6 ok; streaming 5 ok; doc-tests 1 ok -> 33 passed, 0 failed
RUN 2: 同 RUN 1（逐项 passed 数一致）
interop（默认跑）: 2 ignored（需 POKER_INTEROP_URL）
clippy -D warnings: 0 warning（--no-deps）
cargo fmt --check: 通过（无 diff）
~~~

---

## 10. 已知限制

1. wss:// 不支持：本 build 的 tokio-tungstenite 未启用 TLS 特性；client 明确返回 UnsupportedScheme 而不是握手深错。Phase 7 公共部署需要 TLS 时，给 client 加 rustls feature 并在 open_ws 分支处理。
2. 无 sessionStorage 等价持久化：由调用方保存 SessionCredentials（桌面端推荐写 settings/JSON）。
3. 无“服务端 hello 版本不匹配自动降级”：策略由调用方选 HelloPolicy。
4. 无 Origin 头：tokio-tungstenite 默认握手不带 Origin；如果服务端配置了 POKER_ALLOWED_ORIGINS，本 client 会被 401 拒绝（互通待办 §8.4 #8）。
5. 无后台 actor/通道封装：Client 是单所有者 API；UI 侧需按 §2.8 的推荐模式自行包一个任务。若后续 poker-app 需要，可再加 spawn_client()（本任务未包含）。
6. 协议层去重、pendingAction 语义、UI 文案与提醒不在本 crate（见 ui.md §5）。
7. 真实服务端互通仅提供 ignored 冒烟，尚未在 CI 运行。

---

## 11. 文件行数（2026-10-23）

| 文件 | 行数 |
| --- | --- |
| src/client.rs | 607 |
| src/config.rs | 391 |
| src/error.rs | 95 |
| src/event.rs | 74 |
| src/lib.rs | 94 |
| src/probe.rs | 48 |
| src/request_id.rs | 129 |
| src/transport.rs | 68 |
| tests/support/mod.rs | 250 |
| tests/connection.rs | 379 |
| tests/streaming.rs | 208 |
| tests/requests.rs | 249 |
| tests/golden_client.rs | 137 |
| tests/interop.rs | 165 |
| Cargo.toml | 24 |
| 合计 | 2,918 |

（Cargo.lock 由 workspace 统一管理，本 crate 未新增外部依赖；Cargo.toml 未改动。）
