# WebSocket 协议 v2 规格（与原 JS 客户端/服务端互通）

> 状态：Phase 5 预研定稿（2026-10-23）
> 权威实现：只读参考仓库 reference/texas-holdem-game（commit e0c6c6f88d3e64324e1d83bfc700dd3a77f71b90，v1.5.8）中的
> server/local-server.js、server/server-game.js、js/network.js、main.js。
> 权威语料：tests/golden/protocol/（由 oracle/golden-protocol.js 从真实 JS 服务端捕获，不手写）。
> Rust 实现：crates/poker-protocol（serde DTO、消息枚举、解析、错误码、常量、配置白名单）。
> 硬约束：Rust 服务端必须能服务原 JS 桌面客户端；Rust 客户端必须能连接原 JS 服务端。
> 任何字段名、JSON 类型、可选性、错误码、错误文本、关闭码的改动都必须先重跑 oracle 并更新本文档。

## 1. 传输层

- 协议：WebSocket（ws:// / wss://），JSON 文本帧，UTF-8。二进制帧未定义。
- 最大载荷：16384 字节（JS 侧 ws 的 maxPayload）。超过时由 ws 协议层直接以关闭码 1009 关闭；
  local-server.js 里另有一次 data.length > 16*1024 的应用层检查（"message too large"），
  实际被 ws 层先拦截，所以线上捕获到的 1009 reason 是空字符串。Rust 实现按 16 KiB 帧上限处理。
- 不启用 perMessageDeflate（JS 显式关闭），Rust 侧也不要协商压缩。
- 心跳：服务端每 15000ms 对本轮所有连接 ping；上一轮没有 pong 的连接直接 terminate。
  客户端不发送应用层心跳。
- 连接建立后服务端立即发送 hello（任何请求之前）。
- 每条服务端消息都带 protocolVersion: 2（safeSend 统一前置）。原 JS 客户端的 send() 同样
  对每条客户端消息前置 protocolVersion: 2。

## 2. 版本校验与请求 ID

### 2.1 版本校验（服务端处理客户端消息的顺序）

参考 server/local-server.js 的 handle()：

1. message 必须是非空值且 message.type 是 string，否则 error(INVALID_REQUEST, "缺少消息类型")。
2. 若消息携带 protocolVersion 字段：按 JavaScript Number() 转换后必须等于 2，
   否则 error(PROTOCOL_MISMATCH, "协议版本不匹配（服务端 v2）")。
3. 按 type 分发；未知 type → error(INVALID_REQUEST, "未知消息类型")。

注意顺序：版本校验在 dispatch 之前，所以 "未知 type + 错误版本" 返回的是 PROTOCOL_MISMATCH。

JavaScript Number() 强制转换的实际接受/拒绝矩阵（已由 parse-errors.jsonl 的 golden 用例固定）：

| protocolVersion JSON 值 | JS Number() | 结果 |
| --- | --- | --- |
| 2 | 2 | 接受 |
| "2" | 2 | 接受 |
| " 2 " | 2 | 接受 |
| [2] | 2 | 接受 |
| 2.0（JSON 里就是 2） | 2 | 接受 |
| 缺省 | 不校验 | 接受 |
| 3 / "3" / true / null / [] / [2,3] / "2x" / {} | 非 2（true 为 1，null 为 0，[] 为 0） | PROTOCOL_MISMATCH |

服务端→客户端消息里的 protocolVersion 原 JS 客户端完全不校验（network.js 的 handleMessage 不看它），
Rust 客户端同样忽略；Rust 的 ServerMessage DTO 允许任意 u8 并原样保留。

### 2.2 请求 ID（requestId）

- 对客户端是不透明字符串。network.js 的格式：prefix-base36(Date.now())-base36(序号)，
  prefix 取 {action, create, join, leave, ready, resume, start}（client-behaviour.json 固定）。
- 服务端不校验格式，只回显：ack / action_ack / error 的 requestId 字段是请求值的原样回显；
  JS 的 error() 用 requestId || null，所以空串、0、false 等假值变成 null。
- player_action 必须带 requestId，否则 error(INVALID_REQUEST, "动作缺少 requestId")（注意是 error 而不是 action_ack）。
- 去重缓存：每个玩家最近 64 条 action_ack 以 requestId 为 key 缓存。相同 requestId 的重复
  player_action 不再执行，直接重放上一次的 action_ack（旧客户端网络重试依赖此语义）。
  缓存满 64 条后淘汰最旧的一条。
- Rust DTO 用 RequestId(serde_json::Value) 保存原始 JSON，从而能逐字复现回显语义
  （包括对恶意客户端回显数字等非字符串值）。

## 3. 消息目录（共 20 种）

| 方向 | type | 触发 | 接收者 |
| --- | --- | --- | --- |
| s2c | hello | 连接建立 | 该连接 |
| c2s | create_room | 客户端创建房间 | — |
| c2s | join_room | 客户端加入房间（房间不存在时创建） | — |
| c2s | leave_room | 离开房间 | — |
| c2s | start_game | 房主开始游戏 | — |
| c2s | player_action | 提交行动 | — |
| c2s | ready_for_next | 结算后准备下一手 | — |
| c2s | resume_session | 断线重连恢复座位 | — |
| c2s | get_snapshot | 请求权威快照（UI 未使用，服务端支持） | — |
| s2c | room_created | 创建房间成功 | 创建者 |
| s2c | room_joined | 加入/恢复房间成功 | 加入者 |
| s2c | player_joined | 有人加入 | 房内已连接人类 |
| s2c | player_left | 有人离开/断线/宽限到期 | 房内已连接人类 |
| s2c | host_changed | 房主迁移 | 房内已连接人类 |
| s2c | game_starting | 开局 | 房内已连接人类 |
| s2c | game_state | 权威牌桌快照 | 房内已连接人类（每人个性化） |
| s2c | action_ack | player_action 结果 | 请求者 |
| s2c | ready_status | 准备状态变化 | 房内已连接人类 |
| s2c | ack | 通用请求确认 | 请求者 |
| s2c | error | 请求失败 | 请求者 |

