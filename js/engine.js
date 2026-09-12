/**
 * Shared, variant-agnostic betting engine.
 * ========================================
 * PokerGame (js/game.js, 52-card blinds) and ShortDeckGame (js/shortdeck.js,
 * 36-card antes) used to carry two copy-pasted betting state machines: 29
 * same-named methods, 5 of them byte-for-byte identical. Everything below is
 * independent of the deck, the hand ranking and the forced-bet style, so it
 * now lives in exactly one place and a rule fix only has to be made once.
 *
 * The variants keep owning their data:
 *   PokerGame      : blinds, evaluateHand/compareHands, Deck
 *   ShortDeckGame  : antes,  evaluateSDHand/compareSDHands, ShortDeckDeck
 * and they expose the same instance shape (players, roundBets, currentBet,
 * lastRaise, playersActed, actedAtBet, raiseSizeAtAction, ...). Every helper
 * therefore takes the game instance as its first argument and mutates it in
 * place: this is a library of functions rather than a base class, so either
 * variant can adopt the kernel method by method without a shared hierarchy.
 *
 * The one nominal difference between the two state machines is the unit that
 * bounds a bet/raise:
 *   PokerGame.bigBlind   vs   ShortDeckGame.minBet
 * minBetUnit() resolves it, so no shared function has to know which variant is
 * calling. Any hook a variant wants to override is looked up on the instance
 * (deck, dealCommunityCards, showdown, calculateProbability, onUpdate, ...).
 *
 * Dual environment: index.html loads this file with
 *   <script src="js/engine.js"></script>
 * after js/game-rules-core.js and before js/game.js / js/shortdeck.js, while
 * Node reaches the same export through require('../js/engine').
 */
