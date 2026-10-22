# poker-app UI/交互规格（Phase 6 预研）

状态：Phase 6 预研完成（2026-10-23，task-6）。**只做静态分析，不含实现**。
权威 oracle：只读参考仓库 reference/texas-holdem-game（commit e0c6c6f8，v1.5.8）。
分析范围（全部逐行通读）：js/ui.js(3341)、index.html(490)、css/style.css(1970)、js/controls.js(134)、js/audio.js(298)、js/network.js(377)、js/debug.js(246)、main.js(152)、preload.js(13)、desktop-attention.js(93)；
辅助核对：js/game.js、js/engine.js、js/game-rules-core.js、js/cards.js、js/shortdeck.js、js/ai.js、js/probability.js、tests/ui-network.test.js、README.md、交接文档 §3.3/§6.3/§8.2、docs/spikes/egui-spike.md（egui 0.37 实测，§9 引用）。

阅读约定：
- 引用格式 ui.js:123、style.css:456、main.js:101 均为原仓库文件:行号；index.html:174 同理。
- 【未定义】表示原 JS 实现中不存在对应行为，Rust 重写时需新决策（汇总见 §9.11）。
- 原项目是 Electron（Chromium）DOM + CSS；本文描述的是**行为与视觉规格**，§9 给出 egui 落地建议（建议部分不是 oracle）。
- 本文代码块用 ~~~ 围栏（避免与 JS 模板字符串冲突）；egui 结论以 docs/spikes/egui-spike.md 的 0.37 实测为准。

---

## 0. 摘要（一页纸）

- 5 个用户可见页面：**主页（StartScreen）/ 本地牌桌 / 联机连接 / 联机大厅 / 联机牌桌**，外加 2 个覆盖层：**结算弹窗**、**调试面板**；3 个常驻折叠区：**规则说明**、**胜率分析面板**、**顶部工具栏（悬停/滚出）**。
- 状态全部由 6 个全局变量 + DOM display 隐式表达，**没有显式状态枚举**【原实现未定义】；Rust 必须显式建模（§9.1）。
- 桌面壳：无边框窗口 1280×860、最小 900×600（main.js:101-106），-webkit-app-region 拖动，单实例，内置本地 WS 服务（端口 3000，EADDRINUSE 回退随机端口）。
- 牌桌布局：**唯一坐标函数** calculateTablePositions（ui.js:1996-2040）+ 座位容器 inset 30px；座位节点复用 + 尺寸缓存，避免每帧重建（ui.js:848-971）。
- 键盘：9 组快捷键，处理优先级为 **加注面板 > 结算弹窗 > 手牌/分析开关 > 全屏 Esc > 牌桌动作键**（controls.js:53-133）。
- 音频：4 首 mp3 背景音乐（双 player 交叉淡入淡出）+ 2 个 WebAudio 合成提示音（800/1200Hz 出手；660/990Hz 两声结算）。
- 提醒：任务栏闪烁 + 托盘图标 650ms 交替闪红，按 messageKey 去重、回前台即“已读”（desktop-attention.js）。
- egui 0.37 已由 task-3 spike 验证：中文渲染（system_fonts 回退 + 可选内置子集）、IME 链路完整（App 层零代码）、无边框拖动/双击最大化/BeginResize、RequestUserAttention（=FlashWindowEx）、rodio 0.23 播 mp3 均可用；无边框/Snap/IME/托盘最终需 Windows 实测（见 spike §5）。API 差异见 §9.12。
- 单文件交付物：docs/specs/ui.md（本文件），不修改任何代码/其他文档。

---

## 1. 页面 / 状态机

### 1.1 全局状态变量（ui.js:9-55）

| 变量 | 初值 | 含义 | 源码 |
| --- | --- | --- | --- |
| game | null | 本地牌局实例（PokerGame/ShortDeckGame）；window.game 是 live getter | ui.js:9,3315 |
| gameMode | 'standard' | 'standard' / 'shortdeck'，持久化 | ui.js:10,100 |
| gameStarted | false | 本地是否已开过局（控制主页/牌桌） | ui.js:11 |
| playMode | 'local' | 'local' / 'online' | ui.js:12 |
| isRoomHost | false | 联机房主 | ui.js:13 |
| inRoom | false | 已在房间 | ui.js:14 |
| selectedStartingStack | 20000 | 起始筹码档位 | ui.js:26 |
| selectedSmallBlind / selectedBigBlind | 40 / 80 | 盲注/前注、最小下注 | ui.js:27-28 |
| selectedAiDelay | 1800 | AI 行动节奏（800/1800/3200） | ui.js:45-50 |
| humanCardsHidden | false | 自己的手牌是否翻到背面 | ui.js:29 |
| humanCardsAnimating | false | 翻牌/发牌动画锁 | ui.js:30 |
| raiseSliderVisible | false | 加注面板是否打开（**键盘分发的第一优先级**） | ui.js:2365 |
| musicPreference | 'off' | 音乐开关偏好 | ui.js:51 |
| selectedMusicVolume | 50 | 音量 0-100 | ui.js:53 |
| musicPlaybackMode | 'sequential' | 顺序/单曲/随机 | ui.js:54 |
| turnAlertsEnabled | true | 提示音总开关（出手 + 结算共用） | ui.js:52 |
| lastOnlineState | null | 上一帧联机快照（AI 动作动画检测） | ui.js:1122 |
| appliedRoomConfigFingerprint | null | 服务端配置指纹（避免热路径重建 DOM） | ui.js:41,282-284 |
| _turnAudioCtx | null | 提示音 AudioContext | ui.js:3232 |

### 1.2 状态图

~~~text
                         ┌──────────────────────────────────────────┐
                         │ StartScreen 主页                         │
                         │ 标准/短牌、本地/联机、人数、设置          │
                         └──────┬───────────────────────┬───────────┘
                   本地「开始游戏」│                       │ 选「联网游戏」+「连接」
                                ▼                       ▼
                    ┌───────────────────┐     ┌──────────────────────┐
                    │ LocalTable 本地牌桌│     │ OnlineConnect 连接页 │
                    │ 座位/公共牌/底池/  │     │ server/name/连接按钮 │
                    │ 动作按钮/分析面板  │     └──────────┬───────────┘
                    └────┬──────────────┘                │ onConnected
          摊牌 2s 后/弃牌胜 │                              ▼
                          ▼                    ┌──────────────────────┐
                    ┌───────────────────┐      │ OnlineLobby 房间大厅 │
                    │ SettlementModal   │      │ 房间码/玩家列表/创建/│
                    │ 结算弹窗（可拖动） │      │ 加入/开始/离开/断开  │
                    └────┬──────────────┘      └──────────┬───────────┘
                         │ 「下一手 →」                    │ game_starting
                         ▼                                ▼
                下一手 / busted→StartScreen        ┌──────────────────────┐
                                                   │ OnlineTable 联机牌桌 │
                                                   │ game_state 驱动      │
                                                   └──────────┬───────────┘
                                                              │ lastHandResult
                                                              ▼
                                                   OnlineSettlementModal
                                                   「下一手 →」→ ready_for_next
~~~

### 1.3 状态迁移表

| # | 从 | 触发 | 守卫/条件 | 副作用（按代码顺序） | 源码 |
| --- | --- | --- | --- | --- | --- |
| T1 | 主页 | 点「开始游戏」 | 本地模式 | startNewGame()：reset 旧 game、dispose AI worker、清概率缓存、setAttention(false)、读人数→建游戏→绑定回调→init(aiCount)→updateModeUI()→显示公共牌区/底池→隐藏 startScreen→显示 bottomPanel→startNewHand() | ui.js:2655-2695 |
| T2 | 主页 | 点「联网游戏」 | — | playMode='online'；隐藏 localOptions、显示 onlinePanel；开始页保留 | ui.js:325-342 |
| T3 | 主页 | 点「连接」 | url 非空 | 绑定 13 个网络回调 → connectToServer(url)；错误只写 #gameStatus | ui.js:346-491 |
| T4 | 连接页 | onConnected | 重连时 reconnecting=true 且有 resumeToken/roomCode | 隐藏 connectPanel、显示 roomLobby；重连则隐藏创建/加入表单、显示 roomActions；提示「已重新连接，正在恢复原座位…」/「已连接到服务器」 | ui.js:353-362 |
| T5 | 大厅 | 收到 room_created / room_joined | — | inRoom=true、isRoomHost 取 msg.isHost；applyRoomConfigFromServer；显示房间码；隐藏创建/加入；显示 roomActions；仅房主显示开始按钮，非房主显示「等待房主开始游戏…」；刷新玩家列表；状态行「✅ 房间 N」（房主时原实现再追加竖线+房主） | ui.js:393-406 |
| T6 | 大厅/连接页 | host_changed | 在房间 | 更新 isHost/开始按钮/waitingMsg/玩家列表；提示「房主已离开，你现在是房主」/「房主已变更」 | ui.js:413-420 |
| T7 | 大厅 | game_starting | 在房间 | 复位手牌/公共牌动画；applyRoomConfigFromServer；隐藏 roomLobby；显示 bottomPanel、隐藏 startScreen、显示公共牌/底池；底池清零；提示「游戏开始！N人 (H真人 + A AI)」 | ui.js:422-439 |
| T8 | 联机牌桌 | 每条 game_state | 房间码匹配 | renderOnlineGame(msg)；lastHandResult 存在则开结算弹窗并保持牌桌背景 | ui.js:563-708 |
| T9 | 本地牌桌 | 一手结束 onHandEnd | reason==='showdown' | 状态行「正在结算… 2 秒」→ 2s 后 showShowdown(result)（旧局/旧 handGeneration 则丢弃）；玩家破产则追加「你已输光所有筹码！」 | ui.js:1510-1535 |
| T10 | 本地牌桌 | 一手结束 onHandEnd | 非 showdown（弃牌胜） | 立即 showMessage/showAIMessage（同一个弹窗，标题不同）：你赢/你破产/AI 赢 | ui.js:1526-1534 |
| T11 | 结算弹窗 | 点「下一手 →」 | 本地 | 关闭弹窗、setAttention(false)、300ms 后：破产→goToStartScreen('busted')，否则 game.startNewHand() | ui.js:2575-2604 |
| T12 | 结算弹窗 | 点「下一手 →」 | 联机 | sendReadyForNext()；按钮变「正在开始下一手…」（在线真人≤1）或「等待其他玩家…」并禁用 | ui.js:2576-2589 |
| T13 | 联机结算 | ready_status | 自己已点 | allReady → 关弹窗、恢复「下一手 →」；否则按钮「等待中 (N 人未准备)…」禁用；自己没点则不动按钮 | ui.js:460-483 |
| T14 | 任意 | 顶部「主页」 | 有 game | 弹窗收起、加注面板收起；联机则先离开房间并提示「已离开房间」；game.resetGame()、gameStarted=false、goToStartScreen()；若手牌进行中先 toast「返回主页，当前对局已关闭」 | ui.js:2919-2947 |
| T15 | 任意 | 顶部「刷新」 | 联机/本地 | 联机：reconnectToServer(url)；本地：resetGame() 后 startNewGame()；toast「正在重新连接…」/「重新发牌」 | ui.js:2950-2966 |
| T16 | 联机任意 | onDisconnected | willReconnect | 全按钮禁用；动作解释「连接中断，约 N 秒后恢复原座位…」；状态行同文案；**保留牌桌** | ui.js:364-376 |
| T17 | 联机任意 | onDisconnected | 非重连 | 复位联机表现层；显示 connectPanel、隐藏 roomLobby；回主页（隐藏 bottomPanel、显示 startScreen、隐藏公共牌/底池、清座位）；inRoom=false；提示「已断开连接」 | ui.js:377-391 |
| T18 | 联机任意 | 「断开」 | — | disconnect() 后回连接页，提示「已断开连接」 | ui.js:1340-1352 |
| T19 | 房间内 | 「离开房间」 | — | leaveRoom() 回大厅创建/加入表单，底池/公共牌隐藏，清座位，提示「已离开房间」 | ui.js:1373-1391 |
| T20 | 牌桌 | 破产 | 本地玩家 stack<=0 且非 all-in | goToStartScreen('busted')：显示主页 + 「你已输光所有筹码！重新开始吧」 | ui.js:2598-2599,2625-2630 |
| T21 | 任意 | Esc（全屏时） | 浏览器全屏中 | 退出全屏 | controls.js:109-113 |
| T22 | 任意 | 点规则折叠条 | — | #rulesContent 展开/收起，按钮文案切换 | ui.js:3073-3080 |
| T23 | 牌桌 | 点分析标题 | — | 分析面板展开/收起（箭头 ▼/▲），并重算隐私遮蔽 | ui.js:3082-3089 |

### 1.4 页面可见性矩阵（DOM display 真值）

| 元素（id/class） | 主页 | 本地牌桌 | 连接页 | 大厅 | 联机牌桌 | 结算弹窗打开时 | 源码 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| #startScreen | flex | none | flex | flex | none | none | ui.js:1592-1600,2616 |
| #bottomPanel | none | flex | none | none | flex | flex（保持） | ui.js:1597,2692 |
| #communityArea | none | '' | none | none | '' | ''（牌面保留） | ui.js:3063,2687,617 |
| #potDisplay | none | '' | none | none | '' | '' | ui.js:3065,2689 |
| #aiPlayersContainer 座位 | 空 | 有 | 空 | 空 | 有（N-1） | 保留 | ui.js:965-971 |
| #localOptions | block（本地）/none | — | none | none | — | — | ui.js:340 |
| #onlinePanel | none（本地）/block | — | block | block | block | — | ui.js:341 |
| #connectPanel | — | — | block | none | none | — | ui.js:355,378 |
| #roomLobby | — | — | none | block | none | — | ui.js:356,428 |
| #createRoomForm / #joinRoomForm | — | — | — | block / none（重连时 none） | none | — | ui.js:358-359,399-400 |
| #roomActions | — | — | — | block | none | — | ui.js:360,401,428 |
| #startGameBtnOnline | — | — | — | 房主 block / 非房主 none | — | — | ui.js:402,416 |
| #waitingMsg | — | — | — | 房主 none / 非房主 block | — | — | ui.js:403,417 |
| #showdownModal | none | none | — | — | — | flex（.show） | ui.js:79-85 |
| #raiseSlider | — | .show 时 flex | — | — | 同上 | 若打开需先收起 | ui.js:2367-2416 |
| #bustedInfo | block（仅 busted） | none | — | — | — | — | ui.js:2625-2630 |
| #rulesContainer | 常驻（页面底部） | 常驻 | 常驻 | 常驻 | 常驻 | 常驻 | index.html:369 |
| 键盘提示行（动态插入 #app 末尾） | 常驻 | 常驻 | 常驻 | 常驻 | 常驻 | 常驻 | ui.js:3026-3030 |

### 1.5 启动流程（DOMContentLoaded，ui.js:3014-3067）

1. document.documentElement.classList.toggle('electron-shell', !!window.windowControls) —— 只有 Electron 壳才显示原生窗口按钮/工具栏分离逻辑（ui.js:3015）。
2. setupDesktopHeader()（滚出/小窗口工具栏逻辑，ui.js:3016,2981-3012）。
3. ResizeObserver 观察 #aiPlayersContainer（座位重排，ui.js:3017-3020）。
4. loadAppSettings()；updateTurnAlertButton()；restoreSavedInputs()；syncLocalServerEndpoint()（ui.js:3021-3024）。
5. 插入键盘提示行（ui.js:3027-3030）。
6. 同步模式按钮/短牌 checkbox；updateStartScreenMode()（ui.js:3032-3037）。
7. **立即创建一个“预览用”牌局实例**（标准 9 AI / 短牌 5 AI），绑定回调、音乐、init()；但不渲染（ui.js:3039-3045）。
8. updatePlayerCountOptions()、syncGameSettingsUI()；音量滑块、音乐音量/播放模式、音乐按钮（off）、播放模式按钮（ui.js:3047-3055）。
9. 服务器地址/昵称输入框 change 时 saveAppSettings（ui.js:3057-3060）。
10. 隐藏公共牌区/底池（绝对定位会透出）（ui.js:3062-3066）。

> 注意：第 7 步与 T1 重复创建实例——startNewGame 会先 game.resetGame() 再换新实例。egui 实现建议干脆**不要在启动时建局**，只在开始页用常量渲染（见 §9.1）。

### 1.6 返回行为汇总

| 入口 | 行为 | 源码 |
| --- | --- | --- |
| 主页（home 按钮） | 见 T14：**联机先离开房间**，本地重开一局（resetGame→startNewGame），不弹二次确认 | ui.js:2919-2947 |
| goToStartScreen(reason) | 显示主页、隐藏 bottomPanel、恢复人数选择器与「开始游戏」、清空公共牌/底池/座位/状态行/轮次标签；reason==='busted' 显示破产提示 | ui.js:2607-2652 |
| 刷新（refresh 按钮） | 不离开页面；联机重连、本地重开 | ui.js:2950-2966 |
| 窗口关闭 | 直接 window.windowControls.close()，**无确认**【未定义确认对话框】 | ui.js:2910-2912 |
| 关闭窗口 | window-all-closed → app.quit()；关主窗口 closed → destroy 托盘服务 → app.quit() | main.js:135-145 |

---

## 2. 布局规格

### 2.1 窗口与桌面壳（main.js:101-132）

| 项 | 值 | 源码 |
| --- | --- | --- |
| 默认窗口 | 1280 × 860 | main.js:102-103 |
| 最小窗口 | 900 × 600 | main.js:104-105 |
| 边框 | frame: false（无边框）+ thickFrame: true（Windows 保留边缘缩放/阴影） | main.js:106-107 |
| 阴影 | hasShadow: true | main.js:108 |
| 初始背景色 | #090d18（避免白闪） | main.js:110 |
| 窗口标题 | 创建时 Texas Hold'em；加载后设 Texas Hold'em ♠♥♣♦ | main.js:111,125 |
| 菜单栏 | 隐藏 | main.js:124 |
| 单实例 | app.requestSingleInstanceLock()；第二个实例聚焦/还原已有窗口后退出 | main.js:12-13,29-34 |
| 低内存开关 | --poker-low-memory → app.disableHardwareAcceleration() | main.js:17 |
| 页面标题 | 德州扑克 v1.5.8 | index.html:8 |
| 页面 CSP | default-src 'self'; connect-src 'self' ws: wss:; …（egui 无对应） | index.html:6-7 |
| 本地服务 | 应用内启动 127.0.0.1:3000；EADDRINUSE 时先探测既有服务（要求 hello.serverId='texas-holdem-game'、protocolVersion=2、appVersion 完全相等），否则用 port 0 随机端口；UI 显示实际地址 | main.js:36-71,86,128-132 |
| 本地服务地址 UI | #localServerHint 文案改为「本机服务会随应用自动启动 · 当前地址 ws://127.0.0.1:PORT」；仅当地址栏是 localhost/127.0.0.1 时覆盖 | ui.js:147-166 |

