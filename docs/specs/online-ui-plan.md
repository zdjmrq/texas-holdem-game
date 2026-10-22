# 联机 UI 实现方案（Phase 6.2 预研）

状态：阶段 1 规划（2026-10-23，task-14）。**本阶段只写文档，不改代码**；等 Lead 通知「app 架构已冻结」后进入阶段 2 实现。
权威依据：docs/specs/ui.md §3.11/§3.12/§4/§5/§7/§8、docs/specs/client.md（poker-client API/事件）、docs/specs/protocol-v2.md、crates/poker-app 现有代码（app.rs 1229 行、session.rs 627、ui/table.rs 713、ui/components.rs 159、table_ctrl.rs 321、settings.rs 252、assets.rs 94、audio.rs 469、platform/、cli.rs 187）、crates/poker-client（src 1506 + tests 1388 行）。
写作用域（阶段 2）：crates/poker-app/src/online/**、crates/poker-app/tests/online_*.rs（含 tests/online_support/）；app.rs/lib.rs/Cargo.toml/cli.rs 的挂载改动由 Lead/app-spike 执行或对齐后执行。

---

## 1. 现有 poker-app 结构梳理（读代码，不改）

### 1.1 模块与职责

| 文件 | 行数 | 职责 | 与联机 UI 的关系 |
| --- | --- | --- | --- |
| src/app.rs | 1229 | 根 App：Screen 路由（Home/Table/Settings/Online）、键盘分发、设置持久化、提示音/任务栏闪光、截图 | **唯一挂载点**；Screen::Online 目前是占位页（app.rs:614-636） |
| src/session.rs | 627 | TableSession：poker-engine 驱动、AI 调度、结算/下一手（本地） | 联机不走它；Settlement/ShowdownEntry 类型可参考，但结算展示自绘 |
| src/table_ctrl.rs | 321 | 纯逻辑：TableCommand/快捷键映射、primary_action、RaisePanel 网格/步进/默认值、AnalysisMask | **直接复用**：联机加注面板与快捷键 |
| src/ui/table.rs | 713 | 无状态绘制：TableSnapshot → 顶栏/动作条/牌桌/座位/结算弹窗/分析面板；返回 TableUiAction | **复用 show/top_bar/action_bar**（构造 TableSnapshot）；结算弹窗另写 |
| src/ui/components.rs | 159 | 牌面/座位几何/配色等基础绘制 | 复用 |
| src/settings.rs | 252 | Settings JSON 持久化 + 夹取；PlayMode/VariantSetting/常量 | 联机配置来源（人数/筹码/盲注/AI 延迟/名字） |
| src/assets.rs | 94 | 资源定位、config_dir()/settings_path()、音乐扫描 | 联机会话持久化文件位置（建议 config_dir()/online-session.json） |
| src/audio.rs | 469 | 音乐 + 轮到你/结算提示音 | 联机 turn/settlement 提示音复用 |
| src/platform/ | 15+50+138 | Windows 托盘/单实例/托盘闪红；Linux stub | 联机提醒复用（平台事件已由 app.rs 处理） |
| src/shell.rs | 217 | 自定义标题栏/拖动/BeginResize/任务栏注意力 | 复用 |
| src/cli.rs | 187 | --screenshot/--scenario/--seed；Scenario 枚举（home/table*/settings） | 阶段 2 需加 online 场景用于截图验收（app-spike 文件） |
| src/analysis.rs | 280 | 后台线程 poker-prob 计算 + 缓存 | 联机分析面板可复用（Phase 6.3） |

PokerApp 关键字段（app.rs:36-56）：cli/settings/last_saved_settings/screen/session/analysis/current_analysis/analysis_key_value/audio/platform/font_report/screenshot/raise_panel/frame_index/last_prompt_turn/last_settled_hand/attention_until/hint/open_raise_requested。
联机新增：online: OnlineController（以及 effects 应用方法）。

### 1.2 每帧数据流（app.rs:1152-1184）

~~~text
eframe::App::ui(ui)
  ctx = ui.ctx()
  analysis.poll(); audio.tick(); handle_screenshot(); handle_platform_events(); handle_keyboard()
  tick_session()（本地 AI 推进）
  open_raise_requested / maybe_request_analysis / prompt_effects（本地提示音/提醒）
  custom_titlebar?
  match screen { Home => home_ui, Table => table_ui, Settings => settings_ui, Online => online_ui }
  handle_attention_timeout(); persist_if_changed(); request_repaint_after(16ms)
~~~

关键事实：
- egui 是同步立即模式；poker-client 是 async tokio → 必须做线程/通道桥接（§2.2）。
- 现有 UI 模式是「每帧构造只读快照 → widget 返回动作枚举 → app 应用」；联机沿用（快照来自 OnlineView，动作转 OnlineCommand）。
- 键盘处理 app.rs:916-954：text_edit_focused 时全跳过；TableCommand 只对 Screen::Table 生效，其余屏幕仅 Back/DismissOverlay 回主页。联机需要自己的按键分发（§5.4）。
- 提示音/提醒时机在 prompt_effects（本地 turn_key=(hand_id,turn_id)、结算 hand_id）；联机等价 key 为 (roomCode,handId,turnId) 与 (roomCode, hand_id) 结算 serial（ui.md §7.1）。

### 1.3 可复用资产与缺口

