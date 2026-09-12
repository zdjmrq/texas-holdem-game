/**
 * UI layer + client network regression tests (task-2 scope).
 * js/ui.js, js/network.js, css/style.css have no browser runtime in this repo,
 * so ui.js runs inside a vm with a small DOM shim; network.js runs with a fake WebSocket.
 */
const { test } = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const vm = require('vm');
const path = require('path');

const ROOT = path.join(__dirname, '..');
const read = rel => fs.readFileSync(path.join(ROOT, rel), 'utf8');

// ─────────────────────────────────────────────────────────────
// network.js — close-code handling
// ─────────────────────────────────────────────────────────────
function loadNetwork() {
    const sockets = [];
    const timers = [];
    function FakeWebSocket(url) { this.url = url; this.readyState = 1; this.sent = []; sockets.push(this); }
    FakeWebSocket.OPEN = 1;
    FakeWebSocket.prototype.send = function (payload) { this.sent.push(JSON.parse(payload)); };
    FakeWebSocket.prototype.close = function (code, reason) { this.closedWith = { code, reason }; };

    const sessionStore = {};
    const errors = [];
    const context = {
        console: { log() {}, warn() {}, error() {} },
        JSON, Math, Date, Number, String, Boolean, Object, Array, Promise, Error, TypeError, Symbol, parseInt, parseFloat, isNaN,
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
    vm.runInContext(read('js/network.js'), context, { filename: 'js/network.js' });
    vm.runInContext('showNetworkError = function (message) { this.__errors.push(String(message)); };', context);
    context.__errors = errors;
    return { context, sockets, timers, errors };
}

/** Connect, then fire onclose with the given code; return {reconnects, info}. */
function closeWith(code) {
    const env = loadNetwork();
    const { context, sockets, timers } = env;
    vm.runInContext(`
        network.shouldReconnect = true;
        network.resumeToken = 'resume-token';
        network.roomCode = '1234';
        this.__info = [];
        onDisconnected = function (info) { this.__info.push(info); };
        connectToServer('ws://localhost:9999');
    `, context);
    const socket = sockets[sockets.length - 1];
    const before = timers.length;
    socket.onclose({ code });
    const reconnects = timers.length - before;
    return { reconnects, info: context.__info[0], socket };
}

test('client network: 1008/4001/1013 (and 1000) are terminal and must not auto-reconnect', () => {
    for (const code of [1000, 1008, 1009, 1013, 4001]) {
        const { reconnects, info } = closeWith(code);
        assert.equal(reconnects, 0, 'close code ' + code + ' must not schedule an auto-reconnect');
        assert.equal(info.willReconnect, false, 'close code ' + code + ' must report willReconnect=false');
        assert.equal(info.code, code);
    }
});

test('client network: transport failures still auto-reconnect', () => {
    for (const code of [1006, 1011, 1005]) {
        const { reconnects, info } = closeWith(code);
        assert.equal(reconnects, 1, 'close code ' + code + ' should schedule exactly one reconnect');
        assert.equal(info.willReconnect, true);
        assert.ok(info.reconnectDelay > 0);
    }
});

test('client network: manual reconnect entry point stays available', () => {
    const env = loadNetwork();
    vm.runInContext(`
        network.serverUrl = 'ws://localhost:9999';
        network.resumeToken = 'resume-token';
        network.roomCode = '1234';
        reconnectToServer('ws://localhost:9998');
    `, env.context);
    const retryTimers = env.timers.filter(timer => timer.ms === 150);
    assert.equal(retryTimers.length, 1, 'reconnectToServer must schedule a manual retry');
});

// ─────────────────────────────────────────────────────────────
// ui.js — DOM shim environment
// ─────────────────────────────────────────────────────────────
function createDomShim() {
    const counts = { createElement: 0, appendChild: 0, innerHTML: 0, textContent: 0, offsetWidth: 0, offsetHeight: 0, innerHTMLSites: {} };
    const makeEl = (tag, id) => {
        const el = {
            tagName: String(tag || 'div').toUpperCase(), id: id || '', children: [], parentNode: null,
            _text: '', _html: '', _cls: '', dataset: {}, attrs: {},
            _ow: 900, _oh: 420, isConnected: true, value: '50', checked: false, disabled: false,
            title: '', hidden: false, style: {},
            classList: {
                _s: new Set(),
                add(...c) { c.forEach(x => this._s.add(x)); el._cls = [...this._s].join(' '); },
                remove(...c) { c.forEach(x => this._s.delete(x)); el._cls = [...this._s].join(' '); },
                toggle(c, f) { if (f === undefined) f = !this._s.has(c); f ? this._s.add(c) : this._s.delete(c); el._cls = [...this._s].join(' '); },
                contains(c) { return this._s.has(c); }
            },
            get innerHTML() { return this._html; },
            set innerHTML(v) {
                counts.innerHTML++;
                const key = this.id || this._cls || this.tagName;
                counts.innerHTMLSites[key] = (counts.innerHTMLSites[key] || 0) + 1;
                this._html = String(v);
                this.children.length = 0;
            },
            get textContent() { return this._text; },
            set textContent(v) { counts.textContent++; this._text = String(v); },
            get className() { return this._cls; },
            set className(v) { this._cls = String(v); this.classList._s = new Set(String(v).split(/\\s+/).filter(Boolean)); },
            get offsetWidth() { counts.offsetWidth++; return this._ow; },
            get offsetHeight() { counts.offsetHeight++; return this._oh; },
            appendChild(c) { counts.appendChild++; c.parentNode = this; this.children.push(c); return c; },
            append(...cs) { cs.forEach(c => this.appendChild(c)); },
            replaceChildren(...cs) { this.children.length = 0; cs.forEach(c => this.appendChild(c)); },
            removeChild(c) { const i = this.children.indexOf(c); if (i >= 0) this.children.splice(i, 1); c.parentNode = null; return c; },
            remove() { if (this.parentNode) this.parentNode.removeChild(this); },
            insertBefore(c, ref) { const i = ref ? this.children.indexOf(ref) : -1; if (i >= 0) this.children.splice(i, 0, c); else this.children.push(c); c.parentNode = this; return c; },
            cloneNode() { const c = makeEl(this.tagName, this.id); c._cls = this._cls; c.classList._s = new Set(this.classList._s); return c; },
            setAttribute(k, v) { this.attrs[k] = String(v); },
            getAttribute(k) { return this.attrs[k]; },
            removeAttribute(k) { delete this.attrs[k]; },
            addEventListener() {}, removeEventListener() {},
            focus() {}, blur() {}, select() {}, play() { return Promise.resolve(); }, pause() {}, load() {},
            getBoundingClientRect() { return { left: 0, top: 0, width: 300, height: 200 }; },
            closest(sel) { let n = this; while (n) { if (matchesSelector(n, sel)) return n; n = n.parentNode; } return null; },
            querySelector(sel) { return findIn(this, sel); },
            querySelectorAll(sel) { return findAllIn(this, sel); },
            contains() { return false; }
        };
        return el;
    };
    function matchesSelector(el, sel) {
        if (!el || !sel) return false;
        if (sel.startsWith('#')) return el.id === sel.slice(1);
        if (sel.startsWith('.')) return el.classList.contains(sel.slice(1));
        const dataSel = sel.match(/^\[data-([\w-]+)\]$/);
        if (dataSel) return el.dataset[dataSel[1].replace(/-(\w)/g, (m, c) => c.toUpperCase())] !== undefined;
        return el.tagName === String(sel).toUpperCase();
    }
    function walk(root, fn) { for (const child of root.children || []) { fn(child); walk(child, fn); } }
    function findIn(root, sel) { const parts = String(sel).trim().split(/\s+/); let cand = [root]; for (const p of parts) { const next = []; for (const c of cand) walk(c, el => { if (matchesSelector(el, p)) next.push(el); }); cand = next; } return cand[0] || null; }
    function findAllIn(root, sel) { const parts = String(sel).trim().split(/\s+/); let cand = [root]; for (const p of parts) { const next = []; for (const c of cand) walk(c, el => { if (matchesSelector(el, p)) next.push(el); }); cand = next; } return cand; }

    const registry = new Map();
    const html = read('index.html');
    for (const m of html.matchAll(/id="([\w-]+)"/g)) registry.set(m[1], makeEl('div', m[1]));
    const byId = id => {
        let el = registry.get(id);
        if (!el) { el = makeEl('div', id); registry.set(id, el); }
        return el;
    };
    const selectorStubs = new Map();
    const document = {
        readyState: 'complete', documentElement: makeEl('html'), body: makeEl('body'), baseURI: 'file:///app/index.html',
        _ev: {},
        getElementById: byId,
        createElement(tag) { counts.createElement++; return makeEl(tag); },
        querySelector(sel) {
            if (String(sel).startsWith('#') && !String(sel).includes(' ')) return byId(String(sel).slice(1));
            if (sel === '.count-btn.selected') { const el = makeEl('button'); el.dataset.count = '10'; return el; }
            if (sel === '.blinds-info') return byId('blindsInfo');
            if (sel === '.count-options') return byId('countOptions');
            if (!selectorStubs.has(sel)) selectorStubs.set(sel, makeEl('div'));
            return selectorStubs.get(sel);
        },
        querySelectorAll() { return []; },
        addEventListener(type, fn) { (this._ev[type] || (this._ev[type] = [])).push(fn); },
        removeEventListener() {},
        dispatchEvent(ev) { (this._ev[ev.type] || []).forEach(fn => fn(ev)); return true; },
        exitFullscreen() { return Promise.resolve(); },
        fullscreenElement: null
    };

    const store = {};
    const audioContexts = [];
    const context = {
        console: { log() {}, warn() {}, error() {} },
        JSON, Math, Date, Number, String, Boolean, Object, Array, Promise, Error, TypeError, Symbol, Intl, parseInt, parseFloat, isNaN, Map, Set, WeakMap,
        setTimeout: () => 0, clearTimeout: () => {}, setInterval: () => 0, clearInterval: () => {},
        requestAnimationFrame: () => 0, cancelAnimationFrame: () => {},
        performance: { now: () => 0 }, document,
        localStorage: { getItem: k => (k in store ? store[k] : null), setItem: (k, v) => { store[k] = String(v); }, removeItem: k => { delete store[k]; } },
        sessionStorage: { getItem: () => null, setItem: () => {}, removeItem: () => {} },
        navigator: { clipboard: { writeText: () => Promise.resolve() } },
        location: { href: 'file:///app/index.html', reload() {} },
        alert() {},
        HTMLMediaElement: { HAVE_FUTURE_DATA: 3, HAVE_CURRENT_DATA: 2 },
        Audio: function () { return makeEl('audio'); },
        getComputedStyle() { return {}; },
        matchMedia() { return { matches: false, addEventListener() {} }; },
        CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init && init.detail; } },
        URL,
        Worker: function () { this.postMessage = () => {}; this.terminate = () => {}; },
        AudioContext: function () {
            const ctx = {
                state: 'suspended', currentTime: 0, destination: {}, resumed: 0, closed: 0,
                resume() { ctx.resumed++; ctx.state = 'running'; return Promise.resolve(); },
                close() { ctx.closed++; return Promise.resolve(); },
                createOscillator() { return { frequency: {}, connect() {}, start() {}, stop() {} }; },
                createGain() { return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect() {} }; }
            };
            audioContexts.push(ctx);
            return ctx;
        },
        WebSocket: function () { this.readyState = 1; this.send = () => {}; this.close = () => {}; }
    };
    context.WebSocket.OPEN = 1;
    context.window = context;
    context.self = context;
    context.globalThis = context;
    context.window.addEventListener = (type, fn) => document.addEventListener(type, fn);
    context.window.removeEventListener = () => {};
    context.window.dispatchEvent = ev => document.dispatchEvent(ev);
    context.webkitAudioContext = context.AudioContext;
    context.window.__DISABLE_DEBUG__ = true; // debug.js panel bootstrap is not part of these tests
    vm.createContext(context);

    const scripts = [...html.matchAll(/<script src="([^"]+)"><\/script>/g)].map(m => m[1]);
    assert.ok(scripts.length >= 13, 'index.html should load the engine + ui scripts');
    for (const src of scripts) vm.runInContext(read(src), context, { filename: src });
    for (const fn of (document._ev['DOMContentLoaded'] || [])) fn({ type: 'DOMContentLoaded' });

    return { context, counts, document, registry, byId, audioContexts, scripts };
}

