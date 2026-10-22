'use strict';
// Golden protocol corpus exporter (task-2 / Phase 5 prep).
//
// The v2 WebSocket protocol lives in the read-only JS reference repo:
//   server/local-server.js   (room server, message assembly, limits)
//   server/server-game.js    (authoritative game + server AI)
//   js/network.js            (browser client: message shapes, reconnect rules)
//   main.js                  (desktop service discovery / hello probe)
//
// This script drives the REAL server through ws clients, captures every message
// it emits (s2c) plus every protocol message the old client shape sends (c2s),
// and exports golden corpora consumed by crates/poker-protocol/tests/.
//
// Determinism: Math.random, performance.now, crypto.randomUUID and
// crypto.randomBytes are patched BEFORE the reference modules load, so two runs
// with the same Node version produce byte-identical corpora.
//
// Usage:
//   node oracle/golden-protocol.js            # writes tests/golden/protocol/
//   node oracle/golden-protocol.js --out DIR  # writes to DIR (reproducibility check)
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const crypto = require('node:crypto');
const { referenceRepo } = require('./lib/load-js');
const { mulberry32 } = require('./lib/prng');

const REPO = referenceRepo();
const PROJECT = path.resolve(__dirname, '..');
const args = process.argv.slice(2);
const outArgIndex = args.indexOf('--out');
const OUT = path.resolve(outArgIndex >= 0 ? args[outArgIndex + 1] : path.join(PROJECT, 'tests', 'golden', 'protocol'));
const SEED = 0x7e57c0de;
const REFERENCE_COMMIT = 'e0c6c6f88d3e64324e1d83bfc700dd3a77f71b90';

// ---------------------------------------------------------------------------
// 1. Deterministic environment (must run before the reference server loads).
// ---------------------------------------------------------------------------
Math.random = mulberry32(SEED);
if (typeof performance !== 'undefined' && typeof performance.now === 'function') performance.now = () => 0;

let uuidCounter = 0;
crypto.randomUUID = () => {
    uuidCounter++;
    return '00000000-0000-4000-8000-' + String(uuidCounter).padStart(12, '0');
};
let tokenCounter = 0;
crypto.randomBytes = (size = 16, callback) => {
    const bytes = Buffer.alloc(size);
    tokenCounter++;
    for (let i = 0; i < size; i++) bytes[i] = (0x41 + tokenCounter * 37 + i * 11) & 0xff;
    if (typeof callback === 'function') { process.nextTick(() => callback(null, bytes)); return undefined; }
    return bytes;
};

const wsModule = require(path.join(REPO, 'node_modules', 'ws'));
const { WebSocket, WebSocketServer } = wsModule;
const serverModule = require(path.join(REPO, 'server', 'local-server.js'));
const { startLocalPokerServer, sanitizeConfig, normalizeName } = serverModule;
const APP_VERSION = require(path.join(REPO, 'package.json')).version;

// ---------------------------------------------------------------------------
// 2. Corpus recording.
// ---------------------------------------------------------------------------
const corpus = { s2c: [], c2s: [], parseErrors: [], configCases: [], nameCases: [], closeEvents: [], probeCases: [] };
const seen = { s2c: new Map(), c2s: new Map() };
const typeCounts = { s2c: {}, c2s: {} };

function recordMessage(kind, scenario, context, message) {
    const key = JSON.stringify(message);
    if (seen[kind].has(key)) return;
    seen[kind].set(key, true);
    corpus[kind].push({ scenario, context, message });
    const type = message.type === undefined ? '<missing>' : String(message.type);
    typeCounts[kind][type] = (typeCounts[kind][type] || 0) + 1;
}

function assert(condition, label) {
    if (!condition) throw new Error('assertion failed: ' + label);
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

class CaptureClient {
    constructor(scenario, label) {
        this.scenario = scenario;
        this.label = label;
        this.ws = null;
        this.messages = [];
        this.waiters = new Set();
        this.closed = null;
        this.closeWaiters = new Set();
    }

    async connect(url, options = {}) {
        const ws = new WebSocket(url, options);
        this.ws = ws;
        ws.on('message', raw => this._onMessage(raw));
        ws.on('error', () => {});
        ws.on('close', (code, reason) => {
            this.closed = { code, reason: reason.toString('utf8') };
            for (const resolve of [...this.closeWaiters]) resolve(this.closed);
            this.closeWaiters.clear();
        });
        await new Promise((resolve, reject) => {
            ws.once('open', resolve);
            ws.once('error', reject);
        });
        return this;
    }

    _onMessage(raw) {
        const message = JSON.parse(raw.toString('utf8'));
        this.messages.push(message);
        recordMessage('s2c', this.scenario, this._contextFor(message), message);
        for (const waiter of [...this.waiters]) {
            if (!waiter.predicate(message, this)) continue;
            this.waiters.delete(waiter);
            clearTimeout(waiter.timer);
            waiter.resolve({ client: this, message });
        }
    }

    _contextFor(message) {
        const type = message.type;
        if (type === 'game_state') {
            const parts = [message.phase === 'idle' ? 'idle' : message.phase];
            if (message.isYourTurn) parts.push('your-turn');
            if (message.lastHandResult) parts.push('hand-result-' + message.lastHandResult.reason);
            return 'game_state:' + parts.join(':');
        }
        if (type === 'action_ack') return 'action_ack:' + (message.accepted ? 'accepted' : 'rejected');
        if (type === 'ready_status') return 'ready_status:' + (message.allReady ? 'all-ready' : 'waiting');
        if (type === 'error') return 'error:' + message.code;
        return String(type);
    }

    send(context, payload) {
        const message = { protocolVersion: 2, ...payload };
        this.ws.send(JSON.stringify(message));
        recordMessage('c2s', this.scenario, context, message);
        return message;
    }

    sendRaw(text) {
        this.ws.send(text);
    }

    waitAfter(predicate, timeout = 8000) {
        return new Promise((resolve, reject) => {
            const waiter = { predicate, resolve, timer: null };
            waiter.timer = setTimeout(() => {
                this.waiters.delete(waiter);
                const seenTypes = this.messages.map(message => message.type + (message.requestId ? '/' + message.requestId : ''));
                reject(new Error(this.label + ' [' + this.scenario + ']: timed out after ' + timeout + 'ms waiting for message; received: ' + seenTypes.join(', ')));
            }, timeout);
            this.waiters.add(waiter);
        });
    }

    // Existing-first wait: hello and other messages that the server may have
    // sent before this waiter was installed are still observable exactly once.
    expectMessage(predicate, timeout = 8000) {
        const existing = this.messages.find(message => predicate(message, this));
        if (existing) return Promise.resolve({ client: this, message: existing });
        return this.waitAfter(predicate, timeout);
    }

    expectType(type, timeout = 8000) {
        return this.expectMessage(message => message.type === type, timeout);
    }

    waitForCount(predicate, count, timeout = 8000) {
        const existing = this.messages.filter(message => predicate(message, this));
        if (existing.length >= count) return Promise.resolve(existing);
        const collected = existing.slice();
        return new Promise((resolve, reject) => {
            const waiter = {
                predicate: message => {
                    if (!predicate(message, this)) return false;
                    collected.push(message);
                    return collected.length >= count;
                },
                resolve: () => resolve(collected),
                timer: null
            };
            waiter.timer = setTimeout(() => {
                this.waiters.delete(waiter);
                reject(new Error(this.label + ': timed out waiting for ' + count + ' matching messages'));
            }, timeout);
            this.waiters.add(waiter);
        });
    }

    waitForType(type, timeout = 8000) {
        return this.expectType(type, timeout);
    }

    waitForClose(timeout = 5000) {
        if (this.closed) return Promise.resolve(this.closed);
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => reject(new Error(this.label + ': timed out waiting for close')), timeout);
            this.closeWaiters.add(code => { clearTimeout(timer); resolve(code); });
        });
    }

    latestState() {
        for (let index = this.messages.length - 1; index >= 0; index--) {
            if (this.messages[index].type === 'game_state') return this.messages[index];
        }
        return null;
    }

    close() {
        try { this.ws?.close(1000, 'capture complete'); } catch (_) {}
    }
}