| 联机需要 | 现有 | 结论 |
| --- | --- | --- |
| 牌桌绘制（座位/公共牌/底池/牌面） | ui/table::show(TableSnapshot) | 复用；protocol GameState 映射成 TableSnapshot |
| 动作条（弃牌/过牌/跟注/加注/全下） | ui/table::action_bar(TableSnapshot) | 复用；LegalActions 从 protocol 映射成 engine 类型 |
| 加注面板数学/默认值/步进 | table_ctrl::RaisePanel + default_raise_amount + snap_to_grid | 复用 |
| 快捷键映射 | table_ctrl::shortcut_for | 复用（联机只在轮到我时生效） |
| 结算弹窗 | ui/table::settlement_modal（吃 engine Settlement） | **不复用**：联机是 protocol LastHandResult + ready 语义，另写 online/ui/settlement.rs（复用 components 绘制） |
| 分析面板 | ui/table::analysis_panel + analysis.rs | Phase 6.3 复用（yourCards/community/对手数来自 GameState） |
| 房间/大厅/连接面板 | 无 | 新增 online/ui/connect.rs、lobby.rs |
| 联机状态机/事件归约 | 无 | 新增 online/view.rs、controller.rs、worker.rs |

---

## 2. 文件级实现方案（新增 src/online/**）

~~~text
crates/poker-app/src/online/
├── mod.rs              # 模块文档、重导出、OnlineEffect/OnlineExit/OnlineConfig
├── controller.rs       # OnlineController：UI 侧同步接口（connect/join/act/poll/...）
├── worker.rs           # 后台线程 + tokio runtime + poker_client::Client 驱动 + 通道
├── commands.rs         # OnlineCommand（UI→worker）
├── events.rs           # WorkerEvent（worker→UI，poker-client 事件投影）
├── view.rs             # OnlineView + reduce(event)：状态机/去重/pending/文案（纯逻辑，可测）
├── mapping.rs          # protocol DTO → App 视图类型（TableSnapshot/SeatView/engine LegalActions/Card/Settlement）
├── session_store.rs    # resume 会话持久化（等价 sessionStorage；config_dir()/online-session.json）
├── demo.rs             # demo_client()：假 room/game_state，供 cli 截图场景（app-spike 需求）
# 注：本地服务（内嵌 poker-server）由 app-spike 的 src/local_server.rs 实现；online 只通过
#     OnlineConfig/set_server_url 读取 URL（见 §13）
├── text.rs             # 全部中文文案常量（ui.md §5.2/§8.9）
└── ui/
    ├── mod.rs          # online::ui::show(...) -> Option<OnlineExit>：按 stage 路由
    ├── connect.rs      # 连接页（服务器地址/昵称/连接/错误/本机服务地址）
    ├── lobby.rs        # 大厅（房间码/玩家列表/创建或加入/房主开始/离开/断开）
    ├── table.rs        # 联机牌桌（包装 ui::table + 顶部联机状态条 + 断线横幅）
    ├── settlement.rs   # 结算覆盖层（lastHandResult + 下一手 ready/waiting）
    └── status.rs       # 统一状态行/错误条/banner/tooltip 小部件
~~~

预计代码量：controller 260、worker 380、view 420、mapping 360、ui 合计 ~700、其余 ~260，共约 2,300 行；测试约 700 行。

### 2.1 与 app.rs 的挂载点（共 7 处，由 Lead/app-spike 改或冻结后按指示改）

1. PokerApp 新增字段 online: crate::online::OnlineController。
2. PokerApp::new 里初始化：OnlineClient::new(OnlineConfig { server_url: None（内嵌服务 URL 异步到达）, player_name, repaint: Some(cc.egui_ctx.clone()), session: session_store::load() })；app.rs 收到内嵌 server URL 后调 trait 默认方法 set_server_url(url)。
3. 每帧开头：let effects = self.online.poll(); self.apply_online_effects(effects);（新增 ~15 行私有方法，处理提示音/提醒/hint）。
4. match screen 的 Screen::Online 分支改为：if let Some(exit) = crate::online::ui::show(ui, &mut self.online, &self.settings) { self.handle_online_exit(exit); }。
5. handle_keyboard 在 Screen::Online 时先调 self.online.handle_key(ctx)（返回 OnlineKeyAction），返回 Exit(Home) 时切屏。
6. back_home()/on_exit() 调 self.online.shutdown()（断开 + 停 worker 线程，带 500ms join 超时）。
7. lib.rs 加 pub mod online;；Cargo.toml 加依赖（§6）。

### 2.2 线程/通道桥接（关键设计）

~~~text
[UI 每帧] online::ui::show → OnlineController 方法 → OnlineCommand → std mpsc
                                                                    ↓
[online 线程] current_thread tokio runtime + worker_main：
    select { client.next_event() => WorkerEvent, cmd_rx.recv() => 执行 client 命令 }
    WorkerEvent 送回 std mpsc；egui::Context.request_repaint()
                                                                    ↓
[UI 每帧] OnlineController::poll 排空 WorkerEvent（上限 256/帧）→ view.reduce(event)
          → 返回 Vec<OnlineEffect>（提示音/提醒/退出）
~~~

- **线程拥有 Client**：poker-client 是单所有者 API（send 与 next_event 都需要 &mut self）；worker 独占避免跨线程共享与乱序。
- worker 主循环用「select 出 Step，再在 select 外处理」规避借用冲突：

~~~rust
loop {
    let step = tokio::select! {
        event = client.next_event() => Step::Event(event),
        command = command_rx.recv() => Step::Command(command),
    };
    match step { /* client 不再被借用，可安全 send */ }
}
~~~

代价：命令到达会取消挂起的 next_event future。poker-client 的 next_event 在取消时状态一致（读 socket 无副作用；重连 sleep 留在 ReconnectPending；进行中的 establish 由下次重试接管），见 docs/specs/client.md §5；阶段 2 加「命令取消事件等待」回归测试（§7 #17）。
- worker 的 runtime：Builder::new_current_thread().enable_all() 在专用 std::thread 上 block_on；UI 线程永不阻塞。
- 测试不需要 egui：repaint 为 Option<egui::Context>，测试传 None，用 std::thread::sleep + poll 驱动。

