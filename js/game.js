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
        this.outsResult = null;

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
        this.outsResult = null;

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

    /** Seat index of the human player (0 for the standard single-human table) */
    get humanSeat() {
        const index = this.players.indexOf(this.humanPlayer);
        return index >= 0 ? index : 0;
    }

    /** Check if it's the human player's turn */
    /** Delegates to the shared betting engine (js/engine.js). */
    isPlayerTurn() {
        const game = this;
        return PokerEngine.isPlayerTurn(game);
    }

    /** Get the current player object */
    get currentPlayer() {
        return this.players[this.currentPlayerIndex];
    }

    /** Get active (not folded, not busted) players */
    /** Delegates to the shared betting engine (js/engine.js). */
    getActivePlayers() {
        const game = this;
        return PokerEngine.getActivePlayers(game);
    }

    /** Get players still in the hand (not folded, has chips or all-in) */
    /** Delegates to the shared betting engine (js/engine.js). */
    getPlayersInHand() {
        const game = this;
        return PokerEngine.getPlayersInHand(game);
    }

    /** Get next active player starting from pos */
    /** Delegates to the shared betting engine (js/engine.js). */
    getNextActivePlayer(startPos) {
        const game = this;
        return PokerEngine.getNextActivePlayer(game, startPos);
    }

    /** Get next player (for dealing, can be all-in) */
    /** Delegates to the shared betting engine (js/engine.js). */
    getNextPlayerInHand(startPos) {
        const game = this;
        return PokerEngine.getNextPlayerInHand(game, startPos);
    }

    /** Position among seats actually dealt into this hand, ignoring empty/busted seats. */
    /** Delegates to the shared betting engine (js/engine.js). */
    getHandPositionInfo(playerIndex) {
        const game = this;
        return PokerEngine.getHandPositionInfo(game, playerIndex);
    }

    /** Find the next live player who has not acted or has chips left to call. */
    /** Delegates to the shared betting engine (js/engine.js). */
    getNextPlayerNeedingAction(startPos) {
        const game = this;
        return PokerEngine.getNextPlayerNeedingAction(game, startPos);
    }

    /** Whether this player still has the right to make a raise this round. */
    /** Delegates to the shared betting engine (js/engine.js). */
    canPlayerRaise(playerIndex) {
        const game = this;
        return PokerEngine.canPlayerRaise(game, playerIndex);
    }

    /** Delegates to the shared betting engine (js/engine.js). */
    recordRoundAction(playerIndex) {
        const game = this;
        return PokerEngine.recordRoundAction(game, playerIndex);
    }

    /** Delegates to the shared betting engine (js/engine.js). */
    getMinRaiseTo() {
        const game = this;
        return PokerEngine.getMinRaiseTo(game);
    }

    /** Auto-close a round when nobody has a call/fold decision left. */
    /** Delegates to the shared betting engine (js/engine.js). */
    markRoundCompleteIfNoDecision() {
        const game = this;
        return PokerEngine.markRoundCompleteIfNoDecision(game);
    }

    /** Return a unique unmatched top wager before advancing the street. */
    /** Delegates to the shared betting engine (js/engine.js). */
    refundUncalledBet() {
        const game = this;
        return PokerEngine.refundUncalledBet(game);
    }

    /** Check if betting round is complete */
    /** Delegates to the shared betting engine (js/engine.js). */
    isBettingRoundComplete() {
        const game = this;
        return PokerEngine.isBettingRoundComplete(game);
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
    /** Delegates to the shared betting engine (js/engine.js). */
    executeAction(playerIndex, action, amount) {
        const game = this;
        return PokerEngine.executeAction(game, playerIndex, action, amount);
    }

    /** Notify all AI players about an action taken by any player */
    /** Delegates to the shared betting engine (js/engine.js). */
    notifyAIsOfAction(playerIndex, action, actionMeta = {}) {
        const game = this;
        return PokerEngine.notifyAIsOfAction(game, playerIndex, action, actionMeta);
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
            const ownBet = this.roundBets[idx] || 0;
            const toCall = Math.max(0, this.currentBet - ownBet);
            const canRaise = this.canPlayerRaise(idx);
            const maxRaiseTo = ownBet + player.stack;
            const legalActions = toCall > 0 ? ['fold', 'call'] : ['check'];
            if (player.stack > 0 && canRaise && maxRaiseTo >= this.getMinRaiseTo()) legalActions.push('raise');
            if (player.stack > 0 && (maxRaiseTo <= this.currentBet || canRaise)) legalActions.push('allin');

            const gameState = {
                handId: this.handGeneration,
                decisionId: PokerGameRules.decisionIdentity('standard', this.phase, this.actionsThisRound, idx),
                seed: 'poker-table:standard',
                variant: 'standard',
                communityCards: this.communityCards,
                pot: this.pot,
                currentBet: this.currentBet,
                toCall,
                yourBet: ownBet,
                stack: player.stack,
                effectiveStack: Math.min(player.stack, Math.max(0, ...opponentStacks)),
                numOpponentsActive: activePlayers.length - 1,
                dealerPosition: this.dealerPosition,
                currentPlayerIndex: idx,
                activeOpponentSeats: activePlayers.filter(p => p !== player)
                    .map(p => this.players.indexOf(p)),
                playerCount: tablePosition.playerCount,
                positionFromButton: tablePosition.positionFromButton,
                legalActions,
                canCheck: ownBet >= this.currentBet,
                canRaise,
                minRaiseTo: this.getMinRaiseTo(),
                maxRaiseTo,
                preflopRaiseCount: this.preflopRaiseCount,
                isPreviousStreetAggressor: this.lastAggressor === idx &&
                    this.lastAggressorStreet === ({ flop:'preflop', turn:'flop', river:'turn' }[this.phase] || null),
                isDelayedCBetCandidate: this.phase === 'turn' && this.preflopAggressor === idx &&
                    !this.flopHadAggression,
                isSmallBlind: idx === this.sbIndex,
                isBigBlind: idx === this.bbIndex,
                bigBlind: this.bigBlind,
                phase: this.phase,
                timeBudgetMs: this.phase === 'river' || activePlayers.length >= 5 ? 150 : 80
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

            const decision = typeof ai.decideAsync === 'function'
                ? await ai.decideAsync(gameState)
                : ai.decide(gameState);
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
    /** Delegates to the shared betting engine (js/engine.js). */
    advanceGame() {
        const game = this;
        return PokerEngine.advanceGame(game);
    }

    /** Advance to next phase */
    /** Delegates to the shared betting engine (js/engine.js). */
    advancePhase() {
        const game = this;
        return PokerEngine.advancePhase(game);
    }

    /** Deal community cards */
    /** Delegates to the shared betting engine (js/engine.js). */
    dealCommunityCards() {
        const game = this;
        return PokerEngine.dealCommunityCards(game);
    }

    /** Calculate win probability for the human player */
    calculateProbability() {
        if (!this.humanPlayer || this.humanPlayer.folded || this.humanPlayer.holeCards.length !== 2) return;
        const communityCards = [...this.communityCards];
        const numOpponents = Math.max(1, this.getPlayersInHand().filter(p => !p.isHuman).length);
        const generation = this.handGeneration;
        const phase = this.phase;
        const requestKey = probabilityService.makeKey({
            playerCards:this.humanPlayer.holeCards,
            communityCards,
            numOpponents,
            isShortDeck:false
        });
        this.probabilityRequestKey = requestKey;
        probabilityService.calculate({
            playerCards:this.humanPlayer.holeCards,
            communityCards,
            numOpponents,
            isShortDeck:false
        }).then(result => {
            if (generation !== this.handGeneration || phase !== this.phase || requestKey !== this.probabilityRequestKey) return;
            this.probabilityResult = result;
            this.outsResult = result.outs || null;
            if (this.onUpdate) this.onUpdate();
        }).catch(error => {
            if (generation === this.handGeneration && requestKey === this.probabilityRequestKey) {
                console.error('Probability calc error:', error);
            }
        });
    }

    /** Check if hand should end */
    /** Delegates to the shared betting engine (js/engine.js). */
    checkHandEnd() {
        const game = this;
        return PokerEngine.checkHandEnd(game);
    }

    /** End hand with a winner */
    endHand(winner) {
        if (this.phase === 'idle') return;
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
    /** Delegates to the shared betting engine (js/engine.js). */
    calculateSidePots() {
        const game = this;
        return PokerEngine.calculateSidePots(game);
    }

    /** Odd chips go clockwise to the first winning seat left of the button. */
    /** Delegates to the shared betting engine (js/engine.js). */
    orderFromLeftOfDealer(players) {
        const game = this;
        return PokerEngine.orderFromLeftOfDealer(game, players);
    }

    /** Showdown: determine winner(s) with proper side pot distribution */
    /** Delegates to the shared betting engine (js/engine.js). */
    showdown() {
        const game = this;
        return PokerEngine.showdown(game, { evaluate: evaluateHand, compare: compareHands });
    }

    /** Delegates to the shared betting engine (js/engine.js). */
    notifyAIsOfShowdown(results, winners) {
        const game = this;
        return PokerEngine.notifyAIsOfShowdown(game, results, winners);
    }

    /** Get possible winning hands for the player */
    getPlayerWinHands() {
        // Always return the same shape so callers never have to branch on the type.
        if (!this.humanPlayer || this.humanPlayer.folded || this.communityCards.length === 0) {
            return { currentHand: null, beats: [], allHandTypes: Object.values(HAND_TYPE_NAMES) };
        }

        const allCards = [...this.humanPlayer.holeCards, ...this.communityCards];
        const currentHand = evaluateHand(allCards);
        if (!currentHand) return { currentHand: null, beats: [], allHandTypes: Object.values(HAND_TYPE_NAMES) };

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
    /** Delegates to the shared betting engine (js/engine.js). */
    getOuts() {
        const game = this;
        return PokerEngine.getOuts(game);
    }

    /** Get available actions for the human player */
    /** Delegates to the shared betting engine (js/engine.js). */
    getAvailableActions() {
        const game = this;
        return PokerEngine.getAvailableActions(game);
    }

    /** Reset game */
    /** Delegates to the shared betting engine (js/engine.js). */
    resetGame() {
        const game = this;
        return PokerEngine.resetGame(game);
    }

    /** Format card for display */
    static formatCard(card) {
        if (!card) return '';
        const suitSymbol = SUIT_SYMBOLS[card.suit];
        const isRed = card.suit === 'hearts' || card.suit === 'diamonds';
        return { rank: card.rank, suit: card.suit, symbol: suitSymbol, red: isRed };
    }
}
