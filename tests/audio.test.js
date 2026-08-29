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
    assert.match(html, /rel="preload" href="assets\/music\/01\.mp3" as="audio"/);
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