---

## 3. OnlineController / OnlineView 接口

### 3.1 公共接口（mod.rs/controller.rs）

~~~rust
pub struct OnlineConfig {
    pub server_url: String,                  // 默认 "ws://127.0.0.1:3000"（会话文件或本地服务）
    pub player_name: String,                 // settings.player_name
    pub repaint: Option<egui::Context>,
    pub session: Option<SessionCredentials>, // 启动恢复
}

impl OnlineController {
    pub fn new(config: OnlineConfig) -> Self;
    pub fn connect(&mut self, server_url: &str);                         // 创建/复用 worker，发 Connect
    pub fn disconnect(&mut self);                                        // 用户「断开」（清 session，回连接页）
    pub fn leave_room(&mut self);                                        // 用户「离开房间」（leave_room + 清 session）
    pub fn reconnect(&mut self);                                         // 手动重连（保留 session）
    pub fn create_room(&mut self, options: RoomOptionsInput);            // 自动分配房间号（便捷项，见 §9 决策 4）
    pub fn join_room(&mut self, code: &str, options: RoomOptionsInput);  // join 即创建（ui.md §3.12）
    pub fn start_game(&mut self);
    pub fn act(&mut self, action: OnlineAction);
    pub fn ready_next(&mut self);
    pub fn poll(&mut self) -> Vec<OnlineEffect>;                         // 每帧调用
    pub fn view(&self) -> &OnlineView;
    pub fn handle_key(&mut self, ctx: &egui::Context) -> Option<OnlineKeyAction>;
    pub fn is_active(&self) -> bool;
    pub fn shutdown(&mut self);
}

pub enum OnlineAction { Fold, Check, Call, AllIn, RaiseTo(u64) }
pub enum OnlineEffect { PlayTurnPrompt, PlaySettlementPrompt, RequestAttention, Hint(String) }
pub enum OnlineExit { Home, Settings }
pub enum OnlineKeyAction { None, Consumed, Exit(OnlineExit) }
~~~

### 3.2 OnlineView（view.rs，纯数据 + 归约）

~~~rust
pub enum OnlineStage { Connect, Lobby, Table }
pub enum ConnStatus { Idle, Connecting, Connected, Reconnecting { attempt: u32, delay_ms: u64 }, Closed }

pub struct OnlineView {
    pub stage: OnlineStage,
    pub status: ConnStatus,
    pub server_url: String,
    pub local_server_url: Option<String>,
    pub hello: Option<Hello>,                     // serverId/appVersion/protocolVersion
    pub session: Option<SessionCredentials>,      // player_id/room_code/resume_token（token 不展示）
    pub room: Option<RoomInfo>,
    pub players: Vec<PublicPlayer>,
    pub is_host: bool,
    pub host_id: Option<String>,
    pub game: Option<GameState>,                  // 最新权威快照（已去重）
    pub last_state_version: i64,
    pub last_hand_id: i64,
    pub pending: Option<PendingAction>,
    pub ready: Option<ReadyStatus>,
    pub last_result: Option<LastHandResult>,
    pub settlement_open: bool,
    pub status_line: String,
    pub error_line: Option<String>,
    pub banner: Option<String>,                   // 重连/房主迁移/终止原因
    pub kicked: bool,                             // 4001 顶号
    pub protocol_version_mismatch: bool,
    pub turn_prompt_sent_for: Option<(String, i64, i64)>,   // (room,hand,turn)
    pub settlement_prompt_sent_for: Option<(String, i64)>,
    pub reconnect_deadline: Option<Instant>,
}

pub struct PendingAction { pub request_id: String, pub kind: PendingKind, pub hand_id: i64, pub turn_id: i64, pub expected_state_version: i64, pub sent_at: Instant }
pub enum PendingKind { Action, StartGame, ReadyNext, LeaveRoom, CreateOrJoin, Resume }
~~~

归约规则（view.reduce，纯函数，重点单测）：
- game_state：有旧状态且 (version <= last_state_version 或 hand_id < last_hand_id) → 丢弃（ui.md §5.3 / protocol-v2 §6.6）；否则更新，stage=Table；phase==idle 且 last_hand_result 存在 → settlement_open=true。
- action_ack：request_id 匹配 pending → 清 pending；accepted=false → error_line=message；accepted=true → status_line=行动已确认。
- game_state.version > pending.expected_state_version → 自动清 pending（服务端先广播 state 后 ack，protocol-v2 §5.5）。
- ready_status：更新 ready；all_ready → settlement_open=false。
- host_changed：更新 is_host/host_id；banner=房主迁移文案。
- error：RESUME_EXPIRED → 清 session + stage=Connect + status_line=服务端原文 + banner=请重新加入房间；ROOM_LIMIT/INVALID_REQUEST → error_line=message；PROTOCOL_MISMATCH → 终止回连接页。
- Disconnected{will_reconnect:false}+Closed → stage=Connect（清 room/players/game，保留 server_url）；will_reconnect=true → banner=连接中断，约 N 秒后恢复原座位…，按钮禁用。
- Reconnecting → banner 带 attempt；Connected{reconnecting:true} → banner=已重新连接，正在恢复原座位…；RoomJoined{resumed:true} → banner 清空、按 phase 回 Lobby/Table。
- ReconnectFailed → status_line=自动重连已停止，请手动重连；stage=Connect，保留 session 供 reconnect()。
- 4001 → kicked=true、清 session、stage=Connect、status_line=账号已在其他窗口恢复连接。
- ProtocolError → error_line=协议错误（已记录原始帧），连接保持。

---

