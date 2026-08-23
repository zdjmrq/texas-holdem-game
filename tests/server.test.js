'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { WebSocket } = require('ws');
const { startLocalPokerServer } = require('../server/local-server');

class TestClient {
    constructor(url) {
        this.url = url;
        this.ws = null;
        this.messages = [];
        this.waiters = new Set();
    }

    async connect() {
        this.ws = new WebSocket(this.url);
        this.ws.on('message', raw => {
            const message = JSON.parse(raw.toString('utf8'));
            this.messages.push(message);
            for (const waiter of [...this.waiters]) waiter();
        });
        await new Promise((resolve, reject) => {
            this.ws.once('open', resolve);
            this.ws.once('error', reject);
        });
        return this;
    }

    mark() { return this.messages.length; }

    send(message) {
        this.ws.send(JSON.stringify({ protocolVersion:2, ...message }));
    }

    latest(predicate) {
        for (let index = this.messages.length - 1; index >= 0; index--) {
            if (predicate(this.messages[index])) return this.messages[index];
        }
        return null;
    }

    waitAfter(mark, predicate, timeout = 3000) {
        const find = () => this.messages.slice(mark).find(predicate);
        const existing = find();
        if (existing) return Promise.resolve(existing);
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                this.waiters.delete(check);
                reject(new Error(`Timed out waiting for message; received: ${JSON.stringify(this.messages.slice(mark))}`));
            }, timeout);
            const check = () => {
                const found = find();
                if (!found) return;
                clearTimeout(timer);
                this.waiters.delete(check);
                resolve(found);
            };
            this.waiters.add(check);
        });
    }

    waitForCountAfter(mark, predicate, count, timeout = 3000) {
        const find = () => this.messages.slice(mark).filter(predicate);
        const existing = find();
        if (existing.length >= count) return Promise.resolve(existing);
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                this.waiters.delete(check);
                reject(new Error(`Timed out waiting for ${count} messages; received: ${JSON.stringify(this.messages.slice(mark))}`));
            }, timeout);
            const check = () => {
                const matches = find();
                if (matches.length < count) return;
                clearTimeout(timer);
                this.waiters.delete(check);
                resolve(matches);
            };
            this.waiters.add(check);
        });
    }

    close() {
        if (!this.ws || this.ws.readyState >= WebSocket.CLOSING) return;
        this.ws.close(1000, 'test complete');
    }
}

function joinPayload(roomCode, name, extra = {}) {
    return {
        type:'join_room', requestId:`join-${name}`, roomCode, name,
        maxPlayers:2, isShortDeck:false, startingStack:20000,
        smallBlind:40, bigBlind:80, ante:40, minBet:80,
        ...extra
    };
}

function actionFromState(state, requestId, action, amount = 0, versionOffset = 0) {
    return {
        type:'player_action', requestId, roomCode:state.roomCode,
        handId:state.handId, turnId:state.turnId,
        expectedStateVersion:state.stateVersion + versionOffset,
        action, amount
    };
}

