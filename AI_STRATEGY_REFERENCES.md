# 德州扑克开源策略调研

查阅日期：2026-10-08。目标是用程序算法实现机器人下注，不加载神经网络或训练模型。现有游戏已经采用 JavaScript 牌力计算、对手范围、概率采样和收益比较；v1.5.6 已沿这条路线升级并完成离线对照测试。以下结论来自项目 README；仓库公开不代表其棋力已被独立验证。

## 优先参考的纯算法项目

| 项目 | 可参考内容 | 局限和采用方式 |
| --- | --- | --- |
| [thotbreakerr/Texas-Holdem-AI](https://github.com/thotbreakerr/Texas-Holdem-AI) | 当前公开代码有 Monte Carlo、启发式、对手统计、概率范围机器人及多人评测工具。重点源码：[opponent_model_bot.py](https://github.com/thotbreakerr/Texas-Holdem-AI/blob/main/bots/opponent_model_bot.py)、[exploitative_bot.py](https://github.com/thotbreakerr/Texas-Holdem-AI/blob/main/bots/exploitative_bot.py) | 最贴近本项目的结构参考。概率范围机器人将牌力分成五档，按行动似然更新，再按范围计算权益；其部分决策仍使用固定阈值，不能直接当作最优策略。这里的对手“模型”是统计分布，非神经网络。评估器查表约 46 MB，整套 Python/Numpy 与锦标赛 ICM 不直接接入桌面 JS。README 的最强等描述未在本项目验证。 |
| [HenryRLee/PokerHandEvaluator](https://github.com/HenryRLee/PokerHandEvaluator)、[thlorenz/phe](https://github.com/thlorenz/phe) | [完美哈希算法说明](https://github.com/HenryRLee/PokerHandEvaluator/blob/develop/Documentation/Algorithm.md)，以及 JavaScript 移植，帮助快速比较 5～7 张牌 | 它提高牌力计算速度，不直接决定下注、也不保证棋力。本项目 `probability-core.js` 已有直接 5～7 张牌评估，未枚举 21 个五张组合，不能假设换库就一定提速。先做等价测试与 Worker 内基准；标准德州排序不能直接用于短牌。 |
| [rosbo/texas-holdem-poker-ai](https://github.com/rosbo/texas-holdem-poker-ai) | Java 实现，从手牌强度到翻前模拟，再到对手行为统计；包含偏诈唬和理性两类策略 | 算法结构符合本项目需求，参考其拆分方法，在共享 JS 核心独立实现。它包含离线模拟和统计工具，但没有要求用神经网络决定下注。README 未提供可用于证明本游戏棋力提升的配对基准。MIT。 |
| [mdp/JsPoker](https://github.com/mdp/JsPoker) | JavaScript 机器人比赛框架，下注接口、机器人示例和赛局工具 | 与本项目语言一致，适合作为规则策略与基准组织方式的参考。README 的 Node 0.10 运行示例较旧，示例机器人不能直接视为强策略。本项目继续使用自己的规则、短牌支持和下注网格。 |
| [ayushmgarg/Poker-AI](https://github.com/ayushmgarg/Poker-AI) | 规则阈值、Monte Carlo、MCTS、Expectiminimax 四种经典算法，README 明确为非机器学习路线 | 可比较算法思路。README 把多人支持列为未来功能；公开的耗时与棋力描述只是作者说明，不能直接用于本游戏性能或棋力结论。普通 MCTS 若未正确处理隐藏信息也不能作为德州策略保证。 |

## v1.5.6 的实际采用方式

没有直接搬运仓库源码、46 MB 查表或训练模型，也没有增加 Python/神经网络运行依赖。游戏继续使用自己的标准德州、短牌规则和共享 JavaScript 核心，独立实现：

1. 每个对手按行动当时的公共牌更新组合概率，区分成牌、听牌、价格和阻断牌。未行动玩家保持未知分布，全下跟注记作跟注，避免范围错误变强。
2. 按加权范围抽样权益；河牌单挑枚举所有合法组合。区分抽样标准误差与对手范围不确定性，枚举精确不代表猜中的范围精确。
3. 听牌需要本方底牌参与且仍有未来公共牌；区分 A/K 同花阻断与成牌，处理短牌低顺子。
4. 按合法金额生成开池、再加注和翻后尺度；整额量化后才评价收益。跟注者筹码限制价值注，已全下玩家不能被诈唬弃牌。
5. 互斥计算全弃、有人跟注且无人再加注、有人再加注。边池按投入上限和资格分层，小筹码不能赢取自己无资格的上层底池。
6. 六种性格保留不同风险、激进度、诈唬和适应参数；本地 Worker 与联网调用同一个决策核心，只看本方底牌及公开信息。
7. 固定种子、交换队伍座位、轮换庄家，分别评估旧版及紧手、跟注、持续施压三类规则对手。记录候选 EV、公开行动、性格频率、计算时间、错误和按种子聚类的盈利区间。

范围似然强度、分街回应、起手牌百分位范围、实际可能跟注组合权益等改法均进入参数对照。后几项在本轮调参没有胜过保留方案，默认关闭；它们留作可复现实验开关，不宣称已经提高默认棋力。工具不会悄悄改写游戏或无限运行，候选需要独立种子验收后才可人工启用。

标准德州与短牌分开验证：最终标准德州使用 `rangeEvidenceWeight=0.45`、`calledRangeDiscount=0.08`，短牌保留 `0.70/0.055`。标准德州两组独立种子共 65,536 手，相对未调参 v2 的配对提升区间为 +7.54～+19.32 BB/100，达到预设启用条件；不能用短牌的收益代替这个验证。v1.5.6 的后续调参工具也改为分规则提名、独立验证与第二种子确认。

### 真人牌手的思路如何转为代码

[PokerStars 的多人底池说明](https://www.pokerstars.com/poker/learn/strategies/a-guide-to-multiway-pots/)强调，多人情况下薄价值和诈唬需要更谨慎，强听牌与可靠成牌更重要。本版在多人范围权益、反加注风险、多人跟注折扣和短牌听牌识别中体现这些原则，而不是把一篇文章中的下注比例硬套到所有牌桌。

[Upswing 的多人底池策略](https://upswingpoker.com/multi-way-pots-strategies-tips/)和[阻断牌示例](https://upswingpoker.com/blockers-poker-card-removal-situations/)用于参考思路：公共牌、对手继续范围、底池人数和阻断组合需要合起来考虑；持有阻断牌不能保证对手没有强牌，爱跟注的对手也不能仅凭阻断牌频繁诈唬。这是启发式设计依据，效果仍以本游戏对照数据为准。

完整测试量、收益区间和 CPU/内存代价见 [AI_BENCHMARK_REPORT_v1.5.6.md](AI_BENCHMARK_REPORT_v1.5.6.md)。有限对手基准不等于真人、职业棋力或多人 GTO 保证。

## 求解与学习框架：保留作背景资料，不作为接入方案

| 项目 | 可参考内容 | 对本项目的建议 |
| --- | --- | --- |
| [TexasSolver](https://github.com/bupticybee/TexasSolver) | C++ 德州和短牌求解器；可配置下注树、通过控制台求解并导出 JSON 策略 | 优先用于离线分析特定单挑翻后局面、校准下注尺度和混合频率。当前游戏支持多人，不能把单挑结果直接当作多人均衡。仓库标注 AGPL-3.0，README 对分发和商业整合另有说明；直接集成前需核实授权范围。 |
| [OpenSpiel](https://github.com/google-deepmind/open_spiel) | 不完全信息博弈框架；[算法文档](https://github.com/google-deepmind/open_spiel/blob/master/docs/algorithms.md)列出 CFR、MCCFR、Deep CFR、NFSP、最佳回应等 | 适合搭建独立训练、策略验证与对手基准；C++/Python 核心，不能直接替换浏览器 JS 的 `AIPlayer`。Apache-2.0。 |
| [RLCard](https://github.com/datamllab/rlcard) | 卡牌强化学习环境，含德州环境及 CFR、DQN、NFSP 示例 | 适合快速搭建训练和评测原型。注意其无限注德州行动空间采用抽象菜单，CFR 示例训练的是简化 Leduc，不能把示例模型当作完整德州强 AI。MIT。 |
| [deepcfr-texas-no-limit-holdem-6-players](https://github.com/dberweger2017/deepcfr-texas-no-limit-holdem-6-players) | 当前 README 描述 v0.5 研究路线，以及受限 20 BB 单挑/三人学习评测 | 项目明确说明六人 100 BB 目标尚未实现，也未证明达到职业级棋力。可以参考评测组织方式，学习策略接入不在本项目当前范围内。MIT。 |

## v1.5.8 补充核查

复查 [dickreuter/Poker 的下注代码](https://github.com/dickreuter/Poker/blob/master/poker/decisionmaker/decisionmaker.py) 与上述概率对手机器人：这些程序结构有参考价值，但外部 README 的强度标签不能替代本游戏的多人评测。没有直接接入它的桌面识别、平台自动点击或神经网络模块。

[GTO Wizard 的权益实现说明](https://blog.gtowizard.com/equity-realization/) 区分了摊牌权益、后续下注可实现收益与全下状态。它帮助审计未来权益折扣；数学修正本身仍须对照。四倍采样候选在共享未知牌范围中降低抽样噪声，所有尺寸共用样本，不读取真实牌堆，也不是训练一个下注模型。方案选择、两次失败独立确认和新确认记录见 [v1.5.8 报告](AI_BENCHMARK_REPORT_v1.5.8.md)。

## 后续改进的边界

目前仍采用近似对手范围和反应概率，未来街价值是启发式；顺序加权抽样打乱座位顺序以减小偏差，但不是严格联合范围求解。更复杂的搜索只有在控制 CPU 预算、处理隐藏信息并通过独立验收后才应启用。

Linux 测试只调试代码、参数和评测方法，不训练神经网络、不要求 GPU。短牌收益与标准德州分别评价，不能互相替代。没有将任何外部求解器或学习模型装入游戏。

## 用户提供的两个仓库：源码核查（2026-10-08）

固定源码版本：OpenSpiel `48401890ee9857e611678302371378175a8e4c6b`，luckyone18/Poker `c3e4e055ecdcdf40183201ad368fbd7f1e0603d7`。查阅了算法和牌局实现，而非仅根据 README 的 AI 名称作判断；所读文件的 SHA-256 清单保存在调研快照中。

### OpenSpiel 可以提供无需神经网络的算法

[表格式算法](https://github.com/google-deepmind/open_spiel/blob/48401890ee9857e611678302371378175a8e4c6b/docs/algorithms.md)包括 CFR、external/outcome sampling MCCFR、最佳回应与不完全信息搜索。`universal_poker` 的源码声明支持 2～10 人；默认 `fcpa` 将下注简化成弃牌、过牌/跟注、满池、全下，也支持更完整的金额空间。支持十人牌局不代表框架附带一个已经求解的十人 25 BB 强策略。

[outcome_sampling_mccfr.py](https://github.com/google-deepmind/open_spiel/blob/48401890ee9857e611678302371378175a8e4c6b/open_spiel/python/algorithms/outcome_sampling_mccfr.py)按行动者可见的 `information_state_string` 保存信息集，并处理机会、对手到达概率与抽样概率的校正；[external_sampling_mccfr.py](https://github.com/google-deepmind/open_spiel/blob/48401890ee9857e611678302371378175a8e4c6b/open_spiel/python/algorithms/external_sampling_mccfr.py)还明确区分多人策略平均的实现。此类离线迭代可产出概率表，运行时查表或局部求解，不需要神经网络。与“对固定单步 EV 反复做 regret matching”不同，它遍历真实的后续行动。

对本项目最有价值的参考是：用公开信息集组织搜索、在所有合法后续分支计算净筹码收益、正确处理概率权重，以及用独立对手群验证。不能直接把 Python 框架装入 Electron 当作已经增强，也不能将单挑/简化牌局的收敛结论直接套到十人无限注。

### luckyone18/Poker 更接近可读的程序 Bot，但需要核查近似

重点读了 `gto_bot.py`、`opponent_model_bot.py`、`exploitative_bot.py`、`monte_carlo_bot.py`、`cfr_bot.py`、`core/bot_api.py` 与评测脚本。位置范围、公开行为跟踪、混合决策与概率范围等组织方式适合参考，但本项目已有相应机制，不能仅换名称当升级。

- [GTOBot](https://github.com/luckyone18/Poker/blob/c3e4e055ecdcdf40183201ad368fbd7f1e0603d7/bots/gto_bot.py)以位置范围、固定阈值、固定随机频率决策，并非完整均衡求解。例如对弱河牌按 33% 频率诈唬，并不等于整段下注范围中的价值/诈唬组合真的满足 2:1；需要考虑弱牌和价值牌的组合数量及实际下注尺寸。
- [CFRBot](https://github.com/luckyone18/Poker/blob/c3e4e055ecdcdf40183201ad368fbd7f1e0603d7/bots/cfr_bot.py)在 `_run_iterations` 明确使用一步终局近似，其跟注价值为 `equity - 0.5`，没有使用当前价格。30% 权益面对 900 底池、100 跟注成本，正确筹码 EV 是 `1000×.30−100=200`，这一近似却给出负值。重复更新 regret 并不能消除收益函数本身的误差。
- [OpponentModelBot](https://github.com/luckyone18/Poker/blob/c3e4e055ecdcdf40183201ad368fbd7f1e0603d7/bots/opponent_model_bot.py)使用五档范围，并在下一手保留上一手范围的 85%。玩家行为统计可以跨手保留，但重新发牌后上一手的具体持牌证据应重置；不能将两种信息混合。本项目已分别保存跨手行为模型和每手持牌范围。

结论：参考 OpenSpiel 的真实信息集与收益遍历方法；借鉴 Poker 仓库的模块化和评测组织，避免搬入上述价格、组合比例和范围生命周期近似。是否提升本游戏强度仍须独立验证，第四轮候选确认未通过；随后独立实现的河牌子博弈候选通过了四个新种子族的 16384 手确认，提升 +1.79 BB/100（95% 区间 +1.13～+2.46），已在目标配置默认启用。