## 4. ui.md §5（联机 UI）→ poker-client API/事件 映射

| ui.md §5 行为 | UI 触发/展示 | OnlineCommand / poker-client API | 事件/字段 | 文案（text.rs） |
| --- | --- | --- | --- | --- |
| 连接服务器（§5.2 地址/昵称） | 连接页「连接」 | Command::Connect → Client::connect(ClientConfig{url, hello: compatible_server(), auto_reconnect:true, handshake_timeout:5s, max_reconnect_attempts:None}) | Connected{reconnecting:false}、Hello | 已连接到服务器 / 无法连接… |
| 连接失败/非法地址 | 状态行 | WorkerEvent::ConnectFailed(ClientError) | InvalidUrl/WebSocket/HandshakeTimeout | 服务器地址必须以 ws:// 或 wss:// 开头 / 无法连接: … |
| 连接成功→大厅（T4） | stage Connect→Lobby | — | Connected | 已连接到服务器 |
| 重连恢复提示（T4） | 大厅顶部横幅 | poker-client 自动（有 session 时每次 establish 后自动 resume） | Connected{reconnecting:true} → RoomJoined{resumed:true} | 已重新连接，正在恢复原座位… |
| 创建/加入房间（join 即创建） | 房间号 select(1–5)+进入房间；可选自动分配 | Command::JoinRoom → Client::join_room / Command::CreateRoom → Client::create_room | room_created / room_joined(RoomInfo) | 已进入房间 N（服务端原文优先） |
| 房间码显示 | 大号数字 + 提示 | — | RoomInfo.room_code | 把房间号发给朋友加入 |
| 玩家列表（§3.12） | 行：连接状态图标 + 人/机头像 + 名字（自己高亮） | — | LobbyUpdate/RoomInfo.players（PublicPlayer） | 玩家列表 |
| 房主开始（§3.12） | 仅房主显示「开始游戏」 | Command::StartGame → Client::start_game(room_code) | pending；ack 清 pending；GameStarting | 只有房主可以开始游戏（服务端原文） |
| 非房主等待 | 「等待房主开始游戏…」 | — | — | 等待房主开始游戏… |
| 房主迁移（T6） | 顶部横幅 + 按钮切换 | — | Message(HostChanged) | 房主已离开，你现在是房主 / 房主已变更 |
| 开局（T7） | stage Lobby→Table | — | Message(GameStarting) → 随后 GameState | 游戏开始！N人 (H真人 + A AI) |
| 牌桌渲染（§3.3–§3.10 联机版） | 复用 ui::table | — | Message(GameState) → mapping → TableSnapshot | 底池 / 轮到你了 / 思考中… |
| 行动提交（§5.5） | 按钮/快捷键 | Command::Act → Client::player_action(ActionRequest{room_code,hand_id,turn_id,expected_state_version,action,amount}) | pending 期间按钮禁用；action_ack 清 pending；game_state version>expected 自动清 | 等待服务器确认… / 服务端拒绝原文 |
| 行动被拒（协议 §5.5） | 状态行/错误条 | — | ActionAck{accepted:false,message} | 房间状态已过期/这是一条旧牌局动作/…（服务端原文） |
| 结算弹窗（§3.9/§5.6） | online/ui/settlement.rs | — | GameState.last_hand_result；ReadyStatus | 摊牌 / 胜出 / 下一手 → |
| 下一手/ready（§5.6） | 「下一手 →」→「等待中 (N 人未准备)…」 | Command::ReadyNext → Client::ready_for_next(room_code, hand_id) | pending；ReadyStatus{all_ready,waiting_for,players}；ack | 下一手 → / 等待中 (N 人未准备)… / 正在开始下一手… |
| 断线重连（§5.3） | 牌桌冻结 + 横幅 + 按钮禁用 | poker-client 自动重连（terminal 码除外） | Disconnected{true} → Reconnecting{attempt,delay_ms} → Connected{true} → RoomJoined{resumed} | 连接中断，约 N 秒后恢复原座位… |
| 终端关闭码（§4 矩阵） | 回连接页并展示原因 | — | Disconnected{code:Some(1000/1008/1009/1013/4001),will_reconnect:false}+Closed | 已断开连接 / 消息发送过于频繁 / 服务器容量已满 / 账号已在其他窗口恢复连接 |
| RESUME_EXPIRED | 回连接页 + 清 session | controller 清 session_store | Message(Error{code:ResumeExpired}) | 恢复信息已失效（服务端原文）+ 请重新加入房间 |
| 手动断开（T18） | 「断开」 | Command::Disconnect → Client::disconnect；清 session | Disconnected{1000} → Closed | 已断开连接 |
| 离开房间（T19） | 「离开房间」 | Command::LeaveRoom → Client::leave_room → 清 session | Ack（requestId 匹配清 pending） | 已离开房间 |
| 回主页（T14） | 顶栏「← 返回主页」 | 若在房间先 LeaveRoom 再 Disconnect | Closed | — |
| 手动重连 | ReconnectFailed/关闭后「重连」 | Command::Reconnect → Client::reconnect() | Connected{true} → Hello → 自动 resume | 正在重新连接… |
| 本地服务地址（§3.12） | 连接页小字 | OnlineView.local_server_url（local_server.rs） | — | 本机服务会随应用自动启动 · 当前地址 ws://127.0.0.1:PORT |
| 服务器版本 | 连接页诊断小字 | Hello | server_id/app_version/protocol_version | 服务器：texas-holdem-game v1.5.8（protocol 2） |
| 提示音/提醒（§7） | 轮到我/结算 | view 产生 OnlineEffect | turn key=(room,hand,turnId)；settlement key=(room,hand_id) | — |
| 分析面板（§3.10，Phase 6.3） | 右侧面板 | 复用 analysis.rs（yourCards+community+对手数） | GameState.your_cards/community_cards/players | 胜率分析 / 手牌已隐藏，分析已遮蔽 |

