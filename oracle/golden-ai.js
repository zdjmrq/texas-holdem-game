'use strict';
/**
 * Exports the fixed-seed AI decision golden corpus from the original JS AI (v1.5.8).
 *
 * The oracle is the reference implementation in reference/texas-holdem-game
 * (commit e0c6c6f). Nothing here changes that repository.
 *
 * What it does:
 *   1. Patches Math.random with oracle/lib/prng mulberry32 and installs a
 *      virtual clock (performance.now() === 0) inside the vm sandbox so the
 *      AI's wall-clock emergency brake can never make a run non-reproducible.
 *   2. Loads the browser modules in index.html order (including
 *      compiled-cfr.js and river-solver.js) so the river CFR solver is active,
 *      exactly like the desktop app.
 *   3. Drives the authoritative server betting loop (ServerPokerGame +
 *      chooseServerAiAction) with all-AI tables and records, for every
 *      decision, the public game state, the observation events the AI received
 *      and the JS decision + trace.
 *   4. Adds synthetic boundary cases (cannot raise, terminal all-in call,
 *      short stack, multiway, style sweep) and river CFR cases.
 *   5. Adds two-seat decision trajectories that replay an AI's full observation
 *      stream across consecutive hands (state machine + self image + plan).
 *   6. Writes JSONL corpora + manifest.json (sha256) under tests/golden/ai/ and
 *      verifies every scenario can be replayed deterministically.
 *
 * Usage:
 *   node oracle/golden-ai.js            # generate corpus + verify replays
 *   node oracle/golden-ai.js --verify   # re-read corpus, replay and compare
 *   node oracle/golden-ai.js --out=/tmp/ai-corpus
 *
 * Environment: Node 24 (no npm dependencies). REFERENCE_REPO overrides the
 * reference repository path.
 */
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const crypto = require('node:crypto');

const { mulberry32 } = require('./lib/prng');
const { referenceRepo } = require('./lib/load-js');

const REPO_ROOT = path.resolve(__dirname, '..');
const DEFAULT_OUT_DIR = path.join(REPO_ROOT, 'tests', 'golden', 'ai');
const ALGO_VERSION = 'v1.5.8';
const STRATEGY_VERSION = 'unified-v2';
const FORMAT_VERSION = 1;
const REFERENCE_COMMIT = 'e0c6c6f88d3e64324e1d83bfc700dd3a77f71b90';
const MATH_RANDOM_SEED = 0x9e3779b9;

// Browser boot order, copied from index.html minus UI-only scripts. The
// compiled CFR solver must be loaded before ai-core.js because ai-core.js
// captures globalThis.PokerRiverSolver at load time.
const BROWSER_FILES = [
    'js/cards.js',
    'js/game-rules-core.js',
    'js/engine.js',
    'js/probability-core.js',
    'js/probability.js',
    'js/ai-shortdeck-stable.js',
    'js/compiled-cfr.js',
    'js/river-solver.js',
    'js/ai-core.js',
    'js/ai-worker-client.js',
    'js/ai.js',
    'js/game.js',
    'js/shortdeck.js'
];

// Files whose exact bytes define the oracle semantics.
const SOURCE_HASH_FILES = [
    'js/ai-core.js',
    'js/ai.js',
    'js/ai-shortdeck-stable.js',
    'js/river-solver.js',
    'js/compiled-cfr.js',
    'js/probability-core.js',
    'js/game-rules-core.js',
    'server/server-game.js'
];

const STYLE_ORDER = ['TAG', 'LAG', 'SOLID', 'MANIAC', 'TAGFISH', 'CALLING_STATION'];
const AI_NAMES = ['Alex', 'Blake', 'Casey', 'Drew', 'Emma', 'Finn', 'Grace', 'Hayes', 'Ivy', 'Jade'];

const SUIT_BY_CHAR = { s: 'spades', h: 'hearts', d: 'diamonds', c: 'clubs' };
const CHAR_BY_SUIT = { spades: 's', hearts: 'h', diamonds: 'd', clubs: 'c' };
const RANK_VALUE = { J: 11, Q: 12, K: 13, A: 14 };

// ---------------------------------------------------------------------------
// Small deterministic helpers
// ---------------------------------------------------------------------------

function sha256(value) {
    return crypto.createHash('sha256').update(value).digest('hex');
}

function cloneJson(value) {
    return JSON.parse(JSON.stringify(value));
}

function cloneWithout(value, keys) {
    const copy = cloneJson(value);
    for (const key of keys) delete copy[key];
    return copy;
}

function cardFromCode(code) {
    const suitChar = code.slice(-1);
    const rankToken = code.slice(0, -1);
    const rank = rankToken === 'T' ? '10' : rankToken;
    const value = RANK_VALUE[rank] !== undefined ? RANK_VALUE[rank] : Number(rank);
    return { rank, suit: SUIT_BY_CHAR[suitChar], value };
}

function cardsFromCodes(codes) {
    return codes.map(cardFromCode);
}

function cardCode(card) {
    return `${card.rank === '10' ? 'T' : card.rank}${CHAR_BY_SUIT[card.suit]}`;
}

function deriveStreet(communityCards) {
    const count = (communityCards || []).length;
    if (count === 0) return 'preflop';
    if (count === 3) return 'flop';
    if (count === 4) return 'turn';
    return 'river';
}

function roundTrip(value) {
    return JSON.parse(JSON.stringify(value));
}

// ---------------------------------------------------------------------------
// Reference sandbox
// ---------------------------------------------------------------------------

let REFS = null;

function loadReference() {
    const repo = referenceRepo();
    const context = vm.createContext({
        console,
        setTimeout: () => 0,
        clearTimeout: () => {},
        setInterval: () => 0,
        clearInterval: () => {},
        // Virtual clock: the AI's only wall-clock use is the emergency brake in
        // estimateRangeEquity; pinning it to zero makes sample counts (and thus
        // every decision) reproducible regardless of machine load.
        performance: { now: () => 0 },
        Math,
        Date
    });
    for (const relative of BROWSER_FILES) {
        const filename = path.join(repo, relative);
        vm.runInContext(fs.readFileSync(filename, 'utf8'), context, { filename });
    }
    const W = vm.runInContext(`({
        PokerAICore,
        PokerProbabilityCore,
        PokerGameRules,
        PokerRiverSolver,
        PokerShortdeckStableCore,
        AIPlayer,
        ShortDeckAIPlayer,
        AI_STYLES,
        STYLE_CONFIGS,
        SD_STYLE_CONFIGS,
        AI_NAMES
    })`, context);

    // Deterministic RNG call counter. Both ai-core and the stabilised short-deck
    // core define their own mulberry32-style SeededRng class; count both.
    const rngCounter = { calls: 0 };
    let activeCounter = null;
    const patched = new Set();
    for (const namespace of ['PokerAICore', 'PokerShortdeckStableCore']) {
        const RngClass = W[namespace] && W[namespace].SeededRng;
        if (!RngClass || patched.has(RngClass)) continue;
        patched.add(RngClass);
        const originalNext = RngClass.prototype.next;
        RngClass.prototype.next = function () {
            if (activeCounter) activeCounter.calls++;
            return originalNext.call(this);
        };
    }

    const { ServerPokerGame, chooseServerAiAction } = require(path.join(repo, 'server', 'server-game.js'));

    return {
        repo,
        context,
        W,
        ServerPokerGame,
        chooseServerAiAction,
        rng: {
            begin() {
                rngCounter.calls = 0;
                activeCounter = rngCounter;
            },
            end() {
                const calls = rngCounter.calls;
                activeCounter = null;
                return calls;
            }
        }
    };
}

function beginRng() {
    REFS.rng.begin();
}

function endRng() {
    return REFS.rng.end();
}

// ---------------------------------------------------------------------------
// Observation / decision instrumentation
// ---------------------------------------------------------------------------

function instrumentSeat(seat, state) {
    const ai = seat.aiRef;

    // Capture the raw (unrounded) decision result for numerical comparisons.
    if (ai.sharedBrain && typeof ai.sharedBrain.decide === 'function') {
        const originalBrainDecide = ai.sharedBrain.decide.bind(ai.sharedBrain);
        ai.sharedBrain.decide = function (rawContext) {
            const result = originalBrainDecide(rawContext);
            ai.__lastRawResult = captureRawResult(result);
            return result;
        };
    }

    const originalObserve = ai.recordOpponentAction.bind(ai);
    const originalShowdown = ai.recordShowdown.bind(ai);
    const originalDecide = ai.decide.bind(ai);
    let mark = 0;

    ai.recordOpponentAction = function (seatId, action, street, handId, meta) {
        state.ops.push({
            op: 'observe',
            event: {
                seatId,
                action: String(action),
                street: String(street),
                handId,
                meta: cloneJson(meta || {})
            }
        });
        return originalObserve(seatId, action, street, handId, meta);
    };

    ai.recordShowdown = function (seatId, info) {
        state.ops.push({ op: 'showdown', seatId, info: cloneJson(info || {}) });
        return originalShowdown(seatId, info);
    };

    ai.decide = function (gameState) {
        const context = cloneWithout(gameState, ['evaluateCards']);
        const aiState = captureAiState(seat, ai, gameState);
        const prefix = state.ops.slice(mark);
        beginRng();
        const decision = originalDecide(gameState);
        const rngCalls = endRng();
        const trace = digestTrace(ai.lastDecisionTrace);
        const raw = ai.__lastRawResult ? cloneJson(ai.__lastRawResult) : null;
        state.snapshots.push({
            handId: context.handId,
            decisionId: context.decisionId,
            street: deriveStreet(context.communityCards),
            aiState,
            prefix,
            mark: state.ops.length,
            context,
            decision: cloneJson(decision),
            trace,
            raw,
            rngCalls
        });
        mark = state.ops.length;
        ai.__lastRawResult = null;
        return decision;
    };
}

