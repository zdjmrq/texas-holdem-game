/**
 * Declarative UI event routing. Keeping executable code out of the markup lets
 * the renderer use a strict script Content Security Policy.
 */
(function bindPokerControls() {
    'use strict';

    const actions = {
        'toggle-music': () => toggleMusic(),
        'toggle-turn-alert': () => toggleTurnAlert(),
        'skip-song': () => skipSong(),
        'cycle-playback-mode': () => cycleMusicPlaybackMode(),
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
        const target = event.target;
        const key = event.key.toLowerCase();
        if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;

        if (target.closest?.('[data-ui-key="human-cards"]')) {
            handleHumanCardsKey(event);
            return;
        }
        if (target.closest?.('#probToggle') && (key === 'enter' || key === ' ')) {
            event.preventDefault();
            toggleProbPanel();
            return;
        }

        // The raise panel owns these keys even while the range input has focus.
        if (raiseSliderVisible) {
            if (key === 'arrowleft' || key === 'arrowright') {
                event.preventDefault();
                adjustRaiseAmount(key === 'arrowright' ? 1 : -1);
            } else if (key === 'escape') {
                event.preventDefault();
                hideRaiseSlider();
            } else if (key === ' ' || key === 'enter') {
                // Native buttons retain their own Enter/Space action, including Cancel.
                if (target.closest?.('button')) return;
                if (event.repeat) return;
                event.preventDefault();
                confirmRaise();
            }
            return;
        }

        const modal = document.getElementById('showdownModal');
        if (modal.classList.contains('show')) {
            const next = modal.querySelector('.modal-btn');
            if (key === 'tab' && next) {
                event.preventDefault();
                next.focus();
                return;
            }
            if ((key === 'enter' || key === ' ' || key === 'n') && next && !next.disabled && !target.closest?.('button')) {
                if (event.repeat) return;
                event.preventDefault();
                closeModalAndContinue();
            }
            return;
        }

        if (key === 'escape' && (document.fullscreenElement || document.webkitFullscreenElement)) {
            event.preventDefault();
            toggleFullscreen();
            return;
        }

        // Typing and native controls keep their usual keyboard behavior.
        if (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName || '') ||
            (target.closest?.('button') && (key === 'enter' || key === ' '))) return;
        const isOnlineTurn = playMode === 'online' && network.gameState?.isYourTurn && !network.pendingAction;
        const isLocalTurn = playMode === 'local' && game?.isPlayerTurn();
        if (!isOnlineTurn && !isLocalTurn) return;
        const buttons = {
            f: ['btnFold', 'fold'],
            c: document.getElementById('btnCheck').disabled ? ['btnCall', 'call'] : ['btnCheck', 'check'],
            a: ['btnAllin', 'allin']
        };
        if (key === 'r' && !document.getElementById('btnRaise').disabled) {
            event.preventDefault();
            showRaiseSlider();
        } else if (buttons[key] && !document.getElementById(buttons[key][0]).disabled) {
            if (event.repeat) return;
            event.preventDefault();
            doAction(buttons[key][1]);
        }
    });
})();