function waitAny(clients, predicate, timeout = 8000) {
    return new Promise((resolve, reject) => {
        let settled = false;
        const cleanups = [];
        const timer = setTimeout(() => {
            if (settled) return;
            settled = true;
            for (const cleanup of cleanups) cleanup();
            reject(new Error('timed out waiting for any client message'));
        }, timeout);
        const finish = hit => {
            if (settled) return;
            settled = true;
            clearTimeout(timer);
            for (const cleanup of cleanups) cleanup();
            resolve(hit);
        };
        for (const client of clients) {
            const waiter = { predicate, timer: null, resolve: finish };
            client.waiters.add(waiter);
            cleanups.push(() => client.waiters.delete(waiter));
        }
    });
}

async function closeAll(clients) {
    for (const client of clients) client.close();
    await sleep(60);
}

function latestStateOf(clients) {
    const entries = [];
    for (const client of clients) {
        const state = client.latestState();
        if (state) entries.push({ client, state });
    }
    return entries;
}

async function waitForState(clients, predicate, timeout = 8000) {
    const deadline = Date.now() + timeout;
    for (;;) {
        const matches = latestStateOf(clients).filter(entry => predicate(entry.state));
        if (matches.length) {
            matches.sort((a, b) => b.state.stateVersion - a.state.stateVersion);
            return matches[0];
        }
        const newest = Math.max(0, ...latestStateOf(clients).map(entry => entry.state.stateVersion));
        const remaining = deadline - Date.now();
        if (remaining <= 0) throw new Error('waitForState timed out');
        try {
            await waitAny(clients, message => message.type === 'game_state' && message.stateVersion > newest, remaining);
        } catch (error) {
            const lastChance = latestStateOf(clients).filter(entry => predicate(entry.state));
            if (lastChance.length) {
                lastChance.sort((a, b) => b.state.stateVersion - a.state.stateVersion);
                return lastChance[0];
            }
            throw new Error('waitForState timed out; latest: ' + JSON.stringify(latestStateOf(clients).map(entry => ({ client: entry.client.label, handId: entry.state.handId, phase: entry.state.phase, stateVersion: entry.state.stateVersion, isYourTurn: entry.state.isYourTurn }))));
        }
    }
}

/**
 * Drives a room to the end of the current hand: checks/calls whenever a human
 * seat is to act, waits through AI turns. Returns the terminal game_state or
 * null on timeout.
 */
async function driveHand(clients, options = {}) {
    const maxActions = options.maxActions || 80;
    const actionLog = [];
    for (let step = 0; step < maxActions; step++) {
        const states = latestStateOf(clients);
        if (!states.length) throw new Error('driveHand: no game_state received');
        const newestVersion = Math.max(...states.map(entry => entry.state.stateVersion));
        const idle = states.find(entry => entry.state.phase === 'idle' && entry.state.lastHandResult && entry.state.stateVersion === newestVersion);
        if (idle) return { state: idle.state, actions: actionLog };
        const staleIdle = states.find(entry => entry.state.phase === 'idle' && entry.state.lastHandResult);
        if (staleIdle && states.every(entry => entry.state.stateVersion <= staleIdle.state.stateVersion)) {
            return { state: staleIdle.state, actions: actionLog };
        }
        const turn = states
            .filter(entry => entry.state.isYourTurn)
            .sort((a, b) => b.state.stateVersion - a.state.stateVersion)[0];
        if (turn) {
            const state = turn.state;
            const legal = state.legalActions.actions;
            const action = legal.includes('check') ? 'check' : legal.includes('call') ? 'call' : 'fold';
            const amount = action === 'call' ? state.legalActions.toCall : 0;
            const requestId = 'act-' + state.handId + '-' + state.turnId;
            const ackPromise = turn.client.waitAfter(message => message.type === 'action_ack' && message.requestId === requestId, 10000);
            turn.client.send('player_action:' + action, {
                type: 'player_action',
                requestId,
                roomCode: state.roomCode,
                handId: state.handId,
                turnId: state.turnId,
                expectedStateVersion: state.stateVersion,
                action,
                amount
            });
            const { message: ack } = await ackPromise;
            actionLog.push({ step, client: turn.client.label, handId: state.handId, turnId: state.turnId, action, amount, accepted: ack.accepted });
            if (!ack.accepted) throw new Error(turn.client.label + ': action rejected: ' + ack.message);
            if (options.stopAfterActions && actionLog.length >= options.stopAfterActions) {
                return { state: null, actions: actionLog, stopped: true };
            }
            await sleep(25);
            continue;
        }
        try {
            await waitAny(clients, message => message.type === 'game_state' && message.stateVersion > newestVersion, 12000);
        } catch (_) {
            return { state: null, actions: actionLog, timedOut: true };
        }
        await sleep(10);
    }
    throw new Error('driveHand: hand did not finish');
}

async function readyHands(clients, handId, timeout = 8000) {
    for (const client of clients) {
        client.send('ready_for_next', { type: 'ready_for_next', requestId: 'ready-' + client.label + '-' + handId, roomCode: client.latestState()?.roomCode, handId });
    }
    return waitForState(clients, state => state.handId > handId && state.phase !== 'idle', timeout);
}

// ---------------------------------------------------------------------------
// 3. Scenarios against the authoritative JS server.
// ---------------------------------------------------------------------------
const capturedClients = [];

async function connectClient(url, scenario, label, options = {}) {
    const client = new CaptureClient(scenario, label);
    await client.connect(url, options);
    capturedClients.push(client);
    return client;
}

function roomOptions(overrides = {}) {
    return {
        maxPlayers: 2,
        isShortDeck: false,
        startingStack: 20000,
        smallBlind: 40,
        bigBlind: 80,
        ante: 40,
        minBet: 80,
        aiDelay: 800,
        ...overrides
    };
}

function stateRequestId(state) {
    return 'act-' + state.handId + '-' + state.turnId;
}