const env = createDomShim();
const ui = env.context;
const run = (code) => vm.runInContext(code, ui);
const keydown = (key, target) => {
    const handlers = env.document._ev['keydown'] || [];
    const withClosest = Object.assign({ closest: () => null }, target);
    handlers.forEach(fn => fn({ key, target: withClosest, preventDefault() {}, defaultPrevented: false }));
};

test('ui: index.html scripts all load in order (shim environment is healthy)', () => {
    assert.ok(env.scripts.includes('js/ui.js'));
    assert.equal(typeof run('typeof doAction'), 'string');
    assert.equal(run('typeof doAction'), 'function');
});

test('ui: window.game is a live getter, never a stale instance', () => {
    const initial = run('window.game');
    assert.ok(initial, 'DOMContentLoaded must have created a game instance');
    assert.equal(run('window.game === game'), true, 'window.game must be the current instance');
    // startNewGame replaces the instance — the getter must follow it
    run('startNewGame()');
    assert.equal(run('window.game === game'), true, 'window.game must follow a new instance');
    assert.notEqual(run('window.game'), initial);
});

test('ui: keyboard shortcuts ignore keystrokes typed into inputs', () => {
    run(`
        playMode = 'local';
        startNewGame();
        game.isPlayerTurn = function () { return true; };
        document.getElementById('btnFold').disabled = false;
        window.__foldCalls = 0;
        const __origDoAction = doAction;
        doAction = function (type) { window.__foldCalls++; return __origDoAction.apply(this, arguments); };
    `);
    keydown('f', { tagName: 'INPUT', isContentEditable: false });
    keydown('a', { tagName: 'INPUT', isContentEditable: false });
    keydown('f', { tagName: 'TEXTAREA', isContentEditable: false });
    keydown('c', { tagName: 'SELECT', isContentEditable: false });
    keydown('f', { tagName: 'DIV', isContentEditable: true });
    assert.equal(run('__foldCalls'), 0, 'inputs must not trigger table actions');
    keydown('f', { tagName: 'DIV', isContentEditable: false });
    assert.equal(run('__foldCalls'), 1, 'shortcut must still work on the table');
});

