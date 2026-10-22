'use strict';
// Exports scripted golden hands for the headless betting engine from the
// original server implementation (server/server-game.js ServerPokerGame, which
// is the strict, injectable-RNG variant of the shared js/engine.js kernel).
//
// Each scenario fixes: config, seat stacks, the shuffled deck order for every
// hand, a deterministic scripted action policy, and full state snapshots after
// startHand and after every action. Rust replays the same deck order and
// actions and must reproduce every snapshot exactly.
//
// Usage: node oracle/golden-engine.js
const fs = require('node:fs');
const path = require('node:path');
const { mulberry32 } = require('./lib/prng');
const { referenceRepo } = require('./lib/load-js');
const { ServerPokerGame } = require(path.join(referenceRepo(), 'server/server-game.js'));

const outDir = path.resolve(__dirname, '..', 'tests', 'golden', 'engine');
fs.mkdirSync(outDir, { recursive: true });

const RANK_TOKEN = { 2:'2',3:'3',4:'4',5:'5',6:'6',7:'7',8:'8',9:'9',10:'T',11:'J',12:'Q',13:'K',14:'A' };
const SUIT_CODE = { spades:'s', hearts:'h', diamonds:'d', clubs:'c' };
function label(card) { return RANK_TOKEN[card.value] + SUIT_CODE[card.suit]; }

function makePlayers(count, startingStack) {
    const players = [];
    for (let seat = 0; seat < count; seat++) {
        players.push({
            id: seat === 0 ? 'human' : 'ai-' + seat,
            seatId: seat,
            name: seat === 0 ? 'You' : 'AI' + seat,
            avatar: '',
            isHuman: seat === 0,
            connected: true,
            left: false,
            stack: startingStack,
            holeCards: [],
            chipsInPot: 0,
            roundBet: 0,
            folded: false,
            isAllIn: false,
            dealtIn: false,
            lastAction: null,
            aiRef: null
        });
    }
    return players;
}

function lastActionOf(player) {
    if (!player.lastAction) return null;
    return { action: player.lastAction.action, amount: player.lastAction.amount };
}

function snapshot(game) {
    return {
        phase: game.phase,
        pot: game.pot,
        currentBet: game.currentBet,
        lastRaise: game.lastRaise,
        hasFullBetThisRound: !!game.hasFullBetThisRound,
        dealerPos: game.dealerPos,
        sbIdx: game.sbIdx,
        bbIdx: game.bbIdx,
        currentPlayerIdx: game.currentPlayerIdx,
        preflopRaiseCount: game.preflopRaiseCount,
        preflopAggressor: game.preflopAggressor,
        lastAggressor: game.lastAggressor,
        lastAggressorStreet: game.lastAggressorStreet,
        community: game.communityCards.map(label),
        deckLen: game.deck.length,
        roundBets: game.players.map((_, idx) => game.roundBets[idx] || 0),
        players: game.players.map(player => ({
            stack: player.stack,
            chipsInPot: player.chipsInPot,
            roundBet: player.roundBet,
            folded: !!player.folded,
            isAllIn: !!player.isAllIn,
            dealtIn: !!player.dealtIn,
            holeCards: (player.holeCards || []).map(label),
            lastAction: lastActionOf(player)
        }))
    };
}

function legalOf(legal) {
    return {
        actions: [...legal.actions],
        toCall: legal.toCall,
        canRaise: !!legal.canRaise,
        minRaiseTo: legal.minRaiseTo,
        maxRaiseTo: legal.maxRaiseTo,
        wagerUnit: legal.wagerUnit
    };
}

// startHand() creates the deck, shuffles it, then deals two rounds of hole
// cards (deck.pop() from the end). To replay in Rust we need the full order
// before any deal: remaining deck followed by the popped cards in reverse deal
// order. Deal order is two clockwise rounds over the dealt-in players.
function nextEligibleFrom(game, start, eligible) {
    const n = game.players.length;
    for (let step = 1; step < n; step++) {
        const idx = (start + step + n) % n;
        if (eligible.includes(idx)) return idx;
    }
    return -1;
}

