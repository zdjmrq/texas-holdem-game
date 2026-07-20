/**
 * Probability Calculator — Win rate, tie rate, loss rate
 */
function calculateProbabilities(holeCards, communityCards, totalPlayers, isShortDeck) {
    const empty = { win: 0, tie: 0, lose: 0, winRate: 0, tieRate: 0, loseRate: 0 };
    if (!holeCards || holeCards.length < 2) return empty;

    const deck = isShortDeck
        ? new Deck(['6','7','8','9','10','J','Q','K','A'])
        : new Deck();
    const used = new Set();
    for (const c of [...holeCards, ...communityCards]) {
        if (c && c.rank) used.add(c.rank + c.suit);
    }
    deck.cards = deck.cards.filter(c => !used.has(c.rank + c.suit));
    deck.shuffle();

    let wins = 0, ties = 0, losses = 0;
    const sims = 500;

    for (let sim = 0; sim < sims; sim++) {
        deck.shuffle();
        const remaining = [...deck.cards];
        const needed = 5 - communityCards.length;
        const board = [...communityCards];
        for (let i = 0; i < needed && remaining.length; i++) {
            board.push(remaining.pop());
        }
        const myBest = getBestHand(holeCards, board);
        let bestScore = 0, bestCount = 1;
        let myScore = myBest ? myBest.value : 0;
        for (let opp = 1; opp < totalPlayers; opp++) {
            if (remaining.length < 2) break;
            const oppCards = [remaining.pop(), remaining.pop()];
            const oppBest = getBestHand(oppCards, board);
            const oppScore = oppBest ? oppBest.value : 0;
            if (oppScore > myScore) { losses++; break; }
            if (oppScore === myScore) bestCount++;
        }
        if (bestScore > myScore) losses++;
        else if (bestCount > 1) ties++;
        else wins++;
    }

    const total = wins + ties + losses;
    return {
        win, tie, lose,
        winRate: total > 0 ? wins / total * 100 : 0,
        tieRate: total > 0 ? ties / total * 100 : 0,
        loseRate: total > 0 ? losses / total * 100 : 0
    };
}

function getHandDescription(holeCards, communityCards) {
    if (!holeCards || holeCards.length < 2) return '等待手牌';
    if (communityCards.length < 3) {
        const v1 = holeCards[0].value, v2 = holeCards[1].value;
        if (v1 === v2) return '一对 ' + holeCards[0].rank;
        const suited = holeCards[0].suit === holeCards[1].suit;
        let desc = Math.max(v1, v2) >= 11 ? '高牌' : '小牌';
        if (Math.max(v1, v2) - Math.min(v1, v2) <= 2) desc = '连张' + desc;
        if (suited) desc += '同花';
        return desc;
    }
    const best = getBestHand(holeCards, communityCards);
    return best ? best.name : '高牌';
}