const { app, BrowserWindow } = require('electron');
const path = require('path');

app.whenReady().then(() => {
    const win = new BrowserWindow({
        width: 1280,
        height: 860,
        minWidth: 900,
        minHeight: 600,
        title: 'Texas Hold\'em',
        icon: path.join(__dirname, 'icon.ico'),
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            webSecurity: false
        }
    });

    win.loadFile('index.html');
    win.setMenuBarVisibility(false);
    win.setTitle('Texas Hold\'em ♠♥♣♦');

    win.on('closed', () => {
        app.quit();
    });
});

app.on('window-all-closed', () => {
    app.quit();
});
