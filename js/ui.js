/**
 * UI Layer — Rendering, event binding
 */
let game = null;
let gameMode = 'standard';
let gameDifficulty = 'normal';
let gameStarted = false;
let playMode = 'local';
let isRoomHost = false;
let inRoom = false;

function selectDifficulty(diff) {
    gameDifficulty = diff;
    document.querySelectorAll('.diff-btn').forEach(b => b.classList.toggle('selected', b.dataset.diff === diff));
}

function selectGameMode(mode) {
    gameMode = mode;
    document.querySelectorAll('.mode-btn').forEach(b => b.classList.toggle('selected', b.dataset.mode === mode));
}

function selectPlayMode(mode) {
    playMode = mode;
    document.querySelectorAll('.play-btn').forEach(b => b.classList.toggle('selected', b.dataset.play === mode));
    document.getElementById('localOptions').style.display = mode === 'local' ? 'block' : 'none';
    document.getElementById('onlinePanel').style.display = mode === 'online' ? 'block' : 'none';
}

const startScreen = document.getElementById('startScreen');
const bottomPanel = document.getElementById('bottomPanel');

function startNewGame() {
    const selectedCount = document.querySelector('.count-btn.selected');
    const count = selectedCount ? parseInt(selectedCount.dataset.count) : 6;
    const isShortDeck = gameMode === 'shortdeck';
    game = new PokerGame();
    game.initPlayers(count, gameDifficulty, isShortDeck);
    game.startNewHand();
    startScreen.style.display = 'none';
    bottomPanel.style.display = 'flex';
    gameStarted = true;
    renderAIPlayers();
    renderCommunityCards();
    renderHumanCards();
    updateBlinds();
    updateUI();
    updatePotDisplay();
    updateHandCount();
    if (game.currentPlayerIndex !== 0 && game.phase !== 'idle') scheduleAIActions();
}

function renderAIPlayers() {
    if (!game) return;
    const container = document.getElementById('aiPlayersContainer');
    container.innerHTML = '';
    const aiPlayers = game.players.filter(p => !p.isHuman);
    const total = aiPlayers.length;
    const radiusX = 280, radiusY = 160;
    const centerX = container.offsetWidth / 2 || 300;
    const centerY = container.offsetHeight / 2 || 180;
    aiPlayers.forEach((p, i) => {
        const angle = (i / total) * Math.PI - Math.PI * 0.65;
        const x = centerX + radiusX * Math.cos(angle);
        const y = centerY + radiusY * Math.sin(angle);
        const div = document.createElement('div');
        div.className = 'player-seat';
        div.id = 'ai-seat-' + p.index;
        div.style.cssText = 'left:' + x + 'px;top:' + y + 'px;position:absolute;transform:translate(-50%,-50%)';
        div.innerHTML = '<div class="player-avatar">' + (p.avatar || '🤖') + '</div>'
            + '<div class="player-name">' + p.name + '</div>'
            + '<div class="player-stack" id="ai-stack-' + p.index + '">$' + p.stack + '</div>'
            + '<div class="player-cards" id="ai-cards-' + p.index + '"><div class="card card-back small"></div><div class="card card-back small"></div></div>'
            + '<div class="player-bet" id="ai-bet-' + p.index + '" style="display:none"></div>'
            + '<div class="player-last-action" id="ai-action-' + p.index + '"></div>';
        container.appendChild(div);
    });
}

function renderCommunityCards() {
    if (!game) return;
    const area = document.getElementById('communityArea');
    area.innerHTML = '';
    const cards = game.communityCards;
    for (let i = 0; i < 5; i++) {
        if (i < cards.length) {
            const c = cards[i];
            const div = document.createElement('div');
            div.className = 'card ' + c.color;
            div.innerHTML = '<span class="card-rank">' + c.rank + '</span><span class="card-suit">' + c.symbol + '</span>';
            area.appendChild(div);
        } else {
            const ph = ['翻牌','翻牌','翻牌','转牌','河牌'];
            const div = document.createElement('div');
            div.className = 'community-placeholder';
            div.textContent = ph[i];
            area.appendChild(div);
        }
    }
}

