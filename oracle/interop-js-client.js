#!/usr/bin/env node
'use strict';
// Interop client for the v2 protocol: drives a Rust poker-server with the
// reference JS client's wire behaviour (the same JSON frames js/network.js
// produces) and writes a transcript JSON. Used by
// crates/poker-server/tests/interop.rs as the Phase 5 compatibility gate.
//
// Usage:
//   node oracle/interop-js-client.js --url ws://127.0.0.1:PORT \
//        --transcript /tmp/interop-js.json [--expected-app-version 1.5.8]
//
// Exit code 0 means every step passed. The transcript records the checks, the
// observed hand results and per-type message counts for evidence.

const fs = require('node:fs');
const path = require('node:path');

function referenceRepo() {
    const env = process.env.REFERENCE_REPO;
    if (env) return path.resolve(env);
    return path.resolve(__dirname, '..', '..', 'reference', 'texas-holdem-game');
}

function parseArgs(argv) {
    const options = { transcript: path.resolve('target/interop-js-transcript.json') };
    for (let index = 0; index < argv.length; index++) {
        const flag = argv[index];
        if (flag === '--url') options.url = argv[++index];
        else if (flag === '--transcript') options.transcript = path.resolve(argv[++index]);
        else if (flag === '--expected-app-version') options.expectedAppVersion = argv[++index];
    }
    if (!options.url) throw new Error('missing --url');
    if (!options.expectedAppVersion) options.expectedAppVersion = '1.5.8';
    return options;
}

const reference = referenceRepo();
const wsModule = require(path.join(reference, 'node_modules', 'ws'));
const WebSocket = wsModule.WebSocket;

const PROTOCOL_VERSION = 2;
const DEFAULT_TIMEOUT = 8000;

class Client {
    constructor(label) {
        this.label = label;
        this.ws = null;
        this.messages = [];
        this.counts = {};
        this.waiters = [];
        this.closed = null;
    }

    connect(url) {
        return new Promise((resolve, reject) => {
            const ws = new WebSocket(url);
            this.ws = ws;
            ws.on('message', raw => this._onMessage(raw));
            ws.on('close', (code, reason) => {
                this.closed = { code, reason: reason.toString('utf8') };
                for (const waiter of [...this.waiters]) this._resolveCloseWaiters(waiter, code, reason.toString('utf8'));
            });
            ws.on('error', () => {});
            ws.once('open', resolve);
            ws.once('error', reject);
        });
    }

    _onMessage(raw) {
        let message;
        try { message = JSON.parse(raw.toString('utf8')); } catch (_) { return; }
        this.messages.push(message);
        this.counts[message.type] = (this.counts[message.type] || 0) + 1;
        for (const waiter of [...this.waiters]) {
            if (waiter.kind !== 'message') continue;
            if (!waiter.predicate(message)) continue;
            this._remove(waiter);
            waiter.resolve(message);
        }
    }

    _resolveCloseWaiters(waiter, code, reason) {
        if (waiter.kind !== 'close') return;
        this._remove(waiter);
        waiter.resolve({ code, reason });
    }

    _remove(waiter) {
        const index = this.waiters.indexOf(waiter);
        if (index >= 0) this.waiters.splice(index, 1);
        clearTimeout(waiter.timer);
    }

    send(payload) {
        const message = Object.assign({ protocolVersion: PROTOCOL_VERSION }, payload);
        this.ws.send(JSON.stringify(message));
    }

    waitFor(predicate, timeout) {
        const existing = this.messages.find(message => predicate(message));
        if (existing) return Promise.resolve(existing);
        return new Promise((resolve, reject) => {
            const waiter = { kind: 'message', predicate, resolve };
            waiter.timer = setTimeout(() => {
                this._remove(waiter);
                reject(new Error(this.label + ': timeout waiting for message; got ' + JSON.stringify(this.messages.map(m => m.type))));
            }, timeout || DEFAULT_TIMEOUT);
            this.waiters.push(waiter);
        });
    }

