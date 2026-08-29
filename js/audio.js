/**
 * Four-track poker soundtrack with gapless preloading and crossfades.
 * Game sound effects remain independent in ui.js.
 */
class BackgroundMusic {
    constructor() {
        this.players = [];
        this.audio = null;
        this.ctx = null; // Existing UI uses this as a readiness marker.
        this.activePlayer = 0;
        this.isPlaying = false;
        // Masters are normalized for headroom; the UI percentage should still be
        // clearly audible instead of being multiplied down to near silence.
        this.volume = 0.216;
        this.phaseGain = 1;
        this.initialized = false;
        this.currentPattern = 0;
        this.fadeFrame = 0;
        this.transitionToken = 0;
        this.firstTrackReady = false;
        this.playbackMode = 'sequential';
        this.queuedPattern = null;
        this.patterns = [
            { name:'1', src:'assets/music/01.mp3', gain:1.00 },
            { name:'2', src:'assets/music/02.mp3', gain:1.00 },
            { name:'3', src:'assets/music/03.mp3', gain:1.00 },
            { name:'4', src:'assets/music/04.mp3', gain:1.00 }
        ];
    }

    _emitState(state) {
        document.dispatchEvent(new CustomEvent('poker-music-state', {
            detail:{ state, track:this.getCurrentPatternName() }
        }));
    }

    init() {
        if (this.initialized) return;
        try {
            this.players = [this._createPlayer(0), this._createPlayer(1)];
            this.activePlayer = 0;
            this.audio = this.players[0];
            this.ctx = this.audio;
            this.initialized = true;
            this._load(0, this.currentPattern);
            // Prioritize the first song. It is requested while the player is in
            // the menu instead of only after the game has already started.
            if (this.audio.readyState >= HTMLMediaElement.HAVE_FUTURE_DATA) {
                this._onFirstTrackReady();
            } else {
                this.audio.addEventListener('canplay', () => this._onFirstTrackReady(), { once:true });
            }
        } catch (error) {
            console.warn('Audio not available:', error);
        }
    }

    _onFirstTrackReady() {
        this.firstTrackReady = true;
        this._preloadFollowing();
    }

    _createPlayer(index) {
        const player = new Audio();
        player.preload = index === 0 ? 'auto' : 'metadata';
        player.loop = this.playbackMode === 'repeat-one';
        player.dataset.playerIndex = String(index);
        player.addEventListener('waiting', () => {
            if (this.isPlaying && index === this.activePlayer) this._emitState('loading');
        });
        player.addEventListener('playing', () => {
            if (this.isPlaying && index === this.activePlayer) this._emitState('playing');
        });
        player.addEventListener('ended', () => {
            if (!this.isPlaying || index !== this.activePlayer) return;
            const next = this.queuedPattern ?? this._pickNextPattern();
            this.queuedPattern = null;
            this._transitionTo(next, 350, false);
        });
        player.addEventListener('error', () => {
            if (!this.isPlaying || index !== this.activePlayer) return;
            this._emitState('error');
            console.warn(`Music track unavailable: ${this.patterns[this.currentPattern]?.src}`);
            this.queuedPattern = null;
            this._transitionTo((this.currentPattern + 1) % this.patterns.length, 100, false);
        });
        return player;
    }

    _load(playerIndex, patternIndex, preload = 'auto') {
        const player = this.players[playerIndex];
        const track = this.patterns[patternIndex];
        if (!player || !track) return;
        player.preload = preload;
        const expected = new URL(track.src, document.baseURI).href;
        if (player.src !== expected) {
            player.src = track.src;
            player.dataset.trackIndex = String(patternIndex);
            player.load();
        } else if (preload === 'auto' && player.readyState < HTMLMediaElement.HAVE_CURRENT_DATA) {
            player.load();
        }
    }

    _preloadFollowing() {
        if (!this.initialized) return;
        if (this.playbackMode === 'repeat-one') {
            this.queuedPattern = this.currentPattern;
            return;
        }
        const standby = 1 - this.activePlayer;
        const next = this._pickNextPattern();
        this.queuedPattern = next;
        // These are local assets. Fully buffering the upcoming song after the
        // first one is playable avoids a pause at the track boundary.
        this._load(standby, next, 'auto');
        this.players[standby].volume = 0;
    }

    _pickNextPattern(manual = false) {
        if (!this.patterns.length) return 0;
        if (this.playbackMode === 'repeat-one' && !manual) return this.currentPattern;
        if (this.playbackMode === 'shuffle' && this.patterns.length > 1) {
            let next = this.currentPattern;
            while (next === this.currentPattern) next = Math.floor(Math.random() * this.patterns.length);
            return next;
        }
        return (this.currentPattern + 1) % this.patterns.length;
    }

    _targetVolume(patternIndex = this.currentPattern) {
        const gain = this.patterns[patternIndex]?.gain ?? 1;
        return Math.max(0, Math.min(1, this.volume * gain * this.phaseGain));
    }

    _cancelFade() {
        this.transitionToken++;
        if (this.fadeFrame) cancelAnimationFrame(this.fadeFrame);
        this.fadeFrame = 0;
    }

