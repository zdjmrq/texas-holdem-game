/**
 * Texas Hold'em Poker - Main Game Controller
 * Manages game state, betting, dealing, and turn flow
 */

class PokerGame {
    constructor(options = {}) {
        const configuredStack = Number(options.startingStack);
        const configuredSmallBlind = Number(options.smallBlind);
        const configuredBigBlind = Number(options.bigBlind);
        this.startingStack = Number.isFinite(configuredStack) && configuredStack > 0
            ? Math.floor(configuredStack) : 20000;
        this.players = [];          // All players (index 0 = human, rest = AI)
        this.humanPlayer = null;
        this.aiPlayers = [];
        this.deck = null;
        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.lastRaise = 0;
        this.dealerPosition = -1;    // First increment in startNewHand makes it 0 (human)
        this.currentPlayerIndex = 0;
        this.smallBlind = Number.isFinite(configuredSmallBlind) && configuredSmallBlind > 0
            ? Math.floor(configuredSmallBlind) : 40;
        this.bigBlind = Number.isFinite(configuredBigBlind) && configuredBigBlind >= this.smallBlind
            ? Math.floor(configuredBigBlind) : Math.max(80, this.smallBlind * 2);
        this.phase = 'idle';         // idle, preflop, flop, turn, river, showdown
        this.bettingRound = 'preflop';
        this.minRaise = this.bigBlind;
        this.handHistory = [];
        this.numHands = 0;
        this.handGeneration = 0;     // Incremented each new hand to kill stale AI loops
        this.lastAggressor = -1;
        this.lastAggressorStreet = null;
        this.preflopRaiseCount = 0;
        this.preflopAggressor = -1;
        this.flopHadAggression = false;
        this.flopCbetResolved = false;

        // Betting round state
        this.roundBets = {};         // Player index -> total bet this round
        this.playersActed = new Set();
        this.actedAtBet = {};
        this.raiseSizeAtAction = {};
        this.actionsThisRound = 0;
        this.lastRaiser = -1;
        this.bbNeedsOption = false;  // BB gets option pre-flop if no raise
        this.bbIndex = -1;           // Track which player is BB this hand
        this.sbIndex = -1;           // Track which player is SB this hand
        this.hasFullBetThisRound = false;

        // UI callbacks
        this.onUpdate = null;
        this.onPlayerAction = null;
        this.onHandEnd = null;
        this.onAIThinking = null;
        this.onAIAction = null;

        // Probability results
        this.probabilityResult = null;

        // Total pacing budget per AI action; configurable from the start screen.
        this.aiDelay = 1800;
        this.isProcessing = false;

        // Human player stack
        this.humanStack = this.startingStack;
    }

    /** Initialize the game with players */
    init(numAiPlayers = 5) {
        this.players = [];
        this.aiPlayers = [];

        // Create human player (index 0)
        this.humanPlayer = { name: 'You', stack: this.startingStack, holeCards: [], chipsInPot: 0, folded: false, isAllIn: false, hasActed: false, isHuman: true, lastAction: null, avatar: '😤' };
        this.players.push(this.humanPlayer);

        const ais = createAIPlayers(numAiPlayers, this.startingStack);
        for (let i = 0; i < ais.length; i++) {
            const ai = ais[i];
            const playerObj = {
                name: ai.name,
                stack: ai.stack,
                holeCards: [],
                chipsInPot: 0,
                folded: false,
                isAllIn: false,
                hasActed: false,
                isHuman: false,
                lastAction: null,
                avatar: ai.avatar,
                aiRef: ai
            };
            this.players.push(playerObj);
            this.aiPlayers.push(playerObj);
        }

        // Set positions for AI players
        for (let i = 0; i < this.aiPlayers.length; i++) {
            this.aiPlayers[i].aiRef.position = i + 1;
            this.aiPlayers[i].aiRef.numPlayers = this.players.length;
            this.aiPlayers[i].aiRef.bigBlind = this.bigBlind;
        }

        this.numHands = 0;

        // 随机选择首次庄家
        const activePlayers = this.getActivePlayers();
        if (activePlayers.length > 0) {
            const activeIndices = activePlayers.map(p => this.players.indexOf(p));
            this.dealerPosition = activeIndices[Math.floor(Math.random() * activeIndices.length)];
        } else {
            this.dealerPosition = 0;
        }
    }

