'use strict';

const crypto = require('crypto');
const { WebSocketServer, WebSocket } = require('ws');
const { ServerPokerGame, chooseServerAiAction, createServerAi } = require('./server-game');

const PROTOCOL_VERSION = 2;
const STACK_LEVELS = [5000, 10000, 20000, 50000, 100000];
const BLIND_LEVELS = [[10,20], [20,40], [40,80], [50,100], [100,200], [200,400]];

function token(bytes = 18) { return crypto.randomBytes(bytes).toString('base64url'); }
function playerId() { return crypto.randomUUID(); }

function boundedNumber(value, fallback, minimum, maximum, integer = false) {
    const parsed = Number(value);
    const safe = Number.isFinite(parsed) ? parsed : fallback;
    const bounded = Math.max(minimum, Math.min(maximum, safe));
    return integer ? Math.floor(bounded) : bounded;
}

function normalizeName(value) {
    const clean = String(value || '玩家').normalize('NFKC').replace(/[\u0000-\u001f\u007f]/g, '').trim();
    return [...(clean || '玩家')].slice(0, 8).join('');
}

function sanitizeConfig(input = {}) {
    const isShortDeck = !!input.isShortDeck;
    const startingCandidate = Math.floor(Number(input.startingStack) || 20000);
    const startingStack = STACK_LEVELS.includes(startingCandidate) ? startingCandidate : 20000;
    const smallCandidate = Math.floor(Number(input.smallBlind || input.ante) || 40);
    const bigCandidate = Math.floor(Number(input.bigBlind || input.minBet) || 80);
    const pair = BLIND_LEVELS.find(([small, big]) => small === smallCandidate && big === bigCandidate) || [40, 80];
    const maxLimit = isShortDeck ? 8 : 10;
    const maxPlayers = Math.max(2, Math.min(maxLimit, Math.floor(Number(input.maxPlayers) || (isShortDeck ? 6 : 10))));
    return {
        maxPlayers,
        isShortDeck,
        startingStack,
        smallBlind: pair[0],
        bigBlind: pair[1],
        ante: pair[0],
        minBet: pair[1]
    };
}

function safeSend(ws, payload) {
    if (ws?.readyState !== WebSocket.OPEN) return false;
    ws.send(JSON.stringify({ protocolVersion: PROTOCOL_VERSION, ...payload }));
    return true;
}

class PokerRoomServer {
    constructor(options = {}) {
        this.host = options.host || '127.0.0.1';
        this.port = Number.isFinite(Number(options.port)) ? Number(options.port) : 3000;
        this.aiDelay = boundedNumber(options.aiDelay, 450, 0, 10000);
        this.disconnectGraceMs = boundedNumber(options.disconnectGraceMs, 1800, 100, 60000);
        this.maxRooms = boundedNumber(options.maxRooms ?? process.env.POKER_MAX_ROOMS, 3, 1, 5, true);
        this.maxConnections = boundedNumber(options.maxConnections ?? process.env.POKER_MAX_CONNECTIONS, 24, 4, 100, true);
        this.maxConnectionsPerIp = boundedNumber(options.maxConnectionsPerIp ?? process.env.POKER_MAX_CONNECTIONS_PER_IP, 20, 2, 30, true);
        this.rateWindowMs = boundedNumber(options.rateWindowMs, 10000, 1000, 60000);
        this.maxMessagesPerWindow = boundedNumber(options.maxMessagesPerWindow, 80, 20, 500, true);
        this.trustProxy = options.trustProxy ?? process.env.POKER_TRUST_PROXY === '1';
        const configuredOrigins = options.allowedOrigins ?? process.env.POKER_ALLOWED_ORIGINS ?? '';
        this.allowedOrigins = new Set(String(configuredOrigins).split(',').map(value => value.trim()).filter(Boolean));
        this.random = options.random || Math.random;
        this.rooms = new Map();
        this.sessions = new Map();
        this.connections = new Map();
        this.wss = null;
        this.heartbeat = null;
        this.aiQueue = [];
        this.aiBusy = false;
    }

    originAllowed(origin) {
        if (!origin || origin === 'null' || origin.startsWith('file://')) return true;
        if (this.allowedOrigins.has(origin)) return true;
        try {
            const host = new URL(origin).hostname;
            return host === 'localhost' || host === '127.0.0.1' || host === '::1';
        } catch (_) { return false; }
    }