    _fade(player, from, to, duration, token, onDone) {
        const started = performance.now();
        const tick = now => {
            if (token !== this.transitionToken || !player) return;
            const progress = duration <= 0 ? 1 : Math.min(1, (now - started) / duration);
            const eased = progress * progress * (3 - 2 * progress);
            player.volume = Math.max(0, Math.min(1, from + (to - from) * eased));
            if (progress < 1) this.fadeFrame = requestAnimationFrame(tick);
            else {
                this.fadeFrame = 0;
                if (onDone) onDone();
            }
        };
        this.fadeFrame = requestAnimationFrame(tick);
    }

    _transitionTo(patternIndex, duration = 1400, crossfade = true) {
        if (!this.initialized) this.init();
        if (!this.players.length) return;
        const oldIndex = this.activePlayer;
        const nextIndex = 1 - oldIndex;
        const oldPlayer = this.players[oldIndex];
        const nextPlayer = this.players[nextIndex];
        this._cancelFade();
        const token = this.transitionToken;
        this._load(nextIndex, patternIndex, 'auto');
        try { nextPlayer.currentTime = 0; } catch (_) {}
        nextPlayer.volume = 0;
        this.currentPattern = patternIndex;
        this.queuedPattern = null;
        this.activePlayer = nextIndex;
        this.audio = nextPlayer;
        this.ctx = nextPlayer;
        this._emitState('loading');

        const playPromise = nextPlayer.play();
        if (playPromise?.catch) playPromise.catch(error => console.warn('Music playback unavailable:', error));
        const target = this._targetVolume(patternIndex);
        const oldStart = oldPlayer.ended || !crossfade ? 0 : oldPlayer.volume;
        const started = performance.now();
        const tick = now => {
            if (token !== this.transitionToken) return;
            const progress = duration <= 0 ? 1 : Math.min(1, (now - started) / duration);
            const eased = progress * progress * (3 - 2 * progress);
            nextPlayer.volume = target * eased;
            if (crossfade && !oldPlayer.ended) oldPlayer.volume = oldStart * (1 - eased);
            if (progress < 1) {
                this.fadeFrame = requestAnimationFrame(tick);
                return;
            }
            oldPlayer.pause();
            oldPlayer.volume = 0;
            this.fadeFrame = 0;
            this._preloadFollowing();
        };
        this.fadeFrame = requestAnimationFrame(tick);
    }

    start() {
        if (!this.initialized) this.init();
        if (!this.audio || this.isPlaying) return;
        this.isPlaying = true;
        this._cancelFade();
        const token = this.transitionToken;
        this._load(this.activePlayer, this.currentPattern, 'auto');
        const target = this._targetVolume();
        this.audio.volume = 0;
        this._emitState('loading');
        const promise = this.audio.play();
        if (promise?.catch) promise.catch(error => console.warn('Music playback unavailable:', error));
        // A short ramp prevents clicks without making successful playback feel
        // like it is still loading.
        this._fade(this.audio, 0, target, 220, token, () => this._preloadFollowing());
    }

    stop(fadeMs = 350) {
        this.isPlaying = false;
        this._cancelFade();
        const token = this.transitionToken;
        const active = this.players.filter(player => player && !player.paused)
            .map(player => ({ player, initial:player.volume }));
        if (!active.length) return;
        const started = performance.now();
        const tick = now => {
            if (token !== this.transitionToken) return;
            const progress = fadeMs <= 0 ? 1 : Math.min(1, (now - started) / fadeMs);
            const eased = progress * progress * (3 - 2 * progress);
            for (const item of active) item.player.volume = Math.max(0, item.initial * (1 - eased));
            if (progress < 1) this.fadeFrame = requestAnimationFrame(tick);
            else {
                for (const item of active) item.player.pause();
                this.fadeFrame = 0;
            }
        };
        this.fadeFrame = requestAnimationFrame(tick);
    }

    toggle() {
        if (this.isPlaying) {
            this.stop();
            return false;
        }
        this.start();
        return this.isPlaying;
    }

    next() {
        const nextPattern = this._pickNextPattern(true);
        this.queuedPattern = null;
        if (!this.initialized) this.init();
        if (!this.isPlaying) {
            this.currentPattern = nextPattern;
            this._load(this.activePlayer, nextPattern, 'auto');
            this._preloadFollowing();
            this.start();
        } else {
            this._transitionTo(nextPattern, 1200, true);
        }
        return this.isPlaying;
    }

    setVolume(value) {
        this.volume = Math.max(0, Math.min(.72, Number(value) || 0));
        if (this.audio && this.isPlaying && !this.fadeFrame) this.audio.volume = this._targetVolume();
    }

    setPlaybackMode(mode) {
        const allowed = ['sequential', 'repeat-one', 'shuffle'];
        this.playbackMode = allowed.includes(mode) ? mode : 'sequential';
        this.queuedPattern = null;
        // Native looping is the fastest and most gapless path for one-song mode.
        for (const player of this.players) player.loop = this.playbackMode === 'repeat-one';
        if (this.initialized && this.playbackMode !== 'repeat-one') this._preloadFollowing();
        return this.playbackMode;
    }

    setGamePhase(phase) {
        const gains = { idle:.90, preflop:.92, flop:.97, turn:1.02, river:1.06, showdown:1.04 };
        this.phaseGain = gains[phase] ?? 1;
        if (this.audio && this.isPlaying && !this.fadeFrame) this.audio.volume = this._targetVolume();
    }

    getCurrentPatternName() {
        return this.patterns[this.currentPattern]?.name || '牌桌音乐';
    }
}

const bgMusic = new BackgroundMusic();