test('ui: applyRoomConfigFromServer only rebuilds settings when the config changed', () => {
    ui.__standard = { roomConfig: { isShortDeck: false, startingStack: 20000, smallBlind: 40, bigBlind: 80, aiDelay: 1800 } };
    run('appliedRoomConfigFingerprint = null; applyRoomConfigFromServer(this.__standard);');
    const base = { ...env.counts.innerHTMLSites };

    // 20 identical game_state messages (the online hot path) must not touch any settings node
    for (let i = 0; i < 20; i++) run('applyRoomConfigFromServer(this.__standard)');
    const afterIdentical = { ...env.counts.innerHTMLSites };
    assert.deepEqual(afterIdentical, base,
        'identical room configs must not rewrite settings nodes: ' + JSON.stringify(afterIdentical));
    for (const id of ['countOptions', 'roomMaxPlayers', 'blindsInfo']) {
        assert.equal((afterIdentical[id] || 0) - (base[id] || 0), 0,
            id + ' must not be rebuilt while the config is unchanged');
    }

    // a real config change must still be applied
    ui.__short = { roomConfig: { isShortDeck: true, startingStack: 20000, smallBlind: 20, bigBlind: 40, ante: 20, minBet: 40, aiDelay: 1800 } };
    run('applyRoomConfigFromServer(this.__short)');
    assert.equal(run('gameMode'), 'shortdeck');
    assert.ok(run("document.querySelector('.count-options').innerHTML.includes('8 人')"),
        'short deck count options must be re-rendered on a real config change');

    // and the new config becomes the new fingerprint
    const afterChange = { ...env.counts.innerHTMLSites };
    for (let i = 0; i < 20; i++) run('applyRoomConfigFromServer(this.__short)');
    assert.deepEqual({ ...env.counts.innerHTMLSites }, afterChange,
        'repeated messages with the new config must not rewrite settings nodes');
});

