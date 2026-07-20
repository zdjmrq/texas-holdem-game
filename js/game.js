/**
 * Texas Hold'em Poker - Main Game Controller
 */
class PokerGame {
    constructor() {
        this.players = [];
        this.humanPlayer = null;
        this.aiPlayers = [];
        this.deck = null;
        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.lastRaise = 0;
        this.dealerPosition = -1;
        this.currentPlayerIndex = 0;
        this.smallBlind = 40;
        this.bigBlind = 80;
        this.phase = 'idle';
        this.bettingRound = 'preflop';
        this.minRaise = this.bigBlind;
        this.handHistory = [];
        this.numHands = 0;
        this.handGeneration = 0;
        this.lastAggressor = -1;
        this.roundBets = {};
        this.playersActed = new Set();
        this.actionsThisRound = 0;
        this.isShortDeck = false;
        this.difficulty = 'normal';
    }

    initPlayers(playerCount, difficulty, isShortDeck) {
        this.isShortDeck = isShortDeck;
        this.difficulty = difficulty;
        this.players = [];
        this.players.push({ name: '你', stack: 1000, cards: [], folded: false, isAllIn: false, isHuman: true, aiStyle: null, index: 0, position: 0 });
        const aiNames = ['Alex','Blake','Casey','Drew','Eden','Finn','Gray','Jade','Kai'];
        for (let i = 1; i < playerCount; i++) {
            this.players.push({
                name: aiNames[i-1] || 'AI_' + i,
                stack: 1000,
                cards: [],
                folded: false,
                isAllIn: false,
                isHuman: false,
                aiStyle: getRandomAIStyle(),
                avatar: AI_AVATARS[i % AI_AVATARS.length],
                index: i,
                position: i
            });
        }
        this.humanPlayer = this.players[0];
        this.aiPlayers = this.players.filter(p => !p.isHuman);
    }

    startNewHand() {
        this.handGeneration++;
        this.dealerPosition = (this.dealerPosition + 1) % this.players.length;
        const activePlayers = this.players.filter(p => p.stack > 0);
        if (activePlayers.length <= 1) {
            this.phase = 'idle';
            return;
        }

        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.lastRaise = 0;
        this.minRaise = this.bigBlind;
        this.phase = 'preflop';
        this.bettingRound = 'preflop';
        this.roundBets = {};
        this.playersActed = new Set();
        this.actionsThisRound = 0;
        this.lastAggressor = -1;

        for (const p of this.players) {
            if (p.stack <= 0) continue;
            p.folded = false;
            p.isAllIn = false;
            p.cards = [];
            p.lastAction = null;
            this.roundBets[p.index] = 0;
        }

        const deckRanks = this.isShortDeck
            ? ['6','7','8','9','10','J','Q','K','A']
            : ['2','3','4','5','6','7','8','9','10','J','Q','K','A'];
        this.deck = new Deck(deckRanks);
        this.deck.shuffle();

        for (const p of this.players) {
            if (p.stack > 0) {
                p.cards.push(this.deck.deal());
                p.cards.push(this.deck.deal());
            }
        }

        const sbPos = (this.dealerPosition + 1) % this.players.length;
        const bbPos = (this.dealerPosition + 2) % this.players.length;

        if (this.players[sbPos].stack > 0) {
            const sb = Math.min(this.smallBlind, this.players[sbPos].stack);
            this.players[sbPos].stack -= sb;
            this.roundBets[sbPos] = (this.roundBets[sbPos] || 0) + sb;
            this.pot += sb;
            this.players[sbPos].lastAction = 'sb';
        }
        if (this.players[bbPos].stack > 0) {
            const bb = Math.min(this.bigBlind, this.players[bbPos].stack);
            this.players[bbPos].stack -= bb;
            this.roundBets[bbPos] = (this.roundBets[bbPos] || 0) + bb;
            this.pot += bb;
            this.players[bbPos].lastAction = 'bb';
        }

        this.currentBet = this.bigBlind;
        this.currentPlayerIndex = (bbPos + 1) % this.players.length;
        this.numHands++;

        if (this.numHands % 5 === 0 && this.bigBlind < 500) {
            this.bigBlind = Math.min(500, this.bigBlind + 20);
            this.smallBlind = Math.floor(this.bigBlind / 2);
        }
    }

