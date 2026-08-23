/**
 * Win Probability Calculator
 * Uses Monte Carlo simulation for pre-flop/flop, exact enumeration for turn/river
 */

class ProbabilityCalculator {
    /**
     * Calculate win probability for a player given known cards
     * @param {Card[]} playerCards - Player's hole cards
     * @param {Card[]} communityCards - Known community cards
     * @param {number} numOpponents - Number of opponents
     * @param {number} numSimulations - Number of Monte Carlo iterations
     * @returns {Object} { winProb, tieProb, loseProb, stats }
     */
    static calculateWinProb(playerCards, communityCards, numOpponents = 3, numSimulations = 2000) {
        const knownCards = [...playerCards, ...communityCards];
        const deck = new Deck();
        deck.removeCards(knownCards);

        const totalCards = 52;
        const unknownCards = totalCards - knownCards.length;
        const remainingCommunity = 5 - communityCards.length;

        // For small enough spaces, do exact enumeration
        if (remainingCommunity === 0 && unknownCards <= 16) {
            return this.enumerateExact(playerCards, communityCards, deck, numOpponents);
        }

        // Monte Carlo simulation
        return this.monteCarlo(playerCards, communityCards, deck, numOpponents, numSimulations, remainingCommunity);
    }

    /** Monte Carlo simulation */
    static monteCarlo(playerCards, communityCards, deck, numOpponents, numSimulations, remainingCommunity) {
        let wins = 0, ties = 0, losses = 0, equityTotal = 0;
        let totalBestHands = null;
        let stats = { bestHandCounts: {} };

        const deckCards = deck.cards;

        for (let sim = 0; sim < numSimulations; sim++) {
            deckCards.sort(() => Math.random() - 0.5);

            let idx = 0;

            // Deal remaining community cards
            const simCommunity = [...communityCards];
            for (let i = 0; i < remainingCommunity; i++) {
                simCommunity.push(deckCards[idx++]);
            }

            // Deal opponent hands
            const opponentHands = [];
            for (let o = 0; o < numOpponents; o++) {
                opponentHands.push([deckCards[idx++], deckCards[idx++]]);
            }

            // Evaluate player hand
            const playerAllCards = [...playerCards, ...simCommunity];
            const playerHand = evaluateHand(playerAllCards);

            // Evaluate opponent hands
            let bestOpponentScore = -1;
            const opponentScores = [];
            for (const hand of opponentHands) {
                const oppAllCards = [...hand, ...simCommunity];
                const oppHand = evaluateHand(oppAllCards);
                if (oppHand && oppHand.score > bestOpponentScore) {
                    bestOpponentScore = oppHand.score;
                }
                if (oppHand) opponentScores.push(oppHand.score);
            }

            // Track best hand type for stats
            if (playerHand) {
                const typeName = playerHand.name;
                stats.bestHandCounts[typeName] = (stats.bestHandCounts[typeName] || 0) + 1;
            }

            if (playerHand && playerHand.score > bestOpponentScore) {
                wins++;
                equityTotal += 1;
            } else if (playerHand && playerHand.score === bestOpponentScore) {
                ties++;
                const tiedOpponents = opponentScores.filter(score => score === playerHand.score).length;
                equityTotal += 1 / (tiedOpponents + 1);
            } else {
                losses++;
            }
        }

        return {
            winProb: wins / numSimulations,
            tieProb: ties / numSimulations,
            loseProb: losses / numSimulations,
            equityProb: equityTotal / numSimulations,
            stats
        };
    }

