/**
 * Short Deck (6+ Hold'em) variant rules
 */
const SHORT_DECK_RANKS = ['6','7','8','9','10','J','Q','K','A'];
const SHORT_DECK_RANK_VALUES = { '6':6,'7':7,'8':8,'9':9,'10':10,'J':11,'Q':12,'K':13,'A':14 };

function initShortDeckGame(game) {
    game.isShortDeck = true;
    game.smallBlind = 40;
    game.bigBlind = 80;
}

function evaluateShortDeckHand(cards) {
    const ranks = cards.map(c => c.value).sort((a,b) => b-a);
    const suits = cards.map(c => c.suit);
    const isFlush = suits.every(s => s === suits[0]);
    const rankCounts = {};
    for (const r of ranks) rankCounts[r] = (rankCounts[r] || 0) + 1;
    const groups = Object.entries(rankCounts).sort((a,b) => b[1]-a[1] || b[0]-a[0]);
    const unique = [...new Set(ranks)].sort((a,b) => b-a);
    let isStraight = false, straightHigh = 0;
    if (unique.length >= 5) {
        for (let i = 0; i <= unique.length - 5; i++) {
            if (unique[i] - unique[i+4] === 4) {
                isStraight = true;
                straightHigh = unique[i];
                break;
            }
        }
        if (!isStraight && unique.includes(14) && unique.includes(6)) {
            const low = [...unique.filter(v => v <= 6), 1].sort((a,b) => b-a);
            for (let i = 0; i <= low.length - 5; i++) {
                if (low[i] - low[i+4] === 4) {
                    isStraight = true;
                    straightHigh = low[i];
                    break;
                }
            }
        }
    }
    const HAND_RANK = isFlush && isStraight && straightHigh === 14 ? 9
        : isFlush && isStraight ? 8
        : groups[0][1] === 4 ? 7
        : isFlush ? 6
        : groups[0][1] === 3 && groups[1] && groups[1][1] >= 2 ? 5
        : isStraight ? 4
        : groups[0][1] === 3 ? 3
        : groups[0][1] === 2 && groups[1] && groups[1][1] === 2 ? 2
        : groups[0][1] === 2 ? 1
        : 0;
    const names = { 9: '皇家同花顺', 8: '同花顺', 7: '四条', 6: '同花', 5: '葫芦', 4: '顺子', 3: '三条', 2: '两对', 1: '一对', 0: '高牌' };
    return { rank: HAND_RANK, name: names[HAND_RANK], value: HAND_RANK * 1e12 + ranks.reduce((s,v,i) => s + v * Math.pow(100, 4-i), 0) };
}

function getBestShortDeckHand(holeCards, communityCards) {
    const all = [...holeCards, ...communityCards];
    if (all.length < 5) return { rank: 0, name: '等待更多牌', value: 0 };
    let best = null;
    function combos(arr, k, start=0, chosen=[]) {
        if (chosen.length === k) {
            const result = evaluateShortDeckHand(chosen);
            if (!best || result.value > best.value) best = result;
            return;
        }
        for (let i = start; i < arr.length; i++) {
            chosen.push(arr[i]);
            combos(arr, k, i+1, chosen);
            chosen.pop();
        }
    }
    combos(all, 5);
    return best;
}