死分支说明：

- player_disconnected：js/network.js 有 case 分支，但 JS 服务端从不发送。语料中故意不含该消息；
  Rust 服务端不实现；Rust 客户端按未知类型忽略（保留 network.js 的宽松行为）。
- get_snapshot：js/network.js 从不发送，但 local-server.js 的 handle 支持。语义 = 回送
  room_joined(resumed: true) ；若在牌局中再补一条 game_state。

## 4. 共享 DTO

所有字段名使用 camelCase（JS 风格）。Rust DTO 用 serde rename 精确对齐；golden roundtrip 测试
保证字段集合、类型、null 与缺省完全一致（键顺序无关）。

### 4.1 roomConfig（权威房间配置）

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| maxPlayers | int | 座位数，2..10（短牌 2..8） |
| isShortDeck | bool | 6+ Hold'em |
| startingStack | int | 起始筹码，白名单档位 |
| smallBlind | int | 小盲，白名单档位 |
| bigBlind | int | 大盲，白名单档位 |
| ante | int | 短牌前注，恒等于 smallBlind |
| minBet | int | 短牌最小注，恒等于 bigBlind |
| aiDelay | int | 服务端 AI 行动延迟（毫秒） |

### 4.2 PublicPlayer

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| id | string | 玩家 id（JS 为 crypto.randomUUID） |
| seatId | int | 座位号（按排序后的玩家列表下标） |
| name | string | 规范化后的名字 |
| avatar | string | 头像字符串（JS 用单个 emoji） |
| isHuman | bool | false = 服务端 AI 补位 |
| connected | bool | 连接状态（断线宽限期内 false） |
| stack | int | 剩余筹码 |
| chipsInPot | int | 本手投入底池总额 |
| roundBet | int | 本轮已下注额 |
| folded | bool | 已弃牌 |
| isAllIn | bool | 已全下 |
| lastAction | null 或对象 | {action: string, amount: int} |

lastAction.action 是自由字符串，不只限于行动词表：强制下注也会写入
"small blind" / "big blind" / "ante" / "dealer ante"；玩家行动写
"fold" / "check" / "call" / "raise" / "allin"。UI 对未识别标签原样显示，因此 DTO 不能用枚举。

### 4.3 LegalActions

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| actions | string[] | 合法行动词表子集 |
| toCall | int | 跟注额（已按 stack 截断） |
| canRaise | bool | 是否允许加注 |
| minRaiseTo | int | 最小加注到（网格对齐；全下例外） |
| maxRaiseTo | int | 最大加注到（本轮下注 + 剩余筹码） |
| wagerUnit | int，**可能缺省** | 加注网格单位（标准=小盲，短牌=前注） |

关键细节：当该玩家不是当前行动者时，JS 的 legalActions() 走 early-return，
返回对象缺少 wagerUnit 键。Rust DTO 中 wagerUnit 是 Option，缺省时序列化不输出该键
（golden 语料中两种形态都有）。

### 4.4 牌

普通牌对象：{"rank": string, "suit": string}

| 字段 | 取值 |
| --- | --- |
| rank | "2","3","4","5","6","7","8","9","10","J","Q","K","A"（注意 10 是两字符串） |
| suit | "spades","hearts","diamonds","clubs"（全小写全名，不是 s/h/d/c） |

摊牌结果 lastHandResult.results[].cards 的牌对象额外带 value（int，2..14）。
Rust 用 CardDto / RevealedCard 两种类型表示。

### 4.5 LastHandResult

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| reason | "showdown" 或 "fold" | 结束原因 |
| isShortDeck | bool | 变体 |
| pot | int | 结算底池 |
| handRank | int | 全场最强牌型序号；fold 结算为 0 |
| winners | PublicIdentity[] | 赢家（从庄家左侧起顺序） |
| winnerAmounts | object<string,int> | 每个赢家实际获得筹码 |
| results | object[] | 每名参与摊牌/入池玩家一行 |

results[] 行：{id, seatId, name, isHuman, cards: RevealedCard[], handRank: int}。
fold 结算时 cards 为空数组、handRank 为 0。

### 4.6 PublicIdentity / ReadyEntry

PublicIdentity：{id, seatId, name, isHuman}。
ReadyEntry：{id, ready}。

### 4.7 错误码与错误文本

| code | 触发 | 文本（逐字一致） |
| --- | --- | --- |
| INVALID_REQUEST | 默认错误码；绝大多数校验失败 | 见下表 |
| PROTOCOL_MISMATCH | protocolVersion 转换后不等于 2 | 协议版本不匹配（服务端 v2） |
| ROOM_LIMIT | join_room 指向不存在房间且房间数已满 | 房间已满（最多 N 个） |
| RESUME_EXPIRED | resume token 失效或原房间已关闭 | 恢复信息已失效 / 原房间已关闭 |

golden 语料捕获到的完整错误文本表（Rust 服务端必须逐字复现；实现时可从语料测试反查）：