    /** Start a new hand */
    startNewHand() {
        const previousBbIndex = this.bbIndex;
        const previousDealtCount = Array.isArray(this.playersDealtThisHand)
            ? this.playersDealtThisHand.length : 0;
        this.handGeneration++;           // Kill any stale processAITurns loop
        this.isProcessing = false;       // Release any stuck processing lock
        // Keep busted players (stack <= 0) folded — they are out of this hand
        for (const p of this.players) {
            p.holeCards = [];
            p.chipsInPot = 0;
            p.folded = p.stack <= 0;  // 0-stack players stay folded
            p.isAllIn = false;
            p.hasActed = false;
            p.lastAction = null;
        }

        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.roundBets = {};
        this.playersActed = new Set();
        this.actedAtBet = {};
        this.raiseSizeAtAction = {};

        // Check if enough players remain
        const activePlayers = this.getActivePlayers();
        if (activePlayers.length < 2) {
            this.phase = 'idle';
            if (this.onUpdate) this.onUpdate();
            if (this.onHandEnd) {
                if (activePlayers.length === 1) {
                    this.onHandEnd({ winner: activePlayers[0], pot: this.pot, reason: 'Not enough players' });
                } else {
                    // Everyone busted — game over
                    this.phase = 'idle';
                }
            }
            return;
        }

        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.lastRaise = 0;
        this.phase = 'preflop';
        this.bettingRound = 'preflop';
        this.roundBets = {};
        this.playersActed = new Set();
        this.actedAtBet = {};
        this.raiseSizeAtAction = {};
        this.actionsThisRound = 0;
        this.lastRaiser = -1;
        this.bbNeedsOption = false;
        this.bbIndex = -1;
        this.sbIndex = -1;
        this.lastAggressor = -1;
        this.lastAggressorStreet = null;
        this.preflopRaiseCount = 0;
        this.preflopAggressor = -1;
        this.flopHadAggression = false;
        this.flopCbetResolved = false;
        this.hasFullBetThisRound = false;
        this.probabilityResult = null;

        // Reset AI internal state at start of each hand
        for (const p of this.aiPlayers) {
            if (p.aiRef && typeof p.aiRef.reset === 'function') {
                p.aiRef.reset();
            }
        }

        // Rotate dealer to the next active player
        const enteringHeadsUp = activePlayers.length === 2 && previousDealtCount > 2 &&
            previousBbIndex >= 0 && activePlayers.includes(this.players[previousBbIndex]);
        if (enteringHeadsUp) {
            // On the first heads-up hand, the surviving previous big blind gets
            // the button/SB so nobody is charged the big blind twice in a row.
            this.dealerPosition = previousBbIndex;
        } else if (this.numHands > 0) {
            // Normal rotation: move button clockwise
            this.dealerPosition = this.getNextActivePlayer(this.dealerPosition);
            // If no next active player found (shouldn't happen with >=2 active), pick first active
            if (this.dealerPosition === -1) {
                this.dealerPosition = this.players.indexOf(activePlayers[0]);
            }
        }
        // On first hand, dealerPosition was already set randomly in init()
        this.playersDealtThisHand = [...activePlayers];

        // Create and shuffle deck
        this.deck = new Deck();
        this.deck.shuffle();

        // Deal hole cards — only to active (non-folded) players
        for (const p of activePlayers) {
            p.holeCards = [this.deck.deal(), this.deck.deal()];
        }

        // Post blinds using active players
        this.postBlinds();

        // Set current player to first to act pre-flop (first active after BB)
        this.currentPlayerIndex = this.getNextActivePlayer(this.bbIndex);

        // When forced blinds have already put every opponent all-in, the lone
        // player has no meaningful decision. Mark the round complete so excess
        // blind chips are returned and the board can run out automatically.
        const forcedRunout = this.markRoundCompleteIfNoDecision();

        this.isProcessing = false;

        // Calculate initial probability for human player
        this.calculateProbability();

        if (this.onUpdate) this.onUpdate();

        // If human player is first to act, wait for input
        // Otherwise start AI turns
        if (forcedRunout) {
            const generation = this.handGeneration;
            setTimeout(() => {
                if (generation === this.handGeneration && this.phase !== 'idle') this.advanceGame();
            }, 350);
        } else if (!this.isPlayerTurn()) {
            setTimeout(() => this.processAITurns(), 500); // 0.5s initial delay
        }
    }

    /** Post blinds */
    postBlinds() {
        const activePlayers = this.getActivePlayers();

        if (activePlayers.length === 2) {
            // Heads-up special rule: dealer = SB, non-dealer = BB
            this.sbIndex = this.dealerPosition;
            this.bbIndex = this.getNextActivePlayer(this.dealerPosition);
        } else {
            // 3+ players: SB = next active after dealer, BB = next active after SB
            this.sbIndex = this.getNextActivePlayer(this.dealerPosition);
            this.bbIndex = this.getNextActivePlayer(this.sbIndex);
        }

        if (this.sbIndex === -1 || this.bbIndex === -1) {
            // Not enough active players — can't post blinds
            return;
        }

        this.postBlind(this.sbIndex, this.smallBlind, 'small blind');
        this.postBlind(this.bbIndex, this.bigBlind, 'big blind');

        // A short-stacked blind may post less than the nominal blind.  The
        // amount other players actually face is the largest posted amount.
        const playersWhoCanStillBet = this.getPlayersInHand().filter(p => !p.isAllIn && p.stack > 0).length;
        this.currentBet = playersWhoCanStillBet >= 2
            ? this.bigBlind
            : Math.max(this.roundBets[this.sbIndex] || 0, this.roundBets[this.bbIndex] || 0);
        this.lastRaise = this.bigBlind;
        this.hasFullBetThisRound = playersWhoCanStillBet >= 2 ||
            (this.roundBets[this.bbIndex] || 0) >= this.bigBlind;
        this.bbNeedsOption = !this.players[this.bbIndex].isAllIn &&
            (this.roundBets[this.bbIndex] || 0) === this.currentBet;

        this.lastAggressor = this.bbIndex;
        this.lastAggressorStreet = 'forced';
    }

    postBlind(playerIndex, amount, blindName) {
        const player = this.players[playerIndex];
        const actualAmount = Math.min(amount, player.stack);
        player.stack -= actualAmount;
        player.chipsInPot += actualAmount;
        this.pot += actualAmount;
        if (player.stack === 0) player.isAllIn = true;
        player.lastAction = { action: blindName, amount: actualAmount };

        // Track round bet
        this.roundBets[playerIndex] = (this.roundBets[playerIndex] || 0) + actualAmount;

        // Notify AI players about blind posting
        this.notifyAIsOfAction(playerIndex, blindName);
    }

    /** Check if it's the human player's turn */
    isPlayerTurn() {
        return this.currentPlayerIndex === 0 && this.players[0] && !this.players[0].folded && !this.players[0].isAllIn;
    }

    /** Get the current player object */
    get currentPlayer() {
        return this.players[this.currentPlayerIndex];
    }

    /** Get active (not folded, not busted) players */
    getActivePlayers() {
        return this.players.filter(p => !p.folded && p.stack > 0);
    }