function dealOrder(game) {
    const eligible = game.players.map((player, idx) => player.dealtIn ? idx : -1).filter(idx => idx >= 0);
    const order = [];
    let pos = game.dealerPos;
    for (let count = 0; count < eligible.length; count++) {
        pos = nextEligibleFrom(game, pos, eligible);
        order.push(pos);
    }
    return order;
}

// Captures the freshly shuffled full deck at the exact moment startHand
// assigns it (before any card is dealt). startHand may auto-deal community
// cards when everyone is all-in, so reconstructing from the leftover deck is
// not safe; the setter records the authoritative order.
function installDeckCapture(game) {
    let deckRef = [];
    let captured = null;
    Object.defineProperty(game, 'deck', {
        configurable: true,
        get: () => deckRef,
        set: (value) => {
            deckRef = value;
            captured = value.map(label);
        }
    });
    return {
        take() {
            const current = captured;
            captured = null;
            return current;
        }
    };
}

function verifyDeckCapture(game, deckOrder) {
    const order = dealOrder(game);
    const copy = deckOrder.slice();
    for (let round = 0; round < 2; round++) {
        for (const seat of order) {
            if (copy.pop() !== label(game.players[seat].holeCards[round])) {
                throw new Error('deck capture mismatch for hand ' + game.handId + ' seat ' + seat);
            }
        }
    }
    return deckOrder;
}

function resultSummary(result) {
    if (!result) return null;
    return {
        reason: result.reason,
        isShortDeck: !!result.isShortDeck,
        pot: result.pot,
        handRank: result.handRank,
        winners: result.winners.map(player => player.id),
        winnerAmounts: result.winnerAmounts,
        results: result.results.map(entry => ({ id: entry.id, handRank: entry.handRank }))
    };
}

function chooseWeighted(rng, entries) {
    const total = entries.reduce((sum, entry) => sum + entry.weight, 0);
    let roll = rng() * total;
    for (const entry of entries) {
        roll -= entry.weight;
        if (roll <= 0) return entry.value;
    }
    return entries[entries.length - 1].value;
}

function pickAction(rng, legal, aggression) {
    const toCall = legal.toCall;
    const has = action => legal.actions.includes(action);
    const raiseCandidates = () => {
        if (!has('raise')) return [];
        const unit = legal.wagerUnit;
        const low = Math.ceil(legal.minRaiseTo / unit) * unit;
        const candidates = [];
        for (let target = low; target <= legal.maxRaiseTo; target += unit) candidates.push(target);
        return candidates;
    };
    if (toCall > 0) {
        const options = [{ value: 'fold', weight: 1 }];
        if (has('call')) options.push({ value: 'call', weight: 6 });
        if (has('raise')) options.push({ value: 'raise', weight: 3 * aggression });
        if (has('allin')) options.push({ value: 'allin', weight: 1.2 * aggression });
        const action = chooseWeighted(rng, options);
        if (action === 'raise') {
            const candidates = raiseCandidates();
            const target = candidates.length ? candidates[Math.floor(rng() * candidates.length)] : legal.minRaiseTo;
            return { action, amount: target };
        }
        return { action, amount: 0 };
    }
    const options = [];
    if (has('check')) options.push({ value: 'check', weight: 5 });
    if (has('raise')) options.push({ value: 'raise', weight: 4 * aggression });
    if (has('allin')) options.push({ value: 'allin', weight: 0.8 * aggression });
    if (!options.length) return { action: 'check', amount: 0 };
    const action = chooseWeighted(rng, options);
    if (action === 'raise') {
        const candidates = raiseCandidates();
        const target = candidates.length ? candidates[Math.floor(rng() * candidates.length)] : legal.minRaiseTo;
        return { action, amount: target };
    }
    return { action, amount: 0 };
}