    clientIp(request) {
        if (this.trustProxy) {
            const forwarded = String(request?.headers?.['x-forwarded-for'] || '').split(',')[0].trim();
            if (forwarded) return forwarded;
        }
        return request?.socket?.remoteAddress || 'unknown';
    }

    start() {
        if (this.wss) return Promise.resolve(this);
        return new Promise((resolve, reject) => {
            const wss = new WebSocketServer({
                host: this.host,
                port: this.port,
                maxPayload:16 * 1024,
                perMessageDeflate:false,
                verifyClient:info => this.originAllowed(info.origin)
            });
            this.wss = wss;
            const onError = error => { wss.removeListener('listening', onListening); reject(error); };
            const onListening = () => {
                wss.removeListener('error', onError);
                wss.on('connection', (ws, request) => this.onConnection(ws, request));
                wss.on('error', error => console.error('[poker-server]', error.message));
                this.heartbeat = setInterval(() => this.pingConnections(), 15000);
                this.heartbeat.unref?.();
                resolve(this);
            };
            wss.once('error', onError);
            wss.once('listening', onListening);
        });
    }

    address() { return this.wss?.address(); }

    async close() {
        if (this.heartbeat) clearInterval(this.heartbeat);
        this.aiQueue.length = 0;
        this.aiBusy = false;
        for (const room of this.rooms.values()) {
            if (room.aiTimer) clearTimeout(room.aiTimer);
            if (room.cleanupTimer) clearTimeout(room.cleanupTimer);
            if (room.nextHandTimer) clearTimeout(room.nextHandTimer);
            for (const player of room.players) {
                if (player.disconnectTimer) clearTimeout(player.disconnectTimer);
                if (player.hostTimer) clearTimeout(player.hostTimer);
            }
        }
        for (const ws of this.connections.keys()) try { ws.close(); } catch (_) {}
        if (!this.wss) return;
        const wss = this.wss;
        this.wss = null;
        await new Promise(resolve => wss.close(resolve));
    }

    pingConnections() {
        for (const [ws, meta] of this.connections) {
            if (meta.alive === false) { ws.terminate(); continue; }
            meta.alive = false;
            try { ws.ping(); } catch (_) { ws.terminate(); }
        }
    }

    onConnection(ws, request) {
        const ip = this.clientIp(request);
        const fromIp = [...this.connections.values()].filter(meta => meta.ip === ip).length;
        if (this.connections.size >= this.maxConnections || fromIp >= this.maxConnectionsPerIp) {
            ws.close(1013, 'server capacity reached');
            return;
        }
        const meta = {
            alive:true,
            roomCode:null,
            playerId:null,
            sessionToken:null,
            ip,
            rateStartedAt:Date.now(),
            messageCount:0
        };
        this.connections.set(ws, meta);
        ws.on('pong', () => { meta.alive = true; });
        ws.on('message', data => {
            if (data.length > 16 * 1024) { ws.close(1009, 'message too large'); return; }
            const now = Date.now();
            if (now - meta.rateStartedAt >= this.rateWindowMs) {
                meta.rateStartedAt = now;
                meta.messageCount = 0;
            }
            meta.messageCount++;
            if (meta.messageCount > this.maxMessagesPerWindow) {
                ws.close(1008, 'message rate exceeded');
                return;
            }
            let message;
            try { message = JSON.parse(data.toString('utf8')); }
            catch (_) { this.error(ws, '消息格式无效'); return; }
            this.handle(ws, message);
        });
        ws.on('close', () => this.onDisconnect(ws, false));
        ws.on('error', () => {});
        safeSend(ws, {
            type:'hello',
            serverId:'texas-holdem-game',
            serverName:'德州扑克服务',
            protocolVersion:PROTOCOL_VERSION
        });
    }

    handle(ws, message) {
        if (!message || typeof message.type !== 'string') return this.error(ws, '缺少消息类型', message?.requestId);
        switch (message.type) {
            case 'join_room': return this.joinRoom(ws, message);
            case 'create_room': return this.createRoom(ws, message);
            case 'resume_session': return this.resumeSession(ws, message);
            case 'leave_room': return this.leaveRoom(ws, message);
            case 'start_game': return this.startGame(ws, message);
            case 'player_action': return this.playerAction(ws, message);
            case 'ready_for_next': return this.readyForNext(ws, message);
            case 'get_snapshot': return this.sendSnapshot(ws);
            default: return this.error(ws, '未知消息类型', message.requestId);
        }
    }

