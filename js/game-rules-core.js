(function(root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    else root.PokerGameRules = api;
})(typeof globalThis !== 'undefined' ? globalThis : self, function() {
    'use strict';

    function decisionIdentity(variant, phase, actionOrdinal, seatId) {
        const normalizedVariant = variant === 'shortdeck' ? 'shortdeck' : 'standard';
        return `${normalizedVariant}:${phase || 'preflop'}:${Math.max(0, Number(actionOrdinal) || 0)}:${seatId}`;
    }

    function buildActionMeta(input = {}) {
        const callCost = Math.max(0, Number(input.callCost ?? input.toCallBefore) || 0);
        const additionalPaid = Math.max(0, Number(input.additionalPaid ?? input.amount) || 0);
        const raiseTo = Math.max(0, Number(input.raiseTo) || 0);
        const currentBetBefore = Math.max(0, Number(input.currentBetBefore) || 0);
        const potBefore = Math.max(0, Number(input.potBefore) || 0);
        const aggressive = !!input.aggressive;
        const wagerSize = aggressive ? Math.max(0, raiseTo - currentBetBefore) : additionalPaid;
        return {
            ...input,
            callCost,
            toCallBefore:callCost,
            additionalPaid,
            amount:additionalPaid,
            raiseTo,
            currentBetBefore,
            potBefore,
            potAfter:Math.max(0, Number(input.potAfter) || 0),
            aggressive,
            betFraction:wagerSize / Math.max(1, Number(input.minimumBet) || 1, potBefore)
        };
    }

    function calculateSidePots(players, totalPot, eligiblePlayers) {
        const eligibleSet = new Set(eligiblePlayers || players.filter(player => !player.folded));
        if (eligibleSet.size <= 1) return [{ amount:Math.max(0, totalPot), eligible:[...eligibleSet] }];
        const levels = [...new Set(players.map(player => Math.max(0, Number(player.chipsInPot) || 0)).filter(Boolean))]
            .sort((a, b) => a - b);
        const pots = [];
        let previous = 0;
        for (const level of levels) {
            const contributors = players.filter(player => (Number(player.chipsInPot) || 0) >= level);
            const eligible = contributors.filter(player => eligibleSet.has(player));
            const amount = (level - previous) * contributors.length;
            if (amount > 0 && eligible.length) pots.push({ amount, eligible });
            previous = level;
        }
        const accounted = pots.reduce((sum, pot) => sum + pot.amount, 0);
        const remainder = Math.max(0, Number(totalPot) || 0) - accounted;
        if (remainder > 0 && pots.length) pots[0].amount += remainder;
        return pots;
    }

    function findUncalledRefund(roundBets) {
        const entries = Object.entries(roundBets || {}).map(([index, amount]) => ({
            index:Number(index),
            amount:Math.max(0, Number(amount) || 0)
        })).sort((a, b) => b.amount - a.amount);
        if (!entries.length) return null;
        const second = entries[1]?.amount || 0;
        if (entries[0].amount === second) return null;
        return { index:entries[0].index, refund:entries[0].amount - second };
    }

    return { decisionIdentity, buildActionMeta, calculateSidePots, findUncalledRefund };
});
