/**
 * Declarative UI event routing. Keeping executable code out of the markup lets
 * the renderer use a strict script Content Security Policy.
 */
(function bindPokerControls() {
    'use strict';

    const actions = {
        'toggle-music': () => toggleMusic(),
        'skip-song': () => skipSong(),
        refresh: () => doRefresh(),
        home: () => goHome(),
        fullscreen: () => toggleFullscreen(),
        'window-minimize': () => minimizeAppWindow(),
        'window-maximize': () => toggleAppWindowMaximize(),
        'window-close': () => closeAppWindow(),
        'start-local-game': () => startNewGame(),
        connect: () => doConnect(),
        'join-room': () => doJoinRoom(),
        'start-online-game': () => doStartGame(),
        'leave-room': () => doLeaveRoom(),
        disconnect: () => doDisconnect(),
        'toggle-human-cards': () => toggleHumanCardsVisibility(),
        'show-raise': () => showRaiseSlider(),
        'confirm-raise': () => confirmRaise(),
        'hide-raise': () => hideRaiseSlider(),
        'toggle-probability': () => toggleProbPanel(),
        'next-hand': () => closeModalAndContinue(),
        'toggle-rules': () => toggleRules()
    };

    document.addEventListener('click', event => {
        const control = event.target.closest('[data-ui-action]');
        if (!control || control.disabled) return;
        const action = control.dataset.uiAction;
        if (action === 'select-game-mode') return selectGameMode(control.dataset.mode);
        if (action === 'select-play-mode') return selectPlayMode(control.dataset.play);
        if (action === 'poker-action') return doAction(control.dataset.pokerAction);
        const handler = actions[action];
        if (handler) handler();
    });

    document.addEventListener('input', event => {
        const control = event.target.closest('[data-ui-input]');
        if (!control) return;
        if (control.dataset.uiInput === 'volume') setVolume(control.value);
        else if (control.dataset.uiInput === 'setting') updateGameSetting(control.dataset.setting, control.value);
        else if (control.dataset.uiInput === 'raise') updateRaiseDisplay();
    });

    document.addEventListener('keydown', event => {
        if (event.target.closest('[data-ui-key="human-cards"]')) handleHumanCardsKey(event);
    });
})();