const SCENARIOS = [
    { name: 'std-6max-normal', isShortDeck: false, players: 6, stack: 20000, smallBlind: 50, bigBlind: 100, aggression: 0.35, hands: 40, shuffleSeed: 0x111101, actionSeed: 0xa11001 },
    { name: 'std-9max-shortstacks', isShortDeck: false, players: 9, stack: 3000, smallBlind: 50, bigBlind: 100, aggression: 0.7, hands: 30, shuffleSeed: 0x111102, actionSeed: 0xa11002 },
    { name: 'std-headsup', isShortDeck: false, players: 2, stack: 10000, smallBlind: 50, bigBlind: 100, aggression: 0.5, hands: 50, shuffleSeed: 0x111103, actionSeed: 0xa11003 },
    { name: 'std-10max-busting', isShortDeck: false, players: 10, stack: 2000, smallBlind: 20, bigBlind: 40, aggression: 0.6, hands: 40, shuffleSeed: 0x111104, actionSeed: 0xa11004 },
    { name: 'sd-6max-normal', isShortDeck: true, players: 6, stack: 20000, ante: 40, minBet: 80, aggression: 0.4, hands: 40, shuffleSeed: 0x111105, actionSeed: 0xa11005 },
    { name: 'sd-headsup', isShortDeck: true, players: 2, stack: 10000, ante: 40, minBet: 80, aggression: 0.55, hands: 40, shuffleSeed: 0x111106, actionSeed: 0xa11006 }
];

function runScenario(scenario) {
    const config = {
        maxPlayers: scenario.players,
        isShortDeck: scenario.isShortDeck,
        startingStack: scenario.stack,
        smallBlind: scenario.smallBlind,
        bigBlind: scenario.bigBlind,
        ante: scenario.ante,
        minBet: scenario.minBet
    };
    const shuffleRng = mulberry32(scenario.shuffleSeed);
    const players = makePlayers(scenario.players, scenario.stack);
    const game = new ServerPokerGame(config, players, { random: shuffleRng });
    const deckCapture = installDeckCapture(game);
    const actionRng = mulberry32(scenario.actionSeed);
    const hands = [];
    let safetyHands = 0;
    while (hands.length < scenario.hands && safetyHands++ < scenario.hands * 2) {
        const preStacks = players.map(player => player.stack);
        const started = game.startHand();
        const capturedDeck = deckCapture.take();
        if (started && !capturedDeck) throw new Error('deck capture missing for hand ' + game.handId);
        const handRecord = {
            handId: game.handId,
            started,
            preStacks,
            deck: started ? verifyDeckCapture(game, capturedDeck) : [],
            start: started ? snapshot(game) : null,
            steps: [],
            result: null,
            final: null
        };
        if (started) {
            let steps = 0;
            while (game.phase !== 'idle' && steps++ < 400) {
                const idx = game.currentPlayerIdx;
                const legal = game.legalActions(idx);
                if (!legal.actions.length) throw new Error('no legal actions for seat ' + idx + ' phase ' + game.phase);
                const choice = pickAction(actionRng, legal, scenario.aggression);
                const before = snapshot(game);
                const legalBefore = legalOf(legal);
                const outcome = game.act(idx, choice.action, choice.amount);
                if (!outcome.ok) {
                    throw new Error('scripted action rejected: ' + JSON.stringify(choice) + ' -> ' + outcome.error + ' legal=' + JSON.stringify(legal));
                }
                handRecord.steps.push({ action: choice, legalBefore, before, after: snapshot(game) });
            }
            if (game.phase !== 'idle') throw new Error('hand did not finish in ' + steps + ' steps');
            handRecord.result = resultSummary(game.lastHandResult);
            handRecord.final = snapshot(game);
        }
        hands.push(handRecord);
        const alive = players.filter(player => player.stack > 0).length;
        if (alive < 2) {
            // Refill stacks so the scenario keeps exercising deals; the golden
            // test only replays up to the recorded hand count.
            for (const player of players) {
                if (player.stack <= 0) player.stack = scenario.stack;
            }
        }
    }
    return { scenario: { ...scenario, config }, hands };
}

const started = Date.now();
let totalHands = 0;
let totalSteps = 0;
for (const scenario of SCENARIOS) {
    const data = runScenario(scenario);
    totalHands += data.hands.length;
    for (const hand of data.hands) totalSteps += hand.steps.length;
    const file = path.join(outDir, scenario.name + '.json');
    fs.writeFileSync(file, JSON.stringify(data));
    const bytes = fs.statSync(file).size;
    console.log(scenario.name, 'hands:', data.hands.length, 'steps:', data.hands.reduce((sum, hand) => sum + hand.steps.length, 0), 'bytes:', bytes);
}
console.log('total hands:', totalHands, 'total steps:', totalSteps, 'elapsedMs:', Date.now() - started);