function captureAiState(seat, ai, gameState) {
    return {
        name: ai.name,
        holeCards: cloneJson(ai.holeCards || []),
        stack: Number(gameState.stack) || 0,
        chipsInPot: Number(ai.chipsInPot) || 0,
        position: ai.position,
        numPlayers: ai.numPlayers,
        bigBlind: ai.bigBlind,
        isShortDeck: !!ai.isShortDeck,
        startingStack: seat.startingStack
    };
}

/** Unrounded view of the shared brain result (trace later rounds these values). */
function captureRawResult(result) {
    if (!result || typeof result !== 'object') return null;
    const equity = result.equity || {};
    const candidates = Array.isArray(result.candidates) ? result.candidates : [];
    const summary = candidates.map(candidate => {
        if (!candidate || typeof candidate !== 'object') return null;
        const response = candidate.response;
        return {
            action: candidate.action,
            amount: candidate.amount,
            invest: candidate.invest,
            ev: candidate.ev,
            risk: candidate.risk,
            utility: candidate.utility,
            reason: candidate.reason,
            calledEquity: candidate.calledEquity,
            terminalCall: candidate.terminalCall || null,
            planBonus: candidate.planBonus,
            response: response
                ? {
                    allFold: response.allFold,
                    anyCall: response.anyCall,
                    anyRaise: response.anyRaise,
                    expectedCallers: response.expectedCallers,
                    credibility: response.credibility,
                    fraction: response.fraction
                }
                : null
        };
    });
    const preLegalize = result.decision
        ? { action: result.decision.action, amount: result.decision.amount }
        : null;
    return {
        equity: {
            mean: equity.mean,
            stdDev: equity.stdDev,
            standardError: equity.standardError,
            rangeConfidence: equity.rangeConfidence,
            subsetEquities: equity.subsetEquities || null,
            samples: equity.samples || 0,
            confidence: equity.confidence,
            exact: !!equity.exact,
            timedOut: !!equity.timedOut,
            stalled: !!equity.stalled
        },
        preLegalize,
        candidates: summary
    };
}

function digestTrace(trace) {
    if (!trace) return null;
    const copy = cloneJson(trace);
    if (copy.riverSolver && copy.riverSolver.elapsedMs !== undefined) {
        delete copy.riverSolver.elapsedMs;
    }
    return copy;
}

// ---------------------------------------------------------------------------
// Table driver
// ---------------------------------------------------------------------------

function makeAi(aiVariant, style, stack, index) {
    const W = REFS.W;
    const name = AI_NAMES[index % AI_NAMES.length];
    if (aiVariant === 'shortdeck-local') return new W.ShortDeckAIPlayer(name, style, stack, index);
    if (aiVariant === 'shortdeck') return new W.AIPlayer(name, style, stack, index, 'shortdeck');
    return new W.AIPlayer(name, style, stack, index, 'standard');
}

function seatRecord(index, ai, stack) {
    return {
        id: `ai-${index}`,
        seatId: index,
        name: AI_NAMES[index % AI_NAMES.length],
        isHuman: false,
        connected: true,
        left: false,
        stack,
        holeCards: [],
        chipsInPot: 0,
        roundBet: 0,
        folded: false,
        isAllIn: false,
        dealtIn: false,
        lastAction: null,
        aiRef: ai
    };
}

function runTable(spec) {
    const players = [];
    const states = [];
    for (let seat = 0; seat < spec.seats; seat++) {
        const style = STYLE_ORDER[(spec.styleOffset + seat) % STYLE_ORDER.length];
        const ai = makeAi(spec.aiVariant, style, spec.startingStack, seat);
        const record = seatRecord(seat, ai, spec.startingStack);
        const state = { tableId: spec.id, seat, style, ops: [], snapshots: [] };
        instrumentSeat(record, state);
        players.push(record);
        states.push(state);
    }
    const config = spec.variant === 'shortdeck'
        ? { isShortDeck: true, startingStack: spec.startingStack, ante: spec.ante, minBet: spec.minBet }
        : { isShortDeck: false, startingStack: spec.startingStack, smallBlind: spec.smallBlind, bigBlind: spec.bigBlind };
    const game = new REFS.ServerPokerGame(config, players, { random: mulberry32(spec.seed) });

    let decisions = 0;
    let errors = 0;
    let handsPlayed = 0;
    for (let hand = 0; hand < spec.hands; hand++) {
        if (players.filter(player => player.stack > 0).length < 2) break;
        if (!game.startHand()) break;
        let guard = 0;
        while (game.phase !== 'idle') {
            if (++guard > 400) { errors += 1; break; }
            const seat = game.currentPlayerIdx;
            if (seat < 0) break;
            const decision = REFS.chooseServerAiAction(game, seat);
            if (!decision) { errors += 1; break; }
            const result = game.act(seat, decision.action, decision.amount);
            if (!result.ok) { errors += 1; break; }
            decisions += 1;
        }
        handsPlayed += 1;
    }
    return { spec, states, players, decisions, handsPlayed, errors };
}

// ---------------------------------------------------------------------------
// Decision cases
// ---------------------------------------------------------------------------

function tagFocus(trace) {
    return trace && trace.parameters ? trace.parameters.strategyFocus : 'general';
}

function snapshotTags(table, state, snapshot) {
    const tags = [];
    const spec = table.spec;
    const context = snapshot.context;
    const trace = snapshot.trace;
    tags.push(spec.variant);
    tags.push(tagFocus(trace));
    tags.push(snapshot.street);
    tags.push(snapshot.decision.action);
    tags.push((context.toCall || 0) > 0 ? 'facing' : 'open');
    const opponents = (context.activeOpponentSeats || []).length;
    tags.push(opponents <= 1 ? 'heads-up' : opponents === 2 ? 'three-way' : 'multiway');
    tags.push(`style-${state.style}`);
    tags.push(`profile-${spec.aiVariant}`);
    if (trace && trace.riverSolver) tags.push('river-cfr');
    const bigBlind = context.bigBlind || spec.bigBlind || 200;
    if ((context.stack || 0) <= 12 * bigBlind) tags.push('short-stack');
    if ((context.stack || 0) >= 80 * bigBlind) tags.push('deep-stack');
    if ((context.publicPlayers || []).some(player => player.allIn || player.stack === 0)) tags.push('allin-opponent');
    if (Array.isArray(trace && trace.candidates) && trace.candidates.some(candidate => candidate.terminalCall)) tags.push('terminal-call');
    if ((context.preflopRaiseCount || 0) >= 2) tags.push('preflop-3bet-plus');
    tags.push('harvested');
    return tags;
}

function harvestedPoolEntry(table, state, snapshot) {
    return {
        kind: 'harvested',
        table,
        state,
        snapshot,
        tags: snapshotTags(table, state, snapshot),
        id: caseId(table.spec.id, snapshot, state.seat)
    };
}

function caseId(tableId, snapshot, seat) {
    const decision = String(snapshot.decisionId || '0').replace(/[^a-zA-Z0-9]+/g, '-');
    return `${tableId}-h${snapshot.handId}-${decision}-s${seat}`;
}

function aiDescriptor(spec, state, aiState) {
    const type = spec.aiVariant === 'shortdeck-local' ? 'ShortDeckAIPlayer' : 'AIPlayer';
    const profileSource = spec.aiVariant === 'shortdeck-local'
        ? 'SD_STYLE_CONFIGS'
        : spec.aiVariant === 'shortdeck'
            ? 'STYLE_CONFIGS'
            : 'STYLE_CONFIGS';
    return {
        type,
        variant: spec.variant,
        style: state.style,
        seatId: aiState.position,
        name: aiState.name,
        profileSource,
        startingStack: spec.startingStack
    };
}

function createCaseAi(ai) {
    const W = REFS.W;
    if (ai.type === 'ShortDeckAIPlayer') {
        return new W.ShortDeckAIPlayer(ai.name, ai.style, ai.startingStack, ai.seatId);
    }
    return new W.AIPlayer(ai.name, ai.style, ai.startingStack, ai.seatId, ai.variant === 'shortdeck' ? 'shortdeck' : 'standard');
}

function applyAiState(ai, aiState) {
    ai.holeCards = cloneJson(aiState.holeCards);
    ai.stack = aiState.stack;
    ai.chipsInPot = aiState.chipsInPot;
    ai.position = aiState.position;
    ai.numPlayers = aiState.numPlayers;
    ai.bigBlind = aiState.bigBlind;
    ai.isShortDeck = !!aiState.isShortDeck;
}

function applyObserves(ai, observes) {
    for (const op of observes) {
        if (op.op === 'observe') {
            ai.recordOpponentAction(op.event.seatId, op.event.action, op.event.street, op.event.handId, cloneJson(op.event.meta));
        } else if (op.op === 'showdown') {
            ai.recordShowdown(op.seatId, cloneJson(op.info));
        } else {
            throw new Error(`unknown observation op ${op.op}`);
        }
    }
}

function decisionContext(context) {
    const copy = cloneJson(context);
    copy.evaluateCards = (cards, variant) => REFS.W.PokerProbabilityCore.evaluate(cards, variant === 'shortdeck');
    return copy;
}

/** Replay a self-contained decision case and return the expected output. */
function replayDecision(aiDescriptor, aiState, observes, context, options = {}) {
    const ai = createCaseAi(aiDescriptor);
    applyAiState(ai, aiState);
    applyObserves(ai, observes);

    let raw = null;
    if (options.captureRaw !== false && ai.sharedBrain && typeof ai.sharedBrain.decide === 'function') {
        const originalBrainDecide = ai.sharedBrain.decide.bind(ai.sharedBrain);
        ai.sharedBrain.decide = function (rawContext) {
            const result = originalBrainDecide(rawContext);
            raw = captureRawResult(result);
            return result;
        };
    }

    beginRng();
    const decision = ai.decide(decisionContext(context));
    const rngCalls = endRng();
    return {
        action: decision.action,
        amount: decision.amount,
        rngCalls,
        trace: digestTrace(ai.lastDecisionTrace),
        raw
    };
}

