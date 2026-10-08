'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { EventEmitter } = require('node:events');
const { DesktopAttention } = require('../desktop-attention');

function setup() {
    const win = new EventEmitter();
    Object.assign(win, { focused:true, destroyed:false, minimized:false, flashes:[],
        isFocused() { return this.focused; }, isDestroyed() { return this.destroyed; },
        isMinimized() { return this.minimized; },
        flashFrame(value) { this.flashes.push(value); },
        restore() { this.minimized = false; }, show() { this.shown = true; },
        focus() { this.focused = true; this.emit('focus'); }
    });
    const timers = new Map();
    let seq = 0;
    class Tray extends EventEmitter {
        setToolTip(text) { this.tooltip = text; }
        setImage(icon) { this.image = icon; }
        setContextMenu(menu) { this.menu = menu; }
        destroy() { this.destroyed = true; }
    }
    const attention = new DesktopAttention(win, {
        Tray, Menu:{buildFromTemplate:items => items}, app:{quit() {}},
        nativeImage:{createFromPath:p => ({ path:p, resize() { return this; } })},
        iconPath:'normal', alertIconPath:'red',
        setInterval:fn => { const id = ++seq; timers.set(id, fn); return id; },
        clearInterval:id => timers.delete(id)
    });
    return { attention, win, timers };
}

test('desktop attention remembers a seen message and only rearms for a new decision', () => {
    const { attention:a, win, timers } = setup();
    a.setPending(true);
    assert.equal(timers.size, 0);
    win.focused = false; win.emit('blur');
    assert.equal(timers.size, 0, 'a decision already displayed in front is read');
    a.setPending(false);
    a.setPending(true, 'turn', 'hand-1-turn-1');
    assert.equal(timers.size, 1);
    assert.equal(win.flashes.at(-1), true);
    assert.equal(a.tray.image.path, 'red');
    const tick = [...timers.values()][0]; tick();
    assert.equal(a.tray.image.path, 'normal'); tick();
    assert.equal(a.tray.image.path, 'red');
    a.setPending(true, 'turn', 'hand-1-turn-1'); a.setPending(true, 'turn', 'hand-1-turn-1');
    assert.equal(timers.size, 1, 'duplicate snapshots must not create timers');
    win.focus();
    assert.equal(timers.size, 0);
    assert.equal(win.flashes.at(-1), false);
    win.focused = false; win.emit('blur');
    assert.equal(timers.size, 0, 'reading without acting must not flash again on blur');
    a.setPending(true, 'turn', 'hand-1-turn-1');
    assert.equal(timers.size, 0, 'duplicate state must preserve read acknowledgement');
    a.setPending(true, 'turn', 'hand-1-turn-2');
    assert.equal(timers.size, 1, 'a genuinely new turn must still alert');
    a.setPending(false);
    assert.equal(timers.size, 0);
    win.emit('blur');
    assert.equal(timers.size, 0, 'completed actions must not rearm');
    a.destroy();
});

test('settlement tray click restores the game and destruction releases timers/listeners', () => {
    const { attention:a, win, timers } = setup();
    win.focused = false; win.minimized = true;
    a.setPending(true, 'settlement');
    assert.match(a.tray.tooltip, /确认下一手/);
    const tray = a.tray;
    tray.emit('click');
    assert.equal(win.minimized, false);
    assert.equal(win.shown, true);
    assert.equal(timers.size, 0);
    win.focused = false; win.emit('blur');
    a.destroy();
    assert.equal(tray.destroyed, true);
    assert.equal(timers.size, 0);
    assert.equal(win.listenerCount('blur'), 0);
});