| 场景 | code | message |
| --- | --- | --- |
| JSON 解析失败 | INVALID_REQUEST | 消息格式无效 |
| 缺 type / type 非字符串 | INVALID_REQUEST | 缺少消息类型 |
| 未知 type | INVALID_REQUEST | 未知消息类型 |
| 版本不匹配 | PROTOCOL_MISMATCH | 协议版本不匹配（服务端 v2） |
| 已在该房间继续 join | INVALID_REQUEST | 请先离开当前房间 |
| 非房主 start | INVALID_REQUEST | 只有房主可以开始游戏 |
| 已有牌局再 start | INVALID_REQUEST | 游戏已经开始 |
| 有人断线宽限期 start | INVALID_REQUEST | 有玩家正在恢复连接，请稍候再开始 |
| 开局时不足两人 | INVALID_REQUEST | 至少需要两名可参赛玩家 |
| 房间号非法 | INVALID_REQUEST | 房间号只能是 1–5 |
| 房间人类已满 | INVALID_REQUEST | 房间已满 |
| 房间数已满（创建） | INVALID_REQUEST | 房间已满（最多 N 个） |
| 房间数已满（直接 join 不存在房间） | ROOM_LIMIT | 房间已满（最多 N 个） |
| 加入已开局房间 | INVALID_REQUEST | 房间已经开局，只能使用恢复令牌返回原座位 |
| 恢复信息无效 | RESUME_EXPIRED | 恢复信息已失效 |
| 原房间已关闭 | RESUME_EXPIRED | 原房间已关闭 |
| get_snapshot 不在房间 | INVALID_REQUEST | 尚未进入房间 |
| 不在房间时 action/ready | INVALID_REQUEST | 牌局尚未开始 |
| player_action 缺 requestId | INVALID_REQUEST | 动作缺少 requestId |
| 房间号过期 | （action_ack accepted=false） | 房间状态已过期 |
| handId 过期 | （action_ack accepted=false） | 这是一条旧牌局动作 |
| turnId 过期 | （action_ack accepted=false） | 行动回合已经变化 |
| expectedStateVersion 过期 | （action_ack accepted=false） | 牌局状态已更新，请重试 |
| 行动不在合法集合 | （action_ack accepted=false） | 当前不能执行这个行动 |
| 加注非网格倍数 | （action_ack accepted=false） | 加注金额必须是 N 的整数倍；零头筹码可使用全下 |
| 加注超范围 | （action_ack accepted=false） | 加注金额不在合法范围内 |
| ready 早于结算 | INVALID_REQUEST | 本手尚未结束 |
| ready handId 过期 | INVALID_REQUEST | 旧牌局的准备信号已忽略 |
| 恢复会话时已在房间 | INVALID_REQUEST | 请先离开当前房间再恢复其他座位 |
| 处理异常兜底 | INVALID_REQUEST | 服务端处理失败，请重试 |

### 4.8 数值语义

- 协议消息中所有数值都是 JS Number 的整数值；不存在浮点、NaN、Infinity（JSON 也无法表达）。
- 负数仅用于“无”的索引：短牌 sbIdx/bbIdx 为 -1；idle/showdown 的 currentPlayerIdx 为 -1。
- Rust DTO 统一用 i64 承载这些整数。

## 5. 客户端→服务端消息

通用约定：每条消息可带 protocolVersion（旧客户端总发 2）和 requestId（见 §2.2）。
下文的“配置字段”指 create_room / join_room 共用的 8 个房间选项：
maxPlayers、isShortDeck、startingStack、smallBlind、bigBlind、ante、minBet、aiDelay；
它们只在“本次 join 真正创建房间”时生效，加入已有房间时被忽略。

### 5.1 create_room

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| protocolVersion | int | 否 | 客户端总发 2 |
| requestId | string | 是 | 回显 |
| name | string | 否 | 缺省 "玩家"；服务端 NFKC 规范化并截断 8 码点 |
| 配置字段 | 见上 | 否 | 用于初始化房间 |

行为：取 1..5 中第一个空闲房间号，等价于以该 roomCode 调用 join_room。若没有空闲号或
房间数达到上限 → error(INVALID_REQUEST, "房间已满（最多 N 个）")。首个玩家成为房主。
注意：create_room 失败不会回 ROOM_LIMIT（与直接 join 不存在房间不同）。

### 5.2 join_room

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| protocolVersion | int | 否 | |
| requestId | string | 是 | |
| roomCode | string | 否 | 必须是 "1".."5"；缺省/非法 → "房间号只能是 1–5" |
| name | string | 否 | 同 create_room |
| 配置字段 | 见上 | 否 | 仅创建时生效 |

行为与错误（按 JS 检查顺序）：

1. 连接已绑定房间 → error "请先离开当前房间"。
2. roomCode 不匹配 ^[1-5]$ → error "房间号只能是 1–5"。
3. 房间不存在：若 rooms.size >= maxRooms → error(ROOM_LIMIT, "房间已满（最多 N 个）")；
   否则创建房间（config 由本条消息经白名单生成，created=true）。
4. 房间已有 game（开局后）→ error "房间已经开局，只能使用恢复令牌返回原座位"。
5. 房间内人类数 >= roomConfig.maxPlayers → error "房间已满"。
6. 成功：分配最小空闲座位；生成 playerId（UUID）与 resumeToken（18 字节 base64url，
   长度 24）；写入 sessions 表；room.stateVersion++；向该玩家发 room_created（创建者）
   或 room_joined；向房内所有已连接人类广播 player_joined。

### 5.3 leave_room

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| protocolVersion | int | 否 | |
| requestId | string | 是 | 用于 ack |
| roomCode | string | 否 | 信息性，不参与校验 |

行为：连接不在房间时静默丢弃（既无 ack 也无 error，旧客户端依赖这一点避免报错）。
在房间时：player.left=true、connected=false、清定时器、删除 session（resumeToken 立即失效）、
连接 meta 解绑；无 game 时从 players 直接移除；有 game 时保留席位（left=true）；
若该玩家正在手中且未弃牌/未全下则自动弃牌并推进牌局；migrateHost(立即)；
stateVersion++；广播 player_left；若有 game 广播 game_state；向离开者 ack；
尝试开下一手；空房清理。

### 5.4 start_game

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| protocolVersion | int | 否 | |
| requestId | string | 是 | 用于 ack |
| roomCode | string | 否 | 信息性 |