test('ui: seat list reuses DOM nodes instead of rebuilding them every frame', () => {
    run("playMode = 'local'; gameMode = 'standard'; startNewGame(); game.phase = 'preflop';");
    run('renderAIPlayers()');
    const container = env.byId('aiPlayersContainer');
    const seats = container.children.slice();
    assert.ok(seats.length > 1, 'AI seats should be rendered');
    const firstAvatar = seats[0].children.find(child => child.className === 'player-avatar');
    assert.ok(firstAvatar, 'seat skeleton must contain .player-avatar');
    assert.ok(firstAvatar.textContent.length > 0, 'avatar text must be filled');
    const created = env.counts.createElement;
    const appended = env.counts.appendChild;
    const readWidth = env.counts.offsetWidth + env.counts.offsetHeight;
    run('renderAIPlayers()');
    assert.equal(env.counts.createElement - created, 0, 'steady-state seat render must create no nodes');
    assert.equal(env.counts.appendChild - appended, 0, 'steady-state seat render must append no nodes');
    assert.equal((env.counts.offsetWidth + env.counts.offsetHeight) - readWidth, 0,
        'cached table size must not read offsetWidth/offsetHeight again');
    assert.deepEqual(container.children, seats, 'seat nodes must keep their identity across frames');
    // player count changes must still resize the list
    run('game.players.pop();');
    run('renderAIPlayers()');
    assert.equal(container.children.length, seats.length - 1);
});

