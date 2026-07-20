/**
 * logger.js — 服务端运行时日志
 */
const fs = require('fs');
const path = require('path');

const PROJECT_ROOT = path.resolve(__dirname, '..');
const UNIFIED_DIR = path.join(PROJECT_ROOT, '错误日志', 'logs');
const LEGACY_DIR = path.join(__dirname, 'logs');
const LOG_DIR = fs.existsSync(path.join(PROJECT_ROOT, '错误日志')) ? UNIFIED_DIR : LEGACY_DIR;
const SESSION_ID = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
if (!fs.existsSync(LOG_DIR)) {
    try { fs.mkdirSync(LOG_DIR, { recursive: true }); } catch (e) {}
}

const MAX_LOG_FILES = 20;
const MAX_FILE_SIZE = 5 * 1024 * 1024;

function getLogPath(prefix) {
    return path.join(LOG_DIR, prefix + '_' + SESSION_ID + '.log');
}

function writeToFile(filePath, text) {
    try {
        if (fs.existsSync(filePath)) {
            const stat = fs.statSync(filePath);
            if (stat.size > MAX_FILE_SIZE) {
                const base = filePath.replace(/\.log$/, '');
                let i = 1;
                while (fs.existsSync(base + '_' + i + '.log')) i++;
                fs.renameSync(filePath, base + '_' + i + '.log');
            }
        }
        fs.appendFileSync(filePath, text, 'utf8');
    } catch (e) { console.error('[Logger] write fail:', e.message); }
}

function ts() {
    const d = new Date();
    return d.toISOString().replace('T', ' ').slice(0, 19) + '.' + String(d.getMilliseconds()).padStart(3, '0');
}

function game(roomCode, label, detail) {
    const line = '[' + ts() + '] [game]' + (roomCode ? ' [' + roomCode + ']' : '') + ' ' + label + (detail ? ' | ' + JSON.stringify(detail) : '') + '\n';
    console.log(line.trim());
    writeToFile(getLogPath('game'), line);
}

function ai(roomCode, label, detail) {
    const line = '[' + ts() + '] [ai]' + (roomCode ? ' [' + roomCode + ']' : '') + ' ' + label + (detail ? ' | ' + JSON.stringify(detail) : '') + '\n';
    writeToFile(getLogPath('ai'), line);
}

function action(roomCode, label, detail) {
    const line = '[' + ts() + '] [action]' + (roomCode ? ' [' + roomCode + ']' : '') + ' ' + label + (detail ? ' | ' + JSON.stringify(detail) : '') + '\n';
    writeToFile(getLogPath('action'), line);
}

function state(roomCode, stateObj) {
    if (!stateObj) return;
    const snapshot = { phase: stateObj.phase, pot: stateObj.pot, playerCount: stateObj.players ? stateObj.players.length : 0 };
    const line = '[' + ts() + '] [state]' + (roomCode ? ' [' + roomCode + ']' : '') + ' 状态快照 | ' + JSON.stringify(snapshot) + '\n';
    writeToFile(getLogPath('state'), line);
}

function ws(roomCode, direction, msgType, detail) {
    const label = direction === 'send' ? '→ send' : '← recv';
    const line = '[' + ts() + '] [ws]' + (roomCode ? ' [' + roomCode + ']' : '') + ' ' + label + ' ' + msgType + (detail ? ' | ' + JSON.stringify(detail) : '') + '\n';
    writeToFile(getLogPath('ws'), line);
}

function error(roomCode, label, err) {
    const errDetail = err ? { message: err.message || String(err) } : null;
    const line = '[' + ts() + '] [error]' + (roomCode ? ' [' + roomCode + ']' : '') + ' ' + label + (errDetail ? ' | ' + JSON.stringify(errDetail) : '') + '\n';
    console.error(line.trim());
    writeToFile(getLogPath('error'), line);
}

function summary() {
    console.log('\n--- Logger ---');
    console.log('Log dir:', LOG_DIR);
    console.log('--------------\n');
}

module.exports = { game, ai, action, state, ws, error, summary, LOG_DIR };