检查顺序：不在房间 → "尚未进入房间"；非房主 → "只有房主可以开始游戏"；
已有 game → "游戏已经开始"；存在断线人类（connected=false）→ "有玩家正在恢复连接，请稍候再开始"。

行为：过滤席位（保留未 left 且（人类或 stack>0）的玩家）；把 0..maxPlayers-1 的空座用
服务端 AI 补齐（名字/头像/AI 人格风格由随机数决定）；构造 ServerPokerGame 并挂
onChange（stateVersion++、广播 game_state、调度 AI）与 onHandEnd（清 ready、广播 ready_status）；
向所有已连接人类发 game_starting；startHand() 失败（可参赛玩家 < 2）→ game=null 并
error "至少需要两名可参赛玩家"；成功 → ack。

### 5.5 player_action

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| protocolVersion | int | 否 | |
| requestId | string | **是** | 缺 → error "动作缺少 requestId" |
| roomCode | string | 否 | 与连接所在房间不同 → action_ack 拒绝 |
| handId | int | 是 | 旧手 → action_ack 拒绝 |
| turnId | int | 是 | 旧回合 → action_ack 拒绝 |
| expectedStateVersion | int | 是 | 旧状态 → action_ack 拒绝 |
| action | string | 是 | 服务端转小写后与合法行动集合比较 |
| amount | int | 否 | raise 时表示“加注到”（raise-to），其它行动忽略；缺省 0 |

严格处理顺序：

1. 重复 requestId（该玩家最近 64 条）→ 直接 safeSend 缓存的 action_ack（不再执行）。
2. roomCode 不匹配 → "房间状态已过期"。
3. handId 不匹配 → "这是一条旧牌局动作"。
4. turnId 不匹配 → "行动回合已经变化"。
5. expectedStateVersion 不匹配 → "牌局状态已更新，请重试"。
6. game.act(idx, action, amount)：行动不在合法集合 → "当前不能执行这个行动"；
   加注金额不是 wagerUnit 倍数 → "加注金额必须是 N 的整数倍；零头筹码可使用全下"；
   加注金额越界 → "加注金额不在合法范围内"。
7. 构造 action_ack 并写缓存；成功 message 为 "行动已确认"，失败为上述原因文本。
8. 失败时额外给该玩家补发一条 game_state（重新同步权威状态）。

顺序保证：成功的 act 会同步触发 onChange → 先广播 game_state，随后才发送 action_ack
（同一 socket 内保序）。Rust 实现必须保持此顺序，客户端以 stateVersion 递增判断成功。

### 5.6 ready_for_next

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| protocolVersion | int | 否 | |
| requestId | string | 是 | |
| roomCode | string | 否 | 信息性 |
| handId | int | 是 | 必须是刚结束的那一手 |

检查：不在房间/无 game → "牌局尚未开始"；phase != idle 或无 lastHandResult → "本手尚未结束"；
handId 不匹配 → "旧牌局的准备信号已忽略"。成功：room.ready.add(playerId)；ack；
waiting = 已连接、未 left、stack>0 的人类；allReady = waiting 非空且全部 ready；
若 allReady → tryStartNextHand（250ms 定时）；广播 ready_status（allReady / waitingFor / players）。

### 5.7 resume_session

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| protocolVersion | int | 否 | |
| requestId | string | 是 | |
| roomCode | string | 否 | 信息性（token 才是权威） |
| resumeToken | string | 是 | room_created/room_joined 下发 |

检查：连接已在房间 → "请先离开当前房间再恢复其他座位"；token 不存在 →
error(RESUME_EXPIRED, "恢复信息已失效")；房间或玩家不存在/已 left →
error(RESUME_EXPIRED, "原房间已关闭")。

成功：若旧 socket 仍 OPEN 且不是当前 socket → close(4001, "session resumed elsewhere")；
连接 meta 重新绑定；清 cleanup/disconnect/host 定时器；发 room_joined(resumed=true)；
room.stateVersion++；广播 player_joined；若有 game 广播 game_state；若 phase=idle 且有
lastHandResult 则尝试开下一手并广播 ready_status。

### 5.8 get_snapshot

无必填字段（可带 requestId；JS 不带）。不在房间 → error(INVALID_REQUEST, "尚未进入房间")，
requestId 为 null。否则 = room_joined(resumed=true) +（若有 game）game_state。

## 6. 服务端→客户端消息

通用约定：全部带 protocolVersion: 2；字段名 camelCase。

### 6.1 hello

| 字段 | 类型 | 值 |
| --- | --- | --- |
| serverId | string | "texas-holdem-game" |
| serverName | string | "德州扑克服务" |
| appVersion | string | "1.5.8"（兼容期；见 §11） |
| protocolVersion | int | 2 |

### 6.2 room_created / room_joined

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| roomCode | string | "1".."5" |
| playerId | string | 本玩家 id |
| seatId | int | 本玩家座位 |
| isHost | bool | 是否房主 |
| hostId | string 或 null | 当前房主 |
| resumeToken | string | 24 字符 base64url |
| resumed | bool | true = 恢复会话/get_snapshot 回送 |
| roomConfig | RoomConfig | 权威配置 |
| players | PublicPlayer[] | 公共玩家列表（含 left 玩家当且仅当有 game；按 seatId 排序） |

### 6.3 player_joined / player_left

| 字段 | 类型 |
| --- | --- |
| roomCode | string |
| hostId | string 或 null |
| roomConfig | RoomConfig |
| players | PublicPlayer[] |

player_left 出现的三个时机：显式 leave、断线瞬间（connected=false）、断线宽限到期（left=true）。

### 6.4 host_changed

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| roomCode | string | |
| hostId | string 或 null | 新房主；无人类时为 null |
| isHost | bool | 针对收件人个性化 |
| stateVersion | int | 变更后的房间版本 |
| players | PublicPlayer[] | 公共玩家列表 |

