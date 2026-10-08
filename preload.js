'use strict';

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('windowControls', Object.freeze({
    minimize: () => ipcRenderer.send('window:minimize'),
    toggleMaximize: () => ipcRenderer.invoke('window:toggle-maximize'),
    close: () => ipcRenderer.send('window:close'),
    getLocalServerUrl: () => ipcRenderer.invoke('server:get-local-url'),
    setAttention: (pending, reason, messageKey) => ipcRenderer.send('window:attention', pending === true,
        reason === 'settlement' ? 'settlement' : 'turn',
        typeof messageKey === 'string' ? messageKey.slice(0, 200) : null)
}));