function renderHumanCards() {
    if (!game || !game.humanPlayer) return;
    const cards = game.humanPlayer.cards;
    for (let i = 0; i < 2; i++) {
        const el = document.getElementById('humanCard' + (i+1));
        if (!el) continue;
        if (cards && cards[i]) {
            const c = cards[i];
            el.className = 'card ' + c.color;
            el.innerHTML = '<span class="card-rank">' + c.rank + '</span><span class="card-suit">' + c.symbol + '</span>';
        } else {
            el.className = 'card card-back';
            el.innerHTML = '';
        }
    }
}

function updateUI() {
    if (!game) return;
    updateAIInfo();
    updateHumanInfo();
    updateActivePlayer();
    updatePhaseLabel();
    updateProbabilities();
    updateActionButtons();
}

function updateAIInfo() {
    for (const p of game.players) {
        if (p.isHuman) continue;
        const stackEl = document.getElementById('ai-stack-' + p.index);
        if (stackEl) stackEl.textContent = '$' + Math.max(0, p.stack);
        const betEl = document.getElementById('ai-bet-' + p.index);
        if (betEl) {
            const bet = game.roundBets[p.index] || 0;
            betEl.style.display = bet > 0 ? 'block' : 'none';
            if (bet > 0) betEl.textContent = '$' + bet;
        }
        const seat = document.getElementById('ai-seat-' + p.index);
        if (seat) {
            seat.className = 'player-seat';
            if (p.folded) seat.classList.add('folded');
            if (p.isAllIn) seat.classList.add('all-in');
        }
        const actionEl = document.getElementById('ai-action-' + p.index);
        if (actionEl && p.lastAction) {
            actionEl.textContent = getActionText(p.lastAction);
            actionEl.className = 'player-last-action action-' + p.lastAction;
        }
    }
}

function updateHumanInfo() {
    if (!game.humanPlayer) return;
    const p = game.humanPlayer;
    document.getElementById('humanStack').textContent = '$' + Math.max(0, p.stack);
    document.getElementById('humanRoundBet').textContent = '$' + (game.roundBets[0] || 0);
    const area = document.getElementById('humanArea');
    if (area) area.className = 'human-area' + (game.currentPlayerIndex === 0 && !p.folded ? ' active' : '');
    if (p.stack <= 0) document.getElementById('bustedInfo').style.display = 'block';
}

function updateActivePlayer() {
    for (const p of game.players) {
        const seat = document.getElementById('ai-seat-' + p.index);
        if (!seat) continue;
        seat.classList.toggle('active', game.currentPlayerIndex === p.index && !p.folded && game.phase !== 'idle');
    }
}

function updatePhaseLabel() {
    const el = document.getElementById('bettingRoundLabel');
    if (!el) return;
    const labels = { preflop: '翻牌前', flop: '翻牌', turn: '转牌', river: '河牌', showdown: '摊牌', idle: '' };
    el.textContent = labels[game.bettingRound] || '';
}

function updatePotDisplay() {
    const el = document.getElementById('potDisplay');
    if (el) el.innerHTML = '底池: <strong>$' + game.pot + '</strong>';
}

function updateBlinds() {
    document.getElementById('sbDisplay').textContent = game ? game.smallBlind : 40;
    document.getElementById('bbDisplay').textContent = game ? game.bigBlind : 80;
}

function updateHandCount() {
    const el = document.getElementById('handCount');
    if (el && game) el.textContent = game.numHands;
}

function updateProbabilities() {
    if (!game || !game.humanPlayer || game.humanPlayer.cards.length < 2) return;
    const prob = calculateProbabilities(game.humanPlayer.cards, game.communityCards, game.players.filter(p => !p.folded).length, game.isShortDeck);
    document.getElementById('probWinBar').style.width = prob.winRate + '%';
    document.getElementById('probWinValue').textContent = prob.winRate.toFixed(1) + '%';
    document.getElementById('probTieBar').style.width = prob.tieRate + '%';
    document.getElementById('probTieValue').textContent = prob.tieRate.toFixed(1) + '%';
    document.getElementById('probLoseBar').style.width = prob.loseRate + '%';
    document.getElementById('probLoseValue').textContent = prob.loseRate.toFixed(1) + '%';
}