### 6.5 game_starting

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| roomCode | string | |
| playerCount | int | 总座位（人类 + AI 补位） |
| humanCount | int | 人类数 |
| aiCount | int | AI 数 |
| roomConfig | RoomConfig | |

### 6.6 game_state（权威快照，每人一份）

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| roomCode | string | |
| stateVersion | int | 单调递增；player_action 的 expectedStateVersion 与之比较 |
| handId | int | 本手编号，从 1 开始 |
| turnId | int | 决策/状态编号；旧动作拒绝与 UI 提示去重依赖它 |
| hostId | string 或 null | |
| isHost | bool | 收件人是否房主 |
| roomConfig | RoomConfig | |
| isShortDeck | bool | |
| phase | "preflop" / "flop" / "turn" / "river" / "showdown" / "idle" | |
| pot | int | |
| communityCards | Card[] | 0/3/4/5 张 |
| players | PublicPlayer[] | 按座位排序；不含任何隐藏手牌 |
| dealerPos | int | 庄家座位 |
| sbIdx | int | 小盲座位；短牌无盲注时为 -1 |
| bbIdx | int | 大盲座位；短牌无盲注时为 -1 |
| currentPlayerIdx | int | 当前行动者；-1 表示 idle/showdown |
| currentBet | int | 本轮需跟注额 |
| lastRaise | int | 本轮最近一次完整加注的增量 |
| isYourTurn | bool | 收件人是否该行动 |
| yourCards | Card[] | 收件人手牌（仅自己可见） |
| yourStack | int | 收件人剩余筹码 |
| yourBet | int | 收件人本轮已下注 |
| legalActions | LegalActions | 收件人合法行动（非其回合时 actions 为空且无 wagerUnit） |
| lastHandResult | LastHandResult 或 null | 上一手结算（phase=idle 时）；新一手开始后为 null |

客户端处理规则（network.js）：
stateVersion 不大于已处理值且已有状态时丢弃；handId 小于已处理 handId 时丢弃；
其余按权威状态渲染。Rust 客户端应照此实现（poker-app 阶段）。

推送时机：开局、任何游戏状态变化、断线/离开导致弃牌、房主迁移、resume、action 失败后补发、
get_snapshot。

### 6.7 action_ack

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| requestId | string（或原始 JSON） | 回显 |
| accepted | bool | |
| message | string | 成功 "行动已确认"，失败为原因 |
| roomCode | string | |
| handId | int | 处理后的手编号 |
| turnId | int | 处理后的决策编号 |
| stateVersion | int | 处理后的房间版本 |

### 6.8 ready_status

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| roomCode | string | |
| handId | int | 结算手编号 |
| allReady | bool | 全部就绪且下一手已调度 |
| waitingFor | int | 未就绪的活跃人类数 |
| players | {id, ready}[] | 每个活跃人类一项 |

### 6.9 ack

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| requestId | string | 仅请求带 requestId 时发送 |
| accepted | bool | 恒 true（失败走 error） |
| roomCode | string 或 null | 无房间时 null |
| stateVersion | int | |
| handId | int | 无牌局时 0 |

发送范围：start_game、ready_for_next、leave_room。create_room/join_room 不返回 ack
（用 room_created/room_joined 代替）。

### 6.10 error

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| requestId | 原始 JSON 或 null | 回显；假值变 null |
| code | "INVALID_REQUEST" / "PROTOCOL_MISMATCH" / "ROOM_LIMIT" / "RESUME_EXPIRED" | 见 §4.7 |
| message | string | 中文错误文本，逐字一致 |

## 7. WebSocket 关闭码

| code | reason（服务端） | 触发 | 客户端行为 |
| --- | --- | --- | --- |
| 1000 | 客户端自定 | 客户端主动正常关闭 | 终止（不重连） |
| 1008 | message rate exceeded | 每连接每窗口消息数超限 | 终止（策略违规，不重连） |
| 1009 | 空字符串 | 帧超过 maxPayload（ws 协议层） | 终止（消息过大，不重连） |
| 1013 | server capacity reached | 连接数/每 IP 连接数超限（关闭发生在 hello 之前） | 终止（容量满，不重连） |
| 4001 | session resumed elsewhere | 同一 resumeToken 在别处恢复连接，旧连接被踢 | 终止（顶号，不重连） |

客户端（js/network.js）把这些码视为 terminal：{1000,1008,1009,1013,4001}；其它关闭码（如
1006/1011）触发自动重连：延迟 = min(10000, 650 * 2^min(attempt,4)) + floor(rand*350) 毫秒，
attempt 从 0 开始，重连成功后清零；resumeToken/roomCode/serverUrl 存在 sessionStorage key
"texas-holdem-network-session-v2" 中，重连 onopen 后自动发 resume_session。
Rust 客户端（Phase 6）必须复刻同一套判定（poker-protocol 已提供 is_terminal_close_code 与
reconnect_delay_ms）。

## 8. 房间生命周期状态机

### 8.1 状态

开放房间只有两种状态：

    (不存在) --create_room/join_room(创建)--> Lobby
    Lobby --start_game 成功--> Playing(handId>=1)
    Playing --每手结束--> Playing(idle 手间等待; phase=idle + lastHandResult)
    Playing --所有人离开/断线且宽限到期且空房 TTL 到期--> (删除)
    Lobby --所有人离开/断线--> (立即删除)

Lobby 内不允许加入新人类当房间已有 game；开局后新连接只能通过 resume_session 回座。

### 8.2 房间对象字段（JS 实现参考）

    room = {
      code: "1".."5",
      config: RoomConfig,
      players: Player[],          // 按 seatId 排序
      hostId: string | null,
      game: ServerPokerGame | null,
      stateVersion: int,          // 每次权威变化 +1
      ready: Set<playerId>,       // 下一手准备集合
      aiTimer, cleanupTimer, nextHandTimer,
      createdAt: 时间戳
    }

