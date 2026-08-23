const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('path');
const { startLocalPokerServer } = require('./server/local-server');

let localPokerServer = null;

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

app.whenReady().then(async () => {
    try {
        localPokerServer = await startLocalPokerServer({ host: '127.0.0.1', port: 3000 });
        console.log('Local poker server listening on ws://127.0.0.1:3000');
    } catch (error) {
        // A second app window or a separately launched test server may already
        // own the port. The window can safely use that existing service.
        if (error?.code !== 'EADDRINUSE') console.error('Local poker server failed:', error);
    }

    const win = new BrowserWindow({
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