### 4.1 AI 补位 / 观战语义

- **AI 补位**：服务端 start_game 时补齐 AI 座位；客户端只看到 PublicPlayer{is_human:false}。UI：AI 座位显示机器人图标 + 名字；协议不下发其手牌；摊牌时从 last_hand_result.results[].cards 显示（RevealedCard 带 value）。
- **断线中的人类**：PublicPlayer.connected=false → 座位显示「断线中」小字 + 置灰；房间列表用红色圆点；协议无剩余宽限时间字段，不显示倒计时（ui.md 原实现也没有）。
- **破产/观战**：服务端无 spectator 角色；stack<=0 的真人留在房间（断线宽限后 left=true）。UI 显示「已破产」+ 动作条禁用 + 「你已破产 — 筹码输光，等待新一局或离开」；**不引入观战概念**（ui.md 原实现未定义）。
- **GameStarting→首个 GameState 之间**：用 Lobby players 占位；收到 game_state 后以 seats 为准。

### 4.2 配置映射（Settings → RoomOptionsInput）

| Settings 字段 | RoomOptionsInput | 说明 |
| --- | --- | --- |
| players | maxPlayers | 2..=10（短牌 2..=8） |
| variant==ShortDeck | isShortDeck | |
| starting_stack | startingStack | 白名单 5000..100000 |
| small_blind/big_blind | smallBlind/bigBlind | 白名单 6 档 |
| ante（短牌） | ante；minBet=ante*2 | 短牌前注制 |
| ai_delay_ms | aiDelay | 白名单 800/1800/3200 |
| player_name | join/create 的 name | UI 先 sanitize_name；服务端 NFKC + ≤8 字符 |

只有创建房间的第一个玩家配置生效；加入已有房间时采用服务端 roomConfig 并更新本地展示（ui.md §3.12；protocol-v2 §5.2）。

---

## 5. 联机 UI 页面与交互（online/ui/**）

### 5.1 连接页（connect.rs，对应 ui.md §3.12 上半）

- 服务器地址输入（默认 ws://127.0.0.1:3000，宽约 260）、昵称输入（≤8 字符）、「连接」按钮。
- 本机服务地址小字（local_server_url，或「本机服务未接入，请手动启动服务器」）。
- 错误条（status_line/error_line）与服务器版本诊断。
- 会话恢复：session_store 有未过期 session 时显示「检测到上次的房间会话，是否恢复？」+「恢复」（connect + 自动 resume）/「忽略」（清 session）；避免 ui.md 原实现「刷新即静默 resume」带来的误连。

### 5.2 大厅（lobby.rs，对应 ui.md §3.12）

- 未进房间：房间号 select(1–5) + 「进入房间」；人数 select（标准 2/6/7/8/9/10，短牌 2~8）+ 短牌 checkbox；可选「自动分配房间」（create_room，见 §9 决策 4）。底部连接状态、「断开」。
- 已进房间：房间码大号显示 + 「把房间号发给朋友加入」；玩家列表（连接状态/人机/名字/筹码/房主标记）；房主显示「开始游戏」，否则「等待房主开始游戏…」；「离开房间」「断开」；重连/房主迁移横幅。
- 与 ui.md 差异：浏览器 select 改 egui ComboBox；原实现无独立「创建房间」，保持一致（join 即创建）。

### 5.3 联机牌桌（table.rs，对应 ui.md §3.3–§3.9 + §5）

- 顶部条：← 主页 / 房间号 / 连接状态（已连接/重连中…/已断开）/ 离开房间 / 断开。
- 主体复用 ui::table::show(TableSnapshot)：座位（is_actor=currentPlayerIdx、自己 seat 高亮、D/SB/BB、筹码/本轮下注、断线/破产标记）、公共牌、底池、牌面。
- 动作条复用 ui::table::action_bar：轮到我且 legal 非空时可用；pending 显示「等待服务器确认…」并禁用；非我回合显示等待/已弃牌/已全下文案（ui.md §8.4）。
- 加注面板复用 RaisePanel：上下限/步进来自 protocol legalActions 映射后的 engine LegalActions（wagerUnit 缺省时按 roomConfig 计算：短牌 ante、标准 smallBlind）；确认 → Command::Act(RaiseTo)。
- 断线横幅覆盖牌桌（不清空牌桌，ui.md T16）；终端关闭回连接页。
- 结算覆盖层（settlement.rs）：赢家/金额/公共牌/摊牌行（名字+牌面 chips+牌型名）；「下一手 →」→ ready_next，随后「等待中 (N 人未准备)…」/「正在开始下一手…」；新一手 game_state 关闭弹层。
- 破产：自己 stack=0 且结算后显示「你已破产 — 筹码输光，等待新一局或离开」，仍可看牌/离开。

### 5.4 键盘（复用 ui.md §4/§4.2 优先级）

- 联机页优先级：加注面板 > 结算覆盖层（Enter/Space/N 下一手）> 文本输入（不抢）> 牌桌动作键（仅轮到我且 legal）> Esc（关加注/回大厅）。
- 复用 table_ctrl::shortcut_for(egui::Key, text_input_focused)；app.rs 在 Screen::Online 时交给 controller.handle_key(ctx)，controller 按 stage 分发并返回 OnlineKeyAction 供 app.rs 决定是否继续原逻辑。
- 输入框焦点：ctx.text_edit_focused() 为真时不处理任何牌桌键（ui.md §4.3）。

### 5.5 文案常量（text.rs）

