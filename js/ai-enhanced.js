/**
 * Enhanced AI Decision Engine
 */
function getEnhancedAIDecision(player, gameState, isShortDeck) {
    const basic = getAIDecision(player, gameState, isShortDeck, 'elite');
    const hand = player.cards;
    const community = gameState.communityCards;
    const pot = gameState.pot;
    const toCall = Math.max(0, gameState.currentBet - (gameState.roundBets[player.index] || 0));
    const stack = player.stack;
    const phase = gameState.phase;
    const totalPlayers = gameState.players.filter(p => !p.folded && !p.isAllIn).length;

    if (phase === 'preflop' || phase === 'idle') return basic;

    const bestHand = getBestHand(hand, community);
    const handStrength = bestHand ? bestHand.rank : 0;
    const outs = countOuts(hand, community, isShortDeck);
    const equity = outs / (isShortDeck ? 36 : 52);

    if (handStrength >= 4 && toCall > stack * 0.3) return { action: 'call', amount: toCall };
    if (equity > 0.3 && toCall / (pot + toCall) < equity * 0.8) {
        if (Math.random() < 0.3 && toCall < stack * 0.2) {
            return { action: 'raise', amount: Math.min(toCall * 3 + Math.floor(pot * 0.5), stack) };
        }
        return { action: 'call', amount: toCall };
    }
    if (player.aiStyle === 'Maniac' && Math.random() < 0.2) {
        return { action: 'raise', amount: Math.min(toCall * 3 + pot, stack) };
    }
    return basic;
}

function countOuts(holeCards, communityCards, isShortDeck) {
    const allCards = [...holeCards, ...communityCards];
    const deck = new Deck();
    const remaining = deck.cards.filter(c => !allCards.some(hc => hc.rank === c.rank && hc.suit === c.suit));
    let outs = 0;
    for (const c of remaining) {
        const testCards = [...allCards, c];
        const hand = getBestHand(holeCards, [...communityCards, c]);
        if (hand && hand.rank >= 4) outs++;
    }
    return outs;
}