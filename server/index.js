/**
 * 德州扑克服务器 — WebSocket 入口
 */
const { WebSocketServer } = require('ws');
const { RoomManager } = require('./room');
const { GameEngine } = require('./game-engine');
const log = require('./logger');

const PORT = parseInt(process.argv[2]) || 3000;
const wss = new WebSocketServer({ port: PORT });
const rooms = new RoomManager();

console.log('🃏 德州扑克服务器启动 | ws://0.0.0.0:' + PORT);
log.summary();

wss.on('connection', (ws, req) => {
    const ip = req.socket.remoteAddress;
    console.log('[连接] ' + ip);
    ws.playerInfo = null;

    ws.on('message', (raw) => {
        let msg;
        try { msg = JSON.parse(raw.toString()); } catch (e) { return; }
        try {
            handleMessage(ws, msg);
        } catch (e) {
            const roomCode = ws.playerInfo ? ws.playerInfo.room : '?';
            log.error(roomCode, '消息处理异常', e);
            send(ws, { type: 'error', message: e.message });
        }
    });

    ws.on('close', () => handleDisconnect(ws));
    ws.on('error', (e) => {
        const roomCode = ws.playerInfo ? ws.playerInfo.room : '?';
        log.error(roomCode, 'WebSocket错误', e);
    });
});

function handleMessage(ws, msg) {
    switch (msg.type) {
        case 'create_room': return handleCreateRoom(ws, msg);
        case 'join_room': return handleJoinRoom(ws, msg);
        case 'leave_room': return handleLeaveRoom(ws);
        case 'player_action': return handlePlayerAction(ws, msg);
        case 'start_game': return handleStartGame(ws, msg);
        default: send(ws, { type: 'error', message: '未知消息类型' });
    }
}

function handleCreateRoom(ws, msg) {
    if (ws.playerInfo) throw new Error('已在房间中');
    const playerName = (msg.name || '玩家').slice(0, 12);
    const maxPlayers = Math.max(1, Math.min(10, parseInt(msg.maxPlayers) || 6));
    const isShortDeck = !!msg.isShortDeck;
    const useEliteAI = !!msg.eliteAI;
    const room = rooms.createRoom({ maxPlayers, isShortDeck, useEliteAI });
    const player = room.addPlayer(ws, playerName);
    ws.playerInfo = { id: player.id, name: playerName, room: room.code };
    send(ws, { type: 'room_created', roomCode: room.code, playerId: player.id, playerName: playerName, maxPlayers: maxPlayers, isShortDeck: isShortDeck });
    broadcastRoom(room, { type: 'player_joined', players: room.getPlayerInfo() });
}

function handleJoinRoom(ws, msg) {
    if (ws.playerInfo) throw new Error('已在房间中');
    const code = (msg.roomCode || '').toUpperCase().trim();
    const room = rooms.getRoom(code);
    if (!room) throw new Error('房间不存在');
    if (room.isPlaying()) throw new Error('游戏已开始');
    const playerName = (msg.name || '玩家').slice(0, 12);
    const player = room.addPlayer(ws, playerName);
    ws.playerInfo = { id: player.id, name: playerName, room: room.code };
    send(ws, { type: 'room_joined', roomCode: room.code, playerId: player.id, playerName: playerName, players: room.getPlayerInfo() });
    broadcastRoom(room, { type: 'player_joined', players: room.getPlayerInfo() });
}

function handleLeaveRoom(ws) {
    if (!ws.playerInfo) return;
    const room = rooms.getRoom(ws.playerInfo.room);
    if (!room) return;
    room.removePlayer(ws.playerInfo.id);
    broadcastRoom(room, { type: 'player_left', players: room.getPlayerInfo() });
    ws.playerInfo = null;
    if (room.isEmpty()) rooms.deleteRoom(room.code);
}