    createRoom(ws, message) {
        const free = ['1','2','3','4','5'].find(code => !this.rooms.has(code));
        if (!free || this.rooms.size >= this.maxRooms) return this.error(ws, `房间已满（最多 ${this.maxRooms} 个）`, message.requestId);
        this.joinRoom(ws, { ...message, type: 'join_room', roomCode: free });
    }

    joinRoom(ws, message) {
        const meta = this.connections.get(ws);
        if (meta?.roomCode) return this.error(ws, '请先离开当前房间', message.requestId);
        const code = String(message.roomCode || '').trim();
        if (!/^[1-5]$/.test(code)) return this.error(ws, '房间号只能是 1–5', message.requestId);
        let room = this.rooms.get(code);
        const created = !room;
        if (!room) {
            if (this.rooms.size >= this.maxRooms) {
                return this.error(ws, `房间已满（最多 ${this.maxRooms} 个）`, message.requestId, 'ROOM_LIMIT');
            }
            room = {
                code,
                config: sanitizeConfig(message),
                players: [],
                hostId: null,
                game: null,
                stateVersion: 0,
                ready: new Set(),
                aiTimer: null,
                cleanupTimer: null
            };
            this.rooms.set(code, room);
        }
        if (room.game) return this.error(ws, '房间已经开局，只能使用恢复令牌返回原座位', message.requestId);
        const humans = room.players.filter(player => player.isHuman && !player.left);
        if (humans.length >= room.config.maxPlayers) return this.error(ws, '房间已满', message.requestId);

        const occupied = new Set(room.players.filter(player => !player.left).map(player => player.seatId));
        let seatId = 0;
        while (occupied.has(seatId)) seatId++;
        const id = playerId();
        const resumeToken = token();
        const player = {
            id,
            seatId,
            name: normalizeName(message.name),
            avatar: '👤',
            isHuman: true,
            connected: true,
            left: false,
            stack: room.config.startingStack,
            holeCards: [], chipsInPot: 0, roundBet: 0,
            folded: false, isAllIn: false, dealtIn: false, lastAction: null,
            ws, resumeToken, processed: new Map(), joinedAt: Date.now()
        };
        room.players.push(player);
        room.players.sort((a, b) => a.seatId - b.seatId);
        if (!room.hostId) room.hostId = id;
        this.sessions.set(resumeToken, { roomCode: code, playerId: id });
        Object.assign(meta, { roomCode: code, playerId: id, sessionToken: resumeToken });
        room.stateVersion++;
        this.sendRoomJoined(player, room, created, resumeToken);
        this.broadcastLobby(room, 'player_joined');
    }

    resumeSession(ws, message) {
        const resumeToken = String(message.resumeToken || '');
        const connectionMeta = this.connections.get(ws);
        if (connectionMeta?.roomCode) return this.error(ws, '请先离开当前房间再恢复其他座位', message.requestId);
        const session = this.sessions.get(resumeToken);
        if (!session) return this.error(ws, '恢复信息已失效', message.requestId, 'RESUME_EXPIRED');
        const room = this.rooms.get(session.roomCode);
        const player = room?.players.find(item => item.id === session.playerId && !item.left);
        if (!room || !player) return this.error(ws, '原房间已关闭', message.requestId, 'RESUME_EXPIRED');

        if (player.ws && player.ws !== ws && player.ws.readyState === WebSocket.OPEN) player.ws.close(4001, 'session resumed elsewhere');
        const meta = this.connections.get(ws);
        Object.assign(meta, { roomCode: room.code, playerId: player.id, sessionToken: resumeToken });
        player.ws = ws;
        player.connected = true;
        if (room.cleanupTimer) { clearTimeout(room.cleanupTimer); room.cleanupTimer = null; }
        if (player.disconnectTimer) { clearTimeout(player.disconnectTimer); player.disconnectTimer = null; }
        if (player.hostTimer) { clearTimeout(player.hostTimer); player.hostTimer = null; }
        this.sendRoomJoined(player, room, false, resumeToken, true);
        room.stateVersion++;
        this.broadcastLobby(room, 'player_joined');
        if (room.game) {
            this.broadcastGame(room);
            if (room.game.phase === 'idle' && room.game.lastHandResult) {
                const scheduled = this.tryStartNextHand(room);
                this.broadcastReady(room, scheduled);
            }
        }
    }

