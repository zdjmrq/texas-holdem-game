/**
 * Original professional poker-table soundtrack.
 *
 * Music is file-backed so every local and network game hears the exact same
 * authored arrangements. Game/turn sound effects remain independent in ui.js.
 */
class BackgroundMusic {
    constructor() {
        this.audio = null;
        this.ctx = null; // Public readiness marker retained for the existing UI.
        this.isPlaying = false;
        this.volume = 0.03;
        this.initialized = false;
        this.currentPattern = 0;
        this.fadeTimer = null;
        this.loadToken = 0;
        this.patterns = [
            { name:'牌桌之下', subtitle:'Beneath the Felt', src:'assets/music/01_Beneath_the_Felt.wav', gain:0.68 },
            { name:'读牌者', subtitle:'The Read', src:'assets/music/02_The_Read.wav', gain:1.00 },
            { name:'压力几何', subtitle:'Pressure Geometry', src:'assets/music/03_Pressure_Geometry.wav', gain:0.90 },
            { name:'河牌之前', subtitle:'Before the River', src:'assets/music/04_Before_the_River.wav', gain:1.00 },
            { name:'无声诈唬', subtitle:'The Silent Bluff', src:'assets/music/05_The_Silent_Bluff.wav', gain:0.92 },
            { name:'最后一盏灯', subtitle:'Last Table Standing', src:'assets/music/06_Last_Table_Standing.wav', gain:0.96 }
        ];
    }

    init() {
        if (this.initialized) return;
        try {
            this.audio = new Audio();
            this.audio.preload = 'auto';
            this.audio.loop = false;
            this.audio.addEventListener('ended', () => {
                if (!this.isPlaying) return;
                this.currentPattern = (this.currentPattern + 1) % this.patterns.length;
                this._loadCurrent();
                this._playLoaded(true);
            });
            this.audio.addEventListener('playing', () => { this.loadToken = 0; });
            this.audio.addEventListener('error', () => {
                if (!this.isPlaying || this.loadToken >= this.patterns.length) return;
                console.warn(`Music track unavailable: ${this.patterns[this.currentPattern]?.src}`);
                this.loadToken++;
                this.currentPattern = (this.currentPattern + 1) % this.patterns.length;
                this._loadCurrent();
                this._playLoaded(false);
            });
            this.ctx = this.audio;
            this.initialized = true;
            this._loadCurrent();
        } catch (error) {
            console.warn('Audio not available:', error);
        }
    }

    _trackVolume() {
        const gain = this.patterns[this.currentPattern]?.gain ?? 1;
        return Math.max(0, Math.min(1, this.volume * gain));
    }

    _clearFade() {
        if (this.fadeTimer) clearInterval(this.fadeTimer);
        this.fadeTimer = null;
    }

    _loadCurrent() {
        if (!this.audio) return;
        const track = this.patterns[this.currentPattern];
        const expected = new URL(track.src, document.baseURI).href;
        if (this.audio.src !== expected) {
            this.audio.src = track.src;
            this.audio.dataset.trackIndex = String(this.currentPattern);
            this.audio.load();
        }
        this.audio.volume = this._trackVolume();
    }

    _playLoaded(fadeIn = true) {
        if (!this.audio || !this.isPlaying) return;
        this._clearFade();
        const target = this._trackVolume();
        this.audio.volume = fadeIn ? 0 : target;
        const promise = this.audio.play();
        if (promise?.catch) promise.catch(error => console.warn('Music playback unavailable:', error));
        if (!fadeIn || target <= 0) return;
        const started = performance.now();
        this.fadeTimer = setInterval(() => {
            if (!this.audio || !this.isPlaying) return this._clearFade();
            const progress = Math.min(1, (performance.now() - started) / 650);
            this.audio.volume = target * progress;
            if (progress >= 1) this._clearFade();
        }, 30);
    }

    start() {
        if (!this.initialized) this.init();
        if (!this.audio || this.isPlaying) return;
        this.isPlaying = true;
        if (this.audio.dataset.trackIndex !== String(this.currentPattern)) this._loadCurrent();
        this._playLoaded(true);
    }

    stop(fadeMs = 350) {
        this.isPlaying = false;
        this._clearFade();
        if (!this.audio) return;
        if (fadeMs <= 0 || this.audio.paused) {
            this.audio.pause();
            return;
        }
        const initial = this.audio.volume;
        const started = performance.now();
        this.fadeTimer = setInterval(() => {
            if (!this.audio) return this._clearFade();
            const progress = Math.min(1, (performance.now() - started) / fadeMs);
            this.audio.volume = initial * (1 - progress);
            if (progress >= 1) {
                this.audio.pause();
                this._clearFade();
            }
        }, 25);
    }

    toggle() {
        if (this.isPlaying) {
            this.stop();
            return false;
        }
        this.start();
        return this.isPlaying;
    }

    setVolume(value) {
        this.volume = Math.max(0, Math.min(0.1, Number(value) || 0));
        if (this.audio && this.isPlaying && !this.fadeTimer) this.audio.volume = this._trackVolume();
    }

    /** Existing skip control calls this method before restarting playback. */
    randomizeKey() {
        this.currentPattern = (this.currentPattern + 1) % this.patterns.length;
    }

    getCurrentPatternName() {
        return this.patterns[this.currentPattern]?.name || '牌桌音乐';
    }
}

const bgMusic = new BackgroundMusic();