function updateActionButtons() {
    const isTurn = game.currentPlayerIndex === 0 && !game.humanPlayer.folded && !game.humanPlayer.isAllIn && game.phase !== 'idle';
    ['btnFold','btnCheck','btnCall','btnRaise','btnAllin'].forEach(id => document.getElementById(id).disabled = true);
    if (!isTurn || playMode === 'online') return;
    const actions = game.getAvailableActions(0);
    document.getElementById('btnFold').disabled = false;
    document.getElementById('btnCheck').disabled = !actions.includes('check');
    document.getElementById('btnCall').disabled = !actions.includes('call');
    const toCall = Math.max(0, game.currentBet - (game.roundBets[0] || 0));
    if (actions.includes('call')) document.getElementById('btnCall').textContent = '跟注 $' + toCall;
    document.getElementById('btnRaise').disabled = !actions.includes('raise');
    document.getElementById('btnAllin').disabled = !actions.includes('allin');
    if (actions.includes('raise')) {
        const slider = document.getElementById('raiseRange');
        slider.min = Math.max(game.minRaise, toCall * 2);
        slider.max = game.humanPlayer.stack;
        slider.value = Math.min(parseInt(slider.min) * 3, game.humanPlayer.stack);
        updateRaiseDisplay();
    }
}

function updateRaiseDisplay() {
    const slider = document.getElementById('raiseRange');
    const display = document.getElementById('raiseAmountDisplay');
    if (slider && display) display.textContent = '加注到 $' + slider.value;
}

function showRaiseSlider() { document.getElementById('raiseSlider').classList.add('show'); }
function hideRaiseSlider() { document.getElementById('raiseSlider').classList.remove('show'); }

function confirmRaise() {
    const amount = parseInt(document.getElementById('raiseRange').value) || 0;
    hideRaiseSlider();
    doAction('raise', amount);
}

function doAction(action, amount) {
    if (playMode === 'online') { window.doAction(action); return; }
    if (!game || game.currentPlayerIndex !== 0) return;
    const toCall = Math.max(0, game.currentBet - (game.roundBets[0] || 0));
    if (action === 'call' && toCall === 0) action = 'check';
    game.doAction(0, action, amount || 0);
    renderCommunityCards(); renderHumanCards(); updateUI(); updatePotDisplay(); updateHandCount();
    if (game.phase === 'showdown' || game.phase === 'idle') {
        setTimeout(() => showShowdownModal(), 500);
    } else if (game.currentPlayerIndex !== 0 && game.phase !== 'idle') {
        scheduleAIActions();
    }
}

function scheduleAIActions() { setTimeout(() => processAIAction(), 800 + Math.random() * 500); }

function processAIAction() {
    if (!game || game.phase === 'idle' || game.phase === 'showdown') return;
    if (game.currentPlayerIndex === 0) { updateActionButtons(); return; }
    const idx = game.currentPlayerIndex;
    const player = game.players[idx];
    if (!player || player.folded || player.isAllIn) { game.advanceGame(); renderUI(); return; }
    setTimeout(() => {
        if (!game || game.phase === 'idle') return;
        const decision = game.getAIDecision(idx);
        game.doAction(idx, decision.action, decision.amount);
        renderUI();
        if (game.phase === 'showdown' || game.phase === 'idle') {
            setTimeout(() => showShowdownModal(), 500);
        } else if (game.currentPlayerIndex !== 0 && game.phase !== 'idle') {
            scheduleAIActions();
        } else {
            updateActionButtons();
        }
    }, 500 + Math.random() * 300);
}

function renderUI() { renderCommunityCards(); renderHumanCards(); updateUI(); updatePotDisplay(); updateHandCount(); }

