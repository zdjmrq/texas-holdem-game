'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '..', 'js', 'audio.js'), 'utf8');
const context = {
    console,
    document:{ dispatchEvent() {}, baseURI:'file:///app/index.html' },
    CustomEvent:class {},
    Math:Object.create(Math)
};
vm.runInNewContext(`${source}\nthis.BackgroundMusicForTest = BackgroundMusic;`, context);
const BackgroundMusic = context.BackgroundMusicForTest;

test('replacement soundtrack is ordered and named 1 through 4', () => {
    const music = new BackgroundMusic();
    assert.deepEqual(
        Array.from(music.patterns, track => ({ name:track.name, src:track.src })),
        [
            { name:'1', src:'assets/music/01.mp3' },
            { name:'2', src:'assets/music/02.mp3' },
            { name:'3', src:'assets/music/03.mp3' },
            { name:'4', src:'assets/music/04.mp3' }
        ]
    );
    for (const track of music.patterns) {
        assert.equal(fs.existsSync(path.join(__dirname, '..', track.src)), true, `${track.src} should exist`);
    }
});

test('music controls default to off, volume 50 and sequential playback', () => {
    const html = fs.readFileSync(path.join(__dirname, '..', 'index.html'), 'utf8');
    assert.match(html, /id="musicBtn"[^>]*>🎵 音乐关</);
    assert.match(html, /id="playbackModeBtn"[^>]*[\s\S]*?>🔁 顺序</);
    assert.match(html, /id="volSlider"[^>]*[\s\S]*?value="50"/);
    assert.doesNotMatch(html, /rel="preload"[^>]*as="audio"/);
});

test('playback modes choose sequential, repeated and non-repeating random tracks', () => {
    const music = new BackgroundMusic();
    music.currentPattern = 1;
    assert.equal(music._pickNextPattern(), 2);

    music.setPlaybackMode('repeat-one');
    assert.equal(music._pickNextPattern(), 1);
    assert.equal(music._pickNextPattern(true), 2);

    context.Math.random = () => 0.9;
    music.setPlaybackMode('shuffle');
    assert.equal(music._pickNextPattern(), 3);
});

test('a delayed manual skip keeps the old song audible until the next track can play', async () => {
    const frames = [];
    let now = 0, ready;
    context.performance = {now:()=>now};
    context.requestAnimationFrame = fn => {frames.push(fn); return frames.length;};
    context.cancelAnimationFrame = () => {};
    const music = new BackgroundMusic();
    const old = {volume:.216, ended:false, paused:false, pause(){this.paused=true;}};
    const next = {volume:0, play:()=>new Promise(resolve=>{ready=resolve;})};
    music.players = [old, next];
    music.initialized = true;
    music.isPlaying = true;
    music._load = () => {};
    music._preloadFollowing = () => {};
    const transition = music._transitionTo(1, 1200, true);
    assert.equal(frames.length, 0, 'the fade must not start while the next song is still loading');
    assert.equal(old.volume, .216);
    assert.equal(old.paused, false);
    now = 2000;
    ready();
    await transition;
    frames.shift()(2600);
    assert.ok(old.volume > 0 && next.volume > 0);
    frames.shift()(3200);
    assert.equal(old.paused, true);
    assert.equal(next.volume, music.volume);
});

test('stopping music while a skip is loading cannot restart the stale fade', async () => {
    let ready;
    const frames=[];
    context.requestAnimationFrame=fn=>{frames.push(fn);return frames.length;};
    const music=new BackgroundMusic();
    music.initialized=true;
    music.isPlaying=true;
    music.players=[{volume:.2,ended:false,paused:false,pause(){}},{volume:0,play:()=>new Promise(resolve=>{ready=resolve;})}];
    music._load=()=>{};
    const transition=music._transitionTo(1);
    music.stop();
    const stopFrames=frames.length;
    ready();
    await transition;
    assert.equal(frames.length,stopFrames);
    assert.equal(music.isPlaying,false);
});
