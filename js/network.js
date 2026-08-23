/**
 * Reliable WebSocket client for the authoritative room server.
 * Old socket events, stale hand states and duplicate actions are rejected.
 */

const NETWORK_SESSION_KEY = 'texas-holdem-network-session-v2';

let network = {
    ws: null,
    connected: false,
    inRoom: false,
    roomCode: null,
    playerId: null,
    seatId: null,
    isHost: false,
    hostId: null,
    serverUrl: null,
    gameState: null,
    resumeToken: null,
    connectionGeneration: 0,
    requestSequence: 0,
    lastStateVersion: -1,
    lastHandId: 0,
    pendingAction: null,
    reconnectTimer: null,
    reconnectAttempts: 0,
    shouldReconnect: false
};

function loadNetworkSession() {
    try {
        const saved = JSON.parse(sessionStorage.getItem(NETWORK_SESSION_KEY) || 'null');
        if (!saved || !saved.resumeToken || !saved.roomCode || !saved.serverUrl) return;
        network.resumeToken = saved.resumeToken;
        network.roomCode = saved.roomCode;
        network.playerId = saved.playerId || null;
        network.serverUrl = saved.serverUrl;
    } catch (_) {}
}

function persistNetworkSession() {
    try {
        if (!network.resumeToken || !network.roomCode) {
            sessionStorage.removeItem(NETWORK_SESSION_KEY);
            return;
        }
        sessionStorage.setItem(NETWORK_SESSION_KEY, JSON.stringify({
            resumeToken: network.resumeToken,
            roomCode: network.roomCode,
            playerId: network.playerId,
            serverUrl: network.serverUrl
        }));
    } catch (_) {}
}

function clearNetworkSession() {
    network.resumeToken = null;
    try { sessionStorage.removeItem(NETWORK_SESSION_KEY); } catch (_) {}
}

loadNetworkSession();

function nextRequestId(prefix = 'req') {
    network.requestSequence++;
    return `${prefix}-${Date.now().toString(36)}-${network.requestSequence.toString(36)}`;
}

function isCurrentSocket(socket, generation) {
    return network.ws === socket && network.connectionGeneration === generation;
}

function connectToServer(url, options = {}) {
    const target = String(url || network.serverUrl || 'ws://localhost:3000').trim();
    if (!/^wss?:\/\//i.test(target)) {
        showNetworkError('服务器地址必须以 ws:// 或 wss:// 开头');
        return;
    }

    if (network.reconnectTimer) clearTimeout(network.reconnectTimer);
    network.reconnectTimer = null;
    if (!options.reconnecting) network.reconnectAttempts = 0;
    const previous = network.ws;
    const generation = ++network.connectionGeneration;
    if (previous) {
        previous.onopen = previous.onclose = previous.onmessage = previous.onerror = null;
        try { previous.close(); } catch (_) {}
    }

    network.serverUrl = target;
    network.shouldReconnect = options.autoReconnect !== false;
    network.connected = false;
    let socket;
    try { socket = new WebSocket(target); }
    catch (error) { showNetworkError('无法连接: ' + error.message); return; }
    network.ws = socket;

    socket.onopen = () => {
        if (!isCurrentSocket(socket, generation)) return;
        network.connected = true;
        onConnected({ reconnecting: !!options.reconnecting });
        if (options.resume !== false && network.resumeToken && network.roomCode) {
            send({
                type: 'resume_session',
                requestId: nextRequestId('resume'),
                roomCode: network.roomCode,
                resumeToken: network.resumeToken
            });
        }
    };

    socket.onclose = event => {
        if (!isCurrentSocket(socket, generation)) return;
        network.connected = false;
        network.ws = null;
        const canResume = network.shouldReconnect && !!network.resumeToken && !!network.roomCode && event.code !== 1000;
        const reconnectDelay = canResume
            ? Math.min(10000, 650 * (2 ** Math.min(network.reconnectAttempts, 4))) + Math.floor(Math.random() * 350)
            : 0;
        onDisconnected({ willReconnect: canResume, code: event.code, reconnectDelay });
        if (canResume) {
            network.reconnectAttempts++;
            network.reconnectTimer = setTimeout(() => {
                network.reconnectTimer = null;
                connectToServer(network.serverUrl, { reconnecting: true, resume: true, autoReconnect: true });
            }, reconnectDelay);
        }
    };

    socket.onmessage = event => {
        if (!isCurrentSocket(socket, generation)) return;
        try { handleMessage(JSON.parse(event.data)); }
        catch (error) { console.error('Network message error:', error); }
    };

    socket.onerror = () => {
        if (isCurrentSocket(socket, generation)) showNetworkError('连接错误，正在检查本地服务...');
    };
}

