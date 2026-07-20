/**
 * 房间管理
 */
const { GameEngine } = require('./game-engine');

let roomCounter = 0;

class Room {
    constructor(options) {
        this.code = generateCode();
        this.maxPlayers = options.maxPlayers || 6;
        this.isShortDeck = !!options.isShortDeck;
        this.useEliteAI = !!options.useEliteAI;
        this.players = [];
        this.engine = null;
        this.createdAt = Date.now();
    }

    addPlayer(ws, name) {
        if (this.players.length >= this.maxPlayers) throw new Error('房间已满');
        if (this.engine) throw new Error('游戏已开始');
        const player = { id: 'p' + Date.now().toString(36) + Math.random().toString(36).slice(2, 6), name: name, ws: ws, isHuman: true, disconnected: false };
        this.players.push(player);
        return player;
    }

    removePlayer(id) {
        this.players = this.players.filter(p => p.id !== id);
    }

    getPlayer(id) { return this.players.find(p => p.id === id); }

    getHumanPlayers() { return this.players.filter(p => p.isHuman && !p.disconnected); }

    getPlayerInfo() {
        return this.players.map(p => ({ id: p.id, name: p.name, isHuman: p.isHuman, disconnected: p.disconnected }));
    }

    isPlaying() { return this.engine !== null; }

    isEmpty() { return this.players.length === 0; }

    markDisconnected(id) {
        const p = this.getPlayer(id);
        if (p) p.disconnected = true;
    }

    startGame() {
        const humanPlayers = this.players.filter(p => p.isHuman);
        const wantPlayers = Math.max(humanPlayers.length, Math.min(this.maxPlayers, 6 + Math.floor(Math.random() * 2)));
        this.engine = new GameEngine(this.isShortDeck, this.useEliteAI);
        for (const hp of humanPlayers) {
            this.engine.addPlayer(hp.name, hp.id, true);
        }
        const aiNames = ['Alex','Blake','Casey','Drew','Eden','Finn','Gray','Jade','Kai'];
        let aiIdx = 0;
        while (this.engine.players.length < wantPlayers) {
            const id = 'ai_' + (aiIdx++);
            this.engine.addPlayer(aiNames[aiIdx % aiNames.length] || 'AI', id, false);
        }
    }
}

class RoomManager {
    constructor() { this.rooms = new Map(); }

    createRoom(options) {
        const room = new Room(options);
        this.rooms.set(room.code, room);
        return room;
    }

    getRoom(code) { return this.rooms.get(code); }

    deleteRoom(code) { this.rooms.delete(code); }
}

function generateCode() {
    const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
    let code = '';
    for (let i = 0; i < 4; i++) code += chars[Math.floor(Math.random() * chars.length)];
    return code;
}

module.exports = { Room, RoomManager };