连接/大厅/牌桌/结算/错误/重连/关闭码/房主/破产/本机服务地址/服务器版本全部集中 pub const；单测锁定 ui.md §8.9 列表。服务端 error.message 原文优先展示（协议要求逐字一致）。

---

## 6. 依赖与构建（需 Lead/app-spike 改 Cargo.toml）

已由 app-spike 在 task-15 落地（2026-10-23 确认）：poker-protocol、poker-client、poker-server、tokio；dev-dependencies tokio(test-util) + tokio-tungstenite。原计划依赖清单如下（保留作记录）：

~~~toml
[dependencies]
poker-client = { workspace = true }
tokio = { workspace = true }            # rt/current_thread、sync、time、macros
# 若采用「内嵌 local server」（§9 决策 2）：
# poker-server = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt", "time", "sync"] }
tokio-tungstenite = { workspace = true }  # mock server
~~~

说明：workspace 已有 tokio/tokio-tungstenite 版本；poker-app 当前刻意未依赖 poker-client/poker-protocol（Cargo.toml 注释），加入时机由 Lead 决定（阶段 2 开始）。

~~~bash
export CARGO_TARGET_DIR=/tmp/target-online
cd texas-holdem-rs
cargo fmt -p poker-app -- --check
cargo clippy -p poker-app --all-targets --no-deps -- -D warnings
cargo test -p poker-app --offline
~~~

---

## 7. 测试方案（阶段 2，≥8 项，不依赖真实服务端）

### 7.1 纯逻辑测试（无 I/O）

| # | 测试 | 断言 |
| --- | --- | --- |
| 1 | view: room_created → 大厅 | room/room_code/is_host/players/stage=Lobby |
| 2 | view: game_state 去重 | 旧 stateVersion/旧 handId 丢弃；新状态更新 stage=Table |
| 3 | view: pending 生命周期 | act → pending；action_ack 同 requestId 清除；game_state version>expected 自动清除；拒绝显示 message |
| 4 | view: 终端码矩阵 | 1000/1008/1009/1013/4001 → 不重连、回连接页、文案正确；4001 清 session |
| 5 | view: 重连文案链 | Disconnected{true} → Reconnecting → Connected{true} → RoomJoined{resumed} 文案依次正确 |
| 6 | view: 房主迁移 / RESUME_EXPIRED | host_changed 文案；RESUME_EXPIRED 清 session 回连接页 |
| 7 | mapping: GameState→TableSnapshot | 座位/名字/筹码/公共牌/牌面/轮次/底池/当前行动者/自己手牌可见性 |
| 8 | mapping: LegalActions + RaisePanel | protocol → engine legal（含 wagerUnit 缺省回退）；RaisePanel min/max/step/默认值 |
| 9 | mapping: 结算投影 | lastHandResult → 赢家/金额/公共牌/摊牌行/准备态按钮文案 |
| 10 | mapping: 卡牌转换 | CardDto→Card::new(Suit::from_name, Rank::from_str_token)；非法 token 返回 None 不 panic |
| 11 | text 常量快照 | ui.md §8.9 联机文案全部存在且非空 |

### 7.2 集成测试（tests/online_controller.rs，mock server + controller 线程）

tokio 起 mock server（复制 poker-client tests/support 的精简版：accept_async + hello + 脚本化 room_created/room_joined/game_starting/game_state/ready_status/close）；controller 在独立线程自建 runtime，测试线程 poll+sleep 驱动：

| # | 场景 | 断言 |
| --- | --- | --- |
| 12 | connect→join(即创建)→start→收到 game_state | stage/room/players/game 正确；mock 收到的 join/start requestId 前缀正确 |
| 13 | 行动往返 | act(check) → mock 收到 player_action（hand/turn/expectedStateVersion 与快照一致）→ action_ack → pending 清空 |
| 14 | 断线重连 | mock 主动关闭非终端码 → view Reconnecting → 重连自动 resume，mock 收到 resume_session → room_joined(resumed) → 回牌桌 |
| 15 | 终止码踢号 | mock 发 4001 → kicked/清 session/stage=Connect |
| 16 | ready 流 | 结算 game_state → 下一手 → ready_for_next → ready_status(waiting) → allReady 关弹层 |
| 17 | 命令取消事件等待 | 连接后发 act 同时 mock 长时间不推事件：命令仍被处理（验证 select 取消 next_event 安全性） |

mock helpers 放 tests/online_support/mod.rs（目录名以 online_ 开头，符合写作用域）；不需要 egui 窗口。

### 7.3 演示/截图（验收）

- cli.rs（app-spike）新增 --scenario online-connect / online-lobby / online-table（mock 数据或本地 poker-server），复用 src/screenshot.rs 出图 docs/spikes/phase6-online-*.png。
- 可演示：mock/真实服务端下 首页 → 联机对局 → 连接 → 建房/加入 → game_state 牌桌 → 断线重连提示 → 4001/终端码回连接页。

---

## 8. 阶段 2 实施顺序与验收

1. 依赖 + lib.rs 模块挂载（Lead/app-spike）。
2. text.rs、commands.rs、events.rs、mapping.rs、view.rs + 纯逻辑测试 #1-#11。
3. worker.rs（线程/runtime/select/事件转发）+ controller.rs（poll/命令/pending/去重/持久化）。
4. ui/connect.rs、ui/lobby.rs、ui/status.rs；app.rs 挂载（按 Lead/app-spike 确认的改法）。
5. ui/table.rs（包装 ui::table）+ ui/settlement.rs + 键盘 + OnlineEffect。
6. 集成测试 #12-#17；cargo fmt/clippy/test；截图（cli 场景就绪后）。
7. 对照 ui.md §5/§8.9 文案清单逐项勾选；与真实 poker-server 的互通冒烟（POKER_INTEROP_URL 或内嵌）作为可选手动项。