egui/eframe 0.37 对应实现（spike 已实测，见 §9.12）：
- ViewportBuilder::default().with_inner_size([1280.0, 860.0]).with_min_inner_size([900.0, 600.0]).with_decorations(false).with_resizable(true)；
- 无边框拖动/双击最大化/8 方向 BeginResize/最小化/关闭均有 egui ViewportCommand 对应（spike §3.7/§8.3）；原生系统边框可用 with_decorations(true) 或运行时 ViewportCommand::Decorations 切换。
- 注意：原实现的 -webkit-app-region 拖动与 thickFrame 原生缩放，在 egui 需自己实现（§9.8）。

### 2.2 页面骨架（DOM 顺序 → egui 区域）

~~~text
body(.bg-layer 固定背景装饰: 20 个花色/🃏/✦, 不响应指针)
└─ #app (max-width 1200px, margin auto, padding 10px)
   ├─ button#headerHoverZone(固定顶部悬停触发条, 仅 header-detached 显示)
   ├─ #headerHome
   │  └─ #appHeader.header (-webkit-app-region: drag = 窗口拖动区)
   │     ├─ h1 ♠ <span>Texas|Short Deck</span> Hold'em ♥ <small>v1.5.8</small>
   │     ├─ .header-info: 手牌 #N | SB/BB(Ante/最小下注) | 音乐 提示音 下一首 播放模式 音量滑块
   │     └─ .header-actions(绝对定位右上): 刷新 主页 全屏 [最小化 最大化 关闭]
   ├─ #gameStatus.game-status (居中文案, min-height 30px)
   ├─ p#strategyFocusNote (Bot 策略重点小字)
   ├─ #tableContainer.table-container (椭圆牌桌, overflow hidden)
   │  ├─ .table-felt (内阴影晕影)
   │  ├─ .table-decor d1/d2/d3 (大花色水印)
   │  ├─ #bettingRoundLabel (顶部居中: 翻牌前/翻牌 ♠/…)
   │  ├─ #aiPlayersContainer.players-layout (absolute inset 30px, pointer-events none)
   │  ├─ #communityArea (绝对定位 top:58% 居中, 5 个槽)
   │  ├─ #potDisplay (绝对定位 top:58% translateY(70px))
   │  └─ #startScreen (覆盖层, 主页内容; z-index 10)
   ├─ #bottomPanel (display:none 直到开局; flex row wrap gap 20)
   │  ├─ #humanArea.human-area (flex:1, min-width 320)
   │  │  ├─ #humanPosBadges (D/SB/BB)
   │  │  ├─ .human-card-area: #humanCards(2 张, 可点击翻面) + #humanCardsLabel
   │  │  ├─ .human-stack-info: 本局下注 $ / 筹码 $
   │  │  ├─ #actionsContainer: 弃牌 过牌 跟注 加注 全下
   │  │  ├─ #actionExplain (解释文字, min-height 22)
   │  │  └─ #raiseSlider (.raise-slider-container, 默认 display none)
   │  └─ #probPanel.probability-panel (flex 0 0 280px)
   │     ├─ #probToggle 胜率分析 ▼
   │     ├─ #probContent.collapsed (默认收起): 3 条概率条 + 手牌分析 + outs
   │     └─ #privacyAnalysisShield (手牌隐藏时遮蔽层)
   └─ (动态插入) 键盘提示行
#rulesContainer (页面底部, 折叠规则说明, 独立于 #app)
#showdownModal.modal-overlay > #modalContent (可拖动结算弹窗, 独立于 #app)
#debugPanel + #debugFab (debug.js 动态创建, 固定定位)
~~~

源码：index.html:36-347（结构）、ui.js:3026-3030（提示行）、debug.js:18-112（调试面板）。

### 2.3 尺寸常量表

| 项 | 值 | 源码 |
| --- | --- | --- |
| 内容最大宽度 | 1200px（窗口 1280 时左右 40px 留白） | style.css:28-33 |
| 页面 padding | 10px | style.css:31 |
| 头部 padding | 16px 20px 12px；圆角 16；下边距 15 | style.css:83-95 |
| 头部 h1 | 28px（高度 ≤760 紧凑模式 22px） | style.css:188-195,1507-1508 |
| 头部 info / 盲注 | 14px / 12px | style.css:218-226,1174-1181 |
| 牌桌 | 圆角 200px/130px，8px #5d4037 木边，padding 30，min-height: max(520px, var(--seat-min-table-height))，margin 12px 0 | style.css:309-323 |
| 牌桌内边距 | 座位容器 inset 30px（absolute top/left/right/bottom） | style.css:456-465 |
| 公共牌区 | 绝对定位 top:58%; left:50%; translate(-50%,-50%)，gap 10，min-height 110px | style.css:350-363 |
| 公共牌尺寸 | 正常 58×82（圆角 6）；槽 58×82；占位 70×100 虚线；≤768 宽 46×66 | style.css:377-384,557-572,1199-1204 |
| 底池 | 绝对定位 top 58%，居中后 translateY(70px)；22px 加粗金色；≤768 → 18px | style.css:433-449,1198 |
| 玩家座位 | width:104px; height:210px; min-width:95; min-height:130；translate(-50%,-50%)（left/top 是中心点） | style.css:467-482 |
| 座位（≤650 高） | height:180px、padding-block 4、头像 19px、小牌 32×46、dots 16px | style.css:1519-1527 |
| 座位（≤768 宽） | min-width 80 / min-height 110 / padding 4px 6px；容器 inset 15px | style.css:1206-1207 |
| 手牌（大） | 58×82，白底，rank 16 / suit 20 | style.css:557-585 |
| 手牌（小，座位内） | 38×56，rank 11 / suit 14 | style.css:613-620 |
| 底部区域 | flex wrap gap 20px；human-area flex:1 min-width:320；prob-panel flex:0 0 280px | style.css:623-642,844-851 |
| 动作按钮 | padding 10px 20px，14px 加粗，圆角 8 | style.css:715-727 |
| 开始按钮 | 18px，padding 15px 40px，圆角 12 | style.css:802-808 |
| 加注滑块 | 宽 280，max-width 90vw；容器 padding 10 | style.css:816-841 |
| 结算弹窗 | 内容 max-width:500px; width:90%，padding 30，圆角 20，2px #e94560 边 | style.css:1043-1052 |
| 分析面板 | 280 固定宽，padding 15，圆角 15 | style.css:844-851 |
| 概率条 | track 高 14，圆角 7；label/value 各 45px | style.css:887-934 |
| 滚动条 | 8px，轨道 #1a1a2e，滑块 #e94560（hover #ff6b81） | style.css:1219-1223 |
| 悬停条默认高度 | var(--header-hover-height, 64px)（JS 写入 85% 头高） | style.css:109-119 |

### 2.4 牌桌几何与座位布局（核心算法，必须 1:1 复刻）

源：ui.js:884-971（尺寸缓存/重排/节点同步）、ui.js:1996-2040（坐标函数）。

#### 2.4.1 容器尺寸

~~~js
getTableSize(container):
  if cache.node === container && cache.width>0 && cache.height>0: return cache
  w = container.offsetWidth; h = container.offsetHeight      // 座位容器 = 牌桌内缩 30px 的区域
  if w>0 && h>0: cache = {node, w, h}
  return { w: w || 900, h: h || 420 }                        // 隐藏时兜底 900x420（不写缓存）
~~~

#### 2.4.2 座位尺寸（动态测量）

~~~js
relayoutSeats():
  seats = container.children
  width  = max(95,  max(seat.offsetWidth))                   // 正常 104（窄屏 80）→ 取 104
  height = max(innerHeight<=650 ? 180 : 210, max(seat.offsetHeight))
  border = table 的 border-top-width (8)
  inset  = style.top(30) + style.bottom(30) + border*2 (16)   // = 76
  drop   = innerHeight<=650 ? 16 : 42                         // 弧线最大下沉
  topCapacity = clamp(floor(container.offsetWidth / 130), 3, 7)
  // 座位数超过一排容量时，把牌桌撑高到能放下两排完整座位，避免侧面座位被裁剪：
  minimum = seats.length > topCapacity ? ceil(height*2 + drop + 32 + inset) : 0
  table.style["--seat-min-table-height"] = minimum + "px"
  positions = calculateTablePositions(seats.length, size.w, size.h, width, height)
  for seat in seats: seat.left = positions[seat.dataset.tablePositionIndex].x; seat.top = positions[...].y   // px，中心点
~~~

- 重排在 requestAnimationFrame 中合并（ui.js:904-910）；窗口 resize、全屏切换、ResizeObserver、座位数变化都会触发（ui.js:942-947,3017-3020）。
- 座位节点**只增删到目标数量**，已有节点复用（syncSeatNodes，ui.js:950-962）；回主页必须 unobserve 并清空（clearSeatNodes，ui.js:965-971；回归测试 tests/ui-network.test.js:511-520）。
- dataset.tablePositionIndex 决定该座位用第几个坐标（本地=渲染顺序 i，联机=视角换算后的 screenIdx，见 2.4.4）。

#### 2.4.3 calculateTablePositions(aiCount, cw, ch, seatWidth=95, seatHeight=170)

~~~js
if (aiCount === 1) return [{ x: cw/2, y: Math.max(seatHeight/2 + 12, ch*0.07) }]   // 单挑：正上方

cap        = clamp(floor(cw/130), 3, 7)          // 一排最多放的座位数（宽屏 7）
sideCount  = min(2, max(0, aiCount - cap))       // 最多左右各 1 个
topCount   = aiCount - sideCount
left  = sideCount > 0 ? 1 : 0
right = sideCount > 1 ? 1 : 0

topY   = max(seatHeight/2 + 12, ch*0.06)
margin = max(seatWidth/2 + 12, cw*0.03 + (left+right)*20)
span   = cw - 2*margin
sideX  = max(seatWidth/2 + 32, cw*0.09)
sideY  = max(topY, ch - seatHeight/2 - 16)
arcDrop= innerHeight<=650 ? 16 : 42

positions = []
if (left)  positions.push({x: sideX, y: sideY})                       // 1. 左侧（顺时针第一个，人类左手边）
for i in 0..topCount-1:                                              // 2. 顶排从左到右
    t    = topCount>1 ? i/(topCount-1) : 0.5
    x    = margin + span*t
    dist = abs(t-0.5)*2                                              // 0 中心，1 两端
    drop = arcDrop * dist*dist                                       // 两端下沉，形成弧线
    positions.push({x, y: topY + drop})
if (right) positions.push({x: cw - sideX, y: sideY})                 // 3. 右侧（顺时针最后，回到人类）
~~~

语义：**人类固定在 6 点钟方向（底部 humanArea，不在座位容器里）**；座位按顺时针顺序 = 左侧 → 顶排左→右 → 右侧；弧线让顶排两端更低，避免与公共牌/底池重叠。

#### 2.4.4 联机视角换算（每人从自己座位看牌桌）

~~~js
// players 数组是服务端座位序；mySeat = 我的下标；totalN = 人数
for each player i != mySeat:
    steps     = ((i - mySeat) % totalN + totalN) % totalN   // 顺时针步数
    screenIdx = steps - 1                                    // positions[0] = 我左手边第 1 个
    seat.dataset.tablePositionIndex = screenIdx
~~~

source ui.js:998-1006。本地模式：AI 座位按 game.players 顺序（human 为 0）渲染，tablePositionIndex = i（ui.js:1814-1826）。

#### 2.4.5 公共牌 / 底池 / 轮次标签定位（style.css）

| 元素 | 定位 |
| --- | --- |
| #communityArea | position:absolute; top:58%; left:50%; transform:translate(-50%,-50%)（桌内坐标） |
| #potDisplay | 同上，transform:translate(-50%,70px) → 落在公共牌下方 70px |
| #bettingRoundLabel | 牌桌顶部居中 top:15px，11px 半透明大写，字距 2px |
| 座位容器 | inset:30px（本地直接等于牌桌内区域） |
| 弧线/兜底 | ≤650 高时公共牌/底池 top 改 56%（style.css:1519-1527） |

### 2.5 底部区域（人类 + 分析）

~~~text
#bottomPanel (display:flex; flex-wrap:wrap; gap:20px; margin-top:15px)
├─ #humanArea (flex:1; min-width:320; display:flex; align-items:center; gap:20)
│  ├─ 位置徽章（在 human-card-area 上方，或 human-area 首个 flex item）
│  ├─ human-card-area（竖排: 2 张手牌 + 标签 11px）
│  ├─ human-stack-info（竖排: 本局下注 11px 灰 + 金额 20px 绿；筹码 同）
│  ├─ actions-container（5 按钮 flex wrap gap 8）
│  ├─ action-explain (12px 灰, max-width 450, min-height 22)
│  └─ raise-slider-container（打开时显示, 竖排居中）
└─ #probPanel (280px 固定)
~~~

- #humanArea.active：轮到自己时加金色边框+光晕（ui.js:661-666,1615-1620；style.css:644-647,1819-1821）。
- ≤768 宽：#bottomPanel 改列；human-area min-width:auto 且 wrap；prob-panel flex:1（style.css:1212-1214）。

### 2.6 顶部工具栏悬停区（滚出/小窗口时）

原实现（Electron 壳）：
- 头部默认随页面滚动；当 innerHeight<=650（compact，头部高度记 0）**或** #headerHome 底边 <=0（滚出视口）时，给 html 加 .header-detached（ui.js:2996）。
- .header-detached 下：#headerHoverZone 显示（position:fixed; inset:0 0 auto; height: 0.85*header.offsetHeight，默认回退 64px；-webkit-app-region:no-drag）；.header 变 position:fixed; top:0; left:50%; width:min(1180px, 100%-20px); z-index:100; transform:translate(-50%,-110%); visibility:hidden; pointer-events:none（style.css:121-135）。
- 显示条件（:has CSS）：悬停/聚焦 #headerHoverZone、悬停 .header、或键盘焦点在 header 内（.header-keyboard-focus）；显示时 z-index:102; translate(-50%,0); visible; pointer-events:auto，同时悬停区 pointer-events:none 防止挡住头部（style.css:137-151）。
- 键盘焦点标记：任意 Tab keydown 给 html 加 .header-keyboard-focus，任意 pointerdown 移除（ui.js:3004-3007）。
- 过渡 0.15s；prefers-reduced-motion 时关闭（style.css:134,153-155）。
- --header-hover-height = round(header.offsetHeight * 0.85)px 由 JS 写入；头部高度 + margin-bottom 用于 #headerHome 占位高度（ui.js:2988-2996）。

egui 落地：无“页面滚动”，但可用 ScrollArea 包住主页+牌桌，按 scroll_offset > header_h 判定 detached；inner_size.y < 650 直接 detached。用 ui.interact(rect, id, Sense::hover()) 实现悬停条，焦点用 response.has_focus() 或 ctx.memory(|m| m.focused()) 判定（详见 §9.6）。注意 egui 0.37 的稳定 Id 用 ui.scope_id().with(...) 或 Id::unique，不要用已弃用的 Id::new（spike §9）。

### 2.7 小窗口与滚动策略

- 牌桌 overflow:hidden（style.css:322），座位容器不裁剪，因此**座位必须始终落在牌桌内**：靠 --seat-min-table-height 撑高牌桌，让侧面两排座位有空间（§2.4.2）。
- 页面整体滚动（body 无 overflow-y 限制，仅 overflow-x:hidden，style.css:9-16）；README:78 明确：“为保证满桌座位完整，小窗口保留牌桌最低高度，可继续上下滚动”。
- 高度断点：≤760 → 整体紧凑（头 22px、padding 缩小、设置区间距缩小，style.css:1505-1517）；≤650 → 座位 180 高、头像 19px、公共牌/底池 56%、--seat-min-table-height 下限 480（style.css:1519-1527）；宽度断点 ≤768（style.css:1190-1217）。
- 紧凑模式下头部脱离文档流（height:0），所以“滚出”判定优先 compact（ui.js:2989-2996）。
- egui：窗口内容用 egui::ScrollArea::vertical()；牌桌区域最小高度 = max(520, seat_min_table_height)；≤760/≤650 两档缩放用逻辑像素判断 available_height，不要乘 DPI（spike §3.3）。

### 2.8 字体与颜色主题

**原实现没有 :root CSS 变量**；只有 3 个 JS 注入的自定义属性：--header-hover-height、--seat-min-table-height、--deal-delay（ui.js:778,928,2992）。颜色是散落的字面量，下表是**去重后的主题 token**（建议直接转成 Rust 常量/Theme 结构）：