    sendRoomJoined(player, room, created, resumeToken, resumed = false) {
        safeSend(player.ws, {
            type: created ? 'room_created' : 'room_joined',
            roomCode: room.code,
            playerId: player.id,
            seatId: player.seatId,
            isHost: room.hostId === player.id,
            hostId: room.hostId,
            resumeToken,
            resumed,
            roomConfig: room.config,
            players: this.publicPlayers(room)
        });
    }

    publicPlayers(room) {
        return room.players.filter(player => room.game || !player.left).sort((a, b) => a.seatId - b.seatId).map(player => ({
            id: player.id,
            seatId: player.seatId,
            name: player.name,
            avatar: player.avatar,
            isHuman: player.isHuman,
            connected: !!player.connected,
            stack: player.stack,
            chipsInPot: player.chipsInPot || 0,
            roundBet: player.roundBet || 0,
            folded: !!player.folded,
            isAllIn: !!player.isAllIn,
            lastAction: player.lastAction || null
        }));
    }

    broadcastLobby(room, type = 'player_joined') {
        const payload = {
            type,
            roomCode: room.code,
            hostId: room.hostId,
            roomConfig: room.config,
            players: this.publicPlayers(room)
        };
        for (const player of room.players) if (player.isHuman && player.connected) safeSend(player.ws, payload);
    }

    startGame(ws, message) {
        const found = this.roomPlayer(ws);
        if (!found) return this.error(ws, '尚未进入房间', message.requestId);
        const { room, player } = found;
        if (room.hostId !== player.id) return this.error(ws, '只有房主可以开始游戏', message.requestId);
        if (room.game) return this.error(ws, '游戏已经开始', message.requestId);
        if (room.players.some(item => item.isHuman && !item.left && !item.connected)) {
            return this.error(ws, '有玩家正在恢复连接，请稍候再开始', message.requestId);
        }

        room.players = room.players.filter(item => !item.left && (item.isHuman || item.stack > 0));
        const occupied = new Set(room.players.map(item => item.seatId));
        for (let seat = 0; seat < room.config.maxPlayers; seat++) {
            if (!occupied.has(seat)) room.players.push(createServerAi(seat, room.config.startingStack));
        }
        room.players.sort((a, b) => a.seatId - b.seatId);
        room.game = new ServerPokerGame(room.config, room.players, { random: this.random });
        room.game.onChange = () => {
            room.stateVersion++;
            this.broadcastGame(room);
            this.scheduleAi(room);
        };
        room.game.onHandEnd = () => {
            room.ready.clear();
            this.broadcastReady(room);
        };
        const humanCount = room.players.filter(item => item.isHuman && !item.left).length;
        const aiCount = room.players.filter(item => !item.isHuman).length;
        for (const human of room.players.filter(item => item.isHuman && item.connected)) safeSend(human.ws, {
            type: 'game_starting', roomCode: room.code, playerCount: room.players.length,
            humanCount, aiCount, roomConfig: room.config
        });
        if (!room.game.startHand()) {
            room.game = null;
            return this.error(ws, '至少需要两名可参赛玩家', message.requestId);
        }
        this.ack(ws, message.requestId, true, room);
    }