    /** Exact enumeration (used when few unknown cards remain) */
    static enumerateExact(playerCards, communityCards, deck, numOpponents) {
        let wins = 0, ties = 0, losses = 0, equityTotal = 0;
        let stats = { bestHandCounts: {} };
        const deckCards = deck.cards;
        const n = deckCards.length;
        let total = 0;
        const maxEnum = 500000; // Cap to avoid freeze

        // Choose remaining community cards (if any)
        const remainingCommunity = 5 - communityCards.length;

        const communityCardCombos = remainingCommunity > 0
            ? this.getCombinations(deckCards, remainingCommunity)
            : [[]];

        // Limit combinations
        const communityLimit = Math.min(communityCardCombos.length, maxEnum / (numOpponents > 0 ? Math.min(this.nCr(n - remainingCommunity, 2), 100) : 1));
        const limitedCommunityCombos = communityCardCombos.slice(0, communityLimit);

        for (const communityGroup of limitedCommunityCombos) {
            const simCommunity = [...communityCards, ...communityGroup];
            const remaining = deckCards.filter(c => !communityGroup.includes(c));

            // Enumerate opponent hands
            const oppCombos = numOpponents > 0
                ? this.getCombinations(remaining, 2)
                : [[]];
            const oppLimit = Math.min(oppCombos.length, maxEnum / limitedCommunityCombos.length / numOpponents);

            for (const oppGroup of oppCombos.slice(0, oppLimit)) {
                const playerAllCards = [...playerCards, ...simCommunity];
                const playerHand = evaluateHand(playerAllCards);

                const oppCards = [oppGroup[0], oppGroup[1]];
                const oppAllCards = [...oppCards, ...simCommunity];
                const oppHand = evaluateHand(oppAllCards);

                if (playerHand) {
                    const typeName = playerHand.name;
                    stats.bestHandCounts[typeName] = (stats.bestHandCounts[typeName] || 0) + 1;
                }

                if (playerHand && oppHand && playerHand.score > oppHand.score) {
                    wins++;
                    equityTotal += 1;
                } else if (playerHand && oppHand && playerHand.score === oppHand.score) {
                    ties++;
                    equityTotal += 0.5;
                } else {
                    losses++;
                }
                total++;
            }
        }

        if (total === 0) return { winProb: 0, tieProb: 0, loseProb: 0, equityProb: 0, stats };

        return {
            winProb: wins / total,
            tieProb: ties / total,
            loseProb: losses / total,
            equityProb: equityTotal / total,
            stats
        };
    }

    /**
     * Find outs: which specific cards improve the player's hand
     * @param {Card[]} playerCards
     * @param {Card[]} communityCards
     * @param {number} numOpponents
     * @returns {Object} { outs: Card[], byHandType: Object }
     */
    static findOuts(playerCards, communityCards, numOpponents = 3) {
        const knownCards = [...playerCards, ...communityCards];
        const deck = new Deck();
        deck.removeCards(knownCards);
        const remainingCommunity = 5 - communityCards.length;

        if (remainingCommunity <= 0) return { outs: [], byHandType: {} };

        // Evaluate current best hand
        const currentHand = evaluateHand([...playerCards, ...communityCards]);
        const baseRank = currentHand ? currentHand.rank : -1;
        const baseScore = currentHand ? currentHand.score : 0;

        // For each possible remaining community card, see if it improves hand
        const improvements = {};
        let totalOuts = 0;

        for (const card of deck.cards) {
            const newCommunity = [...communityCards, card];
            const newHand = evaluateHand([...playerCards, ...newCommunity]);
            if (newHand) {
                // Only count as an out if it improves the hand rank or meaningfully improves same-rank
                const isBetter = newHand.rank > baseRank ||
                    (newHand.rank === baseRank && newHand.score > baseScore + 10**4); // meaningful same-rank improvement
                if (!isBetter) continue;

                const typeName = newHand.name;
                if (!improvements[typeName]) {
                    improvements[typeName] = { cards: [], count: 0 };
                }
                improvements[typeName].cards.push(card);
                improvements[typeName].count++;
                totalOuts++;
            }
        }

        return { outs: deck.cards, totalOuts, byHandType: improvements };
    }

    /** Utility: n choose r */
    static nCr(n, r) {
        if (r < 0 || r > n) return 0;
        let result = 1;
        for (let i = 1; i <= r; i++) {
            result = result * (n - r + i) / i;
        }
        return Math.floor(result);
    }

    /** Get all combinations of size k from array (capped) */
    static getCombinations(arr, k) {
        if (k === 0) return [[]];
        if (arr.length < k) return [];

        const result = [];
        const first = arr[0];
        const rest = arr.slice(1);

        const withFirst = this.getCombinations(rest, k - 1);
        for (const combo of withFirst) {
            result.push([first, ...combo]);
        }

        const withoutFirst = this.getCombinations(rest, k);
        for (const combo of withoutFirst) {
            result.push(combo);
        }

        // Cap at reasonable limit
        if (result.length > 100000) {
            return result.slice(0, 100000);
        }

        return result;
    }
}
