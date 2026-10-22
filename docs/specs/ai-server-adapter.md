# AI Server / App 适配契约（poker-ai 接入）

> task-9 交付（2026-10-23）。冻结接口见 crates/poker-ai/src/{lib,input,adapter}.rs，行为规格见 docs/specs/ai-strategy.md。
> 面向前端/服务端接入方（protocol-dev、app-spike）。原则：AI 只接收公开信息 + 自己的底牌；宿主负责构造上下文、广播观察事件、执行动作。

## 1. 冻结的公共接口（Rust）

~~~rust
pub struct AIPlayer;

impl AIPlayer {
    pub fn new(name: &str, style: Style, starting_stack: f64, seat_id: usize,
               variant: poker_core::Variant, source: ProfileSource) -> AIPlayer;
    pub fn set_hole_cards(&mut self, cards: &[poker_core::Card]);
    pub fn decide(&mut self, context: &DecisionContext) -> DecisionWithTrace;
    pub fn record_opponent_action(&mut self, event: &ObservedAction);
    pub fn record_showdown(&mut self, seat: usize, result: &ShowdownResult);
    pub fn begin_hand(&mut self, hand_id: u64);
    pub fn last_trace(&self) -> Option<&DecisionTrace>;
}

pub struct DecisionWithTrace {
    pub action: poker_engine::ActionType, // Fold/Check/Call/Raise/AllIn
    pub amount: u64,                      // Raise=加注到总额; Call=跟注额; Fold/Check=0; AllIn=剩余筹码
    pub trace: DecisionTrace,
}
~~~

- Style: StandardStyle 使用 STYLE_CONFIGS；短牌统一 ShortDeckLocalStyle（SD_STYLE_CONFIGS，已决策）；ShortDeckServerStyle 仅回归对照。
- Style 枚举：Tag/Lag/Solid/Maniac/Tagfish/CallingStation；Style::from_name("TAG"|"LAG"|"SOLID"|"MANIAC"|"TAGFISH"|"CALLING_STATION")。
- AIPlayer 是纯值类型（无线程/定时器）；server 每座位长期持有一个实例（跨手保留 BehavioralModel/SelfImage），decide 放到 spawn_blocking/专用线程，不要跨座位共享。

## 2. AiView -> RawDecisionContext（server/app 通用）

宿主构造 poker_ai::input::RawDecisionContext（serde camelCase，字段可选默认），再 .normalize() 得到 DecisionContext。

| Raw 字段 | 来源 | 说明 |
| --- | --- | --- |
| handId | 当前手牌编号 | 每手递增；变化触发 beginHand |
| decisionId | poker-engine decision_identity(variant, phase, actionsThisRound, seat) | 影响 RNG 种子，必须与引擎一致 |
| seed | "poker-table:standard" / "poker-table:shortdeck" | 表级固定串 |
| variant | "standard" / "shortdeck" | |
| tablePlayerCount | 本桌座位总数 | 仅 standard+10+5000+100/200+unit100 启用 target 策略（含 CFR） |
| startingStack | 本桌初始筹码 | 同上 |
| heroSeat | 当前行动座位 | |
| holeCards | 该 AI 自己的两张底牌 | 只传自己的 |
| communityCards | 公共牌 0/3/4/5 张 | |
| pot / currentBet | 底池 / 本街最高注 | |
| toCall | max(0, currentBet - myRoundBet) 且已 min(myStack) | 与 engine LegalActions.toCall 一致 |
| yourBet / heroRoundBet | 自己本街已投入 | |
| stack / heroStack | 自己剩余筹码 | |
| effectiveStack | min(myStack, max(激活对手 stack)) | |
| minRaiseTo / maxRaiseTo | engine 合法加注范围 | maxRaiseTo = myRoundBet + myStack |
| canRaise / canCheck | engine 合法动作标志 | |
| legalActions | 字符串数组，或 {actions,toCall,canRaise,minRaiseTo,maxRaiseTo,wagerUnit} | 顺序与 engine LegalActions.actions 一致 |
| activeOpponentSeats | 未弃牌/未离桌对手座位 | |
| numOpponentsActive / playerCount | 对手数 / 本手参与人数 | |
| positionFromButton | 距庄家顺时针距离（BTN=0,SB=1,BB=2,...） | 短牌庄家=0 |
| isSmallBlind / isBigBlind | 盲注标记（短牌恒 false） | |
| preflopRaiseCount | 翻前加注次数 | |
| isPreviousStreetAggressor / isDelayedCBetCandidate | 公开行动状态 | |
| bigBlind | 标准=大盲；短牌=minBet(2x ante) | |
| wagerUnit | 标准=小盲；短牌=ante | 加注网格单位 |
| phase | "preflop"/"flop"/"turn"/"river"（可省） | normalized 优先 street，否则按公共牌数推导 |
| timeBudgetMs | 150（river 或 >=4 对手）否则 76/80 | 仅安全阀；Rust 固定按样本数，不因时间截断 |
| publicPlayers | 全部座位: {seat, stack, committed(整手总投入), roundBet(本街投入), positionFromButton?, isSmallBlind, isBigBlind, folded, allIn} | 必须含已弃牌/淘汰座位；CFR 门控与底池层级依赖它 |

默认值（normalize 内）：bigBlind=80、wagerUnit=floor(bigBlind/2)（建议显式传）、timeBudget=150/76、numOpponents=activeOpponentSeats.len().max(1)、playerCount=opponents+1（最小 2）。

最小构造示例（伪代码）：

