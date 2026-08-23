/**
 * Background Music for Texas Hold'em Poker — late-night card-room set
 * Original Web Audio arrangements: jazz, swing, blues, lo-fi and western noir.
 */

class BackgroundMusic {
    constructor() {
        this.ctx = null;
        this.isPlaying = false;
        this.volume = 0.03;
        this.initialized = false;
        this.nodes = [];
        this.loopTimer = null;
        this.stopTimer = null;

        // Musical state
        this.currentPattern = 0;
        this.keyOffset = 0;   // semitone transposition (0-11)
        this.tempo = 84;      // BPM
        this.beatDuration = 60 / this.tempo;

        // Song patterns
        this.patterns = [
            { name: '午夜牌桌', tempo: 84, renderer: 'jazz' },
            { name: '维加斯摇摆', tempo: 104, renderer: 'swing' },
            { name: '烟雾蓝调', tempo: 80, renderer: 'blues' },
            { name: '筹码夜色', tempo: 76, renderer: 'lofi' },
            { name: '河牌决斗', tempo: 72, renderer: 'western' },
            { name: '高额桌律动', tempo: 94, renderer: 'funk' }
        ];
    }

    init() {
        if (this.initialized) return;
        try {
            this.ctx = new (window.AudioContext || window.webkitAudioContext)();
            this.masterGain = this.ctx.createGain();
            this.masterGain.gain.value = this.volume;
            this.masterGain.connect(this.ctx.destination);
            this.initialized = true;
        } catch (e) {
            console.warn('Audio not available:', e);
        }
    }

    start() {
        if (!this.initialized) this.init();
        if (!this.ctx || this.isPlaying) return;
        if (this.stopTimer) {
            clearTimeout(this.stopTimer);
            this.stopTimer = null;
        }
        if (this.ctx.state === 'suspended') this.ctx.resume().catch(() => {});
        this.isPlaying = true;
        const now = this.ctx.currentTime;
        this.masterGain.gain.cancelScheduledValues(now);
        this.masterGain.gain.setValueAtTime(0.0001, now);
        this.masterGain.gain.linearRampToValueAtTime(this.volume, now + 0.38);
        this.scheduleLoop();
    }

    stop(fadeMs = 350) {
        this.isPlaying = false;
        if (this.loopTimer) clearTimeout(this.loopTimer);
        this.loopTimer = null;
        if (this.stopTimer) clearTimeout(this.stopTimer);
        if (!this.ctx || !this.masterGain || fadeMs <= 0) {
            this.cleanupNodes();
            return;
        }
        const now = this.ctx.currentTime;
        this.masterGain.gain.cancelScheduledValues(now);
        this.masterGain.gain.setValueAtTime(Math.max(0.0001, this.masterGain.gain.value), now);
        this.masterGain.gain.linearRampToValueAtTime(0.0001, now + fadeMs / 1000);
        this.stopTimer = setTimeout(() => {
            this.cleanupNodes();
            this.stopTimer = null;
        }, fadeMs + 30);
    }

    toggle() {
        if (this.isPlaying) { this.stop(); return false; }
        else { this.start(); return true; }
    }

    setVolume(v) {
        this.volume = Math.max(0, Math.min(0.1, v));
        if (this.masterGain && this.ctx && this.isPlaying) {
            const now = this.ctx.currentTime;
            this.masterGain.gain.cancelScheduledValues(now);
            this.masterGain.gain.linearRampToValueAtTime(Math.max(0.0001, this.volume), now + 0.08);
        }
    }

    /** Randomize key and cycle to next pattern — called by skip button */
    randomizeKey() {
        const cardRoomOffsets = [-3, -2, 0, 2, 3];
        this.keyOffset = cardRoomOffsets[Math.floor(Math.random() * cardRoomOffsets.length)];
        this.currentPattern = (this.currentPattern + 1) % this.patterns.length;
        this.tempo = this.patterns[this.currentPattern].tempo;
        this.beatDuration = 60 / this.tempo;
    }

    getCurrentPatternName() {
        return this.patterns[this.currentPattern]?.name || '牌桌音乐';
    }

    cleanupNodes() {
        for (const n of this.nodes) {
            try { if (n.stop) n.stop(); } catch(e) {}
            try { if (n.disconnect) n.disconnect(); } catch(e) {}
        }
        this.nodes = [];
    }

    // ==================== Semitone Transposition ====================
    /** Shift a frequency by keyOffset semitones */
    _transpose(freq) {
        return freq * Math.pow(2, this.keyOffset / 12);
    }

    // ==================== Synth Voices ====================

    /** Lead synth: sawtooth + square, filter sweep */
    leadSynth(freq, startTime, duration, vel = 0.25, glide = 0) {
        if (!this.ctx || !this.isPlaying) return;
        const f = this._transpose(freq);

        const osc1 = this.ctx.createOscillator();
        osc1.type = 'sawtooth';
        osc1.frequency.setValueAtTime(f, startTime);
        if (glide > 0) osc1.frequency.linearRampToValueAtTime(f * 1.015, startTime + glide);

        const osc2 = this.ctx.createOscillator();
        osc2.type = 'square';
        osc2.frequency.setValueAtTime(f * 0.5, startTime);

        const filter = this.ctx.createBiquadFilter();
        filter.type = 'lowpass';
        filter.frequency.setValueAtTime(180, startTime);
        filter.frequency.linearRampToValueAtTime(1800, startTime + 0.08);
        filter.frequency.exponentialRampToValueAtTime(700, startTime + duration * 0.7);
        filter.Q.value = 4;

        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(0, startTime);
        gain.gain.linearRampToValueAtTime(vel, startTime + 0.02);
        gain.gain.setValueAtTime(vel * 0.9, startTime + duration * 0.5);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + duration);