async function scenarioLobby(url) {
    const scenario = 'lobby';
    const a = await connectClient(url, scenario, 'lobby-host');
    const hello = await a.waitForType('hello');
    assert(hello.message.type === 'hello' && hello.message.protocolVersion === 2, 'hello shape');
    assert(hello.message.serverId === 'texas-holdem-game', 'hello serverId');
    assert(hello.message.appVersion === APP_VERSION, 'hello appVersion');

    a.send('create_room', { type: 'create_room', requestId: 'create-1-4f2', name: 'Alice', ...roomOptions() });
    const created = await a.waitAfter(message => message.type === 'room_created');
    assert(created.message.roomCode === '1', 'create room code');
    assert(created.message.isHost === true, 'creator is host');
    assert(created.message.roomConfig.isShortDeck === false, 'room config standard');

    const b = await connectClient(url, scenario, 'lobby-guest');
    await b.waitForType('hello');
    b.send('join_room', { type: 'join_room', requestId: 'join-1-4f3', roomCode: '1', name: '<b>Bob</b>', ...roomOptions() });
    const joined = await b.waitAfter(message => message.type === 'room_joined');
    assert(joined.message.roomCode === '1', 'joined room code');
    assert(joined.message.isHost === false, 'guest not host');
    assert(joined.message.resumed === false, 'fresh join is not resumed');
    assert(typeof joined.message.resumeToken === 'string' && joined.message.resumeToken.length >= 20, 'resume token length');
    await a.expectMessage(message => message.type === 'player_joined' && message.players?.length === 2);

    // Lobby/room lifecycle errors.
    let waiter = b.waitAfter(message => message.type === 'error');
    b.send('join_room', { type: 'join_room', requestId: 'join-again', roomCode: '1', name: 'Bob', ...roomOptions() });
    let error = await waiter;
    assert(error.message.message === '请先离开当前房间', 'error already in room');

    waiter = b.waitAfter(message => message.type === 'error');
    b.send('start_game', { type: 'start_game', requestId: 'not-host', roomCode: '1' });
    error = await waiter;
    assert(error.message.message === '只有房主可以开始游戏', 'error host only');

    waiter = b.waitAfter(message => message.type === 'error');
    b.send('start_game', { type: 'start_game', requestId: 'not-host-2' });
    error = await waiter;
    assert(error.message.message === '只有房主可以开始游戏', 'error host only without room code');

    waiter = a.waitAfter(message => message.type === 'error');
    a.send('unknown_type_here', { type: 'unknown_type_here', requestId: 'unknown-1' });
    error = await waiter;
    assert(error.message.message === '未知消息类型', 'error unknown type');

    waiter = a.waitAfter(message => message.type === 'error' && message.message === '缺少消息类型');
    a.sendRaw(JSON.stringify({ requestId: 'missing-type-1' }));
    await waiter;

    waiter = a.waitAfter(message => message.type === 'error' && message.message === '消息格式无效');
    a.sendRaw('not-json{');
    await waiter;

    // get_snapshot in the lobby: echoed as a resumed room_joined.
    const snapshot = a.waitAfter(message => message.type === 'room_joined' && message.resumed === true);
    a.send('get_snapshot', { type: 'get_snapshot' });
    await snapshot;

    // A third client that is not in the room.
    const c = await connectClient(url, scenario, 'lobby-third');
    await c.waitForType('hello');

    waiter = c.waitAfter(message => message.type === 'error');
    c.send('join_room', { type: 'join_room', requestId: 'join-bad-code', roomCode: '9', name: 'Carol', ...roomOptions() });
    error = await waiter;
    assert(error.message.message === '房间号只能是 1–5', 'error room code range');

    waiter = c.waitAfter(message => message.type === 'error');
    c.send('join_room', { type: 'join_room', requestId: 'join-full', roomCode: '1', name: 'Carol', ...roomOptions() });
    error = await waiter;
    assert(error.message.message === '房间已满', 'error room full (humans >= maxPlayers)');

    waiter = c.waitAfter(message => message.type === 'error');
    c.send('resume_session', { type: 'resume_session', requestId: 'resume-bogus', roomCode: '1', resumeToken: 'bogus-token' });
    error = await waiter;
    assert(error.message.code === 'RESUME_EXPIRED', 'error resume expired');

    waiter = c.waitAfter(message => message.type === 'error');
    c.send('get_snapshot', { type: 'get_snapshot' });
    error = await waiter;
    assert(error.message.message === '尚未进入房间', 'error snapshot outside room');

    // The server accepts "get_snapshot" without a requestId and replies with
    // requestId: null on errors; leave_room only acks room members. A leave
    // from a connection that is not in a room is silently dropped.
    c.send('leave_room', { type: 'leave_room', requestId: 'leave-outside' });
    // leave_room outside a room is silently dropped: no ack, no error. Use a
    // second client action to prove the socket is still alive.
    c.send('join_room', { type: 'join_room', requestId: 'join-room-2', roomCode: '2', name: 'Carol', ...roomOptions() });
    const roomTwo = await c.waitAfter(message => message.type === 'room_created');
    assert(roomTwo.message.roomCode === '2', 'third client creates room 2');
    waiter = c.waitAfter(message => message.type === 'ack');
    c.send('leave_room', { type: 'leave_room', requestId: 'leave-room-2', roomCode: '2' });
    const leaveAck = await waiter;
    assert(leaveAck.message.accepted === true, 'leave room ack');

    await closeAll([a, b, c]);
}