~~~rust
let raw = RawDecisionContext {
    hand_id: view.hand_id,
    decision_id: engine::decision_identity(variant, &view.phase_name, view.actions_this_round, view.seat),
    seed: Some(format!("poker-table:{}", if view.variant.is_short_deck() { "shortdeck" } else { "standard" })),
    variant: Some(if view.variant.is_short_deck() { "shortdeck" } else { "standard" }.to_owned()),
    hero_seat: Some(view.seat),
    hole_cards: view.hole_cards.clone(),
    community_cards: view.board.clone(),
    pot: view.pot as f64, current_bet: view.current_bet as f64,
    to_call: Some(view.to_call as f64), hero_round_bet: Some(view.round_bet as f64),
    stack: Some(view.stack as f64), hero_stack: Some(view.stack as f64),
    effective_stack: Some(view.effective_stack as f64),
    min_raise_to: Some(view.legal.min_raise_to as f64),
    max_raise_to: Some(view.legal.max_raise_to as f64),
    can_raise: Some(view.legal.can_raise),
    can_check: Some(view.legal.actions.contains(&ActionType::Check)),
    legal_actions: Some(RawLegalActions::List(view.legal.actions.clone())),
    active_opponent_seats: view.live_seats.iter().copied().map(SeatRef::Number).collect(),
    num_opponents_active: Some(view.live_seats.len()),
    player_count: Some(view.dealt_in_count), table_player_count: Some(view.table_seats),
    starting_stack: Some(view.starting_stack as f64),
    position_from_button: Some(view.position_from_button),
    is_small_blind: view.is_small_blind, is_big_blind: view.is_big_blind,
    preflop_raise_count: Some(view.preflop_raise_count),
    is_previous_street_aggressor: view.is_previous_street_aggressor,
    is_delayed_cbet_candidate: view.is_delayed_cbet_candidate,
    big_blind: Some(view.big_blind as f64), wager_unit: Some(view.wager_unit as f64),
    phase: Some(view.phase_name.clone()),
    public_players: view.players.iter().map(|p| PublicPlayer {
        seat: p.seat, stack: p.stack as f64, committed: p.committed as f64,
        round_bet: p.round_bet as f64, position_from_button: p.position,
        is_small_blind: p.is_small_blind, is_big_blind: p.is_big_blind,
        folded: p.folded, all_in: p.all_in,
    }).collect(),
    ..RawDecisionContext::default()
};
let decision = ai.decide(&raw.normalize());
~~~

## 3. 观察事件契约（observe）

每次任何座位行动后（包括 AI 自己），宿主把同一份 ObservedAction 广播给本桌所有 AI：

~~~rust
pub struct ObservedAction {
    pub seat_id: usize,
    pub action: String,   // 小写 "fold"/"check"/"call"/"raise"/"bet"/"allin"（blind/ante 被忽略）
    pub street: Street,   // Preflop/Flop/Turn/River
    pub hand_id: u64,
    pub meta: ObservedActionMeta,
}
~~~

ObservedActionMeta 必须由 poker_engine::rules::build_action_meta 等价语义生成（engine 与 AI 的接口契约，详见 ai-strategy.md 5.3）：
to_call_before/call_cost、additional_paid/amount、raise_to、current_bet_before、pot_before/pot_after、minimum_bet、aggressive（是否提高 currentBet）、
bet_fraction = (aggressive ? raise_to-current_bet_before : additional_paid) / max(1, minimum_bet, pot_before)（高风险点）、
preflop_raise_count_before、positionFromButton、playerCount、isSmallBlind/isBigBlind、communityCards（行动时公共牌拷贝）、可选 faced3bet/facedCbet/isCbetOpportunity。

ShowdownResult { won, hand_rank, was_bluff, was_trap }：只对实际亮牌的座位广播 record_showdown；弃牌获胜不亮牌时不要广播（信息边界）。
wasBluff/wasTrap 判定沿用原服务器：aggressive && 输 && rank<=2；passive && 赢 && rank>=4。

顺序：行动事件必须在下一个 decide 之前送达所有 AI。盲注/前注不广播（保持与黄金语料一致）。

## 4. 执行与异常

1) set_hole_cards（每手或手牌变化时）；2) decide(&context)；3) engine 校验/执行 decision.action/decision.amount；
4) 只做动作名映射，不改金额（AI legalize 已对齐网格）；5) last_trace() 供调试面板，不要每帧序列化；
6) 同步纯计算，建议 spawn_blocking + 外层超时；超时回退 fold（toCall>0）/check（toCall==0）并告警。

## 5. server / app 职责

| 职责 | server | app |
| --- | --- | --- |
| 每座位长期持有 AIPlayer | 是 | 是 |
| 构造 RawDecisionContext | 是 | 是 |
| 广播 ObservedAction（build_action_meta 语义） | 是 | engine/notifyAIsOfAction 映射 |
| 执行动作/结算/淘汰 | engine | engine |
| 协议/本地 AI 延迟 | 800/1800/3200ms 调度 | 本地延迟（与 AI 计算时间无关） |
| trace/调试面板 | 日志 | UI 分析面板 |

短牌：variant=ShortDeck + ShortDeckLocalStyle；bigBlind=minBet、wagerUnit=ante、位置从庄家算、isSB/isBB 恒 false、committed 为前注累计。

信息边界：RawDecisionContext/ObservedAction 不能出现对手未亮底牌、未发牌堆、真实人格参数或内部计划；AI 的 equity 采样自行重建未知牌。

## 6. 接入自检

1) observes 顺序/内容可用 tests/golden/ai/*.jsonl 的 observes 做单测；2) meta 逐字段与 golden meta 对比（betFraction 分母是高风险点）；
3) decisionIdentity 用 engine 函数（改动会改变 RNG 种子）；4) 三路径 profile_source/variant 显式选择，不要隐式默认。
