/**
 * Debug utilities for poker game
 */
function debugLog(...args) {
    if (typeof DEBUG !== 'undefined' && DEBUG) {
        console.log('[DEBUG]', ...args);
    }
}
function debugState(game) {
    if (!game) return;
    debugLog('=== GAME STATE ===');
    debugLog('Phase:', game.phase);
    debugLog('Pot:', game.pot);
    debugLog('Current Bet:', game.currentBet);
    debugLog('Current Player:', game.currentPlayerIndex);
    debugLog('Community:', game.communityCards.map(c => c.toString()));
    debugLog('Players:', game.players.map((p,i) => `${i}: ${p.name} $${p.stack}`));
}
function debugCard(card) {
    return card ? card.toString() : '?';
}