验收：cargo fmt/clippy（--no-deps）/test -p poker-app 绿；新增联机逻辑测试 ≥8；可演示（mock 服务端）连接→建房/加入→game_state→断线重连提示→终止码回连接页；不改 app-spike 维护的文件（除非 Lead 明确指派挂载改动）。

---

## 9. 待 Lead/用户决策（阻塞阶段 2 的少数点）

| # | 决策 | 选项/建议 | 影响 |
| --- | --- | --- | --- |
| 1 | poker-app 依赖加入时机 | **已定**：app-spike task-15 加 poker-protocol/poker-client/poker-server/tokio + dev tokio(test-util)/tokio-tungstenite | 已解除 |
| 2 | 本地服务地址来源 | **已定 A**：内嵌 poker-server 由 app-spike 的 src/local_server.rs 实现（后台线程 runtime + spawn_server_with_fallback），online 仅通过 set_server_url 读取；未就绪显示「本地服务启动中…」 | 已解除 |
| 3 | 会话持久化位置 | **已定 B**：online/session_store.rs 写 app config 目录下 online-session.json，不改 Settings | 已解除 |
| 4 | 「自动分配房间」按钮 | **已定：不做**，保持 join 即创建（房码 1–5） | 已解除 |
| 5 | 联机分析面板范围 | **已定**：Phase 6.3，本轮不做 | 已解除 |
| 6 | 无限重连 vs UI 上限 | **已定**：保持 JS 无限重连 + 退避，用户可随时取消/手动重连，不做 UI 上限 | 已解除 |
| 7 | 观战语义 | **已定**：跟随原实现语义与文案，不用「观战」字样 | 已解除 |
| 8 | online 场景 CLI + 截图 | **已定**：app-spike 在 cli.rs 加 Scenario::OnlineConnect/OnlineLobby/OnlineTable；我提供 crate::online::demo::demo_client() 假数据 | 已解除 |
| 9 | Windows 双客户端冒烟清单 | **已定**：Lead 最后汇总时补进 Windows 冒烟清单 | 已解除 |

---

## 10. 风险与缓解

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | app.rs 架构冻结前仍可能改（app-spike 收尾） | 阶段 2 等 Lead 通知；挂载点集中 §2.1 的 7 处，冲突面最小 |
| 2 | async/sync 桥接线程生命周期（卡死/泄漏/关窗 join 阻塞） | Drop 只发 Shutdown 不 join；on_exit 调 shutdown 并 join 带 500ms 超时；worker 退出关闭 runtime |
| 3 | next_event 被命令取消的安全性 | poker-client 已保证状态一致（client.md §5）；加测试 #17；人类点击频率低 |
| 4 | stateVersion/pendingAction 竞态 | 服务端保证 game_state 先于 action_ack；version>expected 自动清 pending；pending 5s 超时提示可重试 |
| 5 | 复用 ui::table 与联机数据形状差异（Settlement/Analysis/别人手牌） | mapping 层屏蔽；结算自绘；分析 Phase 6.3；别人手牌只在 showdown 用 lastHandResult |
| 6 | 重连期间用户继续点按钮 | 非 Connected/pending 状态下禁用动作，只保留断开/离开 |
| 7 | 4001 与 RESUME_EXPIRED 混淆 | 4001 是 WS 关闭（顶号）；RESUME_EXPIRED 是 error 消息；两条路径都清 session 但文案不同（Lead 已定由联机 UI 实现） |
| 8 | 本地服务方案未定导致连接页返工 | 先做 provider 抽象 + 默认地址；决策后只改 local_server.rs |
| 9 | Windows 双客户端/真实服务端验收 | mock 覆盖逻辑；真实互通用 poker-server（lib 内嵌或 interop 测试）；Windows 手动项由 Lead 决定是否入清单 |
| 10 | 玩家名/中文输入与 IME | 复用现有 TextEdit（egui 0.37 IME 已验证）；名称走 settings::sanitize_name；服务端 NFKC ≤8 |
| 11 | 事件风暴导致 UI 卡顿 | poll 每帧上限 256 条，其余留到下一帧；request_repaint 合并 |

---

## 11. 与 app-spike 的文件边界（明确）

