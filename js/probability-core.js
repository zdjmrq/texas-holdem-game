(function(root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    else root.PokerProbabilityCore = api;
})(typeof self !== 'undefined' ? self : globalThis, function() {
    'use strict';

    const SUITS = ['spades', 'hearts', 'diamonds', 'clubs'];
    const STANDARD_RANKS = ['2','3','4','5','6','7','8','9','10','J','Q','K','A'];
    const SHORT_RANKS = ['6','7','8','9','10','J','Q','K','A'];
    const VALUES = { '2':2, '3':3, '4':4, '5':5, '6':6, '7':7, '8':8, '9':9, '10':10, J:11, Q:12, K:13, A:14 };
    const RANK_LABELS = Object.fromEntries(Object.entries(VALUES).map(([rank, value]) => [value, rank]));
    const STANDARD_NAMES = ['高牌','一对','两对','三条','顺子','同花','葫芦','四条','同花顺','皇家同花顺'];
    const SHORT_NAMES = ['高牌','一对','两对','三条','顺子','葫芦','同花','四条','同花顺','皇家同花顺'];

    function encode(card) {
        const value = VALUES[card?.rank] || Number(card?.value);
        const suit = SUITS.indexOf(card?.suit);
        if (!Number.isFinite(value) || value < 2 || value > 14 || suit < 0) throw new Error('Invalid card');
        return suit * 15 + value;
    }

    function decode(code) {
        const value = code % 15;
        return { rank:RANK_LABELS[value], suit:SUITS[Math.floor(code / 15)], value };
    }

    function score(rank, values) {
        let total = rank * 10 ** 10;
        for (let index = 0; index < values.length; index++) total += values[index] * 10 ** (8 - index * 2);
        return total;
    }

    function straightHigh(counts, isShortDeck) {
        for (let high = 14; high >= (isShortDeck ? 10 : 6); high--) {
            let present = true;
            for (let value = high; value > high - 5; value--) {
                if (!counts[value]) { present = false; break; }
            }
            if (present) return high;
        }
        if (!isShortDeck && counts[14] && counts[5] && counts[4] && counts[3] && counts[2]) return 5;
        if (isShortDeck && counts[14] && counts[9] && counts[8] && counts[7] && counts[6]) return 9;
        return 0;
    }

    /** Direct 5-7 card evaluator; avoids allocating 21 five-card combinations. */
    function evaluateCodes(codes, isShortDeck = false) {
        const names = isShortDeck ? SHORT_NAMES : STANDARD_NAMES;
        const ranks = isShortDeck
            ? { high:0, pair:1, two:2, trips:3, straight:4, full:5, flush:6, quads:7, sf:8, royal:9 }
            : { high:0, pair:1, two:2, trips:3, straight:4, flush:5, full:6, quads:7, sf:8, royal:9 };
        const counts = new Int8Array(15);
        const suitValues = [[], [], [], []];
        for (const code of codes) {
            const value = code % 15;
            const suit = Math.floor(code / 15);
            counts[value]++;
            suitValues[suit].push(value);
        }

        if (codes.length < 5) {
            const groups = [];
            for (let value = 14; value >= 2; value--) if (counts[value]) groups.push({ value, count:counts[value] });
            groups.sort((a, b) => b.count - a.count || b.value - a.value);
            let rank = ranks.high;
            let kickers = groups.map(group => group.value);
            if (groups[0]?.count === 4) { rank = ranks.quads; kickers = [groups[0].value, ...groups.slice(1).map(g => g.value)]; }
            else if (groups[0]?.count === 3 && groups[1]?.count >= 2) { rank = ranks.full; kickers = [groups[0].value, groups[1].value]; }
            else if (groups[0]?.count === 3) { rank = ranks.trips; kickers = [groups[0].value, ...groups.slice(1).map(g => g.value)]; }
            else if (groups[0]?.count === 2 && groups[1]?.count === 2) { rank = ranks.two; kickers = [groups[0].value, groups[1].value, ...groups.slice(2).map(g => g.value)]; }
            else if (groups[0]?.count === 2) { rank = ranks.pair; kickers = [groups[0].value, ...groups.slice(1).map(g => g.value)]; }
            return { rank, score:score(rank, kickers.slice(0, 5)), name:names[rank] };
        }

        for (const values of suitValues) {
            if (values.length < 5) continue;
            const flushCounts = new Int8Array(15);
            for (const value of values) flushCounts[value]++;
            const high = straightHigh(flushCounts, isShortDeck);
            if (high) {
                const rank = high === 14 ? ranks.royal : ranks.sf;
                return { rank, score:rank * 10 ** 10 + (high === 14 ? 0 : high * 10 ** 8), name:names[rank] };
            }
        }

        let quads = 0;
        const trips = [];
        const pairs = [];
        const ordered = [];
        for (let value = 14; value >= 2; value--) {
            if (!counts[value]) continue;
            ordered.push(value);
            if (counts[value] === 4) quads = Math.max(quads, value);
            if (counts[value] >= 3) trips.push(value);
            if (counts[value] >= 2) pairs.push(value);
        }
        if (quads) {
            const kicker = ordered.find(value => value !== quads);
            return { rank:ranks.quads, score:score(ranks.quads, [quads, kicker]), name:names[ranks.quads] };
        }
        if (!isShortDeck && trips.length && pairs.some(value => value !== trips[0])) {
            const pair = pairs.find(value => value !== trips[0]);
            return { rank:ranks.full, score:score(ranks.full, [trips[0], pair]), name:names[ranks.full] };
        }

        let flushValues = null;
        for (const values of suitValues) {
            if (values.length < 5) continue;
            const top = values.slice().sort((a, b) => b - a).slice(0, 5);
            if (!flushValues || score(0, top) > score(0, flushValues)) flushValues = top;
        }
        if (isShortDeck && flushValues) return { rank:ranks.flush, score:score(ranks.flush, flushValues), name:names[ranks.flush] };
        if (isShortDeck && trips.length && pairs.some(value => value !== trips[0])) {
            const pair = pairs.find(value => value !== trips[0]);
            return { rank:ranks.full, score:score(ranks.full, [trips[0], pair]), name:names[ranks.full] };
        }
        if (flushValues) return { rank:ranks.flush, score:score(ranks.flush, flushValues), name:names[ranks.flush] };

        const highStraight = straightHigh(counts, isShortDeck);
        if (highStraight) return { rank:ranks.straight, score:ranks.straight * 10 ** 10 + highStraight * 10 ** 8, name:names[ranks.straight] };
        if (trips.length) {
            const kickers = ordered.filter(value => value !== trips[0]).slice(0, 2);
            return { rank:ranks.trips, score:score(ranks.trips, [trips[0], ...kickers]), name:names[ranks.trips] };
        }
        if (pairs.length >= 2) {
            const bestPairs = pairs.slice(0, 2);
            const kicker = ordered.find(value => !bestPairs.includes(value));
            return { rank:ranks.two, score:score(ranks.two, [...bestPairs, kicker]), name:names[ranks.two] };
        }
        if (pairs.length === 1) {
            const kickers = ordered.filter(value => value !== pairs[0]).slice(0, 3);
            return { rank:ranks.pair, score:score(ranks.pair, [pairs[0], ...kickers]), name:names[ranks.pair] };
        }
        return { rank:ranks.high, score:score(ranks.high, ordered.slice(0, 5)), name:names[ranks.high] };
    }

    function createDeck(isShortDeck) {
        const deck = [];
        const ranks = isShortDeck ? SHORT_RANKS : STANDARD_RANKS;
        for (let suit = 0; suit < SUITS.length; suit++) {
            for (const rank of ranks) deck.push(suit * 15 + VALUES[rank]);
        }
        return deck;
    }

    function hashSeed(value) {
        const text = String(value || 'poker-probability');
        let hash = 2166136261;
        for (let i = 0; i < text.length; i++) {
            hash ^= text.charCodeAt(i);
            hash = Math.imul(hash, 16777619);
        }
        return hash >>> 0 || 0x9e3779b9;
    }

    function randomFactory(seed) {
        let state = hashSeed(seed);
        return function() {
            state ^= state << 13;
            state ^= state >>> 17;
            state ^= state << 5;
            return (state >>> 0) / 4294967296;
        };
    }

    function prepare(options) {
        const isShortDeck = !!options.isShortDeck;
        const player = (options.playerCards || []).map(encode);
        const board = (options.communityCards || []).map(encode);
        if (player.length !== 2 || board.length > 5) throw new Error('Texas Hold’em needs two hole cards and at most five board cards');
        const known = new Set([...player, ...board]);
        if (known.size !== player.length + board.length) throw new Error('Duplicate known card');
        const deck = createDeck(isShortDeck).filter(code => !known.has(code));
        const opponents = Math.max(1, Math.min(isShortDeck ? 7 : 9, Math.floor(Number(options.numOpponents) || 1)));
        if (deck.length < 5 - board.length + opponents * 2) throw new Error('Too many opponents for remaining cards');
        return { isShortDeck, player, board, deck, opponents };
    }

    function finishResult(wins, ties, losses, equity, samples, method, stats, elapsedMs) {
        const total = Math.max(1, samples);
        return {
            winProb:wins / total,
            tieProb:ties / total,
            loseProb:losses / total,
            equityProb:equity / total,
            samples,
            method,
            elapsedMs,
            stats:{ bestHandCounts:stats }
        };
    }

    function resolveShowdown(playerScore, opponentScores) {
        const best = Math.max(...opponentScores);
        if (playerScore > best) return { win:1, tie:0, loss:0, equity:1 };
        if (playerScore < best) return { win:0, tie:0, loss:1, equity:0 };
        const tied = opponentScores.filter(value => value === playerScore).length;
        return { win:0, tie:1, loss:0, equity:1 / (tied + 1) };
    }

    function exactHeadsUp(prepared) {
        const started = Date.now();
        const { player, board, deck, isShortDeck } = prepared;
        const missing = 5 - board.length;
        let wins = 0, ties = 0, losses = 0, equity = 0, samples = 0;
        const stats = {};
        const riverChoices = missing === 1 ? deck.map((_, index) => index) : [-1];
        for (const riverIndex of riverChoices) {
            const fullBoard = riverIndex >= 0 ? [...board, deck[riverIndex]] : board;
            const playerHand = evaluateCodes([...player, ...fullBoard], isShortDeck);
            let riverSamples = 0;
            for (let first = 0; first < deck.length - 1; first++) {
                if (first === riverIndex) continue;
                for (let second = first + 1; second < deck.length; second++) {
                    if (second === riverIndex) continue;
                    const opponent = evaluateCodes([deck[first], deck[second], ...fullBoard], isShortDeck);
                    const result = resolveShowdown(playerHand.score, [opponent.score]);
                    wins += result.win; ties += result.tie; losses += result.loss; equity += result.equity; samples++;
                    riverSamples++;
                }
            }
            stats[playerHand.name] = (stats[playerHand.name] || 0) + Math.max(1, riverSamples);
        }
        return finishResult(wins, ties, losses, equity, samples, 'exact-heads-up', stats, Date.now() - started);
    }

    function monteCarlo(prepared, options) {
        const started = typeof performance !== 'undefined' ? performance.now() : Date.now();
        const now = () => typeof performance !== 'undefined' ? performance.now() : Date.now();
        const { player, board, deck, opponents, isShortDeck } = prepared;
        const missing = 5 - board.length;
        const drawCount = missing + opponents * 2;
        const pool = deck.slice();
        const swaps = new Int16Array(drawCount);
        const rng = randomFactory(options.seed);
        const targetSamples = Math.max(400, Math.min(12000, Math.floor(Number(options.simulations) || 2400)));
        const minSamples = Math.min(targetSamples, Math.max(350, Math.floor(Number(options.minSamples) || 700)));
        const timeBudgetMs = Math.max(40, Math.min(2000, Number(options.timeBudgetMs) || 450));
        let wins = 0, ties = 0, losses = 0, equity = 0, samples = 0;
        const stats = {};
        while (samples < targetSamples) {
            for (let index = 0; index < drawCount; index++) {
                const swap = index + Math.floor(rng() * (pool.length - index));
                swaps[index] = swap;
                [pool[index], pool[swap]] = [pool[swap], pool[index]];
            }
            const fullBoard = board.concat(pool.slice(0, missing));
            const playerHand = evaluateCodes([...player, ...fullBoard], isShortDeck);
            const opponentScores = [];
            let cursor = missing;
            for (let opponent = 0; opponent < opponents; opponent++) {
                opponentScores.push(evaluateCodes([pool[cursor++], pool[cursor++], ...fullBoard], isShortDeck).score);
            }
            const result = resolveShowdown(playerHand.score, opponentScores);
            wins += result.win; ties += result.tie; losses += result.loss; equity += result.equity;
            stats[playerHand.name] = (stats[playerHand.name] || 0) + 1;
            samples++;
            for (let index = drawCount - 1; index >= 0; index--) {
                const swap = swaps[index];
                [pool[index], pool[swap]] = [pool[swap], pool[index]];
            }
            if (samples >= minSamples && samples % 32 === 0 && now() - started >= timeBudgetMs) break;
        }
        return finishResult(wins, ties, losses, equity, samples, 'monte-carlo', stats, now() - started);
    }

    function findImprovements(playerCards, communityCards, isShortDeck = false) {
        if (!Array.isArray(communityCards) || communityCards.length < 3 || communityCards.length >= 5) {
            return { totalOuts:0, strongOuts:0, thinOuts:0, byHandType:{}, note:'翻牌后才显示牌型改善牌' };
        }
        const player = playerCards.map(encode);
        const board = communityCards.map(encode);
        const known = new Set([...player, ...board]);
        const deck = createDeck(isShortDeck).filter(code => !known.has(code));
        const before = evaluateCodes([...player, ...board], isShortDeck);
        const byHandType = {};
        let strongOuts = 0;
        let thinOuts = 0;
        for (const code of deck) {
            const after = evaluateCodes([...player, ...board, code], isShortDeck);
            if (after.score <= before.score) continue;
            // Do not call ordinary high-card movement an "out". Same-category
            // changes are only retained for made hands and meaningful kickers.
            if (after.rank === before.rank && (before.rank === 0 || after.score - before.score < 10 ** 6)) continue;
            const quality = after.rank > before.rank ? 'strong' : 'thin';
            if (quality === 'strong') strongOuts++;
            else thinOuts++;
            if (!byHandType[after.name]) byHandType[after.name] = { cards:[], count:0, strongCount:0, thinCount:0 };
            const group = byHandType[after.name];
            group.cards.push({ ...decode(code), quality });
            group.count++;
            if (quality === 'strong') group.strongCount++;
            else group.thinCount++;
        }
        return {
            totalOuts:strongOuts + thinOuts,
            strongOuts,
            thinOuts,
            byHandType,
            note:'强提升会提高牌型等级；踢脚改善只增强同一牌型，均不代表必胜'
        };
    }

    function calculate(options = {}) {
        const prepared = prepare(options);
        const useExact = prepared.opponents === 1 && prepared.board.length >= 4;
        const result = useExact ? exactHeadsUp(prepared) : monteCarlo(prepared, options);
        result.outs = findImprovements(options.playerCards, options.communityCards, prepared.isShortDeck);
        return result;
    }

    function evaluate(cards, isShortDeck = false) {
        return evaluateCodes(cards.map(encode), isShortDeck);
    }

    return { calculate, evaluate, findImprovements, encode, decode, createDeck, hashSeed };
});
