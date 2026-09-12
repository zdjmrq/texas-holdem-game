/**
 * Shared poker AI core.
 *
 * This file deliberately has no DOM, Electron, WebSocket or game-controller
 * dependency. It is loaded directly by the browser and required by Node so
 * local and online tables execute the same strategy implementation.
 */
(function (root, factory) {
    const api = factory();
    if (typeof module !== 'undefined' && module.exports) module.exports = api;
    if (root) root.PokerAICore = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    'use strict';

    const SUITS = ['spades', 'hearts', 'diamonds', 'clubs'];
    const STANDARD_RANKS = ['2','3','4','5','6','7','8','9','10','J','Q','K','A'];
    const SHORT_RANKS = ['6','7','8','9','10','J','Q','K','A'];
    const VALUES = { '2':2,'3':3,'4':4,'5':5,'6':6,'7':7,'8':8,'9':9,'10':10,J:11,Q:12,K:13,A:14 };

    const DEFAULT_PROFILE = Object.freeze({
        vpip:0.25,
        aggression:0.55,
        threeBetFreq:0.08,
        bluffFreq:0.10,
        cbFreq:0.58,
        doubleBarrel:0.46,
        checkRaiseFreq:0.10,
        riskAversion:0.52,
        trapPreference:0.18,
        thinValuePreference:0.42,
        adaptationSpeed:0.55,
        uncertaintyTolerance:0.42,
        imageAwareness:0.58,
        patience:0.55
    });

    const PROFILE_OVERRIDES = Object.freeze({
        TAG: { riskAversion:0.67, aggression:0.50, bluffFreq:0.07, trapPreference:0.18, adaptationSpeed:0.48, patience:0.72 },
        LAG: { riskAversion:0.38, aggression:0.70, bluffFreq:0.15, thinValuePreference:0.55, uncertaintyTolerance:0.58, imageAwareness:0.68 },
        SOLID: { riskAversion:0.54, aggression:0.56, bluffFreq:0.10, adaptationSpeed:0.62, imageAwareness:0.66 },
        MANIAC: { riskAversion:0.28, aggression:0.78, bluffFreq:0.19, uncertaintyTolerance:0.68, imageAwareness:0.48, patience:0.28 },
        TAGFISH: { riskAversion:0.76, aggression:0.36, bluffFreq:0.045, trapPreference:0.12, adaptationSpeed:0.38, patience:0.78 },
        CALLING_STATION: { riskAversion:0.46, aggression:0.25, bluffFreq:0.035, thinValuePreference:0.28, uncertaintyTolerance:0.52, patience:0.66 }
    });

    function clamp(value, low, high) {
        return Math.max(low, Math.min(high, Number(value) || 0));
    }

    function finite(value, fallback = 0) {
        const number = Number(value);
        return Number.isFinite(number) ? number : fallback;
    }

    function hashSeed(value) {
        const text = String(value ?? 'poker-ai');
        let hash = 2166136261 >>> 0;
        for (let i = 0; i < text.length; i++) {
            hash ^= text.charCodeAt(i);
            hash = Math.imul(hash, 16777619);
        }
        return hash >>> 0 || 0x9e3779b9;
    }

    class SeededRng {
        constructor(seed) { this.state = hashSeed(seed); }
        next() {
            let t = this.state += 0x6D2B79F5;
            t = Math.imul(t ^ (t >>> 15), t | 1);
            t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
            return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
        }
        int(max) { return Math.floor(this.next() * Math.max(1, max)); }
        normal() {
            const u = Math.max(1e-9, this.next());
            const v = this.next();
            return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v);
        }
    }

    function cardKey(card) { return `${card?.rank}-${card?.suit}`; }
    function normalizeCard(card) {
        if (!card) return null;
        return { rank:String(card.rank), suit:String(card.suit), value:finite(card.value, VALUES[card.rank]) };
    }
    function buildDeck(variant) {
        const ranks = variant === 'shortdeck' ? SHORT_RANKS : STANDARD_RANKS;
        const cards = [];
        for (const suit of SUITS) for (const rank of ranks) cards.push({ rank, suit, value:VALUES[rank] });
        return cards;
    }

    function positionCategory(context) {
        const count = Math.max(2, context.playerCount || context.activeOpponentSeats.length + 1);
        const distance = Math.max(0, finite(context.positionFromButton, 0));
        // In heads-up play the dealer also posts the small blind, but remains
        // the button/late-position range rather than a generic blind range.
        if (count === 2 && distance === 0) return 'LP';
        if (context.isSmallBlind || context.isBigBlind) return 'BL';
        if (distance === 0 || distance >= count - 2) return 'LP';
        if (distance <= Math.max(1, Math.floor(count * 0.32))) return 'MP';
        return 'EP';
    }

    function startingHandStrength(c1, c2, variant = 'standard', position = 'MP') {
        c1 = normalizeCard(c1); c2 = normalizeCard(c2);
        if (!c1 || !c2) return 0;
        const high = Math.max(c1.value, c2.value);
        const low = Math.min(c1.value, c2.value);
        const pair = high === low;
        const suited = c1.suit === c2.suit;
        const gap = high - low;
        let strength;
        if (pair) {
            strength = 0.49 + ((high - (variant === 'shortdeck' ? 6 : 2)) /
                (14 - (variant === 'shortdeck' ? 6 : 2))) * 0.46;
            if (high >= 11) strength += 0.035;
        } else {
            strength = 0.08 + (high - 6) * 0.043 + Math.max(0, low - 7) * 0.025;
            if (high === 14) strength += 0.075;
            if (suited) strength += 0.075;
            if (gap === 1) strength += 0.065;
            else if (gap === 2) strength += 0.035;
            else if (gap >= 5) strength -= 0.055;
            if (high >= 11 && low >= 10) strength += 0.08;
        }
        if (variant === 'shortdeck') {
            strength += pair ? 0.025 : (suited ? 0.025 : 0);
            if (!pair && low < 9) strength -= 0.045;
        }
        if (position === 'LP') strength += 0.055;
        else if (position === 'EP') strength -= 0.045;
        return clamp(strength, 0.02, 0.98);
    }

    function comboFeatures(c1, c2, variant) {
        const high = Math.max(c1.value, c2.value);
        const low = Math.min(c1.value, c2.value);
        return {
            high,
            low,
            pair:high === low,
            suited:c1.suit === c2.suit,
            gap:high - low,
            aceBlocker:high === 14,
            kingBlocker:high === 13,
            strength:startingHandStrength(c1, c2, variant, 'MP')
        };
    }

    class OpponentBelief {
        constructor(seatId) {
            this.seatId = seatId;
            this.rangeWidth = 0.42;
            this.strengthShift = 0;
            this.polarization = 0;
            this.capped = false;
            this.folded = false;
            this.confidence = 0.12;
            this.lastStreet = 'preflop';
            this.history = [];
        }
        beginHand() {
            this.rangeWidth = 0.42;
            this.strengthShift = 0;
            this.polarization = 0;
            this.capped = false;
            this.folded = false;
            this.confidence = 0.12;
            this.lastStreet = 'preflop';
            this.history = [];
        }
        observe(event, model) {
            const action = String(event.action || '').toLowerCase();
            const street = event.street || 'preflop';
            const fraction = clamp(event.meta?.betFraction, 0, 3);
            this.lastStreet = street;
            if (action.includes('fold')) {
                this.folded = true;
                this.rangeWidth = 0;
            } else if (street === 'preflop') {
                if (action.includes('4bet')) { this.rangeWidth = Math.min(this.rangeWidth, 0.055); this.strengthShift += 0.18; this.polarization = 0.24; }
                else if (action.includes('3bet')) { this.rangeWidth = Math.min(this.rangeWidth, 0.115); this.strengthShift += 0.12; this.polarization = 0.28; }
                else if (action === 'raise' || action === 'bet' || action === 'allin') { this.rangeWidth = Math.min(this.rangeWidth, 0.24); this.strengthShift += 0.075; this.polarization = 0.12; }
                else if (action.includes('call')) { this.rangeWidth = Math.min(this.rangeWidth, 0.34); this.strengthShift += 0.025; this.capped = true; }
                else if (action === 'check') { this.rangeWidth = Math.max(this.rangeWidth, 0.68); this.capped = true; }
            } else if (action === 'check') {
                this.strengthShift -= 0.025;
                this.capped = this.capped || fraction === 0;
            } else if (action.includes('call')) {
                this.rangeWidth *= fraction > 0.72 ? 0.64 : 0.76;
                this.strengthShift += fraction > 0.72 ? 0.075 : 0.035;
                this.capped = true;
            } else if (action.includes('raise') || action === 'bet' || action === 'allin') {
                this.rangeWidth *= fraction > 0.9 ? 0.48 : 0.60;
                this.strengthShift += fraction > 0.9 ? 0.13 : 0.085;
                this.polarization = clamp(this.polarization + (fraction > 0.8 ? 0.22 : 0.10), 0, 0.8);
                this.capped = false;
            }
            if (model) {
                this.rangeWidth += (model.vpip - 0.25) * 0.16;
                this.polarization += Math.max(0, model.aggressionRate - 0.45) * 0.08;
            }
            this.rangeWidth = clamp(this.rangeWidth, this.folded ? 0 : 0.025, 0.92);
            this.strengthShift = clamp(this.strengthShift, -0.16, 0.30);
            this.polarization = clamp(this.polarization, 0, 0.85);
            this.confidence = clamp(this.confidence + 0.07, 0.12, 0.92);
            this.history.push({ action, street, fraction });
            if (this.history.length > 32) this.history.shift();
        }
        comboWeight(c1, c2, variant) {
            if (this.folded) return 0;
            const f = comboFeatures(c1, c2, variant);
            const cutoff = clamp(1 - this.rangeWidth + this.strengthShift * 0.25, 0.04, 0.95);
            let weight = 1 / (1 + Math.exp(-(f.strength + this.strengthShift - cutoff) * 15));
            const blockerBluff = f.suited && (f.aceBlocker || f.kingBlocker) && f.strength > 0.24;
            if (this.polarization > 0) {
                if (f.strength > 0.74) weight *= 1 + this.polarization * 0.9;
                else if (blockerBluff) weight += this.polarization * 0.22;
                else if (f.strength > 0.42 && f.strength < 0.64) weight *= 1 - this.polarization * 0.45;
            }
            if (this.capped && f.strength > 0.82) weight *= 0.42;
            return Math.max(0.0001, weight);
        }
        estimatedStrength() {
            return clamp(0.50 + this.strengthShift + (0.30 - this.rangeWidth) * 0.34, 0.18, 0.90);
        }
    }

    function sizeBucket(fraction) {
        fraction = finite(fraction, 0);
        if (fraction <= 0.38) return 'small';
        if (fraction <= 0.78) return 'medium';
        return 'large';
    }

    class BehavioralModel {
        constructor(seatId) {
            this.seatId = seatId;
            this.hands = new Set();
            this.vpipHands = new Set();
            this.pfrHands = new Set();
            this.preflopOpportunities = 0;
            this.postflopFaced = 0;
            this.postflopFolds = 0;
            this.postflopCalls = 0;
            this.postflopRaises = 0;
            this.showdowns = 0;
            this.shownBluffs = 0;
            this.sizing = {
                small:{ faced:0, folded:0, called:0, raised:0 },
                medium:{ faced:0, folded:0, called:0, raised:0 },
                large:{ faced:0, folded:0, called:0, raised:0 }
            };
        }
        observe(event) {
            const action = String(event.action || '').toLowerCase();
            if (action.includes('blind') || action.includes('ante')) return;
            const handId = event.handId ?? `sample-${this.hands.size + 1}`;
            this.hands.add(handId);
            const aggressive = event.meta?.aggressive === true || action.includes('raise') || action === 'bet' || action === 'allin';
            const called = action.includes('call');
            const folded = action.includes('fold');
            if (event.street === 'preflop') {
                this.preflopOpportunities = Math.max(this.preflopOpportunities, this.hands.size);
                if (called || aggressive) this.vpipHands.add(handId);
                if (aggressive) this.pfrHands.add(handId);
            } else if (finite(event.meta?.toCallBefore, 0) > 0) {
                this.postflopFaced++;
                if (folded) this.postflopFolds++;
                else if (aggressive) this.postflopRaises++;
                else if (called) this.postflopCalls++;
                const bucket = sizeBucket(event.meta?.betFraction);
                const stat = this.sizing[bucket];
                stat.faced++;
                if (folded) stat.folded++;
                else if (aggressive) stat.raised++;
                else stat.called++;
            } else if (aggressive) this.postflopRaises++;
        }
        observeShowdown(result = {}) {
            this.showdowns++;
            if (result.wasBluff) this.shownBluffs++;
        }
        posterior(successes, observations, priorMean, priorStrength) {
            return (successes + priorMean * priorStrength) / Math.max(1, observations + priorStrength);
        }
        get handCount() { return this.hands.size; }
        get confidence() { return clamp(this.handCount / (this.handCount + 10), 0, 0.9); }
        get vpip() { return this.posterior(this.vpipHands.size, this.handCount, 0.27, 10); }
        get pfr() { return this.posterior(this.pfrHands.size, this.handCount, 0.18, 10); }
        get aggressionRate() {
            const actions = this.postflopCalls + this.postflopRaises;
            return this.posterior(this.postflopRaises, actions, 0.42, 7);
        }
        get passivity() { return 1 - this.aggressionRate; }
        foldRateFor(fraction) {
            const stat = this.sizing[sizeBucket(fraction)];
            const all = this.posterior(this.postflopFolds, this.postflopFaced, 0.40, 6);
            const sized = this.posterior(stat.folded, stat.faced, all, 7);
            return clamp(sized, 0.08, 0.82);
        }
        get bluffShowRate() { return this.posterior(this.shownBluffs, this.showdowns, 0.24, 8); }
        get callsTooMuch() { return this.postflopFaced >= 4 && this.foldRateFor(0.6) < 0.27 && this.passivity > 0.52; }
        get foldsTooMuch() { return this.postflopFaced >= 4 && this.foldRateFor(0.6) > 0.56; }
    }

    class SelfImage {
        constructor() {
            this.hands = new Set();
            this.actions = [];
            this.showdowns = 0;
            this.shownBluffs = 0;
            this.shownTraps = 0;
        }
        record(decision, context, intent) {
            this.hands.add(context.handId);
            this.actions.push({
                handId:context.handId,
                street:context.street,
                action:decision.action,
                amount:decision.amount,
                aggressive:decision.action === 'raise' || decision.action === 'allin',
                intent
            });
            if (this.actions.length > 80) this.actions.shift();
        }
        observeShowdown(info = {}) {
            this.showdowns++;
            if (info.wasBluff) this.shownBluffs++;
            if (info.wasTrap) this.shownTraps++;
        }
        snapshot() {
            const recent = this.actions.slice(-28);
            const voluntary = recent.filter(a => !['fold','check'].includes(a.action)).length;
            const aggressive = recent.filter(a => a.aggressive).length;
            const riverActions = recent.filter(a => a.street === 'river');
            const riverAggression = riverActions.filter(a => a.aggressive).length;
            return {
                tightness:clamp(1 - (voluntary + 3) / (recent.length + 12), 0.05, 0.95),
                aggression:clamp((aggressive + 3.2) / (recent.length + 8), 0.05, 0.95),
                riverAggression:clamp((riverAggression + 0.5) / (riverActions.length + 4), 0, 1),
                shownBluffRate:(this.shownBluffs + 1.6) / (this.showdowns + 7),
                recentlyBluffed:this.actions.slice(-10).some(a => a.intent === 'bluff')
            };
        }
    }

    class HandPlan {
        constructor(handId) {
            this.id = `plan-${handId}`;
            this.handId = handId;
            this.intent = 'observe';
            this.targetSeats = [];
            this.maxRisk = 0;
            this.preferredSizes = [];
            this.favorableRunouts = [];
            this.unfavorableRunouts = [];
            this.continueConditions = [];
            this.abortConditions = [];
            this.responseToRaise = 'recalculate';
            this.confidence = 0.25;
            this.lastStreet = 'preflop';
            this.history = [];
        }
        continuityBonus(candidate, context, pot) {
            let bonus = 0;
            const aggressive = candidate.action === 'raise' || candidate.action === 'allin';
            if (this.intent === 'value' && aggressive) bonus += pot * 0.025;
            if (this.intent === 'semi_bluff' && aggressive && context.street !== 'river') bonus += pot * 0.018;
            if (this.intent === 'bluff' && aggressive && this.confidence > 0.55) bonus += pot * 0.012;
            if (this.intent === 'pot_control' && ['check','call'].includes(candidate.action)) bonus += pot * 0.020;
            if (this.intent === 'trap' && context.toCall > 0 && aggressive) bonus += pot * 0.040;
            if (this.abortConditions.includes('large_resistance') && context.toCall > pot * 0.8 && aggressive) bonus -= pot * 0.09;
            return bonus;
        }
        update(context, decision, equity, texture, response, profile) {
            const aggressive = decision.action === 'raise' || decision.action === 'allin';
            if (equity > 0.68) this.intent = aggressive ? 'value' : 'trap';
            else if (equity > 0.52) this.intent = aggressive ? 'thin_value' : 'pot_control';
            else if (texture.hasStrongDraw && aggressive && context.street !== 'river') this.intent = 'semi_bluff';
            else if (aggressive) this.intent = 'bluff';
            else if (context.toCall > 0 && decision.action === 'call') this.intent = 'bluff_catch';
            else this.intent = 'pot_control';
            this.targetSeats = context.activeOpponentSeats.slice();
            this.maxRisk = Math.round(context.heroStack * (this.intent === 'value' ? 0.78 : this.intent === 'semi_bluff' ? 0.46 : 0.32));
            this.preferredSizes = aggressive ? [decision.amount] : [];
            this.favorableRunouts = texture.hasFlushDraw ? ['flush_complete','high_pressure_card']
                : texture.hasStraightDraw ? ['straight_complete','high_pressure_card'] : ['range_advantage_card'];
            this.unfavorableRunouts = equity < 0.55 ? ['blank_after_call','opponent_range_strengthens'] : ['four_to_straight','four_to_flush'];
            this.continueConditions = this.intent === 'bluff'
                ? ['opponent_range_capped','credible_pressure_card','useful_blocker']
                : ['equity_holds','price_is_reasonable'];
            this.abortConditions = ['large_resistance'];
            this.responseToRaise = equity > 0.70 ? 'continue_or_reraise'
                : texture.hasStrongDraw ? 'compare_price_and_equity' : 'recalculate_or_fold';
            this.confidence = clamp(0.35 + Math.abs(equity - 0.5) * 0.8 + (response?.confidence || 0) * 0.2, 0.25, 0.92);
            this.lastStreet = context.street;
            this.history.push({ street:context.street, action:decision.action, amount:decision.amount, intent:this.intent });
        }
    }

    function analyzeTexture(board, holeCards, variant) {
        board = (board || []).map(normalizeCard).filter(Boolean);
        holeCards = (holeCards || []).map(normalizeCard).filter(Boolean);
        const all = [...holeCards, ...board];
        const boardRanks = {};
        const boardSuits = {};
        const allSuits = {};
        for (const card of board) {
            boardRanks[card.value] = (boardRanks[card.value] || 0) + 1;
            boardSuits[card.suit] = (boardSuits[card.suit] || 0) + 1;
        }
        for (const card of all) allSuits[card.suit] = (allSuits[card.suit] || 0) + 1;
        const paired = Object.values(boardRanks).some(count => count >= 2);
        const monotone = Object.values(boardSuits).some(count => count >= 3);
        const hasFlushDraw = Object.values(allSuits).some(count => count === 4);
        const values = [...new Set(all.map(card => card.value))];
        if (values.includes(14)) values.push(variant === 'shortdeck' ? 5 : 1);
        const valueSet = new Set(values);
        let openEnded = false, gutshot = false;
        for (let low = variant === 'shortdeck' ? 5 : 1; low <= 10; low++) {
            const count = [low,low+1,low+2,low+3,low+4].filter(value => valueSet.has(value)).length;
            if (count === 4) {
                if (valueSet.has(low+1) && valueSet.has(low+2) && valueSet.has(low+3)) openEnded = true;
                else gutshot = true;
            }
        }
        const sorted = Object.keys(boardRanks).map(Number).sort((a,b) => a-b);
        let connectedness = 0;
        for (let i = 1; i < sorted.length; i++) if (sorted[i] - sorted[i-1] <= 2) connectedness++;
        let wetness = (monotone ? 0.34 : Object.values(boardSuits).some(count => count === 2) ? 0.17 : 0)
            + connectedness * 0.11 + (paired ? 0.10 : 0);
        wetness = clamp(wetness, 0, 1);
        const last = board[board.length - 1];
        return {
            paired,
            monotone,
            hasFlushDraw,
            hasStraightDraw:openEnded || gutshot,
            openEnded,
            gutshot,
            hasStrongDraw:hasFlushDraw || openEnded,
            wetness,
            highCard:board.length ? Math.max(...board.map(card => card.value)) : 0,
            pressureCard:!!last && (last.value >= 11 || paired || monotone)
        };
    }

    function weightedPick(combos, rng, used) {
        if (!combos.length) return null;
        for (let attempt = 0; attempt < 22; attempt++) {
            const target = rng.next() * combos.totalWeight;
            let low = 0, high = combos.length - 1;
            while (low < high) {
                const mid = (low + high) >>> 1;
                if (combos[mid].cumulative < target) low = mid + 1;
                else high = mid;
            }
            const combo = combos[low];
            if (!used.has(combo.key1) && !used.has(combo.key2)) return combo;
        }
        for (const combo of combos) if (!used.has(combo.key1) && !used.has(combo.key2)) return combo;
        return null;
    }

    function buildWeightedCombos(deck, belief, variant) {
        const combos = [];
        let totalWeight = 0;
        for (let i = 0; i < deck.length; i++) {
            for (let j = i + 1; j < deck.length; j++) {
                const weight = belief ? belief.comboWeight(deck[i], deck[j], variant) : 1;
                if (weight <= 0) continue;
                totalWeight += weight;
                combos.push({ c1:deck[i], c2:deck[j], key1:cardKey(deck[i]), key2:cardKey(deck[j]), cumulative:totalWeight });
            }
        }
        combos.totalWeight = totalWeight || 1;
        return combos;
    }

    function handScore(result) {
        if (Number.isFinite(result)) return result;
        return finite(result?.score, finite(result?.rank, 0));
    }

    function estimateRangeEquity(context, beliefs, rng) {
        const evaluator = context.evaluateCards;
        if (typeof evaluator !== 'function') return { mean:0.5, stdDev:0.22, samples:0, confidence:0.05, timedOut:false };
        const start = typeof performance !== 'undefined' && performance.now ? performance.now() : Date.now();
        const known = new Set([...context.holeCards, ...context.communityCards].map(cardKey));
        const deck = buildDeck(context.variant).filter(card => !known.has(cardKey(card)));
        const seats = context.activeOpponentSeats.length
            ? context.activeOpponentSeats : Array.from({length:context.numOpponents}, (_,i) => `unknown-${i}`);
        // A seat whose belief is marked folded produces zero-weight combinations,
        // which used to spin forever (weightedPick returns null, samples never
        // advances, and the wall-clock brake required samples >= minimumSamples).
        // Skip those seats instead of sampling them.
        const samplers = seats
            .map(seat => buildWeightedCombos(deck, beliefs.get(String(seat)), context.variant))
            .filter(sampler => sampler.length > 0);
        if (!samplers.length) return { mean:0.5, stdDev:0.22, samples:0, confidence:0.05, timedOut:false };
        const budget = clamp(context.timeBudgetMs, 20, 180);
        // The wall-clock budget is an emergency brake, not the normal way a
        // decision picks its sample size: the intended sample count is only
        // interrupted when it is genuinely out of reach. Sampling a few extra
        // milliseconds is far cheaper than letting the sample count - and with
        // it the equity estimate and the bet size - wobble between a fast local
        // game and a loaded room server.
        const EMERGENCY_GRACE = 3;
        const baselineSamples = context.numOpponents >= 4 ? 180 : context.numOpponents >= 2 ? 240 : 320;
        const riverBonus = context.communityCards.length >= 5 ? 40 : 0;
        const maxSamples = Math.max(120, Math.floor(context.mcSamples || baselineSamples + riverBonus));
        const minimumSamples = context.numOpponents >= 4 ? 40 : 56;
        let samples = 0, sum = 0, sumSquares = 0, timedOut = false, stalled = false;
        for (let attempt = 0; samples < maxSamples; attempt++) {
            const now = typeof performance !== 'undefined' && performance.now ? performance.now() : Date.now();
            // Check the emergency brake every iteration: a stalled sampler never
            // advances samples, so a "samples % 8" gate would never fire.
            if (now - start >= budget * EMERGENCY_GRACE) {
                if (samples >= minimumSamples) timedOut = true;
                break;
            }
            const used = new Set(known);
            const opponentCards = [];
            let valid = true;
            for (const sampler of samplers) {
                const combo = weightedPick(sampler, rng, used);
                if (!combo) { valid = false; break; }
                used.add(combo.key1); used.add(combo.key2);
                opponentCards.push([combo.c1, combo.c2]);
            }
            if (!valid) {
                // Five consecutive dead attempts mean this seat set can never be
                // sampled (blockers, tiny deck, or a folded belief). Stop instead
                // of spinning the event loop forever.
                if (samples === 0 && attempt >= 5) { stalled = true; break; }
                continue;
            }
            const board = context.communityCards.slice();
            while (board.length < 5) {
                let card = null;
                for (let guard = 0; guard < 30 && !card; guard++) {
                    const candidate = deck[rng.int(deck.length)];
                    if (!used.has(cardKey(candidate))) card = candidate;
                }
                if (!card) { valid = false; break; }
                used.add(cardKey(card)); board.push(card);
            }
            if (!valid) continue;
            let hero;
            try { hero = handScore(evaluator([...context.holeCards, ...board], context.variant)); }
            catch (_) { return { mean:0.5, stdDev:0.25, samples, confidence:0.05, timedOut:false }; }
            let beaten = false, ties = 0;
            for (const cards of opponentCards) {
                const score = handScore(evaluator([...cards, ...board], context.variant));
                if (score > hero) { beaten = true; break; }
                if (score === hero) ties++;
            }
            const outcome = beaten ? 0 : 1 / (ties + 1);
            sum += outcome; sumSquares += outcome * outcome; samples++;
        }
        const mean = samples ? sum / samples : 0.5;
        const variance = samples ? Math.max(0, sumSquares / samples - mean * mean) : 0.0625;
        return {
            mean:clamp(mean, 0.005, 0.995),
            stdDev:Math.sqrt(variance),
            samples,
            confidence:clamp(samples / Math.max(160, maxSamples), 0.08, 0.96),
            timedOut,
            stalled
        };
    }

    function normalizedContext(raw = {}) {
        const legal = raw.legalActions || {};
        const actions = Array.isArray(legal) ? legal : (legal.actions || raw.actions || []);
        const street = raw.street || raw.phase || ((raw.communityCards || []).length === 0 ? 'preflop'
            : (raw.communityCards || []).length === 3 ? 'flop' : (raw.communityCards || []).length === 4 ? 'turn' : 'river');
        const opponents = (raw.activeOpponentSeats || raw.opponents || []).map(item =>
            typeof item === 'object' ? item.seatId ?? item.id ?? item.index : item);
        const context = {
            handId:raw.handId ?? 0,
            decisionId:raw.decisionId ?? raw.turnId ?? 0,
            variant:raw.variant === 'shortdeck' || raw.isShortDeck ? 'shortdeck' : 'standard',
            street,
            heroSeat:raw.heroSeat ?? raw.currentPlayerIndex ?? 0,
            holeCards:(raw.holeCards || []).map(normalizeCard).filter(Boolean),
            communityCards:(raw.communityCards || []).map(normalizeCard).filter(Boolean),
            pot:Math.max(0, finite(raw.pot)),
            currentBet:Math.max(0, finite(raw.currentBet)),
            heroRoundBet:Math.max(0, finite(raw.heroRoundBet, raw.yourBet)),
            toCall:Math.max(0, finite(raw.toCall, legal.toCall)),
            heroStack:Math.max(0, finite(raw.heroStack, raw.stack)),
            effectiveStack:Math.max(0, finite(raw.effectiveStack, raw.stack)),
            minRaiseTo:Math.max(0, finite(raw.minRaiseTo, legal.minRaiseTo)),
            maxRaiseTo:Math.max(0, finite(raw.maxRaiseTo, legal.maxRaiseTo)),
            canRaise:raw.canRaise !== false && legal.canRaise !== false,
            canCheck:raw.canCheck === true || actions.includes('check'),
            legalActions:actions,
            activeOpponentSeats:opponents,
            numOpponents:Math.max(1, finite(raw.numOpponents, finite(raw.numOpponentsActive, opponents.length || 1))),
            playerCount:Math.max(2, finite(raw.playerCount, (opponents.length || 1) + 1)),
            positionFromButton:finite(raw.positionFromButton),
            isSmallBlind:!!raw.isSmallBlind,
            isBigBlind:!!raw.isBigBlind,
            preflopRaiseCount:Math.max(0, finite(raw.preflopRaiseCount)),
            isPreviousStreetAggressor:!!raw.isPreviousStreetAggressor,
            isDelayedCBetCandidate:!!raw.isDelayedCBetCandidate,
            evaluateCards:raw.evaluateCards,
            timeBudgetMs:finite(raw.timeBudgetMs, opponents.length >= 4 || street === 'river' ? 150 : 76),
            mcSamples:finite(raw.mcSamples, 0),
            bigBlind:Math.max(1, finite(raw.bigBlind, 80)),
            publicActionHistory:raw.publicActionHistory || []
        };
        if (!context.legalActions.length) {
            if (context.toCall > 0) context.legalActions = ['fold','call'];
            else context.legalActions = ['check'];
            if (context.canRaise && context.maxRaiseTo > context.currentBet) context.legalActions.push('raise');
            if (context.heroStack > 0) context.legalActions.push('allin');
        }
        return context;
    }

    function betFractionForTarget(context, target) {
        const invest = Math.max(0, target - context.heroRoundBet);
        return invest / Math.max(context.bigBlind, context.pot + context.toCall);
    }

    class UnifiedPokerAI {
        constructor(options = {}) {
            this.name = options.name || 'AI';
            this.style = options.style || 'SOLID';
            this.seatId = options.seatId ?? 0;
            this.profile = Object.freeze({ ...DEFAULT_PROFILE, ...(options.profile || {}), ...(PROFILE_OVERRIDES[this.style] || {}) });
            this.beliefs = new Map();
            this.models = new Map();
            this.selfImage = new SelfImage();
            this.plan = null;
            this.currentHandId = null;
            this.lastTrace = null;
            this.spotFrequencies = new Map();
        }
        ensureOpponent(seatId) {
            const key = String(seatId);
            if (!this.beliefs.has(key)) this.beliefs.set(key, new OpponentBelief(seatId));
            if (!this.models.has(key)) this.models.set(key, new BehavioralModel(seatId));
            return { belief:this.beliefs.get(key), model:this.models.get(key) };
        }
        beginHand(handId) {
            this.currentHandId = handId;
            this.plan = new HandPlan(handId);
            for (const belief of this.beliefs.values()) belief.beginHand();
        }
        observeAction(event = {}) {
            if (String(event.seatId) === String(this.seatId)) return;
            if (event.handId !== undefined && this.currentHandId !== event.handId) this.beginHand(event.handId);
            const { belief, model } = this.ensureOpponent(event.seatId);
            model.observe(event);
            belief.observe(event, model);
        }
        observeShowdown(seatId, result) {
            if (String(seatId) === String(this.seatId)) this.selfImage.observeShowdown(result);
            else this.ensureOpponent(seatId).model.observeShowdown(result);
        }
        activeReads(context) {
            return context.activeOpponentSeats.map(seat => {
                const { belief, model } = this.ensureOpponent(seat);
                return { seat, belief, model };
            });
        }
        preflopDecision(context, rng, reads) {
            const position = positionCategory(context);
            const strength = startingHandStrength(context.holeCards[0], context.holeCards[1], context.variant, position);
            const stackBb = context.heroStack / context.bigBlind;
            const potOdds = context.toCall / Math.max(1, context.pot + context.toCall);
            const avgRangeStrength = reads.length
                ? reads.reduce((sum, read) => sum + read.belief.estimatedStrength(), 0) / reads.length : 0.5;
            const tightTarget = reads.some(read => read.model.foldsTooMuch || read.belief.rangeWidth < 0.22);
            const callsTooMuch = reads.some(read => read.model.callsTooMuch);
            const image = this.selfImage.snapshot();
            const candidates = [];
            if (context.toCall > 0 && context.legalActions.includes('fold')) candidates.push({ action:'fold', amount:0, utility:0, ev:0, reason:'price_vs_range' });
            if (context.canCheck && context.legalActions.includes('check')) candidates.push({ action:'check', amount:0, utility:context.pot * strength * 0.16, ev:0, reason:'free_equity' });

            const openThreshold = position === 'LP' ? 0.38 : position === 'BL' ? 0.44 : position === 'MP' ? 0.47 : 0.54;
            const styleShift = (0.25 - finite(this.profile.vpip, 0.25)) * 0.65;
            const pressure = context.preflopRaiseCount * 0.115 + Math.max(0, avgRangeStrength - 0.5) * 0.18;
            const callThreshold = openThreshold - 0.05 + pressure + potOdds * 0.18;
            if (context.toCall > 0 && context.legalActions.some(action => action === 'call' || action === 'allin')) {
                const callCost = Math.min(context.toCall, context.heroStack);
                const ev = (strength - callThreshold) * (context.pot + callCost) - callCost * Math.max(0, potOdds - strength) * 0.55;
                candidates.push({ action:callCost >= context.heroStack ? 'allin' : 'call', amount:callCost, ev, utility:ev, reason:'range_price_defence' });
            }

            const canRaise = context.canRaise && context.legalActions.some(action => action === 'raise' || action === 'allin');
            if (canRaise) {
                const premiumThreshold = context.preflopRaiseCount >= 2 ? 0.79 : context.preflopRaiseCount === 1 ? 0.68 : openThreshold + styleShift;
                const cards = comboFeatures(context.holeCards[0], context.holeCards[1], context.variant);
                const credibleBluff = context.numOpponents === 1 && (cards.aceBlocker || cards.kingBlocker) && cards.suited && tightTarget;
                const imageLeverage = image.tightness * this.profile.imageAwareness * 0.055;
                const qualifies = strength >= premiumThreshold ||
                    (credibleBluff && strength >= 0.28 && strength + imageLeverage >= premiumThreshold - 0.23);
                if (qualifies) {
                    const multiplier = context.preflopRaiseCount > 0 ? (position === 'LP' ? 3.0 : 3.7)
                        : (position === 'LP' ? 2.25 : 2.55) + Math.max(0, context.numOpponents - 2) * 0.12;
                    let target = Math.round(Math.max(context.bigBlind, context.currentBet || context.bigBlind) * multiplier);
                    target = Math.max(context.minRaiseTo, Math.min(context.maxRaiseTo, target));
                    const invest = Math.max(0, target - context.heroRoundBet);
                    const foldGain = (tightTarget ? 0.13 : 0.05) * context.pot;
                    const callPenalty = callsTooMuch && credibleBluff ? context.pot * 0.08 : 0;
                    const ev = (strength - premiumThreshold) * (context.pot + invest) + foldGain - callPenalty;
                    candidates.push({ action:target >= context.maxRaiseTo ? 'allin' : 'raise', amount:target >= context.maxRaiseTo ? context.heroStack : target, ev, utility:ev, reason:credibleBluff && strength < premiumThreshold ? 'blocker_image_pressure' : 'value_or_range_raise' });
                }
            }

            if (stackBb <= 12 && canRaise) {
                const pushThreshold = position === 'LP' ? 0.48 : position === 'BL' ? 0.53 : 0.59;
                if (strength >= pushThreshold + Math.max(0, context.numOpponents - 1) * 0.025) {
                    const ev = (strength - pushThreshold) * (context.pot + context.heroStack);
                    candidates.push({ action:'allin', amount:context.heroStack, ev, utility:ev, reason:'short_stack_push' });
                }
            }
            const decision = this.selectCandidate(candidates, context, rng, 0.22);
            return { decision, equity:{ mean:strength, stdDev:0.18, confidence:0.66, samples:0, preflopStrength:true }, texture:analyzeTexture([], context.holeCards, context.variant), reads };
        }
        realizationFactor(context, texture, reads) {
            let factor = 0.88;
            if (positionCategory(context) === 'LP') factor += 0.10;
            if (context.numOpponents > 1) factor -= Math.min(0.18, (context.numOpponents - 1) * 0.045);
            if (texture.hasStrongDraw) factor += 0.06;
            if (texture.wetness > 0.55 && context.effectiveStack > context.pot * 5) factor -= 0.06;
            if (reads.some(read => read.model.passivity > 0.62)) factor += 0.035;
            return clamp(factor, 0.66, 1.12);
        }
        storyCredibility(context, texture, action, equity, reads) {
            if (action !== 'raise' && action !== 'allin') return 0.5;
            const image = this.selfImage.snapshot();
            let score = 0.42;
            if (context.isPreviousStreetAggressor) score += 0.12;
            if (context.isDelayedCBetCandidate && texture.pressureCard) score += 0.10;
            if (texture.monotone || texture.paired) score += 0.04;
            if (equity > 0.62 || texture.hasStrongDraw) score += 0.10;
            if (image.tightness > 0.58) score += 0.10 * this.profile.imageAwareness;
            if (image.aggression > 0.68) score -= 0.07 * this.profile.imageAwareness;
            if (this.plan?.intent === 'bluff' && !texture.pressureCard && !texture.hasStrongDraw) score -= 0.09;
            if (reads.every(read => read.belief.capped)) score += 0.08;
            return clamp(score, 0.12, 0.90);
        }
        responseForRaise(context, target, equity, texture, reads) {
            const fraction = betFractionForTarget(context, target);
            const credibility = this.storyCredibility(context, texture, 'raise', equity, reads);
            let allFold = 1, noRaise = 1, confidence = 0;
            for (const read of reads) {
                let fold = read.model.foldRateFor(fraction);
                fold += (fraction - 0.55) * 0.13;
                fold += (credibility - 0.5) * 0.20;
                fold -= (read.belief.estimatedStrength() - 0.5) * 0.32;
                if (read.model.callsTooMuch) fold -= 0.13;
                if (read.model.foldsTooMuch) fold += 0.11;
                if (context.numOpponents > 1) fold -= 0.025;
                fold = clamp(fold, 0.04, 0.86);
                const raise = clamp((read.model.aggressionRate - 0.30) * 0.18 +
                    Math.max(0, read.belief.estimatedStrength() - 0.58) * 0.25 + fraction * 0.025, 0.015, 0.26);
                allFold *= fold;
                noRaise *= 1 - raise;
                confidence += read.model.confidence * 0.55 + read.belief.confidence * 0.45;
            }
            return {
                allFold:clamp(allFold, 0.001, 0.90),
                anyRaise:clamp(1 - noRaise, 0.01, 0.65),
                expectedCallers:reads.reduce((sum, read) => sum + (1 - read.model.foldRateFor(fraction)), 0),
                credibility,
                confidence:reads.length ? confidence / reads.length : 0.2,
                fraction
            };
        }
        riskPenalty(context, invest, equityInfo, texture, action) {
            if (invest <= 0) return 0;
            const stackRatio = invest / Math.max(1, context.heroStack);
            const deepRatio = context.effectiveStack / Math.max(context.bigBlind, context.pot);
            const uncertainty = clamp(equityInfo.stdDev * (1 - equityInfo.confidence * 0.45), 0.06, 0.50);
            let penalty = invest * stackRatio * this.profile.riskAversion * (0.18 + uncertainty * 0.42);
            if (context.numOpponents > 1) penalty *= 1 + (context.numOpponents - 1) * 0.10;
            if (deepRatio > 5 && !texture.hasStrongDraw && equityInfo.mean < 0.62) penalty += invest * 0.045;
            if (stackRatio > 0.65 && equityInfo.mean < 0.64) penalty += invest * (stackRatio - 0.65) * 0.35;
            if (action === 'call' && texture.wetness > 0.62 && equityInfo.mean < 0.52) penalty += invest * 0.035;
            return penalty;
        }
        raiseTargets(context, texture) {
            if (!context.canRaise || context.maxRaiseTo <= context.currentBet) return [];
            const potAfterCall = Math.max(context.bigBlind, context.pot + context.toCall);
            let fractions;
            if (context.street === 'river') fractions = [0.33,0.68,1.08];
            else if (texture.wetness > 0.52) fractions = [0.52,0.75,0.98];
            else fractions = [0.30,0.55,0.76];
            const targets = fractions.map(fraction => context.heroRoundBet + context.toCall + Math.round(potAfterCall * fraction));
            targets.push(context.maxRaiseTo);
            return [...new Set(targets.map(target => Math.min(context.maxRaiseTo, Math.max(context.minRaiseTo, target))))]
                .filter(target => target > context.currentBet && target > context.heroRoundBet)
                .sort((a,b) => a-b);
        }
        buildPostflopCandidates(context, equityInfo, texture, reads) {
            const equity = equityInfo.mean;
            const realization = this.realizationFactor(context, texture, reads);
            const candidates = [];
            if (context.toCall > 0 && context.legalActions.includes('fold')) candidates.push({ action:'fold', amount:0, invest:0, ev:0, utility:0, reason:'preserve_stack' });
            if (context.toCall === 0 && context.canCheck && context.legalActions.includes('check')) {
                const future = equity * context.pot * realization;
                candidates.push({ action:'check', amount:0, invest:0, ev:future, utility:future, reason:'realize_equity_or_control_pot' });
            }
            if (context.toCall > 0 && context.legalActions.some(action => action === 'call' || action === 'allin')) {
                const invest = Math.min(context.toCall, context.heroStack);
                const realizedEquity = clamp(equity * realization, 0.001, 0.995);
                const ev = realizedEquity * (context.pot + invest) - invest;
                const risk = this.riskPenalty(context, invest, equityInfo, texture, 'call');
                candidates.push({ action:invest >= context.heroStack ? 'allin' : 'call', amount:invest, invest, ev, risk, utility:ev-risk, reason:'price_and_range_equity' });
            }
            if (context.legalActions.some(action => action === 'raise' || action === 'allin')) {
                for (const target of this.raiseTargets(context, texture)) {
                    const invest = target - context.heroRoundBet;
                    const response = this.responseForRaise(context, target, equity, texture, reads);
                    const expectedCall = Math.max(0, target - context.currentBet) * Math.max(0.6, response.expectedCallers);
                    const calledEquity = clamp(equity * (response.expectedCallers > 1.15 ? 0.93 : 1.03), 0.001, 0.995);
                    const calledEv = calledEquity * (context.pot + invest + expectedCall) - invest;
                    const reraisedEv = equity > 0.68 ? calledEv * 0.72 : -invest * (0.44 + response.anyRaise * 0.24);
                    const ev = response.allFold * context.pot +
                        (1 - response.allFold) * ((1 - response.anyRaise) * calledEv + response.anyRaise * reraisedEv);
                    const risk = this.riskPenalty(context, invest, equityInfo, texture, target >= context.maxRaiseTo ? 'allin' : 'raise');
                    let deception = (response.credibility - 0.5) * context.pot * 0.035 * this.profile.imageAwareness;
                    if (equity < 0.32 && !texture.hasStrongDraw) deception -= context.pot * 0.025;
                    if (reads.some(read => read.model.callsTooMuch) && equity > 0.57) deception += context.pot * 0.022 * this.profile.thinValuePreference;
                    const action = target >= context.maxRaiseTo ? 'allin' : 'raise';
                    candidates.push({ action, amount:action === 'allin' ? context.heroStack : target, invest, ev, risk, response, utility:ev-risk+deception, reason:equity > 0.62 ? 'value_pressure' : texture.hasStrongDraw ? 'semi_bluff_pressure' : 'credible_range_pressure' });
                }
            }
            for (const candidate of candidates) {
                candidate.planBonus = this.plan ? this.plan.continuityBonus(candidate, context, context.pot) : 0;
                candidate.utility += candidate.planBonus;
            }
            return candidates;
        }
        spotKey(context, candidate) {
            const fraction = candidate.invest / Math.max(context.bigBlind, context.pot + context.toCall);
            return `${context.street}:${context.toCall > 0 ? 'facing' : 'open'}:${candidate.action}:${sizeBucket(fraction)}`;
        }
        selectCandidate(candidates, context, rng, uncertainty = 0.3) {
            if (!candidates.length) return { action:context.toCall > 0 ? 'fold' : 'check', amount:0, utility:0, ev:0, reason:'fallback' };
            candidates.sort((a,b) => b.utility - a.utility);
            let best = candidates[0];
            if (best.utility < 0) {
                const free = candidates.find(candidate => candidate.action === 'fold' || candidate.action === 'check');
                if (free) best = free;
            }
            const gapLimit = Math.max(context.bigBlind * 0.18, context.pot * (0.03 + this.profile.uncertaintyTolerance * 0.018));
            const eligible = candidates.filter(candidate => best.utility - candidate.utility <= gapLimit &&
                (candidate.utility >= -gapLimit * 0.35 || ['fold','check'].includes(candidate.action)));
            if (eligible.length <= 1) return best;
            const temperature = Math.max(context.bigBlind * 0.10, context.pot * 0.012) *
                (0.75 + uncertainty * this.profile.uncertaintyTolerance);
            const weights = [];
            let total = 0;
            for (const candidate of eligible) {
                const key = this.spotKey(context, candidate);
                const used = this.spotFrequencies.get(key) || 0;
                const balance = 1 / Math.sqrt(1 + used * 0.18);
                const weight = Math.exp((candidate.utility - best.utility) / Math.max(0.01, temperature)) * balance;
                weights.push(weight); total += weight;
            }
            let roll = rng.next() * total;
            let chosen = best;
            for (let i = 0; i < eligible.length; i++) {
                roll -= weights[i];
                if (roll <= 0) { chosen = eligible[i]; break; }
            }
            const key = this.spotKey(context, chosen);
            this.spotFrequencies.set(key, (this.spotFrequencies.get(key) || 0) + 1);
            return chosen;
        }
        legalize(decision, context) {
            let action = decision?.action || (context.toCall > 0 ? 'fold' : 'check');
            let amount = Math.max(0, Math.round(finite(decision?.amount)));
            if (!context.legalActions.includes(action)) {
                if (action === 'allin' && context.legalActions.includes('raise') && context.maxRaiseTo > context.currentBet) {
                    action = 'raise'; amount = context.maxRaiseTo;
                } else if (context.toCall > 0 && context.legalActions.includes('call')) {
                    action = 'call'; amount = Math.min(context.toCall, context.heroStack);
                } else if (context.legalActions.includes('check')) { action = 'check'; amount = 0; }
                else { action = 'fold'; amount = 0; }
            }
            if (action === 'call') amount = Math.min(context.toCall, context.heroStack);
            if (action === 'raise') {
                amount = Math.max(context.minRaiseTo, Math.min(context.maxRaiseTo, amount));
                if (amount >= context.maxRaiseTo && context.legalActions.includes('allin')) { action = 'allin'; amount = context.heroStack; }
            }
            if (action === 'allin') amount = context.heroStack;
            if (action === 'fold' || action === 'check') amount = 0;
            return { ...decision, action, amount };
        }
        decide(rawContext) {
            const context = normalizedContext(rawContext);
            if (this.currentHandId !== context.handId || !this.plan) this.beginHand(context.handId);
            const seed = `${rawContext.seed ?? 'table'}:${context.handId}:${this.seatId}:${context.decisionId}:${this.style}`;
            // One stream, exactly as the shipped decision distribution expects:
            // re-seeding the mixing roll changed which action gets picked often
            // enough to measure (~4% of decisions), and a paired benchmark against
            // a fixed opponent showed every alternative prefix doing worse. The
            // "sample count changes the choice" problem is solved at its source in
            // estimateRangeEquity instead, by making the intended sample count
            // finish rather than stopping it on the wall clock.
            const rng = new SeededRng(seed);
            const reads = this.activeReads(context);
            let result;
            if (context.street === 'preflop') result = this.preflopDecision(context, rng, reads);
            else {
                const equity = estimateRangeEquity(context, this.beliefs, rng);
                const texture = analyzeTexture(context.communityCards, context.holeCards, context.variant);
                const candidates = this.buildPostflopCandidates(context, equity, texture, reads);
                const decision = this.selectCandidate(candidates, context, rng, equity.stdDev * (1 - equity.confidence));
                result = { decision, equity, texture, reads, candidates };
            }
            const chosen = this.legalize(result.decision, context);
            const response = chosen.response || null;
            this.plan.update(context, chosen, result.equity.mean, result.texture, response, this.profile);
            this.selfImage.record(chosen, context, this.plan.intent);
            const opponentSummary = result.reads.map(read => ({
                seat:read.seat,
                rangeWidth:Number(read.belief.rangeWidth.toFixed(3)),
                estimatedStrength:Number(read.belief.estimatedStrength().toFixed(3)),
                confidence:Number(((read.belief.confidence + read.model.confidence) / 2).toFixed(3)),
                foldsTooMuch:read.model.foldsTooMuch,
                callsTooMuch:read.model.callsTooMuch
            }));
            const candidates = (result.candidates || [result.decision]).map(candidate => ({
                action:candidate.action,
                amount:Math.round(finite(candidate.amount)),
                ev:Number(finite(candidate.ev).toFixed(2)),
                risk:Number(finite(candidate.risk).toFixed(2)),
                utility:Number(finite(candidate.utility).toFixed(2)),
                reason:candidate.reason
            })).sort((a,b) => b.utility - a.utility);
            this.lastTrace = {
                strategyVersion:'unified-v1',
                handId:context.handId,
                decisionId:context.decisionId,
                intent:this.plan.intent,
                equity:{
                    mean:Number(result.equity.mean.toFixed(4)),
                    stdDev:Number(result.equity.stdDev.toFixed(4)),
                    confidence:Number(result.equity.confidence.toFixed(4)),
                    samples:result.equity.samples || 0,
                    timedOut:!!result.equity.timedOut
                },
                potOdds:Number((context.toCall / Math.max(1, context.pot + context.toCall)).toFixed(4)),
                selfImage:this.selfImage.snapshot(),
                opponents:opponentSummary,
                candidates,
                chosen:{ action:chosen.action, amount:chosen.amount, utility:Number(finite(chosen.utility).toFixed(2)) },
                nextPlan:{
                    id:this.plan.id,
                    intent:this.plan.intent,
                    maxRisk:this.plan.maxRisk,
                    continueConditions:this.plan.continueConditions,
                    abortConditions:this.plan.abortConditions,
                    responseToRaise:this.plan.responseToRaise
                }
            };
            return { action:chosen.action, amount:chosen.amount, trace:this.lastTrace };
        }
    }

    return {
        DEFAULT_PROFILE,
        PROFILE_OVERRIDES,
        SeededRng,
        OpponentBelief,
        BehavioralModel,
        SelfImage,
        HandPlan,
        UnifiedPokerAI,
        analyzeTexture,
        startingHandStrength,
        estimateRangeEquity,
        normalizedContext,
        buildDeck,
        hashSeed
    };
});
