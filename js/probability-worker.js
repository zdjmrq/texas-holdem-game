'use strict';

importScripts('probability-core.js');

self.onmessage = event => {
    const request = event.data || {};
    try {
        const result = self.PokerProbabilityCore.calculate(request.options || {});
        self.postMessage({ id:request.id, key:request.key, result });
    } catch (error) {
        self.postMessage({
            id:request.id,
            key:request.key,
            error:error && error.message ? error.message : String(error)
        });
    }
};