        osc1.connect(filter);
        osc2.connect(filter);
        filter.connect(gain);
        gain.connect(this.masterGain);
        osc1.start(startTime);
        osc1.stop(startTime + duration + 0.05);
        osc2.start(startTime);
        osc2.stop(startTime + duration + 0.05);
        this.nodes.push(osc1, osc2, filter, gain);
    }

    /** Jazz organ / Rhodes style pad */
    rhodes(freq, startTime, duration, vel = 0.15) {
        if (!this.ctx || !this.isPlaying) return;
        const f = this._transpose(freq);

        for (const ratio of [1, 1.25, 1.5, 2]) {
            const osc = this.ctx.createOscillator();
            osc.type = 'sine';
            osc.frequency.setValueAtTime(f * ratio, startTime);

            // Subtle vibrato
            const lfo = this.ctx.createOscillator();
            lfo.type = 'sine';
            lfo.frequency.value = 4 + Math.random() * 1;
            const lfoGain = this.ctx.createGain();
            lfoGain.gain.value = 2;

            const gain = this.ctx.createGain();
            gain.gain.setValueAtTime(0, startTime);
            gain.gain.linearRampToValueAtTime(vel * 0.3, startTime + 0.05);
            gain.gain.setValueAtTime(vel * 0.25, startTime + duration * 0.5);
            gain.gain.exponentialRampToValueAtTime(0.001, startTime + duration);

            lfo.connect(lfoGain);
            lfoGain.connect(osc.frequency);
            osc.connect(gain);
            gain.connect(this.masterGain);
            osc.start(startTime);
            osc.stop(startTime + duration + 0.05);
            lfo.start(startTime);
            lfo.stop(startTime + duration + 0.05);
            this.nodes.push(osc, gain, lfo, lfoGain);
        }
    }

    /** Funky synth stab with chord */
    synthStab(freq, startTime, vel = 0.18) {
        if (!this.ctx || !this.isPlaying) return;
        const f = this._transpose(freq);
        const intervals = [1, 1.19, 1.5, 1.75];

        for (const ratio of intervals) {
            const o = this.ctx.createOscillator();
            o.type = 'sawtooth';
            o.frequency.setValueAtTime(f * ratio, startTime);

            const filter = this.ctx.createBiquadFilter();
            filter.type = 'lowpass';
            filter.frequency.setValueAtTime(250, startTime);
            filter.frequency.exponentialRampToValueAtTime(2500, startTime + 0.03);
            filter.frequency.exponentialRampToValueAtTime(350, startTime + 0.2);

            const g = this.ctx.createGain();
            g.gain.setValueAtTime(0, startTime);
            g.gain.linearRampToValueAtTime(vel * 0.5, startTime + 0.01);
            g.gain.exponentialRampToValueAtTime(0.001, startTime + 0.28);

            o.connect(filter);
            filter.connect(g);
            g.connect(this.masterGain);
            o.start(startTime);
            o.stop(startTime + 0.32);
            this.nodes.push(o, filter, g);
        }
    }

    /** 808 sub-bass */
    bass808(freq, startTime, duration, vel = 0.3) {
        if (!this.ctx || !this.isPlaying) return;
        const f = this._transpose(freq);

        const osc = this.ctx.createOscillator();
        osc.type = 'sine';
        osc.frequency.setValueAtTime(f, startTime);
        osc.frequency.linearRampToValueAtTime(f * 0.98, startTime + 0.04);

        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(0, startTime);
        gain.gain.linearRampToValueAtTime(vel, startTime + 0.01);
        gain.gain.setValueAtTime(vel, startTime + duration * 0.6);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + duration);

        const filter = this.ctx.createBiquadFilter();
        filter.type = 'lowpass';
        filter.frequency.value = 250;

        osc.connect(gain);
        gain.connect(filter);
        filter.connect(this.masterGain);
        osc.start(startTime);
        osc.stop(startTime + duration + 0.05);
        this.nodes.push(osc, gain, filter);
    }

    /** Walking upright bass (Noir Jazz) */
    uprightBass(freq, startTime, duration, vel = 0.2) {
        if (!this.ctx || !this.isPlaying) return;
        const f = this._transpose(freq);

        const osc = this.ctx.createOscillator();
        osc.type = 'sine';
        osc.frequency.setValueAtTime(f, startTime);
        osc.frequency.linearRampToValueAtTime(f * 1.005, startTime + 0.02);

        // "Pluck" envelope
        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(0, startTime);
        gain.gain.linearRampToValueAtTime(vel, startTime + 0.005);
        gain.gain.exponentialRampToValueAtTime(vel * 0.3, startTime + 0.05);
        gain.gain.setValueAtTime(vel * 0.3, startTime + duration * 0.6);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + duration);

        const filter = this.ctx.createBiquadFilter();
        filter.type = 'lowpass';
        filter.frequency.value = 400;
        filter.Q.value = 2;

        osc.connect(gain);
        gain.connect(filter);
        filter.connect(this.masterGain);
        osc.start(startTime);
        osc.stop(startTime + duration + 0.05);
        this.nodes.push(osc, gain, filter);
    }

    /** Kick */
    kick(startTime, vel = 0.4) {
        if (!this.ctx || !this.isPlaying) return;
        const osc = this.ctx.createOscillator();
        osc.type = 'sine';
        osc.frequency.setValueAtTime(150, startTime);
        osc.frequency.exponentialRampToValueAtTime(38, startTime + 0.1);

        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(vel, startTime);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + 0.15);
        osc.connect(gain);
        gain.connect(this.masterGain);
        osc.start(startTime);
        osc.stop(startTime + 0.2);
        this.nodes.push(osc, gain);
    }

    /** Kick with more attack (trap style) */
    trapKick(startTime, vel = 0.45) {
        if (!this.ctx || !this.isPlaying) return;
        const osc = this.ctx.createOscillator();
        osc.type = 'sine';
        osc.frequency.setValueAtTime(200, startTime);
        osc.frequency.exponentialRampToValueAtTime(35, startTime + 0.12);

        // Click layer
        const osc2 = this.ctx.createOscillator();
        osc2.type = 'sine';
        osc2.frequency.setValueAtTime(800, startTime);
        osc2.frequency.exponentialRampToValueAtTime(100, startTime + 0.03);

        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(vel, startTime);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + 0.18);

        const gain2 = this.ctx.createGain();
        gain2.gain.setValueAtTime(vel * 0.5, startTime);
        gain2.gain.exponentialRampToValueAtTime(0.001, startTime + 0.03);

        osc.connect(gain);
        gain.connect(this.masterGain);
        osc2.connect(gain2);
        gain2.connect(this.masterGain);
        osc.start(startTime);
        osc.stop(startTime + 0.2);
        osc2.start(startTime);
        osc2.stop(startTime + 0.05);
        this.nodes.push(osc, osc2, gain, gain2);
    }

    /** Snare */
    snare(startTime, vel = 0.22) {
        if (!this.ctx || !this.isPlaying) return;
        const bufferSize = this.ctx.sampleRate * 0.1;
        const buffer = this.ctx.createBuffer(1, bufferSize, this.ctx.sampleRate);
        const data = buffer.getChannelData(0);
        for (let i = 0; i < bufferSize; i++) {
            data[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / bufferSize, 3);
        }
        const source = this.ctx.createBufferSource();
        source.buffer = buffer;

        const filter = this.ctx.createBiquadFilter();
        filter.type = 'highpass';
        filter.frequency.value = 900;

        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(0, startTime);
        gain.gain.linearRampToValueAtTime(vel, startTime + 0.002);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + 0.1);

        const osc = this.ctx.createOscillator();
        osc.type = 'sine';
        osc.frequency.setValueAtTime(170, startTime);
        osc.frequency.exponentialRampToValueAtTime(75, startTime + 0.08);
        const gain2 = this.ctx.createGain();
        gain2.gain.setValueAtTime(vel * 0.35, startTime);
        gain2.gain.exponentialRampToValueAtTime(0.001, startTime + 0.08);

        source.connect(filter);
        filter.connect(gain);
        gain.connect(this.masterGain);
        osc.connect(gain2);
        gain2.connect(this.masterGain);
        source.start(startTime);
        osc.start(startTime);
        source.stop(startTime + 0.12);
        osc.stop(startTime + 0.1);
        this.nodes.push(source, filter, gain, osc, gain2);
    }

    /** Clap (trap style) */
    clap(startTime, vel = 0.2) {
        if (!this.ctx || !this.isPlaying) return;
        const bufferSize = this.ctx.sampleRate * 0.08;
        const buffer = this.ctx.createBuffer(1, bufferSize, this.ctx.sampleRate);
        const data = buffer.getChannelData(0);
        for (let i = 0; i < bufferSize; i++) {
            data[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / bufferSize, 4);
        }
        const source = this.ctx.createBufferSource();
        source.buffer = buffer;

        const filter = this.ctx.createBiquadFilter();
        filter.type = 'bandpass';
        filter.frequency.value = 1800;
        filter.Q.value = 1.5;

        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(0, startTime);
        gain.gain.linearRampToValueAtTime(vel, startTime + 0.003);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + 0.12);

        source.connect(filter);
        filter.connect(gain);
        gain.connect(this.masterGain);
        source.start(startTime);
        source.stop(startTime + 0.1);
        this.nodes.push(source, filter, gain);
    }

    /** Hi-hat */
    hihat(startTime, vel = 0.08, open = false) {
        if (!this.ctx || !this.isPlaying) return;
        const len = open ? 0.15 : 0.04;
        const bufferSize = this.ctx.sampleRate * len;
        const buffer = this.ctx.createBuffer(1, bufferSize, this.ctx.sampleRate);
        const data = buffer.getChannelData(0);
        for (let i = 0; i < bufferSize; i++) {
            data[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / bufferSize, 2);
        }
        const source = this.ctx.createBufferSource();
        source.buffer = buffer;

        const filter = this.ctx.createBiquadFilter();
        filter.type = 'highpass';
        filter.frequency.value = open ? 5500 : 7500;

        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(vel, startTime);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + len);

        source.connect(filter);
        filter.connect(gain);
        gain.connect(this.masterGain);
        source.start(startTime);
        source.stop(startTime + len + 0.02);
        this.nodes.push(source, filter, gain);
    }

    /** Open hi-hat */
    openHat(startTime, vel = 0.06) { this.hihat(startTime, vel, true); }

    /** Brush / ride cymbal (jazz) */
    brush(startTime, vel = 0.04) {
        if (!this.ctx || !this.isPlaying) return;
        const bufferSize = this.ctx.sampleRate * 0.04;
        const buffer = this.ctx.createBuffer(1, bufferSize, this.ctx.sampleRate);
        const data = buffer.getChannelData(0);
        for (let i = 0; i < bufferSize; i++) {
            data[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / bufferSize, 2);
        }
        const source = this.ctx.createBufferSource();
        source.buffer = buffer;
        const filter = this.ctx.createBiquadFilter();
        filter.type = 'highpass';
        filter.frequency.value = 3000;
        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(vel, startTime);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + 0.04);
        source.connect(filter);
        filter.connect(gain);
        gain.connect(this.masterGain);
        source.start(startTime);
        source.stop(startTime + 0.05);
        this.nodes.push(source, filter, gain);
    }

    /** Casino chip sound */
    chipShuffle(startTime) {
        if (!this.ctx || !this.isPlaying) return;
        for (let j = 0; j < 3; j++) {
            const t = startTime + j * 0.04 + Math.random() * 0.01;
            const freq = 1800 + Math.random() * 2000;
            const osc = this.ctx.createOscillator();
            osc.type = 'sine';
            osc.frequency.setValueAtTime(freq, t);
            const gain = this.ctx.createGain();
            gain.gain.setValueAtTime(0.012, t);
            gain.gain.exponentialRampToValueAtTime(0.001, t + 0.05);
            osc.connect(gain);
            gain.connect(this.masterGain);
            osc.start(t);
            osc.stop(t + 0.06);
            this.nodes.push(osc, gain);
        }
    }

    // ==================== Additional Synth Voices ====================

    /** Brass / trumpet-like — sawtooth with fast attack, medium decay, slight pitch bend */
    brassSynth(freq, startTime, duration, vel = 0.2) {
        if (!this.ctx || !this.isPlaying) return;
        const f = this._transpose(freq);

        const osc = this.ctx.createOscillator();
        osc.type = 'sawtooth';
        osc.frequency.setValueAtTime(f, startTime);
        osc.frequency.linearRampToValueAtTime(f * 1.01, startTime + 0.03);
        osc.frequency.linearRampToValueAtTime(f * 0.995, startTime + 0.08);

        const filter = this.ctx.createBiquadFilter();
        filter.type = 'lowpass';
        filter.frequency.setValueAtTime(400, startTime);
        filter.frequency.linearRampToValueAtTime(2500, startTime + 0.04);
        filter.frequency.exponentialRampToValueAtTime(600, startTime + duration * 0.7);
        filter.Q.value = 6;

        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(0, startTime);
        gain.gain.linearRampToValueAtTime(vel, startTime + 0.01);
        gain.gain.setValueAtTime(vel, startTime + duration * 0.5);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + duration);

        osc.connect(filter);
        filter.connect(gain);
        gain.connect(this.masterGain);
        osc.start(startTime);
        osc.stop(startTime + duration + 0.05);
        this.nodes.push(osc, filter, gain);
    }

    /** Plucked string — fast-decaying sine + harmonics, banjo/guitar feel */
    pluckedString(freq, startTime, vel = 0.15) {
        if (!this.ctx || !this.isPlaying) return;
        const f = this._transpose(freq);

        for (const ratio of [1, 2, 3]) {
            const osc = this.ctx.createOscillator();
            osc.type = 'sine';
            osc.frequency.setValueAtTime(f * ratio, startTime);
            // Slight downward pitch bend for "pluck"
            osc.frequency.linearRampToValueAtTime(f * ratio * 0.98, startTime + 0.03);

            const gain = this.ctx.createGain();
            gain.gain.setValueAtTime(0, startTime);
            gain.gain.linearRampToValueAtTime(vel / ratio, startTime + 0.002);
            gain.gain.exponentialRampToValueAtTime(0.001, startTime + 0.2 + Math.random() * 0.1);

            osc.connect(gain);
            gain.connect(this.masterGain);
            osc.start(startTime);
            osc.stop(startTime + 0.3);
            this.nodes.push(osc, gain);
        }
    }

    /** Whistle — pure sine with vibrato, western feel */
    whistle(freq, startTime, duration, vel = 0.15) {
        if (!this.ctx || !this.isPlaying) return;
        const f = this._transpose(freq);

        const osc = this.ctx.createOscillator();
        osc.type = 'sine';
        osc.frequency.setValueAtTime(f, startTime);

        // Vibrato LFO
        const lfo = this.ctx.createOscillator();
        lfo.type = 'sine';
        lfo.frequency.value = 5;
        const lfoGain = this.ctx.createGain();
        lfoGain.gain.value = 3;
        lfo.connect(lfoGain);
        lfoGain.connect(osc.frequency);

        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(0, startTime);
        gain.gain.linearRampToValueAtTime(vel, startTime + 0.05);
        gain.gain.setValueAtTime(vel * 0.9, startTime + duration * 0.6);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + duration);

        const filter = this.ctx.createBiquadFilter();
        filter.type = 'highpass';
        filter.frequency.value = 500;

        osc.connect(filter);
        filter.connect(gain);
        gain.connect(this.masterGain);
        osc.start(startTime);
        osc.stop(startTime + duration + 0.05);
        lfo.start(startTime);
        lfo.stop(startTime + duration + 0.05);
        this.nodes.push(osc, gain, filter, lfo, lfoGain);
    }

    /** Noise pad — filtered noise for ambient texture / lo-fi */
    noisePad(startTime, duration, vel = 0.06, lowCut = 200, highCut = 3000) {
        if (!this.ctx || !this.isPlaying) return;
        const bufferSize = this.ctx.sampleRate * duration;
        const buffer = this.ctx.createBuffer(1, bufferSize, this.ctx.sampleRate);
        const data = buffer.getChannelData(0);
        for (let i = 0; i < bufferSize; i++) {
            const t = i / this.ctx.sampleRate;
            data[i] = (Math.random() * 2 - 1) * Math.pow(1 - t / duration, 2);
        }
        const source = this.ctx.createBufferSource();
        source.buffer = buffer;

        const bandpass = this.ctx.createBiquadFilter();
        bandpass.type = 'bandpass';
        bandpass.frequency.value = (lowCut + highCut) / 2;
        bandpass.Q.value = 0.8;

        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(0, startTime);
        gain.gain.linearRampToValueAtTime(vel, startTime + 0.1);
        gain.gain.setValueAtTime(vel, startTime + duration * 0.5);
        gain.gain.exponentialRampToValueAtTime(0.001, startTime + duration);

        source.connect(bandpass);
        bandpass.connect(gain);
        gain.connect(this.masterGain);
        source.start(startTime);
        source.stop(startTime + duration + 0.05);
        this.nodes.push(source, bandpass, gain);
    }

    // ==================== Pattern: West Coast Funk ====================
    _patternFunk(t, beatDuration, beats) {
        const bd = beatDuration;
        // Bass line — syncopated 808
        const bassNotes = [
            { f: 65.41, d: 1.5 }, { f: 65.41, d: 1.5 },
            { f: 73.42, d: 1.0 }, { f: 65.41, d: 1.5 },
            { f: 65.41, d: 1.5 }, { f: 55.00, d: 1.0 },
            { f: 65.41, d: 1.5 }, { f: 77.78, d: 1.0 },
        ];
        let bp = 0;
        for (const n of bassNotes) {
            this.bass808(n.f, t + bp * bd, n.d * bd, 0.28);
            bp += n.d;
        }

        // Lead melody
        const lead = [
            { f: 523.25, sb: 0.5, d: 0.5 }, { f: 587.33, sb: 1.0, d: 0.3 },
            { f: 523.25, sb: 1.5, d: 0.5 }, { f: 440.00, sb: 2.0, d: 0.4 },
            { f: 587.33, sb: 3.0, d: 0.8 },
            { f: 659.25, sb: 4.5, d: 0.4 }, { f: 587.33, sb: 5.0, d: 0.3 },
            { f: 523.25, sb: 5.5, d: 0.5 }, { f: 440.00, sb: 6.0, d: 0.4 },
            { f: 523.25, sb: 6.5, d: 0.3 }, { f: 392.00, sb: 7.0, d: 0.8 },
            { f: 349.23, sb: 8.0, d: 0.5 }, { f: 392.00, sb: 8.5, d: 0.3 },
            { f: 440.00, sb: 9.0, d: 0.5 }, { f: 523.25, sb: 10.0, d: 0.8 },
            { f: 440.00, sb: 11.0, d: 0.6 },
            { f: 440.00, sb: 12.0, d: 0.4 }, { f: 523.25, sb: 12.5, d: 0.3 },
            { f: 587.33, sb: 13.0, d: 0.5 }, { f: 659.25, sb: 14.0, d: 0.5 },
            { f: 523.25, sb: 15.0, d: 1.0 },
        ];
        for (const n of lead) {
            this.leadSynth(n.f, t + n.sb * bd, n.d * bd * 0.75, 0.18, 0.03);
        }

        // Stabs
        const roots = [261.63, 311.13, 349.23, 293.66];
        for (let bar = 0; bar < 4; bar++) {
            this.synthStab(roots[bar % 4], t + bar * 4 * bd, 0.1);
            this.synthStab(roots[(bar + 1) % 4] * 1.5, t + (bar * 4 + 2.5) * bd, 0.07);
        }

        // Drums
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            if (beat % 2 === 0) this.kick(bt, 0.38);
            if (beat % 4 === 1) this.kick(bt + bd * 0.5, 0.22);
            if (beat % 2 === 1) this.snare(bt, 0.2);
            const sw = beat % 2 === 1 ? bd * 0.08 : 0;
            this.hihat(bt + sw, beat % 2 === 0 ? 0.07 : 0.05);
            if (beat < beats - 1) this.hihat(bt + bd * 0.5 + sw * 0.3, 0.03);
        }

        // Chips
        for (let i = 0; i < 2; i++) this.chipShuffle(t + 3 + i * 6 + Math.random() * 0.5);

        // Ending swell
        const rs = t + 14 * bd;
        for (let i = 0; i < 4; i++) this.leadSynth(400 + i * 200, rs + i * bd * 0.25, bd * 0.25, 0.1, 0.02);
    }

    // ==================== Pattern: Electro Swing ====================
    _patternSwing(t, beatDuration, beats) {
        const bd = beatDuration;

        // Walking bass
        const bassNotes = [
            { f: 65.41, d: 1.0 }, { f: 73.42, d: 1.0 }, { f: 82.41, d: 1.0 }, { f: 87.31, d: 1.0 },
            { f: 98.00, d: 1.0 }, { f: 87.31, d: 1.0 }, { f: 82.41, d: 1.0 }, { f: 73.42, d: 1.0 },
            { f: 65.41, d: 1.0 }, { f: 73.42, d: 1.0 }, { f: 82.41, d: 1.0 }, { f: 65.41, d: 1.0 },
            { f: 55.00, d: 1.0 }, { f: 65.41, d: 1.0 }, { f: 73.42, d: 1.0 }, { f: 82.41, d: 1.0 },
        ];
        for (let i = 0; i < beats; i++) {
            this.uprightBass(bassNotes[i % 16].f, t + i * bd, bd * 0.9, 0.18);
        }

        // Staccato Rhodes chords (swing feel)
        for (let beat = 0; beat < beats; beat += 2) {
            const chordRoot = [261.63, 329.63, 392.00, 349.23][(beat / 2) % 4];
            const swingDelay = (beat % 4 === 1) ? bd * 0.12 : 0;
            this.rhodes(chordRoot, t + beat * bd + swingDelay, bd * 1.2, 0.12);
        }

        // Lead melody — bright, bouncy
        const melody = [
            { f: 784.0, sb: 0.0, d: 0.6 }, { f: 880.0, sb: 1.0, d: 0.4 },
            { f: 1046.5, sb: 2.0, d: 0.8 }, { f: 880.0, sb: 3.0, d: 0.6 },
            { f: 784.0, sb: 4.0, d: 0.4 }, { f: 659.3, sb: 5.0, d: 0.6 },
            { f: 880.0, sb: 6.0, d: 0.8 }, { f: 784.0, sb: 7.0, d: 0.6 },
            { f: 1046.5, sb: 8.0, d: 0.5 }, { f: 987.8, sb: 9.0, d: 0.4 },
            { f: 880.0, sb: 10.0, d: 0.8 }, { f: 784.0, sb: 11.0, d: 0.6 },
            { f: 659.3, sb: 12.0, d: 0.4 }, { f: 880.0, sb: 13.0, d: 0.4 },
            { f: 1046.5, sb: 14.0, d: 0.8 }, { f: 784.0, sb: 15.0, d: 1.2 },
        ];
        for (const n of melody) {
            this.leadSynth(n.f, t + n.sb * bd, n.d * bd * 0.7, 0.16, 0.02);
        }

        // Drums — swing pattern
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            const swing = (beat % 2 === 1) ? bd * 0.1 : 0;
            // Kick on 1, 3 (more groove)
            if (beat % 4 === 0 || beat % 4 === 2) this.kick(bt, 0.35);
            // Snare on 2, 4
            if (beat % 4 === 1 || beat % 4 === 3) this.snare(bt + swing * 0.5, 0.18);
            // Hi-hat — swing eighth notes
            this.hihat(bt + swing, 0.06);
            this.hihat(bt + bd * 0.5 + swing * 0.3, 0.035);
        }
    }

    // ==================== Pattern: Lounge Trap ====================
    _patternTrap(t, beatDuration, beats) {
        const bd = beatDuration;

        // Deep 808 bass — half-time feel
        const bassNotes = [
            { f: 43.65, d: 4 }, { f: 55.00, d: 4 },  // G1, A1
            { f: 49.00, d: 4 }, { f: 43.65, d: 4 },  // B1, G1
        ];
        let bp = 0;
        for (const n of bassNotes) {
            this.bass808(n.f, t + bp * bd, n.d * bd, 0.35);
            bp += n.d;
        }

        // Atmospheric pads
        const padRoots = [130.81, 146.83, 164.81, 130.81]; // C3, D3, E3, C3
        for (let i = 0; i < 4; i++) {
            this.rhodes(padRoots[i], t + i * 4 * bd, bd * 3.8, 0.08);
        }

        // Sparse synth bells
        const bells = [
            { f: 523.25, sb: 1.0, d: 1.5 }, { f: 659.25, sb: 3.0, d: 1.0 },
            { f: 587.33, sb: 5.0, d: 1.5 }, { f: 523.25, sb: 7.0, d: 1.0 },
            { f: 783.99, sb: 9.0, d: 1.5 }, { f: 659.25, sb: 11.0, d: 1.0 },
            { f: 880.00, sb: 13.0, d: 1.5 }, { f: 1046.5, sb: 15.0, d: 2.0 },
        ];
        for (const n of bells) {
            this.leadSynth(n.f, t + n.sb * bd, n.d * bd * 0.8, 0.12);
        }

        // Trap drums — half-time
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            // Kick on 1, 3, 5, 7 (and syncopated)
            if (beat % 2 === 0) this.trapKick(bt, 0.4);
            if (beat % 8 === 3) this.trapKick(bt + bd * 0.5, 0.25);
            // Clap on 2, 4, 6, 8 (and double time on 4th bar)
            if (beat % 2 === 1) this.clap(bt, 0.18);
            if (beat % 4 === 3) this.clap(bt + bd * 0.5, 0.12);
            // Hi-hat — rapid sixteenths with rolls
            this.hihat(bt, 0.08);
            this.hihat(bt + bd * 0.25, 0.04);
            this.hihat(bt + bd * 0.5, 0.06);
            this.hihat(bt + bd * 0.75, 0.03);
            // Open hat accents
            if (beat % 8 === 0) this.openHat(bt + bd * 0.75, 0.04);
        }

        // Chip sounds
        for (let i = 0; i < 3; i++) this.chipShuffle(t + 2 + i * 4.5 + Math.random());

        // 808 slides on last bar
        const slideStart = t + 14 * bd;
        for (let i = 0; i < 4; i++) {
            this.bass808(55 + i * 15, slideStart + i * bd * 0.5, bd * 0.7, 0.15);
        }
    }

    // ==================== Pattern: Noir Jazz ====================
    _patternJazz(t, beatDuration, beats) {
        const bd = beatDuration;

        // Slow walking upright bass
        const bassNotes = [
            { f: 65.41, d: 2 }, { f: 73.42, d: 2 }, { f: 82.41, d: 2 }, { f: 65.41, d: 2 },
            { f: 55.00, d: 2 }, { f: 65.41, d: 2 }, { f: 73.42, d: 2 }, { f: 82.41, d: 2 },
        ];
        let bp = 0;
        for (const n of bassNotes) {
            this.uprightBass(n.f, t + bp * bd, n.d * bd * 0.85, 0.15);
            bp += n.d;
        }

        // Smoky Rhodes chords
        const jChords = [261.63, 311.13, 349.23, 293.66, 261.63, 349.23, 329.63, 277.18];
        for (let i = 0; i < 8; i++) {
            this.rhodes(jChords[i], t + i * 2 * bd, bd * 1.8, 0.1);
        }

        // Melancholy lead
        const leadNotes = [
            { f: 349.23, sb: 0.5, d: 1.0 }, { f: 392.00, sb: 1.5, d: 0.8 },
            { f: 440.00, sb: 2.5, d: 1.2 }, { f: 349.23, sb: 4.0, d: 0.8 },
            { f: 293.66, sb: 5.0, d: 1.0 }, { f: 329.63, sb: 6.0, d: 0.8 },
            { f: 261.63, sb: 7.0, d: 1.5 },
            { f: 311.13, sb: 8.5, d: 0.8 }, { f: 349.23, sb: 9.5, d: 1.0 },
            { f: 440.00, sb: 10.5, d: 1.2 }, { f: 392.00, sb: 12.0, d: 0.8 },
            { f: 349.23, sb: 13.0, d: 1.0 }, { f: 293.66, sb: 14.0, d: 0.8 },
            { f: 261.63, sb: 15.0, d: 1.5 },
        ];
        for (const n of leadNotes) {
            this.leadSynth(n.f, t + n.sb * bd, n.d * bd * 0.8, 0.14, 0.04);
        }

        // Brush drums (soft jazz)
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            // Brush on 2, 4
            if (beat === 1 || beat === 3 || beat === 5 || beat === 7 ||
                beat === 9 || beat === 11 || beat === 13 || beat === 15) {
                this.brush(bt, 0.035);
            }
            // Hi-hat — light swing
            if (beat % 2 === 0) this.hihat(bt, 0.025);
            // Light kick on 1, 3
            if (beat % 2 === 0) this.kick(bt, 0.25);
        }

        // Piano accent chords
        for (let i = 0; i < 4; i++) {
            const accTime = t + (i * 4 + 1.5) * bd;
            this.synthStab(jChords[i * 2] * 2, accTime, 0.06);
        }
    }

    // ==================== Pattern: Latin Heat ====================
    _patternLatin(t, beatDuration, beats) {
        const bd = beatDuration;

        // Tumbao bass pattern
        const bassNotes = [
            { f: 65.41, d: 1.5 }, { f: 0, d: 0.5 },   // C — rest
            { f: 73.42, d: 0.5 }, { f: 77.78, d: 0.5 }, { f: 82.41, d: 1.0 }, // D–E–F
            { f: 65.41, d: 1.5 }, { f: 0, d: 0.5 },
            { f: 55.00, d: 0.5 }, { f: 65.41, d: 0.5 }, { f: 73.42, d: 1.0 }, // A–C–D
            { f: 65.41, d: 1.5 }, { f: 0, d: 0.5 },
            { f: 82.41, d: 0.5 }, { f: 87.31, d: 0.5 }, { f: 98.00, d: 1.0 }, // F–F#–G
            { f: 65.41, d: 1.5 }, { f: 0, d: 0.5 },
            { f: 49.00, d: 0.5 }, { f: 55.00, d: 0.5 }, { f: 65.41, d: 1.0 }, // B–A–C
        ];
        let bp = 0;
        for (const n of bassNotes) {
            if (n.f > 0) this.uprightBass(n.f, t + bp * bd, n.d * bd * 0.8, 0.16);
            bp += n.d;
        }

        // Spicy montuno-style piano/synth chords — staccato
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            // Chord stab on 1, 3, 5, 7 (every other downbeat)
            if (beat % 4 === 0 || beat % 4 === 2) {
                const chordRoot = [311.13, 349.23, 392.00, 329.63, 311.13, 349.23, 440.00, 392.00][beat % 8];
                this.synthStab(chordRoot, bt, 0.09);
            }
            // Syncopated off-beat "kick" chord
            if (beat % 2 === 1) {
                const chordRoot = [349.23, 392.00, 329.63, 440.00][(beat / 2) % 4];
                this.synthStab(chordRoot * 1.5, bt + bd * 0.3, 0.06);
            }
        }

        // Melody — latin-tinged flute-like lead
        const melody = [
            { f: 587.33, sb: 0.0, d: 0.4 }, { f: 659.25, sb: 0.5, d: 0.3 },
            { f: 783.99, sb: 1.0, d: 0.6 }, { f: 659.25, sb: 1.8, d: 0.3 },
            { f: 587.33, sb: 2.5, d: 0.8 }, { f: 523.25, sb: 3.5, d: 0.4 },
            { f: 659.25, sb: 4.0, d: 0.4 }, { f: 783.99, sb: 4.5, d: 0.3 },
            { f: 880.00, sb: 5.0, d: 0.6 }, { f: 783.99, sb: 5.8, d: 0.3 },
            { f: 659.25, sb: 6.5, d: 0.8 }, { f: 587.33, sb: 7.5, d: 0.4 },
            { f: 523.25, sb: 8.0, d: 0.4 }, { f: 659.25, sb: 8.5, d: 0.3 },
            { f: 783.99, sb: 9.0, d: 0.6 }, { f: 880.00, sb: 9.8, d: 0.3 },
            { f: 1046.5, sb: 10.5, d: 0.8 }, { f: 783.99, sb: 11.5, d: 0.4 },
            { f: 659.25, sb: 12.0, d: 0.4 }, { f: 783.99, sb: 12.5, d: 0.3 },
            { f: 659.25, sb: 13.0, d: 0.6 }, { f: 587.33, sb: 14.0, d: 0.5 },
            { f: 523.25, sb: 15.0, d: 1.0 },
        ];
        for (const n of melody) {
            this.leadSynth(n.f, t + n.sb * bd, n.d * bd * 0.7, 0.16, 0.02);
        }

        // Drums — salsa/breakbeat fusion
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            // Kick: on 1, 3, and syncopated
            if (beat % 2 === 0) this.kick(bt, 0.35);
            if (beat % 4 === 3) this.kick(bt + bd * 0.75, 0.2);
            // Snare: 2, 4 with slight swing
            if (beat % 2 === 1) this.snare(bt + bd * 0.05, 0.18);
            // Hi-hat: driving eighths
            this.hihat(bt, 0.07);
            this.hihat(bt + bd * 0.5, 0.05);
            // Open hat accent on the "and" of 4
            if (beat % 4 === 3) this.openHat(bt + bd * 0.5, 0.04);
        }

        // Maraca/shaker feel — sixteenth note triplet feel
        for (let beat = 0; beat < beats; beat += 2) {
            const shT = t + (beat + 0.25) * bd;
            this.hihat(shT, 0.025);
            this.hihat(shT + bd * 0.15, 0.02);
        }

        // Casino chips
        for (let i = 0; i < 2; i++) this.chipShuffle(t + 3 + i * 6 + Math.random());
    }

    // ==================== Pattern: Retro Arcade ====================
    _patternArcade(t, beatDuration, beats) {
        const bd = beatDuration;

        // Square wave bass — chiptune style
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            const bassFreq = [130.81, 130.81, 146.83, 130.81, 110.00, 110.00, 130.81, 146.83,
                              130.81, 130.81, 164.81, 146.83, 130.81, 110.00, 98.00, 110.00][beat];
            // Use uprightBass with a higher pitch for that 8-bit square wave feel
            // Instead, let's make a quick chiptune bass
            if (!this.ctx || !this.isPlaying) return;
            const f = this._transpose(bassFreq);
            const osc = this.ctx.createOscillator();
            osc.type = 'square';
            osc.frequency.setValueAtTime(f, bt);
            const g = this.ctx.createGain();
            g.gain.setValueAtTime(0.15, bt);
            g.gain.setValueAtTime(0.15, bt + bd * 0.4);
            g.gain.exponentialRampToValueAtTime(0.001, bt + bd * 0.8);
            osc.connect(g);
            g.connect(this.masterGain);
            osc.start(bt);
            osc.stop(bt + bd * 0.85);
            this.nodes.push(osc, g);
        }

        // Bouncy arpeggio lead — classic 8-bit
        const arpNotes = [
            { f: 523.25, sb: 0.0, d: 0.2 }, { f: 659.25, sb: 0.25, d: 0.2 },
            { f: 783.99, sb: 0.5, d: 0.2 }, { f: 1046.5, sb: 0.75, d: 0.3 },
            { f: 783.99, sb: 1.25, d: 0.2 }, { f: 659.25, sb: 1.5, d: 0.2 },
            { f: 523.25, sb: 1.75, d: 0.4 },
            { f: 587.33, sb: 2.25, d: 0.2 }, { f: 698.46, sb: 2.5, d: 0.2 },
            { f: 880.00, sb: 2.75, d: 0.2 }, { f: 1046.5, sb: 3.0, d: 0.3 },
            { f: 880.00, sb: 3.5, d: 0.2 }, { f: 698.46, sb: 3.75, d: 0.2 },
            { f: 587.33, sb: 4.0, d: 0.4 },
            { f: 659.25, sb: 4.5, d: 0.2 }, { f: 783.99, sb: 4.75, d: 0.2 },
            { f: 987.77, sb: 5.0, d: 0.2 }, { f: 1174.7, sb: 5.25, d: 0.3 },
            { f: 987.77, sb: 5.75, d: 0.2 }, { f: 783.99, sb: 6.0, d: 0.2 },
            { f: 659.25, sb: 6.25, d: 0.4 },
            { f: 523.25, sb: 6.75, d: 0.2 }, { f: 659.25, sb: 7.0, d: 0.2 },
            { f: 783.99, sb: 7.25, d: 0.2 }, { f: 1046.5, sb: 7.5, d: 0.6 },
            // Bar 3-4 (variation)
            { f: 392.00, sb: 8.0, d: 0.2 }, { f: 523.25, sb: 8.25, d: 0.2 },
            { f: 659.25, sb: 8.5, d: 0.2 }, { f: 783.99, sb: 8.75, d: 0.3 },
            { f: 659.25, sb: 9.25, d: 0.2 }, { f: 523.25, sb: 9.5, d: 0.2 },
            { f: 392.00, sb: 9.75, d: 0.4 },
            { f: 440.00, sb: 10.25, d: 0.2 }, { f: 587.33, sb: 10.5, d: 0.2 },
            { f: 698.46, sb: 10.75, d: 0.2 }, { f: 880.00, sb: 11.0, d: 0.3 },
            { f: 698.46, sb: 11.5, d: 0.2 }, { f: 587.33, sb: 11.75, d: 0.2 },
            { f: 440.00, sb: 12.0, d: 0.4 },
            { f: 523.25, sb: 12.5, d: 0.2 }, { f: 659.25, sb: 12.75, d: 0.2 },
            { f: 783.99, sb: 13.0, d: 0.2 }, { f: 1046.5, sb: 13.25, d: 0.3 },
            { f: 783.99, sb: 13.75, d: 0.2 }, { f: 659.25, sb: 14.0, d: 0.2 },
            { f: 523.25, sb: 14.25, d: 0.4 }, { f: 392.00, sb: 15.0, d: 0.8 },
        ];
        for (const n of arpNotes) {
            const start = t + n.sb * bd;
            const f = this._transpose(n.f);
            if (!this.ctx || !this.isPlaying) return;
            const osc = this.ctx.createOscillator();
            osc.type = 'square';
            osc.frequency.setValueAtTime(f, start);
            const g = this.ctx.createGain();
            g.gain.setValueAtTime(0, start);
            g.gain.linearRampToValueAtTime(0.13, start + 0.01);
            g.gain.setValueAtTime(0.13, start + n.d * bd * 0.5);
            g.gain.exponentialRampToValueAtTime(0.001, start + n.d * bd * 0.8);
            osc.connect(g);
            g.connect(this.masterGain);
            osc.start(start);
            osc.stop(start + n.d * bd * 0.85);
            this.nodes.push(osc, g);
        }

        // Drums — simple arcade beat
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            // Kick on 1, 3, 5, 7, 9, 11, 13, 15
            if (beat % 2 === 0) this.kick(bt, 0.3);
            // Snare on 2, 4, 6, 8, 10, 12, 14, 16
            if (beat % 2 === 1) this.snare(bt, 0.17);
            // Hi-hat — fast sixteenths for energy
            this.hihat(bt, 0.06);
            this.hihat(bt + bd * 0.25, 0.03);
            this.hihat(bt + bd * 0.5, 0.05);
            this.hihat(bt + bd * 0.75, 0.03);
            // Crash open hat every 4 beats
            if (beat % 4 === 0) this.openHat(bt + bd * 0.9, 0.04);
        }

        // Power-up / coin sound effects (casino meets arcade)
        for (let i = 0; i < 3; i++) {
            const coinTime = t + 3 + i * 5;
            // Rising tone
            if (!this.ctx || !this.isPlaying) return;
            const osc = this.ctx.createOscillator();
            osc.type = 'sine';
            const f = this._transpose(800 + i * 400);
            osc.frequency.setValueAtTime(f, coinTime);
            osc.frequency.exponentialRampToValueAtTime(f * 2, coinTime + 0.12);
            const g = this.ctx.createGain();
            g.gain.setValueAtTime(0.03, coinTime);
            g.gain.exponentialRampToValueAtTime(0.001, coinTime + 0.15);
            osc.connect(g);
            g.connect(this.masterGain);
            osc.start(coinTime);
            osc.stop(coinTime + 0.18);
            this.nodes.push(osc, g);
        }
    }

    // ==================== Pattern: Delta Blues ====================
    _patternBlues(t, beatDuration, beats) {
        const bd = beatDuration;

        // 12-bar blues walking bass (simplified 4-bar loop)
        const bassNotes = [
            { f: 65.41, d: 2 }, { f: 73.42, d: 1 }, { f: 65.41, d: 1 },  // C – D – C
            { f: 77.78, d: 2 }, { f: 73.42, d: 1 }, { f: 65.41, d: 1 },  // E – D – C
            { f: 55.00, d: 2 }, { f: 65.41, d: 1 }, { f: 73.42, d: 1 },  // A – C – D
            { f: 65.41, d: 2 }, { f: 77.78, d: 1 }, { f: 82.41, d: 1 },  // C – E – F
        ];
        let bp = 0;
        for (const n of bassNotes) {
            this.uprightBass(n.f, t + bp * bd, n.d * bd * 0.9, 0.18);
            bp += n.d;
        }

        // Slide guitar lead — using brassSynth with heavy pitch bends
        const slideMelody = [
            { f: 392.00, sb: 0.5, d: 0.8 }, { f: 440.00, sb: 1.5, d: 0.6 },
            { f: 349.23, sb: 2.5, d: 1.0 }, { f: 392.00, sb: 4.0, d: 0.6 },
            { f: 440.00, sb: 5.0, d: 0.8 }, { f: 523.25, sb: 6.0, d: 0.6 },
            { f: 440.00, sb: 7.0, d: 1.0 },
            { f: 293.66, sb: 8.5, d: 0.8 }, { f: 349.23, sb: 9.5, d: 0.6 },
            { f: 392.00, sb: 10.5, d: 1.2 }, { f: 440.00, sb: 12.0, d: 0.6 },
            { f: 523.25, sb: 13.0, d: 0.8 }, { f: 349.23, sb: 14.5, d: 1.2 },
        ];
        for (const n of slideMelody) {
            this.brassSynth(n.f, t + n.sb * bd, n.d * bd * 0.85, 0.14);
        }

        // Finger-picked guitar chords on the off-beats
        for (let beat = 0; beat < beats; beat++) {
            if (beat % 2 === 1) {
                const chordRoot = [261.63, 329.63, 220.00, 261.63][beat % 4];
                this.pluckedString(chordRoot * 2, t + beat * bd + bd * 0.3, 0.08);
            }
        }

        // Drums — simple blues shuffle
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            // Kick on 1, 3
            if (beat % 2 === 0) this.kick(bt, 0.28);
            // Snare on 2, 4 with slight shuffle
            if (beat % 2 === 1) this.snare(bt + bd * 0.06, 0.16);
            // Brush on 2, 4
            if (beat % 2 === 1) this.brush(bt, 0.025);
            // Hi-hat — shuffle feel
            this.hihat(bt, 0.04);
            if (beat % 2 === 0) this.hihat(bt + bd * 0.5 + bd * 0.06, 0.03);
        }

        // Occasional slide "cry"
        for (let i = 0; i < 2; i++) {
            const cryTime = t + 3 + i * 7;
            if (!this.ctx || !this.isPlaying) return;
            const osc = this.ctx.createOscillator();
            osc.type = 'sawtooth';
            osc.frequency.setValueAtTime(300, cryTime);
            osc.frequency.linearRampToValueAtTime(450, cryTime + 0.08);
            osc.frequency.linearRampToValueAtTime(280, cryTime + 0.2);
            const g = this.ctx.createGain();
            g.gain.setValueAtTime(0.06, cryTime);
            g.gain.linearRampToValueAtTime(0.09, cryTime + 0.05);
            g.gain.exponentialRampToValueAtTime(0.001, cryTime + 0.35);
            const f = this.ctx.createBiquadFilter();
            f.type = 'lowpass';
            f.frequency.value = 2000;
            osc.connect(f);
            f.connect(g);
            g.connect(this.masterGain);
            osc.start(cryTime);
            osc.stop(cryTime + 0.4);
            this.nodes.push(osc, g, f);
        }
    }

    // ==================== Pattern: Lo-Fi Beats ====================
    _patternLofi(t, beatDuration, beats) {
        const bd = beatDuration;

        // Ambient noise pad (lo-fi hiss)
        this.noisePad(t, beats * bd, 0.04, 100, 4000);

        // Simple bass — downtempo, filtered
        const bassNotes = [
            { f: 65.41, d: 2 }, { f: 77.78, d: 2 },
            { f: 73.42, d: 2 }, { f: 65.41, d: 2 },
            { f: 55.00, d: 2 }, { f: 65.41, d: 2 },
            { f: 82.41, d: 2 }, { f: 73.42, d: 2 },
        ];
        let bp = 0;
        for (const n of bassNotes) {
            this.bass808(n.f, t + bp * bd, n.d * bd * 0.9, 0.22);
            bp += n.d;
        }

        // Rhodes chords — warm, slightly muffled
        const lofiChords = [261.63, 311.13, 349.23, 293.66, 261.63, 349.23, 329.63, 311.13];
        for (let i = 0; i < 8; i++) {
            this.rhodes(lofiChords[i], t + i * 2 * bd, bd * 1.9, 0.08);
        }

        // Simple melody — mellow, filtered
        const melody = [
            { f: 523.25, sb: 1.0, d: 1.0 }, { f: 587.33, sb: 2.5, d: 0.8 },
            { f: 659.25, sb: 3.5, d: 1.2 }, { f: 523.25, sb: 5.0, d: 1.0 },
            { f: 440.00, sb: 6.5, d: 1.2 }, { f: 392.00, sb: 8.0, d: 1.0 },
            { f: 523.25, sb: 9.5, d: 0.8 }, { f: 587.33, sb: 10.5, d: 1.2 },
            { f: 659.25, sb: 12.0, d: 1.0 }, { f: 783.99, sb: 13.5, d: 1.0 },
            { f: 523.25, sb: 15.0, d: 1.5 },
        ];
        for (const n of melody) {
            // Use muted lead synth (low filter) for lo-fi feel
            if (!this.ctx || !this.isPlaying) return;
            const f = this._transpose(n.f);
            const osc = this.ctx.createOscillator();
            osc.type = 'sine';
            osc.frequency.setValueAtTime(f, t + n.sb * bd);
            const filter = this.ctx.createBiquadFilter();
            filter.type = 'lowpass';
            filter.frequency.value = 1200;
            filter.Q.value = 1;
            const gain = this.ctx.createGain();
            gain.gain.setValueAtTime(0, t + n.sb * bd);
            gain.gain.linearRampToValueAtTime(0.12, t + n.sb * bd + 0.03);
            gain.gain.setValueAtTime(0.12, t + n.sb * bd + n.d * bd * 0.5);
            gain.gain.exponentialRampToValueAtTime(0.001, t + n.sb * bd + n.d * bd);
            osc.connect(filter);
            filter.connect(gain);
            gain.connect(this.masterGain);
            osc.start(t + n.sb * bd);
            osc.stop(t + n.sb * bd + n.d * bd + 0.05);
            this.nodes.push(osc, filter, gain);
        }

        // Drums — low-fi, muffled, with tape wobble feel
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            if (beat % 2 === 0) this.kick(bt, 0.25);
            if (beat % 2 === 1) this.snare(bt, 0.14);
            // Muffled hi-hat
            this.hihat(bt, 0.04);
            this.hihat(bt + bd * 0.5, 0.025);
            // Extra snare taps for swing
            if (beat % 8 === 3 || beat % 8 === 7) {
                this.snare(bt + bd * 0.75, 0.08);
            }
        }

        // Occasional "vinyl crackle" pops
        for (let i = 0; i < 4; i++) {
            const popTime = t + Math.random() * beats * bd;
            if (!this.ctx || !this.isPlaying) return;
            const osc = this.ctx.createOscillator();
            osc.type = 'sine';
            osc.frequency.setValueAtTime(100 + Math.random() * 200, popTime);
            const g = this.ctx.createGain();
            g.gain.setValueAtTime(0.015, popTime);
            g.gain.exponentialRampToValueAtTime(0.001, popTime + 0.03);
            osc.connect(g);
            g.connect(this.masterGain);
            osc.start(popTime);
            osc.stop(popTime + 0.04);
            this.nodes.push(osc, g);
        }
    }

    // ==================== Pattern: House Music ====================
    _patternHouse(t, beatDuration, beats) {
        const bd = beatDuration;

        // Four-on-the-floor kick
        for (let beat = 0; beat < beats; beat++) {
            this.kick(t + beat * bd, 0.38);
        }

        // Bassline — funky, syncopated
        const bassNotes = [
            { f: 65.41, d: 1.5 }, { f: 65.41, d: 0.5 },
            { f: 73.42, d: 1.0 }, { f: 65.41, d: 1.0 },
            { f: 55.00, d: 1.5 }, { f: 55.00, d: 0.5 },
            { f: 65.41, d: 1.0 }, { f: 77.78, d: 1.0 },
            { f: 65.41, d: 1.5 }, { f: 65.41, d: 0.5 },
            { f: 82.41, d: 1.0 }, { f: 73.42, d: 1.0 },
            { f: 55.00, d: 1.5 }, { f: 65.41, d: 0.5 },
            { f: 77.78, d: 1.0 }, { f: 82.41, d: 1.0 },
        ];
        let bp = 0;
        for (const n of bassNotes) {
            this.bass808(n.f, t + bp * bd, n.d * bd * 0.85, 0.3);
            bp += n.d;
        }

        // Synth pad — filtered, evolving
        const padRoots = [261.63, 311.13, 349.23, 293.66];
        for (let i = 0; i < 4; i++) {
            this.rhodes(padRoots[i], t + i * 4 * bd, bd * 3.9, 0.12);
        }

        // Synth stabs on the 2 and 4
        for (let beat = 1; beat < beats; beat += 2) {
            const root = [261.63, 329.63, 392.00, 349.23][(beat / 2) % 4];
            this.synthStab(root * 2, t + beat * bd, 0.1);
        }

        // Clap on 2 and 4
        for (let beat = 1; beat < beats; beat += 2) {
            this.clap(t + beat * bd, 0.18);
        }

        // Hi-hat — eighth notes with open hat accents
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            this.hihat(bt, 0.06);
            this.hihat(bt + bd * 0.5, 0.04);
            if (beat % 4 === 1) this.openHat(bt + bd * 0.5, 0.035);
            if (beat % 4 === 3) this.openHat(bt, 0.03);
        }

        // Rising sweep every 4 bars
        for (let i = 0; i < 4; i++) {
            const sweepTime = t + i * 4 * bd + bd * 3.5;
            if (!this.ctx || !this.isPlaying) return;
            const osc = this.ctx.createOscillator();
            osc.type = 'sawtooth';
            osc.frequency.setValueAtTime(200, sweepTime);
            osc.frequency.exponentialRampToValueAtTime(4000, sweepTime + bd * 0.5);
            const sweepFilter = this.ctx.createBiquadFilter();
            sweepFilter.type = 'lowpass';
            sweepFilter.frequency.setValueAtTime(300, sweepTime);
            sweepFilter.frequency.exponentialRampToValueAtTime(8000, sweepTime + bd * 0.5);
            const g = this.ctx.createGain();
            g.gain.setValueAtTime(0, sweepTime);
            g.gain.linearRampToValueAtTime(0.1, sweepTime + bd * 0.2);
            g.gain.exponentialRampToValueAtTime(0.001, sweepTime + bd * 0.6);
            osc.connect(sweepFilter);
            sweepFilter.connect(g);
            g.connect(this.masterGain);
            osc.start(sweepTime);
            osc.stop(sweepTime + bd * 0.7);
            this.nodes.push(osc, sweepFilter, g);
        }
    }

    // ==================== Pattern: Cinematic Western ====================
    _patternWestern(t, beatDuration, beats) {
        const bd = beatDuration;

        // Sparse bass — sustained, ominous
        const bassNotes = [
            { f: 65.41, d: 4 }, { f: 73.42, d: 4 },
            { f: 55.00, d: 4 }, { f: 65.41, d: 4 },
        ];
        let bp = 0;
        for (const n of bassNotes) {
            this.bass808(n.f, t + bp * bd, n.d * bd * 0.95, 0.25);
            bp += n.d;
        }

        // Whistle melody — Ennio Morricone style
        const whistleMelody = [
            { f: 783.99, sb: 0.5, d: 1.2 }, { f: 880.00, sb: 2.0, d: 0.8 },
            { f: 1046.5, sb: 3.0, d: 1.5 }, { f: 783.99, sb: 5.0, d: 1.0 },
            { f: 659.25, sb: 6.5, d: 1.5 }, { f: 880.00, sb: 8.5, d: 1.2 },
            { f: 783.99, sb: 10.0, d: 0.8 }, { f: 1046.5, sb: 11.0, d: 1.0 },
            { f: 1174.7, sb: 12.5, d: 1.5 }, { f: 1046.5, sb: 14.5, d: 1.5 },
        ];
        for (const n of whistleMelody) {
            this.whistle(n.f, t + n.sb * bd, n.d * bd * 0.9, 0.14);
        }

        // Acoustic guitar plucks — fingerpicking feel
        for (let beat = 0; beat < beats; beat += 2) {
            const fRoot = [261.63, 329.63, 220.00, 261.63][(beat / 2) % 4];
            this.pluckedString(fRoot, t + beat * bd + bd * 0.1, 0.09);
            this.pluckedString(fRoot * 1.5, t + beat * bd + bd * 0.5, 0.06);
        }

        // Sparse percussion — toms and rimshots
        for (let beat = 0; beat < beats; beat++) {
            const bt = t + beat * bd;
            // Light kick on 1, 3
            if (beat % 4 === 0) this.kick(bt, 0.2);
            if (beat % 4 === 2) this.kick(bt + bd * 0.1, 0.15);
            // Rimshot-style snare on 2, 4 (but lighter)
            if (beat % 2 === 1) this.snare(bt, 0.1);
            // Sparse brush
            if (beat % 4 === 3) this.brush(bt + bd * 0.5, 0.02);
            // Rattlesnake / shaker — very sparse
            if (beat % 8 === 1) this.hihat(bt + bd * 0.3, 0.015);
        }

        // Distant "coyote howl" effect on last bar
        const howlStart = t + 14 * bd;
        if (!this.ctx || !this.isPlaying) return;
        const osc = this.ctx.createOscillator();
        osc.type = 'sine';
        osc.frequency.setValueAtTime(400, howlStart);
        osc.frequency.linearRampToValueAtTime(500, howlStart + 0.3);
        osc.frequency.linearRampToValueAtTime(350, howlStart + 0.8);
        osc.frequency.linearRampToValueAtTime(450, howlStart + 1.2);
        const gain = this.ctx.createGain();
        gain.gain.setValueAtTime(0, howlStart);
        gain.gain.linearRampToValueAtTime(0.06, howlStart + 0.15);
        gain.gain.setValueAtTime(0.06, howlStart + 0.6);
        gain.gain.exponentialRampToValueAtTime(0.001, howlStart + 1.5);
        const filt = this.ctx.createBiquadFilter();
        filt.type = 'bandpass';
        filt.frequency.value = 600;
        filt.Q.value = 3;
        osc.connect(filt);
        filt.connect(gain);
        gain.connect(this.masterGain);
        osc.start(howlStart);
        osc.stop(howlStart + 1.6);
        this.nodes.push(osc, gain, filt);

        // Occasional "coin" / bell
        for (let i = 0; i < 3; i++) {
            const bellTime = t + 3 + i * 5;
            this.chipShuffle(bellTime);
        }
    }

    // ==================== Main Loop ====================
    scheduleLoop() {
        if (!this.ctx || !this.isPlaying) return;

        const now = this.ctx.currentTime;
        const t = now + 0.1;
        const bd = this.beatDuration;
        const beats = 16;
        const loopLength = beats * bd;

        // Pick a card-room arrangement by name so reordering tracks stays safe.
        switch (this.patterns[this.currentPattern]?.renderer) {
            case 'jazz': this._patternJazz(t, bd, beats); break;
            case 'swing': this._patternSwing(t, bd, beats); break;
            case 'blues': this._patternBlues(t, bd, beats); break;
            case 'lofi': this._patternLofi(t, bd, beats); break;
            case 'western': this._patternWestern(t, bd, beats); break;
            case 'funk': this._patternFunk(t, bd, beats); break;
            default: this._patternJazz(t, bd, beats);
        }

        // Schedule next loop
        this.loopTimer = setTimeout(() => {
            this.cleanupNodes();
            this.scheduleLoop();
        }, loopLength * 1000 - 100);
    }
}

const bgMusic = new BackgroundMusic();