function handleStartGame(ws, msg) {
    if (!ws.playerInfo) throw new Error('未在房间中');
    const room = rooms.getRoom(ws.playerInfo.room);
    if (!room) throw new Error('房间不存在');
    const player = room.getPlayer(ws.playerInfo.id);
    if (!player || player.id !== room.players[0].id) throw new Error('只有房主可以开始游戏');
    if (room.getHumanPlayers().length < 1) throw new Error('至少需要1名真人玩家');
    room.startGame();
    broadcastRoom(room, { type: 'game_starting', playerCount: room.players.length, humanCount: room.getHumanPlayers().length, aiCount: room.players.length - room.getHumanPlayers().length });
    dealNextHand(room);
    scheduleNextAI(room, 1000 + Math.random() * 500);
}

function handlePlayerAction(ws, msg) {
    if (!ws.playerInfo) throw new Error('未在房间中');
    const room = rooms.getRoom(ws.playerInfo.room);
    if (!room || !room.engine) throw new Error('游戏未开始');
    const result = room.engine.handleAction(ws.playerInfo.id, msg.action, msg.amount || 0);
    if (result.error) { send(ws, { type: 'error', message: result.error }); return; }
    broadcastGameState(room);
    if (result.handEnded) {
        setTimeout(() => { if (room.engine) { dealNextHand(room); scheduleNextAI(room, 1500); } }, 3000);
    } else {
        scheduleNextAI(room, 1000 + Math.random() * 500);
    }
}

function dealNextHand(room) {
    room.engine.startNewHand();
    broadcastGameState(room);
}

function broadcastGameState(room) {
    if (!room.engine) return;
    const state = room.engine.getPublicState();
    for (const p of room.players) {
        if (!p.ws || p.ws.readyState !== 1) continue;
        const privateState = room.engine.getPlayerState(p.id);
        send(p.ws, { type: 'game_state', ...state, yourCards: privateState.cards, yourStack: privateState.stack, yourBet: privateState.bet, isYourTurn: privateState.isYourTurn, availableActions: privateState.availableActions });
    }
}

function scheduleNextAI(room, delay) {
    if (!room.engine) return;
    setTimeout(() => {
        if (!room.engine) return;
        const eng = room.engine;
        if (eng.phase === 'idle' || eng.phase === 'showdown') return;
        const current = eng.players[eng.currentPlayerIdx];
        if (!current || current.folded || current.isAllIn || current.isHuman) return;
        const aiDecision = eng.getAIDecision(eng.currentPlayerIdx);
        eng.executeAction(eng.currentPlayerIdx, aiDecision.action, aiDecision.amount);
        eng.advanceGame();
        if (eng.phase === 'showdown') {
            eng.doShowdown();
            broadcastGameState(room);
            setTimeout(() => { if (room.engine) { dealNextHand(room); scheduleNextAI(room, 1500); } }, 3000);
            return;
        }
        if (eng.phase === 'idle') {
            broadcastGameState(room);
            setTimeout(() => { if (room.engine) { dealNextHand(room); scheduleNextAI(room, 1500); } }, 3000);
            return;
        }
        broadcastGameState(room);
        const next = eng.players[eng.currentPlayerIdx];
        if (next && !next.folded && !next.isAllIn && !next.isHuman) {
            scheduleNextAI(room, 2000 + Math.random() * 1000);
        }
    }, delay);
}

function handleDisconnect(ws) {
    if (!ws.playerInfo) return;
    const room = rooms.getRoom(ws.playerInfo.room);
    if (!room) return;
    room.markDisconnected(ws.playerInfo.id);
    broadcastRoom(room, { type: 'player_disconnected', playerId: ws.playerInfo.id, players: room.getPlayerInfo() });
    if (room.isEmpty()) rooms.deleteRoom(room.code);
}

function send(ws, data) {
    if (ws.readyState === 1) ws.send(JSON.stringify(data));
}

function broadcastRoom(room, data) {
    const str = JSON.stringify(data);
    for (const p of room.players) {
        if (p.ws && p.ws.readyState === 1) p.ws.send(str);
    }
}