| token | 值 | 用途 | 代表位置 |
| --- | --- | --- | --- |
| 页面底色 | #0d0d1a | body | style.css:11 |
| 雾蓝背景层 | 底 #303f57 + radial-gradient(#8aacc152) + repeating-conic-gradient(#aabdd915) 58px | body.bg-layer | style.css:19-26 |
| 窗口初始底色 | #090d18 | Electron backgroundColor | main.js:110 |
| 文本主色 | #e0e0e0 | body | style.css:12 |
| 文本次色 | #aaa | 状态/说明/按钮 | style.css:224,1126-1136 |
| 文本弱化 | #888 / #666 / #777 / #6f7485 / #7f8494 | 提示/刻度/次要 | style.css:1230,1480-1491 |
| 主题红 | #e94560（hover #ff6b81） | 标题/强调/关闭态 | style.css:190,185 |
| 金色 | #ffd700 | 筹码/高亮/焦点 | style.css:438,747 |
| 绿色 | #4caf50 / #81c784 | 筹码/胜率/成功 | style.css:519,922 |
| 蓝色 | #2196f3 / #64b5f6 | 平局/过牌/连接 | style.css:923,552 |
| 红色 | #f44336 / #e57373 | 输率/弃牌 | style.css:924,551 |
| 橙色 | #ff9800 | 加注/本局下注 | style.css:265,554 |
| 紫色 | #7c4dff | SB 徽章 | style.css:1287-1291 |
| 青色 | #00bfa5 | BB 徽章 | style.css:1293-1297 |
| 牌桌绒面 | radial-gradient(#1b5e20, #0d3b0f 50%, #0a280b) | 牌桌 | style.css:311 |
| 牌桌木边 | #5d4037，8px | 牌桌边框 | style.css:315 |
| 卡面 | 白 #fff；红 #d32f2f；黑 #212121 | 扑克牌 | style.css:565,584-585 |
| 牌背 | linear-gradient(135deg,#1a237e,#283593) + #3949ab 边 + 斜纹；弃牌牌背 #c62828/#b71c1c | 牌背 | style.css:587-611 |
| 面板底 | rgba(0,0,0,.28~.35) | 面板/座位/弹窗行 | style.css:844-851,467-482 |
| 分隔线 | rgba(255,255,255,.05/.06/.08/.1) | 分隔 | style.css:883,1929 |
| 焦点环 | 3px solid #ffd700，offset 3px | 所有可聚焦控件 | style.css:743-749 |
| 弹窗遮罩 | rgba(0,0,0,.7) | modal overlay | style.css:1027 |
| 弹窗渐变 | linear-gradient(135deg,#1a1a2e,#16213e) | 弹窗内容 | style.css:1044 |
| 头部渐变 | linear-gradient(135deg,#0f1923,#1a1a2e 40%,#16213e) | 头部 | style.css:86 |
| 规则标题 | #ffd700；小节 #e94560 | 规则说明 | style.css:1866-1881 |

字体规格：

| 用途 | font-family | 字号/字重 | 源码 |
| --- | --- | --- | --- |
| 全局 | 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif | 正文继承；user-select:none | style.css:9-16 |
| h1 标题 | 继承 | 28px / 700 / 字距 4px | style.css:188-195 |
| 版本徽章 | 继承 | 10px / 700，圆角胶囊，金边 | style.css:202-216 |
| 开始页标题 | 继承 | 28px 金色 | style.css:1150-1154 |
| 弹窗标题 | 继承 | 24px 金色居中 | style.css:1059-1064 |
| 底池 | 继承 | 22px / 700 / 金色 | style.css:434-439 |
| 玩家名 | 继承 | 11px / 600 / 白，单行省略 | style.css:505-515 |
| 座位筹码 | 继承 | 12px / 700 / 绿 | style.css:517-522 |
| 按钮 | font-family: inherit | 14px / 600 / 大写 / 字距 .5 | style.css:715-727 |
| 徽章 | 继承 | 10px / 800 / 24×18 / 圆角 3 | style.css:1268-1279 |
| kbd | monospace | 11px，底色半透明白 | style.css:1901-1910 |
| 调试面板 | Consolas, monospace | 11px（正文）/10px | debug.js:21 |
| 单调数字 | font-variant-numeric: tabular-nums | 设置值/刻度 | style.css:1441-1446,1476-1484 |

egui 字体（spike 实测结论，详见 §9.5）：
- eframe 0.37 默认带 system_fonts（fontique）特性，NativeOptions.system_font_fallback 默认 true，Windows 走 DirectWrite、Linux 走 fontconfig；**不写字体加载代码中文也能显示**，但不同机器字形可能不同。
- 推荐：内置 GB2312 子集（实测 3,183,992 B，含 ASCII + 6763 汉字 + 全角标点 + 花色符号）+ 保留 system_font_fallback=true 作罕见字兜底；不要内置 19.5MB TTC。
- 花色符号 ♠♥♦♣ 与 emoji 头像（😤😎🕶️👑🦊🐺🐦…）在 egui 中需要 emoji 字体回退或改为绘制图标；详见 §9.5。

### 2.9 动效时长/缓动表（迁移到 egui 必须保留节奏）

| 动效 | 时长/缓动 | 源码 |
| --- | --- | --- |
| 背景花色淡入 | 2s ease，逐个 delay 0.1~2.0s（仅首屏） | style.css:52-80 |
| 头部悬停展开 | 0.15s ease（translateY + visibility） | style.css:134 |
| 公共牌发牌 | 0.42s cubic-bezier(.2,.8,.25,1)，逐张 delay 150ms | style.css:386-431 |
| 公共牌翻面 | 0.52s cubic-bezier(.2,.72,.25,1)；新牌翻面时刻 = 新牌数*150 + 330 + 序号*180 ms | style.css:391-402；ui.js:775-785 |
| 自己的手牌发牌+翻面 | 260ms 后开始翻，翻牌 420ms（中点 175ms 切换内容） | ui.js:1721-1744 |
| 手牌点击翻面 | 0.36s，中点 175ms | ui.js:1704-1719；style.css:674-683 |
| 座位动作标签弹出 | 0.6s 弹跳；800ms 后移除 action-pop（animation 类） | style.css:1773-1791；ui.js:1905-1936 |
| 最近动作气泡 | 3s 上浮淡出，DOM 3500ms 后删除；横向位置 20% + random*60% | style.css:1673-1748；ui.js:1568-1586 |
| 思考点 | 1s 无限 pulse（透明度 .3↔1，scale .8↔1.1） | style.css:1756-1771 |
| 全下按钮 | 1.5s 无限 pulse 光晕 | style.css:787-800 |
| 分析面板折叠 | max-height 600→0 + opacity 0.3s | style.css:941-953 |
| 概率条 | width 0.5s ease | style.css:916-920 |
| 弹窗 | 遮罩 fadeIn .3s；内容 slideUp .3s | style.css:1031,1051 |
| 加注面板 | 显示/隐藏瞬时（display 切换） | style.css:817-829 |
| reduced-motion | 发牌/翻牌动画时长→0.01ms，头部过渡关闭 | style.css:153-155,1960-1970 |

---

## 3. 组件清单与行为

### 3.1 顶栏（.header，index.html:40-71）

| 子项 | 内容/行为 | 源码 |
| --- | --- | --- |
| 标题 | ♠ <span>Texas</span> Hold'em ♥ <small class="app-version">v1.5.8</small>；updateModeUI() 只改 span 文本为 Texas/Short Deck（短牌时成为“♠ Short Deck Hold'em ♥”） | index.html:41；ui.js:2703-2706 |
| 手牌计数 | 手牌 #<span class=hand-count>；本地每次 onUpdate 写 g.numHands，联机写 handId ?? handNumber ?? handCount ?? numHands | index.html:43；ui.js:1495,571-572 |
| 盲注信息 | 标准 SB: x / BB: y；短牌 Ante: x / 最小下注: y；值来源=设置或服务端 roomConfig | index.html:44-47；ui.js:182-195,804-810 |
| 音乐按钮 | 音乐关 ↔ 音乐开 ↔ ⚠️ 音乐关；title 随状态；aria-pressed | index.html:49；ui.js:3101-3117 |
| 提示音按钮 | 提示音开 ↔ 提示音关；title 开/关提示 | index.html:50；ui.js:3141-3149 |
| 下一首 | 下一首按钮，title 下一首 | index.html:51 |
| 播放模式 | 顺序 → 单曲 → 随机 循环；title 当前：顺序播放；点击切换 等 | index.html:52；ui.js:3165-3185 |
| 音量 | 音量图标 + range 0-100 value 50 width 60，accent #e94560 | index.html:54-58 |
| 窗口/动作按钮 | 刷新（重新连接/刷新牌局）、主页（返回主页）、全屏；窗口控件：— 最小化、□ 最大化（最大化后 ❐ 还原）、× 关闭；**仅 Electron 壳显示窗口控件** | index.html:61-70；style.css:243-277；ui.js:2899-2908 |
| 拖动 | .header 整体 -webkit-app-region: drag；内部 button/input/select/a 为 no-drag；因此**头部空白/标题区可拖窗**，双击原生最大化 | style.css:93-102 |
| 聚焦环 | 所有 button/input/select/[role=button] :focus-visible 金色 3px 外环 | style.css:743-749 |

标题图标细节：.header::before 是 ♠ ♥ ♦ ♣（10px，字距 12，金色 15%）；.header::after 是顶部 2px 渐变彩条（style.css:166-186）。

### 3.2 状态行（#gameStatus）

居中文案，min-height:30px（style.css:1126-1132）；同时被当作**联机错误/提示消息的唯一出口**（showNetworkError 写它，ui.js:485-488）。文案全集见 §8.2。

### 3.3 牌桌背景装饰

- .table-felt：内圈透明→外圈 20% 黑晕影，不响应指针（style.css:339-348）。
- .table-decor d1/d2/d3：♠(左上 10%,10%,-20°)、♥(右下 10%,10%,20°)、♦(上 40%,水平居中) 80px 白 2% 透明度（index.html:81-83；style.css:1225-1234）。
- #bettingRoundLabel：本地 翻牌前 / 翻牌 ♠ / 转牌 ♥ / 河牌 ♦；联机 翻牌前/翻牌/转牌/河牌；idle 清空（ui.js:2314-2322,1193-1196）。
- p#strategyFocusNote：固定小字「Bot 策略重点优化：标准十人桌 · 起始 5000 · 盲注 100/200 · 单挑河牌范围博弈」，11px，rgba(176,199,220,.64)，居中（index.html:78；style.css:157-164）。

### 3.4 公共牌区

- 5 个槽位，占位文案依次 翻牌/翻牌/翻牌/转牌/河牌（index.html:90-96；ui.js:726-734）。
- 渲染策略（renderCommunityCardsWithFlip，ui.js:736-797）：
  1. 用 rank-suit key 数组做脏检查；key 完全一致则跳过（ui.js:742-745）。
  2. 前缀匹配时只给“新增”的牌做发牌+翻面动画，已存在的牌直接正面（ui.js:747-749,763,775-785）。
  3. DOM 结构：stage > flipper > (back, front)；is-dealing 触发发牌关键帧；延时后 is-revealed 触发 rotateY(180deg)。
  4. token 用于作废旧动画回调（快速连续更新时丢弃旧定时器）。
- 牌面：左上角 rank + 花色符号 ♠♥♦♣，红=hearts/diamonds（ui.js:757-770,1641-1652）。

### 3.5 底池

底池: $X，formatChips 千分位（ui.js:622,2310-2312）；初始 底池: $0。

### 3.6 玩家座位

#### 3.6.1 结构（两套骨架，内容字段一致）

| 部件 | 本地座位（createLocalSeatNode，ui.js:1945-1989） | 联机座位（createOnlineSeatNode，ui.js:1064-1119） |
| --- | --- | --- |
| 根 | .player-seat | .player-seat |
| 位置徽章 | .position-badges（绝对定位 top:-12px，动态重建） | 3 个独立 .pos-badge（D/SB/BB，内联样式，只切 display） |
| 头像 | .player-avatar（emoji，22px） | 同（联机：player.avatar / isHuman?'👤':'🤖'） |
| 名字 | .player-name（11px 600，单行 ellipsis，max-width 100%） | 同 |
| 筹码 | .player-stack（12px 绿） | 同（ALL-IN / 已破产 / $X） |
| 手牌 | 2 张 .card.card-back.small，摊牌/idle 时翻成正面，弃牌时用 card-back-logo | 2 张牌背；弃牌 card-back-logo；**showdown/idle 时隐藏（display:none）**，因为拿不到别人的牌 |
| 下注 | .player-bet（金色，setSeatSlotVisible 保留占位） | 同 |
| 最近动作 | .player-last-action（本地有；action-pop 动画） | 无（联机用浮动气泡） |
| 思考点 | .thinking-dots（...，绝对定位 bottom:3px） | 同 |
| 状态类 | folded / all-in / active（本地当前 AI）/ thinking | folded / all-in / thinking（联机无 active，thinking 由 currentPlayerIdx 判定） |
| 数据 | data-player-index（在 game.players 的下标）、data-table-position-index | data-table-position-index |

本地座位细节：
- 状态类由 game.currentPlayer 的 name 相等判定 active（ui.js:1828-1835）。
- 摊牌（phase==='showdown'）或 idle 时 AI 手牌翻正面（ui.js:1882-1887）；弃牌用红色牌背 logo。
- ALL-IN 显示为纯文本，不加 label 前缀（ui.js:1866）。
- 最近动作标签：弃牌/过牌/跟注/加注/全下/小盲/大盲；带金额的动作显示 加注 $X；**只在 action 文本变化时重建并重启动画**，800ms 后移除 action-pop 类（ui.js:1905-1936；不变量测试 tests/ui-network.test.js:522-540）。
- 座位下注：chipsInPot > 0 && phase!=='idle' 时显示 $X，否则用 visibility:hidden 保留槽位（防跳动，ui.js:1897-1903；setSeatSlotVisible ui.js:869-877）。

联机座位细节：
- thinking 只看 state.currentPlayerIdx === i && !folded（ui.js:1008-1014）。
- 位置徽章按“真实座位下标 i”与 dealerPos/sbIdx/bbIdx 比较，与视角无关（ui.js:1016-1019）。
- 头像/名字/筹码来自服务端 player 字段；下注显示 chipsInPot（ui.js:1035-1041）。
- 联机角标样式为内联：D 金底深字、SB #3498db、BB #e67e22（ui.js:1067-1082）；而人类徽章与本地座位用 CSS 类版本（SB 紫、BB 青）。**原实现视觉不一致**，建议统一（§9.11）。

#### 3.6.2 位置徽章（D/SB/BB）

- 人类：#humanPosBadges 在人类手牌上方；本地按 game.dealerPosition===0 / sbIndex===0 / bbIndex===0（人类固定 seat 0），联机按 mySeat 与 dealerPos/sbIdx/bbIdx 比较（ui.js:1764-1798,813-846）。
- AI：座位内 .position-badges，绝对定位 top:-12px（style.css:1255-1259）。
- 徽章样式：24×18，圆角 3，10px/800；D 金色渐变（深字）、SB 紫色、BB 青色；title 庄家/小盲/大盲（style.css:1268-1297；ui.js:1068-1082）。
- 渲染 key 缓存：local:<dealer>:<sb>:<bb> / online:<mySeat>:<dealer>:<sb>:<bb>（ui.js:1768,819）。

#### 3.6.3 座位浮动动作气泡（showFloatingAction，ui.js:1568-1586）

- 内容：<name>: <label><amount? $x : ''>；label：弃牌/过牌/跟注/加注/全下/小盲/大盲/庄前注（ui.js:1555-1563,700-704）。
- 位置：牌桌容器内 top:40%、left: 20%+random*60%（每次随机）。
- 动画 3s 上浮淡出；3500ms 后移除节点；颜色随 action：fold 红/check 蓝/call 绿/raise 橙/allin 深红/盲注紫（style.css:1690-1726）。

### 3.7 人类区域

- 手牌：2 张 58×82，点击整块 #humanCards（role=button tabindex=0 aria-pressed）切换隐藏/显示；隐藏时显示牌背且**分析面板同步遮蔽**（§3.10）。
- 标签文案：我的手牌（无牌）/ 我的手牌 · 点击隐藏（正面）/ 我的手牌 · 已隐藏，点击查看（背面）；aria-label/title 对应变化（ui.js:1664-1681）。
- 发牌动画：新一局开始 260ms 后触发翻牌，420ms 完成（animateHumanCardsDeal，ui.js:1721-1744）；humanCardsAnimating 期间忽略点击。
- 筹码信息：本局下注（chipsInPot）+ 筹码（stack）；联机破产显示 已破产（红），全下显示 ALL-IN（金）（ui.js:644-658）。
- 动作按钮（index.html:269-275）：
  - 固定 5 个：弃牌 / 过牌 / 跟注 / 加注 / 全下；默认 disabled，开局/轮到时按合法动作启用。
  - 文案动态：过牌 ✓（合法 check 时）、跟注 $X（X=min(toCall, stack) 或 legal call amount）、全下 $X（X=stack 或合法 all-in 金额）、加注。
  - 点击按钮的语义：doAction('fold'|'check'|'call'|'allin')；加注按钮只打开加注面板（showRaiseSlider）。
  - 状态：disabled 时 40% 透明度 + not-allowed（style.css:738-741）；hover 上浮 2px 阴影（style.css:729-736）。
- 解释文字 #actionExplain（12px 灰，居中，max-width 450；style.css:1793-1811）。文案见 §8.4。
- 本地 updateActionButtons（ui.js:2042-2137）：
  - 非我回合：按 idle/已弃牌/已全下/AI 思考中/其他 5 种文案。
  - 我回合：getAvailableActions() 遍历启用按钮；解释文字优先级：无人加注且可过牌 → 「无人加注，你可以过牌或加注」；有 toCall → 若 toCall>=stack「对方下注 $X，你只能全下或弃牌」否则「需要跟注 $X 或选择加注/弃牌」；大盲 option（phase==='preflop' && bbIndex===0 && bbNeedsOption && toCall===0）→「你在大盲位，可以选择过牌或加注」。
- 联机 updateOnlineActions（ui.js:1125-1190）：以 legalActions{actions,toCall,minRaiseTo,maxRaiseTo} + isYourTurn + pendingAction 驱动；文案见 §8.4。

### 3.8 加注面板

打开入口：加注 按钮 或 R 键（ui.js:2367-2408）。

边界来源（两套）：

| 来源 | min | max | step（unit） | 默认值 | 源码 |
| --- | --- | --- | --- | --- | --- |
| 本地 | getAvailableActions().find(raise).amount（引擎已按网格取整） | roundBets[0] + human.stack（加注到的总额上限） | PokerGameRules.wagerUnit(game) = 小盲（短牌=前注） | 标准翻前 bigBlind*3；其余 minRaise + bigBlind*2 | ui.js:2382-2402 |
| 联机 | legalActions.minRaiseTo | legalActions.maxRaiseTo | legalActions.wagerUnit / PokerGameRules.wagerUnit(config) | 同上 | ui.js:2372-2381 |

- 网格：raiseBounds(min,max,unit) = {min: ceil(min/unit)*unit, max: floor(max/unit)*unit, unit}；snapRaise(v) = clamp(round(v/unit)*unit)；仅“主动加注”走网格，跟注/全下用实际整数筹码（game-rules-core.js:8-25）。
- 打开：slider.min/max/step = bounds.*；value = snapRaise(default)；加 .show；slider.focus()（ui.js:2397-2407）。
- 显示：加注 $X（parseInt(slider.value)），并写 aria-valuetext（ui.js:2428-2433）。
- 关闭：取消 按钮 / Esc；若之前打开且 加注 键仍可用，焦点回到 #btnRaise（ui.js:2410-2416）。
- 确认：snapRaise 后 hide → 焦点回 #humanCards → 本地 playerAction('raise', amount)（浮气泡「你: 加注 $X」）；联机夹到 [minRaiseTo,maxRaiseTo] 后 sendAction('raise', bounded)，禁用全部按钮并「等待服务器确认…」（ui.js:2435-2455）。
- 加减：1/←/↓ = -unit；2/→/↑ = +unit；clamp 到 [min,max]（controls.js:62-79；ui.js:2418-2426）。
- 初始 HTML 属性（未打开时无意义，JS 会覆盖）：min=80 max=1000 value=120 step=40，显示「加注到 $60」——**原实现静态占位与 JS 不一致**，迁移时不要照抄（index.html:277-286）。

### 3.9 结算弹窗（.modal-overlay > #modalContent）

结构（index.html:334-347）：⠿ 拖拽移动 把手 → 标题 → 赢家行（<name> 赢得 $<amount> + 牌型: <hand>）→ 结果列表 → 下一手 → 按钮。

- 打开：presentSettlementModal(modal) 加 .show、聚焦按钮、首次出现时 settlementAlertSerial++ 且播两声结算音（ui.js:79-85）。
- 本地摊牌（showShowdown，ui.js:2461-2573）：标题 摊牌；赢家为多人时金额显示 A $x + B $y，单人 $总额；赢家名 你/AI 名；下方「公共牌」横排 chip；每位结果行 = 名字（赢家前缀 🏆）+ 手牌 chips + hand.name，赢家行绿色底 rgba(76,175,80,.15)；**结果行人类名字写作英文 You**（ui.js:2534，与联机版 你 不一致）。
- 联机摊牌（showOnlineShowdown，ui.js:1214-1304）：标题 reason==='fold' ? 胜出 : 摊牌；赢家 你/玩家/AI名 逗号连接；牌型名用 HAND_TYPE_NAMES/SD_HAND_TYPE_NAMES；弃牌胜出时牌型 label 隐藏、行内牌型显示 —；同样有公共牌区与结果行。
- 简单消息：showMessage(title,message)（灰字）用于弃牌胜、破产等；showAIMessage(name,message)（金字）用于 AI 赢（ui.js:2813-2853）；两者仍保留「下一手 →」按钮走 T11。
- 可拖动：按住把手 → position:fixed + 记录 getBoundingClientRect；mousemove 更新 left/top；**无边界限制**（可拖出屏幕，原实现未限制）（ui.js:2721-2811）；触屏同样支持（touchstart/move/end）。每次重新打开都会把位置重置回居中（ui.js:2464-2469,1217-1221,2816-2819）。
- 焦点：打开时强制聚焦 .modal-btn；Tab 在弹窗打开时被强制导向该按钮（controls.js:83-97）；Enter/Space/N 触发下一手（除非焦点在按钮上）。
- 联机下一手按钮状态机见 T12/T13（ui.js:2575-2589,460-483）。
- 破产提示：本地摊牌且玩家破产时，#modalResults 末尾追加「你已输光所有筹码！」（ui.js:1518-1524）。

### 3.10 分析面板（#probPanel）

- 标题 胜率分析 + 箭头：**收起时 ▼，展开时 ▲**（index.html:291-292；ui.js:3082-3089）；初始 collapsed。
- 概率条（index.html:295-317）：
  - 三条：胜率（绿渐变）、平局（蓝）、输率（红）；label 与 value 各 45px，track 高 14。
  - 数据：winProb/tieProb/loseProb（0..1）→ (x*100).toFixed(1)+'%'，条宽 x*100%。
  - 无数据：条 0%；value 显示 0%（还没发手牌）或 计算中（已发 2 张待算）（ui.js:2139-2163）。
- 当前手牌（当前手牌）：
  - 已弃牌/ idle → —；
  - 已有牌 <5 张 → N 张牌 (还需 M 张公共牌)；
  - ≥5 张 → PokerProbabilityCore.evaluate(所有已知牌, isShortDeck).name（中文牌型名）（ui.js:2165-2202）。
- 可击败的牌型：列出**全部 10 种牌型**，低→高顺序；rank 低于当前牌型 → 绿色 .chip；否则红色 .chip.beaten-by（ui.js:2196-2201；style.css:970-989）。短牌顺序：高牌/一对/两对/三条/顺子/葫芦/同花/四条/同花顺/皇家同花顺（shortdeck.js:40-51）；标准：…顺子/同花/葫芦…（cards.js:81-92）。
- 改善牌分析（outs）：
  - 只在**翻牌/转牌**（公共牌 3~4 张）、未弃牌时显示；翻前/河牌隐藏（ui.js:2204-2212）。
  - 无结果 → 「正在计算改善牌…」；无直接补牌 → 「暂无直接补牌」。
  - 每个牌型组：<牌型名>: N 张 · 强提升 S 或 · 踢脚改善 T（前 5 组，按牌型从高到低）；牌 chips 最多 8 张，多出显示 +N 张；底部附 outs.note（10px 灰）（ui.js:2241-2279）。
  - 数据来源：本地 game.getOuts()；联机 probabilityService.calculate(...).outs（ui.js:497-522,550-556）。
- 隐私遮蔽（privacyAnalysisShield，「手牌已隐藏，分析数据同步隐藏」）：
  - 条件：humanCardsHidden && 有 2 张手牌 && 面板展开（ui.js:1664-1681）。
  - 效果：面板加 .privacy-hidden → .prob-content { display:none }（整块移出布局，避免拉高面板）；遮蔽层显示；aria-hidden=true。
  - 收起面板时不显示遮蔽层；重新展开且仍隐藏手牌则显示（测试 tests/ui-network.test.js:433-456）。
- 联机概率：每次 game_state 用 makeKey(手牌/公共牌/对手数/变体) 做键去重；numOpponents = 未弃牌且非自己的人数（至少 1）；计算异步，结果回来后再刷新三块（ui.js:525-560）。

### 3.11 开始界面（#startScreen，位于牌桌容器内覆盖层）

结构（index.html:104-246）：

| 区块 | 内容 | 交互 |
| --- | --- | --- |
| 装饰 | ♠ ♥ ♦ ♣ 60px 大字距 | — |
| 标题 | Texas Hold'em / 6+ Short Deck | 随模式切换（ui.js:1470） |
| 副标题 | 短牌：「36张牌的快节奏短牌对战 — 同花大于葫芦，顺子大于三条」；标准：空（:empty 隐藏） | ui.js:1471-1473 |
| 人数 | label 选择人数；按钮组：标准 2/6/7/8/9/10 人（默认 10），短牌 2~8 人（默认 6） | ui.js:1422-1444；点击由事件委托维护 .selected（ui.js:2764-2770） |
| 游戏模式 | ♠ 标准德州 / ♦ 短牌 6+；选中态蓝/绿边框 | ui.js:310-322 |
| 游玩方式 | 本地游戏 / 联网游戏；本地选中橙色、联网蓝色 | ui.js:325-342 |
| 设置区 | 3 行离散滑块（见下） | ui.js:197-266 |
| 起始深度摘要 | 250 BB 起始深度 / <depth> 个最小下注的起始深度（depth = stack/bigBlind 保留 1 位） | ui.js:246-251 |
| 本地入口 | 开始游戏（tabindex="1"，主页首个 Tab 目标） | index.html:174 |
| 联机面板 | 见 §3.12 | — |
| 破产提示 | 你已输光所有筹码！重新开始吧（红底框，仅 busted） | index.html:243-245；ui.js:2625-2630 |

设置区（起始筹码 / 大小盲 / AI 节奏）：

| 设置 | 控件 | 档位 | 显示 |
| --- | --- | --- | --- |
| 起始筹码 | 离散滑块 0..4 | 5000 / 10000 / 20000 / 50000 / 100000 | 刻度 5千 1万 2万 5万 10万；输出 $20,000 |
| 大小盲注（短牌：前注/最小下注） | 滑块 0..5 | (10,20)(20,40)(40,80)(50,100)(100,200)(200,400) | 输出 $40 / $80；刻度 6 个 |
| AI 行动节奏 | 滑块 0..2 | 快速 800 / 标准 1800 / 沉浸 3200 ms | 输出 快速/标准/沉浸 |

- 源码：index.html:135-170；ui.js:16-24,45-50,197-266；style.css:1403-1496。
- 短牌时“大小盲注”label 变「前注 / 最小下注」，滑块 aria 文案变「前注 X，最小下注 Y」（ui.js:234-237,245）。
- 规则说明里的动态值 [data-config-value]（small-blind/big-blind/ante/dealer-ante/min-bet）随设置实时更新（index.html:382-383,469-471；ui.js:255-265）。
- 设置持久化 localStorage['texas-holdem-settings-v1']：stack/smallBlind/bigBlind/aiDelay/gameMode/musicPreference/turnAlertsEnabled/musicVolume/musicPlaybackMode/musicSettingsVersion(=2)/serverUrl/playerName（ui.js:43,118-131）。音乐设置版本 v2 会强制一重置为 off/50/顺序（ui.js:102-110）。

### 3.12 联机面板（#onlinePanel）

| 区块 | 内容 | 源码 |
| --- | --- | --- |
| 连接行 | 服务器地址 input（默认 ws://localhost:3000，flex 1 min 140）、昵称 input（宽 80，maxlength=8，placeholder 输入你的昵称）、连接 按钮 | index.html:181-187 |
| 本机服务提示 | 本机服务会随应用自动启动 · 默认地址 ws://localhost:3000（若拿到实际端口则改「当前地址 …」） | index.html:188；ui.js:161-162 |
| 房间号卡 | 📋 房间号 + 32px 金色 6px 字距的 ——（加入后显示数字）+「把房间号发给朋友加入」 | index.html:193-197；ui.js:398 |
| 玩家列表 | 👥 玩家列表 + 行：🟢/🔴 + 👤/🤖 + 名字（自己金色 + " (你)"）；按 seatId 升序 | ui.js:1397-1414 |
| 房间配置 | 人数: select（标准 2/6/7/8/9/10 默认 10；短牌 2~8 默认 6）+ 短牌 checkbox | index.html:204-214；ui.js:1447-1465 |
| 加入房间 | 房间号: select（**仅 1–5**）+ 进入房间 | index.html:217-229 |
| 房间内操作 | 开始游戏（仅房主）+ 等待房主开始游戏…（非房主） | index.html:232-235；ui.js:401-403 |
| 底部操作 | 离开房间 / 断开 | index.html:237-240 |
| 房间创建 | 原实现**没有独立“创建房间”按钮**：第一次 join_room 不存在的房间码时服务端自动创建；UI 只有「进入房间」【注意：create_room 消息在 network.js 有封装，但 UI 无入口，ui.js:598-600 的测试确认 doCreateRoom 已删除】 | network.js:196-199；tests/ui-network.test.js:598-603 |
| 房主开始 | 开始按钮调用 startGame() → start_game；没有“准备”概念（ready_for_next 只用于结算后下一手）【原实现未定义首局准备】 | ui.js:1393-1395；network.js:229-231 |
| 房间码规则 | 服务端只会分配 1–5，默认最多 3 个房间；非法码报「房间号只能是 1–5」 | server/local-server.js:238-247 |

联机配置传递：joinRoom(code, name, {maxPlayers,isShortDeck,startingStack,smallBlind,bigBlind,ante,minBet,aiDelay})（ui.js:1354-1371）；服务端仅在建房时采纳首个玩家配置，之后加入者用房间已有配置；UI 收到 room_created/room_joined/game_starting/game_state 都会 applyRoomConfigFromServer，按指纹 gameMode|stack|sb|bb|aiDelay 去重（ui.js:268-293）。

### 3.13 规则说明（#rulesContainer）

- 页面底部折叠条：查看德州扑克规则 ↔ 收起规则说明（ui.js:3073-3080）；展开 .rules-content.open，fadeIn .4s（style.css:1853-1864）。
- 内容为**静态 HTML**（index.html:369-485）：标准牌型表 10 行、位置/盲注/流程/行动选项（含 kbd 快捷键）、下注轮次、AI 风格 6 种、短牌章节（36 张、牌型强弱表、A 作 5、前注制、策略提示）。
- 动态数值：[data-config-value] 的 SB/BB/ante/dealer-ante/min-bet 随设置更新（§3.11）。
- 规则表样式：th 红底金字、td 底部分隔线、行 hover 3% 白（style.css:1912-1935）。
- egui：整块作为静态富文本/CollapsingHeader + Grid 移植；不要在 Rust 里用字符串拼接 HTML。

### 3.14 调试面板（debug.js）

- 入口：右上侧固定圆形 FAB 🐞（fixed，top:60px right:12px，36×36，红底；不是右下角），点击开/关面板（debug.js:104-111）。
- 面板：fixed bottom:12 right:12，520×max 420，深底红字，圆角 8；标题栏「运行时日志」+ 计数 + 折叠箭头 + ✕；可拖动标题栏移动；点标题折叠 body。
- 标签过滤：all / error / network / game / ai（颜色灰/红/紫/绿/黄），点标签筛选条目。
- 日志条目：[时间戳毫秒] [type] msg，可带 detail / 源码位置 / 栈前 3 行；最多 200 条（MAX_LOG=200）。
- 底部：拖动标题栏移动 · 点标签过滤 + 复制日志（复制当前过滤分类；navigator.clipboard，失败回退 textarea + execCommand('copy')）。
- 采集源：window.onerror、unhandledrejection、WebSocket 收发（←/→ type …）、game_state 摘要、console.log 捕获、心跳（>3s 停顿记 error「页面可能卡死 — 停滞 Ns」）。
- 开关：window.__DISABLE_DEBUG__=true 时整个 debug.js 不生效；**仓库 index.html 未设置**，安装包/测试自行注入（debug.js:9-10；tests/ui-network.test.js:239）。
- 原实现的 showDebugToast 被 ui.js 同名函数遮蔽（测试确认已移除，tests/ui-network.test.js:598-603）；ui.js 自带 toast（§3.15）。

### 3.15 Toast（showDebugToast，ui.js:2969-2975）

绿色等宽 12px，fixed; bottom:60px; right:20px，2s 后 0.5s 淡出并移除。触发点：返回主页「返回主页，当前对局已关闭」、离开房间「已离开房间」、刷新「正在重新连接…」「重新发牌」、「请先输入服务器地址」。

### 3.16 键盘提示行（启动时追加，ui.js:3026-3030）

键盘: Tab 选择控件 Enter/空格 激活 · F 弃牌 C 过牌/跟注 R 加注 A 全下 · 加注时数字 1 减 / 2 加，方向键调整 空格 确认 Esc 取消
样式：11px，#bac6d8，居中，margin-top:8px。

---

## 4. 键盘快捷键与焦点规则

### 4.1 快捷键全表

| 按键 | 条件 | 行为 | 源码 |
| --- | --- | --- | --- |
| Tab / Shift+Tab | 任意（非输入框） | 浏览器原生顺序切换焦点；主页首个 Tab 命中 #startGameBtn（tabindex=1）；header 获得键盘焦点时显示“键盘模式”头部 | README.md:31；index.html:174；ui.js:3004-3007 |
| Enter / Space | 加注面板打开 | 确认加注；若焦点在面板「取消」按钮上则取消 | controls.js:70-79 |
| Enter / Space | 结算弹窗打开且按钮可用且焦点不在按钮 | 下一手 | controls.js:91-96 |
| Enter / Space | 焦点在 #humanCards（手牌区） | 切换手牌显示/隐藏 | controls.js:99-102；ui.js:1757-1762 |
| Enter / Space | 焦点在 #probToggle（分析标题） | 展开/收起分析面板 | controls.js:103-107 |
| Space / Enter | 焦点在任意 button | 交给浏览器原生激活（不抢） | controls.js:116 |
| F | 轮到我（本地/联机）且弃牌可用 | 弃牌 | controls.js:120-132 |
| C | 同上 | 过牌可用则过牌，否则跟注 | controls.js:122 |
| R | 同上且加注可用 | 打开加注面板 | controls.js:125-127 |
| A | 同上且全下可用 | 全下 | controls.js:123 |
| 1（顶排）/ ← / ↓ | 加注面板打开 | 金额 -unit | controls.js:62-67 |
| 2（顶排）/ → / ↑ | 加注面板打开 | 金额 +unit | 同上 |
| Esc | 加注面板打开 | 取消加注 | controls.js:67-69 |
| Esc | 全屏中 | 退出全屏 | controls.js:109-113 |
| N | 结算弹窗打开、按钮可用、焦点不在按钮 | 下一手 | controls.js:91-96 |
| Tab | 结算弹窗打开 | 强制聚焦「下一手 →」按钮（防止 Tab 逃出弹窗） | controls.js:86-90 |

### 4.2 全局 keydown 处理优先级（controls.js:53-133）

~~~text
onKeyDown(e):
  if e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey: return
  if isTyping(target): return                       // 见 4.3
  if raiseSliderVisible:                            // ① 加注面板
      digit1/2 (code 必须是 Digit1/Digit2) 或 方向键 → preventDefault, adjustRaiseAmount(±1); return
      Esc  → preventDefault, hideRaiseSlider(); return
      Space/Enter → preventDefault; if repeat return;
                    焦点在面板「取消」→ hideRaiseSlider(); else → confirmRaise(); return
  if showdownModal.show:                            // ② 结算弹窗
      Tab → preventDefault, focus(.modal-btn); return
      (Enter|Space|n) && !btn.disabled && 焦点不在 button → preventDefault, closeModalAndContinue(); return
      return                                        // 弹窗打开时吞掉其余按键
  if 焦点在 [data-ui-key="human-cards"] && (Enter|Space) → handleHumanCardsKey; return   // ③
  if 焦点在 #probToggle && (Enter|Space) → toggleProbPanel(); return                     // ④
  if Esc && 全屏 → preventDefault, toggleFullscreen(); return                            // ⑤
  if 焦点在 button && (Enter|Space) → return        // 交给原生                          // ⑥
  if !(联机轮到我 || 本地轮到我) → return
  F/C/R/A 分派到对应按钮（检查 disabled；repeat 时忽略）                                 // ⑦
~~~

### 4.3 “输入中”例外（controls.js:56-59）

以下 target 被视为输入态，**不触发任何牌桌快捷键/F/C/R/A/1/2/方向键**：
- target.isContentEditable === true；
- TEXTAREA、SELECT；
- INPUT 且 type 不是 range / checkbox / radio / button（即 text/password/number/…）。
- 例外：range 输入框在**加注面板打开时**仍受面板快捷键控制（方向键/1/2/空格生效），因为面板优先级更高（controls.js:62-80；测试 tests/ui-network.test.js:295-347）。
- 1/2 只认顶排数字键：要求 event.code === 'Digit1'|'Digit2'；**Numpad1/Numpad2、F1/F2、文本输入中的数字都不响应**（controls.js:63；测试 :318-347）。
- 修饰键组合（Alt/Ctrl/Meta）一律放行给浏览器；Shift 不算修饰（Shift+Tab 需要原生，F 等不受影响）。

### 4.4 Tab 顺序与初始焦点

- 原生 DOM 顺序（大致）：窗口按钮（— □ ×）→ 音乐按钮组（音乐/提示音/下一首/播放模式/音量）→ 刷新 主页 全屏 → 开始页的人数、模式、玩法、设置滑块、开始按钮 → 联机输入框/下拉/按钮 → 牌桌手牌区、5 个动作按钮、加注滑块/确认/取消 → 分析标题 → 全局控件。
- 主页首次 Tab 优先命中「开始游戏」（tabindex="1"，index.html:174）。
- 手牌区 tabindex=0、分析标题 tabindex=0（index.html:255,291），Enter/Space 可激活。
- 出牌快捷键在“Tab 选中手牌/按钮/音量滑块后仍可用”（README:43；测试 tests/ui-network.test.js:605-623）。

### 4.5 焦点移动时机

| 时机 | 焦点去向 | 源码 |
| --- | --- | --- |
| 结算弹窗出现 | .modal-btn（下一手） | ui.js:82 |
| 打开加注面板 | 金额滑块 #raiseRange | ui.js:2407 |
| 取消加注 | 若「加注」仍可用 → #btnRaise | ui.js:2410-2416 |
| 确认加注 | #humanCards | ui.js:2440 |
| 弹窗打开时 Tab | 强制 .modal-btn | controls.js:86-90 |
| 键盘 Tab 后 | header 进入键盘焦点模式（可悬停展开工具栏） | ui.js:3004-3007 |
| 鼠标/触摸 pointerdown | 清除 header 键盘焦点模式 | ui.js:3007 |

焦点样式：3px solid #ffd700 + offset 3（style.css:743-749）；手牌区 :focus-visible 金色 outline（style.css:670-672）。

### 4.6 与 README 的差异（实现时以代码为准）

| README 描述 | 实际实现 |
| --- | --- |
| 「顶排数字 1/2」 | 代码额外要求 event.code === Digit1/Digit2，Numpad 不算（controls.js:63） |
| 「Enter/空格 普通状态激活选中控件」 | 只有 button 走原生；手牌区/分析标题有专门处理（controls.js:99-116） |
| 「Esc 取消加注」 | 加注面板打开时 Esc 取消；全屏时 Esc 退出全屏（controls.js:67-69,109-113） |
| 「N 摊牌后进入下一手」 | 仅在弹窗打开且按钮可用且焦点不在按钮时生效（controls.js:91） |

### 4.7 egui 实现要求（建议，结合 spike）

1. egui 没有浏览器原生 Tab 焦点链：需自己维护 focus_order: Vec<egui::Id>，按 §4.4 顺序 Tab/Shift+Tab 循环，Space/Enter 激活；对 TextEdit 特殊放行（egui 0.37 的 TextEdit 有完整 IME，spike §3.4）。
2. egui Button 默认 Enter/Space 激活行为要显式控制：在全局快捷键处理前判断“当前焦点是否在文本输入/滑块”，并复用 §4.2 优先级；读取用 ctx.input 与 ui.input_mut(consume_key(...))；Id 用 scope_id().with(...)（Id::new 已弃用，spike §9）。
3. 加注滑块获得焦点时要吞掉方向键（egui::Slider 自身会处理），但 1/2 仍应作用于加注值；Esc 收起面板；Space/Enter 确认。
4. 结算弹窗打开时建议用 egui::Modal（或全屏遮罩 + Window）并做焦点陷阱：Tab 只在弹窗内循环。
5. 记住：原实现的快捷键**只在轮到我时**生效（controls.js:117-119），不要做成全局热键。
6. 中文 IME：TextEdit 在 egui 0.37 已完整接入（IMEOutput → egui-winit → winit WM_IME_*），应用层零代码；仍需 Windows 实测（spike §3.4/§5.2）。

---

## 5. 联机 UI

### 5.1 网络回调 → UI 动作映射（ui.js:346-491）

| 回调（network.js 触发点） | UI 动作 | 源码 |
| --- | --- | --- |
| onConnected(info) | 切连接页→大厅；重连时保留会话提示 | ui.js:353-362 |
| onDisconnected(info) | 见 T16/T17；重连中保留牌桌并禁用按钮 | ui.js:364-391 |
| onRoomCreated/onRoomJoined(msg) | 见 T5；applyRoomConfigFromServer(msg) + 房间码 + 玩家列表 | ui.js:393-406 |
| onRoomPlayersChanged(msg) | 刷新玩家列表 | ui.js:408-411 |
| onHostChanged(msg) | 见 T6 | ui.js:413-420 |
| onGameStarting(msg) | 见 T7 | ui.js:422-439 |
| onGameState(msg) | renderOnlineGame(msg) | ui.js:441-444 |
| onActionAck(msg) | 重算动作按钮 + syncDesktopAttention() | ui.js:446-451 |
| onRequestError() | 同上 | ui.js:453-458 |
| onPlayerDisconnected(msg) | **原实现未定义 UI 处理**（network.js 有回调位，ui.js 未绑定） | network.js:334-335 |
| onRequestAck(msg) | **未定义 UI 处理**（仅 network.js 内部） | network.js:331-333 |
| onReadyStatus(msg) | 见 T13 | ui.js:460-483 |

### 5.2 连接状态与文案

| 场景 | 文案 | 位置 |
| --- | --- | --- |
| 地址为空 | 请输入服务器地址 | ui.js:349 |
| 地址非法 | 服务器地址必须以 ws:// 或 wss:// 开头 | network.js:83-85 |
| 创建 WebSocket 抛错 | 无法连接: <error.message> | network.js:103 |
| 连接成功 | 已连接到服务器 | ui.js:361 |
| 重连恢复 | 已重新连接，正在恢复原座位… | ui.js:361 |
| 连接错误 | 连接错误，正在检查本地服务… | network.js:145 |
| 进入房间 | ✅ 房间 12（房主时原实现再追加 竖线+房主） | ui.js:405 |
| 房主迁移 | 房主已离开，你现在是房主 / 房主已变更 | ui.js:419 |
| 开局 | 游戏开始！6人 (2真人 + 4AI) | ui.js:438 |
| 断线重连中 | 状态行 连接中断，约 N 秒后自动重连…；动作解释 连接中断，约 N 秒后恢复原座位… | ui.js:373-374 |
| 手动断开 | 已断开连接 | ui.js:390,1351 |
| 离开房间 | 已离开房间 | ui.js:1390 |
| 未连上就创建/加入 | 未连接到服务器 | network.js:197,202 |
| 服务端错误 | msg.message（如「行动未被服务器接受」） | network.js:329,347 |
| 恢复过期 | RESUME_EXPIRED → 清 session、离开房间，提示服务端文案 | network.js:342-346 |

### 5.3 断线 / 重连 / 终止码

- 可自动重连：shouldReconnect && resumeToken && roomCode 且 close code **不在** {1000,1008,1009,1013,4001}（network.js:8-15,124）。
- 重连延迟：min(10000, 650 * 2^min(attempts,4)) + random(0..350) ms（network.js:125-127）；attempts 在收到新 state/进房时清零（network.js:276,312）。
- 会话持久化：sessionStorage['texas-holdem-network-session-v2'] = {resumeToken, roomCode, playerId, serverUrl}（network.js:6,39-68）；页面刷新会自动 resume_session（network.js:110-117）。
- pendingAction：发送行动后记录 {requestId, expectedStateVersion}；收到 action_ack/error 匹配 requestId 清除，或收到更高 stateVersion 自动清除（network.js:233-245,322-328,341）；pendingAction 期间禁用所有行动按钮并显示「等待服务器确认…」（ui.js:1128,1149-1157,1330-1337）。
- 消息去重：stateVersion 单调、handId 不回退（network.js:310-325）；错误房间消息忽略（network.js:263-269）。

### 5.4 房间 / 房主 / 大厅

见 §3.12；额外规则：
- 非房主不显示开始按钮，显示「等待房主开始游戏…」；房主迁移后立即交换（ui.js:402-403,416-417）。
- 离开房间/断开时清空 inRoom/isRoomHost 与联机表现层（ui.js:1373-1391,1340-1352）。
- 建房无独立按钮（见 §3.12 说明）。
- 房间号显示：大号 32px 金色，6px letter-spacing（index.html:195；ui.js:398）。

### 5.5 联机行动

- onlineDoAction(type)：校验 isYourTurn && !pendingAction && legal.actions.includes(type)；fold/check 金额 0，call 用 min(toCall, stack)，allin 用 stack；发送后立刻禁用 5 个按钮并显示等待文案（ui.js:1307-1338）。
- 加注：confirmRaise 用滑块值夹到 [minRaiseTo,maxRaiseTo] 后 sendAction('raise', amount)（ui.js:2441-2450）。
- 服务端 legalActions 字段：actions[], toCall, minRaiseTo, maxRaiseTo（ui.js:1126），其中 wagerUnit 可选。
- 破产：stack<=0 && !isAllIn → 全按钮禁用 + 「你已破产 — 筹码输光，等待新一局或离开」（ui.js:1149-1152）。

### 5.6 结算后下一手（ready 流）

1. 玩家点「下一手 →」→ sendReadyForNext()（network.js:247-253），按钮变「正在开始下一手…」（只有 ≤1 个在线真人时）或「等待其他玩家…」并禁用（ui.js:2579-2587）。
2. ready_status：allReady → 关弹窗、恢复按钮；自己已点但未全 ready → 「等待中 (N 人未准备)…」禁用；自己没点 → 不动（ui.js:460-483）。
3. 新一局 game_state（新 handId）到达后重新渲染牌桌；renderOnlineGame 在 phase!=='idle' 时会主动清掉旧弹窗/按钮状态（ui.js:585-595）。

### 5.7 错误处理原则

- 所有服务端错误只写状态行，不弹独立对话框（showNetworkError）。
- 终端码断开不再重连，回连接页；非终端码自动重连并提示倒计时文案（固定“约 N 秒”，无实时倒计时【未定义】）。
- 重复/过期行动：action_ack 的 accepted=false 用 msg.message 提示；error 带 requestId 时清 pendingAction（network.js:326-349）。

---

## 6. 音频

### 6.1 背景音乐（js/audio.js）

| 项 | 值/行为 | 源码 |
| --- | --- | --- |
| 曲目 | 4 首：assets/music/01.mp3 ~ 04.mp3，name '1'~'4'，gain 1.0 | audio.js:23-28；实测 3.9/3.1/8.2/5.3 MB，共 ~20MB |
| 引擎 | 两个 new Audio() 交替（crossfade），player0 preload=auto，player1 metadata | audio.js:37-55,62-66 |
| 基准音量 | 类默认 0.216；UI 启动/切歌时按滑块设置 | audio.js:14,272-275；ui.js:3049-3052 |
| 音量映射 | volume = clamp(slider/100*0.72, 0, 0.72)（滑块 0-100） | ui.js:3097-3099 |
| 阶段增益 | idle .90 / preflop .92 / flop .97 / turn 1.02 / river 1.06 / showdown 1.04，最终音量 = volume*gain*phaseGain | audio.js:287-291,134-137 |
| 播放模式 | sequential（顺序循环，末尾跨轨）、repeat-one（原生 loop）、shuffle（随机，且不等于当前曲目） | audio.js:123-132,277-285 |
| 切歌（手动） | next() 淡入新曲 1200ms 交叉淡出 | audio.js:257-270 |
| 自动换曲 | 当前曲 ended → 350ms 交叉淡出到下一首 | audio.js:73-78 |
| 错误换曲 | 当前曲 error → 100ms 切下一首，按钮显示 ⚠️ 音乐关 | audio.js:83-89；ui.js:3101-3117 |
| 预加载 | 首曲 ready 后预载下一首（metadata）；当前曲剩余 ≤15s 时改 auto | audio.js:57-60,79-82,108-121 |
| 播放/暂停 | start() 220ms 淡入；stop() 350ms 淡出后 pause | audio.js:209-246 |
| 状态事件 | document 派发 poker-music-state（loading/playing/error/off + track），UI 更新按钮 | audio.js:31-35；ui.js:3119-3122 |
| 懒加载 | 音乐开关只“打开”偏好；首次 startNewHand 才真正 init/start（关闭音乐不预载） | ui.js:3203-3229；README:78 |
| 错误容忍 | Audio 构造/播放失败只 console.warn，不阻塞游戏 | audio.js:52-54,205,220 |

### 6.2 提示音（WebAudio 合成，ui.js:3231-3311）

| 事件 | 合成参数 | 源码 |
| --- | --- | --- |
| 轮到我（出手音） | 两个 sine 振荡器 800Hz + 1200Hz 同时起，gain 0.08 → 0.001 指数衰减，时长 0.25s；每个 osc 结束即断开 | ui.js:3266-3287 |
| 结算（两声） | 660Hz 与 990Hz 依次、间隔 0.18s；gain: 0.001 → 0.10（12ms 起音）→ 0.001（320ms 衰减），每声时长 0.33s；**两声升调** | ui.js:3290-3311 |

触发点：
- 出手音：本地每次“新的决策”（键 numHands:phase:actionsThisRound 去重，ui.js:1500-1506）；联机每次新 turnId（键 roomCode:handId:turnId，ui.js:680-685）。
- 结算音：presentSettlementModal **每显示一次结算结果只播一次**（settlementAlertSerial），已在显示中不重复（ui.js:79-85；测试 tests/ui-network.test.js:369-388）。
- 静音：turnAlertsEnabled=false 时两种音都不播，且不会创建 AudioContext（ui.js:3267,3291）。

### 6.3 AudioContext 生命周期 / 自动播放策略

- 懒创建：首次需要时创建 AudioContext；若 state==='suspended' 则 resume()（ui.js:3235-3247）。
- 首次用户手势（pointerdown/keydown once）主动初始化，保证联机后续提示音不被浏览器 autoplay 策略挂起（ui.js:3261-3264）。
- 卸载时 close() 释放 context（pagehide/beforeunload，ui.js:3250-3260）。
- 关闭/开启提示音：开启时立即 getTurnAudioContext() 预热（ui.js:3154）。

### 6.4 rodio 落地要点（建议，依据 spike §3.6 的 0.23 实测 API）

- rodio 0.23 与旧教程差异大：rodio::DeviceSinkBuilder::open_default_sink() → MixerDeviceSink；rodio::Player::connect_new(sink.mixer())；Decoder::try_from(File 或 Cursor<Vec<u8>>)；source.repeat_infinite() 需要 use rodio::Source；play/pause/stop/clear/get_pos/is_paused。
- 音乐：两个 Player（交叉淡入淡出），symphonia 解 mp3；音量映射保留 slider/100*0.72 与 phase gain 表；切歌用 set_volume 逐帧 ramp（spike 实测 01.mp3 = 44100Hz/2ch/124.15s，WSLg Pulse 实播位置在推进）。
- 提示音：用 rodio::source::SineWave 组合 + amplify/take_duration，或预渲染 4 段短 WAV 用 include_bytes!（更可控；指数包络自己算）；结算音两声按 0.18s 间隔调度。
- 混音：音乐与音效共用输出设备；结算音在音乐播放时需可辨识（原 gain 0.10 vs 音乐 ≤0.72×1.06，需实测）。
- 失败容忍：设备不可用/解码失败只记录，不 panic；UI 按钮进入 ⚠️ 音乐关。
- WSL 无声卡属开发环境现象（spike §3.6）；Windows WASAPI 共享/独占、蓝牙切换、系统采样率差异都可能导致“第一声不出”，需 Windows 实测（spike §5.5）。

---

## 7. 桌面提醒（任务栏/托盘）

### 7.1 触发条件（syncDesktopAttention，ui.js:58-77）

- **轮到我**（turn）：
  - 本地：game.phase ∉ {idle, showdown} 且 isPlayerTurn() 且（本轮未行动 或 本轮下注 < currentBet）；窗口不在前台/最小化时才真正闪烁。
  - 联机：network.gameState.isYourTurn && !pendingAction && network.connected。
  - messageKey：联机 online:<roomCode>:<handId>:<turnId>；本地 local:<tableSerial>:<numHands>:<phase>:<actionsThisRound>。
- **结算**（settlement）：结算弹窗可见且「下一手」按钮存在且未禁用；messageKey = settlement:<settlementAlertSerial>。
- 优先级：settlement 覆盖 turn（ui.js:75）。
- 调用点：onUpdate/syncDesktopAttention（本地每帧更新）、renderOnlineGame、onActionAck、onRequestError、onReadyStatus、弹窗打开/关闭、goToStartScreen、startNewGame、断开连接（ui.js:76,84,450,456,482,686,1337,2450,2588,2608,2659,296-297,365）。

### 7.2 停止条件（desktop-attention.js）

- 窗口获得焦点且未最小化 → acknowledge()：seen=true 并 stop()。
- 窗口失焦/最小化 → sync() 重新判断。
- pending=false 或 seen=true → stop()。
- stop()：清除定时器、flashFrame(false)、托盘图标还原、tooltip 还原「德州扑克」。
- **已读语义**：回到前台即停止本次闪红，且该 messageKey 不会重复提醒；下一次新 key 才会重新提醒（README:95；desktop-attention.js:38-51）。
- 新 key 判定：setPending(true, reason, messageKey) 时若 messageKey 与上次不同 → seen=false（desktop-attention.js:41）。

### 7.3 托盘 / 任务栏行为

| 项 | 行为 | 源码 |
| --- | --- | --- |
| 托盘图标 | icon.ico（resize 32×32）；提醒时 assets/attention-icon.png | desktop-attention.js:15-18 |
| 托盘闪烁 | 提醒中每 650ms 在 normalIcon/alertIcon 间切换（setInterval） | desktop-attention.js:64-72 |
| 任务栏闪烁 | win.flashFrame(true) / flashFrame(false)（Windows 任务栏；Linux 视桌面环境） | desktop-attention.js:65,78 |
| tooltip | 正常「德州扑克」；提醒 turn「德州扑克：轮到你行动」；结算「德州扑克：本局已结束，请确认下一手」 | desktop-attention.js:18,63,80 |
| 托盘菜单 | 「打开德州扑克」（还原+显示+聚焦）、分隔线、「退出」 | desktop-attention.js:26-30 |
| 托盘单击 | 打开窗口 | desktop-attention.js:25 |
| 销毁 | 关窗时 destroy()（停 timer、移除 listener、销毁 tray） | main.js:135-140；desktop-attention.js:83-90 |

### 7.4 egui 落地（依据 spike §3.8/§8.6）

- 任务栏闪烁**不需要 windows crate**：用 egui 0.37 的 ViewportCommand::RequestUserAttention(UserAttentionType::Critical / Informational / Reset)；egui-winit 会转成 winit::Window::request_user_attention，Windows 实现为 FlashWindowEx（Critical → FLASHW_ALL|FLASHW_TIMERNOFG；Reset → FLASHW_STOP），正好满足“一直闪直到前台/可提前停止”。这样不引入 unsafe，符合 workspace forbid(unsafe_code)。
- 托盘：tray-icon 0.26 + single-instance 0.3 只在 cfg(windows) 编译；托盘闪红用 PlatformShell::set_alert_icon(bool) 切换红色提亮图标（spike 建议 500ms 交替；**原实现是 650ms，以 650ms 为准，可配置**）；托盘菜单事件每帧 MenuEvent::receiver().try_recv() 轮询。
- 650ms 交替用 ctx.request_repaint_after(Duration::from_millis(650)) + 状态机，不要在 UI 线程 sleep。
- 焦点判定：ctx.input(|i| i.viewport().focused) / minimized；或监听 winit 事件。
- messageKey 去重必须保留（语义：同一提示回前台后不重复闪）。
- ⚠️ WSLg 无任务栏/托盘，托盘/闪烁/无边框手感必须在 Windows 实测（spike §5.3/§5.4）。

---

## 8. 文案清单（可直接当 egui 常量表）

> 约定：<...> 为运行时插值；[cond] 为显示条件。源码列给定位。
> 原实现大量字符串直接写在 JS/HTML 里；Rust 建议集中在 ui/strings.rs（或 i18n/zh_cn.rs）按页面分组。
> 注意：emoji/符号（♠♥♦♣ ⛶ ⠿ 🃏 🎵 🔔 🔕 ⏭️ 🔁 🔂 🔀 🔊 🔄 🏠 🏆 💔 📌 🤔 ⚡ 💰 ❌ 💵）在 egui 中依赖 emoji 字体回退，若不可用需替换为绘制图标/纯文字（§9.5）。

### 8.1 顶栏 / 窗口

| 位置 | 文案 | 条件 | 源码 |
| --- | --- | --- | --- |
| 标题 | ♠ Texas Hold'em ♥ / ♠ Short Deck Hold'em ♥ | 模式 | index.html:41；ui.js:2703-2706 |
| 版本徽章 | v1.5.8 | 静态 | index.html:41 |
| 手牌计数 | 手牌 #<N> | 静态前缀 | index.html:43 |
| 盲注 | SB: <x> / BB: <y>；短牌 Ante: <x> / 最小下注: <y> | 标准/短牌 | ui.js:182-186 |
| 音乐按钮 | 🎵 音乐关 / 🎵 音乐开 / ⚠️ 音乐关 | 播放状态 | ui.js:3108 |
| 音乐 title | 播放背景音乐 / 正在播放：<曲名> / 正在准备：<曲名> / 音乐加载失败；点击重试 | 状态 | ui.js:3110-3112 |
| 提示音按钮 | 🔔 提示音开 / 🔕 提示音关 | 开关 | ui.js:3144 |
| 提示音 title | 关闭出手和结算提示音 / 开启出手和结算提示音 | 开关 | ui.js:3145 |
| 下一首 | ⏭️（title 下一首） | — | index.html:51 |
| 播放模式 | 🔁 顺序 / 🔂 单曲 / 🔀 随机 | 当前模式 | ui.js:3168-3172 |
| 播放模式 title | 当前：顺序播放；点击切换 / 当前：单曲循环；点击切换 / 当前：随机播放；点击切换 | 当前模式 | ui.js:3169-3175 |
| 刷新 | 🔄（title 重新连接/刷新牌局） | — | index.html:62 |
| 主页 | 🏠（title 返回主页） | — | index.html:63 |
| 全屏 | ⛶ 全屏 / ⛶ 退出全屏 | 全屏态 | ui.js:2869-2888 |
| 窗口按钮 | —（最小化）/ □ 或 ❐（最大化/还原）/ ×（关闭） | — | index.html:66-68；ui.js:2904 |
| 悬停区 | aria 显示顶部工具栏，title 显示顶部工具栏 | — | index.html:37 |

### 8.2 状态行

| 文案 | 条件 | 源码 |
| --- | --- | --- |
| 点击「开始游戏」开始德州扑克对局！ / 点击「开始游戏」开始短牌对局！ | 主页 | index.html:75；ui.js:1474-1476,2648-2650 |
| 点击 开始游戏 开始！ | idle 且未打过 | ui.js:2289 |
| 手牌结束！点击 下一手 继续 | idle 且 numHands>0 | ui.js:2286-2287 |
| 📌 轮到你了 — <阶段> · <N> 人在局 | 本地/联机轮到我 | ui.js:2301-2302,1202-1203 |
| 🤔 <名字> 思考中… <阶段> · <N> 人在局 | 本地 AI/联机他人 | ui.js:2303-2304,1209 |
| <阶段> · <N> 人在局 · <M> 人弃牌 | 本地兜底 | ui.js:2306 |
| ⏳ 正在结算… 2 秒 | 本地摊牌后 | ui.js:1514 |
| ⏳ 等待新一局… | 联机 idle | ui.js:1204-1205 |
| 🔄 连接中断，约 <N> 秒后自动重连… | 断线重连 | ui.js:374 |
| 🔌 已断开连接 | 断开 | ui.js:390,1351 |
| ✅ 已连接到服务器 / 🔄 已重新连接，正在恢复原座位… | 连接回调 | ui.js:361 |
| ✅ 房间 <code>（房主时原实现再追加 竖线+房主） | 进房 | ui.js:405 |
| 👑 房主已离开，你现在是房主 / 房主已变更 | 房主迁移 | ui.js:419 |
| 🎮 游戏开始！<N>人 (<H>真人 + <A>AI) | 开局 | ui.js:438 |
| 已离开房间 | 离开 | ui.js:1390 |
| 服务端错误消息 / 行动未被服务器接受 / 服务器请求失败 | 错误 | network.js:329,347 |
| 请输入服务器地址 / 请选择房间号 / 服务器地址必须以 ws:// 或 wss:// 开头 / 无法连接: <msg> / 连接错误，正在检查本地服务… / 未连接到服务器 | 连接错误 | ui.js:349；network.js:84-85,103,145,197,202 |

### 8.3 牌桌 / 座位

| 文案 | 条件 | 源码 |
| --- | --- | --- |
| 公共牌占位 翻牌×3 转牌 河牌 | 未发牌 | index.html:91-95；ui.js:728 |
| 底池: $<N> | 常驻 | ui.js:622,2311 |
| 轮次标签 翻牌前 / 翻牌 ♠ / 转牌 ♥ / 河牌 ♦（本地）/ 翻牌前/翻牌/转牌/河牌（联机） | 非 idle | ui.js:2314-2322,1194-1196 |
| AI 默认头像 🤖，真人 👤（联机） | 无 avatar 字段 | ui.js:1021,1864 |
| 本地真人头像 😤、名字 You | 本地固定 | js/game.js:77；js/shortdeck.js:553 |
| 本地 AI 名字 Alex Blake Casey Drew Emma Finn Grace Hayes Ivy Jade；头像 😎 🤠 🕶️ 🎩 👑 🦊 🐺 🦅 🐯 🐉 | 顺序分配 | js/ai.js:55-56；shortdeck.js:445-447 |
| 筹码 $<N> / ALL-IN / 💰 已破产 | 状态 | ui.js:1024-1033,1866,648-655 |
| 下注 $<N> | chipsInPot>0 | ui.js:1036-1041,1898-1903 |
| 最近动作 弃牌/过牌/跟注/加注/全下/小盲/大盲，带金额 加注 $<N> | 本地 | ui.js:1908-1916 |
| 思考 … | thinking | ui.js:1102,1973 |
| 位置徽章 D/SB/BB，title 庄家/小盲/大盲 | 对应位置 | ui.js:1068-1082,1778-1786 |
| 浮动气泡 <名字>: <动作>[ $<金额>] | AI/人类动作 | ui.js:1563,704 |
| 策略小字 | 静态 | index.html:78 |

### 8.4 人类区域 / 动作解释

| 文案 | 条件 | 源码 |
| --- | --- | --- |
| 我的手牌 / 我的手牌 · 点击隐藏 / 我的手牌 · 已隐藏，点击查看 | 无牌/正面/背面 | ui.js:1672-1673 |
| aria 隐藏我的手牌 / 显示我的手牌；title 点击将手牌翻到背面 / 点击将手牌翻回正面 | 正/背面 | ui.js:1670-1671 |
| 按钮 弃牌 过牌 跟注 加注 全下 | 静态 | index.html:270-274 |
| 过牌 ✓ / 跟注 $<N> / 全下 $<N> / 加注 | 动态 | ui.js:1176-1179,2085-2112 |
| 本局下注 / 筹码 | 静态 | index.html:264-267 |
| 本地非回合：⏳ 等待新一局开始… / ❌ 你已弃牌，等待本局结束 / 🃏 你已全下，等待摊牌 / 🤔 <名字> 正在思考… / ⏳ 请等待其他玩家行动 | 状态 | ui.js:2066-2076 |
| 本地回合：💰 当前注额 $<bet>，你需要跟注 $<toCall> 才能继续（被后续覆盖，实际不显示） | — | ui.js:2083 |
| ✅ 无人加注，你可以过牌或加注 | 我回合、可过牌 | ui.js:2120 |
| ⚡ 对方下注 $<N>，你只能全下或弃牌 | toCall ≥ stack | ui.js:2123 |
| 💰 需要跟注 $<N> 或选择加注/弃牌 | 有 toCall | ui.js:2125 |
| 🔄 你在大盲位，可以选择过牌或加注 | BB option | ui.js:2131 |
| 联机：💰 你已破产 — 筹码输光，等待新一局或离开 | stack≤0 | ui.js:1150 |
| ⏳ 等待服务器确认… | pendingAction | ui.js:1156,1336 |
| ⏳ 等待新一局开始… / ❌ 你已弃牌，等待本局结束 / 🃏 你已全下，等待摊牌 / ⏳ 等待其他玩家行动… | 非回合 | ui.js:1159-1167 |
| 💰 需要跟注 $<N>，也可加注或弃牌 / 💰 需要跟注 $<N>，当前加注权未开放 | toCall>0 | ui.js:1181-1184 |
| ✅ 你可以过牌或下注 / ✅ 你可以过牌 | toCall=0 | ui.js:1185-1189 |
| ⏳ 等待服务器确认…（发加注后） | 联机 | ui.js:2449 |

### 8.5 加注面板

| 文案 | 源码 |
| --- | --- |
| 加注 $<N>（金额显示） | ui.js:2431 |
| ✅ 加注 / 取消 | index.html:283-284 |
| slider title 数字 1 / ← / ↓ 减少，数字 2 / → / ↑ 增加 | index.html:278 |
| slider aria-label 加注到的总金额；aria-valuetext = 加注 $N，数字键 1 减少，2 增加，每次 <unit> | index.html:279；ui.js:2432 |
| 面板 aria-label 选择加注额度 | index.html:277 |

### 8.6 分析面板

| 文案 | 条件 | 源码 |
| --- | --- | --- |
| 📊 胜率分析 + ▼/▲ | 常驻 | index.html:292；ui.js:3086 |
| 胜率 平局 输率 | 条标签 | index.html:297-316 |
| 计算中 / 0% / <x.x>% | 数据状态 | ui.js:2149-2162 |
| 🃏 当前手牌 / 可击败的牌型 | 静态 | index.html:319-322 |
| — | 弃牌/idle | ui.js:2168 |
| <N> 张牌 (还需 <M> 张公共牌) | <5 张 | ui.js:2175-2176 |
| 🎯 改善牌分析 | 静态 | index.html:324 |
| 正在计算改善牌… / 暂无直接补牌 | outs 状态 | ui.js:2217,2232 |
| <牌型>: <N> 张 · 强提升 <S> / · 踢脚改善 <T> | 分组 | ui.js:2245-2248 |
| +<N> 张 | 超出 8 张 | ui.js:2267 |
| 🔒 手牌已隐藏，分析数据同步隐藏 | 手牌隐藏+面板展开 | index.html:329 |
| 牌型名（标准，低→高）高牌 一对 两对 三条 顺子 同花 葫芦 四条 同花顺 皇家同花顺 | 10 项 | js/cards.js:81-92 |
| 牌型名（短牌，低→高）高牌 一对 两对 三条 顺子 葫芦 同花 四条 同花顺 皇家同花顺 | 10 项（葫芦/同花互换） | js/shortdeck.js:40-51 |

### 8.7 结算弹窗

| 文案 | 条件 | 源码 |
| --- | --- | --- |
| ⠿ 拖拽移动 | 常驻 | index.html:337 |
| 🏆 摊牌 | 本地摊牌 | ui.js:2470 |
| 🏆 摊牌 / 🏆 胜出 | 联机摊牌/弃牌胜 | ui.js:1222 |
| <赢家> 赢得 + $<金额> + 牌型: <牌型> | 常驻 | index.html:339-343 |
| 🃏 公共牌 | 有公共牌 | ui.js:1248,2512 |
| 下一手 → | 常驻 | index.html:345 |
| 全部弃牌 | 弃牌胜且非摊牌 | ui.js:1232 |
| 💔 你已输光所有筹码！ | 本地摊牌且破产 | ui.js:1522 |
| ⏳ 等待中 (<N> 人未准备)… | 联机自己已点 | ui.js:2476 |
| ⏳ 正在开始下一手… / ⏳ 等待其他玩家… | 联机点击后 | ui.js:2583 |
| 🎉 你赢了！ + <赢家> 赢得 $<pot> (全部弃牌) | 本地弃牌胜 | ui.js:1530 |
| 💔 你输了 + <赢家> 赢得 $<pot> — 你已输光！ | 本地弃牌胜且破产 | ui.js:1528 |
| 🤖 <AI 名字> + 赢得 $<pot> | 本地 AI 弃牌胜 | ui.js:1532,2841 |

### 8.8 开始界面

| 文案 | 条件 | 源码 |
| --- | --- | --- |
| Texas Hold'em / 6+ Short Deck | 模式 | ui.js:1470 |
| 36张牌的快节奏短牌对战 — 同花大于葫芦，顺子大于三条 | 短牌副标题 | ui.js:1472 |
| 选择人数 / 游戏模式 / 游玩方式 | 分组标签 | index.html:109,121,128 |
| <N> 人 | 人数按钮 | ui.js:1437 |
| ♠ 标准德州 / ♦ 短牌 6+ | 模式按钮 | index.html:123-124 |
| 🎮 本地游戏 / 🌐 联网游戏 | 玩法按钮 | index.html:130-131 |
| 起始筹码 / 大小盲注（短牌 前注 / 最小下注）/ AI 行动节奏 | 设置标签 | index.html:138,149,160；ui.js:245 |
| 5千 1万 2万 5万 10万 / 10/20 20/40 40/80 50/100 100/200 200/400 | 刻度 | index.html:144,155 |
| 快速 标准 沉浸 | AI 节奏档 | ui.js:46-48 |
| $<N> / $<sb> / $<bb> | 设置输出 | ui.js:238-239 |
| <depth> BB 起始深度 / <depth> 个最小下注的起始深度 | 摘要 | ui.js:248-250 |
| 🏆 开始游戏 | 本地入口 | index.html:174 |
| 💔 你已输光所有筹码！重新开始吧 | busted | index.html:244 |

### 8.9 联机面板

| 文案 | 源码 |
| --- | --- |
| placeholder 服务器地址、输入你的昵称；🔗 连接 | index.html:182-186 |
| 本机服务会随应用自动启动 · 默认地址 ws://localhost:3000 / 本机服务会随应用自动启动 · 当前地址 <url> | index.html:188；ui.js:162 |
| 📋 房间号、——、把房间号发给朋友加入 | index.html:194-196 |
| 👥 玩家列表 | index.html:200 |
| 人数:、短牌（checkbox）、房间号:、🚪 进入房间 | index.html:207-227 |
| 🏆 开始游戏、等待房主开始游戏…、🚪 离开房间、🔌 断开 | index.html:233-239 |
| 玩家行 🟢/🔴 + 👤/🤖 + <名字>（自己加 " (你)"） | ui.js:1404-1410 |
| <N>人（下拉选项） | ui.js:1461 |

### 8.10 规则说明

| 文案 | 源码 |
| --- | --- |
| 📜 查看德州扑克规则 / 📜 收起规则说明 | ui.js:3077-3079 |
| ♠ 德州扑克 规则说明 ♥、🎯 游戏目标、🏷️ 位置与盲注、🔄 游戏流程、🎮 行动选项、🃏 牌型大小 (从大到小)、♻️ 下注轮次与行动顺序、🤖 AI 风格、♦ 短牌 (6+ Hold'em) 规则说明 ♥、🎴 牌数与牌堆、🏆 牌型大小（与标准德州的最大区别）、🔄 A的特殊用法、💰 前注制代替盲注、📊 策略提示 | index.html:374-483 |
| 牌型行：皇家同花顺/同花顺/四条/葫芦/同花/顺子/三条/两对/一对/高牌 + 描述 + 例子 | index.html:406-418,449-461 |
| 短牌动态值：[data-config-value] = $40 $80 $40 $80 $80（随设置） | index.html:382-383,469-471；ui.js:255-265 |
| 正文另有：行动快捷键 F/C/R/A、AI 风格 6 条、短牌策略 4 条等（逐字照抄 index.html:396-403,430-439,476-482） | 同 |

### 8.11 调试 / Toast / 提示行

| 文案 | 源码 |
| --- | --- |
| 🐞（title 打开调试面板）、🚨 运行时日志、tabs all error network game ai、拖动标题栏移动 · 点标签过滤、📋 复制日志、✅ 日志已复制、该分类暂无日志、✕（title 关闭面板）、⚠️ 页面可能卡死 — 停滞 <N>s | debug.js:21-111,224 |
| 返回主页，当前对局已关闭、已离开房间、🔄 正在重新连接…、⚠️ 请先输入服务器地址、🔄 重新发牌 | ui.js:2924,2941,2952,2957,2961 |
| 键盘提示行（§3.16） | ui.js:3029 |

### 8.12 数值格式规则

- 所有筹码金额：$ + max(0, floor(n)).toLocaleString()（千分位，系统 locale；中文 Windows 为 20,000）。**唯一实现入口 formatChips**（ui.js:173-175；测试 tests/ui-network.test.js:542-558）。Rust 用固定逗号千分位（不要依赖 locale）。
- 概率：(x*100).toFixed(1) + '%'（一位小数）。
- 深度摘要：Math.round(x*10)/10（一位小数，如 250、12.5）。
- 计时/延时文字：秒数向上取整（Math.max(1, Math.ceil(ms/1000))，ui.js:368）。
- 名字：输入框 maxlength=8（按 UTF-16 code unit 截断），保存时 slice(0,8)，空则默认 玩家（index.html:184；ui.js:130,143,348,1356）——egui 应按 chars().count() <= 8 处理（与服务端 NFKC 后 ≤8 字符对齐，见交接 §6.4）。
- 牌面 token：rank 字符串 2..10/J/Q/K/A，suit spades/hearts/diamonds/clubs，显示符号 ♠♥♦♣（ui.js:758,1252,1642）。

---

## 9. Rust / egui 实现建议

> 本节是工程建议，**不是 oracle**；冲突时以 §1–§8 的原实现行为为准。
> egui 0.37 的 API 结论来自 task-3 spike（docs/spikes/egui-spike.md，已在 WSLg 实测 + Windows target check）；Windows 行为仍需 spike §5 实测。

### 9.1 模块与状态

~~~text
crates/poker-app/src/
  main.rs        eframe 入口；windows_subsystem = "windows"（release 无控制台）
  lib.rs         run()、窗口图标、NativeOptions、headless/screenshot 模式
  cli.rs         --screenshot/--frameless/--audio-test 等（Phase 0 spike 已有）
  app.rs         eframe::App 实现（0.37：fn ui(&mut self, ui: &mut egui::Ui, frame)）
  state.rs       AppState / Screen / UiState / Session
  layout.rs      seat_positions / seat_min_table_height / 桌面常量
  theme.rs       颜色/字号/边距/圆角常量（§2.8 的 Rust 版）
  strings.rs     中文文案常量（§8）
  widgets/       seat.rs, human_area.rs, raise_panel.rs, prob_panel.rs,
                 showdown_modal.rs, start_screen.rs, online_panel.rs,
                 header.rs, rules_panel.rs, debug_panel.rs, toast.rs
  anim.rs        发牌/翻牌/浮字/思考点/提醒闪烁的 tween 状态
  audio.rs       rodio 0.23 音乐 + 提示音（复用 spike src/audio.rs）
  attention.rs   注意力状态机（turn/settlement key 去重）
  fonts.rs       system-fallback + 内置子集（复用 spike src/fonts.rs）
  shell.rs       无边框标题栏/拖动/双击最大化/8 方向 BeginResize（复用 spike src/shell.rs）
  platform/      windows.rs（tray-icon/single-instance）、stub.rs（非 Windows 空实现）
~~~

建议的显式状态（替代原实现的 DOM 隐式状态）：

~~~rust
pub enum Screen { Start, LocalTable, OnlineConnect, OnlineLobby, OnlineTable }

pub struct AppState {
    pub screen: Screen,
    pub settings: Settings,          // 对应 localStorage 字段（§3.11）
    pub local: Option<LocalSession>, // 本地引擎实例 + 视图快照
    pub online: Option<OnlineSession>,
    pub ui: UiState,
}

pub struct UiState {
    pub raise_panel: Option<RaisePanelState>,   // min/max/unit/value/default
    pub showdown: Option<ShowdownModalState>,
    pub prob_open: bool,
    pub cards_hidden: bool,
    pub header_detached: bool,
    pub rules_open: bool,
    pub debug_open: bool,
    pub toast: Option<(String, f64)>,           // (text, deadline)
    pub floating: Vec<FloatingAction>,          // 3s 生命期
    pub focus: FocusRing,                       // §4.4 自建 Tab 顺序
    pub animations: AnimState,
}
~~~

- **不要**用 DOM 可见性这种隐式状态；把 §1.4 的矩阵变成 match screen + 少量 overlay 布尔。
- 本地牌局由 poker-engine 驱动；poker-app 只持有视图快照 + 回调事件（AI 决策放到后台线程/任务，避免阻塞 UI 帧）。
- 所有“每帧重建”的风险在 egui 中天然规避（立即模式），但**动画状态必须显式持久化**（§9.4），否则发牌/浮字每帧重启。

### 9.2 布局实现

~~~rust
pub const WINDOW_DEFAULT: [f32; 2] = [1280.0, 860.0];
pub const WINDOW_MIN: [f32; 2] = [900.0, 600.0];
pub const CONTENT_MAX_W: f32 = 1200.0;
pub const TABLE_MIN_H: f32 = 520.0;
pub const TABLE_MIN_H_COMPACT: f32 = 480.0;
pub const SEAT_SIZE: [f32; 2] = [104.0, 210.0];
pub const SEAT_SIZE_COMPACT: [f32; 2] = [104.0, 180.0];
pub const SEAT_INSET: f32 = 30.0;
pub const SEAT_ARC_DROP: f32 = 42.0;
pub const SEAT_ARC_DROP_COMPACT: f32 = 16.0;
pub const SEAT_MIN_SLOT: f32 = 130.0;
pub const COMMUNITY_TOP_RATIO: f32 = 0.58;
pub const COMMUNITY_TOP_RATIO_COMPACT: f32 = 0.56;
pub const POT_OFFSET_Y: f32 = 70.0;

/// 1:1 移植 ui.js:1996-2040；返回座位中心点（相对座位容器左上角）。
pub fn seat_positions(ai_count: usize, cw: f32, ch: f32, seat_w: f32, seat_h: f32) -> Vec<egui::Pos2>;

/// 1:1 移植 ui.js:912-940：计算 --seat-min-table-height 等价物。
pub fn seat_min_table_height(seat_count: usize, seat_h: f32, container_w: f32, table_border: f32) -> f32;
~~~

- 坐标用**逻辑点**（egui 默认，spike §3.3：不要自己乘 DPI）；Windows 125%/150% 由 egui 处理。
- 座位绘制：Area 或 ui.put(Rect::from_center_size(container_rect.min + pos, seat_size), seat_widget)；容器 rect = table_rect.shrink(30 + border)。
- 每帧只重算一次 layout（缓存 cw/ch，与原 getTableSize 等价）；seats 数量变化/窗口 resize 时才重算 seat_min_table_height。
- 页面整体：ScrollArea::vertical().show(ui, ...) 包住 header + table；底部面板建议固定（原实现也在滚动区内，属于可接受的偏差，见 §9.11）。
- eframe 0.37 的 App 入口：CentralPanel::default().show(ui, |ui| ...)；窗口尺寸用 ctx.content_rect()（不再是 screen_rect）；每帧 ctx.request_repaint_after(16ms) 保证动画（spike §8.2/§9）。

### 9.3 渲染顺序（对应 z-index）

~~~text
1. 背景（雾蓝几何 + 20 个花色装饰，静态层缓存成纹理）
2. #app 内容（ScrollArea）
   2.1 header（或 detached 浮层，最后画）
   2.2 game_status / strategy_note
   2.3 table：felt → decor → betting_label → seats → community → pot → start_screen(overlay)
   2.4 bottom_panel：human_area（含 raise panel）→ prob_panel
   2.5 hints
   2.6 rules_container
3. showdown modal（全屏遮罩 + 居中/可拖动窗口）
4. debug panel + FAB
5. toast / floating actions（最上层）
~~~

原 z-index 参考：座位 5、公共牌/底池 3、主页 10、悬停条 101、头部 100/102、弹窗 1000、调试 99998/99999、toast 100000（style.css、debug.js:21,109）。

### 9.4 动画实现表

| 原动效 | 原时长 | egui 建议 |
| --- | --- | --- |
| 公共牌发牌 | 0.42s/张，delay 150ms/张 | AnimState.card_deal_started: [Option<f64>;5]；用 ctx.animate_value_with_time 或自算 t=(now-start)/0.42，插值 translate/scale/rotate |
| 公共牌翻面 | 0.52s，新牌 n*150+330+i*180 ms 后开始 | card_flip_started[5]；rotateY 用横向 scaleX 模拟（1-2t） |
| 自己的手牌发牌 | 260ms 后翻，420ms | hand_deal_started；中点切换牌面 |
| 手牌点击翻面 | 360ms，中点 175ms | hand_flip_started + 中点切换 cards_hidden |
| 动作标签 pop | 0.6s + 800ms 移除 | last_action_key/time；只在 key 变化时重置 |
| 浮动气泡 | 3s 上浮淡出 | Vec<FloatingAction{text,pos,color,start}>，超时 remove |
| 思考点 | 1s 无限 pulse | ctx.request_repaint_after 或 ctx.animate_bool；时间基准 ctx.input 读取 i.time |
| 全下按钮 pulse | 1.5s | 同上 |
| 概率条 | width 0.5s | animate_value_with_time 目标值 |
| 分析面板折叠 | 0.3s max-height | CollapsingHeader::open 自带动画即可 |
| 弹窗出现 | fadeIn/slideUp 0.3s | 自绘遮罩 alpha + 内容 offset |
| 头部悬停 | 0.15s | animate_bool_with_time 控制 translateY/alpha |
| 背景装饰 | 2s 淡入 | 首帧后一次性动画；或直接静态（性能优先） |

**关键**：所有动画状态用 f64 起始时间 + request_repaint()；对 reduced-motion 等价需求（原 CSS media query）加设置项或跳过动画。

### 9.5 字体与主题

spike 实测（§3.3，详见 spike 文档）：
- eframe 0.37 默认特性含 system_fonts（fontique），NativeOptions.system_font_fallback 默认 true；Windows 走 DirectWrite、Linux 走 fontconfig，**不写字体加载代码中文也能显示**（WSL 截图验证）；缺点是不同机器字形/字体可能不一致，且首次 CJK 字形查找可能等系统字体枚举（几百 ms）。
- 推荐发布版内置 **GB2312 子集**（实测 3,183,992 B：ASCII + 6763 汉字 + 全角标点 + 常用符号/花色），保留 system_font_fallback=true 作罕见字兜底；不要内置 19.5MB 的 Noto CJK TTC。
- 字体加载 API（0.37）：egui::FontData::from_blob(Arc<[u8]>, face_index)（TTC 必须带 face index；from_owned 只有 face 0）；ctx.add_font(egui::FontInsert::new("poker-cjk", data, vec![InsertFontFamily{ family: Proportional, priority: FontPriority::Highest }, ...]))。
- **emoji 风险**：原 UI 大量使用 emoji（头像 😤😎🕶️👑🦊🐺🐦、按钮 ⛶ ⠿ 🔄 🏠 🎵 🔔 🏆 💔 等）。egui 默认字体不含彩色 emoji，需要 emoji 字体回退（体积大、图集问题多）。建议：**头像改为字母/绘制图标或 32×32 头像纹理**，按钮符号优先用能覆盖的符号字体/自绘图标；§8 的文案表已标出这些符号，便于集中替换。
- 主题：§2.8 的 token 表直接转 theme.rs 常量；egui Style 里设置 text_styles（28/24/22/18/16/14/13/12/11/10/9）与 spacing；牌桌椭圆、渐变、阴影用 epaint Shape 自绘或预渲染纹理（渐变牌桌建议一次生成纹理，避免每帧绘制渐变）。
- DPI：全部逻辑点；若用户觉得字体过大/过小，Phase 6 可加 zoom factor 设置（ctx.set_zoom_factor），不要硬编码像素（spike §3.3）。

### 9.6 键盘与焦点

~~~rust
pub struct FocusRing { order: Vec<egui::Id>, current: Option<usize> }

// 每帧开头（在绘制控件之前）：
// 1) ctx.wants_keyboard_input() 为真 → 只处理输入框自身按键
// 2) 加注面板打开 → 走面板分支（1/2/方向/Space/Enter/Esc），吞掉其余
// 3) 结算弹窗打开 → Tab 聚焦按钮；Enter/Space/N → 下一手；吞掉其余
// 4) 手牌区/分析标题聚焦 → Enter/Space 切换
// 5) 全屏中 Esc → 退出全屏
// 6) 轮到我 → F/C/R/A（检查按钮可用性；repeat 忽略）
// 7) 否则 Tab/Shift+Tab 在 FocusRing 内循环（按钮/滑块/输入框）
~~~

- 用 egui::Event::Key { pressed, repeat, .. } 读取；repeat=true 的 Enter/Space 必须忽略（原实现 event.repeat 直接 return，controls.js:73,92,129）。
- 输入框例外：TextEdit 获得焦点时 ctx.wants_keyboard_input() 为真，不要抢 F/C/R/A。
- Space 对 Button 的默认激活要关闭（consume_key 后执行自定义分支）。
- Id 注意：egui 0.37 的 Id::new 已弃用，交互 ID 用 ui.scope_id().with(...) 或 Id::unique（spike §9）。
- 中文 IME：TextEdit 在 egui 0.37 已完整接入（spike §3.4：IMEOutput → egui-winit → winit WM_IME_*，应用层零代码）；仍需 Windows 实测候选窗/组合串/全角标点。
- 无障碍（DECISIONS #5：键盘可达性必须，读屏尽力）：egui AccessKit 支持有限；焦点环/顺序自己做，别指望浏览器语义。

### 9.7 音频

见 §6.4（rodio 0.23 API 以 spike §3.6/§8.5 为准）。补充：
- 4 首 mp3 约 20MB：spike 用 include_bytes! 内嵌 4MB 单曲验证可行；发布版单文件 vs 外置 assets 需 Lead/用户拍板（spike §4 #6）。
- 音乐状态机建议移植为 MusicPlayer { state: Off/Loading/Playing/Error, mode, volume, phase_gain, current_track, transition: Option<Tween> }。
- 提示音用 Player/Sink 短暂播放；结算音两声按 0.18s 间隔调度（Vec<(time, sink)> 或预混 WAV）。
- 设备切换/蓝牙/WASAPI 共享模式需 Windows 实测（spike §5.5）。

### 9.8 窗口壳 / 托盘（Windows）

依据 spike §3.7/§3.8/§8.1-§8.7：
- NativeOptions：ViewportBuilder::default().with_title(...).with_inner_size([1280.0,860.0]).with_min_inner_size([900.0,600.0]).with_decorations(!frameless).with_resizable(true).with_icon(icon)；centered: true；system_font_fallback: true。
- 无边框拖动：拖动区 Response::drag_started() → ctx.send_viewport_cmd(ViewportCommand::StartDrag)；双击 → ViewportCommand::Maximized(!maximized)（最大化状态读 ctx.input(|i| i.viewport().maximized)）。
- 边缘缩放：鼠标进入 5px 边缘条 → 设置 cursor → drag_started 时 ViewportCommand::BeginResize(ResizeDirection)（8 方向；矩形用 ctx.content_rect() 计算；Wayland 不支持，Windows/X11 支持）。
- 最小化/关闭：ViewportCommand::Minimized(true) / ViewportCommand::Close；全屏：ViewportCommand::Fullscreen(bool)（替代 DOM Fullscreen API，Esc 行为自己处理）。
- 运行时切换系统边框：ViewportCommand::Decorations(bool)（spike 已验证），可做成设置项。
- 任务栏闪烁：ViewportCommand::RequestUserAttention(UserAttentionType::Critical) / Reset；**不需要 windows crate / 不引入 unsafe**（winit 内部即 FlashWindowEx，spike §3.8/§8.6）。
- 托盘：tray-icon 0.26，仅 cfg(windows)；TrayIconBuilder + 菜单「显示主窗口/退出」；每帧轮询 MenuEvent::receiver().try_recv()；托盘闪红用 set_alert_icon(bool)（原实现 650ms 交替）。
- 单实例：single-instance 0.3，仅 cfg(windows)；spike 当前实现第二个实例直接退出，**不能唤起已有窗口**（正式版建议加命名管道/WM_COPYDATA IPC，spike §3.8 已知限制）。
- 图标：原实现只有 icon.ico + 32×32 attention-icon.png；正式版需 16/24/32/48/256 多尺寸 .ico（spike §4 #5）。
- 关闭/Alt+F4/托盘退出三条路径都要清理：停音频、移除托盘、释放单实例锁（spike §3.7）。
- 窗口位置/大小持久化【原实现未定义】；可加可不加。

### 9.9 与其它 crate 的接口边界

| UI 需要 | 来自 | 说明 |
| --- | --- | --- |
| 本地牌局状态机、合法动作、摊牌结果 | poker-engine | 对应 PokerGame/ShortDeckGame；getAvailableActions() 返回 {type, amount}；摊牌回调 winners/results/winnerAmounts/handName/pot/reason/share |
| 牌型名、评估 | poker-core | 中文牌型名表按变体两套（顺序不同）；evaluate 语义见 docs/golden-corpus.md §0 |
| 胜率/outs | poker-prob | 字段 winProb/tieProb/loseProb/outs.byHandType{cards,count,strongCount,thinCount}/note（见 docs/specs/prob.md） |
| AI 节奏 | poker-ai | 思考延时 max(120, aiDelay*(0.34+rand*0.08)) 由引擎侧负责；UI 只显示 thinking 状态 |
| 联机协议 | poker-protocol + poker-server | UI 只消费 game_state/回调字段；不要自行推断隐藏信息（交接 §8.1） |
| 设置持久化 | 本地 serde_json 文件（原 localStorage） | 字段见 §3.11；原实现无窗口位置持久化 |

### 9.10 验收对照（交接 §8.2 清单 → 本文条目）

| 交接清单项 | 本文位置 |
| --- | --- |
| 标准德州/短牌选择 | §3.11（游戏模式） |
| 本地对局/联机 | §3.11/§3.12 |
| 人数/筹码/盲注/AI 速度 | §3.11 设置区 |
| 玩家名字 | §3.12 连接行 |
| 音乐开关/切歌/音量/播放模式/提示音 | §3.1/§6 |
| 全屏 | §3.1/§9.8 |
| 座位/头像/名字/筹码/状态（行动中/弃牌/全下/庄家位） | §3.6 |
| 自己的两张手牌（可点击隐藏/显示） | §3.7/§3.10 |
| 公共牌/底池/当前下注/盲注前注显示 | §3.3-§3.5、§3.1 |
| 弃牌/过牌/跟注/加注/全下；加注面板 | §3.7/§3.8 |
| 行动解释/牌型名/手牌计数/BB 显示 | §3.7/§3.10/§3.1/§3.11（BB 深度摘要） |
| 结算弹窗/下一手 | §3.9/§5.6 |
| 牌桌历史/分析：胜率/outs/牌型/手牌强度 | §3.10（**“手牌历史”原实现未定义**：没有历史记录面板，只有当前手牌分析） |
| 分析遮蔽 | §3.10 |
| 顶部工具栏悬停展开/收起 | §2.6 |
| 窗口拖动/最小化/最大化/关闭 | §2.1/§9.8 |
| 联机创建/加入/房间码/玩家列表/准备/房主开始/房主迁移 | §3.12/§5（**“准备”首局原实现未定义**，只有结算后 ready） |
| 断线重连/会话恢复/连接状态错误 | §5.2/§5.3 |
| 本地服务地址显示 | §2.1/§3.12 |
| 轮到你/结算任务栏闪烁、托盘闪红 | §7 |
| 回到前台停止、同一提示不重复 | §7.2 |
| 结算两声提示音 | §6.2 |
| 键盘快捷键 | §4 |
| 小窗口滚动 | §2.7 |

### 9.11 原实现未定义 / 需拍板清单

| # | 项 | 现状 | 建议 |
| --- | --- | --- | --- |
| 1 | 显式 UI 状态机 | 无枚举，靠 DOM display + 全局布尔 | Rust 显式 Screen+Overlay（§9.1） |
| 2 | 首局联机“准备” | 无准备按钮，房主直接开始 | 保持原行为；如需准备应是协议扩展（需用户拍板） |
| 3 | 牌桌历史 | 无历史面板 | 交接清单提到“手牌历史”；建议 Phase 6 后补，或明确砍掉 |
| 4 | 设置独立页面 | 设置仅在开始页 | 保持原行为（egui 也放开始页） |
| 5 | 窗口位置/大小持久化 | 无 | 可选增强；不属于原功能 |
| 6 | 关闭确认/离开对局确认 | 无 | 保持 |
| 7 | 结算弹窗拖动边界 | 无限制，可拖出屏幕 | 建议 clamp 到窗口内（改进，需注明差异） |
| 8 | 全屏 Esc 与原生行为 | 依赖 DOM fullscreen + 手动 toggle | egui 需自己实现全屏状态跟踪 |
| 9 | player_disconnected / ack 的 UI 反馈 | 回调位存在但 ui.js 未绑定 | 保留静默或补一条状态行（建议补） |
| 10 | 联机角标与本地角标样式不一致 | 两套颜色/实现 | 统一成 §3.6.2 |
| 11 | 本地摊牌人类名 You vs 联机 你 | 不一致 | 统一为 你 |
| 12 | 分析面板展开箭头方向（收起=▼） | 反直觉 | 保持原样（用户已习惯），或加 aria 修正 |
| 13 | 加注面板初始 HTML 占位值 | 与 JS 不一致（80/1000/120 与“$60”） | 迁移时不照抄 |
| 14 | 音乐“曲名”只有 1/2/3/4 | 无真实曲名 | 保持数字或加映射 |
| 15 | __DISABLE_DEBUG__ 开关 | 仓库 index.html 未设置 | 建议 Rust 用 feature flag / 环境变量代替 |
| 16 | 无障碍（屏幕阅读器） | 仅 aria 属性；egui AccessKit 有限 | DECISIONS #5：键盘可达性必须，读屏尽力（需用户确认） |
| 17 | reduced-motion | CSS media query | egui 需自建设置项/读取系统偏好 |
| 18 | 联机“约 N 秒后重连”无倒计时刷新 | 单次文案 | 可保留（不必逐秒刷新） |
| 19 | 无边框 vs 系统边框默认策略 | 原实现无边框 | spike 建议第一版默认系统边框 + 设置项可切无边框（§9.12，需 Lead/用户拍板） |
| 20 | 头像/按钮 emoji 在 egui 的替代 | 原实现用 emoji | 建议头像/图标改绘制或纹理；文案符号逐一确认字体覆盖（§9.5） |

### 9.12 egui 0.37 API 差异与 spike 结论（供实现时避免踩坑）

来自 docs/spikes/egui-spike.md §3.3-§3.8、§5、§9（task-3 实测 + Windows target check）：

1. App trait 方法是 fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)，不是旧版 update(ctx, frame)；另有 fn logic(&mut self, ctx, frame)（窗口隐藏时仍跑逻辑）。
2. 面板：CentralPanel::default().show(ui, ...)；SidePanel/TopBottomPanel 统一成 egui::Panel::left/right/top/bottom(id).show(ui, ...)，接收 &mut Ui。
3. ctx.screen_rect() 改为 ctx.content_rect()；窗口信息用 ctx.input(|i| i.viewport())。
4. Id::new 已 deprecated：全局唯一用 Id::unique，局部用 parent_id.with(salt)（无边框边缘交互不要用常量 Id::new）。
5. 截图取证：ViewportCommand::Screenshot(ScreenshotCallback) / ctx.request_screenshot(|img| ...) 回调式（不再是 Event::Screenshot）；可用于 Phase 6 UI 验收截图。
6. eframe 默认特性含 wgpu（不是 glow）+ system_fonts + wayland/x11；glow 回退需显式开特性（eframe/glow + Renderer::Glow）。
7. FontData::from_owned 只有 face 0；TTC 用 FontData::from_blob(bytes, face_index)。
8. TextEdit 的 IME 已完整接入（IMEOutput → egui-winit → winit WM_IME_*），一般不需要手动 IMEAllowed。
9. UserAttentionType 新增 Reset，可主动停止闪烁；Windows 映射为 FlashWindowEx（Critical → FLASHW_ALL|FLASHW_TIMERNOFG）。
10. ViewportBuilder::with_decorations(false) + BeginResize 是无边框标准做法；Wayland 不支持 BeginResize（Windows/X11 支持）。
11. 已知 MSRV 约束：eframe 0.37 要求 rustc >= 1.95、rodio 0.23 要求 >= 1.87；workspace rust-version 目前 1.85，需 Lead bump（spike §4 #1）。
12. WSLg 只有软件渲染（llvmpipe 10-26fps），**帧率/性能数据不可作验收**；Windows DX12 实测 60fps（spike §5.6）。

与 UI 规格相关的待拍板项（摘自 spike §4，需 Lead/用户决定）：
- 无边框默认策略：建议第一版默认系统边框 + 设置项可切无边框（运行时已可切）。
- 字体打包：内置 3.1MB GB2312 子集 + 系统字体兜底。
- 任务栏闪烁路径：用 egui RequestUserAttention 安全封装（不引入 unsafe）。
- 托盘/窗口图标：需多尺寸 .ico。
- 音乐打包：4 首约 20MB 先内嵌，Phase 7 再决定压缩/外置。
- GPU 回退：第一版只验 wgpu/DX12，兼容问题再加 glow 特性。

---

## 10. 验证计划

### 10.1 可自动化（Rust 单测/集成测试）

1. seat_positions 黄金值：对 (ai_count, cw, ch) 的固定集合断言坐标（从 JS 直接导出作为 oracle；可写 tests/golden/ui/seat-layout.jsonl）。
2. seat_min_table_height 边界（≤7 座位、8-10 座位、宽 900/1180/1280）。
3. 文案常量表完整性（与 §8 对照，编译期 const &str）。
4. 键盘优先级状态机（纯函数 handle_key(state, key) -> Action）：覆盖 §4.2 的 7 个分支 + 输入框例外 + Numpad/F1 例外 + repeat。
5. 加注边界：snap_raise/raise_bounds 与 JS 一致（game-rules-core 迁移已可复用）。
6. 结算弹窗状态机：ready_status 的 3 种分支、settlement 音只播一次。
7. 注意力提醒 key 生成与去重（纯函数）。
8. 设置持久化 round-trip（JSON 字段与默认值）。
9. 联机状态字段映射（game_state → 视图模型；可用 poker-protocol 的 golden 语料）。
10. 截图回归（可选）：spike 的 ctx.request_screenshot 回调可自动截取指定帧，作为布局冒烟。

### 10.2 手动回归（Windows；决定 Phase 6 验收）

按 spike §5 的 Windows 清单执行，并补充 UI 功能项：

1. 本地标准 10 人：开始→下注全流程→摊牌弹窗→下一手→破产回主页。
2. 本地短牌 6 人：同流程 + 牌型强弱/前注显示。
3. 键盘：README 全部快捷键 + Tab/Shift+Tab + 输入框例外 + 加注面板（1/2/方向/Esc/Space/取消）。
4. 手牌隐藏：牌背/分析遮蔽/重新展开/新一局复位。
5. 音乐：开关、切歌、音量、三种播放模式、结算/出手提示音、静音、关音乐不预载。
6. 提醒：最小化后轮到/结算 → 任务栏闪烁+托盘闪红 650ms；回前台停止；同一提示不重复。
7. 联机：创建/加入/房主开始/房主迁移/断线自动重连/手动断开/离开房间/下一手 ready/破产。
8. 小窗口：900×600 + 高度 <650 + <760 三档；座位不重叠、可滚动；顶部工具栏悬停展开/收起。
9. 窗口：拖动、双击最大化、边缘缩放、最小化/最大化/关闭、全屏 + Esc、单实例、DPI 125%/150%。
10. 中文 IME（spike §5.2，最高风险）、托盘/任务栏闪烁（spike §5.4）、音频（spike §5.5）、GPU/性能（spike §5.6）。
11. 若 IME/无边框/托盘任一不达标：按 spike §0 停止 Phase 6 牌桌 UI 开发，回退方案需用户重新拍板。

---

## 附录 A：源码位置索引（按文件）

- js/ui.js
  - 全局状态：1-55；注意力：58-77；结算弹窗呈现：79-85；设置读写：87-145；本地服务地址：147-166；formatChips：173-175；盲注文案：182-195；设置更新/同步：197-266；服务端配置应用：268-293；联机表现复位：295-307；模式选择：310-342；联机连接与回调：346-491；分析上下文/联机概率：497-560；联机渲染入口：563-708；公共牌动画：710-846；座位工具：848-971；联机玩家渲染：974-1119；联机动作/状态/摊牌：1125-1304；联机动作发送：1307-1338；断开/加入/离开/开始：1340-1395；玩家列表/人数选项：1397-1477；本地回调：1480-1565；浮动气泡：1568-1586；本地渲染：1592-1625；牌面：1627-1653；人类手牌：1655-1762；人类徽章：1764-1798；公共牌/座位：1800-1989；座位坐标：1996-2040；动作按钮/概率/手牌分析/outs/状态/底池/轮次：2042-2322；行动与加注：2328-2455；摊牌弹窗：2461-2604；主页/开新局/模式 UI：2607-2715；弹窗拖动：2721-2811；消息弹窗：2813-2853；全屏/窗口：2860-2912；主页/刷新/toast：2919-2975；桌面头部：2981-3012；DOMContentLoaded：3014-3067；规则/分析开关：3073-3089；音乐：3091-3229；提示音：3231-3311；全局导出：3313-3341。
- index.html：head/CSP 1-10；背景装饰 12-34；悬停区 37；头部 38-72；状态行 74-75；策略小字 78；牌桌 79-247；底部面板 249-331；结算弹窗 334-347；脚本加载 349-364；规则说明 366-485；ui.js/controls.js 487-488。
- css/style.css：变量/基础 1-33；背景层 35-80；头部 82-306；牌桌 308-348；公共牌 350-431；底池 433-454；座位 456-555；牌 557-620；底部 622-841；分析面板 843-1017；弹窗 1019-1123；状态/主页/盲注 1125-1187；响应式 1189-1217；滚动条 1219-1223；牌桌装饰 1225-1245；徽章 1247-1297；人数选择 1299-1345；模式选择 1347-1401；设置 1403-1517；玩法/联机/弹窗拖动/音乐 1529-1671；浮动动作 1673-1748；思考 1750-1771；动作 pop 1773-1791；解释 1793-1811；规则 1823-1935；窄屏 1937-1958；reduced-motion 1960-1970。
- js/controls.js：动作映射 8-32；click 34-43；input 45-51；keydown 53-133。
- js/audio.js：曲目 23-28；初始化/播放器 37-91；加载/预载/选曲 93-137；淡入淡出 139-207；start/stop/toggle/next/setVolume/setPlaybackMode/setGamePhase 209-295。
- js/network.js：终止码 8-15；状态 17-70；连接/重连 81-181；房间操作 183-253；消息分发 255-351；回调默认值 353-366。
- js/debug.js：面板 18-112；toast 114-120；日志/复制 122-158；错误/WS/game/心跳/console 采集 160-245。
- main.js：单实例/低内存 12-17；本地服务 36-71；IPC 77-90；窗口 101-132；生命周期 128-152。
- js/game.js：human 对象 77；AI 初始化 85-107；阶段 21-56；AI 回调 486-531。
- js/game-rules-core.js：wagerUnit 8-13；raiseBounds 16-19；snapRaise 21-25。
- js/cards.js：牌型名 81-92；js/shortdeck.js：短牌牌型名 40-51；js/ai.js：AI 名字/头像 55-56。
- tests/ui-network.test.js：不变量 259-658（焦点/键盘/音频/座位复用/配置指纹/分析遮蔽/结算音）。

## 附录 B：常量总表

| 常量 | 值 | 出处 |
| --- | --- | --- |
| 窗口默认/最小 | 1280×860 / 900×600 | main.js:101-106 |
| 内容最大宽 | 1200 | style.css:29 |
| 牌桌最小高 | 520（紧凑 480） | style.css:314,1520 |
| 座位尺寸 | 104×210（≤650 高 180；≤768 宽 min 80×110） | style.css:467-482,1206,1521 |
| 座位排容量 | floor(cw/130) clamp 3..7；侧座最多 2 | ui.js:2005-2009 |
| 弧线 drop | 42（≤650 高 16） | ui.js:2018 |
| 公共牌/底池位置 | top 58%（≤650 高 56%），底池 +70px | style.css:359,447,1525-1526 |
| 悬停区高度 | 0.85 × header 高（CSS 兜底 64px） | ui.js:2992；style.css:113 |
| 起步筹码档 | 5k/10k/20k/50k/100k（默认 20k） | ui.js:16,26 |
| 盲注档 | 10/20、20/40、40/80、50/100、100/200、200/400（默认 40/80） | ui.js:17-24,27-28 |
| AI 节奏 | 快速 800 / 标准 1800 / 沉浸 3200（默认 1800） | ui.js:45-50 |
| AI 思考延时 | max(120, aiDelay×(0.34+rand×0.08)) ms | js/game.js:529 |
| 标准人数 | 2/6/7/8/9/10（默认 10）；短牌 2~8（默认 6） | ui.js:1431-1433 |
| 结算延时 | 2000ms（摊牌后弹窗）；下一手 300ms；结算提示音去重 | ui.js:1515,2603 |
| 发牌/翻牌 | 420ms、150ms/张、翻转 520ms；手牌 260+420ms | §2.9 |
| 动作标签移除 | 800ms；浮动气泡 3500ms | ui.js:1923-1926,1583-1585 |
| 音乐曲目 | 4 首，gain 1.0；音量上限 0.72 | audio.js:14,23-28；ui.js:3097-3099 |
| 音乐阶段增益 | idle .90 preflop .92 flop .97 turn 1.02 river 1.06 showdown 1.04 | audio.js:288 |
| 淡入淡出 | 手动切 1200ms、自动换 350ms、开始 220ms、停止 350ms、错误 100ms | audio.js:73-77,205,223,226,257-267 |
| 提示音 | 出手 800+1200Hz 0.25s；结算 660→990Hz 间隔 0.18s | ui.js:3273,3295 |
| 托盘闪烁 | 650ms 交替 | desktop-attention.js:71 |
| 调试日志上限 | 200 条；面板 520×420；心跳阈值 3000ms | debug.js:13,21,224 |
| 名字长度 | 8（UTF-16 code units，输入框 maxlength） | index.html:184；ui.js:130 |
| 联机房间码 | 1–5，默认最多 3 房间 | server/local-server.js:73,238-247 |
| 消息大小上限 | 16KB（服务端） | server/local-server.js:188 |
| 协议版本 | 2 | network.js:257；交接 §8.3 |
| 设置存储键 | texas-holdem-settings-v1（音乐版本 2） | ui.js:43-44 |
| 联机会话键 | texas-holdem-network-session-v2 | network.js:6 |

## 附录 C：关键不变量（来自回归测试，Rust 侧应保留等价测试）

1. window.game 必须是当前实例，不能是陈旧引用（tests/ui-network.test.js:265-273）。
2. 输入框/可编辑区/下拉框里的按键不触发牌桌动作（:275-293）。
3. 方向键调加注、Space 确认；1/2 只认顶排 Digit、不偷 F1/F2/Numpad/文本数字（:295-347）。
4. 音乐按钮在 loading/playing 交替时文本稳定，不重建（:349-367）。
5. 每条结算结果只响一次；静音时完全不创建/使用音频（:369-388）。
6. 桌面提醒独立于声音：turn/settlement/ack 三种信号正确（:390-411）。
7. 本地每次新决策只响一次出手音（:413-431）。
8. 手牌隐藏时，分析面板收起→无遮蔽；展开→遮蔽；再收起→隐藏；牌面恢复→正常（:433-456）。
9. applyRoomConfigFromServer 相同配置不得重建设置节点（联机热路径，:458-485）。
10. 座位渲染稳态不创建/追加节点、不读 offsetWidth/Height、座位节点身份不变（:487-509）。
11. 回主页必须对每个被移除座位 unobserve（:511-520）。
12. 最近动作文案/类不变时不得重写（防动画重启，:522-540）。
13. 只有 formatChips 可调用 toLocaleString（:542-558）。
14. AudioContext 复用、卸载时 close、释放后重建（:560-570）。
15. 提示音默认开、可单独静音、持久化（:572-583）。
16. Tab 选中手牌/按钮/滑块后仍可用 F/C/R/A（:605-623）。
17. 加注面板打开时 Enter/Space 优先确认/取消，不被旧焦点控件抢走；repeat 不重复提交（:625-657）。

## 附录 D：本文与交接文档/README 的差异记录（便于验收时澄清）

1. 交接 §8.2 提到“手牌历史”，原实现只有“当前手牌分析”，**没有历史记录面板**【未定义】。
2. 交接 §8.2 提到联机“准备”，原实现首局没有准备按钮，只有结算后 ready_for_next【未定义】。
3. README 说“顶排数字 1/2”，代码还要求 event.code=Digit1/Digit2，Numpad 无效。
4. README 的“Enter/空格普通状态激活选中控件”只对 button 原生生效；手牌区/分析标题有专门处理。
5. 交接 §3.3 说原项目最小窗口 900×600、默认 1280×860 —— 与 main.js:101-106 一致。
6. 交接 §6.3 提到“无障碍”；原 UI 仅有 aria 属性与键盘操作，无读屏专门适配。
7. 交接 §8.3 的“默认配置：标准十人桌、起始 5000、盲注 100/200、25 BB”是 AI 优化重点配置的标注，**不是 UI 默认值**；UI 默认是 10 人 / 20000 / 40/80（index.html:116,139,150；ui.js:26-28），README:80 的小字注释也说明“该配置默认加入单挑河牌范围博弈”指的是 AI 重点配置。实现时以 ui 默认为准，可用策略小字提示。
8. §9 的 egui API 结论来自 task-3 spike（docs/spikes/egui-spike.md），与原 JS 实现无关；冲突时以本文 §1-§8 的行为规格为准。
