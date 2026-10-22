'use strict';
// Loads the original browser JS modules in a vm sandbox (same approach as
// tests/rules.test.js) so scripts that attach globals can be used from Node.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function referenceRepo() {
    const env = process.env.REFERENCE_REPO;
    if (env) return path.resolve(env);
    // oracle/lib -> oracle -> texas-holdem-rs -> projects
    return path.resolve(__dirname, '..', '..', '..', 'reference', 'texas-holdem-game');
}

const BROWSER_MODULES = [
    'js/cards.js',
    'js/game-rules-core.js',
    'js/engine.js',
    'js/probability-core.js',
    'js/probability.js',
    'js/ai-shortdeck-stable.js',
    'js/ai-core.js',
    'js/ai.js',
    'js/game.js',
    'js/shortdeck.js'
];

function loadBrowserModules() {
    const repo = referenceRepo();
    const context = vm.createContext({
        console,
        setTimeout: () => 0,
        clearTimeout: () => {},
        setInterval: () => 0,
        clearInterval: () => {},
        performance: { now: () => 0 },
        Math,
        Date
    });
    for (const relative of BROWSER_MODULES) {
        const filename = path.join(repo, relative);
        vm.runInContext(fs.readFileSync(filename, 'utf8'), context, { filename });
    }
    return {
        context,
        modules: vm.runInContext(`({
            Card,
            SUITS, RANKS, RANK_VALUES,
            Deck, evaluateHand, evaluatePartialHand, compareHands,
            evaluateSDHand, compareSDHands, SD_HAND_TYPES, SD_HAND_TYPE_NAMES,
            PokerEngine, PokerGame, ShortDeckGame, AIPlayer, AI_STYLES,
            PlayerModel, PokerProbabilityCore, PokerGameRules
        })`, context)
    };
}

function loadProbabilityCore() {
    return require(path.join(referenceRepo(), 'js/probability-core.js'));
}
function loadServerRules() {
    return require(path.join(referenceRepo(), 'server/poker-rules.js'));
}

module.exports = { referenceRepo, loadBrowserModules, loadProbabilityCore, loadServerRules };