async function scenarioStandardTwoHumans(url) {
    const scenario = 'standard-two-humans';
    const a = await connectClient(url, scenario, 'host');
    await a.waitForType('hello');
    const b = await connectClient(url, scenario, 'guest');
    await b.waitForType('hello');

    a.send('create_room', { type: 'create_room', requestId: 'create-1-9aa', name: 'Host', ...roomOptions() });
    const created = await a.waitAfter(message => message.type === 'room_created');
    assert(created.message.roomCode === '1', 'room 1 created');
    b.send('join_room', { type: 'join_room', requestId: 'join-1-9ab', roomCode: '1', name: 'Guest', ...roomOptions() });
    await b.waitAfter(message => message.type === 'room_joined');
    await a.expectMessage(message => message.type === 'player_joined' && message.players?.length === 2);

    // start_game from a non-host, then from the host.
    let waiter = b.waitAfter(message => message.type === 'error');
    b.send('start_game', { type: 'start_game', requestId: 'start-not-host', roomCode: '1' });
    await waiter;

    const startMark = [a.messages.length, b.messages.length];
    a.send('start_game', { type: 'start_game', requestId: 'start-1-9ac', roomCode: '1' });
    const startingA = a.waitAfter(message => message.type === 'game_starting');
    const startingB = b.waitAfter(message => message.type === 'game_starting');
    const startAck = a.waitForType('ack');
    const stateA = a.waitAfter(message => message.type === 'game_state' && message.handId === 1);
    const stateB = b.waitAfter(message => message.type === 'game_state' && message.handId === 1);
    await Promise.all([startingA, startingB, startAck, stateA, stateB]);
    assert(a.messages.length > startMark[0] && b.messages.length > startMark[1], 'both got game messages');

    // A third client cannot join a room that is already playing; host cannot start twice.
    const c = await connectClient(url, scenario, 'late-joiner');
    await c.waitForType('hello');
    waiter = c.waitAfter(message => message.type === 'error');
    c.send('join_room', { type: 'join_room', requestId: 'join-started', roomCode: '1', name: 'Late', ...roomOptions() });
    const joinStarted = await waiter;
    assert(joinStarted.message.message === '房间已经开局，只能使用恢复令牌返回原座位', 'error join started room');
    waiter = c.waitAfter(message => message.type === 'error');
    c.send('resume_session', { type: 'resume_session', requestId: 'resume-late', roomCode: '1', resumeToken: 'bogus' });
    await waiter;

    waiter = a.waitAfter(message => message.type === 'error');
    a.send('start_game', { type: 'start_game', requestId: 'start-again', roomCode: '1' });
    const startAgain = await waiter;
    assert(startAgain.message.message === '游戏已经开始', 'error game already started');

    // Isolated non-room client behaviour (for the protocol corpus).
    waiter = c.waitAfter(message => message.type === 'error');
    c.send('player_action', { type: 'player_action', requestId: 'action-outside', roomCode: '1', handId: 1, turnId: 0, expectedStateVersion: 0, action: 'fold', amount: 0 });
    await waiter;
    waiter = c.waitAfter(message => message.type === 'error');
    c.send('ready_for_next', { type: 'ready_for_next', requestId: 'ready-outside', roomCode: '1', handId: 1 });
    await waiter;
    c.close();

    const actorHit = await waitForState([a, b], state => state.isYourTurn && state.phase !== 'idle');
    const actor = actorHit.client;
    const actorState = actorHit.state;
    const ack = predicate => actor.waitAfter(message => message.type === 'action_ack' && predicate(message), 8000);

    // Rejected action matrix (each produces an action_ack accepted:false).
    const rejections = [
        { label: 'stale-room', payload: { roomCode: '2' }, expected: '房间状态已过期' },
        { label: 'old-hand', payload: { handId: actorState.handId - 1 }, expected: '这是一条旧牌局动作' },
        { label: 'old-turn', payload: { turnId: actorState.turnId - 1 }, expected: '行动回合已经变化' },
        { label: 'stale-version', payload: { expectedStateVersion: actorState.stateVersion - 1 }, expected: '牌局状态已更新，请重试' },
        { label: 'off-grid-raise', payload: { action: 'raise', amount: 201 }, expected: '加注金额必须是 40 的整数倍；零头筹码可使用全下' },
        { label: 'out-of-range-raise', payload: { action: 'raise', amount: 120 }, expected: '加注金额不在合法范围内' },
        { label: 'illegal-check', payload: { action: 'check', amount: 0 }, expected: '当前不能执行这个行动' }
    ];
    for (const rejection of rejections) {
        const requestId = 'reject-' + rejection.label;
        const base = {
            type: 'player_action',
            requestId,
            roomCode: actorState.roomCode,
            handId: actorState.handId,
            turnId: actorState.turnId,
            expectedStateVersion: actorState.stateVersion,
            action: 'fold',
            amount: 0
        };
        const pending = ack(message => message.requestId === requestId);
        actor.send('player_action:rejected-' + rejection.label, { ...base, ...rejection.payload });
        const { message } = await pending;
        assert(message.accepted === false, rejection.label + ' must be rejected');
        assert(message.message === rejection.expected, rejection.label + ' message mismatch: ' + message.message);
        await sleep(20);
    }

    // Missing requestId is a server-level error, not an action_ack.
    waiter = actor.waitAfter(message => message.type === 'error' && message.message === '动作缺少 requestId');
    actor.sendRaw(JSON.stringify({
        protocolVersion: 2,
        type: 'player_action',
        roomCode: actorState.roomCode,
        handId: actorState.handId,
        turnId: actorState.turnId,
        expectedStateVersion: actorState.stateVersion,
        action: 'fold',
        amount: 0
    }));
    await waiter;

    // Play hand 1 to showdown: both humans check/call.
    const handOne = await driveHand([a, b]);
    assert(handOne.state?.lastHandResult?.reason === 'showdown', 'hand 1 must reach showdown');
    assert(handOne.state.lastHandResult.results.some(result => result.cards.length === 2), 'showdown reveals hole cards');
    assert(handOne.state.pot === 0, 'settled pot is zero in the idle snapshot');

    // Both ready -> hand 2. The first ready produces allReady:false, the second
    // allReady:true and a short 250ms countdown before the deal.
    const readyStatuses = [];
    const readyWait = b.waitAfter(message => message.type === 'ready_status' && message.allReady === false);
    a.send('ready_for_next', { type: 'ready_for_next', requestId: 'ready-host-hand-1', roomCode: '1', handId: 1 });
    readyStatuses.push((await readyWait).message);
    const readyBoth = a.waitAfter(message => message.type === 'ready_status' && message.allReady === true);
    b.send('ready_for_next', { type: 'ready_for_next', requestId: 'ready-guest-hand-1', roomCode: '1', handId: 1 });
    readyStatuses.push((await readyBoth).message);
    const handTwo = await waitForState([a, b], state => state.handId === 2 && state.phase !== 'idle');
    assert(handTwo.state.phase === 'preflop', 'hand 2 preflop');

    // A stale ready signal for hand 1 is rejected.
    waiter = a.waitAfter(message => message.type === 'error' && message.requestId === 'ready-stale');
    a.send('ready_for_next', { type: 'ready_for_next', requestId: 'ready-stale', roomCode: '1', handId: 1 });
    await waiter;

    // Duplicate requestId: the same fold is replayed from the dedupe cache.
    const foldActor = await waitForState([a, b], state => state.isYourTurn && state.handId === 2);
    const foldPayload = {
        type: 'player_action',
        requestId: 'duplicate-fold-hand-2',
        roomCode: '1',
        handId: foldActor.state.handId,
        turnId: foldActor.state.turnId,
        expectedStateVersion: foldActor.state.stateVersion,
        action: 'fold',
        amount: 0
    };
    const duplicateWait = foldActor.client.waitForCount(message => message.type === 'action_ack' && message.requestId === 'duplicate-fold-hand-2', 2);
    foldActor.client.send('player_action:fold-duplicate', foldPayload);
    foldActor.client.send('player_action:fold-duplicate', foldPayload);
    const duplicateAcks = await duplicateWait;
    assert(duplicateAcks.every(message => message.accepted === true), 'duplicate action must replay an accepted ack');
    const handTwoEnd = await waitForState([a, b], state => state.phase === 'idle' && state.lastHandResult && state.handId === 2);
    assert(handTwoEnd.state.lastHandResult.reason === 'fold', 'hand 2 ends by fold');

    // Ready for hand 3, then the host leaves mid-hand (host migration + AI fill).
    await readyHands([a, b], 2);
    const handThree = await waitForState([a, b], state => state.handId === 3 && state.phase !== 'idle');
    assert(handThree.state.handId === 3, 'hand 3 starts');
    const guestClient = b;
    const leaveWaiters = [
        b.waitAfter(message => message.type === 'player_left'),
        b.waitAfter(message => message.type === 'host_changed' && message.isHost === true)
    ];
    a.send('leave_room', { type: 'leave_room', requestId: 'leave-host-hand-3', roomCode: '1' });
    await Promise.all(leaveWaiters);
    // The leave ack goes to the leaving socket, not to the remaining player.
    await a.expectMessage(message => message.type === 'ack' && message.requestId === 'leave-host-hand-3');
    const handThreeEnd = await waitForState([b], state => state.phase === 'idle' && state.lastHandResult);
    assert(handThreeEnd.state.lastHandResult.reason === 'fold', 'leaving heads-up host folds the hand');

    // The vacated human seat is refilled with a server AI for the next hand.
    b.send('ready_for_next', { type: 'ready_for_next', requestId: 'ready-guest-hand-3', roomCode: '1', handId: 3 });
    const handFour = await waitForState([b], state => state.handId === 4 && state.phase !== 'idle', 10000);
    assert(handFour.state.players.filter(player => !player.isHuman).length === 1, 'vacated seat filled by AI');
    await driveHand([b], { stopAfterActions: 6 });

    // Leaving the room cleans up the connection metadata.
    b.send('leave_room', { type: 'leave_room', requestId: 'leave-guest', roomCode: '1' });
    await b.expectMessage(message => message.type === 'ack' && message.requestId === 'leave-guest');
    await closeAll([a, b]);
}