function buildDecisionCase(entry, options = {}) {
    const { table, state, snapshot } = entry;
    const ai = aiDescriptor(table.spec, state, snapshot.aiState);
    const tags = entry.tags.slice();
    if (options.extraTags) tags.push(...options.extraTags);
    const base = {
        id: entry.id,
        category: options.category || 'harvested',
        tags,
        table: tableSummary(table.spec),
        ai,
        aiState: snapshot.aiState,
        observes: snapshot.prefix,
        context: snapshot.context
    };
    const first = replayDecision(base.ai, base.aiState, base.observes, base.context);
    const second = replayDecision(base.ai, base.aiState, base.observes, base.context);
    if (JSON.stringify(first) !== JSON.stringify(second)) {
        throw new Error(`non-deterministic replay for ${base.id}`);
    }
    base.expect = first;
    base.inputSha256 = sha256(JSON.stringify({ ai: base.ai, aiState: base.aiState, observes: base.observes, context: base.context }));
    return base;
}

function tableSummary(spec) {
    const summary = {
        id: spec.id,
        variant: spec.variant,
        aiVariant: spec.aiVariant,
        seats: spec.seats,
        startingStack: spec.startingStack,
        handsSimulated: spec.hands,
        seed: `0x${(spec.seed >>> 0).toString(16)}`
    };
    if (spec.variant === 'shortdeck') {
        summary.ante = spec.ante;
        summary.minBet = spec.minBet;
    } else {
        summary.smallBlind = spec.smallBlind;
        summary.bigBlind = spec.bigBlind;
    }
    return summary;
}

// ---------------------------------------------------------------------------
// Harvest selection rules
// ---------------------------------------------------------------------------

function has(tags, ...wanted) {
    return wanted.every(tag => tags.includes(tag));
}

function selectEntries(pool, rules) {
    const selected = [];
    const seen = new Set();
    for (const rule of rules) {
        let count = 0;
        for (const entry of pool) {
            if (count >= rule.limit) break;
            if (seen.has(entry.id)) continue;
            if (!rule.match(entry)) continue;
            selected.push(entry);
            seen.add(entry.id);
            count += 1;
        }
    }
    return selected;
}

const STANDARD_TARGET_RULES = [
    { name: 'preflop-open', limit: 8, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'preflop', 'open') },
    { name: 'preflop-facing', limit: 12, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'preflop', 'facing') },
    { name: 'preflop-3bet-plus', limit: 6, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'preflop-3bet-plus') },
    { name: 'flop-open', limit: 6, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'flop', 'open') },
    { name: 'flop-facing', limit: 8, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'flop', 'facing') },
    { name: 'turn-any', limit: 10, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'turn') },
    { name: 'river-plain', limit: 8, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'river') && !e.tags.includes('river-cfr') },
    { name: 'river-cfr', limit: 14, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'river-cfr') },
    { name: 'multiway', limit: 10, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'multiway') },
    { name: 'short-stack', limit: 8, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'short-stack') },
    { name: 'allin-action', limit: 6, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'allin') },
    { name: 'allin-opponent', limit: 4, match: e => has(e.tags, 'standard', 'ten-player-25bb', 'allin-opponent') },
    ...STYLE_ORDER.map(style => ({
        name: `style-${style}`,
        limit: 2,
        match: e => has(e.tags, 'standard', 'ten-player-25bb', `style-${style}`)
    }))
];

const STANDARD_GENERAL_RULES = [
    { name: 'preflop-open', limit: 6, match: e => has(e.tags, 'standard', 'general', 'preflop', 'open') },
    { name: 'preflop-facing', limit: 6, match: e => has(e.tags, 'standard', 'general', 'preflop', 'facing') },
    { name: 'flop-open', limit: 5, match: e => has(e.tags, 'standard', 'general', 'flop', 'open') },
    { name: 'flop-facing', limit: 6, match: e => has(e.tags, 'standard', 'general', 'flop', 'facing') },
    { name: 'turn-any', limit: 6, match: e => has(e.tags, 'standard', 'general', 'turn') },
    { name: 'river-any', limit: 5, match: e => has(e.tags, 'standard', 'general', 'river') },
    { name: 'multiway', limit: 5, match: e => has(e.tags, 'standard', 'general', 'multiway') },
    { name: 'deep-stack', limit: 5, match: e => has(e.tags, 'standard', 'general', 'deep-stack') },
    { name: 'allin-action', limit: 3, match: e => has(e.tags, 'standard', 'general', 'allin') }
];

const SHORTDECK_LOCAL_RULES = [
    { name: 'preflop-open', limit: 6, match: e => has(e.tags, 'shortdeck', 'shortdeck-local', 'preflop', 'open') },
    { name: 'preflop-facing', limit: 6, match: e => has(e.tags, 'shortdeck', 'shortdeck-local', 'preflop', 'facing') },
    { name: 'flop-any', limit: 8, match: e => has(e.tags, 'shortdeck', 'shortdeck-local', 'flop') },
    { name: 'turn-any', limit: 8, match: e => has(e.tags, 'shortdeck', 'shortdeck-local', 'turn') },
    { name: 'river-any', limit: 6, match: e => has(e.tags, 'shortdeck', 'shortdeck-local', 'river') },
    { name: 'multiway', limit: 6, match: e => has(e.tags, 'shortdeck', 'shortdeck-local', 'multiway') },
    { name: 'allin-action', limit: 4, match: e => has(e.tags, 'shortdeck', 'shortdeck-local', 'allin') },
    ...STYLE_ORDER.map(style => ({
        name: `style-${style}`,
        limit: 2,
        match: e => has(e.tags, 'shortdeck', 'shortdeck-local', `style-${style}`)
    }))
];

const SHORTDECK_SERVER_RULES = [
    { name: 'preflop', limit: 5, match: e => has(e.tags, 'shortdeck', 'shortdeck', 'preflop') },
    { name: 'flop', limit: 6, match: e => has(e.tags, 'shortdeck', 'shortdeck', 'flop') },
    { name: 'turn', limit: 5, match: e => has(e.tags, 'shortdeck', 'shortdeck', 'turn') },
    { name: 'river', limit: 4, match: e => has(e.tags, 'shortdeck', 'shortdeck', 'river') },
    { name: 'allin-opponent', limit: 3, match: e => has(e.tags, 'shortdeck', 'shortdeck', 'allin-opponent') }
];

// ---------------------------------------------------------------------------
// Synthetic cases
// ---------------------------------------------------------------------------

function playerRow(seat, overrides = {}) {
    return {
        seat,
        stack: overrides.stack !== undefined ? overrides.stack : 5000,
        committed: overrides.committed !== undefined ? overrides.committed : 0,
        roundBet: overrides.roundBet !== undefined ? overrides.roundBet : 0,
        positionFromButton: overrides.positionFromButton,
        isSmallBlind: !!overrides.isSmallBlind,
        isBigBlind: !!overrides.isBigBlind,
        folded: !!overrides.folded,
        allIn: !!overrides.allIn
    };
}

function baseStandardContext(options) {
    const variant = options.variant || 'standard';
    const bigBlind = options.bigBlind || 200;
    const tablePlayerCount = options.tablePlayerCount || 10;
    const startingStack = options.startingStack || 5000;
    const handId = options.handId || 1;
    return {
        handId,
        decisionId: options.decisionId || `${variant}:${options.street || 'flop'}:0:${options.seatId || 0}`,
        seed: options.seed || (variant === 'shortdeck' ? 'poker-table:shortdeck' : 'poker-table:standard'),
        variant,
        startingStack,
        tablePlayerCount,
        heroSeat: options.seatId || 0,
        communityCards: cardsFromCodes(options.communityCards || []),
        publicPlayers: options.publicPlayers || [],
        pot: options.pot || 0,
        currentBet: options.currentBet || 0,
        toCall: options.toCall || 0,
        yourBet: options.yourBet || 0,
        stack: options.stack !== undefined ? options.stack : startingStack,
        effectiveStack: options.effectiveStack !== undefined ? options.effectiveStack : options.stack,
        numOpponentsActive: options.numOpponentsActive || (options.activeOpponentSeats || []).length,
        activeOpponentSeats: options.activeOpponentSeats || [],
        playerCount: options.playerCount || ((options.activeOpponentSeats || []).length + 1),
        currentPlayerIndex: options.seatId || 0,
        positionFromButton: options.positionFromButton !== undefined ? options.positionFromButton : 0,
        legalActions: options.legalActions || ['fold', 'call', 'raise', 'allin'],
        canCheck: !!options.canCheck,
        canRaise: options.canRaise !== false,
        minRaiseTo: options.minRaiseTo || 0,
        maxRaiseTo: options.maxRaiseTo !== undefined ? options.maxRaiseTo : startingStack,
        preflopRaiseCount: options.preflopRaiseCount || 0,
        isPreviousStreetAggressor: !!options.isPreviousStreetAggressor,
        isDelayedCBetCandidate: !!options.isDelayedCBetCandidate,
        isSmallBlind: !!options.isSmallBlind,
        isBigBlind: !!options.isBigBlind,
        bigBlind,
        wagerUnit: options.wagerUnit || bigBlind / 2,
        timeBudgetMs: options.timeBudgetMs !== undefined ? options.timeBudgetMs : 150,
        ...(variant === 'shortdeck' ? { isShortDeck: true } : {}),
        ...(options.extra || {})
    };
}

function observeEvent(seatId, action, street, handId, meta) {
    return {
        op: 'observe',
        event: { seatId, action, street, handId, meta }
    };
}

function actionMeta(options = {}) {
    return {
        callCost: options.toCallBefore || 0,
        toCallBefore: options.toCallBefore || 0,
        additionalPaid: options.additionalPaid || 0,
        amount: options.additionalPaid || 0,
        raiseTo: options.raiseTo || 0,
        currentBetBefore: options.currentBetBefore || 0,
        potBefore: options.potBefore || 0,
        potAfter: options.potAfter || 0,
        minimumBet: options.minimumBet || 200,
        aggressive: !!options.aggressive,
        betFraction: options.betFraction !== undefined ? options.betFraction : 0,
        preflopRaiseCountBefore: options.preflopRaiseCountBefore || 0,
        positionFromButton: options.positionFromButton,
        playerCount: options.playerCount,
        isSmallBlind: !!options.isSmallBlind,
        isBigBlind: !!options.isBigBlind,
        communityCards: cardsFromCodes(options.communityCards || [])
    };
}

