'use strict';

/** Native reminders track whether a decision is outstanding, independently of sound. */
class DesktopAttention {
    constructor(win, { Tray, Menu, nativeImage, app, iconPath, alertIconPath,
        setInterval: schedule = setInterval, clearInterval: cancel = clearInterval }) {
        this.win = win;
        this.pending = false;
        this.reason = 'turn';
        this.messageKey = null;
        this.seen = false;
        this.timer = null;
        this.schedule = schedule;
        this.cancel = cancel;
        this.normalIcon = nativeImage.createFromPath(iconPath).resize({ width:32, height:32 });
        this.alertIcon = nativeImage.createFromPath(alertIconPath);
        this.tray = new Tray(this.normalIcon);
        this.tray.setToolTip('德州扑克');
        const open = () => {
            if (win.isDestroyed()) return;
            if (win.isMinimized()) win.restore();
            win.show();
            win.focus();
        };
        this.tray.on('click', open);
        this.tray.setContextMenu(Menu.buildFromTemplate([
            { label:'打开德州扑克', click:open },
            { type:'separator' },
            { label:'退出', click:() => app.quit() }
        ]));
        this.onFocus = () => this.acknowledge();
        this.onBlur = () => this.sync();
        win.on('focus', this.onFocus);
        win.on('blur', this.onBlur);
        win.on('minimize', this.onBlur);
    }

    setPending(pending, reason = 'turn', messageKey = null) {
        const nextReason = reason === 'settlement' ? 'settlement' : 'turn';
        const nextKey = typeof messageKey === 'string' ? messageKey : nextReason;
        if (pending === true && (!this.pending || nextKey !== this.messageKey)) this.seen = false;
        this.pending = pending === true;
        this.reason = nextReason;
        this.messageKey = this.pending ? nextKey : null;
        this.sync();
    }

    acknowledge() {
        if (this.pending) this.seen = true;
        this.stop();
    }

    sync() {
        if (this.win.isDestroyed()) { this.stop(); return; }
        if (this.win.isFocused() && !this.win.isMinimized()) {
            this.acknowledge();
            return;
        }
        if (!this.pending || this.seen) {
            this.stop();
            return;
        }
        this.tray.setToolTip(this.reason === 'settlement' ? '德州扑克：本局已结束，请确认下一手' : '德州扑克：轮到你行动');
        if (this.timer !== null) return;
        this.win.flashFrame(true);
        let red = true;
        this.tray.setImage(this.alertIcon);
        this.timer = this.schedule(() => {
            red = !red;
            this.tray.setImage(red ? this.alertIcon : this.normalIcon);
        }, 650);
        this.timer?.unref?.();
    }

    stop() {
        if (this.timer !== null) this.cancel(this.timer);
        this.timer = null;
        if (!this.win.isDestroyed()) this.win.flashFrame(false);
        this.tray?.setImage(this.normalIcon);
        this.tray?.setToolTip('德州扑克');
    }

    destroy() {
        this.stop();
        this.win.removeListener('focus', this.onFocus);
        this.win.removeListener('blur', this.onBlur);
        this.win.removeListener('minimize', this.onBlur);
        this.tray?.destroy();
        this.tray = null;
    }
}

module.exports = { DesktopAttention };