Player：id、seatId、name、avatar、isHuman、connected、left、stack、holeCards、chipsInPot、
roundBet、folded、isAllIn、dealtIn、lastAction、ws、resumeToken、processed(<=64 条动作去重)、
joinedAt、disconnectTimer、hostTimer。

### 8.3 stateVersion / handId / turnId 语义

| 字段 | 递增时机 | 用途 |
| --- | --- | --- |
| room.stateVersion | join、resume、leave、断线、宽限到期、host 迁移、每次游戏 onChange | 客户端丢弃旧 game_state；player_action 的 expectedStateVersion |
| game.handId | 每次 startHand | 旧手动作拒绝、ready 信号匹配、UI 手数 |
| game.turnId | 每次 emitChange 与 finishHand | 旧回合动作拒绝、回合提示去重 |

### 8.4 定时器与超时（JS 默认值）

| 定时器 | 时长 | 触发 | 取消时机 |
| --- | --- | --- | --- |
| AI 行动延迟 | roomConfig.aiDelay（800/1800/3200；服务器选项兜底 450） | 当前行动者是 AI 时调度 AI 决策 | 再次 onChange 时重建；关闭/离开时清理 |
| 下一手延迟 | 250ms | 所有活跃人类 ready 后发牌 | 状态不再满足时清理 |
| 断线宽限 | 1800ms（可配置 100..60000） | 宽限内未 resume → left=true、fold、host 迁移、清 token | resume 成功、显式 leave |
| 空房 TTL | 15000ms | 有 game 且无已连接人类的房间删除 | 有人 resume/连接 |
| 心跳 | 15000ms | 对未回 pong 的连接 terminate | 关闭服务 |

注意：从未开过手的房间在所有人类离开/断线后立即删除（不等 TTL），释放房间号。

### 8.5 AI 行动队列

服务端 AI 决策是同步重计算（可能数十毫秒），JS 用单条串行队列（aiQueue + aiBusy + setImmediate）
避免并发重入：定时器到点后入队，执行前复核 game 身份、handId、turnId、currentPlayerIdx 未变，
再 chooseServerAiAction 并 act。Rust 实现必须保持串行（建议每房间一个专用任务/阻塞线程池），
且不得阻塞其他房间的连接处理。

### 8.6 房主迁移

新 host = 当前 players 中 isHuman && connected && !left 且 joinedAt 最早者；无则 hostId=null。
迁移时 stateVersion++，向所有已连接人类广播 host_changed（含 players），若有 game 再广播 game_state。
显式 leave 触发立即迁移；断线宽限到期触发迁移；断线瞬间不迁移（等待 resume）。

## 9. 配置白名单与名字规范化

### 9.1 sanitizeConfig 规则（自 local-server.js 逐字移植）

输入是 join/create 消息里的 8 个配置字段（任意 JSON 值）。输出权威 RoomConfig：

1. isShortDeck = JS 真值判断 (!!input.isShortDeck)。字符串 "false" 为真、0/""/null 为假。
2. startingCandidate = floor(Number(startingStack) || 20000)；若不在
   [5000,10000,20000,50000,100000] → 20000。
3. smallCandidate = floor(Number(smallBlind || ante) || 40)；
   bigCandidate = floor(Number(bigBlind || minBet) || 80)。
   两者必须整体命中盲注档位 [[10,20],[20,40],[40,80],[50,100],[100,200],[200,400]]，
   否则回落到 (40,80)。
4. ante = pair[0]、minBet = pair[1]（永远镜像选中的盲注档位，不取输入原始值）。
5. maxPlayers = max(2, min(短牌?8:10, floor(Number(maxPlayers) || (短牌?6:10))))。
6. aiDelay = floor(Number(aiDelay))；若属于 [800,1800,3200] 直接采用；
   否则 boundedNumber(serverFallback, 1800, 0, 10000, integer)（服务器选项兜底默认 450）。

JS 强制转换示例（golden 用例固定）：

| 输入 | 结果 |
| --- | --- |
| {} | maxPlayers=10, startingStack=20000, 盲注(40,80), aiDelay=fallback |
| {"startingStack":"5000","smallBlind":"10","bigBlind":"20","maxPlayers":"2"} | 数值字符串被接受 |
| {"startingStack":[],"smallBlind":[40],"bigBlind":[80],"aiDelay":[800]} | [] 视为 0 回落，[40]/[80]/[800] 视为数字 |
| {"startingStack":{},"smallBlind":{},"bigBlind":{},"aiDelay":{}} | 对象 → NaN → 全部回落 |
| {"isShortDeck":"false"} | 真（非空字符串）→ 短牌 |
| {"startingStack":20000.9,"smallBlind":40.9,"bigBlind":80.9,"aiDelay":800.9} | floor → 合法档位 |
| {"maxPlayers":-5,"startingStack":1e12,"smallBlind":1e9,"bigBlind":-1e9} | maxPlayers=2、筹码回落、盲注回落 |
| {"ante":40,"minBet":80,"smallBlind":0,"bigBlind":0} | 0 为假 → 取 ante/minBet |

### 9.2 字符串强制转换（JS String semantics）

Rust 的 normalize_name_value 复刻 String(value || "玩家")：
null/0/false/""/缺省 → "玩家"；数字 → 十进制字符串；true → "true"；
数组 → 逗号连接（null 元素为空串）；普通对象 → "[object Object]"。
DTO 层面 create_room/join_room 的 name 是 Option<String>，旧客户端只会发字符串；
非字符串值在 Rust 严格解析阶段会被拒绝（见 §12）。

### 9.3 normalizeName 规则

