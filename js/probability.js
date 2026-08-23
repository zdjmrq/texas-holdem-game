/**
 * Non-blocking player equity service.
 *
 * Production calculations run in probability-worker.js. A small asynchronous
 * fallback is retained for browsers that cannot create workers from file://.
 */
class ProbabilityService {
    constructor() {
        this.worker = null;
        this.sequence = 0;
        this.pending = new Map();
        this.cache = new Map();
        this.workerUnavailable = false;
    }

    _cardKey(card) {
        return `${card?.rank || ''}-${card?.suit || ''}`;
    }

    makeKey(options) {
        const hole = (options.playerCards || []).map(card => this._cardKey(card)).sort().join('|');
        const board = (options.communityCards || []).map(card => this._cardKey(card)).join('|');
        return `${options.isShortDeck ? 'short' : 'standard'}:${hole}:${board}:o${options.numOpponents || 1}`;
    }

    _ensureWorker() {
        if (this.worker || this.workerUnavailable) return this.worker;
        try {
            this.worker = new Worker('js/probability-worker.js');
            this.worker.onmessage = event => this._resolve(event.data || {});
            this.worker.onerror = error => {
                console.warn('Probability worker unavailable; using reduced fallback:', error.message || error);
                this.workerUnavailable = true;
                const pending = [...this.pending.values()];
                this.pending.clear();
                try { this.worker.terminate(); } catch (_) {}
                this.worker = null;
                for (const request of pending) this._fallback(request);
            };
        } catch (error) {
            this.workerUnavailable = true;
            console.warn('Probability worker could not start:', error);
        }
        return this.worker;
    }

    _remember(key, result) {
        if (this.cache.has(key)) this.cache.delete(key);
        this.cache.set(key, result);
        while (this.cache.size > 48) this.cache.delete(this.cache.keys().next().value);
    }

    _resolve(message) {
        const request = this.pending.get(message.id);
        if (!request) return;
        this.pending.delete(message.id);
        if (message.error) {
            request.reject(new Error(message.error));
            return;
        }
        this._remember(request.key, message.result);
        request.resolve(message.result);
    }

    _fallback(request) {
        setTimeout(() => {
            try {
                const result = PokerProbabilityCore.calculate({
                    ...request.options,
                    simulations:Math.min(500, request.options.simulations),
                    minSamples:300,
                    timeBudgetMs:75
                });
                result.method = 'reduced-fallback';
                this._remember(request.key, result);
                request.resolve(result);
            } catch (error) {
                request.reject(error);
            }
        }, 0);
    }

    calculate(options) {
        const normalized = {
            ...options,
            numOpponents:Math.max(1, Math.floor(Number(options.numOpponents) || 1))
        };
        normalized.simulations = Math.max(900, Math.floor(4400 / (1 + .36 * (normalized.numOpponents - 1))));
        normalized.minSamples = Math.min(800, normalized.simulations);
        normalized.timeBudgetMs = 500;
        const key = this.makeKey(normalized);
        if (this.cache.has(key)) return Promise.resolve(this.cache.get(key));
        for (const request of this.pending.values()) {
            if (request.key === key) return request.promise;
        }

        const id = ++this.sequence;
        let resolve;
        let reject;
        const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
        const request = { id, key, options:{ ...normalized, seed:key }, resolve, reject, promise };
        this.pending.set(id, request);
        const worker = this._ensureWorker();
        if (worker) worker.postMessage({ id, key, options:request.options });
        else {
            this.pending.delete(id);
            this._fallback(request);
        }
        return promise;
    }

    clear() {
        this.cache.clear();
    }
}

const probabilityService = new ProbabilityService();

/** Compatibility facade for diagnostics; gameplay uses ProbabilityService. */
class ProbabilityCalculator {
    static calculateWinProb(playerCards, communityCards, numOpponents = 3, numSimulations = 2000) {
        return PokerProbabilityCore.calculate({
            playerCards,
            communityCards,
            numOpponents,
            simulations:numSimulations,
            minSamples:numSimulations,
            timeBudgetMs:2000,
            seed:'diagnostic-standard',
            isShortDeck:false
        });
    }

    static findOuts(playerCards, communityCards) {
        return PokerProbabilityCore.findImprovements(playerCards, communityCards, false);
    }
}