function disconnect(options = {}) {
    network.shouldReconnect = false;
    if (network.reconnectTimer) clearTimeout(network.reconnectTimer);
    network.reconnectTimer = null;
    network.reconnectAttempts = 0;
    const socket = network.ws;
    ++network.connectionGeneration;
    network.ws = null;
    network.connected = false;
    network.pendingAction = null;
    if (socket) {
        socket.onopen = socket.onclose = socket.onmessage = socket.onerror = null;
        try { socket.close(1000, 'client disconnect'); } catch (_) {}
    }
    if (!options.preserveSession) {
        network.inRoom = false;
        network.roomCode = null;
        network.playerId = null;
        network.seatId = null;
        network.isHost = false;
        network.hostId = null;
        network.gameState = null;
        network.lastStateVersion = -1;
        network.lastHandId = 0;
        clearNetworkSession();
    }
}

function reconnectToServer(url) {
    const target = url || network.serverUrl;
    disconnect({ preserveSession: true });
    setTimeout(() => connectToServer(target, { reconnecting: true, resume: true, autoReconnect: true }), 150);
}

function roomOptions(options = {}) {
    return {
        maxPlayers: options.maxPlayers || 6,
        isShortDeck: !!options.isShortDeck,
        startingStack: Math.max(1, Math.floor(Number(options.startingStack) || 20000)),
        smallBlind: Math.max(1, Math.floor(Number(options.smallBlind) || 40)),
        bigBlind: Math.max(1, Math.floor(Number(options.bigBlind) || 80)),
        ante: Math.max(1, Math.floor(Number(options.ante) || 40)),
        minBet: Math.max(1, Math.floor(Number(options.minBet) || 80))
    };
}

function createRoom(options = {}) {
    if (!network.connected) return showNetworkError('未连接到服务器');
    send({ type:'create_room', requestId:nextRequestId('create'), name:options.name || '玩家', ...roomOptions(options) });
}

function joinRoom(roomCode, playerName, options = {}) {
    if (!network.connected) return showNetworkError('未连接到服务器');
    send({
        type: 'join_room',
        requestId: nextRequestId('join'),
        roomCode: String(roomCode || ''),
        name: playerName || '玩家',
        ...roomOptions(options)
    });
}

function leaveRoom() {
    if (network.inRoom) send({
        type: 'leave_room', requestId: nextRequestId('leave'), roomCode: network.roomCode
    });
    network.inRoom = false;
    network.roomCode = null;
    network.playerId = null;
    network.seatId = null;
    network.isHost = false;
    network.hostId = null;
    network.gameState = null;
    network.lastStateVersion = -1;
    network.lastHandId = 0;
    network.pendingAction = null;
    clearNetworkSession();
}

function startGame() {
    send({ type:'start_game', requestId:nextRequestId('start'), roomCode:network.roomCode });
}

function sendAction(action, amount) {
    const state = network.gameState;
    if (!network.connected || !network.inRoom || !state?.isYourTurn || network.pendingAction) return null;
    const requestId = nextRequestId('action');
    network.pendingAction = { requestId, expectedStateVersion: state.stateVersion };
    send({
        type: 'player_action', requestId, roomCode: network.roomCode,
        handId: state.handId, turnId: state.turnId,
        expectedStateVersion: state.stateVersion,
        action, amount: Math.max(0, Math.floor(Number(amount) || 0))
    });
    return requestId;
}

function sendReadyForNext() {
    const state = network.gameState;
    if (!state) return null;
    const requestId = nextRequestId('ready');
    send({ type:'ready_for_next', requestId, roomCode:network.roomCode, handId:state.handId });
    return requestId;
}

function send(data) {
    if (network.ws?.readyState === WebSocket.OPEN) {
        network.ws.send(JSON.stringify({ protocolVersion: 2, ...data }));
        return true;
    }
    return false;
}