1. String(value || "玩家")。
2. slice(0, 256)（UTF-16 码元）。
3. Unicode NFKC（全角/兼容字符折叠，如 Ｔｏｍ→Tom、ﬁ→fi）。
4. 删除 U+0000..U+001F 与 U+007F（不删除 C1 控制符 U+0080..U+009F）。
5. JS trim()（Unicode WhiteSpace + LineTerminator，含 U+FEFF BOM）。
6. 若为空 → "玩家"。
7. 按 Unicode 码点取前 8 个。

golden 示例： "<b>Alice</b>" → "<b>Alice"（8 码点截断）；"玩家玩家玩家玩家玩家" → 前 4 个；
"😀"×9 → 前 8 个；"ＡＢＣ" → "ABC"；"😀" 按 1 个码点计。
已知差异：第 2 步按 UTF-16 切分，若 256 处切在星光面字符（emoji）中间会留下孤立代理项；
Rust String 不能表示孤立代理项，用 U+FFFD 代替。只有 >128 个 emoji 才会触发，旧客户端不可能发送。

## 10. 限制、安全与环境变量

### 10.1 服务端选项默认值与范围（boundedNumber 语义）

| 选项 | 默认 | 范围 | 说明 |
| --- | --- | --- | --- |
| host | 127.0.0.1 | — | 监听地址 |
| port | 3000 | — | 0 = 随机端口 |
| aiDelay | 450 | 0..10000 | 房间 aiDelay 非法时的兜底 |
| disconnectGraceMs | 1800 | 100..60000 | 断线宽限 |
| maxRooms | 3 | 1..5 | 同时存在房间数 |
| maxConnections | 24 | 4..100 | 总连接数 |
| maxConnectionsPerIp | 20 | 2..30 | 每 IP 连接数 |
| rateWindowMs | 10000 | 1000..60000 | 限流窗口 |
| maxMessagesPerWindow | 80 | 20..500 | 每连接每窗口消息数 |
| trustProxy | false | — | 信任 X-Forwarded-For |
| allowedOrigins | 空 | — | Origin 白名单 |

### 10.2 连接准入与限流

- 建立连接时若 connections.size >= maxConnections 或同 IP 连接数 >= maxConnectionsPerIp，
  立即 close(1013, "server capacity reached")，且不发送 hello。
- 每条消息先计长度：> 16384 字节 → close(1009)（实际由 ws maxPayload 层处理）。
- 每连接固定窗口计数：窗口过期重置；messageCount++ 后若超过 maxMessagesPerWindow →
  close(1008, "message rate exceeded")。限流在 JSON 解析之前执行。
- 其它所有异常都被捕获：JSON 解析失败 → error "消息格式无效"；handler 抛异常 →
  error "服务端处理失败，请重试"（并记录日志，绝不崩溃进程——JS 服务端与桌面 UI 同进程）。

### 10.3 Origin 白名单

允许：无 Origin、Origin 为 "null"、file:// 开头、allowedOrigins 精确匹配、或 Origin 的
hostname 是 localhost / 127.0.0.1 / ::1。其余拒绝。
拒绝发生在 WebSocket upgrade 前（ws 的 verifyClient），HTTP 状态 401（旧测试只断言 >=400）。
Rust/axum 需在 upgrade 提取阶段返回同样语义（401 或 403 均可，客户端只关心握手失败）。

### 10.4 环境变量

| 变量 | 作用 | 默认 |
| --- | --- | --- |
| POKER_HOST | 监听地址 | 127.0.0.1 |
| POKER_PORT | 监听端口 | 3000 |
| POKER_MAX_ROOMS | 房间数上限 | 3 |
| POKER_MAX_CONNECTIONS | 连接数上限 | 24 |
| POKER_MAX_CONNECTIONS_PER_IP | 每 IP 连接上限 | 20 |
| POKER_ALLOWED_ORIGINS | 逗号分隔 Origin 白名单 | 空 |
| POKER_TRUST_PROXY | 值为 "1" 时信任 X-Forwarded-For | 关闭 |

环境变量在 PokerRoomServer 构造时读取并受 boundedNumber 范围约束。
另建议 Rust 服务端提供 POKER_APP_VERSION（默认 1.5.8）用于 hello.appVersion，见 §11。

## 11. hello 与桌面端服务发现

原桌面端（main.js probePokerServer）启动时探测本地服务，超时 900ms，要求：

    hello.type === "hello"
    && hello.serverId === "texas-holdem-game"
    && hello.protocolVersion === 2   （数值严格相等）
    && hello.appVersion === APP_VERSION（本机版本，v1.5.8 为 "1.5.8"）

满足才复用已有服务；否则在 3000 被占用时改用随机端口自建。因此 Rust 服务端在兼容期必须报告
appVersion = "1.5.8"（poker-protocol 常量 COMPAT_APP_VERSION），否则旧桌面端会认为它不是
自家服务并另起一个。hello-probe-cases.jsonl 固定了 6 组接受/拒绝矩阵。
Phase 5 需要拍板：Rust 桌面端与 Rust 服务端同时上线后可去掉该兼容版本，或改探测逻辑。

## 12. 已知差异与兼容性风险

