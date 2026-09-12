/** Browser-only bridge that keeps local AI calculation off the render thread. */
// Generous ceiling: the shared brain targets 76-180ms of equity work, so this
// only trips when the worker is genuinely wedged.
const DECIDE_TIMEOUT_MS = 2500;

class AIWorkerService {
    constructor() {
        this.worker = null;
        this.unavailable = false;
        this.sequence = 0;
        this.pending = new Map();
    }

    _ensure() {
        if (this.worker || this.unavailable) return this.worker;
        try {
            this.worker = new Worker('js/ai-worker.js');
            this.worker.onmessage = event => {
                const message = event.data || {};
                if (message.error) this._settle(message.id, null, new Error(message.error));
                else this._settle(message.id, message.result, null);
            };
            this.worker.onerror = error => {
                this.unavailable = true;
                console.warn('AI worker unavailable; using the safe local fallback:', error.message || error);
                // Reject through _settle so every waiter is cleared exactly once,
                // including requests posted while the error was being handled.
                for (const id of [...this.pending.keys()]) this._settle(id, null, new Error('AI worker unavailable'));
                try { this.worker.terminate(); } catch (_) {}
                this.worker = null;
            };
        } catch (error) {
            this.unavailable = true;
            console.warn('AI worker could not start:', error);
        }
        return this.worker;
    }

    post(type, key, meta, payload = {}) {
        const worker = this._ensure();
        if (!worker) return false;
        worker.postMessage({ type, key, meta, ...payload });
        return true;
    }

    observe(key, meta, event) {
        this.post('observe', key, meta, { event });
    }

    showdown(key, meta, seatId, result) {
        this.post('showdown', key, meta, { seatId, result });
    }

    decide(key, meta, context) {
        const worker = this._ensure();
        if (!worker) return Promise.reject(new Error('AI worker unavailable'));
        const id = ++this.sequence;
        const cloneableContext = { ...context };
        delete cloneableContext.evaluateCards;
        const promise = new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
        // A worker that hangs (a stalled search, OOM without onerror) would
        // otherwise leave this promise pending forever and freeze the table.
        // Timing out rejects the decision, and the caller already falls back to
        // the synchronous in-thread brain.
        const timer = setTimeout(() => {
            this._settle(id, null, new Error('AI worker timed out'));
        }, DECIDE_TIMEOUT_MS);
        this.pending.get(id).timer = timer;
        worker.postMessage({ type:'decide', id, key, meta, context:cloneableContext });
        return promise;
    }

    _settle(id, result, error) {
        const request = this.pending.get(id);
        if (!request) return;
        this.pending.delete(id);
        if (request.timer) clearTimeout(request.timer);
        if (error) request.reject(error);
        else request.resolve(result);
    }
}

const aiWorkerService = typeof Worker === 'function' ? new AIWorkerService() : null;