    doAction(playerIdx, action, amount) {
        const player = this.players[playerIdx];
        if (!player || player.folded || player.isAllIn) return;
        const toCall = Math.max(0, this.currentBet - (this.roundBets[playerIdx] || 0));

        switch (action) {
            case 'fold':
                player.folded = true;
                player.lastAction = 'fold';
                break;
            case 'check':
                player.lastAction = 'check';
                break;
            case 'call':
                const callAmount = Math.min(toCall, player.stack);
                player.stack -= callAmount;
                this.roundBets[playerIdx] = (this.roundBets[playerIdx] || 0) + callAmount;
                this.pot += callAmount;
                player.lastAction = 'call';
                break;
            case 'raise':
                const totalBet = Math.max(toCall, Math.min(amount || this.minRaise * 2, player.stack));
                const raiseAmount = Math.min(totalBet, player.stack);
                player.stack -= raiseAmount;
                this.roundBets[playerIdx] = (this.roundBets[playerIdx] || 0) + raiseAmount;
                this.pot += raiseAmount;
                this.currentBet = this.roundBets[playerIdx];
                this.lastRaise = raiseAmount;
                this.lastAggressor = playerIdx;
                player.lastAction = 'raise';
                break;
            case 'allin':
                const allAmount = player.stack;
                player.stack = 0;
                this.roundBets[playerIdx] = (this.roundBets[playerIdx] || 0) + allAmount;
                this.pot += allAmount;
                if (this.roundBets[playerIdx] > this.currentBet) {
                    this.currentBet = this.roundBets[playerIdx];
                    this.lastAggressor = playerIdx;
                }
                player.isAllIn = true;
                player.lastAction = 'allin';
                break;
        }

        this.playersActed.add(playerIdx);
        this.actionsThisRound++;
        this.advanceGame();
    }

    advanceGame() {
        const inHand = this.players.filter(p => !p.folded && p.stack > 0);
        const canAct = inHand.filter(p => !p.isAllIn);

        if (canAct.length <= 1) {
            this.endHand(inHand);
            return;
        }

        const allActed = this.players.every(p => {
            if (p.folded || p.isAllIn) return true;
            return this.playersActed.has(p.index) && this.roundBets[p.index] >= this.currentBet;
        });

        if (allActed && this.actionsThisRound >= inHand.length) {
            this.nextStreet();
            return;
        }

        let next = (this.currentPlayerIndex + 1) % this.players.length;
        let attempts = 0;
        while (attempts < this.players.length) {
            const p = this.players[next];
            if (!p.folded && !p.isAllIn && p.stack > 0) {
                this.currentPlayerIndex = next;
                return;
            }
            next = (next + 1) % this.players.length;
            attempts++;
        }
        this.endHand(inHand);
    }

    nextStreet() {
        this.playersActed = new Set();
        this.actionsThisRound = 0;

        switch (this.bettingRound) {
            case 'preflop':
                this.bettingRound = 'flop';
                this.phase = 'flop';
                this.communityCards.push(this.deck.deal());
                this.communityCards.push(this.deck.deal());
                this.communityCards.push(this.deck.deal());
                break;
            case 'flop':
                this.bettingRound = 'turn';
                this.phase = 'turn';
                this.communityCards.push(this.deck.deal());
                break;
            case 'turn':
                this.bettingRound = 'river';
                this.phase = 'river';
                this.communityCards.push(this.deck.deal());
                break;
            case 'river':
                this.doShowdown();
                return;
        }

        this.currentBet = 0;
        this.lastRaise = 0;
        this.minRaise = this.bigBlind;
        for (const p of this.players) {
            this.roundBets[p.index] = 0;
        }

        const inHand = this.players.filter(p => !p.folded && p.stack > 0);
        if (inHand.length <= 1) {
            this.endHand(inHand);
            return;
        }

        this.currentPlayerIndex = this.players.findIndex(p => !p.folded && p.stack > 0);
    }

