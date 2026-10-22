# 双向互通矩阵（Phase 5 验收）

> task-16 交付（2026-10-23）。协议权威规格见 docs/specs/protocol-v2.md；服务端状态见
> docs/specs/server-plan.md §15/§16。所有互通测试都跑真实 WebSocket，不使用 mock 帧。

## 1. 矩阵

| 方向 | 驱动方式 | 覆盖 | 运行命令 |
| --- | --- | --- | --- |
| 原 JS 协议客户端 → Rust 服务端 | oracle/interop-js-client.js（参考仓库 ws，JSON 帧） | create→join→start→showdown→ready→下一手→断线 resume→顶号 4001→房主迁移→AI 补位→leave | cargo test -p poker-server --test interop |
| Rust 客户端 → 原 JS 服务端 | oracle/interop-rust-client.js 启动 reference server；Rust tokio-tungstenite + poker-protocol DTO | hello/probe/create/start/action/settlement/ready/下一手/leave | 同上（同一测试文件第二个测试） |
| Rust 客户端 → Rust 服务端 | poker-client 公开 Client API（与桌面端同一条路径） | 全流程 + 手动 reconnect/resume + 4001 + 指标断言 | cargo test -p poker-server --test rust_interop |
| poker-client ignored 冒烟测试 → Rust 服务端 example | oracle/interop-rust-rust.sh 启动 examples/serve（端口 0）并注入 POKER_INTEROP_URL | create/join/start 冒烟 + 断线 resume 冒烟 | ./oracle/interop-rust-rust.sh |

环境要求：前两行需要 Node + reference 仓库 node_modules/ws（缺失时测试打印 skip 并返回）；
后两行只需 Rust 工具链。

## 2. 证据文件

- tests/golden/server/interop-js-transcript.json：JS 客户端 → Rust 服务端完整 transcript（id/token 脱敏）。
- tests/golden/server/interop-summary.json：11 项 check、手 1 showdown（pot 160）、手 2 fold（pot 80）、
  4001 关闭码与消息类型清单。
- 本次收尾运行：poker-client ignored 冒烟 2/2 通过（create/join/start、resume）；
  Rust↔Rust 全流程 1/1 通过（含 4001 与 metrics 断言）。

## 3. protocol-v2.md §14 验收清单状态

| # | 项 | 状态 |
| --- | --- | --- |
| 1 | hello / 旧桌面端 probe | 完成（GOLDEN hello-probe + probe_server + 双向互通） |
| 2 | 大厅 create/join/leave/players/hostChanged/错误文本 | 完成 |
| 3 | 开局 start_game/错误/座位补齐/短牌配置 | 完成 |
| 4 | 牌局隐私/isYourTurn/legalActions/action_ack/重复 requestId | 完成 |
| 5 | 结算 showdown|fold、ready 两种形态、250ms 下一手 | 完成 |
| 6 | 断线 resume/宽限 fold/顶号 4001 | 完成（lobby 宽限保留差异见 §4） |
| 7 | 房主迁移 | 完成 |
| 8 | 空房 TTL 释放房间号 | 完成（15s 默认，测试可配） |
| 9 | 安全：非法 JSON/type/版本/超长 1009/超频 1008/容量 1013/Origin/错误动作 | 完成 |
| 10 | 环境变量 7 项生效且受范围约束 | 完成（from_env + options 单测） |
| 11 | Rust 客户端 ↔ 原 JS 服务端 | 完成 |

## 4. 与参考实现的已知差异

1. **大厅断线宽限保留**：参考 JS 在无牌局房间最后一人断线时立即删房，导致 lobby 里
   reconnect/resume 存在竞态；Rust 服务端保留到 1800ms 宽限计时器结束（便于 UI 刷新/重连）。
   房间号释放最多延后一个宽限窗口。
2. **慢消费者**：参考实现没有该路径（ws 本地缓冲）。Rust 使用容量 256 的有界出站队列，
   队列满时标记连接失败并由写任务直接关闭 socket（客户端看到 1006），计数 slow_consumers。
3. **心跳**：15s ping，未在本轮 ping 前回复 pong 的连接在下一次 tick 被断开（参考实现 terminate
   语义相同；关闭帧尽力发送，客户端可能只看到 1006）。
4. 1009 reason 文本、Rust 严格解析差异见 protocol-v2.md §12。

## 5. 剩余 TODO

- 真实 AI 工厂：docs/specs/ai-server-adapter.md 接口已冻结；PokerAiFactory + observe 契约
  （record_opponent_action/record_showdown）待 poker-ai baseline 达标后按 feature 门控接入并跑
  1-2 手延迟验证（Lead 通知后另开任务）。