    /** Get players still in the hand (not folded, has chips or all-in) */
    getPlayersInHand() {
        return this.players.filter(p => !p.folded);
    }

    /** Get next active player starting from pos */
    getNextActivePlayer(startPos) {
        const n = this.players.length;
        for (let i = 1; i < n; i++) {
            const idx = (startPos + i) % n;
            if (!this.players[idx].folded && this.players[idx].stack > 0 && !this.players[idx].isAllIn) {
                return idx;
            }
        }
        return -1;
    }

    /** Get next player (for dealing, can be all-in) */
    getNextPlayerInHand(startPos) {
        const n = this.players.length;
        for (let i = 1; i < n; i++) {
            const idx = (startPos + i) % n;
            if (!this.players[idx].folded) {
                return idx;
            }
        }
        return -1;
    }

    /** Position among seats actually dealt into this hand, ignoring empty/busted seats. */
    getHandPositionInfo(playerIndex) {
        let seats = Array.isArray(this.playersDealtThisHand)
            ? this.playersDealtThisHand.map(player => this.players.indexOf(player)).filter(index => index >= 0)
            : [];
        if (!seats.length) {
            seats = this.players.map((player, index) => player.holeCards?.length === 2 ? index : -1)
                .filter(index => index >= 0);
        }
        seats.sort((a, b) => ((a - this.dealerPosition + this.players.length) % this.players.length) -
            ((b - this.dealerPosition + this.players.length) % this.players.length));
        return {
            positionFromButton: Math.max(0, seats.indexOf(playerIndex)),
            playerCount: Math.max(2, seats.length)
        };
    }

    /** Find the next live player who has not acted or has chips left to call. */
    getNextPlayerNeedingAction(startPos) {
        const n = this.players.length;
        for (let step = 1; step <= n; step++) {
            const idx = (startPos + step) % n;
            const player = this.players[idx];
            if (!player || player.folded || player.isAllIn || player.stack <= 0) continue;
            const matched = (this.roundBets[idx] || 0) >= this.currentBet;
            if (!matched || !this.playersActed.has(idx)) return idx;
        }
        return -1;
    }

    /** Whether this player still has the right to make a raise this round. */
    canPlayerRaise(playerIndex) {
        const player = this.players[playerIndex];
        if (!player || player.folded || player.isAllIn || player.stack <= 0) return false;
        if (!this.playersActed.has(playerIndex)) return true;
        // Multiple short all-ins can cumulatively reopen action. Compare the
        // total increase this player now faces with the full-raise size that
        // applied when they last acted (TDA Rule 47 semantics).
        const facedBefore = this.actedAtBet[playerIndex] || 0;
        const required = this.raiseSizeAtAction[playerIndex] || this.bigBlind;
        return this.currentBet - facedBefore >= required;
    }

    recordRoundAction(playerIndex) {
        this.playersActed.add(playerIndex);
        this.actedAtBet[playerIndex] = this.currentBet;
        this.raiseSizeAtAction[playerIndex] = Math.max(this.lastRaise, this.bigBlind);
    }

    getMinRaiseTo() {
        if (this.currentBet === 0) return this.bigBlind;
        if (!this.hasFullBetThisRound && this.currentBet < this.bigBlind) return this.bigBlind;
        return this.currentBet + Math.max(this.lastRaise, this.bigBlind);
    }

    /** Auto-close a round when nobody has a call/fold decision left. */
    markRoundCompleteIfNoDecision() {
        const live = this.getPlayersInHand().filter(p => !p.isAllIn && p.stack > 0);
        if (live.length === 0) return true;
        if (live.length !== 1) return false;
        const idx = this.players.indexOf(live[0]);
        const toCall = Math.max(0, this.currentBet - (this.roundBets[idx] || 0));
        if (toCall > 0) return false;
        this.recordRoundAction(idx);
        this.bbNeedsOption = false;
        return true;
    }

    /** Return a unique unmatched top wager before advancing the street. */
    refundUncalledBet() {
        const entries = this.players.map((player, index) => ({
            player,
            index,
            amount: Math.max(0, this.roundBets[index] || 0)
        })).sort((a, b) => b.amount - a.amount);
        if (entries.length < 2 || entries[0].amount === entries[1].amount) return 0;

        const top = entries[0];
        const refund = top.amount - entries[1].amount;
        top.player.stack += refund;
        top.player.chipsInPot -= refund;
        this.pot -= refund;
        this.roundBets[top.index] -= refund;
        if (top.player.isAllIn && top.player.stack > 0) top.player.isAllIn = false;

        const liveBets = this.getPlayersInHand().map(p => this.roundBets[this.players.indexOf(p)] || 0);
        this.currentBet = liveBets.length ? Math.max(...liveBets) : 0;
        return refund;
    }

    /** Check if betting round is complete */
    isBettingRoundComplete() {
        const inHand = this.getPlayersInHand();
        if (inHand.length <= 1) return true;

        // Every active non-all-in player must have acted AND matched the current bet
        for (const p of inHand) {
            if (p.isAllIn) continue;
            const idx = this.players.indexOf(p);
            const myBet = this.roundBets[idx] || 0;
            // Must have matched the current bet
            if (myBet < this.currentBet) return false;
            // Must have acted this round
            if (!this.playersActed.has(idx)) return false;
        }

        // Pre-flop BB option: if no raise, BB gets a chance to check/raise
        if (this.phase === 'preflop' && this.bbNeedsOption) {
            const bb = this.players[this.bbIndex];
            if (!bb.folded && !bb.isAllIn && this.currentBet === this.bigBlind) {
                return false; // BB still has the option
            }
        }

        return true;
    }