    doShowdown() {
        this.phase = 'showdown';
        const inHand = this.players.filter(p => !p.folded && p.stack >= 0);
        const results = inHand.map(p => ({
            player: p,
            hand: getBestHand(p.cards, this.communityCards)
        }));
        results.sort((a, b) => b.hand.value - a.hand.value);
        const winner = results[0];
        const potWinners = results.filter(r => r.hand.value === winner.hand.value);
        const share = Math.floor(this.pot / potWinners.length);
        for (const w of potWinners) {
            w.player.stack += share;
        }
        this.lastHandResult = {
            winners: potWinners.map(w => ({ name: w.player.name, hand: w.hand })),
            pot: this.pot
        };
        this.pot = 0;
    }

    endHand(inHand) {
        if (inHand.length === 1) {
            inHand[0].stack += this.pot;
            this.lastHandResult = {
                winners: [{ name: inHand[0].name, hand: null }],
                pot: this.pot
            };
            this.pot = 0;
        }
        this.phase = 'idle';
    }

    getAIDecision(playerIdx) {
        const player = this.players[playerIdx];
        if (!player || player.isHuman) return { action: 'check', amount: 0 };
        const gameState = this.getPublicState();
        if (this.difficulty === 'elite' || this.players.length <= 4) {
            return getEnhancedAIDecision(player, gameState, this.isShortDeck);
        }
        return getAIDecision(player, gameState, this.isShortDeck, this.difficulty);
    }

    getPublicState() {
        return {
            players: this.players.map(p => ({
                name: p.name,
                stack: p.stack,
                folded: p.folded,
                isAllIn: p.isAllIn,
                isHuman: p.isHuman,
                lastAction: p.lastAction,
                index: p.index,
                position: p.position
            })),
            communityCards: this.communityCards.map(c => ({ rank: c.rank, suit: c.suit })),
            pot: this.pot,
            currentBet: this.currentBet,
            phase: this.phase,
            bettingRound: this.bettingRound,
            dealerPosition: this.dealerPosition,
            currentPlayerIndex: this.currentPlayerIndex,
            roundBets: { ...this.roundBets },
            smallBlind: this.smallBlind,
            bigBlind: this.bigBlind,
            numHands: this.numHands
        };
    }

    getPlayerState(playerIdx) {
        const p = this.players[playerIdx];
        if (!p) return null;
        return {
            cards: p.cards.map(c => ({ rank: c.rank, suit: c.suit })),
            stack: p.stack,
            bet: this.roundBets[playerIdx] || 0,
            position: p.position,
            isYourTurn: this.currentPlayerIndex === playerIdx && !p.folded && !p.isAllIn && this.phase !== 'idle',
            availableActions: this.getAvailableActions(playerIdx)
        };
    }

    getAvailableActions(playerIdx) {
        const p = this.players[playerIdx];
        if (!p || p.folded || p.isAllIn || this.phase === 'idle' || this.phase === 'showdown') return [];
        const toCall = Math.max(0, this.currentBet - (this.roundBets[playerIdx] || 0));
        const actions = [];
        if (toCall === 0) {
            actions.push('check');
        } else {
            actions.push('call', 'fold');
        }
        const minRaiseAmount = Math.max(this.minRaise, toCall * 2);
        if (p.stack > toCall + this.minRaise) {
            actions.push('raise');
        } else if (p.stack > toCall) {
            actions.push('allin');
        }
        if (p.stack > 0 && toCall > 0) actions.push('allin');
        return actions;
    }
}