function buildSyntheticCase(definition) {
    const ai = {
        type: definition.aiType || 'AIPlayer',
        variant: definition.variant,
        style: definition.style || 'SOLID',
        seatId: definition.context.seatId,
        name: 'Corpus',
        profileSource: definition.aiType === 'ShortDeckAIPlayer' ? 'SD_STYLE_CONFIGS' : 'STYLE_CONFIGS',
        startingStack: definition.context.startingStack
    };
    const aiState = {
        name: 'Corpus',
        holeCards: cardsFromCodes(definition.holeCards),
        stack: definition.context.stack,
        chipsInPot: definition.context.yourBet,
        position: definition.context.seatId,
        numPlayers: definition.context.playerCount,
        bigBlind: definition.context.bigBlind,
        isShortDeck: definition.variant === 'shortdeck',
        startingStack: definition.context.startingStack
    };
    const base = {
        id: definition.id,
        category: 'synthetic',
        tags: ['synthetic', definition.variant, ...(definition.tags || [])],
        table: definition.table || {
            id: `synthetic-${definition.variant}`,
            variant: definition.variant,
            seats: definition.context.tablePlayerCount,
            startingStack: definition.context.startingStack,
            smallBlind: definition.variant === 'standard' ? definition.context.wagerUnit : undefined,
            bigBlind: definition.context.bigBlind,
            ante: definition.variant === 'shortdeck' ? definition.context.wagerUnit : undefined,
            minBet: definition.variant === 'shortdeck' ? definition.context.bigBlind : undefined,
            seed: '0x0'
        },
        ai,
        aiState,
        observes: definition.observes || [],
        context: definition.context
    };
    const first = replayDecision(base.ai, base.aiState, base.observes, base.context);
    const second = replayDecision(base.ai, base.aiState, base.observes, base.context);
    if (JSON.stringify(first) !== JSON.stringify(second)) {
        throw new Error(`non-deterministic synthetic replay for ${base.id}`);
    }
    base.expect = first;
    base.inputSha256 = sha256(JSON.stringify({ ai: base.ai, aiState: base.aiState, observes: base.observes, context: base.context }));
    return base;
}

function syntheticStandardCases() {
    const cases = [];

    // --- Style sweep: identical public state, six personalities. ---------
    const styleState = {
        variant: 'standard',
        street: 'preflop',
        seatId: 3,
        handId: 1,
        decisionId: 'standard:preflop:0:3',
        holeCards: ['As', 'Qh'],
        communityCards: [],
        pot: 150,
        currentBet: 100,
        toCall: 100,
        yourBet: 0,
        stack: 19700,
        effectiveStack: 19700,
        activeOpponentSeats: [0, 1, 2, 4, 5],
        numOpponentsActive: 5,
        playerCount: 6,
        positionFromButton: 3,
        canCheck: false,
        canRaise: true,
        minRaiseTo: 200,
        maxRaiseTo: 19700,
        bigBlind: 100,
        wagerUnit: 50,
        tablePlayerCount: 6,
        startingStack: 20000,
        legalActions: ['fold', 'call', 'raise', 'allin'],
        publicPlayers: [
            playerRow(0, { stack: 20000, committed: 50, roundBet: 50, positionFromButton: 0, isSmallBlind: true }),
            playerRow(1, { stack: 20000, committed: 100, roundBet: 100, positionFromButton: 1, isBigBlind: true }),
            playerRow(2, { stack: 20000, committed: 0, positionFromButton: 2 }),
            playerRow(3, { stack: 20000, committed: 0, positionFromButton: 3 }),
            playerRow(4, { stack: 20000, committed: 0, positionFromButton: 4 }),
            playerRow(5, { stack: 20000, committed: 0, positionFromButton: 5 })
        ]
    };
    for (const style of STYLE_ORDER) {
        cases.push({
            id: `synthetic-style-${style.toLowerCase()}-aqo-utg`,
            style,
            variant: 'standard',
            holeCards: styleState.holeCards,
            tags: ['style-sweep', 'preflop', 'open', `style-${style}`, 'general'],
            context: baseStandardContext({ ...styleState, decisionId: 'standard:preflop:0:3' })
        });
    }

    // --- Short stack 4BB preflop push/fold. -------------------------------
    cases.push({
        id: 'synthetic-shortstack-4bb-preflop-facing-raise',
        style: 'LAG',
        variant: 'standard',
        holeCards: ['9s', '9d'],
        tags: ['short-stack', 'preflop', 'facing', 'general'],
        context: baseStandardContext({
            variant: 'standard',
            seatId: 2,
            handId: 3,
            decisionId: 'standard:preflop:2:2',
            street: 'preflop',
            pot: 900,
            currentBet: 600,
            toCall: 400,
            stack: 800,
            effectiveStack: 800,
            activeOpponentSeats: [0, 1, 3],
            numOpponentsActive: 3,
            playerCount: 6,
            positionFromButton: 2,
            preflopRaiseCount: 1,
            legalActions: ['fold', 'call', 'raise', 'allin'],
            canRaise: true,
            minRaiseTo: 1000,
            maxRaiseTo: 800,
            bigBlind: 200,
            wagerUnit: 100,
            tablePlayerCount: 6,
            startingStack: 5000,
            publicPlayers: [
                playerRow(0, { stack: 4200, committed: 100, roundBet: 100, isSmallBlind: true, positionFromButton: 0 }),
                playerRow(1, { stack: 2000, committed: 200, roundBet: 200, isBigBlind: true, positionFromButton: 1 }),
                playerRow(2, { stack: 800, positionFromButton: 2 }),
                playerRow(3, { stack: 5000, committed: 600, roundBet: 600, positionFromButton: 3 })
            ]
        })
    });

    // --- Cannot raise: facing an all-in. ----------------------------------
    cases.push({
        id: 'synthetic-cannot-raise-facing-allin-flop',
        style: 'TAG',
        variant: 'standard',
        holeCards: ['Kh', 'Qh'],
        tags: ['cannot-raise', 'flop', 'facing', 'allin-opponent', 'general'],
        context: baseStandardContext({
            variant: 'standard',
            seatId: 1,
            handId: 5,
            decisionId: 'standard:flop:1:1',
            street: 'flop',
            communityCards: ['Qc', '9d', '4c'],
            pot: 2800,
            currentBet: 3000,
            toCall: 1400,
            yourBet: 0,
            stack: 1400,
            effectiveStack: 1400,
            activeOpponentSeats: [3],
            numOpponentsActive: 1,
            playerCount: 3,
            positionFromButton: 1,
            canCheck: false,
            canRaise: false,
            legalActions: ['fold', 'call'],
            minRaiseTo: 0,
            maxRaiseTo: 1400,
            bigBlind: 200,
            wagerUnit: 100,
            tablePlayerCount: 6,
            startingStack: 5000,
            publicPlayers: [
                playerRow(1, { stack: 1400, positionFromButton: 1 }),
                playerRow(3, { stack: 0, committed: 4500, roundBet: 3000, allIn: true, positionFromButton: 2 })
            ]
        })
    });

    // --- Terminal river call: both stacks committed. ---------------------
    cases.push({
        id: 'synthetic-terminal-river-call',
        style: 'CALLING_STATION',
        variant: 'standard',
        holeCards: ['Ah', '8h'],
        tags: ['terminal-call', 'river', 'facing', 'allin-opponent', 'general'],
        context: baseStandardContext({
            variant: 'standard',
            seatId: 2,
            handId: 7,
            decisionId: 'standard:river:4:2',
            street: 'river',
            communityCards: ['Qs', '9d', '6c', 'Jd', '2h'],
            pot: 4200,
            currentBet: 1600,
            toCall: 1600,
            yourBet: 0,
            stack: 1600,
            effectiveStack: 1600,
            activeOpponentSeats: [0],
            numOpponentsActive: 1,
            playerCount: 2,
            positionFromButton: 0,
            canCheck: false,
            canRaise: false,
            legalActions: ['fold', 'call'],
            minRaiseTo: 0,
            maxRaiseTo: 1600,
            bigBlind: 200,
            wagerUnit: 100,
            tablePlayerCount: 2,
            startingStack: 5000,
            publicPlayers: [
                playerRow(0, { stack: 0, committed: 4600, roundBet: 1600, allIn: true, positionFromButton: 0 }),
                playerRow(2, { stack: 1600, positionFromButton: 1 })
            ]
        })
    });

    // --- Check-only input (raise rights gone, no bet to face). -----------
    cases.push({
        id: 'synthetic-check-only-turn',
        style: 'SOLID',
        variant: 'standard',
        holeCards: ['Js', 'Th'],
        tags: ['check-only', 'turn', 'open', 'general'],
        context: baseStandardContext({
            variant: 'standard',
            seatId: 1,
            handId: 9,
            decisionId: 'standard:turn:2:1',
            street: 'turn',
            communityCards: ['9s', '8h', '2c', 'Kd'],
            pot: 900,
            currentBet: 0,
            toCall: 0,
            yourBet: 0,
            stack: 4500,
            effectiveStack: 4500,
            activeOpponentSeats: [0],
            numOpponentsActive: 1,
            playerCount: 2,
            positionFromButton: 1,
            canCheck: true,
            canRaise: false,
            legalActions: ['check'],
            minRaiseTo: 0,
            maxRaiseTo: 4500,
            bigBlind: 200,
            wagerUnit: 100,
            tablePlayerCount: 2,
            startingStack: 5000,
            publicPlayers: [
                playerRow(0, { stack: 4500, committed: 450, roundBet: 0, positionFromButton: 0 }),
                playerRow(1, { stack: 4500, positionFromButton: 1 })
            ]
        })
    });

    // --- Model-driven response: opponent folds too much / calls too much. -
    const folderObserves = [];
    const callerObserves = [];
    for (let handId = 1; handId <= 14; handId++) {
        const board = ['Qs', '9d', '4c'];
        const meta = actionMeta({
            toCallBefore: 300,
            additionalPaid: 0,
            potBefore: 1200,
            minimumBet: 200,
            betFraction: 0.62,
            positionFromButton: 0,
            playerCount: 2,
            communityCards: board
        });
        folderObserves.push(observeEvent(0, 'fold', 'flop', handId, meta));
        callerObserves.push(observeEvent(0, 'call', 'flop', handId, actionMeta({
            ...meta,
            additionalPaid: 300,
            aggressive: false
        })));
    }
    for (const [variantName, observes] of [['folder', folderObserves], ['caller', callerObserves]]) {
        const folder = variantName === 'folder';
        cases.push({
            id: `synthetic-response-vs-${variantName}`,
            style: 'LAG',
            variant: 'standard',
            holeCards: ['As', 'Ks'],
            tags: ['response-model', 'facing', 'flop', `opponent-${variantName}`, 'general'],
            observes,
            context: baseStandardContext({
                variant: 'standard',
                seatId: 1,
                handId: 15,
                decisionId: 'standard:flop:3:1',
                street: 'flop',
                communityCards: ['Qs', '9d', '4c'],
                pot: 1200,
                currentBet: 300,
                toCall: 300,
                yourBet: 0,
                stack: 4700,
                effectiveStack: 4700,
                activeOpponentSeats: [0],
                numOpponentsActive: 1,
                playerCount: 2,
                positionFromButton: 1,
                canCheck: false,
                canRaise: true,
                minRaiseTo: 600,
                maxRaiseTo: 4700,
                bigBlind: 200,
                wagerUnit: 100,
                tablePlayerCount: 2,
                startingStack: 5000,
                publicPlayers: [
                    playerRow(0, { stack: 4700, committed: 600, roundBet: 300, positionFromButton: 0 }),
                    playerRow(1, { stack: 4700, committed: 300, positionFromButton: 1 })
                ]
            })
        });
    }

    // --- Turned nuts with a raise available. -----------------------------
    cases.push({
        id: 'synthetic-turn-nuts-raise',
        style: 'MANIAC',
        variant: 'standard',
        holeCards: ['Ts', '9s'],
        tags: ['value', 'turn', 'facing', 'general'],
        context: baseStandardContext({
            variant: 'standard',
            seatId: 3,
            handId: 11,
            decisionId: 'standard:turn:1:3',
            street: 'turn',
            communityCards: ['8d', '7c', '2h', 'Jd'],
            pot: 1600,
            currentBet: 500,
            toCall: 500,
            yourBet: 0,
            stack: 6800,
            effectiveStack: 6800,
            activeOpponentSeats: [1, 2],
            numOpponentsActive: 2,
            playerCount: 4,
            positionFromButton: 1,
            canCheck: false,
            canRaise: true,
            minRaiseTo: 1000,
            maxRaiseTo: 6800,
            bigBlind: 200,
            wagerUnit: 100,
            tablePlayerCount: 4,
            startingStack: 8000,
            publicPlayers: [
                playerRow(1, { stack: 9000, committed: 800, roundBet: 500, positionFromButton: 1 }),
                playerRow(2, { stack: 7000, committed: 700, roundBet: 500, positionFromButton: 2 }),
                playerRow(3, { stack: 6800, committed: 300, positionFromButton: 3 })
            ]
        })
    });

    // --- Deep stack (200BB) dry turn facing a check. ---------------------
    cases.push({
        id: 'synthetic-deep-200bb-dry-turn',
        style: 'TAG',
        variant: 'standard',
        holeCards: ['Ac', 'Kd'],
        tags: ['deep-stack', 'turn', 'open', 'general'],
        context: baseStandardContext({
            variant: 'standard',
            seatId: 1,
            handId: 13,
            decisionId: 'standard:turn:2:1',
            street: 'turn',
            communityCards: ['As', '7d', '2c', 'Kh'],
            pot: 1200,
            currentBet: 0,
            toCall: 0,
            yourBet: 0,
            stack: 19600,
            effectiveStack: 19600,
            activeOpponentSeats: [0],
            numOpponentsActive: 1,
            playerCount: 2,
            positionFromButton: 1,
            canCheck: true,
            canRaise: true,
            minRaiseTo: 100,
            maxRaiseTo: 19600,
            bigBlind: 100,
            wagerUnit: 50,
            tablePlayerCount: 4,
            startingStack: 20000,
            publicPlayers: [
                playerRow(0, { stack: 19600, committed: 600, roundBet: 0, positionFromButton: 0 }),
                playerRow(1, { stack: 19600, committed: 600, positionFromButton: 1 })
            ]
        })
    });

    return cases;
}