    /** Handle player action */
    playerAction(action, amount) {
        if (this.isProcessing || !this.isPlayerTurn()) return;

        this.isProcessing = true;
        this.executeAction(0, action, amount);
        this.isProcessing = false;

        if (this.onUpdate) this.onUpdate();

        // Check hand end or advance
        if (this.checkHandEnd()) return;

        // Advance game with guard loop (same as processAITurns)
        for (let safety = 0; safety < 5; safety++) {
            const prevPhase = this.phase;
            this.advanceGame();
            if (this.phase === 'showdown' || this.phase === 'idle') break;
            if (this.phase !== prevPhase) break;
            const gi = this.currentPlayerIndex;
            const gp = this.players[gi];
            if (gp.folded || gp.isAllIn) continue;
            const gb = this.roundBets[gi] || 0;
            if (gb >= this.currentBet && this.playersActed.has(gi)) continue;
            break;
        }

        // If it's now an AI's turn, start processing with a short delay
        if (this.currentPlayerIndex !== 0 && this.phase !== 'showdown' && this.phase !== 'idle') {
            setTimeout(() => this.processAITurns(), 500);
        }
    }

    /** Execute an action for a player */
    executeAction(playerIndex, action, amount) {
        const player = this.players[playerIndex];
        if (!player) {
            console.warn(`executeAction: player ${playerIndex} not found (action=${action})`);
            return;
        }
        const actionLower = action.toLowerCase();
        const contributed = this.roundBets[playerIndex] || 0;
        const toCall = Math.max(0, this.currentBet - contributed);
        const currentBetBefore = this.currentBet;
        const lastAggressorBefore = this.lastAggressor;
        const lastAggressorStreetBefore = this.lastAggressorStreet;
        const preflopRaiseCountBefore = this.preflopRaiseCount;

        switch (actionLower) {
            case 'fold':
                player.folded = true;
                player.lastAction = { action: 'fold', amount: 0 };
                break;

            case 'check':
                if (toCall > 0) {
                    this.executeAction(playerIndex, 'call', 0);
                    return;
                }
                player.lastAction = { action: 'check', amount: 0 };
                this.recordRoundAction(playerIndex);
                if (playerIndex === this.bbIndex) {
                    this.bbNeedsOption = false; // BB exercised option
                }
                break;

            case 'call':
                if (toCall === 0) {
                    this.executeAction(playerIndex, 'check', 0);
                    return;
                }
                const callAmount = Math.min(toCall, player.stack);
                player.stack -= callAmount;
                player.chipsInPot += callAmount;
                this.pot += callAmount;
                if (player.stack === 0) player.isAllIn = true;
                player.lastAction = { action: 'call', amount: callAmount };
                this.roundBets[playerIndex] = (this.roundBets[playerIndex] || 0) + callAmount;
                this.recordRoundAction(playerIndex);
                if (playerIndex === this.bbIndex) {
                    this.bbNeedsOption = false; // BB exercised option by calling
                }
                break;

            case 'raise':
            case 'bet':
                // amount is total bet (including call portion)
                const currentContrib = this.roundBets[playerIndex] || 0;
                const totalBet = Math.min(amount, player.stack + currentContrib);
                const additionalAmount = totalBet - currentContrib;

                if (!this.canPlayerRaise(playerIndex)) {
                    this.executeAction(playerIndex, toCall > 0 ? 'call' : 'check', 0);
                    return;
                }

                if (additionalAmount <= 0) {
                    // Just call instead
                    this.executeAction(playerIndex, 'call', 0);
                    return;
                }

                // Validate minimum raise size
                const minRaiseSize = Math.max(this.lastRaise, this.bigBlind);
                const minTotalBet = this.getMinRaiseTo();

                if (totalBet < minTotalBet) {
                    // A player may still make a genuine sub-minimum all-in.
                    if (totalBet === player.stack + currentContrib && totalBet > this.currentBet) {
                        this.executeAction(playerIndex, 'allin', 0);
                    } else this.executeAction(playerIndex, toCall > 0 ? 'call' : 'check', 0);
                    return;
                }

                const actualAdd = Math.min(additionalAmount, player.stack);
                player.stack -= actualAdd;
                player.chipsInPot += actualAdd;
                this.pot += actualAdd;

                // Calculate raise SIZE = increase above the previous currentBet
                // (NOT totalBet - currentContrib, which includes the call portion)
                const prevCurrentBet = this.currentBet;
                this.currentBet = totalBet;
                this.lastRaise = this.currentBet - prevCurrentBet;
                this.hasFullBetThisRound = true;
                player.lastAction = { action: 'raise', amount: actualAdd };
                this.roundBets[playerIndex] = totalBet;
                this.playersActed.clear();
                this.recordRoundAction(playerIndex);
                this.lastRaiser = playerIndex;
                this.lastAggressor = playerIndex;
                this.lastAggressorStreet = this.phase;
                if (this.phase === 'preflop') this.preflopRaiseCount++;
                this.bbNeedsOption = false; // Any raise pre-flop ends BB option
                if (player.stack === 0) player.isAllIn = true;

                // Reset hasActed for all other non-all-in players
                for (let i = 0; i < this.players.length; i++) {
                    if (i !== playerIndex && !this.players[i].folded && !this.players[i].isAllIn) {
                        if (this.roundBets[i] !== undefined && this.roundBets[i] < this.currentBet) {
                            // This player hasn't matched the raise yet
                        }
                    }
                }
                break;

            case 'allin':
                const allInAmount = player.stack;
                const beforeAllIn = this.roundBets[playerIndex] || 0;
                const totalAfterAllIn = beforeAllIn + allInAmount;

                // A partial all-in by somebody else does not reopen raising. If
                // this shove would be a raise while action is closed, make the
                // largest legal call and leave the remaining chips behind.
                if (totalAfterAllIn > this.currentBet && !this.canPlayerRaise(playerIndex)) {
                    this.executeAction(playerIndex, toCall > 0 ? 'call' : 'check', 0);
                    return;
                }
                player.chipsInPot += allInAmount;
                this.pot += allInAmount;

                if (totalAfterAllIn > this.currentBet) {
                    // Always update currentBet so others must at least call this amount
                    const minRaiseSize = Math.max(this.lastRaise, this.bigBlind);
                    if (totalAfterAllIn >= this.currentBet + minRaiseSize) {
                        // Full raise — reopen betting for all active players
                        this.lastRaise = totalAfterAllIn - this.currentBet;
                        this.currentBet = totalAfterAllIn;
                        this.playersActed.clear();
                        this.lastRaiser = playerIndex;
                        this.lastAggressor = playerIndex;
                        this.lastAggressorStreet = this.phase;
                        if (this.phase === 'preflop') this.preflopRaiseCount++;
                        this.hasFullBetThisRound = true;
                    } else {
                        // Partial all-in (< min raise) — update currentBet but do NOT
                        // reopen betting for players who already matched the prior bet
                        this.currentBet = totalAfterAllIn;
                        if (!this.hasFullBetThisRound && this.currentBet >= this.bigBlind) {
                            this.hasFullBetThisRound = true;
                            this.lastRaise = this.bigBlind;
                        }
                    }
                }

                player.stack = 0;
                player.isAllIn = true;
                this.roundBets[playerIndex] = totalAfterAllIn;
                this.recordRoundAction(playerIndex);
                player.lastAction = { action: 'allin', amount: allInAmount };
                break;
        }

        this.actionsThisRound++;

        const resolvedAction = player.lastAction?.action || actionLower;
        const raisedCurrentBet = this.currentBet > currentBetBefore;
        if (raisedCurrentBet) {
            // Track strategic aggression even for a legal partial all-in that
            // does not reopen the betting under the rules engine.
            this.lastAggressor = playerIndex;
            this.lastAggressorStreet = this.phase;
            if (this.phase === 'preflop') {
                this.preflopAggressor = playerIndex;
                if (this.preflopRaiseCount === preflopRaiseCountBefore) this.preflopRaiseCount++;
            }
            if (this.phase === 'flop') this.flopHadAggression = true;
        }
        const isCbetOpportunity = this.phase === 'flop' && playerIndex === this.preflopAggressor &&
            !this.flopCbetResolved && currentBetBefore === 0;
        if (this.phase === 'flop' && playerIndex === this.preflopAggressor && !this.flopCbetResolved) {
            this.flopCbetResolved = true;
        }
        const actionMeta = {
            toCallBefore: toCall,
            aggressive: raisedCurrentBet,
            isCbetOpportunity,
            facedCbet: this.phase === 'flop' && toCall > 0 &&
                lastAggressorBefore === this.preflopAggressor && lastAggressorStreetBefore === 'flop',
            faced3bet: this.phase === 'preflop' && toCall > 0 && preflopRaiseCountBefore >= 2,
            preflopRaiseCountBefore
        };

        // Notify AI players about this action for opponent modeling
        this.notifyAIsOfAction(playerIndex, resolvedAction, actionMeta);
    }

