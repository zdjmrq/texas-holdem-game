/**
 * Audio System — Background music
 */
let audioCtx = null;
let musicNodes = [];
let isMusicPlaying = false;
let currentTrack = 0;
let masterGain = null;

const BPM = 90;
const BEAT = 60 / BPM;

const melodies = [
    { name: 'Jazzy Blues', notes: [
        { freq: 262, dur: 0.5 }, { freq: 294, dur: 0.5 }, { freq: 330, dur: 0.5 }, { freq: 349, dur: 0.5 },
        { freq: 330, dur: 0.5 }, { freq: 294, dur: 0.5 }, { freq: 262, dur: 1.0 }, { freq: 0, dur: 0.5 },
        { freq: 349, dur: 0.5 }, { freq: 392, dur: 0.5 }, { freq: 440, dur: 0.5 }, { freq: 392, dur: 0.5 },
        { freq: 349, dur: 0.5 }, { freq: 330, dur: 0.5 }, { freq: 294, dur: 1.0 }, { freq: 0, dur: 0.5 },
    ]},
    { name: 'Smooth Groove', notes: [
        { freq: 196, dur: 0.5 }, { freq: 220, dur: 0.25 }, { freq: 196, dur: 0.25 },
        { freq: 262, dur: 0.5 }, { freq: 220, dur: 0.5 }, { freq: 196, dur: 0.5 },
        { freq: 175, dur: 0.5 }, { freq: 165, dur: 0.5 }, { freq: 196, dur: 1.0 },
    ]},
];

function initAudio() {
    if (audioCtx) return;
    try {
        audioCtx = new (window.AudioContext || window.webkitAudioContext)();
        masterGain = audioCtx.createGain();
        masterGain.gain.value = 0.3;
        masterGain.connect(audioCtx.destination);
    } catch (e) { console.warn('Audio not available:', e); }
}

function toggleMusic() {
    if (!audioCtx) initAudio();
    if (!audioCtx) return;
    isMusicPlaying ? stopMusic() : startMusic();
}

function startMusic() {
    if (!audioCtx || isMusicPlaying) return;
    isMusicPlaying = true;
    document.getElementById('musicBtn').textContent = '🎵 音乐开';
    document.getElementById('musicBtn').classList.add('music-on');
    playTrack(currentTrack);
}

function stopMusic() {
    isMusicPlaying = false;
    document.getElementById('musicBtn').textContent = '🎵 音乐关';
    document.getElementById('musicBtn').classList.remove('music-on');
    for (const n of musicNodes) { try { n.stop(); } catch(e) {} }
    musicNodes = [];
}

function skipSong() {
    if (!isMusicPlaying) { startMusic(); return; }
    for (const n of musicNodes) { try { n.stop(); } catch(e) {} }
    musicNodes = [];
    currentTrack = (currentTrack + 1) % melodies.length;
    playTrack(currentTrack);
}

function playTrack(trackIdx) {
    if (!audioCtx || !isMusicPlaying) return;
    const melody = melodies[trackIdx];
    let time = audioCtx.currentTime + 0.1;
    musicNodes = [];
    for (const note of melody.notes) {
        if (note.freq > 0) {
            const osc = audioCtx.createOscillator();
            const gain = audioCtx.createGain();
            osc.type = 'sine';
            osc.frequency.value = note.freq;
            gain.gain.setValueAtTime(0.3, time);
            gain.gain.exponentialRampToValueAtTime(0.01, time + note.dur * BEAT);
            osc.connect(gain);
            gain.connect(masterGain);
            osc.start(time);
            osc.stop(time + note.dur * BEAT);
            musicNodes.push(osc);
        }
        time += note.dur * BEAT;
    }
    setTimeout(() => {
        if (isMusicPlaying) {
            currentTrack = (currentTrack + 1) % melodies.length;
            playTrack(currentTrack);
        }
    }, (time - audioCtx.currentTime) * 1000 + 500);
}

function setVolume(val) {
    if (masterGain) masterGain.gain.value = val / 100;
}