    playerAction(ws, message) {
        const found = this.roomPlayer(ws);
        if (!found?.room.game) return this.error(ws, '牌局尚未开始', message.requestId);
        const { room, player } = found;
        if (!message.requestId) return this.error(ws, '动作缺少 requestId');
        if (player.processed.has(message.requestId)) return safeSend(ws, player.processed.get(message.requestId));
        const game = room.game;
        let result;
        if (String(message.roomCode || room.code) !== room.code) result = { ok:false, error:'房间状态已过期' };
        else if (Number(message.handId) !== game.handId) result = { ok:false, error:'这是一条旧牌局动作' };
        else if (Number(message.turnId) !== game.turnId) result = { ok:false, error:'行动回合已经变化' };
        else if (Number(message.expectedStateVersion) !== room.stateVersion) result = { ok:false, error:'牌局状态已更新，请重试' };
        else {
            const idx = room.players.indexOf(player);
            result = game.act(idx, message.action, message.amount);
        }
        const payload = {
            type: 'action_ack', requestId: message.requestId, accepted: !!result.ok,
            message: result.ok ? '行动已确认' : result.error,
            roomCode: room.code, handId: game.handId, turnId: game.turnId, stateVersion: room.stateVersion
        };
        player.processed.set(message.requestId, payload);
        while (player.processed.size > 64) player.processed.delete(player.processed.keys().next().value);
        safeSend(ws, payload);
        if (!result.ok) this.broadcastGameTo(player, room);
    }

    readyForNext(ws, message) {
        const found = this.roomPlayer(ws);
        if (!found?.room.game) return this.error(ws, '牌局尚未开始', message.requestId);
        const { room, player } = found;
        const game = room.game;
        if (game.phase !== 'idle' || !game.lastHandResult) return this.error(ws, '本手尚未结束', message.requestId);
        if (Number(message.handId) !== game.handId) return this.error(ws, '旧牌局的准备信号已忽略', message.requestId);
        room.ready.add(player.id);
        this.ack(ws, message.requestId, true, room);
        const waiting = room.players.filter(item => item.isHuman && item.connected && !item.left && item.stack > 0);
        const allReady = waiting.length > 0 && waiting.every(item => room.ready.has(item.id));
        const scheduled = allReady && this.tryStartNextHand(room);
        this.broadcastReady(room, scheduled);
    }

    refillVacatedSeats(room) {
        if (!room.game || room.game.phase !== 'idle') return;
        for (let index = 0; index < room.players.length; index++) {
            const current = room.players[index];
            if (!current.isHuman || !current.left) continue;
            if (current.resumeToken) this.sessions.delete(current.resumeToken);
            room.players.splice(index, 1, createServerAi(current.seatId, room.config.startingStack));
        }
        room.players.sort((a, b) => a.seatId - b.seatId);
        room.game.players = room.players;
    }

    tryStartNextHand(room) {
        const game = room.game;
        if (room.nextHandTimer) return true;
        if (!game || game.phase !== 'idle' || !game.lastHandResult) return false;
        const humans = room.players.filter(item => item.isHuman && item.connected && !item.left && item.stack > 0);
        if (!humans.length || !humans.every(item => room.ready.has(item.id))) return false;
        this.refillVacatedSeats(room);
        if (room.players.filter(item => game.activeForNewHand(item)).length < 2) return false;
        room.nextHandTimer = setTimeout(() => {
            room.nextHandTimer = null;
            if (this.rooms.get(room.code) !== room || room.game !== game || game.phase !== 'idle') return;
            const currentHumans = room.players.filter(item => item.isHuman && item.connected && !item.left && item.stack > 0);
            if (!currentHumans.length || !currentHumans.every(item => room.ready.has(item.id))) {
                this.broadcastReady(room, false);
                return;
            }
            // A player can leave during the short "all ready" countdown. Fill
            // that seat here as well so the remaining player never gets stuck
            // between hands with a disabled ready button.
            this.refillVacatedSeats(room);
            if (room.players.filter(item => game.activeForNewHand(item)).length < 2) {
                this.broadcastReady(room, false);
                return;
            }
            room.ready.clear();
            if (!game.startHand()) this.broadcastReady(room, false);
        }, 250);
        room.nextHandTimer.unref?.();
        return true;
    }

    broadcastReady(room, allReady = false) {
        const humans = room.players.filter(item => item.isHuman && item.connected && !item.left && item.stack > 0);
        const payload = {
            type: 'ready_status', roomCode: room.code, handId: room.game?.handId || 0,
            allReady, waitingFor: humans.filter(item => !room.ready.has(item.id)).length,
            players: humans.map(item => ({ id: item.id, ready: room.ready.has(item.id) }))
        };
        for (const player of humans) safeSend(player.ws, payload);
    }

