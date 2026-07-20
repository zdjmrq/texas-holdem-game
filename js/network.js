/**
 * Network Module — WebSocket client for online multiplayer
 */

let ws = null;
let onlinePlayerId = null;
let onlinePlayerName = '';

function doConnect() {
    const addr = document.getElementById('serverAddress').value.trim() || 'ws://localhost:3000';
    const name = document.getElementById('playerNameInput').value.trim() || '玩家' + Math.floor(Math.random()*1000);
    onlinePlayerName = name;
    try {
        ws = new WebSocket(addr);
        ws.onopen = () => {
            setStatus('已连接', 'green');
            document.getElementById('connectPanel').style.display = 'none';
            document.getElementById('roomLobby').style.display = 'block';
        };
        ws.onmessage = (event) => {
            const msg = JSON.parse(event.data);
            handleServerMessage(msg);
        };
        ws.onclose = () => {
            setStatus('连接断开', 'red');
            ws = null;
            document.getElementById('connectPanel').style.display = 'block';
            document.getElementById('roomLobby').style.display = 'none';
        };
        ws.onerror = () => setStatus('连接失败', 'red');
    } catch (e) {
        setStatus('连接错误: ' + e.message, 'red');
    }
}

function doDisconnect() {
    if (ws) { ws.close(); ws = null; }
    document.getElementById('connectPanel').style.display = 'block';
    document.getElementById('roomLobby').style.display = 'none';
}

function doCreateRoom() {
    if (!ws) return;
    const maxPlayers = parseInt(document.getElementById('roomMaxPlayers').value) || 4;
    const isShortDeck = document.getElementById('roomShortDeck').checked;
    const eliteAI = document.getElementById('roomEliteAI').checked;
    ws.send(JSON.stringify({ type: 'create_room', name: onlinePlayerName, maxPlayers, isShortDeck, eliteAI }));
}

function doJoinRoom() {
    if (!ws) return;
    const code = document.getElementById('joinRoomCode').value.trim().toUpperCase();
    if (code.length < 3) { setStatus('请输入有效的房间号', 'red'); return; }
    ws.send(JSON.stringify({ type: 'join_room', roomCode: code, name: onlinePlayerName }));
}

function doLeaveRoom() {
    if (!ws) return;
    ws.send(JSON.stringify({ type: 'leave_room' }));
}

function doStartGame() {
    if (!ws) return;
    ws.send(JSON.stringify({ type: 'start_game' }));
}

function handleServerMessage(msg) {
    switch (msg.type) {
        case 'room_created':
            onlinePlayerId = msg.playerId;
            document.getElementById('roomCodeDisplay').textContent = msg.roomCode;
            document.getElementById('createRoomForm').style.display = 'none';
            document.getElementById('joinRoomForm').style.display = 'none';
            document.getElementById('roomActions').style.display = 'block';
            document.getElementById('startGameBtnOnline').style.display = 'block';
            document.getElementById('waitingMsg').style.display = 'none';
            isRoomHost = true; inRoom = true;
            updatePlayerList(msg.players);
            setStatus('房间 ' + msg.roomCode + ' 已创建', 'green');
            break;
        case 'room_joined':
            onlinePlayerId = msg.playerId;
            document.getElementById('roomCodeDisplay').textContent = msg.roomCode;
            document.getElementById('createRoomForm').style.display = 'none';
            document.getElementById('joinRoomForm').style.display = 'none';
            document.getElementById('roomActions').style.display = 'block';
            document.getElementById('startGameBtnOnline').style.display = 'none';
            document.getElementById('waitingMsg').style.display = 'block';
            isRoomHost = false; inRoom = true;
            updatePlayerList(msg.players);
            setStatus('已加入房间 ' + msg.roomCode, 'green');
            break;
        case 'player_joined':
        case 'player_left':
            updatePlayerList(msg.players);
            break;
        case 'game_starting':
            setStatus('游戏开始！' + msg.playerCount + '人(' + msg.humanCount + '真人+' + msg.aiCount + 'AI)', 'gold');
            break;
        case 'game_state':
            handleOnlineGameState(msg);
            break;
        case 'error':
            setStatus('错误: ' + msg.message, 'red');
            break;
    }
}

function updatePlayerList(players) {
    const list = document.getElementById('playerList');
    if (!players) { list.innerHTML = '<div style="color:#888;">暂无玩家</div>'; return; }
    list.innerHTML = players.map(p =>
        '<div style="display:flex;align-items:center;gap:6px;padding:4px 0;">' +
        '<span>' + (p.isHuman ? '👤' : '🤖') + '</span>' +
        '<span>' + p.name + '</span>' +
        '<span style="color:#888;font-size:11px;">' + (p.isHuman ? '(真人)' : '(AI)') + '</span>' +
        (p.id === onlinePlayerId ? '<span style="color:#ffd700;font-size:11px;">(你)</span>' : '') +
        '</div>'
    ).join('');
}

function handleOnlineGameState(state) {
    if (!game) { initOnlineGame(state); }
    updateOnlineGameState(state);
}

function initOnlineGame(state) {
    startScreen.style.display = 'none';
    bottomPanel.style.display = 'flex';
    gameStarted = true;
}

function updateOnlineGameState(state) {
    if (!game) return;
    game.pot = state.pot || 0;
    game.currentBet = state.currentBet || 0;
    game.phase = state.phase || 'idle';
    game.communityCards = (state.communityCards || []).map(c => new Card(c.rank, c.suit));
    game.dealerPosition = state.dealerPosition || 0;
    if (state.yourCards) game.humanPlayer.cards = state.yourCards.map(c => new Card(c.rank, c.suit));
    if (state.yourStack !== undefined) game.humanPlayer.stack = state.yourStack;
    if (state.yourBet !== undefined) game.roundBets[0] = state.yourBet;
    renderCommunityCards();
    renderHumanCards();
    updateUI();
    updatePotDisplay();
}

function doAction(action) {
    if (!ws || !onlinePlayerId) return;
    let amount = 0;
    if (action === 'raise') amount = parseInt(document.getElementById('raiseRange').value) || 0;
    ws.send(JSON.stringify({ type: 'player_action', action, amount }));
}

function setStatus(msg, color) {
    const el = document.getElementById('gameStatus');
    if (el) el.innerHTML = '<span style="color:' + (color || '#aaa') + ';">' + msg + '</span>';
}