test('local server keeps two clients authoritative, private, resumable and synchronized across hands', async t => {
    const server = await startLocalPokerServer({ port:0, host:'127.0.0.1', aiDelay:0, disconnectGraceMs:700 });
    const address = server.address();
    const url = `ws://127.0.0.1:${address.port}`;
    const clients = [];
    t.after(async () => {
        for (const client of clients) client.close();
        await server.close();
    });

    const a = await new TestClient(url).connect();
    const b = await new TestClient(url).connect();
    clients.push(a, b);

    let markA = a.mark();
    a.send(joinPayload('1', '<b>Alice</b>'));
    const joinedA = await a.waitAfter(markA, message => message.type === 'room_created');
    assert.equal(joinedA.isHost, true);
    assert.ok(joinedA.resumeToken.length >= 20);

    const markB = b.mark();
    b.send(joinPayload('1', 'Bob'));
    const joinedB = await b.waitAfter(markB, message => message.type === 'room_joined');
    assert.equal(joinedB.isHost, false);
    await a.waitAfter(markA, message => message.type === 'player_joined' && message.players?.length === 2);

    let rejectedMark = b.mark();
    b.send({ type:'start_game', requestId:'not-host', roomCode:'1' });
    const hostError = await b.waitAfter(rejectedMark, message => message.type === 'error' && message.requestId === 'not-host');
    assert.match(hostError.message, /房主/);

    markA = a.mark();
    const startMarkB = b.mark();
    a.send({ type:'start_game', requestId:'start-1', roomCode:'1' });
    const [stateA, stateB] = await Promise.all([
        a.waitAfter(markA, message => message.type === 'game_state' && message.handId === 1),
        b.waitAfter(startMarkB, message => message.type === 'game_state' && message.handId === 1)
    ]);
    assert.equal(stateA.stateVersion, stateB.stateVersion);
    assert.equal(stateA.yourCards.length, 2);
    assert.equal(stateB.yourCards.length, 2);
    assert.ok(stateA.players.every(item => !Object.hasOwn(item, 'holeCards')));
    assert.ok(stateB.players.every(item => !Object.hasOwn(item, 'holeCards')));
    assert.notDeepEqual(stateA.yourCards, stateB.yourCards);

    const actor = stateA.isYourTurn ? a : b;
    const actorState = stateA.isYourTurn ? stateA : stateB;
    const staleMark = actor.mark();
    actor.send(actionFromState(actorState, 'stale-action', 'fold', 0, -1));
    const staleAck = await actor.waitAfter(staleMark,
        message => message.type === 'action_ack' && message.requestId === 'stale-action');
    assert.equal(staleAck.accepted, false);

    const duplicateMark = actor.mark();
    const duplicate = actionFromState(actorState, 'duplicate-fold', 'fold');
    actor.send(duplicate);
    actor.send(duplicate);
    const duplicateAcks = await actor.waitForCountAfter(duplicateMark,
        message => message.type === 'action_ack' && message.requestId === 'duplicate-fold', 2);
    assert.ok(duplicateAcks.every(message => message.accepted));

    const [idleA, idleB] = await Promise.all([
        a.waitAfter(markA, message => message.type === 'game_state' && message.handId === 1 && message.phase === 'idle'),
        b.waitAfter(startMarkB, message => message.type === 'game_state' && message.handId === 1 && message.phase === 'idle')
    ]);
    assert.ok(idleA.lastHandResult);
    assert.equal(idleA.pot, 0);
    assert.equal(idleA.players.reduce((sum, item) => sum + item.stack, 0), 40000);
    assert.equal(idleB.lastHandResult.pot, idleA.lastHandResult.pot);

    const nextMarkA = a.mark();
    const nextMarkB = b.mark();
    a.send({ type:'ready_for_next', requestId:'ready-a', roomCode:'1', handId:1 });
    b.send({ type:'ready_for_next', requestId:'ready-b', roomCode:'1', handId:1 });
    const [secondA, secondB] = await Promise.all([
        a.waitAfter(nextMarkA, message => message.type === 'game_state' && message.handId === 2 && message.phase === 'preflop'),
        b.waitAfter(nextMarkB, message => message.type === 'game_state' && message.handId === 2 && message.phase === 'preflop')
    ]);
    for (const state of [secondA, secondB]) {
        assert.deepEqual(state.communityCards, []);
        assert.equal(state.lastHandResult, null);
        assert.equal(state.yourCards.length, 2);
    }

    const oldReadyMark = a.mark();
    a.send({ type:'ready_for_next', requestId:'old-ready', roomCode:'1', handId:1 });
    const oldReadyError = await a.waitAfter(oldReadyMark,
        message => message.type === 'error' && message.requestId === 'old-ready');
    assert.match(oldReadyError.message, /尚未结束|旧牌局/);

    const oldPlayerId = joinedB.playerId;
    const oldToken = joinedB.resumeToken;
    const oldCards = secondB.yourCards;
    const disconnectMarkA = a.mark();
    b.ws.terminate();
    await a.waitAfter(disconnectMarkA, message => message.type === 'player_left');
    const resumed = await new TestClient(url).connect();
    clients.push(resumed);
    const resumeMark = resumed.mark();
    resumed.send({ type:'resume_session', requestId:'resume-b', roomCode:'1', resumeToken:oldToken });
    const resumedJoin = await resumed.waitAfter(resumeMark,
        message => message.type === 'room_joined' && message.resumed === true);
    const resumedState = await resumed.waitAfter(resumeMark,
        message => message.type === 'game_state' && message.handId === 2);
    assert.equal(resumedJoin.playerId, oldPlayerId);
    assert.deepEqual(resumedState.yourCards, oldCards);
    const resumedPlayer = resumedState.players.find(item => item.id === oldPlayerId);
    assert.equal(resumedPlayer.folded, false);
    assert.equal(resumedPlayer.connected, true);

    // The resumed client and the existing client receive the same authoritative
    // version on separate sockets. Wait for both deliveries before acting so
    // the test never submits the previous connection-state version.
    const liveA = await a.waitAfter(disconnectMarkA,
        message => message.type === 'game_state' && message.handId === 2 &&
            message.phase !== 'idle' && message.stateVersion >= resumedState.stateVersion);
    const liveB = resumed.latest(message => message.type === 'game_state' && message.handId === 2 && message.phase !== 'idle');
    const secondActor = liveA?.isYourTurn ? a : resumed;
    const secondActorState = liveA?.isYourTurn ? liveA : liveB;
    assert.ok(secondActorState?.isYourTurn);
    const finishMarkA = a.mark();
    const finishMarkB = resumed.mark();
    secondActor.send(actionFromState(secondActorState, 'finish-second', 'fold'));
    await Promise.all([
        a.waitAfter(finishMarkA, message => message.type === 'game_state' && message.handId === 2 && message.phase === 'idle'),
        resumed.waitAfter(finishMarkB, message => message.type === 'game_state' && message.handId === 2 && message.phase === 'idle')
    ]);

    const hostMark = resumed.mark();
    a.send({ type:'leave_room', requestId:'leave-host', roomCode:'1' });
    const hostChanged = await resumed.waitAfter(hostMark,
        message => message.type === 'host_changed' && message.isHost === true);
    assert.equal(hostChanged.hostId, oldPlayerId);

    const migratedReadyMark = resumed.mark();
    resumed.send({ type:'ready_for_next', requestId:'ready-after-host-left', roomCode:'1', handId:2 });
    const thirdHand = await resumed.waitAfter(migratedReadyMark,
        message => message.type === 'game_state' && message.handId === 3 &&
            (message.isYourTurn || message.phase === 'idle'), 3500);
    assert.equal(thirdHand.players.filter(item => !item.isHuman).length, 1);
    if (thirdHand.phase !== 'idle') {
        const migratedAction = thirdHand.legalActions.actions.includes('fold') ? 'fold' : 'check';
        const migratedActionMark = resumed.mark();
        resumed.send(actionFromState(thirdHand, 'action-after-host-change', migratedAction));
        const migratedAck = await resumed.waitAfter(migratedActionMark,
            message => message.type === 'action_ack' && message.requestId === 'action-after-host-change');
        assert.equal(migratedAck.accepted, true);
    } else {
        assert.equal(thirdHand.lastHandResult?.reason, 'fold');
    }

    // A current actor that does not return within the grace period is folded by
    // the server, so the other client is never left in a permanently stuck hand.
    const d = await new TestClient(url).connect();
    const e = await new TestClient(url).connect();
    clients.push(d, e);
    let markD = d.mark();
    d.send(joinPayload('3', 'D'));
    await d.waitAfter(markD, message => message.type === 'room_created');
    const markE = e.mark();
    e.send(joinPayload('3', 'E'));
    await e.waitAfter(markE, message => message.type === 'room_joined');
    markD = d.mark();
    const startE = e.mark();
    d.send({ type:'start_game', requestId:'start-3', roomCode:'3' });
    const [stateD, stateE] = await Promise.all([
        d.waitAfter(markD, message => message.type === 'game_state' && message.handId === 1),
        e.waitAfter(startE, message => message.type === 'game_state' && message.handId === 1)
    ]);
    const disconnectedActor = stateD.isYourTurn ? d : e;
    const survivor = stateD.isYourTurn ? e : d;
    const survivorMark = survivor.mark();
    disconnectedActor.ws.terminate();
    const timeoutResult = await survivor.waitAfter(survivorMark,
        message => message.type === 'game_state' && message.phase === 'idle' && message.lastHandResult, 3500);
    assert.equal(timeoutResult.lastHandResult.reason, 'fold');

    // Short-deck mode is also driven by the same authoritative protocol.
    const c = await new TestClient(url).connect();
    clients.push(c);
    const markC = c.mark();
    c.send(joinPayload('2', 'Short', { isShortDeck:true }));
    const joinedC = await c.waitAfter(markC, message => message.type === 'room_created');
    assert.equal(joinedC.roomConfig.isShortDeck, true);
    const shortStart = c.mark();
    c.send({ type:'start_game', requestId:'start-short', roomCode:'2' });
    const shortState = await c.waitAfter(shortStart,
        message => message.type === 'game_state' && message.handId === 1, 3500);
    assert.equal(shortState.isShortDeck, true);
    assert.equal(shortState.yourCards.length, 2);
    assert.ok(shortState.players.every(item => !Object.hasOwn(item, 'holeCards')));
});