function isWrongRoom(msg) {
    return !!msg.roomCode && !!network.roomCode && msg.roomCode !== network.roomCode;
}

function handleMessage(msg) {
    if (!msg || typeof msg.type !== 'string') return;
    if (isWrongRoom(msg) && !['room_created','room_joined'].includes(msg.type)) return;

    switch (msg.type) {
        case 'hello':
            break;
        case 'room_created':
        case 'room_joined': {
            network.reconnectAttempts = 0;
            network.roomCode = String(msg.roomCode);
            network.playerId = msg.playerId;
            network.seatId = msg.seatId;
            network.resumeToken = msg.resumeToken || network.resumeToken;
            network.hostId = msg.hostId || null;
            network.isHost = typeof msg.isHost === 'boolean' ? msg.isHost : network.hostId === network.playerId;
            network.inRoom = true;
            network.lastStateVersion = -1;
            persistNetworkSession();
            if (msg.type === 'room_created') onRoomCreated(msg);
            else onRoomJoined(msg);
            break;
        }
        case 'player_joined':
        case 'player_left':
            if (!network.inRoom) return;
            if (msg.hostId) {
                network.hostId = msg.hostId;
                network.isHost = msg.hostId === network.playerId;
            }
            onRoomPlayersChanged(msg);
            break;
        case 'host_changed':
            if (!network.inRoom) return;
            network.hostId = msg.hostId || null;
            network.isHost = !!msg.isHost;
            onHostChanged(msg);
            onRoomPlayersChanged(msg);
            break;
        case 'game_starting':
            if (!network.inRoom) return;
            onGameStarting(msg);
            break;
        case 'game_state': {
            if (!network.inRoom) return;
            network.reconnectAttempts = 0;
            const version = Number(msg.stateVersion);
            const handId = Number(msg.handId);
            if (Number.isFinite(version) && version <= network.lastStateVersion && network.gameState) return;
            if (Number.isFinite(handId) && handId < network.lastHandId) return;
            network.lastStateVersion = Number.isFinite(version) ? version : network.lastStateVersion;
            network.lastHandId = Number.isFinite(handId) ? handId : network.lastHandId;
            network.hostId = msg.hostId || network.hostId;
            network.isHost = typeof msg.isHost === 'boolean' ? msg.isHost : network.isHost;
            network.gameState = msg;
            if (network.pendingAction && version > network.pendingAction.expectedStateVersion) network.pendingAction = null;
            onGameState(msg);
            break;
        }
        case 'action_ack':
            if (network.pendingAction?.requestId === msg.requestId) network.pendingAction = null;
            onActionAck(msg);
            if (!msg.accepted) showNetworkError(msg.message || '行动未被服务器接受');
            break;
        case 'ack':
            onRequestAck(msg);
            break;
        case 'player_disconnected':
            onPlayerDisconnected(msg);
            break;
        case 'ready_status':
            onReadyStatus(msg);
            break;
        case 'error':
            if (network.pendingAction?.requestId === msg.requestId) network.pendingAction = null;
            if (msg.code === 'RESUME_EXPIRED') {
                network.inRoom = false;
                network.roomCode = null;
                clearNetworkSession();
            }
            showNetworkError(msg.message || '服务器请求失败');
            onRequestError(msg);
            break;
    }
}

var onConnected = function() {};
var onDisconnected = function() {};
var onRoomCreated = function() {};
var onRoomJoined = function() {};
var onRoomPlayersChanged = function() {};
var onHostChanged = function() {};
var onGameStarting = function() {};
var onGameState = function() {};
var onPlayerDisconnected = function() {};
var onReadyStatus = function() {};
var onActionAck = function() {};
var onRequestAck = function() {};
var onRequestError = function() {};
var showNetworkError = function(message) { console.error('Network:', message); };

window.network = network;
window.connectToServer = connectToServer;
window.reconnectToServer = reconnectToServer;
window.disconnect = disconnect;
window.createRoom = createRoom;
window.joinRoom = joinRoom;
window.leaveRoom = leaveRoom;
window.startGame = startGame;
window.sendAction = sendAction;
window.sendReadyForNext = sendReadyForNext;