async function scenarioResume(url) {
    const scenario = 'resume-and-snapshot';
    const d = await connectClient(url, scenario, 'resume-host');
    await d.waitForType('hello');
    const e = await connectClient(url, scenario, 'resume-guest');
    await e.waitForType('hello');
    d.send('create_room', { type: 'create_room', requestId: 'create-3-77a', name: 'D', ...roomOptions() });
    const createdRoom = await d.waitAfter(message => message.type === 'room_created');
    const roomCode = createdRoom.message.roomCode;
    e.send('join_room', { type: 'join_room', requestId: 'join-3-77b', roomCode, name: 'E', ...roomOptions() });
    const joinedE = await e.waitAfter(message => message.type === 'room_joined');
    const resumeToken = joinedE.message.resumeToken;
    d.send('start_game', { type: 'start_game', requestId: 'start-3-77c', roomCode });
    await Promise.all([
        d.waitAfter(message => message.type === 'game_starting'),
        e.waitAfter(message => message.type === 'game_state' && message.handId === 1)
    ]);

    // Drop the guest socket, then resume the session from a fresh connection.
    const guestState = e.latestState();
    e.ws.terminate();
    await d.waitAfter(message => message.type === 'player_left');
    const e2 = await connectClient(url, scenario, 'resume-guest-2');
    await e2.waitForType('hello');
    e2.send('resume_session', { type: 'resume_session', requestId: 'resume-3-77d', roomCode, resumeToken });
    const resumed = await e2.waitAfter(message => message.type === 'room_joined' && message.resumed === true);
    assert(resumed.message.playerId === joinedE.message.playerId, 'resume keeps player id');
    assert((await e2.expectMessage(message => message.type === 'game_state' && message.handId === 1)).message.yourCards.length === 2, 'resume restores hole cards');

    // get_snapshot re-sends room_joined(resumed) plus the authoritative game_state.
    const snapshotWaiters = [
        d.waitAfter(message => message.type === 'room_joined' && message.resumed === true),
        d.waitAfter(message => message.type === 'game_state' && message.handId === 1)
    ];
    d.send('get_snapshot', { type: 'get_snapshot' });
    await Promise.all(snapshotWaiters);

    // Resuming the same token from another socket kicks the current one with 4001.
    const e3 = await connectClient(url, scenario, 'resume-guest-3');
    await e3.waitForType('hello');
    const kickedClose = e2.waitForClose(5000);
    e3.send('resume_session', { type: 'resume_session', requestId: 'resume-3-77e', roomCode, resumeToken });
    const kicked = await kickedClose;
    assert(kicked.code === 4001, 'resume elsewhere closes old socket with 4001');
    corpus.closeEvents.push({ scenario, kind: 'ws-close', context: 'resume-session-elsewhere', code: kicked.code, reason: kicked.reason });
    await e3.expectMessage(message => message.type === 'room_joined' && message.resumed === true);

    // Unknown resume token.
    const e4 = await connectClient(url, scenario, 'resume-guest-4');
    await e4.waitForType('hello');
    let waiter = e4.waitAfter(message => message.type === 'error');
    e4.send('resume_session', { type: 'resume_session', requestId: 'resume-3-77f', roomCode, resumeToken: 'expired-token' });
    const expired = await waiter;
    assert(expired.message.code === 'RESUME_EXPIRED', 'expired resume token');
    e4.close();

    // A disconnected player that does not return within the grace period is
    // folded and removed, so the survivor is never stuck.
    await sleep(50);
    const states = latestStateOf([d, e3]).sort((x, y) => y.state.stateVersion - x.state.stateVersion);
    const currentTurn = states.find(entry => entry.state.isYourTurn);
    if (currentTurn) {
        const survivor = currentTurn.client === d ? e3 : d;
        const idleWait = survivor.waitAfter(message => message.type === 'game_state' && message.phase === 'idle' && message.lastHandResult, 6000);
        currentTurn.client.ws.terminate();
        const idle = await idleWait;
        assert(idle.message.lastHandResult.reason === 'fold', 'disconnect grace folds the hand');
        await survivor.expectMessage(message => message.type === 'player_left', 5000);
    } else {
        // No human is to act (should not happen with two humans); let the server
        // advance and retry once.
        await sleep(400);
    }
    await closeAll([d, e3]);
}

async function scenarioShortDeckAi(url) {
    const scenario = 'shortdeck-ai';
    const f = await connectClient(url, scenario, 'shortdeck-host');
    await f.waitForType('hello');
    f.send('create_room', {
        type: 'create_room',
        requestId: 'create-2-55a',
        name: 'Shorty',
        ...roomOptions({ maxPlayers: 3, isShortDeck: true, aiDelay: 3200 })
    });
    const created = await f.waitAfter(message => message.type === 'room_created');
    assert(created.message.roomConfig.isShortDeck === true, 'shortdeck room config');
    assert(created.message.roomConfig.aiDelay === 3200, 'shortdeck ai delay level');
    const shortRoomCode = created.message.roomCode;
    f.send('start_game', { type: 'start_game', requestId: 'start-2-55b', roomCode: shortRoomCode });
    const starting = await f.waitAfter(message => message.type === 'game_starting');
    assert(starting.message.humanCount === 1 && starting.message.aiCount === 2, 'AI seats filled');
    const initial = await f.expectMessage(message => message.type === 'game_state' && message.handId === 1);
    assert(initial.message.isShortDeck === true, 'game_state shortdeck flag');
    assert(initial.message.players.length === 3, 'three seats in shortdeck game');
    if (initial.message.isYourTurn) {
        const state = initial.message;
        const legal = state.legalActions.actions;
        const action = legal.includes('check') ? 'check' : legal.includes('call') ? 'call' : 'fold';
        f.send('player_action:' + action, {
            type: 'player_action',
            requestId: stateRequestId(state),
            roomCode: state.roomCode,
            handId: state.handId,
            turnId: state.turnId,
            expectedStateVersion: state.stateVersion,
            action,
            amount: action === 'call' ? state.legalActions.toCall : 0
        });
        await f.waitAfter(message => message.type === 'action_ack');
    }
    // Wait for an AI decision to appear in a broadcast (aiDelay=3200ms plus AI time).
    try {
        await f.expectMessage(message => message.type === 'game_state' && message.turnId > initial.message.turnId, 12000);
        await f.expectMessage(message => message.type === 'game_state' &&
            message.players.some(player => !player.isHuman && ['fold', 'check', 'call', 'raise', 'allin'].includes(player.lastAction?.action)), 12000);
    } catch (error) {
        console.warn('[golden-protocol] shortdeck AI capture warning:', error.message);
    }
    f.send('leave_room', { type: 'leave_room', requestId: 'leave-2-55c', roomCode: shortRoomCode });
    await f.expectMessage(message => message.type === 'ack' && message.requestId === 'leave-2-55c');
    await closeAll([f]);
}

async function scenarioLimits() {
    const scenario = 'limits';
    const server = await startLocalPokerServer({
        port: 0, host: '127.0.0.1', maxRooms: 1, maxConnections: 8,
        maxConnectionsPerIp: 8, maxMessagesPerWindow: 20, rateWindowMs: 10000
    });
    const url = 'ws://127.0.0.1:' + server.address().port;
    const clients = [];
    try {
        const first = await connectClient(url, scenario, 'limits-first');
        clients.push(first);
        await first.waitForType('hello');
        first.send('create_room', { type: 'create_room', requestId: 'create-1-lim', name: 'One', ...roomOptions() });
        await first.waitAfter(message => message.type === 'room_created');

        const second = await connectClient(url, scenario, 'limits-second');
        clients.push(second);
        await second.waitForType('hello');
        let waiter = second.waitAfter(message => message.type === 'error');
        second.send('create_room', { type: 'create_room', requestId: 'create-limit', name: 'Two', ...roomOptions() });
        const roomFull = await waiter;
        assert(/房间已满（最多 1 个）/.test(roomFull.message.message), 'create when rooms full');

        waiter = second.waitAfter(message => message.type === 'error');
        second.send('join_room', { type: 'join_room', requestId: 'join-missing-room', roomCode: '2', name: 'Two', ...roomOptions() });
        const roomLimit = await waiter;
        assert(roomLimit.message.code === 'ROOM_LIMIT', 'join missing room when full → ROOM_LIMIT');

        // Message rate limit: the 21st message in the 10s window closes 1008.
        const closed = second.waitForClose(5000);
        const floodPayload = JSON.stringify({ protocolVersion: 2, type: 'unknown_type', requestId: 'flood' });
        for (let index = 0; index < 22; index++) second.sendRaw(floodPayload);
        const close = await closed;
        assert(close.code === 1008, 'rate limit close code');
        assert(/rate/.test(close.reason), 'rate limit close reason');
        corpus.closeEvents.push({ scenario, kind: 'ws-close', context: 'message-rate-exceeded', code: close.code, reason: close.reason });
    } finally {
        await closeAll(clients);
        await server.close();
    }
}

async function scenarioCapacity() {
    const scenario = 'capacity';
    const server = await startLocalPokerServer({
        port: 0, host: '127.0.0.1', maxConnections: 8, maxConnectionsPerIp: 2
    });
    const url = 'ws://127.0.0.1:' + server.address().port;
    const clients = [];
    try {
        const first = await connectClient(url, scenario, 'capacity-1');
        clients.push(first);
        await first.waitForType('hello');
        const second = await connectClient(url, scenario, 'capacity-2');
        clients.push(second);
        await second.waitForType('hello');
        const third = new WebSocket(url);
        const closed = new Promise(resolve => third.once('close', (code, reason) => resolve({ code, reason: reason.toString('utf8') })));
        third.on('error', () => {});
        const close = await closed;
        assert(close.code === 1013, 'capacity close code');
        corpus.closeEvents.push({ scenario, kind: 'ws-close', context: 'server-capacity-reached', code: close.code, reason: close.reason });
    } finally {
        await closeAll(clients);
        await server.close();
    }
}

