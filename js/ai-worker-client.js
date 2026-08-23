/** Browser-only bridge that keeps local AI calculation off the render thread. */
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
                const request = this.pending.get(message.id);
                if (!request) return;
                this.pending.delete(message.id);
                if (message.error) request.reject(new Error(message.error));
                else request.resolve(message.result);
            };
            this.worker.onerror = error => {
                this.unavailable = true;
                console.warn('AI worker unavailable; using the safe local fallback:', error.message || error);
                for (const request of this.pending.values()) request.reject(new Error('AI worker unavailable'));
                this.pending.clear();
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
        worker.postMessage({ type:'decide', id, key, meta, context:cloneableContext });
        return promise;
    }
}

const aiWorkerService = typeof Worker === 'function' ? new AIWorkerService() : null;