    scheduleAi(room) {
        if (room.aiTimer) clearTimeout(room.aiTimer);
        const game = room.game;
        const idx = game?.currentPlayerIdx;
        const player = idx >= 0 ? room.players[idx] : null;
        if (!game || game.phase === 'idle' || !player || player.isHuman) return;
        const handId = game.handId, turnId = game.turnId;
        room.aiTimer = setTimeout(() => {
            room.aiTimer = null;
            this.enqueueAi(() => {
                if (room.game !== game || game.handId !== handId || game.turnId !== turnId || game.currentPlayerIdx !== idx) return;
                const decision = chooseServerAiAction(game, idx);
                if (decision) game.act(idx, decision.action, decision.amount);
            });
        }, this.aiDelay);
    }

    enqueueAi(job) {
        this.aiQueue.push(job);
        this.drainAiQueue();
    }

    drainAiQueue() {
        if (this.aiBusy || !this.aiQueue.length) return;
        this.aiBusy = true;
        const job = this.aiQueue.shift();
        setImmediate(() => {
            try { job(); }
            catch (error) { console.error('[poker-server] AI decision failed:', error.message); }
            finally {
                this.aiBusy = false;
                this.drainAiQueue();
            }
        });
    }

    broadcastGame(room) {
        for (const player of room.players) if (player.isHuman && player.connected) this.broadcastGameTo(player, room);
    }

    broadcastGameTo(player, room) {
        const game = room.game;
        if (!game) return;
        const idx = room.players.indexOf(player);
        const legalActions = game.legalActions(idx);
        safeSend(player.ws, {
            type: 'game_state',
            roomCode: room.code,
            stateVersion: room.stateVersion,
            handId: game.handId,
            turnId: game.turnId,
            hostId: room.hostId,
            isHost: room.hostId === player.id,
            roomConfig: room.config,
            isShortDeck: game.isShortDeck,
            phase: game.phase,
            pot: game.pot,
            communityCards: game.communityCards.map(card => ({ rank: card.rank, suit: card.suit })),
            players: this.publicPlayers(room),
            dealerPos: game.dealerPos,
            sbIdx: game.sbIdx,
            bbIdx: game.bbIdx,
            currentPlayerIdx: game.currentPlayerIdx,
            currentBet: game.currentBet,
            lastRaise: game.lastRaise,
            isYourTurn: game.currentPlayerIdx === idx && game.phase !== 'idle',
            yourCards: player.holeCards.map(card => ({ rank: card.rank, suit: card.suit })),
            yourStack: player.stack,
            yourBet: game.roundBets[idx] || 0,
            legalActions,
            lastHandResult: game.lastHandResult
        });
    }

    sendSnapshot(ws) {
        const found = this.roomPlayer(ws);
        if (!found) return this.error(ws, '尚未进入房间');
        this.sendRoomJoined(found.player, found.room, false, found.player.resumeToken, true);
        if (found.room.game) this.broadcastGameTo(found.player, found.room);
    }

    leaveRoom(ws, message = {}) {
        const found = this.roomPlayer(ws);
        if (!found) return;
        const { room, player } = found;
        player.left = true;
        player.connected = false;
        if (player.disconnectTimer) { clearTimeout(player.disconnectTimer); player.disconnectTimer = null; }
        if (player.hostTimer) { clearTimeout(player.hostTimer); player.hostTimer = null; }
        this.sessions.delete(player.resumeToken);
        this.detach(ws, false);
        if (!room.game) room.players = room.players.filter(item => item !== player);
        this.foldDisconnectedPlayer(room, player);
        this.migrateHost(room, true);
        room.stateVersion++;
        this.broadcastLobby(room, 'player_left');
        if (room.game) this.broadcastGame(room);
        this.ack(ws, message.requestId, true, room);
        this.tryStartNextHand(room);
        this.cleanupRoom(room);
    }

    onDisconnect(ws, explicit) {
        const found = this.roomPlayer(ws);
        if (!found) { this.connections.delete(ws); return; }
        const { room, player } = found;
        if (player.ws !== ws) { this.connections.delete(ws); return; }
        player.connected = false;
        player.ws = null;
        this.detach(ws, true);
        room.stateVersion++;
        this.broadcastLobby(room, 'player_left');
        if (room.game) this.broadcastGame(room);
        if (player.disconnectTimer) clearTimeout(player.disconnectTimer);
        player.disconnectTimer = setTimeout(() => {
            player.disconnectTimer = null;
            if (player.connected || player.left || this.rooms.get(room.code) !== room) return;
            player.left = true;
            if (player.resumeToken) this.sessions.delete(player.resumeToken);
            this.foldDisconnectedPlayer(room, player);
            this.migrateHost(room, false);
            room.stateVersion++;
            this.broadcastLobby(room, 'player_left');
            if (room.game) this.broadcastGame(room);
            this.tryStartNextHand(room);
            this.cleanupRoom(room);
        }, this.disconnectGraceMs);
        player.disconnectTimer.unref?.();
        this.cleanupRoom(room);
    }