(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    if (root) root.PokerEngine = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    'use strict';

    // ── Environment glue ────────────────────────────────────────────────
    let cachedRules = null;
    let cachedEngine = null;

    /** js/game-rules-core.js, resolved lazily so script order stays forgiving. */
    function rules() {
        if (cachedRules) return cachedRules;
        if (typeof PokerGameRules !== 'undefined' && PokerGameRules) cachedRules = PokerGameRules;
        else if (typeof module !== 'undefined' && module.exports) cachedRules = require('./game-rules-core');
        if (!cachedRules) throw new Error('PokerEngine requires js/game-rules-core.js to be loaded first');
        return cachedRules;
    }

    /**
     * The bet/raise unit of the calling variant. Short deck stores it as
     * `minBet` (2× ante), the standard table as `bigBlind`.
     */
    function minBetUnit(game) {
        const minBet = Number(game.minBet);
        if (Number.isFinite(minBet) && minBet > 0) return minBet;
        return Number(game.bigBlind) || 0;
    }

    function roundBet(game, playerIndex) {
        return game.roundBets[playerIndex] || 0;
    }

    /**
     * Seat index of the human player. Variants expose a humanSeat getter; the
     * fallback keeps seats that only define humanPlayer working.
     */
    function humanSeatIndex(game) {
        if (typeof game?.humanSeat === 'number' && game.humanSeat >= 0) return game.humanSeat;
        const index = Array.isArray(game?.players) ? game.players.indexOf(game.humanPlayer) : -1;
        return index >= 0 ? index : 0;
    }

    // ── Seat lookups ────────────────────────────────────────────────────

    /** Active (not folded, not busted) players. */
    function getActivePlayers(game) {
        return game.players.filter(p => !p.folded && p.stack > 0);
    }

    /** Players still in the hand (not folded, may be all-in). */
    function getPlayersInHand(game) {
        return game.players.filter(p => !p.folded);
    }

    /** Whether it is the human player's turn. */
    function isPlayerTurn(game) {
        const seat = humanSeatIndex(game);
        const player = game.players[seat];
        return game.currentPlayerIndex === seat && !!player && !player.folded && !player.isAllIn;
    }

    /** Next seat that can still put chips in, starting after startPos. */
    function getNextActivePlayer(game, startPos) {
        const n = game.players.length;
        for (let i = 1; i < n; i++) {
            const idx = (startPos + i) % n;
            if (!game.players[idx].folded && game.players[idx].stack > 0 && !game.players[idx].isAllIn) {
                return idx;
            }
        }
        return -1;
    }

    /** Next seat still dealt into the hand — may be all-in. */
    function getNextPlayerInHand(game, startPos) {
        const n = game.players.length;
        for (let i = 1; i < n; i++) {
            const idx = (startPos + i) % n;
            if (!game.players[idx].folded) return idx;
        }
        return -1;
    }

    /** Seat distance from the button, ignoring seats that were not dealt in. */
    function getHandPositionInfo(game, playerIndex) {
        let seats = Array.isArray(game.playersDealtThisHand)
            ? game.playersDealtThisHand.map(player => game.players.indexOf(player)).filter(index => index >= 0)
            : [];
        if (!seats.length) {
            seats = game.players.map((player, index) => player.holeCards?.length === 2 ? index : -1)
                .filter(index => index >= 0);
        }
        seats.sort((a, b) => ((a - game.dealerPosition + game.players.length) % game.players.length) -
            ((b - game.dealerPosition + game.players.length) % game.players.length));
        return {
            positionFromButton: Math.max(0, seats.indexOf(playerIndex)),
            playerCount: Math.max(2, seats.length)
        };
    }

    // ── Betting-round rules ─────────────────────────────────────────────

    /** Find the next live player who has not acted or has chips left to call. */
    function getNextPlayerNeedingAction(game, startPos) {
        const n = game.players.length;
        for (let step = 1; step <= n; step++) {
            const idx = (startPos + step) % n;
            const player = game.players[idx];
            if (!player || player.folded || player.isAllIn || player.stack <= 0) continue;
            const matched = roundBet(game, idx) >= game.currentBet;
            if (!matched || !game.playersActed.has(idx)) return idx;
        }
        return -1;
    }

    /** Whether this player still has the right to make a raise this round. */
    function canPlayerRaise(game, playerIndex) {
        const player = game.players[playerIndex];
        if (!player || player.folded || player.isAllIn || player.stack <= 0) return false;
        if (!game.playersActed.has(playerIndex)) return true;
        // Multiple short all-ins can cumulatively reopen action. Compare the
        // total increase this player now faces with the full-raise size that
        // applied when they last acted (TDA Rule 47 semantics).
        const facedBefore = game.actedAtBet[playerIndex] || 0;
        const required = game.raiseSizeAtAction[playerIndex] || minBetUnit(game);
        return game.currentBet - facedBefore >= required;
    }

    function recordRoundAction(game, playerIndex) {
        game.playersActed.add(playerIndex);
        game.actedAtBet[playerIndex] = game.currentBet;
        game.raiseSizeAtAction[playerIndex] = Math.max(game.lastRaise, minBetUnit(game));
    }

    function getMinRaiseTo(game) {
        if (game.currentBet === 0) return minBetUnit(game);
        if (!game.hasFullBetThisRound && game.currentBet < minBetUnit(game)) return minBetUnit(game);
        return game.currentBet + Math.max(game.lastRaise, minBetUnit(game));
    }

    /** Auto-close a round when nobody has a call/fold decision left. */
    function markRoundCompleteIfNoDecision(game) {
        const live = getPlayersInHand(game).filter(p => !p.isAllIn && p.stack > 0);
        if (live.length === 0) return true;
        if (live.length !== 1) return false;
        const idx = game.players.indexOf(live[0]);
        const toCall = Math.max(0, game.currentBet - roundBet(game, idx));
        if (toCall > 0) return false;
        recordRoundAction(game, idx);
        game.bbNeedsOption = false;
        return true;
    }

    /** Return a unique unmatched top wager before advancing the street. */
    function refundUncalledBet(game) {
        const uncalled = rules().findUncalledRefund(game.roundBets);
        if (!uncalled) return 0;
        const player = game.players[uncalled.index];
        if (!player) return 0;
        const refund = uncalled.refund;
        player.stack += refund;
        player.chipsInPot -= refund;
        game.pot -= refund;
        game.roundBets[uncalled.index] -= refund;
        if (player.isAllIn && player.stack > 0) player.isAllIn = false;

        const liveBets = getPlayersInHand(game).map(p => roundBet(game, game.players.indexOf(p)));
        game.currentBet = liveBets.length ? Math.max(...liveBets) : 0;
        return refund;
    }

    /** Check if betting round is complete. */
    function isBettingRoundComplete(game) {
        const inHand = getPlayersInHand(game);
        if (inHand.length <= 1) return true;

        // Every active non-all-in player must have acted AND matched the bet
        for (const p of inHand) {
            if (p.isAllIn) continue;
            const idx = game.players.indexOf(p);
            if (roundBet(game, idx) < game.currentBet) return false;
            if (!game.playersActed.has(idx)) return false;
        }

        // Pre-flop BB option: if no raise, BB gets a chance to check/raise.
        // Short deck never sets bbNeedsOption, so this stays standard-only.
        if (game.phase === 'preflop' && game.bbNeedsOption) {
            const bb = game.players[game.bbIndex];
            if (bb && !bb.folded && !bb.isAllIn && game.currentBet === minBetUnit(game)) {
                return false;
            }
        }

        return true;
    }

    /** The BB exercised its option by checking or calling. */
    function closeBlindOption(game, playerIndex) {
        if (game.bbIndex >= 0 && playerIndex === game.bbIndex) game.bbNeedsOption = false;
    }

    /**
     * Any pre-flop raise removes the blind's option: the BB has already been
     * given the chance to act again by the raise itself. Mirrors the original
     * PokerGame bookkeeping exactly, so the flag keeps the same values it had
     * before the two state machines were merged.
     */
    function closeBlindOptionOnRaise(game) {
        if (game.phase === 'preflop') game.bbNeedsOption = false;
    }

    // ── Actions ─────────────────────────────────────────────────────────

    /** Execute an action for a player; keeps every piece of round bookkeeping. */
    function executeAction(game, playerIndex, action, amount) {
        const player = game.players[playerIndex];
        if (!player) {
            console.warn(`executeAction: player ${playerIndex} not found (action=${action})`);
            return;
        }
        const actionLower = action.toLowerCase();
        const contributed = roundBet(game, playerIndex);
        const toCall = Math.max(0, game.currentBet - contributed);
        const potBefore = game.pot;
        const currentBetBefore = game.currentBet;
        const lastAggressorBefore = game.lastAggressor;
        const lastAggressorStreetBefore = game.lastAggressorStreet;
        const preflopRaiseCountBefore = game.preflopRaiseCount;

        switch (actionLower) {
            case 'fold':
                player.folded = true;
                player.lastAction = { action: 'fold', amount: 0 };
                break;

            case 'check':
                if (toCall > 0) {
                    executeAction(game, playerIndex, 'call', 0);
                    return;
                }
                player.lastAction = { action: 'check', amount: 0 };
                recordRoundAction(game, playerIndex);
                closeBlindOption(game, playerIndex);
                break;

            case 'call': {
                if (toCall === 0) {
                    executeAction(game, playerIndex, 'check', 0);
                    return;
                }
                const callAmount = Math.min(toCall, player.stack);
                player.stack -= callAmount;
                player.chipsInPot += callAmount;
                game.pot += callAmount;
                if (player.stack === 0) player.isAllIn = true;
                player.lastAction = { action: 'call', amount: callAmount };
                game.roundBets[playerIndex] = roundBet(game, playerIndex) + callAmount;
                recordRoundAction(game, playerIndex);
                closeBlindOption(game, playerIndex);
                break;
            }

            case 'raise':
            case 'bet': {
                // amount is total bet (including call portion)
                const currentContrib = roundBet(game, playerIndex);
                const totalBet = Math.min(amount, player.stack + currentContrib);
                const additionalAmount = totalBet - currentContrib;

                if (!canPlayerRaise(game, playerIndex)) {
                    executeAction(game, playerIndex, toCall > 0 ? 'call' : 'check', 0);
                    return;
                }

                if (additionalAmount <= 0) {
                    // Just call instead
                    executeAction(game, playerIndex, 'call', 0);
                    return;
                }

                // Validate minimum raise size; a player may still make a
                // genuine sub-minimum all-in.
                const minTotalBet = getMinRaiseTo(game);
                if (totalBet < minTotalBet) {
                    if (totalBet === player.stack + currentContrib && totalBet > game.currentBet) {
                        executeAction(game, playerIndex, 'allin', 0);
                    } else executeAction(game, playerIndex, toCall > 0 ? 'call' : 'check', 0);
                    return;
                }

                const actualAdd = Math.min(additionalAmount, player.stack);
                player.stack -= actualAdd;
                player.chipsInPot += actualAdd;
                game.pot += actualAdd;

                // Raise SIZE = increase above the previous currentBet (NOT
                // totalBet - currentContrib, which includes the call portion)
                const prevCurrentBet = game.currentBet;
                game.currentBet = totalBet;
                game.lastRaise = game.currentBet - prevCurrentBet;
                game.hasFullBetThisRound = true;
                player.lastAction = { action: 'raise', amount: actualAdd };
                game.roundBets[playerIndex] = totalBet;
                game.playersActed.clear();
                recordRoundAction(game, playerIndex);
                game.lastRaiser = playerIndex;
                game.lastAggressor = playerIndex;
                game.lastAggressorStreet = game.phase;
                if (game.phase === 'preflop') game.preflopRaiseCount++;
                closeBlindOptionOnRaise(game);
                if (player.stack === 0) player.isAllIn = true;
                break;
            }

            case 'allin': {
                const allInAmount = player.stack;
                const beforeAllIn = roundBet(game, playerIndex);
                const totalAfterAllIn = beforeAllIn + allInAmount;

                // A partial all-in by somebody else does not reopen raising. If
                // this shove would be a raise while action is closed, make the
                // largest legal call and leave the remaining chips behind.
                if (totalAfterAllIn > game.currentBet && !canPlayerRaise(game, playerIndex)) {
                    executeAction(game, playerIndex, toCall > 0 ? 'call' : 'check', 0);
                    return;
                }
                player.chipsInPot += allInAmount;
                game.pot += allInAmount;

                if (totalAfterAllIn > game.currentBet) {
                    // Always update currentBet so others must at least call this amount
                    const minRaiseSize = Math.max(game.lastRaise, minBetUnit(game));
                    if (totalAfterAllIn >= game.currentBet + minRaiseSize) {
                        // Full raise — reopen betting for all active players
                        game.lastRaise = totalAfterAllIn - game.currentBet;
                        game.currentBet = totalAfterAllIn;
                        game.playersActed.clear();
                        game.lastRaiser = playerIndex;
                        game.lastAggressor = playerIndex;
                        game.lastAggressorStreet = game.phase;
                        if (game.phase === 'preflop') game.preflopRaiseCount++;
                        closeBlindOptionOnRaise(game);
                        game.hasFullBetThisRound = true;
                    } else {
                        // Partial all-in (< min raise) — update currentBet but do NOT
                        // reopen betting for players who already matched the prior bet
                        game.currentBet = totalAfterAllIn;
                        closeBlindOptionOnRaise(game);
                        if (!game.hasFullBetThisRound && game.currentBet >= minBetUnit(game)) {
                            game.hasFullBetThisRound = true;
                            game.lastRaise = minBetUnit(game);
                        }
                    }
                }

                player.stack = 0;
                player.isAllIn = true;
                game.roundBets[playerIndex] = totalAfterAllIn;
                recordRoundAction(game, playerIndex);
                player.lastAction = { action: 'allin', amount: allInAmount };
                break;
            }

            default:
                // Unknown action: keep the original no-op behaviour but still
                // run the action-notification epilogue below.
                break;
        }

        game.actionsThisRound++;

        const resolvedAction = player.lastAction?.action || actionLower;
        const raisedCurrentBet = game.currentBet > currentBetBefore;
        if (raisedCurrentBet) {
            // Track strategic aggression even for a legal partial all-in that
            // does not reopen the betting under the rules engine.
            game.lastAggressor = playerIndex;
            game.lastAggressorStreet = game.phase;
            if (game.phase === 'preflop') {
                game.preflopAggressor = playerIndex;
                if (game.preflopRaiseCount === preflopRaiseCountBefore) game.preflopRaiseCount++;
            }
            if (game.phase === 'flop') game.flopHadAggression = true;
        }
        const isCbetOpportunity = game.phase === 'flop' && playerIndex === game.preflopAggressor &&
            !game.flopCbetResolved && currentBetBefore === 0;
        if (game.phase === 'flop' && playerIndex === game.preflopAggressor && !game.flopCbetResolved) {
            game.flopCbetResolved = true;
        }
        const additionalPaid = Math.max(0, Number(player.lastAction?.amount) || 0);
        const raiseTo = Math.max(0, roundBet(game, playerIndex));
        const actionMeta = rules().buildActionMeta({
            callCost: toCall,
            toCallBefore: toCall,
            additionalPaid,
            amount: additionalPaid,
            raiseTo,
            currentBetBefore,
            potBefore,
            potAfter: game.pot,
            minimumBet: minBetUnit(game),
            aggressive: raisedCurrentBet,
            isCbetOpportunity,
            facedCbet: game.phase === 'flop' && toCall > 0 &&
                lastAggressorBefore === game.preflopAggressor && lastAggressorStreetBefore === 'flop',
            faced3bet: game.phase === 'preflop' && toCall > 0 && preflopRaiseCountBefore >= 2,
            preflopRaiseCountBefore
        });

        // Notify AI players about this action for opponent modeling
        notifyAIsOfAction(game, playerIndex, resolvedAction, actionMeta);
    }

    /** Notify all AI players about an action taken by any player. */
    function notifyAIsOfAction(game, playerIndex, action, actionMeta = {}) {
        const actionStreet = game.phase;
        const actorIsBlind = (playerIndex === game.sbIndex || playerIndex === game.bbIndex);
        const isPreFlop = game.phase === 'preflop';

        for (let i = 0; i < game.players.length; i++) {
            if (i === playerIndex) continue; // Don't self-report
            const p = game.players[i];
            if (!p || p.isHuman) continue; // Only notify AI players
            const ai = p.aiRef;
            if (!ai) continue;

            // Record action for exploitative player model
            if (typeof ai.recordOpponentAction === 'function') {
                ai.recordOpponentAction(playerIndex, action, actionStreet, game.handGeneration, actionMeta);
            }

            // Update opponent range estimation
            if (typeof ai.updateOpponentRange === 'function') {
                let rangeAction = action;
                if (action === 'small blind' || action === 'big blind') rangeAction = 'call';
                else if (isPreFlop && actionMeta.aggressive) {
                    if (actionMeta.preflopRaiseCountBefore >= 2) rangeAction = '4bet';
                    else if (actionMeta.preflopRaiseCountBefore >= 1) rangeAction = '3bet';
                    else rangeAction = 'raise';
                } else if (isPreFlop && action === 'call' && actionMeta.preflopRaiseCountBefore >= 2) {
                    rangeAction = 'call3bet';
                }
                ai.updateOpponentRange(playerIndex, rangeAction, actorIsBlind, null);
            }
        }
    }

    /** Legal action list for the human seat (index 0). */
    function getAvailableActions(game) {
        if (!isPlayerTurn(game)) return [];

        const seat = humanSeatIndex(game);
        const toCall = Math.max(0, game.currentBet - roundBet(game, seat));
        const player = game.humanPlayer;
        const actions = [];

        if (toCall === 0) {
            actions.push({ type: 'check', label: 'Check', amount: 0 });
        } else {
            actions.push({ type: 'call', label: `Call ${Math.min(toCall, player.stack)}`, amount: Math.min(toCall, player.stack) });
        }

        if (player.stack > 0) {
            if (toCall > 0) actions.push({ type: 'fold', label: 'Fold', amount: 0 });

            const canRaise = canPlayerRaise(game, seat);
            if (canRaise) {
                // Minimum raise
                const raiseTotal = getMinRaiseTo(game);
                const raiseAmount = raiseTotal - roundBet(game, seat);

                if (raiseAmount < player.stack) {
                    actions.push({ type: 'raise', label: `Raise to ${raiseTotal}`, amount: raiseTotal });
                }
            }

            const allInTotal = roundBet(game, seat) + player.stack;
            if (allInTotal <= game.currentBet || canRaise) {
                actions.push({ type: 'allin', label: 'All-in', amount: player.stack });
            }
        }

        return actions;
    }

    // ── Street / hand flow ──────────────────────────────────────────────

    /** Clear every per-round counter between streets. */
    function resetRoundState(game) {
        game.roundBets = {};
        game.playersActed = new Set();
        game.actedAtBet = {};
        game.raiseSizeAtAction = {};
        game.actionsThisRound = 0;
        game.currentBet = 0;
        game.lastRaise = minBetUnit(game);
        game.bbNeedsOption = false; // BB option only applies pre-flop
        game.hasFullBetThisRound = false;
    }

    /** Deal community cards */
    function dealCommunityCards(game) {
        if (game.phase !== 'preflop' && game.phase !== 'flop' && game.phase !== 'turn') return;
        game.deck.deal(); // Burn
        const count = game.phase === 'preflop' ? 3 : 1;
        for (let i = 0; i < count; i++) game.communityCards.push(game.deck.deal());
    }

    /** Advance to next phase */
    function advancePhase(game) {
        resetRoundState(game);

        switch (game.phase) {
            case 'preflop':
                game.dealCommunityCards(); // Deal flop (3 cards)
                game.phase = 'flop';
                game.bettingRound = 'flop';
                break;
            case 'flop':
                game.dealCommunityCards(); // Deal turn (1 card)
                game.phase = 'turn';
                game.bettingRound = 'turn';
                break;
            case 'turn':
                game.dealCommunityCards(); // Deal river (1 card)
                game.phase = 'river';
                game.bettingRound = 'river';
                break;
            case 'river':
                game.phase = 'showdown';
                game.bettingRound = 'showdown';
                return;
        }

        // Set starter for new round (first active player after dealer)
        game.currentPlayerIndex = game.getNextActivePlayer(game.dealerPosition);

        // Calculate new probability
        game.calculateProbability();

        if (game.onUpdate) game.onUpdate();
    }

    /** Advance to next player or next phase */
    function advanceGame(game) {
        if (game.phase === 'idle') return false; // Hand already ended — safety guard
        if (game.checkHandEnd()) return false;

        const inHand = getPlayersInHand(game);
        if (inHand.length <= 1) {
            game.endHand(inHand[0]);
            return false;
        }

        // Check if betting round is complete
        if (game.isBettingRoundComplete()) {
            game.refundUncalledBet();
            // Check if only one player not all-in (or everyone all-in)
            if (inHand.filter(p => !p.isAllIn).length <= 1) {
                // Everyone is all-in - deal remaining community cards
                game.advancePhase();
                while (game.phase !== 'showdown') {
                    game.advancePhase(); // advancePhase deals cards internally
                }
                game.showdown();
                if (game.onUpdate) game.onUpdate();
                return false;
            }

            game.advancePhase();
            if (game.phase === 'showdown') {
                game.showdown();
                if (game.onUpdate) game.onUpdate();
                return false;
            }

            return true;
        }

        // Move to next player
        const next = game.getNextPlayerNeedingAction(game.currentPlayerIndex);
        if (next === -1) {
            // All active players are all-in
            if (getPlayersInHand(game).filter(p => !p.isAllIn).length <= 1) {
                game.advancePhase();
                while (game.phase !== 'showdown') {
                    game.advancePhase(); // advancePhase deals cards internally
                }
                game.showdown();
                if (game.onUpdate) game.onUpdate();
                return false;
            }
            return false;
        }

        game.currentPlayerIndex = next;

        if (game.onUpdate) game.onUpdate();

        return true;
    }

    /** Check if hand should end */
    function checkHandEnd(game) {
        // An outer action loop can observe the hand once more after settlement.
        // Do not announce the already-cleared pot a second time.
        if (game.phase === 'idle') return true;
        const inHand = getPlayersInHand(game);
        if (inHand.length <= 1) {
            game.endHand(inHand.length === 1 ? inHand[0] : null);
            return true;
        }
        return false;
    }

    /** Outs of the current street, as produced by the probability service. */
    function getOuts(game) {
        if (!game.humanPlayer || game.humanPlayer.folded ||
            game.communityCards.length < 3 || game.communityCards.length >= 5) {
            return null;
        }
        return game.outsResult;
    }

    // ── Settlement ──────────────────────────────────────────────────────

    /** Calculate main/side pots from every contribution, including folded chips. */
    function calculateSidePots(game) {
        const inHand = getPlayersInHand(game);
        return rules().calculateSidePots(game.players, game.pot, inHand);
    }

    /** Odd chips go clockwise to the first winning seat left of the button. */
    function orderFromLeftOfDealer(game, players) {
        const n = game.players.length;
        return [...players].sort((a, b) => {
            const ai = game.players.indexOf(a);
            const bi = game.players.indexOf(b);
            const ad = ((ai - game.dealerPosition + n) % n) || n;
            const bd = ((bi - game.dealerPosition + n) % n) || n;
            return ad - bd;
        });
    }

    /**
     * Showdown: determine winner(s) with proper side pot distribution.
     * The variant supplies its hand ranking through `hooks`:
     *   hooks.evaluate(cards)   -> { rank, score, name, ... }
     *   hooks.compare(h1, h2)   -> -1 | 0 | 1
     */
    function showdown(game, hooks) {
        const inHand = getPlayersInHand(game);
        if (inHand.length <= 1) {
            game.endHand(inHand[0]);
            return;
        }

        // Evaluate all hands
        const results = [];
        for (const p of inHand) {
            const allCards = [...p.holeCards, ...game.communityCards];
            results.push({ player: p, hand: hooks.evaluate(allCards) });
        }

        // Build a lookup map for quick hand access
        const handMap = new Map();
        for (const r of results) handMap.set(r.player, r.hand);

        // Calculate side pots
        const sidePots = game.calculateSidePots();

        // Award each side pot (from highest to lowest)
        const allWinners = [];
        let totalAwarded = 0;
        const winnerAmounts = new Map(); // Player -> amount won

        for (const pot of sidePots) {
            if (pot.amount <= 0) continue;

            // Find best hand among eligible players
            let bestHand = null;
            let potWinners = [];

            for (const p of pot.eligible) {
                const hand = handMap.get(p);
                if (!hand) continue;
                if (!bestHand || hooks.compare(hand, bestHand) > 0) {
                    bestHand = hand;
                    potWinners = [p];
                } else if (hooks.compare(hand, bestHand) === 0) {
                    if (!potWinners.includes(p)) potWinners.push(p);
                }
            }

            if (potWinners.length === 0) {
                // Safety: no eligible winner, distribute to all eligible players
                const share = Math.floor(pot.amount / pot.eligible.length);
                for (const p of pot.eligible) {
                    p.stack += share;
                    winnerAmounts.set(p, (winnerAmounts.get(p) || 0) + share);
                    totalAwarded += share;
                }
                allWinners.push(...pot.eligible);
                continue;
            }

            // Split this pot among winners (distribute remainder chip by chip)
            potWinners = game.orderFromLeftOfDealer(potWinners);
            const share = Math.floor(pot.amount / potWinners.length);
            let remainder = pot.amount - share * potWinners.length;
            for (const w of potWinners) {
                const award = share + (remainder > 0 ? 1 : 0);
                if (remainder > 0) remainder--;
                w.stack += award;
                winnerAmounts.set(w, (winnerAmounts.get(w) || 0) + award);
                totalAwarded += award;
            }

            allWinners.push(...potWinners);
        }

        // Handle any micro-leftover due to integer math
        if (game.pot > totalAwarded && allWinners.length > 0) {
            const leftover = game.pot - totalAwarded;
            const firstLeft = game.orderFromLeftOfDealer([...new Set(allWinners)])[0];
            firstLeft.stack += leftover;
            winnerAmounts.set(firstLeft, (winnerAmounts.get(firstLeft) || 0) + leftover);
        }

        // Deduplicate winners for display
        const uniqueWinners = [...new Set(allWinners)];

        // Best hand for display (based on the highest side pot's winner)
        const bestResult = results.reduce((best, r) =>
            (!best || (r.hand && hooks.compare(r.hand, best.hand) > 0)) ? r : best, null);

        game.notifyAIsOfShowdown(results, uniqueWinners);

        const settledPot = game.pot;
        if (game.onHandEnd) {
            game.onHandEnd({
                winners: uniqueWinners,
                winnerAmounts,
                pot: settledPot,
                hand: bestResult?.hand,
                handName: bestResult?.hand?.name || 'Unknown',
                results,
                reason: 'showdown',
                share: uniqueWinners.length > 0 ? Math.floor(game.pot / uniqueWinners.length) : 0
            });
        }

        game.phase = 'idle';
        game.numHands++;
        game.pot = 0;
        if (game.onUpdate) game.onUpdate();
    }

    /** Feed the showdown result to every AI seat still at the table. */
    function notifyAIsOfShowdown(game, results, winners) {
        const winnerSet = new Set(winners);
        for (let observerIdx = 0; observerIdx < game.players.length; observerIdx++) {
            const ai = game.players[observerIdx]?.aiRef;
            if (!ai || typeof ai.recordShowdown !== 'function') continue;
            ai.position = observerIdx;
            for (const result of results) {
                const seatIdx = game.players.indexOf(result.player);
                const action = result.player.lastAction?.action;
                const aggressive = action === 'raise' || action === 'allin';
                const passive = action === 'check' || action === 'call';
                ai.recordShowdown(seatIdx, {
                    won:winnerSet.has(result.player),
                    handRank:result.hand?.rank ?? 0,
                    wasBluff:aggressive && !winnerSet.has(result.player) && (result.hand?.rank ?? 0) <= 2,
                    wasTrap:passive && winnerSet.has(result.player) && (result.hand?.rank ?? 0) >= 4
                });
            }
        }
    }

    // ── Lifecycle ───────────────────────────────────────────────────────

    /** Reset game */
    function resetGame(game) {
        game.players = [];
        game.humanPlayer = null;
        game.aiPlayers = [];
        game.communityCards = [];
        game.pot = 0;
        game.currentBet = 0;
        game.phase = 'idle';
        game.numHands = 0;
        game.dealerPosition = -1;
        game.sbIndex = -1;
        game.bbIndex = -1;
        if (game.onUpdate) game.onUpdate();
    }

    cachedEngine = {
        minBetUnit,
        getActivePlayers,
        getPlayersInHand,
        isPlayerTurn,
        getNextActivePlayer,
        getNextPlayerInHand,
        getHandPositionInfo,
        getNextPlayerNeedingAction,
        canPlayerRaise,
        recordRoundAction,
        getMinRaiseTo,
        markRoundCompleteIfNoDecision,
        refundUncalledBet,
        isBettingRoundComplete,
        executeAction,
        notifyAIsOfAction,
        getAvailableActions,
        resetRoundState,
        dealCommunityCards,
        advancePhase,
        advanceGame,
        checkHandEnd,
        getOuts,
        calculateSidePots,
        orderFromLeftOfDealer,
        showdown,
        notifyAIsOfShowdown,
        resetGame
    };
    return cachedEngine;
});