// Ten-player 25BB table: river heads-up spots that activate the public-range
// CFR solver. publicPlayers include the eight folded seats so the solver's
// "no third live player" gate and contestable-pot checks are realistic.
const CFR_BOARDS = [
    ['Kc', '8d', '3h', '2s', 'Jc'],
    ['9h', '8d', '7c', 'Js', '2d'],
    ['Qc', 'Qd', '9h', '2s', '3c'],
    ['Th', '7h', '2h', 'Kh', '5c'],
    ['As', '7d', '4c', '2h', '9s'],
    ['6c', '5d', '4h', '8s', '2c']
];
const CFR_HOLE = [
    ['Ah', 'Kd'],
    ['Ts', '9s'],
    ['Qh', 'Jh'],
    ['Ac', '5d'],
    ['8c', '8h'],
    ['Ks', 'Qc']
];

function cfrPlayers(heroSeat, villainSeat, options = {}) {
    const heroCommitted = options.heroCommitted !== undefined ? options.heroCommitted : 350;
    const villainCommitted = options.villainCommitted !== undefined ? options.villainCommitted : 350;
    const heroRoundBet = options.heroRoundBet || 0;
    const villainRoundBet = options.villainRoundBet || 0;
    const heroStack = options.heroStack !== undefined ? options.heroStack : 4650;
    const villainStack = options.villainStack !== undefined ? options.villainStack : 4650;
    const extraLive = options.extraLive || [];
    const rows = [];
    for (let seat = 0; seat < 10; seat++) {
        if (seat === heroSeat) {
            rows.push(playerRow(seat, {
                stack: heroStack,
                committed: heroCommitted,
                roundBet: heroRoundBet,
                positionFromButton: options.heroPosition,
                isSmallBlind: !!options.heroIsSmallBlind,
                isBigBlind: !!options.heroIsBigBlind
            }));
        } else if (seat === villainSeat) {
            rows.push(playerRow(seat, {
                stack: villainStack,
                committed: villainCommitted,
                roundBet: villainRoundBet,
                positionFromButton: options.villainPosition,
                isSmallBlind: !!options.villainIsSmallBlind,
                isBigBlind: !!options.villainIsBigBlind
            }));
        } else if (extraLive.includes(seat)) {
            rows.push(playerRow(seat, { stack: 1000, committed: 350, roundBet: 0, positionFromButton: 5, folded: false }));
        } else {
            rows.push(playerRow(seat, { stack: 0, committed: 0, folded: true }));
        }
    }
    return rows;
}

