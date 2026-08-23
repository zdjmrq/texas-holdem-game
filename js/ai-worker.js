'use strict';

importScripts('probability-core.js', 'ai-core.js');

const brains = new Map();

function ensureBrain(key, meta = {}) {
    let brain = brains.get(key);
    if (!brain) {
        brain = new self.PokerAICore.UnifiedPokerAI({
            name:meta.name || 'AI',
            style:meta.style || 'SOLID',
            seatId:meta.seatId ?? 0,
            profile:meta.profile || undefined
        });
        brains.set(key, brain);
    }
    brain.seatId = meta.seatId ?? brain.seatId;
    return brain;
}

self.onmessage = event => {
    const message = event.data || {};
    try {
        const brain = ensureBrain(message.key, message.meta);
        if (message.type === 'observe') {
            brain.observeAction(message.event || {});
            return;
        }
        if (message.type === 'showdown') {
            brain.observeShowdown(message.seatId, message.result || {});
            return;
        }
        if (message.type === 'decide') {
            const context = message.context || {};
            const result = brain.decide({
                ...context,
                evaluateCards:(cards, variant) => self.PokerProbabilityCore.evaluate(cards, variant === 'shortdeck')
            });
            self.postMessage({ id:message.id, result });
        }
    } catch (error) {
        if (message.id !== undefined) {
            self.postMessage({ id:message.id, error:error?.message || String(error) });
        }
    }
};