test('ui: last action label is not rewritten (CSS animation) on unchanged frames', () => {
    run(`
        playMode = 'local';
        startNewGame();
        game.phase = 'flop';
        game.players[1].lastAction = { action: 'raise', amount: 120 };
        renderAIPlayers();
    `);
    const seat = env.byId('aiPlayersContainer').children[0];
    const action = seat.children.find(child => String(child.className).includes('player-last-action'));
    assert.ok(action, 'seat must expose a .player-last-action node');
    assert.ok(action.textContent.includes('加注'), 'action label must be rendered: ' + action.textContent);
    const textBefore = action.textContent;
    const classBefore = action.className;
    action.classList.remove('action-pop'); // simulate the 800ms cleanup timer
    run('renderAIPlayers()');
    assert.equal(action.textContent, textBefore, 'unchanged action must not be rewritten');
    assert.equal(action.className, classBefore, 'unchanged action must not restart its animation');
});

test('ui: chip amounts go through formatChips (no hand-rolled toLocaleString)', () => {
    const source = read('js/ui.js');
    const occurrences = source.match(/toLocaleString/g) || [];
    assert.equal(occurrences.length, 1, 'only formatChips may call toLocaleString');
    assert.match(source, /function formatChips\(value\) \{\s*return '\$' \+ Math\.max\(0, Math\.floor\(Number\(value\) \|\| 0\)\)\.toLocaleString\(\);/);
    run(`
        playMode = 'local';
        startNewGame();
        game.phase = 'flop';
        game.players[1].stack = 20000;
        game.players[1].lastAction = null;
        renderAIPlayers();
    `);
    const seat = env.byId('aiPlayersContainer').children[0];
    const stack = seat.children.find(child => child.className === 'player-stack');
    assert.equal(stack.textContent, '$20,000', 'stack must use the shared formatter');
});

test('ui: turn alert resumes (and later releases) the AudioContext', () => {
    run('playTurnAlert()');
    assert.equal(env.audioContexts.length, 1, 'one AudioContext for the turn alert');
    assert.ok(env.audioContexts[0].resumed > 0, 'suspended context must be resumed so the first beep is audible');
    run('playTurnAlert()');
    assert.equal(env.audioContexts.length, 1, 'context must be reused, not recreated');
    run('releaseTurnAudioContext()');
    assert.equal(env.audioContexts[0].closed, 1, 'context must be closable on unload');
    run('playTurnAlert()');
    assert.equal(env.audioContexts.length, 2, 'a fresh context is created after release');
});

test('css: dead rules stay deleted and live seat/ability rules stay', () => {
    const css = read('css/style.css');
    for (const dead of ['.dealer-btn', '.chip-stack', '.tooltip', '.side-pot-badge', '.action-blind', '.player-seat.winner', '@keyframes winAnnounce', '@keyframes winnerGlow', '@keyframes chipBounce']) {
        assert.ok(!css.includes(dead), 'dead CSS rule ' + dead + ' should have been removed');
    }
    const open = (css.match(/\{/g) || []).length;
    const close = (css.match(/\}/g) || []).length;
    assert.equal(open, close, 'style.css braces must stay balanced');
    for (const live of ['.player-seat.thinking', '.thinking-dots', '.action-pop', '.position-badges', '.player-last-action']) {
        assert.ok(css.includes(live), 'live rule ' + live + ' must remain');
    }
});

test('js: dead doCreateRoom export and debug.js shadowed toast are gone', () => {
    assert.ok(!read('js/ui.js').includes('doCreateRoom'), 'doCreateRoom had no UI entry point');
    assert.ok(!read('js/debug.js').includes('window.showDebugToast = showToast'),
        'debug.js assignment was shadowed by the ui.js function declaration');
    assert.ok(read('js/ui.js').includes('function showDebugToast'), 'ui.js keeps the live toast implementation');
});
