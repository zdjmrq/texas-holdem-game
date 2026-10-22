'use strict';
// Deterministic PRNGs shared by all oracle exporters.
// mulberry32 for card/hand sampling (stable across Node versions).
function mulberry32(seed) {
    let a = (seed >>> 0) || 0x9e3779b9;
    return function () {
        a |= 0; a = (a + 0x6D2B79F5) | 0;
        let t = Math.imul(a ^ (a >>> 15), 1 | a);
        t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
}
// Port of js/probability-core.js hashSeed + xorshift32 randomFactory so the
// Rust Monte Carlo can reproduce the JS sample stream bit-for-bit.
function hashSeed(value) {
    const text = String(value || 'poker-probability');
    let hash = 2166136261;
    for (let i = 0; i < text.length; i++) {
        hash ^= text.charCodeAt(i);
        hash = Math.imul(hash, 16777619);
    }
    return hash >>> 0 || 0x9e3779b9;
}
function xorshift32(seed) {
    let state = hashSeed(seed);
    return function () {
        state ^= state << 13;
        state ^= state >>> 17;
        state ^= state << 5;
        return (state >>> 0) / 4294967296;
    };
}
function pick(rng, arr) { return arr[Math.floor(rng() * arr.length)]; }
function shuffle(rng, arr) {
    for (let i = arr.length - 1; i > 0; i--) {
        const j = Math.floor(rng() * (i + 1));
        [arr[i], arr[j]] = [arr[j], arr[i]];
    }
    return arr;
}
module.exports = { mulberry32, hashSeed, xorshift32, pick, shuffle };