function getActionText(action) {
    return ({ fold: '弃牌', check: '过牌', call: '跟注', raise: '加注', allin: '全下', sb: '小盲', bb: '大盲' })[action] || action;
}

function showShowdownModal() {
    if (!game || !game.lastHandResult) return;
    const result = game.lastHandResult;
    const modal = document.getElementById('showdownModal');
    const winner = result.winners[0];
    document.getElementById('winnerName').textContent = winner.name;
    document.getElementById('winnerAmount').textContent = '$' + result.pot;
    document.getElementById('winningHandName').textContent = winner.hand ? winner.hand.name : '其他玩家全部弃牌';
    document.getElementById('modalResults').innerHTML = game.players.filter(p => !p.folded).map(p => {
        const w = result.winners.find(w => w.name === p.name);
        const handInfo = p.isHuman ? getBestHand(p.cards, game.communityCards) : null;
        return '<div class="modal-result-row"><span>' + p.name + '</span>'
            + '<span style="color:' + (w ? '#4caf50' : '#aaa') + '">' + (w ? '🏆 +' + Math.floor(result.pot / result.winners.length) : '') + '</span>'
            + (handInfo ? '<span class="hand-name">' + handInfo.name + '</span>' : '') + '</div>';
    }).join('');
    modal.classList.add('show');
}

function closeModalAndContinue() {
    document.getElementById('showdownModal').classList.remove('show');
    if (game && game.phase === 'idle') {
        game.startNewHand();
        if (game.phase !== 'idle') {
            renderUI(); updateBlinds();
            if (game.currentPlayerIndex !== 0) scheduleAIActions();
        } else {
            startScreen.style.display = 'flex'; bottomPanel.style.display = 'none'; gameStarted = false;
        }
    }
}

function goHome() {
    document.getElementById('showdownModal').classList.remove('show');
    startScreen.style.display = 'flex'; bottomPanel.style.display = 'none'; gameStarted = false; game = null;
}

function doRefresh() { if (game) { renderUI(); updateBlinds(); } }

function toggleFullscreen() {
    document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen();
}

function toggleRules() { document.getElementById('rulesContent').classList.toggle('open'); }

function toggleProbPanel() {
    const content = document.getElementById('probContent');
    const icon = document.getElementById('probToggleIcon');
    content.classList.toggle('collapsed');
    if (icon) icon.textContent = content.classList.contains('collapsed') ? '▶' : '▼';
}

// Keyboard shortcuts
document.addEventListener('keydown', (e) => {
    if (!game || !gameStarted) return;
    const key = e.key.toLowerCase();
    const toCall = Math.max(0, game.currentBet - (game.roundBets[0] || 0));
    if (key === 'f') { e.preventDefault(); doAction('fold'); }
    else if (key === 'c') { e.preventDefault(); if (toCall > 0) doAction('call'); else doAction('check'); }
    else if (key === 'r') { e.preventDefault(); showRaiseSlider(); }
    else if (key === 'a') { e.preventDefault(); doAction('allin'); }
});

// Draggable modal
let dragActive = false;
document.addEventListener('mousedown', (e) => {
    const handle = e.target.closest('.modal-drag-handle');
    if (!handle) return;
    const modal = document.getElementById('showdownModal');
    const content = document.getElementById('modalContent');
    if (!modal.classList.contains('show')) return;
    dragActive = true;
    const rect = content.getBoundingClientRect();
    content._offX = e.clientX - rect.left;
    content._offY = e.clientY - rect.top;
});
document.addEventListener('mousemove', (e) => {
    if (!dragActive) return;
    const content = document.getElementById('modalContent');
    content.style.left = (e.clientX - content._offX) + 'px';
    content.style.top = (e.clientY - content._offY) + 'px';
    content.style.margin = '0';
});
document.addEventListener('mouseup', () => { dragActive = false; });

document.addEventListener('click', (e) => {
    const btn = e.target.closest('.count-btn');
    if (btn) {
        document.querySelectorAll('.count-btn').forEach(b => b.classList.remove('selected'));
        btn.classList.add('selected');
    }
});