    waitClose(timeout) {
        if (this.closed) return Promise.resolve(this.closed);
        return new Promise((resolve, reject) => {
            const waiter = { kind: 'close', resolve };
            waiter.timer = setTimeout(() => {
                this._remove(waiter);
                reject(new Error(this.label + ': timeout waiting for close'));
            }, timeout || DEFAULT_TIMEOUT);
            this.waiters.push(waiter);
        });
    }

    lastState() {
        for (let index = this.messages.length - 1; index >= 0; index--) {
            if (this.messages[index].type === 'game_state') return this.messages[index];
        }
        return null;
    }

    terminate() {
        this.ws.terminate();
    }

    close() {
        try { this.ws.close(1000, 'interop complete'); } catch (_) {}
    }
}

function assert(condition, label) {
    if (!condition) throw new Error('assertion failed: ' + label);
}

const sleep = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));

async function waitTurn(clients, afterTurnId) {
    for (;;) {
        for (const client of clients) {
            const state = client.lastState();
            if (state && state.isYourTurn && (afterTurnId === undefined || state.turnId > afterTurnId)) return { client, state };
        }
        await sleep(10);
    }
}

async function waitSettled(clients) {
    for (;;) {
        for (const client of clients) {
            const state = client.lastState();
            if (state && state.phase === 'idle' && state.lastHandResult) return state;
        }
        await sleep(10);
    }
}

async function act(client, state, action) {
    const requestId = 'interop-' + state.handId + '-' + state.turnId + '-' + action;
    const legal = state.legalActions || { actions: [] };
    const chosen = legal.actions.includes(action) ? action : legal.actions.includes('check') ? 'check' : legal.actions.includes('call') ? 'call' : 'fold';
    const amount = chosen === 'call' ? Number(legal.toCall || 0) : 0;
    client.send({
        type: 'player_action',
        requestId,
        roomCode: state.roomCode,
        handId: state.handId,
        turnId: state.turnId,
        expectedStateVersion: state.stateVersion,
        action: chosen,
        amount
    });
    const ack = await client.waitFor(message => message.type === 'action_ack' && message.requestId === requestId);
    assert(ack.accepted === true, 'action ' + chosen + ' accepted (got ' + JSON.stringify(ack) + ')');
    return ack;
}

async function driveHand(clients, expectedHandId) {
    const started = Date.now();
    for (;;) {
        assert(Date.now() - started < 15000, 'hand ' + expectedHandId + ' did not finish');
        const states = clients.map(client => ({ client, state: client.lastState() })).filter(entry => entry.state);
        const settled = states.find(entry => entry.state.phase === 'idle' && entry.state.lastHandResult && entry.state.handId === expectedHandId);
        if (settled) return settled.state;
        const turn = states.find(entry => entry.state.isYourTurn && entry.state.handId === expectedHandId);
        if (!turn) {
            await sleep(15);
            continue;
        }
        const legal = turn.state.legalActions.actions;
        const action = legal.includes('check') ? 'check' : legal.includes('call') ? 'call' : 'fold';
        await act(turn.client, turn.state, action);
        await sleep(15);
    }
}