function syntheticCfrCases() {
    const cases = [];
    const heroSeat = 3;
    const villainSeat = 8;

    for (let index = 0; index < CFR_BOARDS.length; index++) {
        const board = CFR_BOARDS[index];
        const hole = CFR_HOLE[index];
        for (const checkedBefore of [true, false]) {
            // checkedBefore true  -> hero has position and the villain already checked.
            // checkedBefore false -> hero acts first (solver must treat a check as non-terminal).
            const heroPosition = checkedBefore ? 0 : 1;
            const villainPosition = checkedBefore ? 1 : 0;
            const observes = checkedBefore
                ? [observeEvent(villainSeat, 'check', 'river', 21, actionMeta({
                    potBefore: 700,
                    toCallBefore: 0,
                    minimumBet: 200,
                    betFraction: 0,
                    positionFromButton: villainPosition,
                    playerCount: 2,
                    communityCards: board
                }))]
                : [];
            cases.push({
                id: `synthetic-cfr-river-dry-${index + 1}-${checkedBefore ? 'checked' : 'first'}`,
                style: STYLE_ORDER[index % STYLE_ORDER.length],
                variant: 'standard',
                holeCards: hole,
                tags: ['river-cfr', 'river', 'heads-up', 'ten-player-25bb', checkedBefore ? 'checked-before' : 'hero-first'],
                observes,
                context: baseStandardContext({
                    variant: 'standard',
                    seatId: heroSeat,
                    handId: 21,
                    decisionId: `standard:river:5:${heroSeat}`,
                    street: 'river',
                    communityCards: board,
                    pot: 700,
                    currentBet: 0,
                    toCall: 0,
                    yourBet: 0,
                    stack: 4650,
                    effectiveStack: 4650,
                    activeOpponentSeats: [villainSeat],
                    numOpponentsActive: 1,
                    playerCount: 2,
                    positionFromButton: heroPosition,
                    canCheck: true,
                    canRaise: true,
                    minRaiseTo: 200,
                    maxRaiseTo: 4650,
                    bigBlind: 200,
                    wagerUnit: 100,
                    tablePlayerCount: 10,
                    startingStack: 5000,
                    legalActions: ['check', 'raise', 'allin'],
                    publicPlayers: cfrPlayers(heroSeat, villainSeat, {
                        heroPosition,
                        villainPosition,
                        heroIsSmallBlind: !checkedBefore,
                        heroIsBigBlind: false,
                        villainIsSmallBlind: false,
                        villainIsBigBlind: !checkedBefore
                    })
                })
            });
        }
    }

    // Facing a river bet: pre-street commitments are equal, so the solver gate
    // passes and the call/fold/raise decision is part of the subgame.
    cases.push({
        id: 'synthetic-cfr-river-facing-bet',
        style: 'SOLID',
        variant: 'standard',
        holeCards: ['Ah', 'Kd'],
        tags: ['river-cfr', 'river', 'facing', 'heads-up', 'ten-player-25bb'],
        context: baseStandardContext({
            variant: 'standard',
            seatId: heroSeat,
            handId: 22,
            decisionId: `standard:river:6:${heroSeat}`,
            street: 'river',
            communityCards: CFR_BOARDS[0],
            pot: 1000,
            currentBet: 300,
            toCall: 300,
            yourBet: 0,
            stack: 4350,
            effectiveStack: 4350,
            activeOpponentSeats: [villainSeat],
            numOpponentsActive: 1,
            playerCount: 2,
            positionFromButton: 1,
            canCheck: false,
            canRaise: true,
            minRaiseTo: 500,
            maxRaiseTo: 4350,
            bigBlind: 200,
            wagerUnit: 100,
            tablePlayerCount: 10,
            startingStack: 5000,
            legalActions: ['fold', 'call', 'raise', 'allin'],
            publicPlayers: cfrPlayers(heroSeat, villainSeat, {
                heroPosition: 1,
                villainPosition: 0,
                heroCommitted: 350,
                villainCommitted: 650,
                villainRoundBet: 300,
                heroStack: 4350,
                villainStack: 4350
            })
        })
    });

    // Rejected CFR: a third live opponent keeps the general evaluator.
    cases.push({
        id: 'synthetic-cfr-rejected-multiway',
        style: 'TAG',
        variant: 'standard',
        holeCards: ['Ah', 'Kd'],
        tags: ['river-cfr-rejected', 'river', 'multiway', 'ten-player-25bb'],
        context: baseStandardContext({
            variant: 'standard',
            seatId: heroSeat,
            handId: 23,
            decisionId: `standard:river:7:${heroSeat}`,
            street: 'river',
            communityCards: CFR_BOARDS[0],
            pot: 1050,
            currentBet: 0,
            toCall: 0,
            yourBet: 0,
            stack: 4650,
            effectiveStack: 4650,
            activeOpponentSeats: [villainSeat, 5],
            numOpponentsActive: 2,
            playerCount: 3,
            positionFromButton: 1,
            canCheck: true,
            canRaise: true,
            minRaiseTo: 200,
            maxRaiseTo: 4650,
            bigBlind: 200,
            wagerUnit: 100,
            tablePlayerCount: 10,
            startingStack: 5000,
            legalActions: ['check', 'raise', 'allin'],
            publicPlayers: cfrPlayers(heroSeat, villainSeat, {
                heroPosition: 1,
                villainPosition: 0,
                extraLive: [5]
            })
        })
    });

    // Rejected CFR: general focus (default 6-max table).
    cases.push({
        id: 'synthetic-cfr-rejected-general-focus',
        style: 'LAG',
        variant: 'standard',
        holeCards: ['Ah', 'Kd'],
        tags: ['river-cfr-rejected', 'river', 'heads-up', 'general'],
        context: baseStandardContext({
            variant: 'standard',
            seatId: 1,
            handId: 24,
            decisionId: 'standard:river:5:1',
            street: 'river',
            communityCards: CFR_BOARDS[0],
            pot: 700,
            currentBet: 0,
            toCall: 0,
            yourBet: 0,
            stack: 19300,
            effectiveStack: 19300,
            activeOpponentSeats: [0],
            numOpponentsActive: 1,
            playerCount: 2,
            positionFromButton: 1,
            canCheck: true,
            canRaise: true,
            minRaiseTo: 100,
            maxRaiseTo: 19300,
            bigBlind: 100,
            wagerUnit: 50,
            tablePlayerCount: 6,
            startingStack: 20000,
            legalActions: ['check', 'raise', 'allin'],
            publicPlayers: [
                playerRow(0, { stack: 19300, committed: 350, roundBet: 0, positionFromButton: 0 }),
                playerRow(1, { stack: 19300, committed: 350, positionFromButton: 1 })
            ]
        })
    });

    return cases;
}

function syntheticShortdeckCases() {
    const cases = [];
    const basePublicPlayers = (heroSeat, stack, pot) => {
        const rows = [];
        for (let seat = 0; seat < 6; seat++) {
            if (seat === heroSeat) rows.push(playerRow(seat, { stack, committed: pot / 2, roundBet: 0, positionFromButton: heroSeat }));
            else rows.push(playerRow(seat, { stack: 5000, committed: 0, positionFromButton: seat, folded: true }));
        }
        return rows;
    };
    const heroSeat = 2;
    // Ante-structure preflop: 6 x 40 ante + dealer double ante (240+40=280 pot).
    for (const style of STYLE_ORDER) {
        cases.push({
            id: `synthetic-sd-style-${style.toLowerCase()}-preflop`,
            style,
            variant: 'shortdeck',
            aiType: 'ShortDeckAIPlayer',
            holeCards: ['Ah', 'Kh'],
            tags: ['shortdeck-local', 'style-sweep', 'preflop', 'open', `style-${style}`],
            context: baseStandardContext({
                variant: 'shortdeck',
                seatId: heroSeat,
                handId: 2,
                decisionId: `shortdeck:preflop:0:${heroSeat}`,
                street: 'preflop',
                pot: 280,
                currentBet: 0,
                toCall: 0,
                yourBet: 0,
                stack: 5000,
                effectiveStack: 5000,
                activeOpponentSeats: [0, 1, 3, 4, 5],
                numOpponentsActive: 5,
                playerCount: 6,
                positionFromButton: heroSeat,
                canCheck: true,
                canRaise: true,
                minRaiseTo: 80,
                maxRaiseTo: 5000,
                bigBlind: 80,
                wagerUnit: 40,
                tablePlayerCount: 6,
                startingStack: 5000,
                legalActions: ['check', 'raise', 'allin'],
                publicPlayers: [
                    playerRow(0, { stack: 4960, committed: 40, positionFromButton: 0 }),
                    playerRow(1, { stack: 4960, committed: 40, positionFromButton: 1 }),
                    playerRow(2, { stack: 5000, committed: 40, positionFromButton: 2 }),
                    playerRow(3, { stack: 4960, committed: 40, positionFromButton: 3 }),
                    playerRow(4, { stack: 4960, committed: 40, positionFromButton: 4 }),
                    playerRow(5, { stack: 4920, committed: 80, positionFromButton: 5 })
                ]
            })
        });
    }

    // Short-deck flop with a flush draw + open-ended straight draw.
    cases.push({
        id: 'synthetic-sd-flop-draw-facing-bet',
        style: 'LAG',
        variant: 'shortdeck',
        aiType: 'ShortDeckAIPlayer',
        holeCards: ['As', 'Ks'],
        tags: ['shortdeck-local', 'flop', 'facing'],
        context: baseStandardContext({
            variant: 'shortdeck',
            seatId: heroSeat,
            handId: 4,
            decisionId: `shortdeck:flop:1:${heroSeat}`,
            street: 'flop',
            communityCards: ['Qs', '9s', '6c'],
            pot: 900,
            currentBet: 300,
            toCall: 300,
            yourBet: 0,
            stack: 4800,
            effectiveStack: 4800,
            activeOpponentSeats: [0, 4],
            numOpponentsActive: 2,
            playerCount: 4,
            positionFromButton: 1,
            canCheck: false,
            canRaise: true,
            minRaiseTo: 600,
            maxRaiseTo: 4800,
            bigBlind: 80,
            wagerUnit: 40,
            tablePlayerCount: 6,
            startingStack: 5000,
            legalActions: ['fold', 'call', 'raise', 'allin'],
            publicPlayers: [
                playerRow(0, { stack: 4700, committed: 500, roundBet: 300, positionFromButton: 0 }),
                playerRow(1, { stack: 5000, committed: 200, folded: true, positionFromButton: 1 }),
                playerRow(2, { stack: 4800, committed: 200, positionFromButton: 2 }),
                playerRow(3, { stack: 4900, committed: 200, folded: true, positionFromButton: 3 }),
                playerRow(4, { stack: 4900, committed: 500, roundBet: 300, positionFromButton: 4 }),
                playerRow(5, { stack: 5000, committed: 200, folded: true, positionFromButton: 5 })
            ]
        })
    });

    // Short-deck river facing an all-in from the short stack.
    cases.push({
        id: 'synthetic-sd-river-facing-allin',
        style: 'CALLING_STATION',
        variant: 'shortdeck',
        aiType: 'ShortDeckAIPlayer',
        holeCards: ['Ad', 'Qd'],
        tags: ['shortdeck-local', 'river', 'facing', 'allin-opponent'],
        context: baseStandardContext({
            variant: 'shortdeck',
            seatId: heroSeat,
            handId: 6,
            decisionId: `shortdeck:river:4:${heroSeat}`,
            street: 'river',
            communityCards: ['Jh', 'Td', '8c', '6s', '2h'],
            pot: 2400,
            currentBet: 1800,
            toCall: 1800,
            yourBet: 0,
            stack: 3200,
            effectiveStack: 3200,
            activeOpponentSeats: [5],
            numOpponentsActive: 1,
            playerCount: 2,
            positionFromButton: 1,
            canCheck: false,
            canRaise: true,
            minRaiseTo: 3600,
            maxRaiseTo: 3200,
            bigBlind: 80,
            wagerUnit: 40,
            tablePlayerCount: 6,
            startingStack: 5000,
            legalActions: ['fold', 'call'],
            publicPlayers: [
                playerRow(2, { stack: 3200, committed: 600, positionFromButton: 1 }),
                playerRow(5, { stack: 0, committed: 2400, roundBet: 1800, allIn: true, positionFromButton: 0 })
            ]
        })
    });

    // Server profile equivalent of a short-deck preflop spot (AIPlayer with
    // variant shortdeck uses the standard STYLE_CONFIGS table, unlike the
    // local ShortDeckAIPlayer which uses SD_STYLE_CONFIGS).
    cases.push({
        id: 'synthetic-sd-server-preflop-bb-check',
        style: 'MANIAC',
        variant: 'shortdeck',
        aiType: 'AIPlayer',
        holeCards: ['Kd', 'Jd'],
        tags: ['shortdeck', 'preflop', 'open'],
        context: baseStandardContext({
            variant: 'shortdeck',
            seatId: 3,
            handId: 8,
            decisionId: 'shortdeck:preflop:0:3',
            street: 'preflop',
            pot: 280,
            currentBet: 0,
            toCall: 0,
            yourBet: 0,
            stack: 5000,
            effectiveStack: 5000,
            activeOpponentSeats: [0, 1, 2, 4, 5],
            numOpponentsActive: 5,
            playerCount: 6,
            positionFromButton: 3,
            canCheck: true,
            canRaise: true,
            minRaiseTo: 80,
            maxRaiseTo: 5000,
            bigBlind: 80,
            wagerUnit: 40,
            tablePlayerCount: 6,
            startingStack: 5000,
            legalActions: ['check', 'raise', 'allin'],
            publicPlayers: [
                playerRow(0, { stack: 4960, committed: 40, positionFromButton: 0 }),
                playerRow(1, { stack: 4960, committed: 40, positionFromButton: 1 }),
                playerRow(2, { stack: 4960, committed: 40, positionFromButton: 2 }),
                playerRow(3, { stack: 5000, committed: 40, positionFromButton: 3 }),
                playerRow(4, { stack: 4960, committed: 40, positionFromButton: 4 }),
                playerRow(5, { stack: 4920, committed: 80, positionFromButton: 5 })
            ]
        })
    });

    return cases;
}