async function scenarioPayload() {
    const scenario = 'payload';
    const server = await startLocalPokerServer({ port: 0, host: '127.0.0.1' });
    const url = 'ws://127.0.0.1:' + server.address().port;
    const clients = [];
    try {
        const client = await connectClient(url, scenario, 'payload-sender');
        clients.push(client);
        await client.waitForType('hello');
        const closed = client.waitForClose(5000);
        client.sendRaw('x'.repeat(17 * 1024));
        const close = await closed;
        assert(close.code === 1009, 'oversized message close code: ' + close.code);
        corpus.closeEvents.push({ scenario, kind: 'ws-close', context: 'message-too-large', code: close.code, reason: close.reason });
    } finally {
        await closeAll(clients);
        await server.close();
    }
}

async function scenarioOrigin() {
    const scenario = 'origin';
    const server = await startLocalPokerServer({ port: 0, host: '127.0.0.1', allowedOrigins: 'https://cards.example' });
    const url = 'ws://127.0.0.1:' + server.address().port;
    const clients = [];
    try {
        const rejected = new WebSocket(url, { origin: 'https://untrusted.example' });
        const rejection = await new Promise(resolve => {
            rejected.once('unexpected-response', (_request, response) => { response.resume(); resolve(response.statusCode); });
            rejected.once('error', () => resolve(403));
        });
        corpus.closeEvents.push({ scenario, kind: 'http-upgrade', context: 'disallowed-origin', statusCode: rejection });
        const allowed = await connectClient(url, scenario, 'origin-allowed', { origin: 'https://cards.example' });
        clients.push(allowed);
        await allowed.waitForType('hello');
    } finally {
        await closeAll(clients);
        await server.close();
    }
}

async function scenarioProbe() {
    const scenario = 'hello-probe';
    const source = fs.readFileSync(path.join(REPO, 'main.js'), 'utf8');
    const start = source.indexOf('function probePokerServer(');
    const end = source.indexOf('\nasync function ensureLocalServer', start);
    assert(start >= 0 && end > start, 'probePokerServer source slice');
    const context = vm.createContext({ WebSocket, setTimeout, clearTimeout, APP_VERSION });
    vm.runInContext(source.slice(start, end), context);
    const cases = [
        { appVersion: APP_VERSION, serverId: 'texas-holdem-game', protocolVersion: 2 },
        { appVersion: '1.5.7', serverId: 'texas-holdem-game', protocolVersion: 2 },
        { serverId: 'texas-holdem-game', protocolVersion: 2 },
        { appVersion: APP_VERSION, serverId: 'other-service', protocolVersion: 2 },
        { appVersion: APP_VERSION, serverId: 'texas-holdem-game', protocolVersion: 3 },
        { appVersion: APP_VERSION, serverId: 'texas-holdem-game', protocolVersion: 2, type: 'not_hello' }
    ];
    for (const hello of cases) {
        const server = new WebSocketServer({ port: 0, host: '127.0.0.1' });
        await new Promise(resolve => server.once('listening', resolve));
        server.on('connection', socket => socket.send(JSON.stringify({ type: 'hello', ...hello })));
        let expected;
        try {
            expected = await context.probePokerServer('ws://127.0.0.1:' + server.address().port);
        } finally {
            for (const socket of server.clients) socket.terminate();
            await new Promise(resolve => server.close(resolve));
        }
        corpus.probeCases.push({ scenario, hello: { type: 'hello', ...hello }, expected });
    }
}

// ---------------------------------------------------------------------------
// 4. Config / name golden cases (pure helpers of server/local-server.js).
// ---------------------------------------------------------------------------
function collectConfigCases() {
    const inputs = [
        {},
        { maxPlayers: 2, isShortDeck: false, startingStack: 5000, smallBlind: 10, bigBlind: 20, ante: 10, minBet: 20, aiDelay: 800 },
        { maxPlayers: 3, isShortDeck: true, startingStack: 20000, smallBlind: 40, bigBlind: 80, aiDelay: 3200 },
        { maxPlayers: 99, startingStack: 12345, smallBlind: 17, bigBlind: 34, aiDelay: 1800 },
        { maxPlayers: 1, startingStack: 100000, smallBlind: 200, bigBlind: 400, ante: 200, minBet: 400, aiDelay: 999999 },
        { isShortDeck: true, startingStack: 20000, smallBlind: 40, bigBlind: 80 },
        { isShortDeck: 1, startingStack: 50000, smallBlind: 100, bigBlind: 200, aiDelay: 0 },
        { startingStack: '5000', smallBlind: '10', bigBlind: '20', maxPlayers: '2', aiDelay: '800' },
        { startingStack: null, smallBlind: null, bigBlind: null, maxPlayers: null, isShortDeck: null, aiDelay: null },
        { startingStack: true, smallBlind: true, bigBlind: true, aiDelay: true },
        { startingStack: 20000.9, smallBlind: 40.9, bigBlind: 80.9, aiDelay: 800.9 },
        { maxPlayers: 0, startingStack: 0, smallBlind: 0, bigBlind: 0, aiDelay: -1 },
        { maxPlayers: -5, startingStack: 1e12, smallBlind: 1e9, bigBlind: -1e9, aiDelay: 1e9 },
        { startingStack: 'abc', smallBlind: 'abc', bigBlind: 'abc', aiDelay: 'abc' },
        { startingStack: [], smallBlind: [40], bigBlind: [80], aiDelay: [800] },
        { startingStack: {}, smallBlind: {}, bigBlind: {}, aiDelay: {} },
        { isShortDeck: 'false', ant: 1 },
        { ante: 40, minBet: 80 },
        { ante: 40, minBet: 80, smallBlind: 0, bigBlind: 0 },
        { aiDelay: 450 }
    ];
    for (const input of inputs) {
        for (const fallbackAiDelay of [1800, 450, 0]) {
            corpus.configCases.push({ scenario: 'config', input, fallbackAiDelay, output: sanitizeConfig(input, fallbackAiDelay) });
        }
    }
}

function collectNameCases() {
    const inputs = [
        'Alice', '  Bob  ', '<b>Alice</b>', '玩家', '玩家玩家玩家玩家玩家', '123456789',
        '', ' ', '\u0000A\u0001B', 'ＡＢＣ', 'Ｔｏｍ', '😀😀😀😀😀😀😀😀😀', '😀', '👤👤👤👤👤👤👤👤👤',
        '①②③④⑤⑥⑦⑧⑨', 'ﬁle', 'ẛ̣', 'A\tB\nC', 'Alice👤', 'a\u007fb', null, 42, 0, false, true,
        'Ｋｉｎｇ', 'Ωμέγα', 'ЖЖЖЖЖЖЖЖЖ', 'x'.repeat(300), 'Ａ'.repeat(10)
    ];
    for (const input of inputs) {
        corpus.nameCases.push({ scenario: 'name', input, output: normalizeName(input) });
    }
}