    foldDisconnectedPlayer(room, player) {
        const game = room.game;
        if (!game || game.phase === 'idle' || !player.dealtIn || player.folded || player.isAllIn) return;
        const idx = room.players.indexOf(player);
        player.folded = true;
        player.lastAction = { action: 'fold', amount: 0 };
        if (game.inHand().length <= 1) game.finishByFold(game.inHand()[0]);
        else if (game.currentPlayerIdx === idx) game.advanceAfterAction();
        else game.emitChange();
    }

    migrateHost(room, immediate) {
        const current = room.players.find(item => item.id === room.hostId);
        if (!immediate && current?.connected && !current.left) return;
        if (current && !current.left && current.connected) return;
        const next = room.players.filter(item => item.isHuman && item.connected && !item.left)
            .sort((a, b) => a.joinedAt - b.joinedAt)[0];
        const old = room.hostId;
        room.hostId = next?.id || null;
        if (old !== room.hostId) {
            room.stateVersion++;
            for (const player of room.players) if (player.isHuman && player.connected) safeSend(player.ws, {
                type: 'host_changed', roomCode: room.code, hostId: room.hostId,
                isHost: player.id === room.hostId, stateVersion: room.stateVersion,
                players: this.publicPlayers(room)
            });
            if (room.game) this.broadcastGame(room);
        }
    }

    cleanupRoom(room) {
        if (room.cleanupTimer) clearTimeout(room.cleanupTimer);
        if (room.players.some(item => item.isHuman && item.connected && !item.left)) return;
        room.cleanupTimer = setTimeout(() => {
            if (room.players.some(item => item.isHuman && item.connected && !item.left)) return;
            if (room.aiTimer) clearTimeout(room.aiTimer);
            if (room.nextHandTimer) clearTimeout(room.nextHandTimer);
            for (const player of room.players) if (player.resumeToken) this.sessions.delete(player.resumeToken);
            this.rooms.delete(room.code);
        }, 60000);
        room.cleanupTimer.unref?.();
    }

    roomPlayer(ws) {
        const meta = this.connections.get(ws);
        const room = meta?.roomCode ? this.rooms.get(meta.roomCode) : null;
        const player = room?.players.find(item => item.id === meta.playerId);
        return room && player ? { room, player } : null;
    }

    detach(ws, removeConnection = false) {
        const meta = this.connections.get(ws);
        if (meta) Object.assign(meta, { roomCode: null, playerId: null, sessionToken: null });
        if (removeConnection) this.connections.delete(ws);
    }

    ack(ws, requestId, accepted, room) {
        if (!requestId) return;
        safeSend(ws, {
            type: 'ack', requestId, accepted, roomCode: room?.code || null,
            stateVersion: room?.stateVersion || 0, handId: room?.game?.handId || 0
        });
    }

    error(ws, message, requestId, code = 'INVALID_REQUEST') {
        safeSend(ws, { type: 'error', requestId: requestId || null, code, message: String(message || '请求失败') });
    }
}

async function startLocalPokerServer(options = {}) {
    const server = new PokerRoomServer(options);
    await server.start();
    return server;
}

if (require.main === module) {
    startLocalPokerServer({
        host: process.env.POKER_HOST || '127.0.0.1',
        port: Number(process.env.POKER_PORT || 3000)
    }).then(server => {
        const address = server.address();
        console.log(`本地德州扑克服务已启动：ws://${address.address}:${address.port}`);
    }).catch(error => {
        console.error('本地服务启动失败：', error.message);
        process.exitCode = 1;
    });
}

module.exports = { PROTOCOL_VERSION, PokerRoomServer, startLocalPokerServer, sanitizeConfig, normalizeName };