| # | 差异/风险 | 影响 | 处理 |
| --- | --- | --- | --- |
| 1 | Rust 解析比 JS 严格（JS 处处 Number()/String() 强转） | 旧客户端不受影响；异常客户端从 handler 级拒绝变为 INVALID_REQUEST | 已文档化；parse-errors golden 覆盖旧服务端返回语义；Rust 逐字保留 4 类 parse 错误文本 |
| 2 | 1009 reason 为空（ws 层先拦截） | 客户端只看 code | Rust 侧设置 16 KiB 帧上限即可；不要依赖 reason |
| 3 | Origin 拒绝状态 401（ws verifyClient 默认） | 握手失败即可 | 测试按 >=400/401 断言 |
| 4 | player_disconnected 死分支 | 无 | Rust 服务端不发；Rust 客户端按未知类型忽略 |
| 5 | get_snapshot 未文档化但服务端支持 | 部分调试客户端依赖 | Rust 服务端保留实现 |
| 6 | appVersion 兼容 | 旧桌面端服务发现 | 默认报告 1.5.8；Phase 5 决定长期策略 |
| 7 | 服务端 AI 与 JS AI 决策不逐手一致 | AI 强度/风格可能不同 | 协议不受影响；AI 验收在 poker-ai 阶段 |
| 8 | AI 决策耗时阻塞风险 | 一个房间的 AI 计算不能卡住 tokio reactor | 串行队列 + spawn_blocking / 专用任务；每房间状态隔离 |
| 9 | 浮点 | 协议无浮点；权益面板是本地计算 | 无需处理 |
| 10 | 中文错误文本逐字一致 | 客户端直接展示 | Rust 用 UTF-8 常量；golden 测试已锁定 |
| 11 | create_room 失败错误码与 join 不存在房间不同（INVALID_REQUEST vs ROOM_LIMIT） | 客户端提示一致 | 照抄 |
| 12 | stateVersion 只增不去重广播（同一版本可能重复） | 客户端按 <= 丢弃 | Rust 服务端可在广播前过滤重复版本（可选优化），但语义必须单调 |

## 13. 黄金语料与验证

### 13.1 文件清单（tests/golden/protocol/）

| 文件 | 条数 | 内容 |
| --- | --- | --- |
| s2c-messages.jsonl | 153 | 真实 JS 服务端发出的去重消息样本（覆盖全部 12 种），含正常/错误/牌局/短牌/重连 |
| c2s-messages.jsonl | 62 | 旧客户端形状发出的消息（覆盖全部 8 种） |
| parse-errors.jsonl | 18 | 恶意/畸形原始载荷 → JS 服务端真实错误回复（含 JS Number 强转矩阵） |
| config-cases.jsonl | 60 | sanitizeConfig 输入/兜底/输出矩阵 |
| name-cases.jsonl | 30 | normalizeName 字符串与非字符串输入矩阵 |
| close-events.jsonl | 5 | 1008/1009/1013/4001 关闭事件与 Origin 拒绝 |
| hello-probe-cases.jsonl | 6 | 桌面端 hello 探测接受/拒绝矩阵 |
| server-constants.json | — | 协议版本、常量、白名单、服务端默认值 |
| client-behaviour.json | — | 客户端 terminal 关闭码、重连退避、session key、requestId 前缀 |
| meta.json | — | 条数统计、seed、参考 commit、Node 版本 |

### 13.2 生成与确定性

    node oracle/golden-protocol.js              # 写入 tests/golden/protocol/
    node oracle/golden-protocol.js --out DIR    # 写入指定目录（用于重复性校验）

脚本在加载参考模块前 patch Math.random（mulberry32 固定种子）、performance.now（固定 0）、
crypto.randomUUID / crypto.randomBytes（固定序列），并对房间号/座位/消息做去重。同一机器
连续运行输出逐字节一致（本项目已验证 4 次运行 sha256 相同）。语料重新生成后必须同步
meta.json 条数与本文档。

### 13.3 Rust 测试

crates/poker-protocol/tests/golden_protocol.rs（9 项）+ tests/behaviour.rs（14 项）：

- s2c / c2s roundtrip：真实消息 → DTO → 序列化 → serde_json::Value 深比较。
  键顺序无关，但字段集合、JSON 类型、null、缺省键必须完全一致；未知字段会导致比较失败，
  缺字段会导致反序列化失败，因此该测试等价于“字段逐项一致”。
- parse-errors：原始载荷 → parse_client_message → 错误 DTO 序列化 → 与 JS 服务端实际回复
  逐字段相等（含 protocolVersion/requestId 回显）。
- config / name / hello-probe / close-events / client-behaviour / server-constants：
  常量与行为逐项比对。

本地验证命令（不要与其他 agent 抢 target 目录）：

    export CARGO_TARGET_DIR=/tmp/target-protocol
    cargo test -p poker-protocol --offline
    cargo clippy -p poker-protocol --offline --all-targets -- -D warnings
    cargo fmt -p poker-protocol -- --check

## 14. Phase 5 互操作验收清单

用原 JS 客户端 / 原 JS 服务端与 Rust 实现交叉连接，逐项通过：

1. hello：旧桌面端 probe 能识别 Rust 服务端（serverId/protocolVersion/appVersion 全等）。
2. 大厅：create_room、join_room、player_joined、player_left、host_changed、房间号与满房错误。
3. 开局：start_game 权限与错误、game_starting、座位补齐（AI）、shortdeck 配置。
4. 牌局：game_state 隐私（players 不含 holeCards）、isYourTurn 个性化、legalActions、
   action_ack accepted/rejected、重复 requestId 重放。
5. 结算：showdown/fold 的 lastHandResult（含 RevealedCard.value、winnerAmounts）、
   ready_status 两种形态、250ms 后下一手。
6. 断线重连：宽限内 resume_session 恢复手牌/座位；宽限外 fold + left + token 失效；
   同 token 顶号旧连接收到 4001。
7. 房主迁移：房主 leave/断线后 host_changed 到最早加入的在线人类；无人时 hostId=null。
8. 空房 TTL：有 game 的房间 15s 后释放房间号；未开局房间立即释放。
9. 安全：非法 JSON、缺 type、未知 type、版本不符、超长帧（1009）、超频（1008）、
   超容量（1013）、非法 Origin（握手失败）、错误房间/手/回合/版本的 action 拒绝。
10. 环境变量：7 个 POKER_* 变量生效且受范围约束。
11. 兼容方向二：Rust 客户端（用 poker-protocol 的 ServerMessage 解析）连原 JS 服务端，
    完成创建/加入/开局/行动/结算/重连/房主迁移全流程。
