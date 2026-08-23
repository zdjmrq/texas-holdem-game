const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('path');
const { WebSocket } = require('ws');
const { startLocalPokerServer } = require('./server/local-server');

let localPokerServer = null;
let localServerUrl = 'ws://127.0.0.1:3000';
let mainWindow = null;
const hasSingleInstanceLock = app.requestSingleInstanceLock();
if (!hasSingleInstanceLock) app.quit();

app.on('second-instance', () => {
    if (!mainWindow) return;
    if (mainWindow.isMinimized()) mainWindow.restore();
    mainWindow.show();
    mainWindow.focus();
});

function probePokerServer(url, timeoutMs = 900) {
    return new Promise(resolve => {
        let settled = false;
        const finish = (valid, socket) => {
            if (settled) return;
            settled = true;
            clearTimeout(timer);
            try { socket?.close(); } catch (_) {}
            resolve(valid);
        };
        let socket;
        try { socket = new WebSocket(url, { handshakeTimeout:timeoutMs }); }
        catch (_) { resolve(false); return; }
        const timer = setTimeout(() => finish(false, socket), timeoutMs);
        socket.on('message', data => {
            try {
                const hello = JSON.parse(data.toString('utf8'));
                finish(hello.type === 'hello' && hello.serverId === 'texas-holdem-game' && hello.protocolVersion === 2, socket);
            } catch (_) { finish(false, socket); }
        });
        socket.on('error', () => finish(false, socket));
        socket.on('close', () => finish(false, socket));
    });
}

async function ensureLocalServer() {
    try {
        localPokerServer = await startLocalPokerServer({ host:'127.0.0.1', port:3000 });
    } catch (error) {
        if (error?.code !== 'EADDRINUSE') throw error;
        if (await probePokerServer(localServerUrl)) return;
        localPokerServer = await startLocalPokerServer({ host:'127.0.0.1', port:0 });
    }
    const address = localPokerServer.address();
    localServerUrl = `ws://127.0.0.1:${address.port}`;
}

function senderWindow(event) {
    return BrowserWindow.fromWebContents(event.sender);
}

ipcMain.on('window:minimize', event => senderWindow(event)?.minimize());
ipcMain.on('window:close', event => senderWindow(event)?.close());
ipcMain.handle('window:toggle-maximize', event => {
    const win = senderWindow(event);
    if (!win) return false;
    if (win.isMaximized()) win.unmaximize();
    else win.maximize();
    return win.isMaximized();
});
ipcMain.handle('server:get-local-url', () => localServerUrl);

app.whenReady().then(async () => {
    if (!hasSingleInstanceLock) return;
    try {
        await ensureLocalServer();
        console.log(`Local poker server ready at ${localServerUrl}`);
    } catch (error) {
        console.error('Local poker server failed:', error);
    }

    const win = mainWindow = new BrowserWindow({
        width: 1280,
        height: 860,
        minWidth: 900,
        minHeight: 600,
        frame: false,
        thickFrame: true,
        hasShadow: true,
        show: false,
        backgroundColor: '#090d18',
        title: 'Texas Hold\'em',
        icon: path.join(__dirname, 'icon.ico'),
        webPreferences: {
            preload: path.join(__dirname, 'preload.js'),
            nodeIntegration: false,
            contextIsolation: true,
            sandbox: true,
            webSecurity: true
        }
    });

    win.loadFile(path.join(__dirname, 'index.html'));
    win.once('ready-to-show', () => win.show());
    win.setMenuBarVisibility(false);
    win.setTitle('Texas Hold\'em ♠♥♣♦');
    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    win.webContents.on('will-navigate', event => event.preventDefault());

    win.on('closed', () => {
        mainWindow = null;
        app.quit();
    });
});

app.on('window-all-closed', () => {
    app.quit();
});

app.on('before-quit', () => {
    if (localPokerServer) {
        localPokerServer.close().catch(() => {});
        localPokerServer = null;
    }
});