// ---------------------------------------------------------------------------
// 5. Server defaults / constants / client behaviour / parse errors.
// ---------------------------------------------------------------------------
async function collectServerConstants() {
    const source = fs.readFileSync(path.join(REPO, 'server', 'local-server.js'), 'utf8');
    const constant = name => {
        const match = source.match(new RegExp('const ' + name + ' = ([^;]+);'));
        if (!match) throw new Error('constant not found: ' + name);
        return vm.runInNewContext('(' + match[1] + ')');
    };
    const server = await startLocalPokerServer({ port: 0, host: '127.0.0.1' });
    let defaults;
    try {
        defaults = {
            aiDelay: server.aiDelay,
            disconnectGraceMs: server.disconnectGraceMs,
            maxRooms: server.maxRooms,
            maxConnections: server.maxConnections,
            maxConnectionsPerIp: server.maxConnectionsPerIp,
            rateWindowMs: server.rateWindowMs,
            maxMessagesPerWindow: server.maxMessagesPerWindow,
            trustProxy: server.trustProxy,
            allowedOrigins: [...server.allowedOrigins].sort()
        };
    } finally {
        await server.close();
    }
    return {
        scenario: 'constants',
        referenceCommit: REFERENCE_COMMIT,
        appVersion: APP_VERSION,
        protocolVersion: serverModule.PROTOCOL_VERSION,
        serverId: 'texas-holdem-game',
        serverName: '德州扑克服务',
        emptyRoomTtlMs: constant('EMPTY_ROOM_TTL_MS'),
        maxNameLength: constant('MAX_NAME_LENGTH'),
        rxMaxPayloadBytes: 16 * 1024,
        heartbeatIntervalMs: 15000,
        nextHandDelayMs: 250,
        roomCodes: ['1', '2', '3', '4', '5'],
        stackLevels: constant('STACK_LEVELS'),
        blindLevels: constant('BLIND_LEVELS'),
        aiDelayLevels: constant('AI_DELAY_LEVELS'),
        defaults
    };
}

function collectParseErrors(url) {
    // Raw payloads mapped to the exact error the server answers. These pin the
    // JS validation order (type -> protocolVersion -> dispatch) and its messages.
    const cases = [
        { label: 'invalid-json', raw: 'not-json{', expected: { kind: 'error', code: 'INVALID_REQUEST', message: '消息格式无效', requestId: null } },
        { label: 'missing-type', raw: JSON.stringify({ requestId: 'missing-type' }), expected: { kind: 'error', code: 'INVALID_REQUEST', message: '缺少消息类型', requestId: 'missing-type' } },
        { label: 'null-message', raw: 'null', expected: { kind: 'error', code: 'INVALID_REQUEST', message: '缺少消息类型', requestId: null } },
        { label: 'array-message', raw: '[]', expected: { kind: 'error', code: 'INVALID_REQUEST', message: '缺少消息类型', requestId: null } },
        { label: 'scalar-message', raw: '42', expected: { kind: 'error', code: 'INVALID_REQUEST', message: '缺少消息类型', requestId: null } },
        { label: 'string-message', raw: '"hello"', expected: { kind: 'error', code: 'INVALID_REQUEST', message: '缺少消息类型', requestId: null } },
        { label: 'non-string-type', raw: JSON.stringify({ type: 5, requestId: 'non-string-type' }), expected: { kind: 'error', code: 'INVALID_REQUEST', message: '缺少消息类型', requestId: 'non-string-type' } },
        { label: 'unknown-type', raw: JSON.stringify({ protocolVersion: 2, type: 'nope', requestId: 'unknown-type' }), expected: { kind: 'error', code: 'INVALID_REQUEST', message: '未知消息类型', requestId: 'unknown-type' } },
        { label: 'version-3', raw: JSON.stringify({ protocolVersion: 3, type: 'get_snapshot', requestId: 'version-3' }), expected: { kind: 'error', code: 'PROTOCOL_MISMATCH', message: '协议版本不匹配（服务端 v2）', requestId: 'version-3' } },
        { label: 'version-string-3', raw: JSON.stringify({ protocolVersion: '3', type: 'get_snapshot', requestId: 'version-string-3' }), expected: { kind: 'error', code: 'PROTOCOL_MISMATCH', message: '协议版本不匹配（服务端 v2）', requestId: 'version-string-3' } },
        { label: 'version-bool', raw: JSON.stringify({ protocolVersion: true, type: 'get_snapshot', requestId: 'version-bool' }), expected: { kind: 'error', code: 'PROTOCOL_MISMATCH', message: '协议版本不匹配（服务端 v2）', requestId: 'version-bool' } },
        { label: 'version-null', raw: JSON.stringify({ protocolVersion: null, type: 'get_snapshot', requestId: 'version-null' }), expected: { kind: 'error', code: 'PROTOCOL_MISMATCH', message: '协议版本不匹配（服务端 v2）', requestId: 'version-null' } },
        { label: 'version-string-2', raw: JSON.stringify({ protocolVersion: '2', type: 'get_snapshot', requestId: 'version-string-2' }), expected: { kind: 'parsed', dispatchError: '尚未进入房间' } },
        { label: 'version-absent', raw: JSON.stringify({ type: 'get_snapshot', requestId: 'version-absent' }), expected: { kind: 'parsed', dispatchError: '尚未进入房间' } },
        { label: 'version-array-2', raw: JSON.stringify({ protocolVersion: [2], type: 'get_snapshot', requestId: 'version-array-2' }), expected: { kind: 'parsed', dispatchError: '尚未进入房间' } },
        { label: 'version-empty-array', raw: JSON.stringify({ protocolVersion: [], type: 'get_snapshot', requestId: 'version-empty-array' }), expected: { kind: 'error', code: 'PROTOCOL_MISMATCH', message: '协议版本不匹配（服务端 v2）', requestId: 'version-empty-array' } },
        { label: 'version-string-spaces', raw: JSON.stringify({ protocolVersion: ' 2 ', type: 'get_snapshot', requestId: 'version-string-spaces' }), expected: { kind: 'parsed', dispatchError: '尚未进入房间' } },
        { label: 'missing-type-empty-request-id', raw: JSON.stringify({ requestId: '' }), expected: { kind: 'error', code: 'INVALID_REQUEST', message: '缺少消息类型', requestId: null } }
    ];
    return (async () => {
        const client = await connectClient(url, 'parse-errors', 'parse-errors');
        await client.waitForType('hello');
        const results = [];
        for (const testCase of cases) {
            const pending = client.waitAfter(message => message.type === 'error' || message.type === 'room_joined', 4000);
            client.sendRaw(testCase.raw);
            const response = await pending;
            const message = response.message;
            if (testCase.expected.kind === 'error') {
                assert(message.type === 'error', testCase.label + ': expected error');
                assert(message.code === testCase.expected.code, testCase.label + ': code mismatch ' + message.code);
                assert(message.message === testCase.expected.message, testCase.label + ': message mismatch ' + message.message);
                assert((message.requestId ?? null) === testCase.expected.requestId, testCase.label + ': requestId mismatch ' + message.requestId);
                results.push({ scenario: 'parse-errors', label: testCase.label, raw: testCase.raw, expected: testCase.expected, actual: message });
            } else {
                assert(message.type === 'error' && message.message === testCase.expected.dispatchError,
                    testCase.label + ': expected dispatch error ' + testCase.expected.dispatchError + ', got ' + JSON.stringify(message));
                results.push({ scenario: 'parse-errors', label: testCase.label, raw: testCase.raw, expected: testCase.expected, actual: message });
            }
        }
        await closeAll([client]);
        return results;
    })();
}

