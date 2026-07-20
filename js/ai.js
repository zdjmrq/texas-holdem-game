/**
 * AI Decision Engine — Multi-style poker AI
 */
const AI_STYLES = ['TAG', 'LAG', 'Tight-Passive', 'Maniac', 'Calling Station', 'Solid'];
const AI_AVATARS = ['🤖','🎩','🕶️','😈','🧊','🦊','🐺','🐯','🦁','🐻'];

function getRandomAIStyle() {
    return AI_STYLES[Math.floor(Math.random() * AI_STYLES.length)];
}

function getAIDecision(player, gameState, isShortDeck, difficulty) {
    const hand = player.cards;
    const community = gameState.communityCards;
    const style = player.aiStyle || 'TAG';
    const pot = gameState.pot;
    const currentBet = gameState.currentBet;
    const toCall = Math.max(0, currentBet - (gameState.roundBets[player.index] || 0));
    const stack = player.stack;
    const phase = gameState.phase;

    const handStrength = evaluateHandStrength(hand, community, isShortDeck);
    const potOdds = toCall > 0 ? toCall / (pot + toCall) : 0;

    let baseAggression = 0.5;
    let bluffFrequency = 0.1;
    let foldThreshold = 0.6;

    switch (style) {
        case 'TAG': baseAggression = 0.6; bluffFrequency = 0.08; foldThreshold = 0.65; break;
        case 'LAG': baseAggression = 0.8; bluffFrequency = 0.25; foldThreshold = 0.5; break;
        case 'Tight-Passive': baseAggression = 0.3; bluffFrequency = 0.02; foldThreshold = 0.75; break;
        case 'Maniac': baseAggression = 0.95; bluffFrequency = 0.4; foldThreshold = 0.3; break;
        case 'Calling Station': baseAggression = 0.2; bluffFrequency = 0.0; foldThreshold = 0.9; break;
        case 'Solid': baseAggression = 0.5; bluffFrequency = 0.12; foldThreshold = 0.6; break;
    }

    if (difficulty === 'elite') {
        baseAggression *= 1.2;
        foldThreshold *= 1.1;
    }

    const decisionScore = handStrength * (1 + baseAggression * 0.5) + (Math.random() - 0.5) * 0.15;
    const isBluff = Math.random() < bluffFrequency;

    if (phase === 'idle' || phase === 'showdown') {
        return { action: 'check', amount: 0 };
    }

    if (toCall === 0) {
        if (decisionScore > 0.7 || isBluff) {
            const raiseAmount = Math.max(gameState.bigBlind, Math.floor(stack * (0.3 + Math.random() * 0.5)));
            return { action: 'raise', amount: Math.min(raiseAmount, stack) };
        }
        return { action: 'check', amount: 0 };
    }

    if (decisionScore < 0.2 && toCall > stack * 0.1) {
        return { action: 'fold', amount: 0 };
    }

    const effectiveHandStrength = isBluff ? handStrength + 0.3 : handStrength;

    if (effectiveHandStrength > 0.8) {
        const raiseAmount = Math.max(toCall * 2, Math.floor(stack * (0.4 + Math.random() * 0.4)));
        return { action: 'raise', amount: Math.min(raiseAmount, stack) };
    }

    if (effectiveHandStrength > 0.5 || potOdds < 0.3) {
        return { action: 'call', amount: toCall };
    }

    if (effectiveHandStrength > foldThreshold * 0.6) {
        return { action: 'call', amount: toCall };
    }

    if (isBluff && toCall < stack * 0.15) {
        return { action: 'raise', amount: Math.min(toCall * 3, stack) };
    }

    return { action: 'fold', amount: 0 };
}

function evaluateHandStrength(holeCards, communityCards, isShortDeck) {
    if (!holeCards || holeCards.length < 2) return 0;
    const allCards = [...holeCards, ...communityCards];
    if (allCards.length < 5) {
        return evaluatePreFlopStrength(holeCards, isShortDeck);
    }
    const bestHand = getBestHand(holeCards, communityCards);
    if (!bestHand) return 0;
    const strengths = { 9: 1.0, 8: 0.98, 7: 0.95, 6: 0.9, 5: 0.85, 4: 0.8, 3: 0.7, 2: 0.6, 1: 0.4, 0: 0.2 };
    let strength = strengths[bestHand.rank] || 0.2;
    if (isShortDeck && bestHand.rank >= 4) strength = Math.min(1.0, strength + 0.08);
    return strength;
}

function evaluatePreFlopStrength(cards, isShortDeck) {
    if (!cards || cards.length < 2) return 0.2;
    const v1 = cards[0].value, v2 = cards[1].value;
    const paired = v1 === v2;
    const suited = cards[0].suit === cards[1].suit;
    const high = Math.max(v1, v2), low = Math.min(v1, v2);
    let strength = 0.2;
    if (paired) {
        strength = 0.3 + low / 14 * 0.5;
    } else {
        strength = high / 14 * 0.4 + low / 14 * 0.15;
        if (suited) strength += 0.05;
        if (high - low === 1) strength += 0.05;
        if (high >= 12 && low >= 11) strength += 0.1;
    }
    if (isShortDeck) strength = Math.min(1, strength * 1.15);
    return Math.min(1, Math.max(0.05, strength));
}