| 路径 | 归属 | 说明 |
| --- | --- | --- |
| crates/poker-app/src/online/** | **本任务（阶段 2）新增** | 全部联机 UI/逻辑/映射/测试支持 |
| crates/poker-app/tests/online_*.rs（含 tests/online_support/） | **本任务（阶段 2）新增** | mock server 驱动测试 |
| crates/poker-app/src/app.rs | app-spike/Lead | 挂载点（§2.1 的 7 处）由 Lead/app-spike 改，或冻结后按指示改 |
| crates/poker-app/src/lib.rs | app-spike/Lead | 仅加 pub mod online; |
| crates/poker-app/Cargo.toml | app-spike/Lead | 依赖（§6） |
| crates/poker-app/src/cli.rs | app-spike/Lead | online 场景（§7.3） |
| docs/specs/online-ui-plan.md | 本任务（阶段 1） | 本文档 |

阶段 1 交付即本文；阶段 2 开工信号：Lead 通知「app 架构已冻结」。收到后先发「挂载点改动清单」给 Lead/app-spike 确认，再开始写 src/online/**。

---

## 12. 阶段 1 结论

- 现有 poker-app 的「快照 → 动作枚举」模式可以直接承载联机页：牌桌/动作条/加注面板/快捷键全部复用，新增面集中在连接页、大厅、联机状态机与事件桥接。
- poker-client 的事件模型与 ui.md §5 的行为一一对应（§4 映射表），其中终端码、RESUME_EXPIRED、4001 顶号、房主迁移、ready 流、pending action 六类状态都有明确的 UI 归约规则。
- 唯一新增基础设施是「专用线程 + current_thread tokio runtime + std mpsc + egui repaint」桥接；测试可在无窗口、无真实服务端下覆盖 ≥17 项。
- 阶段 2 的开工阻塞项只有依赖挂载（§6）与 9 个待拍板项（§9）；其中本地服务方案（决策 2）影响连接页文案，建议先定为「内嵌 poker-server」（server 已提供 spawn_server/probe_server/url()，与协议/互通测试就绪度最高）。


---

## 13. app-spike 接入确认（task-15，2026-10-23）

app-spike 回复确认（与本文档的差异以此节为准）：

1. **字段类型**：app.rs 用具体类型 crate::online::OnlineClient；trait crate::online_controller::OnlineController 仍由 OnlineClient 实现（NoopOnlineController 保留作占位/测试替身）。app.rs 在 online/** 落地前继续用 Box<dyn OnlineController> + Noop，落地后切换。
2. **trait 新增默认方法**（建议 OnlineClient 实现）：fn set_server_url(&mut self, url: String)；fn server_url(&self) -> Option<String>。app 拿到内嵌 server URL 后调用 set_server_url。
3. **TableSnapshot 最终签名（task-15 冻结）**：现有字段 + tier: crate::layout::WindowTier、theme: crate::theme::Theme；SeatView 与 table::show 不变；action_bar 增加第 4 参数 focus_raise_slider: bool；settlement_modal 变化不影响（联机结算自绘）。online/ui/table.rs 构造 TableSnapshot 时 tier 用 layout::WindowTier::Normal（或按窗口高度计算），theme 用 settings.theme。
4. **主题组件**：theme::Palette 提供 page_bg/panel_bg/window_bg/felt_outer/felt_inner/table_edge/gold/theme_red/text_primary/text_secondary/seat_bg/seat_human/seat_actor/seat_folded/overlay/win/lose/draw；components::felt_color/table_edge_color/seat_color 改为接收 &Palette；card 尺寸用 scaled_card_size(scale)/scaled_mini_card_size(scale)。online ui 全部使用这些 API，不另造配色。
5. **内嵌本地服务归 app-spike**：app 的 src/local_server.rs 负责后台 runtime + spawn_server_with_fallback + URL 到达/关闭；online **不再实现 local_server.rs**，只显示 set_server_url 提供的 URL，未就绪显示「本地服务启动中…」。
6. **demo_client()**：online/demo.rs 提供 crate::online::demo::demo_client() -> OnlineClient（假 OnlineView：Lobby/Table 两种；不连服务器），供 cli 截图场景 OnlineLobby/OnlineTable 使用；OnlineConnect 场景可由真实 URL 或 demo 占位。
7. Cargo.toml 依赖已由 app-spike 加好（poker-protocol、poker-client、poker-server、tokio；dev tokio test-util + tokio-tungstenite）；默认构建保持绿。
8. 文件边界不变：我只写 src/online/** 与 tests/online_*.rs；lib.rs 的 pub mod online; 由 app-spike 在我 mod.rs 落地后添加并 ping。


---

## 14. 收尾状态（项目进入收尾模式时的实际交付）

Lead 于 2026-10-23 宣布项目进入收尾，task-14 只交付阶段 2a（纯逻辑层），阶段 2b（worker/controller/ui/demo）不再实现：

已交付（crates/poker-app/src/online/）：
- mod.rs：模块说明与重导出。
- text.rs（180 行）：ui.md §5.2/§8.9 全部联机文案常量 + 错误/关闭码/筹码格式化。
- commands.rs（59 行）：OnlineAction + OnlineCommand。
- events.rs（45 行）：WorkerEvent / CommandReceipt / PendingKind。
- mapping.rs（250 行）：CardDto/RevealedCard 转 poker_core::Card、wire LegalActions 转 engine LegalActions（wagerUnit 回退）、牌型名按变体、player_action 金额语义、room options、结算投影 SettlementView。
- view.rs（509 行）：OnlineView 状态机（stage/ConnStatus/pending）、事件归约（stateVersion/handId 去重、pending 生命周期、4001 踢号、RESUME_EXPIRED 清 session、重连横幅、ready 流、turn/settlement 提示去重）、OnlineEffect。
- tests/online_logic.rs（12 项测试）：文案/mapping/归约覆盖，使用 tests/golden/protocol/s2c-messages.jsonl 真实 payload。
- lib.rs 的 pub mod online; 由 ui-analyst 在收尾时加入（app-spike 已确认）。

未交付（阶段 2b，后续接手时按 §2/§3 实现）：
- worker.rs（专用线程 + current_thread tokio + poker-client 驱动）、controller.rs（OnlineClient 实现 crate::online_controller::OnlineController，含 set_server_url/server_url 默认方法）、session_store.rs、demo.rs（demo_client 假数据）、ui/**（连接页/大厅/联机牌桌/结算/状态条）。
- tests/online_controller.rs 的 6 项 mock-server 集成测试。
- app-spike 的 task-15 已把线上接口/依赖/本地服务准备好：OnlineClient 落地后按 §2.1 的 7 处挂载点切换即可。

验证（2026-10-23，CARGO_TARGET_DIR=/tmp/target-online）：
- cargo check -p poker-app --offline：通过。
- cargo test -p poker-app --offline：72 项通过（lib 42 + audio_decode 2 + cli_options 6 + fonts 4 + online_logic 12 + settings_persist 6）。
- cargo clippy -p poker-app --offline --all-targets --no-deps -- -D warnings：0 warning。
- rustfmt --check（src/online/*.rs + tests/online_logic.rs）：通过。