async function run(options) {
    const checks = [];
    const transcript = { url: options.url, checks, sequences: {}, results: [], closeCodes: [] };
    const record = (label, detail) => checks.push({ label, detail: detail === undefined ? null : detail });

    const host = new Client('host');
    await host.connect(options.url);
    const hostHello = await host.waitFor(message => message.type === 'hello');
    assert(hostHello.protocolVersion === PROTOCOL_VERSION, 'hello protocolVersion 2');
    assert(hostHello.serverId === 'texas-holdem-game', 'hello serverId');
    assert(hostHello.appVersion === options.expectedAppVersion, 'hello appVersion ' + options.expectedAppVersion + ' (got ' + hostHello.appVersion + ')');
    record('hello', { serverId: hostHello.serverId, appVersion: hostHello.appVersion, serverName: hostHello.serverName });

    const guest = new Client('guest');
    await guest.connect(options.url);
    await guest.waitFor(message => message.type === 'hello');

    const roomOptions = { maxPlayers: 2, isShortDeck: false, startingStack: 20000, smallBlind: 40, bigBlind: 80, ante: 40, minBet: 80, aiDelay: 800 };
    host.send(Object.assign({ type: 'create_room', requestId: 'interop-create-1', name: 'JS-Host' }, roomOptions));
    const created = await host.waitFor(message => message.type === 'room_created');
    assert(created.roomConfig.maxPlayers === 2, 'room config maxPlayers');
    assert(created.roomConfig.aiDelay === 800, 'room config aiDelay');
    assert(typeof created.resumeToken === 'string' && created.resumeToken.length === 24, 'resume token length');
    record('create_room', { roomCode: created.roomCode, playerId: created.playerId });

    guest.send(Object.assign({ type: 'join_room', requestId: 'interop-join-1', roomCode: created.roomCode, name: 'JS-Guest' }, roomOptions));
    const joined = await guest.waitFor(message => message.type === 'room_joined');
    assert(joined.isHost === false, 'guest is not host');
    await host.waitFor(message => message.type === 'player_joined' && message.players && message.players.length === 2);
    record('join_room', { roomCode: joined.roomCode, playerId: joined.playerId });

    host.send({ type: 'start_game', requestId: 'interop-start-1', roomCode: created.roomCode });
    await host.waitFor(message => message.type === 'game_starting');
    const ack = await host.waitFor(message => message.type === 'ack' && message.requestId === 'interop-start-1');
    assert(ack.accepted === true, 'start ack');
    const stateHost = await host.waitFor(message => message.type === 'game_state' && message.handId === 1);
    const stateGuest = await guest.waitFor(message => message.type === 'game_state' && message.handId === 1);
    assert(stateHost.yourCards.length === 2, 'host hole cards');
    assert(stateGuest.yourCards.length === 2, 'guest hole cards');
    void stateGuest;
    assert(stateHost.players.every(player => !Object.prototype.hasOwnProperty.call(player, 'holeCards')), 'no hidden hole cards');
    assert(stateHost.stateVersion === stateGuest.stateVersion, 'state versions agree');
    record('game_started', { handId: stateHost.handId, dealerPos: stateHost.dealerPos, pot: stateHost.pot });

    const settled1 = await driveHand([host, guest], 1);
    assert(settled1.lastHandResult.reason === 'showdown', 'hand 1 reaches showdown (got ' + settled1.lastHandResult.reason + ')');
    assert(settled1.lastHandResult.results.length === 2, 'showdown reveals both players');
    assert(settled1.lastHandResult.results[0].cards[0].value >= 2, 'revealed cards carry value');
    assert(settled1.pot === 0, 'settled pot zero');
    transcript.results.push({ handId: 1, reason: settled1.lastHandResult.reason, pot: settled1.lastHandResult.pot });
    record('hand_1_settled', { reason: settled1.lastHandResult.reason, pot: settled1.lastHandResult.pot });

    host.send({ type: 'ready_for_next', requestId: 'interop-ready-host-1', roomCode: created.roomCode, handId: 1 });
    guest.send({ type: 'ready_for_next', requestId: 'interop-ready-guest-1', roomCode: created.roomCode, handId: 1 });
    await host.waitFor(message => message.type === 'game_state' && message.handId === 2 && message.phase === 'preflop');
    const guestState2 = await guest.waitFor(message => message.type === 'game_state' && message.handId === 2);
    const guestCardsBeforeResume = guestState2.yourCards;
    record('hand_2_started', { handId: 2 });

    // Reconnect: drop the guest socket, then resume the same seat.
    guest.terminate();
    await host.waitFor(message => message.type === 'player_left');
    const guest2 = new Client('guest-resumed');
    await guest2.connect(options.url);
    await guest2.waitFor(message => message.type === 'hello');
    guest2.send({ type: 'resume_session', requestId: 'interop-resume-1', roomCode: created.roomCode, resumeToken: joined.resumeToken });
    const resumed = await guest2.waitFor(message => message.type === 'room_joined' && message.resumed === true);
    assert(resumed.playerId === joined.playerId, 'resume keeps player id');
    const resumedState = await guest2.waitFor(message => message.type === 'game_state' && message.handId === 2);
    assert(JSON.stringify(resumedState.yourCards) === JSON.stringify(guestCardsBeforeResume), 'resume restores hole cards');
    record('resume_session', { playerId: resumed.playerId, handId: resumedState.handId });

    // Resuming again kicks the previous socket with 4001.
    const guest3 = new Client('guest-kicked');
    await guest3.connect(options.url);
    await guest3.waitFor(message => message.type === 'hello');
    guest3.send({ type: 'resume_session', requestId: 'interop-resume-2', roomCode: created.roomCode, resumeToken: joined.resumeToken });
    const kicked = await guest2.waitClose();
    assert(kicked.code === 4001, 'old socket kicked with 4001 (got ' + kicked.code + ')');
    transcript.closeCodes.push({ context: 'resume-elsewhere', code: kicked.code, reason: kicked.reason });
    await guest3.waitFor(message => message.type === 'room_joined' && message.resumed === true);
    record('resume_kicks_old_socket', { code: kicked.code, reason: kicked.reason });

    // Host leaves mid-hand: the retained guest becomes host and wins by fold.
    const guest3Mark = guest3.messages.length;
    host.send({ type: 'leave_room', requestId: 'interop-leave-host', roomCode: created.roomCode });
    const hostChanged = await guest3.waitFor(message => message.type === 'host_changed' && message.isHost === true);
    assert(hostChanged.hostId === joined.playerId, 'guest becomes host');
    await guest3.waitFor(message => message.type === 'player_left');
    const foldState = await guest3.waitFor(message => message.type === 'game_state' && message.phase === 'idle' && message.lastHandResult && message.handId === 2);
    assert(foldState.lastHandResult.reason === 'fold', 'hand 2 ends by fold (got ' + foldState.lastHandResult.reason + ')');
    transcript.results.push({ handId: 2, reason: foldState.lastHandResult.reason, pot: foldState.lastHandResult.pot });
    record('host_migration', { newHostId: hostChanged.hostId, handResult: foldState.lastHandResult.reason });

    // Hands-free AI refill: ready for hand 3, the vacated seat becomes an AI.
    guest3.send({ type: 'ready_for_next', requestId: 'interop-ready-host-2', roomCode: created.roomCode, handId: 2 });
    const hand3 = await guest3.waitFor(message => message.type === 'game_state' && message.handId === 3 && message.phase !== 'idle', 10000);
    const aiSeats = hand3.players.filter(player => player.isHuman === false).length;
    assert(aiSeats === 1, 'vacated seat filled by AI (got ' + aiSeats + ')');
    record('ai_refill', { aiSeats, handId: hand3.handId, phase: hand3.phase });
    assert(guest3.messages.length > guest3Mark, 'guest received host-change messages');

    // Leave cleanly.
    guest3.send({ type: 'leave_room', requestId: 'interop-leave-guest', roomCode: created.roomCode });
    const leaveAck = await guest3.waitFor(message => message.type === 'ack' && message.requestId === 'interop-leave-guest');
    assert(leaveAck.accepted === true, 'leave ack');
    record('leave_room', { accepted: leaveAck.accepted });

    transcript.sequences = { host: host.counts, guest: guest.counts, guestResumed: guest2.counts, guestKicked: guest3.counts };
    host.close();
    guest3.close();
    return transcript;
}

(async () => {
    const options = parseArgs(process.argv.slice(2));
    try {
        const transcript = await run(options);
        transcript.ok = true;
        fs.mkdirSync(path.dirname(options.transcript), { recursive: true });
        fs.writeFileSync(options.transcript, JSON.stringify(transcript, null, 2) + '\n');
        console.log(JSON.stringify({ ok: true, checks: transcript.checks.length, transcript: options.transcript }));
        process.exit(0);
    } catch (error) {
        console.error('[interop-js-client] failed:', error && error.stack ? error.stack : error);
        const failed = { ok: false, error: String(error && error.message ? error.message : error), url: options.url };
        try {
            fs.mkdirSync(path.dirname(options.transcript), { recursive: true });
            fs.writeFileSync(options.transcript, JSON.stringify(failed, null, 2) + '\n');
        } catch (_) {}
        process.exit(1);
    }
})();