function buildSyntheticCases() {
    const definitions = [
        ...syntheticStandardCases(),
        ...syntheticCfrCases(),
        ...syntheticShortdeckCases()
    ];
    return definitions.map(buildSyntheticCase);
}

// ---------------------------------------------------------------------------
// Trajectories: exact replay of one AI's observation + decision stream
// ---------------------------------------------------------------------------

function buildTrajectory(table, state, options = {}) {
    const maxHands = options.maxHands || 2;
    const maxObserveOps = options.maxObserveOps || 160;
    const lastHand = Math.min(...state.snapshots.map(snapshot => snapshot.handId).filter(handId => handId <= maxHands + 1).concat([maxHands + 1])) || maxHands;
    let snapshotCount = state.snapshots.filter(snapshot => snapshot.handId <= lastHand).length;
    if (snapshotCount < 2) snapshotCount = Math.min(state.snapshots.length, 4);
    if (snapshotCount < 1) return null;
    const lastSnapshot = state.snapshots[snapshotCount - 1];
    const ops = [];
    let cursor = 0;
    for (let index = 0; index < snapshotCount; index++) {
        const snapshot = state.snapshots[index];
        for (; cursor < snapshot.mark - snapshot.prefix.length; cursor++) ops.push(state.ops[cursor]);
        for (const op of snapshot.prefix) ops.push(op);
        cursor = snapshot.mark;
        ops.push({
            op: 'decide',
            aiState: snapshot.aiState,
            context: snapshot.context,
            expect: null
        });
    }
    // Cap observation volume by keeping the most recent ops window while
    // preserving decide ordering.
    let trimmed = ops;
    if (ops.length > maxObserveOps) {
        const excess = ops.length - maxObserveOps;
        let removed = 0;
        trimmed = ops.filter(op => {
            if (op.op !== 'decide' && removed < excess) { removed += 1; return false; }
            return true;
        });
    }
    const trajectory = {
        id: `trajectory-${table.spec.id}-s${state.seat}`,
        category: 'trajectory',
        tags: ['trajectory', table.spec.variant, table.spec.aiVariant],
        table: tableSummary(table.spec),
        ai: {
            type: table.spec.aiVariant === 'shortdeck-local' ? 'ShortDeckAIPlayer' : 'AIPlayer',
            variant: table.spec.variant,
            style: state.style,
            seatId: state.seat,
            name: AI_NAMES[state.seat % AI_NAMES.length],
            profileSource: table.spec.aiVariant === 'shortdeck-local' ? 'SD_STYLE_CONFIGS' : 'STYLE_CONFIGS',
            startingStack: table.spec.startingStack
        },
        ops: trimmed
    };
    const replayed = replayTrajectory(trajectory);
    for (let index = 0; index < trimmed.length; index++) {
        if (trimmed[index].op !== 'decide') continue;
        const production = state.snapshots.find(snapshot => snapshot.decisionId === trimmed[index].context.decisionId &&
            snapshot.handId === trimmed[index].context.handId);
        trimmed[index].expect = replayed[index];
        if (production && JSON.stringify(replayed[index]) !== JSON.stringify({
            action: production.decision.action,
            amount: production.decision.amount,
            rngCalls: production.rngCalls,
            trace: production.trace,
            raw: production.raw
        })) {
            throw new Error(`trajectory replay diverges from production decision ${trajectory.id}#${index}`);
        }
    }
    trajectory.inputSha256 = sha256(JSON.stringify(trajectory.ops.map(op => op.op === 'decide'
        ? { op: op.op, aiState: op.aiState, context: op.context }
        : op)));
    return trajectory;
}

function replayTrajectory(trajectory) {
    const ai = createCaseAi(trajectory.ai);
    let raw = null;
    if (ai.sharedBrain && typeof ai.sharedBrain.decide === 'function') {
        const originalBrainDecide = ai.sharedBrain.decide.bind(ai.sharedBrain);
        ai.sharedBrain.decide = function (rawContext) {
            const result = originalBrainDecide(rawContext);
            raw = captureRawResult(result);
            return result;
        };
    }
    const results = [];
    for (const op of trajectory.ops) {
        if (op.op === 'observe') {
            ai.recordOpponentAction(op.event.seatId, op.event.action, op.event.street, op.event.handId, cloneJson(op.event.meta));
        } else if (op.op === 'showdown') {
            ai.recordShowdown(op.seatId, cloneJson(op.info));
        } else if (op.op === 'decide') {
            applyAiState(ai, op.aiState);
            raw = null;
            beginRng();
            const decision = ai.decide(decisionContext(op.context));
            const rngCalls = endRng();
            results.push({
                action: decision.action,
                amount: decision.amount,
                rngCalls,
                trace: digestTrace(ai.lastDecisionTrace),
                raw
            });
        } else {
            throw new Error(`unknown trajectory op ${op.op}`);
        }
    }
    return results;
}

// ---------------------------------------------------------------------------
// Corpus assembly
// ---------------------------------------------------------------------------

const TABLE_SPECS = [
    { id: 'std-target-t1', variant: 'standard', aiVariant: 'standard', seats: 10, startingStack: 5000, smallBlind: 100, bigBlind: 200, hands: 20, seed: 0x5e1e0001, styleOffset: 0 },
    { id: 'std-target-t2', variant: 'standard', aiVariant: 'standard', seats: 10, startingStack: 5000, smallBlind: 100, bigBlind: 200, hands: 20, seed: 0x5e1e0002, styleOffset: 2 },
    { id: 'std-target-t3', variant: 'standard', aiVariant: 'standard', seats: 10, startingStack: 5000, smallBlind: 100, bigBlind: 200, hands: 20, seed: 0x5e1e0003, styleOffset: 4 },
    { id: 'std-general-t1', variant: 'standard', aiVariant: 'standard', seats: 6, startingStack: 20000, smallBlind: 50, bigBlind: 100, hands: 12, seed: 0x5e1e0011, styleOffset: 1 },
    { id: 'std-general-t2', variant: 'standard', aiVariant: 'standard', seats: 4, startingStack: 20000, smallBlind: 40, bigBlind: 80, hands: 12, seed: 0x5e1e0012, styleOffset: 3 },
    { id: 'std-general-t3', variant: 'standard', aiVariant: 'standard', seats: 9, startingStack: 20000, smallBlind: 50, bigBlind: 100, hands: 10, seed: 0x5e1e0013, styleOffset: 5 },
    { id: 'sd-local-t1', variant: 'shortdeck', aiVariant: 'shortdeck-local', seats: 6, startingStack: 5000, ante: 40, minBet: 80, hands: 12, seed: 0x5e1e0021, styleOffset: 0 },
    { id: 'sd-local-t2', variant: 'shortdeck', aiVariant: 'shortdeck-local', seats: 6, startingStack: 20000, ante: 100, minBet: 200, hands: 10, seed: 0x5e1e0022, styleOffset: 3 },
    { id: 'sd-server-t1', variant: 'shortdeck', aiVariant: 'shortdeck', seats: 6, startingStack: 5000, ante: 40, minBet: 80, hands: 12, seed: 0x5e1e0031, styleOffset: 1 },
    { id: 'sd-server-t2', variant: 'shortdeck', aiVariant: 'shortdeck', seats: 6, startingStack: 20000, ante: 100, minBet: 200, hands: 10, seed: 0x5e1e0032, styleOffset: 4 }
];

function writeJsonl(filePath, records) {
    const body = records.map(record => JSON.stringify(record)).join('\n') + '\n';
    fs.writeFileSync(filePath, body, 'utf8');
    return { path: filePath, body, bytes: Buffer.byteLength(body, 'utf8'), records: records.length, sha256: sha256(body) };
}

