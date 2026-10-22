#!/usr/bin/env node
'use strict';
// Starts the reference JS room server for the Rust-client interop test.
// Prints exactly one JSON line with the chosen WebSocket URL on stdout and then
// keeps running until stdin closes or SIGTERM/SIGINT arrives (then shuts down).
//
// Used by crates/poker-server/tests/interop.rs to prove that a Rust client
// speaking poker-protocol v2 can drive the original server.

const path = require('node:path');

function referenceRepo() {
    const env = process.env.REFERENCE_REPO;
    if (env) return path.resolve(env);
    return path.resolve(__dirname, '..', '..', 'reference', 'texas-holdem-game');
}

const reference = referenceRepo();
const { startLocalPokerServer } = require(path.join(reference, 'server', 'local-server.js'));

(async () => {
    const port = process.env.POKER_PORT ? Number(process.env.POKER_PORT) : 0;
    const server = await startLocalPokerServer({
        host: '127.0.0.1',
        port,
        aiDelay: 0,
        disconnectGraceMs: 300,
        maxRooms: 5,
        maxConnections: 16,
        maxConnectionsPerIp: 16,
        rateWindowMs: 60000,
        maxMessagesPerWindow: 500
    });
    const address = server.address();
    process.stdout.write(JSON.stringify({ url: 'ws://127.0.0.1:' + address.port }) + '\n');
    const shutdown = async () => {
        try { await server.close(); } catch (_) {}
        process.exit(0);
    };
    process.on('SIGTERM', shutdown);
    process.on('SIGINT', shutdown);
    process.stdin.on('end', shutdown);
    process.stdin.resume();
})().catch(error => {
    console.error('[interop-rust-client] failed:', error && error.stack ? error.stack : error);
    process.exit(1);
});