    /** Notify all AI players about an action taken by any player */
    notifyAIsOfAction(playerIndex, action, actionMeta = {}) {
        const actionStreet = this.phase;
        const actorIsBlind = (playerIndex === this.sbIndex || playerIndex === this.bbIndex);
        const isPreFlop = this.phase === 'preflop';

        for (let i = 0; i < this.players.length; i++) {
            if (i === playerIndex) continue; // Don't self-report
            const p = this.players[i];
            if (!p || p.isHuman) continue; // Only notify AI players
            const ai = p.aiRef;
            if (!ai) continue;

            // Record action for exploitative player model
            if (typeof ai.recordOpponentAction === 'function') {
                ai.recordOpponentAction(playerIndex, action, actionStreet, this.handGeneration, actionMeta);
            }

            // Update opponent range estimation
            if (typeof ai.updateOpponentRange === 'function') {
                // Map action type for range narrowing
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

    /** Process AI turns */
    async processAITurns() {
        const currentGen = this.handGeneration;

        while (!this.isPlayerTurn() && !this.isProcessing && this.handGeneration === currentGen && this.phase !== 'idle') {
            if (this.checkHandEnd()) return;

            const idx = this.currentPlayerIndex;
            const player = this.players[idx];

            // Safety: player not found — skip
            if (!player) { this.advanceGame(); continue; }

            if (player.folded || player.isAllIn) {
                this.advanceGame();
                continue;
            }

            this.isProcessing = true;

            // Build game state for AI
            const activePlayers = this.getPlayersInHand();
            const numActiveOpponents = activePlayers.filter(p => !p.isHuman).length - 1 +
                (activePlayers.filter(p => p.isHuman).length > 0 ? 1 : 0);
            const tablePosition = this.getHandPositionInfo(idx);
            const opponentStacks = activePlayers.filter(p => p !== player)
                .map(p => p.stack + (this.roundBets[this.players.indexOf(p)] || 0));

            const gameState = {
                communityCards: this.communityCards,
                pot: this.pot,
                currentBet: this.currentBet,
                toCall: Math.max(0, this.currentBet - (this.roundBets[idx] || 0)),
                yourBet: this.roundBets[idx] || 0,
                stack: player.stack,
                effectiveStack: Math.min(player.stack, Math.max(0, ...opponentStacks)),
                numOpponentsActive: activePlayers.length - 1,
                dealerPosition: this.dealerPosition,
                currentPlayerIndex: idx,
                positionFromButton: tablePosition.positionFromButton,
                canCheck: (this.roundBets[idx] || 0) >= this.currentBet,
                canRaise: this.canPlayerRaise(idx),
                minRaiseTo: this.getMinRaiseTo(),
                maxRaiseTo: (this.roundBets[idx] || 0) + player.stack,
                preflopRaiseCount: this.preflopRaiseCount,
                isPreviousStreetAggressor: this.lastAggressor === idx &&
                    this.lastAggressorStreet === ({ flop:'preflop', turn:'flop', river:'turn' }[this.phase] || null),
                isDelayedCBetCandidate: this.phase === 'turn' && this.preflopAggressor === idx &&
                    !this.flopHadAggression,
                isSmallBlind: idx === this.sbIndex,
                isBigBlind: idx === this.bbIndex,
                bigBlind: this.bigBlind,
                phase: this.phase
            };

            // Phase 1: "Thinking" show thinking dots
            if (this.onAIThinking) this.onAIThinking(idx);
            const thinkDelay = Math.max(120, this.aiDelay * (0.34 + Math.random() * 0.08));
            await new Promise(resolve => setTimeout(resolve, thinkDelay));
            if (this.handGeneration !== currentGen) return;

            // AI makes decision based on its own cards only
            const ai = player.aiRef;
            ai.holeCards = player.holeCards;
            ai.stack = player.stack;
            ai.chipsInPot = player.chipsInPot;
            ai.position = idx;
            ai.numPlayers = tablePosition.playerCount;

            // Phase 2: "Decision made" - short pause before reveal
            const revealDelay = Math.max(100, this.aiDelay * (0.14 + Math.random() * 0.05));
            await new Promise(resolve => setTimeout(resolve, revealDelay));
            if (this.handGeneration !== currentGen) return;

            const decision = ai.decide(gameState);
            this.executeAction(idx, decision.action, decision.amount);

            this.isProcessing = false;

            // Show action label
            if (this.onAIAction) this.onAIAction(idx, decision);

            // ADVANCE GAME with guard loop: keep advancing until we land on a
            // player who genuinely needs to act, or the phase changes (round ends),
            // or the hand ends. This catches any edge case where the betting round
            // should have completed but isBettingRoundComplete() didn't catch it.
            let guardPhase, guardChanged;
            for (let safety = 0; safety < 5; safety++) {
                guardPhase = this.phase;
                this.advanceGame(); // onUpdate() called internally
                if (this.phase === 'showdown' || this.phase === 'idle') break;

                // If the betting round changed (new community cards dealt), we're done
                if (this.phase !== guardPhase) { guardChanged = true; break; }

                // Check if the player we landed on should NOT need to act
                const gi = this.currentPlayerIndex;
                const gp = this.players[gi];
                if (gp.folded || gp.isAllIn) continue; // skip dead players
                const gb = this.roundBets[gi] || 0;
                if (gb >= this.currentBet && this.playersActed.has(gi)) {
                    // This player already matched the bet and acted — round is actually
                    // complete. Loop back to force advanceGame to advance the phase.
                    continue;
                }
                // Legitimate player who needs to act — stop guarding
                break;
            }
            const guardHandEnd = this.phase === 'showdown' || this.phase === 'idle';

            // Visual pause before next action (total interval 3-5s per action)
            if (!guardHandEnd) {
                if (guardChanged) {
                    await new Promise(resolve => setTimeout(resolve, 1000 + Math.random() * 500));
                } else {
                    const remainingPause = Math.max(120, this.aiDelay * (0.38 + Math.random() * 0.10));
                    await new Promise(resolve => setTimeout(resolve, remainingPause));
                }
                if (this.handGeneration !== currentGen) return;
            }

            if (this.checkHandEnd()) return;

            // The while loop continues with the current player.
            // If it's the human's turn, isPlayerTurn() returns true and we exit.
        }
    }

    /** Advance to next player or next phase */
    advanceGame() {
        if (this.phase === 'idle') return false; // Hand already ended — safety guard
        if (this.checkHandEnd()) return false;

        const inHand = this.getPlayersInHand();
        if (inHand.length <= 1) {
            this.endHand(inHand[0]);
            return false;
        }

        // Check if betting round is complete
        if (this.isBettingRoundComplete()) {
            this.refundUncalledBet();
            // Check if only one player not all-in (or everyone all-in)
            if (inHand.filter(p => !p.isAllIn).length <= 1) {
                // Everyone is all-in - deal remaining community cards
                this.advancePhase();
                // Deal all remaining cards by advancing through phases
                while (this.phase !== 'showdown') {
                    this.advancePhase();
                }
                // Showdown
                this.showdown();
                if (this.onUpdate) this.onUpdate();
                return false;
            }

            this.advancePhase();
            if (this.phase === 'showdown') {
                this.showdown();
                if (this.onUpdate) this.onUpdate();
                return false;
            }

            return true;
        }

        // Move to next player
        const next = this.getNextPlayerNeedingAction(this.currentPlayerIndex);
        if (next === -1) {
            // All active players are all-in
            if (this.getPlayersInHand().filter(p => !p.isAllIn).length <= 1) {
                this.advancePhase();
                while (this.phase !== 'showdown') {
                    this.advancePhase(); // advancePhase deals cards internally
                }
                this.showdown();
                if (this.onUpdate) this.onUpdate();
                return false;
            }
            return false;
        }

        this.currentPlayerIndex = next;

        if (this.onUpdate) this.onUpdate();

        return true;
    }

    /** Advance to next phase */
    advancePhase() {
        this.roundBets = {};
        this.playersActed = new Set();
        this.actedAtBet = {};
        this.raiseSizeAtAction = {};
        this.actionsThisRound = 0;
        this.currentBet = 0;
        this.lastRaise = this.bigBlind;
        this.bbNeedsOption = false; // BB option only applies pre-flop
        this.hasFullBetThisRound = false;

        switch (this.phase) {
            case 'preflop':
                this.dealCommunityCards(); // Deal flop (3 cards)
                this.phase = 'flop';
                this.bettingRound = 'flop';
                break;
            case 'flop':
                this.dealCommunityCards(); // Deal turn (1 card)
                this.phase = 'turn';
                this.bettingRound = 'turn';
                break;
            case 'turn':
                this.dealCommunityCards(); // Deal river (1 card)
                this.phase = 'river';
                this.bettingRound = 'river';
                break;
            case 'river':
                this.phase = 'showdown';
                this.bettingRound = 'showdown';
                return;
        }

        // Set starter for new round (first active player after dealer)
        this.currentPlayerIndex = this.getNextActivePlayer(this.dealerPosition);

        // Calculate new probability
        this.calculateProbability();

        if (this.onUpdate) this.onUpdate();
    }

    /** Deal community cards */
    dealCommunityCards() {
        if (this.phase === 'preflop') {
            // Deal flop: 3 cards
            this.deck.deal(); // Burn
            this.communityCards.push(this.deck.deal());
            this.communityCards.push(this.deck.deal());
            this.communityCards.push(this.deck.deal());
        } else if (this.phase === 'flop') {
            this.deck.deal(); // Burn
            this.communityCards.push(this.deck.deal());
        } else if (this.phase === 'turn') {
            this.deck.deal(); // Burn
            this.communityCards.push(this.deck.deal());
        }
    }

    /** Calculate win probability for the human player */
    calculateProbability() {
        if (!this.humanPlayer || this.humanPlayer.folded) return;

        const communityCards = [...this.communityCards];
        const numOpponents = this.getPlayersInHand().filter(p => !p.isHuman).length;

        if (this.phase === 'preflop' && communityCards.length === 0) {
            // Simple pre-flop evaluation
            const cards = this.humanPlayer.holeCards;
            const c1 = cards[0];
            const c2 = cards[1];

            let strength = 0;
            if (c1.rank === c2.rank) {
                const v = c1.value;
                strength = v >= 10 ? 0.75 : v >= 7 ? 0.58 : v >= 5 ? 0.45 : 0.38;
            } else {
                const high = Math.max(c1.value, c2.value);
                const low = Math.min(c1.value, c2.value);
                const suited = c1.suit === c2.suit;
                const gap = high - low;

                if (high === 14 && low >= 12) strength = 0.72;
                else if (high === 14 && low >= 11) strength = 0.65;
                else if (high >= 12 && low >= 11) strength = 0.58;
                else if (high >= 12 && suited) strength = 0.40;
                else if (high >= 14) strength = 0.35;
                else if (suited && gap <= 2) strength = 0.32;
                else strength = 0.25;
            }

            this.probabilityResult = {
                winProb: strength,
                tieProb: 0.05,
                loseProb: 1 - strength - 0.05
            };
            return;
        }

        // Use the calculator
        try {
            const result = ProbabilityCalculator.calculateWinProb(
                this.humanPlayer.holeCards,
                communityCards,
                Math.max(1, numOpponents),
                communityCards.length >= 3 ? 2000 : 1500
            );
            this.probabilityResult = result;
        } catch (e) {
            console.error('Probability calc error:', e);
            this.probabilityResult = { winProb: 0.5, tieProb: 0.05, loseProb: 0.45 };
        }
    }

    /** Check if hand should end */
    checkHandEnd() {
        const inHand = this.getPlayersInHand();
        if (inHand.length <= 1) {
            this.endHand(inHand.length === 1 ? inHand[0] : null);
            return true;
        }
        return false;
    }

    /** End hand with a winner */
    endHand(winner) {
        this.refundUncalledBet();
        const settledPot = this.pot;
        if (winner) {
            winner.stack += settledPot;
            const action = winner.isHuman ? 'win' : 'lost';
            if (this.onHandEnd) {
                this.onHandEnd({
                    winner,
                    pot: settledPot,
                    reason: winner.folded ? 'fold' : 'all folded',
                    name: winner.name
                });
            }
        }

        // Reset AI internal state for new hand
        for (const p of this.aiPlayers) {
            if (p.aiRef && typeof p.aiRef.reset === 'function') {
                p.aiRef.reset();
            }
        }

        // Reset for next hand
        this.phase = 'idle';
        this.numHands++;
        this.pot = 0;

        if (this.onUpdate) this.onUpdate();
    }

    /** Calculate main/side pots from every contribution, including folded chips. */
    calculateSidePots() {
        const inHand = this.getPlayersInHand();
        if (inHand.length <= 1) return [{ amount: this.pot, eligible: [...inHand] }];

        const levels = [...new Set(this.players.map(p => Math.max(0, p.chipsInPot || 0)).filter(Boolean))]
            .sort((a, b) => a - b);
        const pots = [];
        let previous = 0;
        for (const level of levels) {
            const contributors = this.players.filter(p => (p.chipsInPot || 0) >= level);
            const eligible = inHand.filter(p => (p.chipsInPot || 0) >= level);
            const amount = (level - previous) * contributors.length;
            if (amount > 0 && eligible.length > 0) pots.push({ amount, eligible });
            previous = level;
        }

        // Contributions are the source of truth. Keep a defensive reconciliation
        // for legacy saves whose pot field may differ by a chip.
        const accounted = pots.reduce((sum, pot) => sum + pot.amount, 0);
        if (this.pot > accounted && pots.length) pots[0].amount += this.pot - accounted;
        return pots;
    }

    /** Odd chips go clockwise to the first winning seat left of the button. */
    orderFromLeftOfDealer(players) {
        const n = this.players.length;
        return [...players].sort((a, b) => {
            const ai = this.players.indexOf(a);
            const bi = this.players.indexOf(b);
            const ad = ((ai - this.dealerPosition + n) % n) || n;
            const bd = ((bi - this.dealerPosition + n) % n) || n;
            return ad - bd;
        });
    }

    /** Showdown: determine winner(s) with proper side pot distribution */
    showdown() {
        const inHand = this.getPlayersInHand();
        if (inHand.length <= 1) {
            this.endHand(inHand[0]);
            return;
        }

        // Evaluate all hands
        const results = [];
        for (const p of inHand) {
            const allCards = [...p.holeCards, ...this.communityCards];
            const hand = evaluateHand(allCards);
            results.push({ player: p, hand });
        }

        // Build a lookup map for quick hand access
        const handMap = new Map();
        for (const r of results) {
            handMap.set(r.player, r.hand);
        }

        // Calculate side pots
        const sidePots = this.calculateSidePots();

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
                if (!bestHand || compareHands(hand, bestHand) > 0) {
                    bestHand = hand;
                    potWinners = [p];
                } else if (compareHands(hand, bestHand) === 0) {
                    potWinners.push(p);
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
            potWinners = this.orderFromLeftOfDealer(potWinners);
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
        if (this.pot > totalAwarded && allWinners.length > 0) {
            const leftover = this.pot - totalAwarded;
            const firstLeft = this.orderFromLeftOfDealer([...new Set(allWinners)])[0];
            firstLeft.stack += leftover;
            winnerAmounts.set(firstLeft, (winnerAmounts.get(firstLeft) || 0) + leftover);
        }

        // Deduplicate winners for display
        const uniqueWinners = [...new Set(allWinners)];

        // Best hand for display (based on the highest side pot's winner)
        const bestResult = results.reduce((best, r) =>
            (!best || (r.hand && compareHands(r.hand, best.hand) > 0)) ? r : best, null);

        const settledPot = this.pot;
        if (this.onHandEnd) {
            this.onHandEnd({
                winners: uniqueWinners,
                winnerAmounts,
                pot: settledPot,
                hand: bestResult?.hand,
                handName: bestResult?.hand?.name || 'Unknown',
                results,
                reason: 'showdown',
                share: uniqueWinners.length > 0 ? Math.floor(this.pot / uniqueWinners.length) : 0
            });
        }

        this.phase = 'idle';
        this.numHands++;
        this.pot = 0;
        if (this.onUpdate) this.onUpdate();
    }

    /** Get possible winning hands for the player */
    getPlayerWinHands() {
        if (!this.humanPlayer || this.humanPlayer.folded || this.communityCards.length === 0) {
            return [];
        }

        const allCards = [...this.humanPlayer.holeCards, ...this.communityCards];
        const currentHand = evaluateHand(allCards);
        if (!currentHand) return [];

        const handName = currentHand.name;

        // Compare with all possible hand types
        const beats = [];
        for (let rank = 0; rank < currentHand.rank; rank++) {
            beats.push(HAND_TYPE_NAMES[rank]);
        }

        return {
            currentHand: handName,
            beats: beats,
            allHandTypes: Object.values(HAND_TYPE_NAMES)
        };
    }

    /** Get outs (cards that improve the player's hand) */
    getOuts() {
        if (!this.humanPlayer || this.humanPlayer.folded || this.communityCards.length >= 5) {
            return null;
        }

        return ProbabilityCalculator.findOuts(
            this.humanPlayer.holeCards,
            this.communityCards,
            this.getPlayersInHand().filter(p => !p.isHuman).length
        );
    }

    /** Get available actions for the human player */
    getAvailableActions() {
        if (!this.isPlayerTurn()) return [];

        const toCall = Math.max(0, this.currentBet - (this.roundBets[0] || 0));
        const player = this.humanPlayer;
        const actions = [];

        if (toCall === 0) {
            actions.push({ type: 'check', label: 'Check', amount: 0 });
        } else {
            actions.push({ type: 'call', label: `Call ${Math.min(toCall, player.stack)}`, amount: Math.min(toCall, player.stack) });
        }

        if (player.stack > 0) {
            if (toCall > 0) actions.push({ type: 'fold', label: 'Fold', amount: 0 });

            const canRaise = this.canPlayerRaise(0);
            if (canRaise) {
                // Minimum raise
                const raiseTotal = this.getMinRaiseTo();
                const raiseAmount = raiseTotal - (this.roundBets[0] || 0);

                if (raiseAmount < player.stack) {
                    actions.push({ type: 'raise', label: `Raise to ${raiseTotal}`, amount: raiseTotal });
                }

            }

            const allInTotal = (this.roundBets[0] || 0) + player.stack;
            if (allInTotal <= this.currentBet || canRaise) {
                actions.push({ type: 'allin', label: 'All-in', amount: player.stack });
            }
        }

        return actions;
    }

    /** Reset game */
    resetGame() {
        this.players = [];
        this.humanPlayer = null;
        this.aiPlayers = [];
        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.phase = 'idle';
        this.numHands = 0;
        this.dealerPosition = -1;
        this.sbIndex = -1;
        this.bbIndex = -1;
        if (this.onUpdate) this.onUpdate();
    }

    /** Format card for display */
    static formatCard(card) {
        if (!card) return '';
        const suitSymbol = SUIT_SYMBOLS[card.suit];
        const isRed = card.suit === 'hearts' || card.suit === 'diamonds';
        return { rank: card.rank, suit: card.suit, symbol: suitSymbol, red: isRed };
    }
}