function generate(outDir) {
    const started = Date.now();
    REFS = loadReference();

    const tables = TABLE_SPECS.map(spec => {
        const table = runTable(spec);
        console.log(`  table ${spec.id}: hands=${table.handsPlayed} decisions=${table.decisions} errors=${table.errors}`);
        if (table.errors > 0) throw new Error(`table ${spec.id} produced errors: ${table.errors}`);
        return table;
    });

    const pools = {
        standardTarget: [],
        standardGeneral: [],
        shortdeckLocal: [],
        shortdeckServer: []
    };
    for (const table of tables) {
        for (const state of table.states) {
            for (const snapshot of state.snapshots) {
                const entry = harvestedPoolEntry(table, state, snapshot);
                if (table.spec.id.startsWith('std-target')) pools.standardTarget.push(entry);
                else if (table.spec.id.startsWith('std-general')) pools.standardGeneral.push(entry);
                else if (table.spec.aiVariant === 'shortdeck-local') pools.shortdeckLocal.push(entry);
                else pools.shortdeckServer.push(entry);
            }
        }
    }

    const usedInputs = new Set();
    const dedupe = cases => {
        const kept = [];
        for (const item of cases) {
            if (usedInputs.has(item.inputSha256)) continue;
            usedInputs.add(item.inputSha256);
            kept.push(item);
        }
        return kept;
    };

    const files = [];
    const targetEntries = selectEntries(pools.standardTarget, STANDARD_TARGET_RULES);
    const generalEntries = selectEntries(pools.standardGeneral, STANDARD_GENERAL_RULES);
    const localEntries = selectEntries(pools.shortdeckLocal, SHORTDECK_LOCAL_RULES);
    const serverEntries = selectEntries(pools.shortdeckServer, SHORTDECK_SERVER_RULES);

    const standardTargetCases = dedupe(targetEntries.map(entry => buildDecisionCase(entry, { category: 'harvested-standard-target' })))
        .sort((a, b) => a.id.localeCompare(b.id));
    const standardGeneralCases = dedupe(generalEntries.map(entry => buildDecisionCase(entry, { category: 'harvested-standard-general' })))
        .sort((a, b) => a.id.localeCompare(b.id));
    const shortdeckLocalCases = dedupe(localEntries.map(entry => buildDecisionCase(entry, { category: 'harvested-shortdeck-local' })))
        .sort((a, b) => a.id.localeCompare(b.id));
    const shortdeckServerCases = dedupe(serverEntries.map(entry => buildDecisionCase(entry, { category: 'harvested-shortdeck-server' })))
        .sort((a, b) => a.id.localeCompare(b.id));
    const shortdeckCases = dedupe([...shortdeckLocalCases, ...shortdeckServerCases])
        .sort((a, b) => a.id.localeCompare(b.id));
    const syntheticCases = dedupe(buildSyntheticCases()).sort((a, b) => a.id.localeCompare(b.id));

    const trajectorySpecs = [
        { tableId: 'std-target-t1', seat: 3, maxHands: 2 },
        { tableId: 'std-general-t1', seat: 1, maxHands: 2 },
        { tableId: 'sd-local-t1', seat: 2, maxHands: 2 },
        { tableId: 'sd-server-t1', seat: 0, maxHands: 2 }
    ];
    const trajectories = [];
    for (const spec of trajectorySpecs) {
        const table = tables.find(item => item.spec.id === spec.tableId);
        const state = table.states.find(item => item.seat === spec.seat);
        const trajectory = buildTrajectory(table, state, { maxHands: spec.maxHands });
        if (trajectory) trajectories.push(trajectory);
    }

    const manifest = {
        formatVersion: FORMAT_VERSION,
        algoVersion: ALGO_VERSION,
        strategyVersion: STRATEGY_VERSION,
        referenceCommit: REFERENCE_COMMIT,
        referenceRepo: 'texas-holdem-game',
        generator: 'oracle/golden-ai.js',
        generateCommand: 'node oracle/golden-ai.js',
        verifyCommand: 'node oracle/golden-ai.js --verify',
        determinism: {
            seedPolicy: 'all table shuffles use oracle/lib/prng mulberry32 with fixed integer seeds; every AI decision uses the JS SeededRng seeded by "<seed>:<handId>:<seatId>:<decisionId>:<style>"',
            clock: 'the vm sandbox exposes performance.now() === 0, so the AI wall-clock emergency brake never truncates the fixed sample count',
            mathRandom: `patched with mulberry32(0x${MATH_RANDOM_SEED.toString(16)}) before loading the reference modules`,
            noTimestamps: 'the corpus contains no wall-clock or generated-at fields; two runs must produce byte-identical files'
        },
        sourceSha256: Object.fromEntries(SOURCE_HASH_FILES.map(relative => [
            relative,
            sha256(fs.readFileSync(path.join(REFS.repo, relative)))
        ])),
        recompenseNote: 'recording format version 1; see docs/specs/ai-strategy.md §9 for field semantics and comparison tolerances',
        rngVectors: buildRngVectors(),
        files: [],
        counts: {}
    };

    files.push(writeJsonl(path.join(outDir, 'decisions-standard-target.jsonl'), standardTargetCases));
    files.push(writeJsonl(path.join(outDir, 'decisions-standard-general.jsonl'), standardGeneralCases));
    files.push(writeJsonl(path.join(outDir, 'decisions-shortdeck.jsonl'), shortdeckCases));
    files.push(writeJsonl(path.join(outDir, 'decisions-synthetic.jsonl'), syntheticCases));
    files.push(writeJsonl(path.join(outDir, 'trajectories.jsonl'), trajectories));

    const allDecisions = [...standardTargetCases, ...standardGeneralCases, ...shortdeckCases, ...syntheticCases];
    const countBy = (items, pick) => items.reduce((bucket, item) => {
        const key = pick(item);
        bucket[key] = (bucket[key] || 0) + 1;
        return bucket;
    }, {});
    manifest.counts = {
        decisions: allDecisions.length,
        harvested: allDecisions.filter(item => item.category.startsWith('harvested')).length,
        synthetic: allDecisions.filter(item => item.category === 'synthetic').length,
        riverCfr: allDecisions.filter(item => item.tags.includes('river-cfr')).length,
        trajectories: trajectories.length,
        trajectoryDecisions: trajectories.reduce((sum, trajectory) => sum + trajectory.ops.filter(op => op.op === 'decide').length, 0),
        byVariant: countBy(allDecisions, item => item.ai.variant),
        byCategory: countBy(allDecisions, item => item.category),
        byStreet: countBy(allDecisions, item => (item.tags.includes('preflop') ? 'preflop' : item.tags.includes('flop') ? 'flop' : item.tags.includes('turn') ? 'turn' : item.tags.includes('river') ? 'river' : 'unknown')),
        byStyle: countBy(allDecisions, item => item.ai.style),
        byAction: countBy(allDecisions, item => item.expect.action)
    };
    manifest.files = files.map(file => ({
        path: path.basename(file.path),
        records: file.records,
        bytes: file.bytes,
        sha256: file.sha256
    }));
    manifest.corpusSha256 = sha256(manifest.files.map(file => file.sha256).join('\n'));
    const manifestBody = JSON.stringify(manifest, null, 2) + '\n';
    fs.writeFileSync(path.join(outDir, 'manifest.json'), manifestBody, 'utf8');

    verifyCorpus(outDir, { quiet: true });

    console.log(`  decisions: ${manifest.counts.decisions} (river CFR ${manifest.counts.riverCfr}, synthetic ${manifest.counts.synthetic})`);
    console.log(`  trajectories: ${manifest.counts.trajectories} (${manifest.counts.trajectoryDecisions} decisions)`);
    console.log(`  corpusSha256: ${manifest.corpusSha256}`);
    console.log(`  elapsedMs: ${Date.now() - started}`);
    return manifest;
}

function buildRngVectors() {
    const seedTexts = [
        'table:1:0:1:SOLID',
        'poker-table:standard:12:3:standard:river:5:3:TAG',
        'poker-table:shortdeck:4:2:shortdeck:flop:1:2:LAG'
    ];
    const hashSeed = {};
    const next8 = {};
    for (const seedText of seedTexts) {
        hashSeed[seedText] = REFS.W.PokerAICore.hashSeed(seedText);
        const rng = new REFS.W.PokerAICore.SeededRng(seedText);
        next8[seedText] = Array.from({ length: 8 }, () => rng.next());
    }
    return { note: 'ai-core.js SeededRng = FNV-1a hashSeed + mulberry32-style next(); short-deck stable core is identical', hashSeed, next8 };
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

function readJsonl(filePath) {
    const text = fs.readFileSync(filePath, 'utf8');
    return text.split('\n').filter(line => line.length > 0).map(line => JSON.parse(line));
}

function compareValues(actual, expected, pathText, failures) {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
        failures.push({ path: pathText, actual, expected });
        return false;
    }
    return true;
}

function verifyCorpus(outDir, options = {}) {
    REFS = REFS || loadReference();
    const manifest = JSON.parse(fs.readFileSync(path.join(outDir, 'manifest.json'), 'utf8'));
    const failures = [];
    for (const file of manifest.files) {
        const filePath = path.join(outDir, file.path);
        const body = fs.readFileSync(filePath, 'utf8');
        if (sha256(body) !== file.sha256) failures.push({ path: file.path, error: 'sha256 mismatch' });
        const records = readJsonl(filePath);
        if (records.length !== file.records) failures.push({ path: file.path, error: 'record count mismatch' });
        if (file.path === 'trajectories.jsonl') {
            for (const trajectory of records) {
                const replayed = replayTrajectory(trajectory);
                const expected = trajectory.ops.filter(op => op.op === 'decide').map(op => op.expect);
                compareValues(replayed, expected, `${file.path}:${trajectory.id}`, failures);
            }
        } else {
            for (const record of records) {
                const replayed = replayDecision(record.ai, record.aiState, record.observes, record.context);
                compareValues(replayed, record.expect, `${file.path}:${record.id}`, failures);
            }
        }
    }
    if (failures.length) {
        console.error(`verify failed: ${failures.length} mismatch(es)`);
        console.error(JSON.stringify(failures.slice(0, 10), null, 2));
        process.exitCode = 1;
        return null;
    }
    if (!options.quiet) {
        console.log(`verify ok: ${manifest.counts.decisions} decisions + ${manifest.counts.trajectoryDecisions} trajectory decisions, corpusSha256=${manifest.corpusSha256}`);
    }
    return manifest;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
    const args = Object.fromEntries(process.argv.slice(2).map(item => {
        const [key, ...rest] = item.replace(/^--/, '').split('=');
        return [key, rest.join('=')];
    }));
    const outDir = path.resolve(args.out || DEFAULT_OUT_DIR);
    if (args.verify) {
        verifyCorpus(outDir);
        return;
    }
    fs.mkdirSync(outDir, { recursive: true });
    reflectMathRandom();
    const manifest = generate(outDir);
    if (!manifest) process.exitCode = 1;
}

function reflectMathRandom() {
    // Patch before the reference modules are loaded (golden-corpus rule §5.3).
    Math.random = mulberry32(MATH_RANDOM_SEED);
}

if (require.main === module) {
    main();
}

module.exports = { loadReference, generate, verifyCorpus, replayDecision, replayTrajectory };