function collectClientBehaviour() {
    const source = fs.readFileSync(path.join(REPO, 'js', 'network.js'), 'utf8');
    const sockets = [];
    const timers = [];
    class FakeWebSocket {
        constructor(url) { this.url = url; this.readyState = 1; this.sent = []; sockets.push(this); }
        send(payload) { this.sent.push(JSON.parse(payload)); }
        close(code, reason) { this.closedWith = { code, reason }; }
    }
    FakeWebSocket.OPEN = 1;
    const sessionStore = {};
    const mathProxy = new Proxy(Math, {
        get: (target, property) => (property === 'random' ? () => 0.5 : target[property])
    });
    const context = {
        console: { log() {}, warn() {}, error() {} },
        JSON, Date, Number, String, Boolean, Object, Array, Promise, Error, TypeError, Symbol,
        parseInt, parseFloat, isNaN, Math: mathProxy,
        setTimeout: (fn, ms) => { timers.push({ fn, ms }); return timers.length; },
        clearTimeout: () => {},
        WebSocket: FakeWebSocket,
        sessionStorage: {
            getItem: key => (key in sessionStore ? sessionStore[key] : null),
            setItem: (key, value) => { sessionStore[key] = String(value); },
            removeItem: key => { delete sessionStore[key]; }
        }
    };
    context.window = context;
    context.self = context;
    context.globalThis = context;
    vm.createContext(context);
    vm.runInContext(source, context, { filename: 'js/network.js' });
    const terminalCloseCodes = Array.from(vm.runInContext('[...TERMINAL_CLOSE_CODES].sort((a, b) => a - b)', context));
    const sessionStorageKeyMatch = source.match(/const NETWORK_SESSION_KEY = '([^']+)'/);
    assert(!!sessionStorageKeyMatch, 'session storage key');
    const requestIdPrefixes = [...source.matchAll(/nextRequestId\('([\w-]+)'\)/g)].map(match => match[1]);
    const backoffMatch = source.match(/Math\.min\((\d+), (\d+) \* \(2 \*\* Math\.min\(network\.reconnectAttempts, (\d+)\)\)\)/);
    assert(!!backoffMatch, 'reconnect backoff source');
    const delays = [];
    vm.runInContext(`
        network.shouldReconnect = true;
        network.resumeToken = 'resume-token';
        network.roomCode = '1';
        this.__infos = [];
        onDisconnected = info => this.__infos.push(info);
        connectToServer('ws://127.0.0.1:3000');
    `, context);
    for (let attempt = 0; attempt < 6; attempt++) {
        const socket = sockets[sockets.length - 1];
        const before = timers.length;
        socket.onclose({ code: 1006 });
        if (timers.length <= before) break;
        const scheduled = timers[timers.length - 1];
        delays.push({ attempt, delayMs: scheduled.ms });
        scheduled.fn();
    }
    return {
        scenario: 'client-behaviour',
        terminalCloseCodes,
        sessionStorageKey: sessionStorageKeyMatch[1],
        requestIdPrefixes: [...new Set(requestIdPrefixes)].sort(),
        reconnectDelayCapMs: Number(backoffMatch[1]),
        reconnectBaseDelayMs: Number(backoffMatch[2]),
        reconnectMaxDoublings: Number(backoffMatch[3]),
        reconnectJitterMs: 350,
        reconnectDelaysMs: delays,
        protocolVersionInSend: 2,
        sendMessage: 'renderOnlineGame'
    };
}

// ---------------------------------------------------------------------------
// 6. Write the corpus.
// ---------------------------------------------------------------------------
function writeJsonl(name, rows) {
    const file = path.join(OUT, name);
    fs.writeFileSync(file, rows.map(row => JSON.stringify(row)).join('\n') + (rows.length ? '\n' : ''));
    return rows.length;
}

function writeJson(name, value) {
    const file = path.join(OUT, name);
    fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n');
    return file;
}

async function main() {
    fs.mkdirSync(OUT, { recursive: true });
    const started = Date.now();
    const servers = [];
    const mainServer = await startLocalPokerServer({
        host: '127.0.0.1', port: 0, aiDelay: 0, disconnectGraceMs: 1500,
        maxRooms: 5, maxConnections: 64, maxConnectionsPerIp: 64,
        rateWindowMs: 60000, maxMessagesPerWindow: 500
    });
    servers.push(mainServer);
    const url = 'ws://127.0.0.1:' + mainServer.address().port;
    try {
        await scenarioLobby(url);
        await scenarioStandardTwoHumans(url);
        await scenarioResume(url);
        await scenarioShortDeckAi(url);
        await scenarioLimits();
        await scenarioCapacity();
        await scenarioPayload();
        await scenarioOrigin();
        await scenarioProbe();
        collectConfigCases();
        collectNameCases();
        const parseErrors = await collectParseErrors(url);
        const constants = await collectServerConstants();
        const clientBehaviour = collectClientBehaviour();
        await closeAll(capturedClients);

        // Every protocol message type must be represented in the corpus.
        const expectedS2c = ['hello', 'room_created', 'room_joined', 'player_joined', 'player_left', 'host_changed',
            'game_starting', 'game_state', 'action_ack', 'ready_status', 'ack', 'error'];
        const expectedC2s = ['create_room', 'join_room', 'leave_room', 'start_game', 'player_action', 'ready_for_next', 'resume_session', 'get_snapshot'];
        const missingS2c = expectedS2c.filter(type => !typeCounts.s2c[type]);
        const missingC2s = expectedC2s.filter(type => !typeCounts.c2s[type]);
        if (missingS2c.length || missingC2s.length) {
            throw new Error('corpus incomplete; missing s2c=' + JSON.stringify(missingS2c) + ' c2s=' + JSON.stringify(missingC2s));
        }

        const knownC2sTypes = ['create_room', 'join_room', 'leave_room', 'start_game', 'player_action', 'ready_for_next', 'resume_session', 'get_snapshot'];
        const s2cCount = writeJsonl('s2c-messages.jsonl', corpus.s2c);
        const c2sCount = writeJsonl('c2s-messages.jsonl', corpus.c2s.filter(entry => knownC2sTypes.includes(entry.message.type)));
        const parseErrorCount = writeJsonl('parse-errors.jsonl', parseErrors);
        const configCount = writeJsonl('config-cases.jsonl', corpus.configCases);
        const nameCount = writeJsonl('name-cases.jsonl', corpus.nameCases);
        const closeCount = writeJsonl('close-events.jsonl', corpus.closeEvents);
        const probeCount = writeJsonl('hello-probe-cases.jsonl', corpus.probeCases);
        writeJson('server-constants.json', constants);
        writeJson('client-behaviour.json', clientBehaviour);
        writeJson('meta.json', {
            referenceCommit: REFERENCE_COMMIT,
            protocolVersion: serverModule.PROTOCOL_VERSION,
            appVersion: APP_VERSION,
            seed: SEED,
            nodeVersion: process.version,
            generatedBy: 'oracle/golden-protocol.js',
            counts: {
                s2cMessages: s2cCount,
                c2sMessages: c2sCount,
                parseErrors: parseErrorCount,
                configCases: configCount,
                nameCases: nameCount,
                closeEvents: closeCount,
                probeCases: probeCount
            },
            s2cTypeCounts: typeCounts.s2c,
            c2sTypeCounts: typeCounts.c2s,
            notes: [
                's2c messages were captured from the real reference server; c2s messages are exactly the payloads the old browser client shape sends (protocolVersion prepended).',
                'The server runs with Math.random / performance.now / crypto.randomUUID / crypto.randomBytes patched so the corpus is reproducible.',
                'roomConfig.aiDelay can be 0 in the shortdeck/game AI room only via the server fallback path; the reference whitelist is [800, 1800, 3200].',
                'player_disconnected is handled by js/network.js but never emitted by the reference server; it is deliberately absent from the corpus.'
            ]
        });
        console.log('golden protocol corpus written to', OUT);
        console.log('s2c messages:', s2cCount, 'c2s messages:', c2sCount, 'parse errors:', parseErrorCount);
        console.log('config cases:', configCount, 'name cases:', nameCount, 'close events:', JSON.stringify(corpus.closeEvents));
        console.log('elapsed ms:', Date.now() - started);
    } finally {
        for (const client of capturedClients) client.close();
        for (const server of servers) {
            try { await server.close(); } catch (_) {}
        }
    }
}

main().catch(error => {
    console.error('[golden-protocol] failed:', error && error.stack ? error.stack : error);
    process.exitCode = 1;
});
