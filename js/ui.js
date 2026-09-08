/**
 * UI Layer — Rendering, event binding, app initialization
 * Loaded after all game engine scripts.
 */
// ============================================================
// Game Initialization & UI Binding
// ============================================================

let game = null;
let gameMode = 'standard'; // 'standard' | 'shortdeck'
let gameStarted = false;
let playMode = 'local'; // 'local' | 'online'
let isRoomHost = false;
let inRoom = false;

const STARTING_STACK_LEVELS = [5000, 10000, 20000, 50000, 100000];
const BLIND_LEVELS = [
    { small: 10, big: 20 },
    { small: 20, big: 40 },
    { small: 40, big: 80 },
    { small: 50, big: 100 },
    { small: 100, big: 200 },
    { small: 200, big: 400 }
];

let selectedStartingStack = 20000;
let selectedSmallBlind = 40;
let selectedBigBlind = 80;
let humanCardsHidden = false;
let humanCardsAnimating = false;
let humanDealAnimationToken = 0;
let lastOnlineHoleCardsKey = '';
let lastOnlineHandMarker = null;
let lastShownOnlineResultHandId = null;
let onlineProbabilityResult = null;
let onlineOutsResult = null;
let onlineProbabilityKey = '';

const APP_SETTINGS_KEY = 'texas-holdem-settings-v1';
const MUSIC_SETTINGS_VERSION = 2;
const AI_SPEED_LEVELS = [
    { label: '快速', delay: 800 },
    { label: '标准', delay: 1800 },
    { label: '沉浸', delay: 3200 }
];
let selectedAiDelay = AI_SPEED_LEVELS[1].delay;
let musicPreference = 'off';
let selectedMusicVolume = 50;
let musicPlaybackMode = 'sequential';

function loadAppSettings() {
    try {
        const saved = JSON.parse(localStorage.getItem(APP_SETTINGS_KEY) || 'null');
        if (!saved || typeof saved !== 'object') return;
        if (STARTING_STACK_LEVELS.includes(saved.startingStack)) selectedStartingStack = saved.startingStack;
        const blindLevel = BLIND_LEVELS.find(level =>
            level.small === saved.smallBlind && level.big === saved.bigBlind);
        if (blindLevel) {
            selectedSmallBlind = blindLevel.small;
            selectedBigBlind = blindLevel.big;
        }
        const speed = AI_SPEED_LEVELS.find(level => level.delay === saved.aiDelay);
        if (speed) selectedAiDelay = speed.delay;
        if (saved.gameMode === 'standard' || saved.gameMode === 'shortdeck') gameMode = saved.gameMode;
        // Version 2 intentionally resets the replaced soundtrack to the new
        // defaults once: off, 50 volume and sequential playback.
        if (saved.musicSettingsVersion === MUSIC_SETTINGS_VERSION) {
            if (['on', 'off'].includes(saved.musicPreference)) musicPreference = saved.musicPreference;
            if (Number.isFinite(saved.musicVolume)) selectedMusicVolume = Math.max(0, Math.min(100, saved.musicVolume));
            if (['sequential', 'repeat-one', 'shuffle'].includes(saved.musicPlaybackMode)) {
                musicPlaybackMode = saved.musicPlaybackMode;
            }
        }
    } catch (_) {}
}

function saveAppSettings() {
    try {
        const serverInput = document.getElementById('serverAddress');
        const nameInput = document.getElementById('playerNameInput');
        localStorage.setItem(APP_SETTINGS_KEY, JSON.stringify({
            startingStack: selectedStartingStack,
            smallBlind: selectedSmallBlind,
            bigBlind: selectedBigBlind,
            aiDelay: selectedAiDelay,
            gameMode,
            musicPreference,
            musicVolume: selectedMusicVolume,
            musicPlaybackMode,
            musicSettingsVersion: MUSIC_SETTINGS_VERSION,
            serverUrl: serverInput ? serverInput.value.trim() : undefined,
            playerName: nameInput ? nameInput.value.trim().slice(0, 8) : undefined
        }));
    } catch (_) {}
}

function restoreSavedInputs() {
    try {
        const saved = JSON.parse(localStorage.getItem(APP_SETTINGS_KEY) || 'null') || {};
        const serverInput = document.getElementById('serverAddress');
        const nameInput = document.getElementById('playerNameInput');
        if (serverInput && typeof saved.serverUrl === 'string' && /^wss?:\/\//i.test(saved.serverUrl)) {
            serverInput.value = saved.serverUrl;
        }
        if (nameInput && typeof saved.playerName === 'string') nameInput.value = saved.playerName.slice(0, 8);
    } catch (_) {}
}

async function syncLocalServerEndpoint() {
    if (navigator.userAgent.includes('TexasHoldemAndroid/')) {
        const hint = document.getElementById('localServerHint');
        if (hint) hint.textContent = '安卓端不启动服务器；请输入电脑或服务器的 WebSocket 地址，不能使用 localhost。';
        const serverInput = document.getElementById('serverAddress');
        if (serverInput && /^ws:\/\/(localhost|127\.0\.0\.1)(:|\/|$)/i.test(serverInput.value)) {
            serverInput.value = '';
            serverInput.placeholder = 'ws://电脑局域网IP:3000 或 wss://服务器域名';
        }
        return;
    }
    if (!window.windowControls?.getLocalServerUrl) return;
    try {
        const localUrl = await window.windowControls.getLocalServerUrl();
        if (!/^ws:\/\/127\.0\.0\.1:\d+$/.test(localUrl || '')) return;
        const serverInput = document.getElementById('serverAddress');
        if (serverInput) {
            let isLocalAddress = false;
            try {
                const parsed = new URL(serverInput.value);
                isLocalAddress = ['localhost','127.0.0.1'].includes(parsed.hostname);
            } catch (_) {}
            if (isLocalAddress) serverInput.value = localUrl;
        }
        const hint = document.getElementById('localServerHint');
        if (hint) hint.textContent = `本机服务会随应用自动启动 · 当前地址 ${localUrl}`;
    } catch (error) {
        console.warn('Could not read the local server address:', error);
    }
}

const communityAnimationState = {
    local: { keys: [], token: 0, dirty: true },
    online: { keys: [], token: 0, dirty: true }
};

function formatChips(value) {
    return '$' + Math.max(0, Math.floor(Number(value) || 0)).toLocaleString();
}

function updateGameSetting(type, rawIndex) {
    if (type === 'stack') {
        const index = Math.max(0, Math.min(STARTING_STACK_LEVELS.length - 1, parseInt(rawIndex) || 0));
        selectedStartingStack = STARTING_STACK_LEVELS[index];
    } else if (type === 'blinds') {
        const index = Math.max(0, Math.min(BLIND_LEVELS.length - 1, parseInt(rawIndex) || 0));
        selectedSmallBlind = BLIND_LEVELS[index].small;
        selectedBigBlind = BLIND_LEVELS[index].big;
    } else if (type === 'aiSpeed') {
        const index = Math.max(0, Math.min(AI_SPEED_LEVELS.length - 1, parseInt(rawIndex) || 0));
        selectedAiDelay = AI_SPEED_LEVELS[index].delay;
    }
    syncGameSettingsUI();
    saveAppSettings();
}

function syncGameSettingsUI() {
    const stackIndex = Math.max(0, STARTING_STACK_LEVELS.indexOf(selectedStartingStack));
    const blindIndex = Math.max(0, BLIND_LEVELS.findIndex(level =>
        level.small === selectedSmallBlind && level.big === selectedBigBlind));
    const stackSlider = document.getElementById('stackSlider');
    const blindsSlider = document.getElementById('blindsSlider');
    const stackValue = document.getElementById('stackValue');
    const blindsValue = document.getElementById('blindsValue');
    const blindsLabel = document.getElementById('blindsSettingLabel');
    const summary = document.getElementById('settingSummary');
    const speedIndex = Math.max(0, AI_SPEED_LEVELS.findIndex(level => level.delay === selectedAiDelay));
    const speedSlider = document.getElementById('aiSpeedSlider');
    const speedValue = document.getElementById('aiSpeedValue');

    if (stackSlider) {
        stackSlider.value = stackIndex;
        stackSlider.setAttribute('aria-valuetext', formatChips(selectedStartingStack));
    }
    if (blindsSlider) {
        blindsSlider.value = blindIndex;
        blindsSlider.setAttribute('aria-valuetext', gameMode === 'shortdeck'
            ? `前注 ${selectedSmallBlind}，最小下注 ${selectedBigBlind}`
            : `小盲 ${selectedSmallBlind}，大盲 ${selectedBigBlind}`);
    }
    if (stackValue) stackValue.textContent = formatChips(selectedStartingStack);
    if (blindsValue) blindsValue.textContent = `${formatChips(selectedSmallBlind)} / ${formatChips(selectedBigBlind)}`;
    if (speedSlider) {
        speedSlider.value = speedIndex;
        speedSlider.setAttribute('aria-valuetext', AI_SPEED_LEVELS[speedIndex].label);
    }
    if (speedValue) speedValue.textContent = AI_SPEED_LEVELS[speedIndex].label;
    if (blindsLabel) blindsLabel.textContent = gameMode === 'shortdeck' ? '前注 / 最小下注' : '大小盲注';
    if (summary) {
        const depth = Math.round((selectedStartingStack / selectedBigBlind) * 10) / 10;
        summary.textContent = gameMode === 'shortdeck'
            ? `${depth} 个最小下注的起始深度`
            : `${depth} BB 起始深度`;
    }

    const headerBlinds = document.querySelector('.blinds-info');
    if (headerBlinds) {
        headerBlinds.innerHTML = gameMode === 'shortdeck'
            ? `<span>Ante: <span id="sbDisplay">${selectedSmallBlind}</span></span><span>最小下注: <span id="bbDisplay">${selectedBigBlind}</span></span>`
            : `<span>SB: <span id="sbDisplay">${selectedSmallBlind}</span></span><span>BB: <span id="bbDisplay">${selectedBigBlind}</span></span>`;
    }

    const dynamicValues = {
        'small-blind': selectedSmallBlind,
        'big-blind': selectedBigBlind,
        'ante': selectedSmallBlind,
        'dealer-ante': selectedSmallBlind * 2,
        'min-bet': selectedBigBlind
    };
    document.querySelectorAll('[data-config-value]').forEach(function(el) {
        const value = dynamicValues[el.dataset.configValue];
        if (value !== undefined) el.textContent = formatChips(value);
    });
}

function applyRoomConfigFromServer(message) {
    const config = (message && (message.roomConfig || message.config)) || message || {};
    const stack = Number(config.startingStack);
    const small = Number(config.smallBlind || config.ante);
    const big = Number(config.bigBlind || config.minBet);
    const aiDelay = Number(config.aiDelay);
    if (typeof config.isShortDeck === 'boolean') gameMode = config.isShortDeck ? 'shortdeck' : 'standard';
    if (STARTING_STACK_LEVELS.includes(stack)) selectedStartingStack = stack;
    const blindLevel = BLIND_LEVELS.find(level => level.small === small && level.big === big);
    if (blindLevel) {
        selectedSmallBlind = blindLevel.small;
        selectedBigBlind = blindLevel.big;
    }
    if (AI_SPEED_LEVELS.some(level => level.delay === aiDelay)) selectedAiDelay = aiDelay;
    document.querySelectorAll('.mode-btn').forEach(button => {
        button.classList.toggle('selected', button.dataset.mode === gameMode);
    });
    const roomMode = document.getElementById('roomShortDeck');
    if (roomMode) roomMode.checked = gameMode === 'shortdeck';
    updatePlayerCountOptions();
    updateStartScreenMode();
    syncGameSettingsUI();
}

function resetOnlinePresentationState() {
    lastOnlineState = null;
    lastOnlineHoleCardsKey = '';
    lastOnlineHandMarker = null;
    lastShownOnlineResultHandId = null;
    onlineProbabilityResult = null;
    onlineOutsResult = null;
    onlineProbabilityKey = '';
    resetCommunityCardAnimation('online');
    resetHumanCardsVisibility();
}

/** Select game mode (standard/shortdeck) */
function selectGameMode(mode) {
    gameMode = mode;
    document.querySelectorAll('.mode-btn').forEach(b => {
        b.classList.toggle('selected', b.dataset.mode === mode);
    });
    updatePlayerCountOptions();
    updateStartScreenMode();
    const roomMode = document.getElementById('roomShortDeck');
    if (roomMode) roomMode.checked = mode === 'shortdeck';
    syncGameSettingsUI();
    saveAppSettings();
}

/** Select play mode (local/online) */
function selectPlayMode(mode) {
    if (mode === 'local' && playMode === 'online' && inRoom) {
        leaveRoom();
        inRoom = false;
        isRoomHost = false;
        resetOnlinePresentationState();
        document.getElementById('bottomPanel').style.display = 'none';
        document.getElementById('startScreen').style.display = 'flex';
        document.getElementById('communityArea').style.display = 'none';
        document.getElementById('potDisplay').style.display = 'none';
    }
    playMode = mode;
    document.querySelectorAll('.play-btn').forEach(b => {
        b.classList.toggle('selected', b.dataset.play === mode);
    });
    document.getElementById('localOptions').style.display = mode === 'local' ? 'block' : 'none';
    document.getElementById('onlinePanel').style.display = mode === 'online' ? 'block' : 'none';
}

// ── Online mode functions ──

function doConnect() {
    const url = document.getElementById('serverAddress').value.trim();
    const name = document.getElementById('playerNameInput').value.trim() || '玩家';
    if (!url) { showNetworkError('请输入服务器地址'); return; }
    saveAppSettings();

    // Setup network callbacks
    onConnected = info => {
        if (playMode !== 'online') return;
        document.getElementById('connectPanel').style.display = 'none';
        document.getElementById('roomLobby').style.display = 'block';
        const resuming = !!(info && info.reconnecting && network.resumeToken && network.roomCode);
        document.getElementById('createRoomForm').style.display = resuming ? 'none' : 'block';
        document.getElementById('joinRoomForm').style.display = resuming ? 'none' : 'block';
        document.getElementById('roomActions').style.display = resuming ? 'block' : 'none';
        showNetworkError(resuming ? '🔄 已重新连接，正在恢复原座位...' : '✅ 已连接到服务器');
    };

    onDisconnected = info => {
        if (playMode !== 'online') return;
        if (info && info.willReconnect) {
            const waitSeconds = Math.max(1, Math.ceil((Number(info.reconnectDelay) || 0) / 1000));
            ['btnFold','btnCheck','btnCall','btnRaise','btnAllin'].forEach(id => {
                const button = document.getElementById(id);
                if (button) button.disabled = true;
            });
            document.getElementById('actionExplain').textContent = `🔄 连接中断，约 ${waitSeconds} 秒后恢复原座位...`;
            showNetworkError(`🔄 连接中断，约 ${waitSeconds} 秒后自动重连...`);
            return;
        }
        resetOnlinePresentationState();
        document.getElementById('connectPanel').style.display = 'block';
        document.getElementById('roomLobby').style.display = 'none';
        // 回到主界面
        document.getElementById('bottomPanel').style.display = 'none';
        document.getElementById('startScreen').style.display = 'flex';
        document.getElementById('communityArea').style.display = 'none';
        document.getElementById('potDisplay').style.display = 'none';
        document.getElementById('aiPlayersContainer').innerHTML = '';
        document.getElementById('gameStatus').textContent = gameMode === 'shortdeck'
            ? '点击「开始游戏」开始短牌对局！'
            : '点击「开始游戏」开始德州扑克对局！';
        inRoom = false;
        showNetworkError('🔌 已断开连接');
    };

    onRoomCreated = onRoomJoined = (msg) => {
        if (playMode !== 'online') return;
        inRoom = true;
        isRoomHost = typeof msg.isHost === 'boolean' ? msg.isHost : !!network.isHost;
        applyRoomConfigFromServer(msg);
        document.getElementById('roomCodeDisplay').textContent = msg.roomCode;
        document.getElementById('createRoomForm').style.display = 'none';
        document.getElementById('joinRoomForm').style.display = 'none';
        document.getElementById('roomActions').style.display = 'block';
        document.getElementById('startGameBtnOnline').style.display = isRoomHost ? 'block' : 'none';
        document.getElementById('waitingMsg').style.display = isRoomHost ? 'none' : 'block';
        updatePlayerList(msg.players);
        showNetworkError('✅ 房间 ' + msg.roomCode + (isRoomHost ? ' | 房主' : ''));
    };

    onRoomPlayersChanged = (msg) => {
        if (playMode !== 'online') return;
        if (msg.players) updatePlayerList(msg.players);
    };

    onHostChanged = msg => {
        if (playMode !== 'online') return;
        isRoomHost = typeof msg.isHost === 'boolean' ? msg.isHost : network.isHost;
        document.getElementById('startGameBtnOnline').style.display = isRoomHost ? 'block' : 'none';
        document.getElementById('waitingMsg').style.display = isRoomHost ? 'none' : 'block';
        if (msg.players) updatePlayerList(msg.players);
        showNetworkError(isRoomHost ? '👑 房主已离开，你现在是房主' : '房主已变更');
    };

    onGameStarting = (msg) => {
        if (playMode !== 'online') return;
        resetHumanCardsVisibility();
        resetCommunityCardAnimation('online');
        applyRoomConfigFromServer(msg);
        // 隐藏大厅，切换为牌桌界面
        document.getElementById('roomLobby').style.display = 'none';
        const bottomPanel = document.getElementById('bottomPanel');
        const startScreen = document.getElementById('startScreen');
        bottomPanel.style.display = 'flex';
        startScreen.style.display = 'none';
        const communityArea = document.getElementById('communityArea');
        if (communityArea) communityArea.style.display = '';
        const potDisplay = document.getElementById('potDisplay');
        if (potDisplay) potDisplay.style.display = '';
        document.getElementById('potDisplay').textContent = '底池: $0';
        showNetworkError('🎮 游戏开始！' + msg.playerCount + '人 (' + msg.humanCount + '真人 + ' + msg.aiCount + 'AI)');
    };

    onGameState = (msg) => {
        if (playMode !== 'online') return;
        renderOnlineGame(msg);
    };

    onActionAck = () => {
        if (playMode === 'online' && network.gameState) updateOnlineActions(network.gameState);
    };

    onRequestError = () => {
        if (playMode === 'online' && network.gameState) updateOnlineActions(network.gameState);
    };

    onReadyStatus = (msg) => {
        const btn = document.querySelector('#showdownModal .modal-btn');
        if (!btn) return;
        // 检查自己是否已经点了"下一手"——没点的话不锁按钮
        const myStatus = (msg.players || []).find(function(p) { return p.id === network.playerId; });
        const iHaveClicked = myStatus ? myStatus.ready : false;

        if (msg.allReady) {
            document.getElementById('showdownModal').classList.remove('show');
            btn.textContent = '下一手 →';
            btn.disabled = false;
            btn.style.opacity = '';
            btn.style.cursor = '';
        } else if (iHaveClicked) {
            // 我点了但别人还没 → 显示等待
            const waiting = msg.waitingFor || 0;
            btn.textContent = '⏳ 等待中 (' + waiting + ' 人未准备)...';
            btn.disabled = true;
            btn.style.opacity = '0.6';
            btn.style.cursor = 'not-allowed';
        }
        // 我没点 → 保持"下一手 →"可点击状态，不动按钮
    };

    showNetworkError = (msg) => {
        const el = document.getElementById('gameStatus');
        if (el) el.textContent = String(msg || '');
    };

    connectToServer(url);
}

// ============================================================
// 联网游戏渲染（服务端状态 → UI）
// ============================================================

function getAnalysisContext() {
    if (playMode === 'online') {
        const state = network.gameState;
        if (!state) return null;
        const me = (state.players || []).find(player => player.id === network.playerId);
        const roomConfig = state.roomConfig || state.config || {};
        return {
            playerCards:state.yourCards || [],
            communityCards:state.communityCards || [],
            folded:!!me?.folded,
            phase:state.phase,
            isShortDeck:!!(state.isShortDeck || roomConfig.isShortDeck),
            probabilityResult:onlineProbabilityResult,
            outsResult:onlineOutsResult
        };
    }
    if (!game || !game.humanPlayer) return null;
    return {
        playerCards:game.humanPlayer.holeCards || [],
        communityCards:game.communityCards || [],
        folded:!!game.humanPlayer.folded,
        phase:game.phase,
        isShortDeck:game instanceof ShortDeckGame,
        probabilityResult:game.probabilityResult,
        outsResult:typeof game.getOuts === 'function' ? game.getOuts() : null
    };
}

function requestOnlineProbability(state) {
    const cards = state.yourCards || [];
    const communityCards = state.communityCards || [];
    const me = (state.players || []).find(player => player.id === network.playerId);
    if (cards.length !== 2 || me?.folded || state.phase === 'idle') {
        onlineProbabilityResult = null;
        onlineOutsResult = null;
        onlineProbabilityKey = '';
        return;
    }
    const numOpponents = Math.max(1, (state.players || []).filter(player =>
        player.id !== network.playerId && !player.folded && player.stack + (player.chipsInPot || 0) >= 0).length);
    const config = state.roomConfig || state.config || {};
    const options = {
        playerCards:cards,
        communityCards,
        numOpponents,
        isShortDeck:!!(state.isShortDeck || config.isShortDeck)
    };
    const key = probabilityService.makeKey(options);
    if (key === onlineProbabilityKey) return;
    onlineProbabilityKey = key;
    onlineProbabilityResult = null;
    onlineOutsResult = null;
    updateProbability();
    probabilityService.calculate(options).then(result => {
        if (playMode !== 'online' || key !== onlineProbabilityKey) return;
        onlineProbabilityResult = result;
        onlineOutsResult = result.outs || null;
        updateProbability();
        updateHandAnalysis();
        updateOuts();
    }).catch(error => {
        if (key === onlineProbabilityKey) console.error('Online probability error:', error);
    });
}

/** 主渲染入口：根据服务端 game_state 渲染整个牌桌 */
function renderOnlineGame(msg) {
    if (playMode !== 'online' || !msg || !msg.phase) return;
    if (network.roomCode && msg.roomCode && msg.roomCode !== network.roomCode) return;
    if (typeof bgMusic !== 'undefined') bgMusic.setGamePhase(msg.phase);

    applyRoomConfigFromServer(msg);
    updateOnlineBlindDisplay(msg);

    const marker = msg.handId ?? msg.handNumber ?? msg.handCount ?? msg.numHands ?? null;
    if (marker !== null) document.getElementById('handCount').textContent = String(marker);
    const holeKey = (msg.yourCards || []).map(card => `${card.rank}-${card.suit}`).join('|');
    const isNewMarker = marker !== null && marker !== lastOnlineHandMarker;
    const looksLikeNewHand = msg.phase === 'preflop' && (msg.communityCards || []).length === 0 &&
        (isNewMarker || (holeKey && lastOnlineHoleCardsKey && holeKey !== lastOnlineHoleCardsKey));
    const shouldAnimateHoleCards = isNewMarker || looksLikeNewHand;
    if (shouldAnimateHoleCards) {
        resetHumanCardsVisibility();
        resetCommunityCardAnimation('online');
    }
    if (marker !== null) lastOnlineHandMarker = marker;
    if (holeKey) lastOnlineHoleCardsKey = holeKey;

    // ── 新一局开始 → 关闭旧的摊牌弹窗 + 重置按钮 ──
    if (msg.phase !== 'idle') {
        document.getElementById('showdownModal').classList.remove('show');
        const btn = document.querySelector('#showdownModal .modal-btn');
        if (btn) {
            btn.textContent = '下一手 →';
            btn.disabled = false;
            btn.style.opacity = '';
            btn.style.cursor = '';
        }
    }

    // ── 摊牌/结束 → 显示结果弹窗 ──
    if (msg.lastHandResult) {
        if (lastShownOnlineResultHandId !== marker) {
            lastShownOnlineResultHandId = marker;
            showOnlineShowdown(msg.lastHandResult);
        }
        // 保持牌桌背景可见，不渲染新牌
        lastOnlineState = msg;
        return;
    }

    if (msg.phase === 'idle') return;

    // ── 正常渲染牌桌 ──
    const bottomPanel = document.getElementById('bottomPanel');
    const startScreen = document.getElementById('startScreen');
    bottomPanel.style.display = 'flex';
    startScreen.style.display = 'none';

    const communityArea = document.getElementById('communityArea');
    if (communityArea) communityArea.style.display = '';
    const potDisplay = document.getElementById('potDisplay');
    if (potDisplay) potDisplay.style.display = '';

    // 底池
    document.getElementById('potDisplay').textContent = '底池: $' + (msg.pot || 0).toLocaleString();

    // 公共牌
    renderOnlineCommunityCards(msg.communityCards || []);

    // 玩家手牌
    if (msg.yourCards && msg.yourCards.length === 2 && !humanCardsAnimating) {
        renderCard('humanCard1', msg.yourCards[0], !humanCardsHidden);
        renderCard('humanCard2', msg.yourCards[1], !humanCardsHidden);
        syncHumanCardsControl();
    } else if (!humanCardsAnimating) {
        renderCard('humanCard1', null, false);
        renderCard('humanCard2', null, false);
        syncHumanCardsControl();
    }
    if (shouldAnimateHoleCards && msg.yourCards?.length === 2) animateHumanCardsDeal();
    requestOnlineProbability(msg);
    updateProbability();
    updateHandAnalysis();
    updateOuts();

    // 筹码 & 下注（含 all-in 场景：用 chipsInPot 显示总下注）
    var myFullInfo = (msg.players || []).find(function(p) { return p.id === network.playerId; });
    var myStack = msg.yourStack || 0;
    var myIsAllIn = myFullInfo ? myFullInfo.isAllIn : false;
    if (myStack <= 0 && !myIsAllIn) {
        document.getElementById('humanStack').textContent = '💰 已破产';
        document.getElementById('humanStack').style.color = '#e94560';
    } else if (myIsAllIn) {
        document.getElementById('humanStack').textContent = 'ALL-IN';
        document.getElementById('humanStack').style.color = '#ffd700';
    } else {
        document.getElementById('humanStack').textContent = '$' + myStack.toLocaleString();
        document.getElementById('humanStack').style.color = '';
    }
    var totalBet = myFullInfo ? myFullInfo.chipsInPot || 0 : (msg.yourBet || 0);
    document.getElementById('humanRoundBet').textContent = '$' + (totalBet).toLocaleString();

    // 高亮
    const humanArea = document.getElementById('humanArea');
    if (msg.isYourTurn) {
        humanArea.classList.add('active');
    } else {
        humanArea.classList.remove('active');
    }

    // 位置标识
    renderOnlinePositionBadges(msg);

    // AI 玩家（绕桌排列）
    renderOnlinePlayers(msg.players || [], msg);

    // 行动按钮
    updateOnlineActions(msg);

    // 状态文字 & 下注轮次标签
    updateOnlineStatus(msg);

    // ── 轮到我了 → 提示音 ──
    if (msg.isYourTurn && lastOnlineState && !lastOnlineState.isYourTurn) {
        playTurnAlert();
    }

    // ── AI 行动动画检测（使用服务端 lastAction，不再推测） ──
    if (lastOnlineState) {
        const prevIdx = lastOnlineState.currentPlayerIdx;
        const prevPlayer = lastOnlineState.players ? lastOnlineState.players[prevIdx] : null;
        const currPlayer = (msg.players || [])[prevIdx];
        // 检测：上一位玩家在本帧有了新的 lastAction（服务端刚记录）
        if (prevPlayer && currPlayer && currPlayer.lastAction &&
            prevPlayer.id !== network.playerId &&
            (!prevPlayer.lastAction ||
             prevPlayer.lastAction.action !== currPlayer.lastAction.action ||
             prevPlayer.lastAction.amount !== currPlayer.lastAction.amount)) {
            const la = currPlayer.lastAction;
            const labels = { 'fold':'弃牌', 'check':'过牌', 'call':'跟注', 'raise':'加注', 'allin':'全下' };
            const label = labels[la.action] || la.action;
            const amtStr = (la.amount > 0 && la.action !== 'fold' && la.action !== 'check')
                ? ' $' + la.amount : '';
            showFloatingAction(currPlayer.name + ': ' + label + amtStr, la.action, false);
        }
    }
    lastOnlineState = msg;
}

function resetCommunityCardAnimation(source) {
    const sources = source ? [source] : Object.keys(communityAnimationState);
    for (const key of sources) {
        const state = communityAnimationState[key];
        if (!state) continue;
        state.keys = [];
        state.token++;
        // Force the next render even when the new hand has zero community cards.
        // Without this flag, [] matched [] and the previous hand's DOM stayed put.
        state.dirty = true;
    }

    const area = document.getElementById('communityArea');
    if (area) renderEmptyCommunitySlots(area);
}

function renderEmptyCommunitySlots(area) {
    area.replaceChildren();
    ['翻牌', '翻牌', '翻牌', '转牌', '河牌'].forEach(function(label) {
        const placeholder = document.createElement('div');
        placeholder.className = 'community-placeholder';
        placeholder.textContent = label;
        area.appendChild(placeholder);
    });
}

function renderCommunityCardsWithFlip(cards, source) {
    const area = document.getElementById('communityArea');
    if (!area) return;
    const state = communityAnimationState[source];
    if (!state) return;

    const cardKeys = cards.map(card => `${card.rank}-${card.suit}`);
    if (!state.dirty && cardKeys.length === state.keys.length && cardKeys.every((key, i) => key === state.keys[i])) {
        return;
    }

    const prefixMatches = state.keys.every((key, i) => cardKeys[i] === key);
    const previousCount = prefixMatches && cardKeys.length >= state.keys.length ? state.keys.length : 0;
    const newCardCount = Math.max(0, cardKeys.length - previousCount);
    const token = ++state.token;
    const reduceMotion = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    area.innerHTML = '';
    for (let i = 0; i < 5; i++) {
        if (i < cards.length) {
            const card = cards[i];
            const isRed = card.suit === 'hearts' || card.suit === 'diamonds';
            const suitSymbols = { spades: '♠', hearts: '♥', diamonds: '♦', clubs: '♣' };
            const stage = document.createElement('div');
            const flipper = document.createElement('div');
            const back = document.createElement('div');
            const front = document.createElement('div');
            const isNew = i >= previousCount;

            stage.className = 'community-card-stage';
            stage.setAttribute('aria-label', `${card.rank}${suitSymbols[card.suit]}`);
            flipper.className = 'community-card-flipper';
            back.className = 'card card-back community-card-face community-card-back';
            front.className = `card ${isRed ? 'red' : 'black'} community-card-face community-card-front`;
            front.innerHTML = `<span class="card-rank">${card.rank}</span><span class="card-suit">${suitSymbols[card.suit]}</span>`;
            flipper.appendChild(back);
            flipper.appendChild(front);
            stage.appendChild(flipper);

            if (isNew && !reduceMotion) {
                const dealIndex = i - previousCount;
                stage.classList.add('is-dealing');
                stage.style.setProperty('--deal-delay', `${dealIndex * 150}ms`);
                const flipDelay = newCardCount * 150 + 330 + dealIndex * 180;
                setTimeout(function() {
                    if (state.token === token && stage.isConnected) stage.classList.add('is-revealed');
                }, flipDelay);
            } else {
                stage.classList.add('is-revealed');
            }
            area.appendChild(stage);
        } else {
            const ph = document.createElement('div');
            ph.className = 'community-placeholder';
            ph.textContent = ['翻牌','翻牌','翻牌','转牌','河牌'][i];
            area.appendChild(ph);
        }
    }

    state.keys = cardKeys;
    state.dirty = false;
}

/** 渲染公共牌（联网版） */
function renderOnlineCommunityCards(cards) {
    renderCommunityCardsWithFlip(cards, 'online');
}

function updateOnlineBlindDisplay(msg) {
    const config = (msg && (msg.roomConfig || msg.config)) || msg || {};
    const isShortDeck = !!(config.isShortDeck || (msg && msg.isShortDeck));
    const small = Number(config.smallBlind || config.ante) || selectedSmallBlind;
    const big = Number(config.bigBlind || config.minBet) || selectedBigBlind;
    const blindsInfo = document.querySelector('.blinds-info');
    if (!blindsInfo) return;
    blindsInfo.innerHTML = isShortDeck
        ? `<span>Ante: <span id="sbDisplay">${small}</span></span><span>最小下注: <span id="bbDisplay">${big}</span></span>`
        : `<span>SB: <span id="sbDisplay">${small}</span></span><span>BB: <span id="bbDisplay">${big}</span></span>`;
}

/** 渲染联网玩家位置标识（服务端已传 sbIdx/bbIdx） */
function renderOnlinePositionBadges(msg) {
    const container = document.getElementById('humanPosBadges');
    if (!container) return;
    container.style.display = 'none';
    container.innerHTML = '';
    if (!msg.phase || msg.phase === 'idle') return;

    const myPlayer = (msg.players || []).find(p => p.id === network.playerId);
    const allPlayers = msg.players || [];
    const mySeat = myPlayer ? allPlayers.indexOf(myPlayer) : -1;

    const badges = [];
    if (mySeat >= 0 && mySeat === msg.dealerPos) {
        badges.push({ label: 'D', cls: 'badge-dealer', title: '庄家' });
    }
    if (mySeat >= 0 && mySeat === msg.sbIdx) {
        badges.push({ label: 'SB', cls: 'badge-sb', title: '小盲' });
    }
    if (mySeat >= 0 && mySeat === msg.bbIdx) {
        badges.push({ label: 'BB', cls: 'badge-bb', title: '大盲' });
    }
    if (badges.length > 0) {
        container.style.display = 'flex';
        for (const b of badges) {
            const badge = document.createElement('span');
            badge.className = `pos-badge ${b.cls}`;
            badge.textContent = b.label;
            badge.title = b.title;
            container.appendChild(badge);
        }
    }
}

/** 渲染玩家座位（联网版）—— 每人从自己座位视角看牌桌 */
function renderOnlinePlayers(players, state) {
    const container = document.getElementById('aiPlayersContainer');
    if (!container) return;

    const myId = network.playerId;
    const totalN = players.length;
    const mySeat = players.findIndex(function(p) { return p.id === myId; });
    if (mySeat < 0) return;
    container.innerHTML = '';

    const cw = container.offsetWidth || 900;
    const ch = container.offsetHeight || 420;
    // 计算 N-1 个非我的座位位置（第0个是"我左边第一个"顺时针方向）
    const positions = calculateTablePositions(totalN - 1, cw, ch);

    for (const seatIdx in players) {
        const i = parseInt(seatIdx);
        const player = players[i];
        if (player.id === myId) continue; // 我在 humanArea

        // 视角转换：这个玩家在我的屏幕上应该出现在哪个位置？
        // 顺时针步数 steps = 这个座位到我的顺时针距离
        // screenIdx = steps - 1（因为 positions[0] = 我左边第1个）
        var steps = ((i - mySeat) % totalN + totalN) % totalN;
        var screenIdx = steps - 1;

        const seat = document.createElement('div');
        seat.className = 'player-seat';
        if (player.folded) seat.classList.add('folded');
        if (player.isAllIn) seat.classList.add('all-in');

        // 按视角位置定位
        if (positions && positions[screenIdx]) {
            seat.style.left = positions[screenIdx].x + 'px';
            seat.style.top = positions[screenIdx].y + 'px';
        }

        // 位置标识（D / SB / BB）—— 基于实际座位索引 i，与视角无关
        const posBadges = [];
        if (i === state.dealerPos) posBadges.push({label:'D',title:'庄家',bg:'#ffd700',color:'#1a1a2e'});
        if (i === state.sbIdx) posBadges.push({label:'SB',title:'小盲',bg:'#3498db',color:'#fff'});
        if (i === state.bbIdx) posBadges.push({label:'BB',title:'大盲',bg:'#e67e22',color:'#fff'});
        for (const b of posBadges) {
            const badge = document.createElement('span');
            badge.className = 'pos-badge';
            badge.textContent = b.label;
            badge.title = b.title;
            badge.style.cssText = 'position:absolute;top:-12px;left:50%;transform:translateX(-50%);font-size:10px;background:' + b.bg + ';color:' + b.color + ';padding:1px 6px;border-radius:8px;font-weight:bold;z-index:5;';
            seat.appendChild(badge);
        }

        const avatar = document.createElement('div');
        avatar.className = 'player-avatar';
        avatar.textContent = player.avatar || (player.isHuman ? '👤' : '🤖');
        seat.appendChild(avatar);

        const name = document.createElement('div');
        name.className = 'player-name';
        name.textContent = player.name;
        seat.appendChild(name);

        const stack = document.createElement('div');
        stack.className = 'player-stack';
        if (player.isAllIn) {
            stack.textContent = 'ALL-IN';
        } else if (player.stack <= 0) {
            stack.textContent = '💰 已破产';
            stack.style.color = '#e94560';
        } else {
            stack.textContent = '$' + (player.stack || 0).toLocaleString();
        }
        seat.appendChild(stack);

        // 下注额
        if (player.chipsInPot > 0 && state.phase && state.phase !== 'idle') {
            const bet = document.createElement('div');
            bet.className = 'player-bet';
            bet.textContent = '$' + (player.chipsInPot || 0);
            seat.appendChild(bet);
        }

        // 后手空白手牌
        const cardsDiv = document.createElement('div');
        cardsDiv.className = 'player-cards';
        if (player.folded) {
            const c1 = document.createElement('div');
            c1.className = 'card card-back card-back-logo small';
            cardsDiv.appendChild(c1);
            const c2 = c1.cloneNode();
            cardsDiv.appendChild(c2);
        } else if (state.phase === 'showdown' || state.phase === 'idle') {
            // 摊牌时也不显示牌，空着
        } else {
            const c1 = document.createElement('div');
            c1.className = 'card card-back small';
            cardsDiv.appendChild(c1);
            const c2 = c1.cloneNode();
            cardsDiv.appendChild(c2);
        }
        seat.appendChild(cardsDiv);

        // 思考动画
        if (state && state.currentPlayerIdx !== undefined && i === state.currentPlayerIdx && !player.folded) {
            seat.classList.add('thinking');
            var dots = document.createElement('div');
            dots.className = 'thinking-dots';
            dots.textContent = '...';
            seat.appendChild(dots);
        }

        container.appendChild(seat);
    }
}

/** 追踪上一帧的联网状态，用于 AI 行动动画检测 */
var lastOnlineState = null;

/** 更新联网模式下的行动按钮 */
function updateOnlineActions(msg) {
    const legal = msg.legalActions || { actions: [], toCall: 0, canRaise: false, minRaiseTo: 0, maxRaiseTo: 0 };
    const legalTypes = Array.isArray(legal.actions) ? legal.actions : [];
    const isTurn = !!msg.isYourTurn && !network.pendingAction;
    const toCall = Math.max(0, Number(legal.toCall) || 0);
    const stack = msg.yourStack || 0;
    const myPlayer = (msg.players || []).find(p => p.id === network.playerId);
    const folded = myPlayer ? myPlayer.folded : false;
    const allIn = myPlayer ? myPlayer.isAllIn : false;

    const btnFold = document.getElementById('btnFold');
    const btnCheck = document.getElementById('btnCheck');
    const btnCall = document.getElementById('btnCall');
    const btnRaise = document.getElementById('btnRaise');
    const btnAllin = document.getElementById('btnAllin');
    const explainEl = document.getElementById('actionExplain');

    btnFold.disabled = true;
    btnCheck.disabled = true;
    btnCall.disabled = true;
    btnRaise.disabled = true;
    btnAllin.disabled = true;

    // 破产玩家：全按钮禁用 + 提示
    if (stack <= 0 && !allIn) {
        explainEl.textContent = '💰 你已破产 — 筹码输光，等待新一局或离开';
        return;
    }

    if (!isTurn) {
        if (network.pendingAction) {
            explainEl.textContent = '⏳ 等待服务器确认...';
            return;
        }
        if (!msg.phase || msg.phase === 'idle') {
            explainEl.textContent = '⏳ 等待新一局开始...';
        } else if (folded) {
            explainEl.textContent = '❌ 你已弃牌，等待本局结束';
        } else if (allIn) {
            explainEl.textContent = '🃏 你已全下，等待摊牌';
        } else {
            explainEl.textContent = '⏳ 等待其他玩家行动...';
        }
        return;
    }

    btnFold.disabled = !legalTypes.includes('fold');
    btnCheck.disabled = !legalTypes.includes('check');
    btnCall.disabled = !legalTypes.includes('call');
    btnRaise.disabled = !legalTypes.includes('raise');
    btnAllin.disabled = !legalTypes.includes('allin');
    btnCheck.textContent = '过牌 ✓';
    btnCall.textContent = '跟注 $' + Math.min(toCall, stack);
    btnRaise.textContent = '加注';
    btnAllin.textContent = '全下 $' + stack;

    if (toCall > 0) {
        explainEl.textContent = legalTypes.includes('raise')
            ? '💰 需要跟注 $' + Math.min(toCall, stack) + '，也可加注或弃牌'
            : '💰 需要跟注 $' + Math.min(toCall, stack) + '，当前加注权未开放';
    } else {
        explainEl.textContent = legalTypes.includes('raise')
            ? '✅ 你可以过牌或下注'
            : '✅ 你可以过牌';
    }
}

/** 更新联网模式下的状态文字 */
function updateOnlineStatus(msg) {
    const labels = { preflop: '翻牌前', flop: '翻牌', turn: '转牌', river: '河牌' };
    const phaseLabel = labels[msg.phase] || msg.phase;
    document.getElementById('bettingRoundLabel').textContent = msg.phase !== 'idle' ? phaseLabel : '';

    const status = document.getElementById('gameStatus');
    const active = (msg.players || []).filter(p => !p.folded && !p.isAllIn).length;
    const folded = (msg.players || []).filter(p => p.folded && !p.isAllIn).length;

    if (msg.isYourTurn) {
        status.textContent = '📌 轮到你了 — ' + phaseLabel + ' · ' + active + ' 人在局';
    } else if (msg.phase === 'idle') {
        status.textContent = '⏳ 等待新一局...';
    } else {
        const current = (msg.players || [])[msg.currentPlayerIdx];
        const name = current ? current.name : '未知';
        status.textContent = '🤔 ' + name + ' 思考中... ' + phaseLabel + ' · ' + active + ' 人在局' + (folded ? ' · ' + folded + ' 人弃牌' : '');
    }
}

/** 联网摊牌弹窗 */
function showOnlineShowdown(result) {
    const modal = document.getElementById('showdownModal');
    const content = document.getElementById('modalContent');
    content.style.position = '';
    content.style.left = '';
    content.style.top = '';
    content.style.margin = '0 auto';
    content.style.transform = 'none';
    document.getElementById('modalTitle').textContent = result.reason === 'fold' ? '🏆 胜出' : '🏆 摊牌';

    // 赢家
    const winnerNames = result.winners.map(w => w.isHuman ? (w.id === network.playerId ? '你' : (w.name || '玩家')) : w.name).join(', ');
    document.getElementById('winnerName').textContent = winnerNames;
    document.getElementById('winnerAmount').textContent = '$' + (result.pot || 0).toLocaleString();

    // 牌型名称：弃牌胜出不显示牌型，摊牌才显示
    const nameMap = result.isShortDeck ? SD_HAND_TYPE_NAMES : HAND_TYPE_NAMES;
    const isFoldWin = result.reason === 'fold';
    const handName = isFoldWin ? '全部弃牌' : (nameMap[result.handRank] || '—');
    const handLabel = document.getElementById('winningHandName');
    handLabel.textContent = handName;
    handLabel.style.display = isFoldWin ? 'none' : '';

    // 玩家结果列表
    const resultsDiv = document.getElementById('modalResults');
    resultsDiv.innerHTML = '';

    // 公共牌
    const state = network.gameState;
    if (state && state.communityCards && state.communityCards.length > 0) {
        const section = document.createElement('div');
        section.style.cssText = 'text-align:center;margin-bottom:12px;padding:8px 10px;background:rgba(0,0,0,0.25);border-radius:10px;';
        const label = document.createElement('div');
        label.style.cssText = 'font-size:11px;color:#888;margin-bottom:6px;';
        label.textContent = '🃏 公共牌';
        section.appendChild(label);
        const cc = document.createElement('div');
        cc.style.cssText = 'display:flex;justify-content:center;gap:4px;';
        const suitSymbols = { spades: '♠', hearts: '♥', diamonds: '♦', clubs: '♣' };
        for (const card of state.communityCards) {
            const isRed = card.suit === 'hearts' || card.suit === 'diamonds';
            const el = document.createElement('span');
            el.style.cssText = 'display:inline-flex;align-items:center;gap:2px;padding:3px 8px;border-radius:5px;background:rgba(255,255,255,0.12);font-size:14px;font-weight:600;' + (isRed ? 'color:#ef5350;' : 'color:#fff;');
            el.textContent = card.rank + ' ' + (suitSymbols[card.suit] || '');
            cc.appendChild(el);
        }
        section.appendChild(cc);
        resultsDiv.appendChild(section);
    }

    // 每位玩家的牌和牌型
    for (const r of result.results) {
        const row = document.createElement('div');
        row.className = 'modal-result-row';
        const isWinner = result.winners.some(w => w.id === r.id);
        const isMe = r.id === network.playerId;
        const name = r.isHuman ? (isMe ? '你' : (r.name || '玩家')) : r.name;

        const nameSpan = document.createElement('span');
        if (isWinner) nameSpan.textContent = '🏆 ';
        const strong = document.createElement('strong');
        strong.textContent = name;
        nameSpan.appendChild(strong);

        // 手牌
        const cardsContainer = document.createElement('span');
        cardsContainer.style.cssText = 'display:inline-flex;gap:4px;';
        const suitSymbols2 = { spades: '♠', hearts: '♥', diamonds: '♦', clubs: '♣' };
        for (const c of (r.cards || [])) {
            if (!c.rank) continue;
            const isRed = c.suit === 'hearts' || c.suit === 'diamonds';
            const chip = document.createElement('span');
            chip.style.cssText = 'display:inline-flex;align-items:center;gap:2px;padding:3px 8px;border-radius:5px;background:rgba(255,255,255,0.12);font-size:14px;font-weight:600;' + (isRed ? 'color:#ef5350;' : 'color:#fff;');
            chip.textContent = c.rank + ' ' + (suitSymbols2[c.suit] || '');
            cardsContainer.appendChild(chip);
        }

        const handSpan = document.createElement('span');
        handSpan.className = 'hand-name';
        handSpan.textContent = isFoldWin ? '—' : (nameMap[r.handRank] || '—');

        row.appendChild(nameSpan);
        row.appendChild(cardsContainer);
        row.appendChild(handSpan);
        if (isWinner) row.style.background = 'rgba(76,175,80,0.15)';

        resultsDiv.appendChild(row);
    }

    modal.classList.add('show');
}

/** 联网模式下的玩家行动 */
function onlineDoAction(type) {
    if (!network.gameState || !network.gameState.isYourTurn || network.pendingAction) return;

    const legal = network.gameState.legalActions || { actions: [], toCall: 0 };
    if (!Array.isArray(legal.actions) || !legal.actions.includes(type)) return;
    const toCall = Math.max(0, Number(legal.toCall) || 0);
    const stack = network.gameState.yourStack || 0;

    switch (type) {
        case 'fold':
            sendAction('fold', 0);
            break;
        case 'check':
            sendAction('check', 0);
            break;
        case 'call':
            sendAction('call', toCall);
            break;
        case 'allin':
            sendAction('allin', stack);
            break;
    }

    // 立即禁用按钮，防重复点击
    document.getElementById('btnFold').disabled = true;
    document.getElementById('btnCheck').disabled = true;
    document.getElementById('btnCall').disabled = true;
    document.getElementById('btnRaise').disabled = true;
    document.getElementById('btnAllin').disabled = true;
    document.getElementById('actionExplain').textContent = '⏳ 等待服务器确认...';
}

function doDisconnect() {
    disconnect();
    inRoom = false;
    isRoomHost = false;
    resetOnlinePresentationState();
    document.getElementById('connectPanel').style.display = 'block';
    document.getElementById('roomLobby').style.display = 'none';
    document.getElementById('bottomPanel').style.display = 'none';
    document.getElementById('startScreen').style.display = 'flex';
    document.getElementById('communityArea').style.display = 'none';
    document.getElementById('potDisplay').style.display = 'none';
    showNetworkError('🔌 已断开连接');
}

function doCreateRoom() {
    // 去掉了单独创建 — 统一用"进入房间"，创建=第一个进入
    doJoinRoom();
}

function doJoinRoom() {
    const code = document.getElementById('joinRoomCode').value.trim();
    const name = document.getElementById('playerNameInput').value.trim() || '玩家';
    if (!code) { showNetworkError('请选择房间号'); return; }
    // 传递房间配置（房间不存在时服务端会用这些配置创建）
    const maxPlayers = parseInt(document.getElementById('roomMaxPlayers').value) || 6;
    const isShortDeck = document.getElementById('roomShortDeck').checked;
    joinRoom(code, name, {
        maxPlayers,
        isShortDeck,
        startingStack: selectedStartingStack,
        smallBlind: selectedSmallBlind,
        bigBlind: selectedBigBlind,
        ante: selectedSmallBlind,
        minBet: selectedBigBlind,
        aiDelay: selectedAiDelay
    });
}

function doLeaveRoom() {
    leaveRoom();
    inRoom = false;
    isRoomHost = false;
    resetOnlinePresentationState();
    document.getElementById('createRoomForm').style.display = 'block';
    document.getElementById('joinRoomForm').style.display = 'block';
    document.getElementById('roomActions').style.display = 'none';
    // 回到主界面
    document.getElementById('bottomPanel').style.display = 'none';
    document.getElementById('startScreen').style.display = 'flex';
    document.getElementById('communityArea').style.display = 'none';
    document.getElementById('potDisplay').style.display = 'none';
    document.getElementById('gameStatus').textContent = gameMode === 'shortdeck'
        ? '点击「开始游戏」开始短牌对局！'
        : '点击「开始游戏」开始德州扑克对局！';
    document.getElementById('aiPlayersContainer').innerHTML = '';
    showNetworkError('已离开房间');
}

function doStartGame() {
    startGame();
}

function updatePlayerList(players) {
    const el = document.getElementById('playerList');
    if (!el) return;
    el.replaceChildren();
    for (const player of (players || []).slice().sort((a, b) => (a.seatId || 0) - (b.seatId || 0))) {
        const row = document.createElement('div');
        row.style.cssText = 'display:flex;align-items:center;gap:8px;padding:4px 0;border-bottom:1px solid rgba(255,255,255,0.05);';
        const connection = document.createElement('span');
        connection.textContent = player.connected ? '🟢' : '🔴';
        const kind = document.createElement('span');
        kind.textContent = player.isHuman ? '👤' : '🤖';
        const name = document.createElement('span');
        name.style.color = player.id === network.playerId ? '#ffd700' : '#e0e0e0';
        name.textContent = String(player.name || '玩家') + (player.id === network.playerId ? ' (你)' : '');
        row.append(connection, kind, name);
        el.appendChild(row);
    }
}

/** Update player count buttons based on mode */
function updatePlayerCountOptions() {
    const container = document.querySelector('.count-options');
    if (!container) return;

    const isSD = gameMode === 'shortdeck';

    // Short deck: 2-8人, 默认6人
    // Standard: 2,6,7,8,9,10人, 默认10人
    const options = isSD
        ? [2, 3, 4, 5, 6, 7, 8]
        : [2, 6, 7, 8, 9, 10];
    const defaultOption = isSD ? 6 : 10;

    container.innerHTML = options.map(n =>
        `<button class="count-btn${n === defaultOption ? ' selected' : ''}" data-count="${n}">${n} 人</button>`
    ).join('');

    // 同步更新联机模式的人数下拉框
    updateOnlinePlayerCountOptions();
}

/** 更新联机模式人数下拉框（跟随游戏模式） */
function updateOnlinePlayerCountOptions() {
    const select = document.getElementById('roomMaxPlayers');
    if (!select) return;

    const isSD = gameMode === 'shortdeck';
    // 标准: 2,6,7,8,9,10人 默认10人
    // 短牌: 2-8人 默认6人
    const options = isSD
        ? [2, 3, 4, 5, 6, 7, 8]
        : [2, 6, 7, 8, 9, 10];
    const defaultOption = isSD ? 6 : 10;

    select.innerHTML = options.map(n =>
        `<option value="${n}"${n === defaultOption ? ' selected' : ''}>${n}人</option>`
    ).join('');
}

/** Update start screen title/subtitle based on current mode */
function updateStartScreenMode() {
    const isSD = gameMode === 'shortdeck';
    document.getElementById('startTitle').textContent = isSD ? '6+ Short Deck' : 'Texas Hold\'em';
    document.getElementById('startSubtitle').textContent = isSD
        ? '36张牌的快节奏短牌对战 — 同花大于葫芦，顺子大于三条'
        : '';
    document.getElementById('gameStatus').textContent = isSD
        ? '点击「开始游戏」开始短牌对局！'
        : '点击「开始游戏」开始德州扑克对局！';
}

/** Attach all UI callbacks to a game instance */
function setupGameCallbacks(g) {
    g.onUpdate = function() {
        if (typeof bgMusic !== 'undefined') bgMusic.setGamePhase(g.phase);
        renderUI();
        updateActionButtons();
        updateProbability();
        updateHandAnalysis();
        updateOuts();
        updateGameStatus();
        renderCommunityCards();
        renderAIPlayers();
        updatePot();
        updateBettingRoundLabel();
        document.getElementById('handCount').textContent = g.numHands;
    };

    g.onHandEnd = function(result) {
        const playerBusted = g.humanPlayer && g.humanPlayer.stack <= 0;
        if (result.reason === 'showdown') {
            document.getElementById('gameStatus').textContent = '⏳ 正在结算... 2 秒';
            setTimeout(() => {
                showShowdown(result);
                if (playerBusted) {
                    const modalResults = document.getElementById('modalResults');
                    const bustedNotice = document.createElement('div');
                    bustedNotice.style.cssText = 'text-align:center;padding:10px;margin-top:8px;background:rgba(233,69,96,0.15);border-radius:8px;color:#e94560;font-weight:bold;';
                    bustedNotice.textContent = '💔 你已输光所有筹码！';
                    modalResults.appendChild(bustedNotice);
                }
            }, 2000);
        } else if (result.winner) {
            if (playerBusted) {
                showMessage('💔 你输了', `${result.winner.name} 赢得 $${result.pot} — 你已输光！`);
            } else if (result.winner.isHuman) {
                showMessage('🎉 你赢了！', `${result.winner.name} 赢得 $${result.pot} (全部弃牌)`);
            } else {
                showAIMessage(result.winner.name, `赢得 $${result.pot}`);
            }
        }
    };

    g.onAIThinking = function(idx) {
        const seat = document.querySelector(`.player-seat[data-player-index="${idx}"]`);
        if (seat) {
            seat.classList.add('thinking');
            let dots = seat.querySelector('.thinking-dots');
            if (!dots) {
                dots = document.createElement('div');
                dots.className = 'thinking-dots';
                dots.textContent = '...';
                seat.appendChild(dots);
            }
        }
    };

    g.onAIAction = function(idx, decision) {
        const seat = document.querySelector(`.player-seat[data-player-index="${idx}"]`);
        if (seat) {
            seat.classList.remove('thinking');
            const dots = seat.querySelector('.thinking-dots');
            if (dots) dots.remove();
        }
        const player = g.players[idx];
        if (!player) return;
        const labels = {
            'fold': '弃牌', 'check': '过牌', 'call': '跟注',
            'raise': '加注', 'allin': '全下',
            'small blind': '小盲', 'big blind': '大盲', 'dealer ante': '庄前注'
        };
        const label = labels[decision.action] || decision.action;
        const amt = decision.amount > 0 && !['fold','check'].includes(decision.action)
            ? ` $${decision.amount}` : '';
        showFloatingAction(`${player.name}: ${label}${amt}`, decision.action, player.isHuman);
    };
}

// Floating action animation
function showFloatingAction(text, actionType, isHuman) {
    const container = document.getElementById('tableContainer');
    if (!container) return;

    const floatEl = document.createElement('div');
    floatEl.className = `floating-action action-${actionType}`;
    floatEl.textContent = text;

    // Random horizontal position
    const leftPos = 20 + Math.random() * 60;
    floatEl.style.left = leftPos + '%';

    container.appendChild(floatEl);

    // Remove after animation completes (3s animation)
    setTimeout(() => {
        if (floatEl.parentNode) floatEl.parentNode.removeChild(floatEl);
    }, 3500);
}

// ============================================================
// UI Rendering Functions
// ============================================================

function renderUI() {
    const bottomPanel = document.getElementById('bottomPanel');
    const startScreen = document.getElementById('startScreen');

    if (game.phase !== 'idle' || gameStarted) {
        bottomPanel.style.display = 'flex';
        startScreen.style.display = 'none';
        gameStarted = true;
    }

    // Human player area
    if (game.humanPlayer) {
        document.getElementById('humanStack').textContent = '$' + game.humanPlayer.stack.toLocaleString();
        document.getElementById('humanRoundBet').textContent = '$' + (game.humanPlayer.chipsInPot || 0).toLocaleString();

        // Human cards
        if (game.humanPlayer.holeCards && game.humanPlayer.holeCards.length === 2 && !humanCardsAnimating) {
            renderCard('humanCard1', game.humanPlayer.holeCards[0], !humanCardsHidden);
            renderCard('humanCard2', game.humanPlayer.holeCards[1], !humanCardsHidden);
            syncHumanCardsControl();
        }

        // Highlight human area when active
        const humanArea = document.getElementById('humanArea');
        if (game.isPlayerTurn()) {
            humanArea.classList.add('active');
        } else {
            humanArea.classList.remove('active');
        }

        // Human position badges
        renderHumanPositionBadges();
    }
}

function renderCard(elementId, card, faceUp) {
    const el = document.getElementById(elementId);
    if (!el) return;

    if (!card || !faceUp) {
        el.className = 'card card-back';
        el.replaceChildren();
        el.style.background = '';
        return;
    }

    const isRed = card.suit === 'hearts' || card.suit === 'diamonds';
    const suitSymbols = { spades: '♠', hearts: '♥', diamonds: '♦', clubs: '♣' };

    el.className = `card ${isRed ? 'red' : 'black'}`;
    el.replaceChildren();
    const rank = document.createElement('span');
    rank.className = 'card-rank';
    rank.textContent = String(card.rank || '');
    const suit = document.createElement('span');
    suit.className = 'card-suit';
    suit.textContent = suitSymbols[card.suit] || '';
    el.append(rank, suit);
}

function getCurrentHumanCards() {
    if (playMode === 'online') {
        return network.gameState && Array.isArray(network.gameState.yourCards)
            ? network.gameState.yourCards : [];
    }
    return game && game.humanPlayer && Array.isArray(game.humanPlayer.holeCards)
        ? game.humanPlayer.holeCards : [];
}

function syncHumanCardsControl() {
    const container = document.getElementById('humanCards');
    const label = document.getElementById('humanCardsLabel');
    if (!container || !label) return;
    const hasCards = getCurrentHumanCards().length === 2;
    container.setAttribute('aria-pressed', humanCardsHidden ? 'true' : 'false');
    container.setAttribute('aria-label', humanCardsHidden ? '显示我的手牌' : '隐藏我的手牌');
    container.title = humanCardsHidden ? '点击将手牌翻回正面' : '点击将手牌翻到背面';
    label.textContent = !hasCards ? '我的手牌'
        : humanCardsHidden ? '我的手牌 · 已隐藏，点击查看' : '我的手牌 · 点击隐藏';
    const panel = document.getElementById('probPanel');
    const shield = document.getElementById('privacyAnalysisShield');
    if (panel) panel.classList.toggle('privacy-hidden', humanCardsHidden && hasCards);
    if (shield) shield.hidden = !(humanCardsHidden && hasCards);
}

function renderCurrentHumanCards() {
    const cards = getCurrentHumanCards();
    if (cards.length === 2) {
        renderCard('humanCard1', cards[0], !humanCardsHidden);
        renderCard('humanCard2', cards[1], !humanCardsHidden);
    } else {
        renderCard('humanCard1', null, false);
        renderCard('humanCard2', null, false);
    }
    syncHumanCardsControl();
}

function resetHumanCardsVisibility() {
    humanDealAnimationToken++;
    humanCardsHidden = false;
    humanCardsAnimating = false;
    const container = document.getElementById('humanCards');
    if (container) container.classList.remove('flipping');
    renderCurrentHumanCards();
}

function playHumanCardsFlip(token, renderAtMidpoint, onComplete) {
    const container = document.getElementById('humanCards');
    if (!container || token !== humanDealAnimationToken) return;
    container.classList.remove('flipping');
    void container.offsetWidth;
    container.classList.add('flipping');
    setTimeout(() => {
        if (token === humanDealAnimationToken) renderAtMidpoint();
    }, 175);
    setTimeout(() => {
        if (token !== humanDealAnimationToken) return;
        container.classList.remove('flipping');
        humanCardsAnimating = false;
        if (onComplete) onComplete();
    }, 420);
}

function animateHumanCardsDeal() {
    const cards = getCurrentHumanCards();
    const container = document.getElementById('humanCards');
    if (!container || cards.length !== 2) return;
    const reduceMotion = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    const token = ++humanDealAnimationToken;
    humanCardsHidden = false;
    if (reduceMotion) {
        humanCardsAnimating = false;
        renderCurrentHumanCards();
        return;
    }
    humanCardsAnimating = true;
    renderCard('humanCard1', null, false);
    renderCard('humanCard2', null, false);
    container.classList.remove('flipping');
    syncHumanCardsControl();
    setTimeout(() => {
        playHumanCardsFlip(token, () => {
            renderCard('humanCard1', cards[0], true);
            renderCard('humanCard2', cards[1], true);
        }, renderCurrentHumanCards);
    }, 260);
}

function toggleHumanCardsVisibility() {
    const container = document.getElementById('humanCards');
    if (!container || humanCardsAnimating || getCurrentHumanCards().length !== 2) return;
    const token = ++humanDealAnimationToken;
    humanCardsAnimating = true;
    playHumanCardsFlip(token, () => {
        humanCardsHidden = !humanCardsHidden;
        renderCurrentHumanCards();
    });
}

function handleHumanCardsKey(event) {
    if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        toggleHumanCardsVisibility();
    }
}

function renderHumanPositionBadges() {
    const container = document.getElementById('humanPosBadges');
    if (!container) return;

    container.style.display = 'none';
    container.innerHTML = '';

    if (game.phase === 'idle') return;

    const badges = [];
    if (game.dealerPosition === 0) {
        badges.push({ label: 'D', cls: 'badge-dealer', title: '庄家' });
    }
    if (game.sbIndex === 0) {
        badges.push({ label: 'SB', cls: 'badge-sb', title: '小盲' });
    }
    if (game.bbIndex === 0) {
        badges.push({ label: 'BB', cls: 'badge-bb', title: '大盲' });
    }

    if (badges.length > 0) {
        container.style.display = 'flex';
        for (const b of badges) {
            const badge = document.createElement('span');
            badge.className = `pos-badge ${b.cls}`;
            badge.textContent = b.label;
            badge.title = b.title;
            container.appendChild(badge);
        }
    }
}

function renderCommunityCards() {
    const cards = game.communityCards || [];
    renderCommunityCardsWithFlip(cards, 'local');
}

function renderAIPlayers() {
    const container = document.getElementById('aiPlayersContainer');
    if (!container) return;

    const aiPlayers = game.players.filter(p => !p.isHuman);
    container.innerHTML = '';

    const aiCount = aiPlayers.length;

    // Get container dimensions for positioning
    const cw = container.offsetWidth || 900;
    const ch = container.offsetHeight || 420;

    // Calculate positions based on player count
    const positions = calculateTablePositions(aiCount, cw, ch);

    for (const [i, player] of aiPlayers.entries()) {
        const playerIdx = game.players.indexOf(player);
        const seat = document.createElement('div');
        seat.className = 'player-seat';
        seat.id = `ai-seat-${i}`;
        seat.dataset.playerIndex = playerIdx;

        // Apply absolute positioning
        if (positions && positions[i]) {
            seat.style.left = positions[i].x + 'px';
            seat.style.top = positions[i].y + 'px';
        }

        if (player.folded) seat.classList.add('folded');
        if (player.isAllIn) seat.classList.add('all-in');
        if (game.currentPlayer && !game.currentPlayer.isHuman &&
            game.currentPlayer.name === player.name) {
            seat.classList.add('active');
        }

        // Position badges (D, SB, BB)
        const posBadges = [];
        if (game.phase !== 'idle') {
            if (playerIdx === game.dealerPosition) {
                posBadges.push({ label: 'D', cls: 'badge-dealer', title: '庄家' });
            }
            if (playerIdx === game.sbIndex) {
                posBadges.push({ label: 'SB', cls: 'badge-sb', title: '小盲' });
            }
            if (playerIdx === game.bbIndex) {
                posBadges.push({ label: 'BB', cls: 'badge-bb', title: '大盲' });
            }
        }

        if (posBadges.length > 0) {
            const badgeContainer = document.createElement('div');
            badgeContainer.className = 'position-badges';
            for (const b of posBadges) {
                const badge = document.createElement('span');
                badge.className = `pos-badge ${b.cls}`;
                badge.textContent = b.label;
                badge.title = b.title;
                badgeContainer.appendChild(badge);
            }
            seat.appendChild(badgeContainer);
        }

        const avatar = document.createElement('div');
        avatar.className = 'player-avatar';
        avatar.textContent = player.avatar || '🤖';
        seat.appendChild(avatar);

        const name = document.createElement('div');
        name.className = 'player-name';
        name.textContent = player.name;
        seat.appendChild(name);

        const stack = document.createElement('div');
        stack.className = 'player-stack';
        stack.textContent = '$' + player.stack.toLocaleString();
        if (player.isAllIn) stack.textContent = 'ALL-IN';
        seat.appendChild(stack);

        // Cards
        const cardsDiv = document.createElement('div');
        cardsDiv.className = 'player-cards';
        if (player.holeCards && player.holeCards.length === 2) {
            if (player.folded) {
                const c1 = document.createElement('div');
                c1.className = 'card card-back card-back-logo small';
                cardsDiv.appendChild(c1);
                const c2 = c1.cloneNode();
                cardsDiv.appendChild(c2);
            } else if (game.phase === 'showdown' || game.phase === 'idle') {
                for (const card of player.holeCards) {
                    const isRed = card.suit === 'hearts' || card.suit === 'diamonds';
                    const suitSymbols = { spades: '♠', hearts: '♥', diamonds: '♦', clubs: '♣' };
                    const cardEl = document.createElement('div');
                    cardEl.className = `card small ${isRed ? 'red' : 'black'}`;
                    cardEl.innerHTML = `
                        <span class="card-rank">${card.rank}</span>
                        <span class="card-suit">${suitSymbols[card.suit]}</span>
                    `;
                    cardsDiv.appendChild(cardEl);
                }
            } else {
                const c1 = document.createElement('div');
                c1.className = 'card card-back small';
                cardsDiv.appendChild(c1);
                const c2 = c1.cloneNode();
                cardsDiv.appendChild(c2);
            }
        }
        seat.appendChild(cardsDiv);

        // Bet/chips
        if (player.chipsInPot > 0 && game.phase !== 'idle') {
            const bet = document.createElement('div');
            bet.className = 'player-bet';
            bet.textContent = '$' + player.chipsInPot;
            seat.appendChild(bet);
        }

        // Last action
        if (player.lastAction && game.phase !== 'idle') {
            const action = document.createElement('div');
            action.className = `player-last-action action-${player.lastAction.action} action-pop`;
            const actionLabels = {
                'fold': '弃牌', 'check': '过牌', 'call': '跟注',
                'raise': '加注', 'allin': '全下',
                'small blind': '小盲', 'big blind': '大盲'
            };
            const label = actionLabels[player.lastAction.action] || player.lastAction.action;
            action.textContent = player.lastAction.amount > 0 && !['fold','check'].includes(player.lastAction.action)
                ? `${label} $${player.lastAction.amount}`
                : label;
            seat.appendChild(action);
            setTimeout(() => action.classList.remove('action-pop'), 800);
        }

        container.appendChild(seat);
    }
}

/**
 * Calculate AI player seat positions — compact top row, minimal sides.
 * 2 players: AI opposite the human (top center).
 * 6-10 players: tight top row (fit as many as possible), overflow on sides.
 */
function calculateTablePositions(aiCount, cw, ch) {
    const positions = [];

    if (aiCount === 1) {
        return [{ x: cw / 2, y: Math.max(75, ch * 0.07) }];
    }

    // Compact: each seat slot ≈ 130px with the smaller seat sizes
    // This fits up to 7 on wide screens, fewer on narrow screens.
    const cap = Math.max(3, Math.min(7, Math.floor(cw / 130)));
    const sideCount = Math.min(2, Math.max(0, aiCount - cap));
    const topCount = aiCount - sideCount;
    const left = sideCount > 0 ? 1 : 0;
    const right = sideCount > 1 ? 1 : 0;

    // ── Clockwise order from human seat (bottom) ──
    // Human at 6 o'clock. Clockwise: left side → top (left-to-right) → right side
    const topY = Math.max(70, ch * 0.06);
    const margin = Math.max(45, cw * 0.03 + (left + right) * 20);
    const span = cw - 2 * margin;

    // 1. Left side (clockwise first from human)
    if (left > 0) {
        const sx = Math.max(100, cw * 0.09);
        const sy = Math.min(Math.max(topY + 320, ch * 0.46), ch * 0.74);
        positions.push({ x: sx, y: sy });
    }

    // 2. Top row: left-to-right (clockwise across the top)
    for (let i = 0; i < topCount; i++) {
        const t = topCount > 1 ? i / (topCount - 1) : 0.5;
        const x = margin + span * t;
        const dist = Math.abs(t - 0.5) * 2; // 0 center, 1 edges
        const drop = 42 * dist * dist;      // quadratic, max 42px at edges
        positions.push({ x, y: topY + drop });
    }

    // 3. Right side (clockwise last, comes back to human)
    if (right > 0) {
        const sx = Math.max(100, cw * 0.09);
        const sy = Math.min(Math.max(topY + 320, ch * 0.46), ch * 0.74);
        positions.push({ x: cw - sx, y: sy });
    }

    return positions;
}

function updateActionButtons() {
    const btnFold = document.getElementById('btnFold');
    const btnCheck = document.getElementById('btnCheck');
    const btnCall = document.getElementById('btnCall');
    const btnRaise = document.getElementById('btnRaise');
    const btnAllin = document.getElementById('btnAllin');
    const explainEl = document.getElementById('actionExplain');

    const isTurn = game.isPlayerTurn();
    const toCall = Math.max(0, game.currentBet - (game.roundBets[0] || 0));
    const playerStack = game.humanPlayer ? game.humanPlayer.stack : 0;
    const isFolded = game.humanPlayer ? game.humanPlayer.folded : false;
    const isAllIn = game.humanPlayer ? game.humanPlayer.isAllIn : false;

    btnFold.disabled = true;
    btnCheck.disabled = true;
    btnCall.disabled = true;
    btnRaise.disabled = true;
    btnAllin.disabled = true;

    // Build explanation
    let explainText = '';

    if (!isTurn) {
        if (game.phase === 'idle') {
            explainText = '⏳ 等待新一局开始...';
        } else if (isFolded) {
            explainText = '❌ 你已弃牌，等待本局结束';
        } else if (isAllIn) {
            explainText = '🃏 你已全下，等待摊牌';
        } else if (game.currentPlayer && !game.currentPlayer.isHuman) {
            explainText = `🤔 ${game.currentPlayer.name} 正在思考...`;
        } else {
            explainText = '⏳ 请等待其他玩家行动';
        }
    } else if (isTurn) {
        const actions = game.getAvailableActions();

        // Check if player hasn't acted yet this round - build explanation
        if (toCall > 0) {
            // There's a bet to call
            explainText = `💰 当前注额 $${game.currentBet}，你需要跟注 $${toCall} 才能继续`;
            btnCall.disabled = false;
            btnCall.textContent = `跟注 $${toCall}`;
        } else {
            // No bet to call - can check
            btnCheck.disabled = false;
            btnCheck.textContent = '过牌 ✓';
        }

        for (const a of actions) {
            switch (a.type) {
                case 'fold':
                    btnFold.disabled = false;
                    break;
                case 'check':
                    btnCheck.disabled = false;
                    btnCheck.textContent = '过牌 ✓';
                    break;
                case 'call':
                    btnCall.disabled = false;
                    btnCall.textContent = `跟注 $${a.amount}`;
                    break;
                case 'raise':
                    btnRaise.disabled = false;
                    btnRaise.textContent = `加注`;
                    btnRaise.dataset.targetAmount = a.amount;
                    break;
                case 'allin':
                    btnAllin.disabled = false;
                    btnAllin.textContent = `全下 $${a.amount}`;
                    break;
            }
        }

        // Build explanation based on available actions
        const actionTypes = actions.map(a => a.type);
        if (toCall === 0 && actionTypes.includes('check')) {
            explainText = '✅ 无人加注，你可以过牌或加注';
        } else if (toCall > 0) {
            if (toCall >= playerStack) {
                explainText = `⚡ 对方下注 $${toCall}，你只能全下或弃牌`;
            } else {
                explainText = `💰 需要跟注 $${toCall} 或选择加注/弃牌`;
            }
        }

        // Specific case: BB option pre-flop
        if (game.phase === 'preflop' && game.bbIndex === 0 && game.bbNeedsOption && toCall === 0) {
            explainText = '🔄 你在大盲位，可以选择过牌或加注';
        }
    }

    // Set explanation text
    explainEl.textContent = explainText;
}

function updateProbability() {
    const context = getAnalysisContext();
    const result = context?.probabilityResult;
    if (!result) {
        for (const id of ['probWinBar','probTieBar','probLoseBar']) {
            const bar = document.getElementById(id);
            if (bar) bar.style.width = '0%';
        }
        for (const id of ['probWinValue','probTieValue','probLoseValue']) {
            const value = document.getElementById(id);
            if (value) value.textContent = context?.playerCards?.length === 2 ? '计算中' : '0%';
        }
        return;
    }
    const winPct = (result.winProb * 100).toFixed(1);
    const tiePct = (result.tieProb * 100).toFixed(1);
    const losePct = (result.loseProb * 100).toFixed(1);

    document.getElementById('probWinBar').style.width = (result.winProb * 100) + '%';
    document.getElementById('probWinValue').textContent = winPct + '%';
    document.getElementById('probTieBar').style.width = (result.tieProb * 100) + '%';
    document.getElementById('probTieValue').textContent = tiePct + '%';
    document.getElementById('probLoseBar').style.width = (result.loseProb * 100) + '%';
    document.getElementById('probLoseValue').textContent = losePct + '%';
}

function updateHandAnalysis() {
    const context = getAnalysisContext();
    if (!context || context.folded || context.phase === 'idle') {
        document.getElementById('currentHandName').textContent = '—';
        document.getElementById('handBeatsContainer').innerHTML = '';
        return;
    }

    const allCards = [...context.playerCards, ...context.communityCards];
    if (allCards.length < 5) {
        document.getElementById('currentHandName').textContent =
            allCards.length + ' 张牌 (还需 ' + (5 - allCards.length) + ' 张公共牌)';
        document.getElementById('handBeatsContainer').innerHTML = '';
        return;
    }

    const hand = PokerProbabilityCore.evaluate(allCards, context.isShortDeck);
    if (!hand) return;

    // Current hand
    document.getElementById('currentHandName').textContent = hand.name;

    // What it beats
    const beatsContainer = document.getElementById('handBeatsContainer');
    beatsContainer.innerHTML = '';

    // Determine hand type names based on game mode
    const typeNames = context.isShortDeck ? SD_HAND_TYPE_NAMES : HAND_TYPE_NAMES;
    const allHandTypes = Object.values(typeNames);
    const currentRank = hand.rank;

    for (let r = 0; r < allHandTypes.length; r++) {
        const chip = document.createElement('span');
        chip.className = r < currentRank ? 'chip' : 'chip beaten-by';
        chip.textContent = allHandTypes[r];
        beatsContainer.appendChild(chip);
    }
}

function updateOuts() {
    const outsSection = document.getElementById('outsSection');
    const outsList = document.getElementById('outsList');
    const context = getAnalysisContext();

    if (!context || context.folded || context.communityCards.length < 3 || context.communityCards.length >= 5) {
        outsSection.style.display = 'none';
        return;
    }

    const outs = context.outsResult;
    if (!outs || !outs.byHandType) {
        outsSection.style.display = 'block';
        outsList.innerHTML = '<span style="color:#888;font-size:11px;">正在计算改善牌...</span>';
        return;
    }

    outsSection.style.display = 'block';
    outsList.innerHTML = '';

    const suitSymbols = { spades: '♠', hearts: '♥', diamonds: '♦', clubs: '♣' };

    // Show improvements
    const byType = outs.byHandType;
    const typeEntries = Object.entries(byType);
    const hasBetter = typeEntries.some(([name, data]) => data.count > 0);

    if (!hasBetter) {
        outsList.innerHTML = '<span style="color:#888;font-size:11px;">暂无直接补牌</span>';
        return;
    }

    // Sort by hand rank (highest improvement first)
    const outTypeNames = context.isShortDeck ? SD_HAND_TYPE_NAMES : HAND_TYPE_NAMES;
    const rankOrder = Object.values(outTypeNames);
    typeEntries.sort((a, b) => rankOrder.indexOf(b[0]) - rankOrder.indexOf(a[0]));

    for (const [handName, data] of typeEntries.slice(0, 5)) {
        if (data.cards.length === 0) continue;
        const group = document.createElement('div');
        group.style.cssText = 'font-size:11px;color:#aaa;margin-bottom:4px;';
        const quality = data.strongCount > 0
            ? `<span style="color:#79d89a;">强提升 ${data.strongCount}</span>`
            : `<span style="color:#aaa;">踢脚改善 ${data.thinCount}</span>`;
        group.innerHTML = `<span style="color:#ffd700;">${handName}</span>: ${data.count} 张 · ${quality}`;

        const cardsContainer = document.createElement('div');
        cardsContainer.style.cssText = 'display:flex;flex-wrap:wrap;gap:2px;margin-top:2px;';

        const suitSymbolsMap = { spades: '♠', hearts: '♥', diamonds: '♦', clubs: '♣' };

        for (const card of data.cards.slice(0, 8)) {
            const chip = document.createElement('span');
            chip.className = 'out-chip';
            const isRed = card.suit === 'hearts' || card.suit === 'diamonds';
            chip.style.color = isRed ? '#ef5350' : '#fff';
            chip.textContent = card.rank + suitSymbolsMap[card.suit];
            cardsContainer.appendChild(chip);
        }

        if (data.cards.length > 8) {
            const more = document.createElement('span');
            more.className = 'out-chip';
            more.textContent = '+' + (data.cards.length - 8) + ' 张';
            cardsContainer.appendChild(more);
        }

        group.appendChild(cardsContainer);
        outsList.appendChild(group);
    }
    if (outs.note) {
        const note = document.createElement('div');
        note.style.cssText = 'font-size:10px;color:#777;line-height:1.35;margin-top:5px;';
        note.textContent = outs.note;
        outsList.appendChild(note);
    }
}

function updateGameStatus() {
    const status = document.getElementById('gameStatus');

    if (game.phase === 'idle') {
        if (game.numHands > 0) {
            status.innerHTML = '手牌结束！点击 <span class="highlight">下一手</span> 继续';
        } else {
            status.innerHTML = '点击 <span class="highlight">开始游戏</span> 开始！';
        }
        return;
    }

    const phaseLabels = { preflop: '翻牌前', flop: '翻牌', turn: '转牌', river: '河牌' };
    const phaseLabel = phaseLabels[game.phase] || game.phase;

    // Count active players
    const active = game.getPlayersInHand().length;
    const folded = game.players.filter(p => p.folded && p.stack > 0).length;

    if (game.isPlayerTurn()) {
        status.innerHTML = `📌 轮到你了 — ${phaseLabel} · ${active} 人在局`;
    } else if (game.currentPlayer && !game.currentPlayer.isHuman) {
        status.innerHTML = `🤔 ${game.currentPlayer.name} 思考中... ${phaseLabel} · ${active} 人在局`;
    } else {
        status.innerHTML = `${phaseLabel} · ${active} 人在局 · ${folded} 人弃牌`;
    }
}

function updatePot() {
    document.getElementById('potDisplay').textContent = '底池: $' + game.pot.toLocaleString();
}

function updateBettingRoundLabel() {
    const labels = { preflop: '翻牌前', flop: '翻牌 ♠', turn: '转牌 ♥', river: '河牌 ♦' };
    const el = document.getElementById('bettingRoundLabel');
    if (game.phase !== 'idle') {
        el.textContent = labels[game.phase] || '';
    } else {
        el.textContent = '';
    }
}

// ============================================================
// Player Actions
// ============================================================

function doAction(type) {
    if (playMode === 'online') {
        onlineDoAction(type);
        return;
    }

    if (!game.isPlayerTurn()) return;

    const toCall = Math.max(0, game.currentBet - (game.roundBets[0] || 0));
    const actionLabels = {
        'fold': '弃牌',
        'check': '过牌',
        'call': '跟注',
        'raise': '加注',
        'allin': '全下'
    };

    switch (type) {
        case 'fold':
            showFloatingAction(`你: ${actionLabels.fold}`, 'fold', true);
            game.playerAction('fold', 0);
            break;
        case 'check':
            showFloatingAction(`你: ${actionLabels.check}`, 'check', true);
            game.playerAction('check', 0);
            break;
        case 'call':
            showFloatingAction(`你: ${actionLabels.call} $${toCall}`, 'call', true);
            game.playerAction('call', toCall);
            break;
        case 'allin':
            showFloatingAction(`你: ${actionLabels.allin} $${game.humanPlayer.stack}`, 'allin', true);
            game.playerAction('allin', game.humanPlayer.stack);
            break;
    }
}

let raiseSliderVisible = false;

function showRaiseSlider() {
    const container = document.getElementById('raiseSlider');

    let minRaise, maxRaise, bigBlind;

    if (playMode === 'online' && network.gameState) {
        const gs = network.gameState;
        const legal = gs.legalActions || {};
        if (!gs.isYourTurn || network.pendingAction || !(legal.actions || []).includes('raise')) return;
        const config = gs.roomConfig || gs.config || gs;
        bigBlind = Number(gs.isShortDeck ? (config.minBet || gs.minBet) : (config.bigBlind || gs.bigBlind)) || selectedBigBlind;
        minRaise = Math.max(0, Number(legal.minRaiseTo) || 0);
        maxRaise = Math.max(0, Number(legal.maxRaiseTo) || 0);
    } else {
        if (!game || !game.isPlayerTurn()) return;
        const raiseAction = game.getAvailableActions().find(action => action.type === 'raise');
        if (!raiseAction) return;
        bigBlind = game instanceof ShortDeckGame ? game.minBet : game.bigBlind;
        minRaise = raiseAction.amount;
        maxRaise = (game.roundBets[0] || 0) + (game.humanPlayer ? game.humanPlayer.stack : 0);
    }

    if (!Number.isFinite(minRaise) || !Number.isFinite(maxRaise) || minRaise > maxRaise) return;

    const slider = document.getElementById('raiseRange');
    slider.min = minRaise;
    slider.max = maxRaise;
    slider.value = Math.min(minRaise + bigBlind * 2, maxRaise);
    slider.step = Math.max(1, Math.floor(bigBlind / 2));

    container.classList.add('show');
    raiseSliderVisible = true;
    updateRaiseDisplay();
}

function hideRaiseSlider() {
    document.getElementById('raiseSlider').classList.remove('show');
    raiseSliderVisible = false;
}

function updateRaiseDisplay() {
    const slider = document.getElementById('raiseRange');
    const display = document.getElementById('raiseAmountDisplay');
    display.textContent = '加注 $' + parseInt(slider.value);
}

function confirmRaise() {
    const amount = parseInt(document.getElementById('raiseRange').value);
    hideRaiseSlider();
    if (playMode === 'online') {
        const legal = network.gameState && network.gameState.legalActions;
        if (!legal || !(legal.actions || []).includes('raise')) return;
        const boundedAmount = Math.max(legal.minRaiseTo, Math.min(legal.maxRaiseTo, amount));
        if (!sendAction('raise', boundedAmount)) return;
        ['btnFold','btnCheck','btnCall','btnRaise','btnAllin'].forEach(id => {
            document.getElementById(id).disabled = true;
        });
        document.getElementById('actionExplain').textContent = '⏳ 等待服务器确认...';
    } else {
        showFloatingAction(`你: 加注 $${amount}`, 'raise', true);
        game.playerAction('raise', amount);
    }
}

// ============================================================
// Showdown Modal
// ============================================================

function showShowdown(result) {
    const modal = document.getElementById('showdownModal');
    const content = document.getElementById('modalContent');
    // Reset modal position
    content.style.position = '';
    content.style.left = '';
    content.style.top = '';
    content.style.margin = '0 auto';
    content.style.transform = 'none';
    document.getElementById('modalTitle').textContent = '🏆 摊牌';

    if (result.winners && result.winners.length > 0) {
        const winnerAmounts = result.winnerAmounts;
        let amountDisplay;

        if (winnerAmounts && result.winners.length > 1) {
            // Multiple winners — show per-winner breakdown
            const parts = result.winners.map(w => {
                const name = w.isHuman ? '你' : w.name;
                const amt = winnerAmounts.get ? (winnerAmounts.get(w) || 0) : result.share;
                return `${name} $${amt}`;
            });
            amountDisplay = parts.join(' + ');
        } else {
            // Single winner — show total
            const winner = result.winners[0];
            const amt = winnerAmounts?.get ? (winnerAmounts.get(winner) || result.pot) : result.pot;
            amountDisplay = `$${amt}`;
        }

        const winnerNames = result.winners.map(w => w.isHuman ? '你' : w.name).join(', ');
        document.getElementById('winnerName').textContent = winnerNames + (result.winners.length > 1 ? '' : '');
        document.getElementById('winnerAmount').textContent = amountDisplay;

        if (result.handName) {
            document.getElementById('winningHandName').textContent = result.handName;
            document.getElementById('winningHandName').style.display = '';
        } else {
            document.getElementById('winningHandName').style.display = 'none';
        }
    }

    // ── Show Community Cards ──
    const resultsDiv = document.getElementById('modalResults');
    resultsDiv.innerHTML = '';

    if (game.communityCards && game.communityCards.length > 0) {
        const communitySection = document.createElement('div');
        communitySection.style.cssText = 'text-align:center;margin-bottom:12px;padding:8px 10px;background:rgba(0,0,0,0.25);border-radius:10px;';
        const communityLabel = document.createElement('div');
        communityLabel.style.cssText = 'font-size:11px;color:#888;margin-bottom:6px;';
        communityLabel.textContent = '🃏 公共牌';
        communitySection.appendChild(communityLabel);

        const cardsContainer = document.createElement('div');
        cardsContainer.style.cssText = 'display:flex;justify-content:center;gap:4px;';
        const suitSymbols = { spades: '♠', hearts: '♥', diamonds: '♦', clubs: '♣' };
        for (const card of game.communityCards) {
            const isRed = card.suit === 'hearts' || card.suit === 'diamonds';
            const cardEl = document.createElement('span');
            cardEl.style.cssText = `display:inline-flex;align-items:center;gap:2px;padding:3px 8px;border-radius:5px;background:rgba(255,255,255,0.12);font-size:14px;font-weight:600;${isRed ? 'color:#ef5350;' : 'color:#fff;'}`;
            cardEl.innerHTML = `${card.rank} ${suitSymbols[card.suit]}`;
            cardsContainer.appendChild(cardEl);
        }
        communitySection.appendChild(cardsContainer);
        resultsDiv.appendChild(communitySection);
    }

    if (result.results) {
        for (const r of result.results) {
            const row = document.createElement('div');
            row.className = 'modal-result-row';
            const isWinner = result.winners && result.winners.includes(r.player);
            const name = r.player.isHuman ? 'You' : r.player.name;

            // Show cards as chips (same style as community cards)
            const cardChipStyle = 'display:inline-flex;align-items:center;gap:2px;padding:3px 8px;border-radius:5px;background:rgba(255,255,255,0.12);font-size:14px;font-weight:600;';
            const suitSymbols = { spades: '♠', hearts: '♥', diamonds: '♦', clubs: '♣' };

            // Name part
            const nameSpan = document.createElement('span');
            if (isWinner) nameSpan.textContent = '🏆 ';
            const strong = document.createElement('strong');
            strong.textContent = name;
            nameSpan.appendChild(strong);

            // Cards part
            const cardsContainer = document.createElement('span');
            cardsContainer.style.cssText = 'display:inline-flex;gap:4px;';
            for (const c of r.player.holeCards) {
                const isRed = c.suit === 'hearts' || c.suit === 'diamonds';
                const chip = document.createElement('span');
                chip.style.cssText = cardChipStyle + (isRed ? 'color:#ef5350;' : 'color:#fff;');
                chip.textContent = c.rank + ' ' + suitSymbols[c.suit];
                cardsContainer.appendChild(chip);
            }

            // Hand name part
            const handSpan = document.createElement('span');
            handSpan.className = 'hand-name';
            handSpan.textContent = r.hand ? r.hand.name : '—';

            row.appendChild(nameSpan);
            row.appendChild(cardsContainer);
            row.appendChild(handSpan);
            if (isWinner) row.style.background = 'rgba(76,175,80,0.15)';

            resultsDiv.appendChild(row);
        }
    }

    modal.classList.add('show');
}

function closeModalAndContinue() {
    if (playMode === 'online') {
        // 多人桌等待其他真人；单人桌只等待服务端开始下一手。
        sendReadyForNext();
        const btn = document.querySelector('#showdownModal .modal-btn');
        if (btn) {
            const connectedHumans = (network.gameState?.players || []).filter(player =>
                player.isHuman && player.connected && player.stack > 0).length;
            btn.textContent = connectedHumans <= 1 ? '⏳ 正在开始下一手...' : '⏳ 等待其他玩家...';
            btn.disabled = true;
            btn.style.opacity = '0.6';
            btn.style.cursor = 'not-allowed';
        }
        return;
    }
    document.getElementById('showdownModal').classList.remove('show');
    setTimeout(() => {
        if (!game || !game.humanPlayer) return;
        if (game.humanPlayer.stack <= 0) {
            goToStartScreen('busted');
        } else {
            game.startNewHand();
        }
    }, 300);
}

/** Return to start/main screen */
function goToStartScreen(reason) {
    const startScreen = document.getElementById('startScreen');
    const bottomPanel = document.getElementById('bottomPanel');
    const tableContainer = document.getElementById('tableContainer');
    resetHumanCardsVisibility();
    resetCommunityCardAnimation();

    startScreen.style.display = 'flex';
    bottomPanel.style.display = 'none';

    // Reset the start screen
    updatePlayerCountOptions();
    updateStartScreenMode();
    document.getElementById('playerCountSelector').style.display = 'block';
    document.getElementById('startGameBtn').textContent = '🏆 开始游戏';

    const bustedInfo = document.getElementById('bustedInfo');
    if (reason === 'busted') {
        bustedInfo.style.display = 'block';
    } else {
        bustedInfo.style.display = 'none';
    }

    // Clear the table — also hide community area
    const communityArea = document.getElementById('communityArea');
    if (communityArea) {
        communityArea.innerHTML = `
            <div class="community-placeholder">翻牌</div>
            <div class="community-placeholder">翻牌</div>
            <div class="community-placeholder">翻牌</div>
            <div class="community-placeholder">转牌</div>
            <div class="community-placeholder">河牌</div>
        `;
        communityArea.style.display = 'none';
    }
    const potDisplay = document.getElementById('potDisplay');
    potDisplay.textContent = '底池: $0';
    potDisplay.style.display = 'none';
    document.getElementById('aiPlayersContainer').innerHTML = '';
    document.getElementById('gameStatus').textContent = gameMode === 'shortdeck'
        ? '点击「开始游戏」开始短牌对局！'
        : '点击「开始游戏」开始德州扑克对局！';
    document.getElementById('bettingRoundLabel').textContent = '';
}

/** Start a new game with selected player count and mode */
function startNewGame() {
    const selectedBtn = document.querySelector('.count-btn.selected');
    const totalPlayers = parseInt(selectedBtn ? selectedBtn.dataset.count : 4);
    const aiCount = totalPlayers - 1;

    // Create the right game based on mode
    if (gameMode === 'shortdeck') {
        game = new ShortDeckGame({
            startingStack: selectedStartingStack,
            ante: selectedSmallBlind,
            minBet: selectedBigBlind
        });
    } else {
        game = new PokerGame({
            startingStack: selectedStartingStack,
            smallBlind: selectedSmallBlind,
            bigBlind: selectedBigBlind
        });
    }
    setupGameCallbacks(game);
    setupMusicAutoStart(game);
    game.aiDelay = selectedAiDelay;
    game.init(aiCount);

    // Update UI for mode
    updateModeUI();

    const communityArea = document.getElementById('communityArea');
    if (communityArea) communityArea.style.display = '';
    const potDisplay = document.getElementById('potDisplay');
    if (potDisplay) potDisplay.style.display = '';

    document.getElementById('startScreen').style.display = 'none';
    document.getElementById('bottomPanel').style.display = 'flex';

    game.startNewHand();
}

/** Update UI elements based on game mode */
function updateModeUI() {
    const isSD = gameMode === 'shortdeck';
    const status = document.getElementById('gameStatus');

    // Update header title
    const title = document.querySelector('.header h1 span');
    if (title) {
        title.textContent = isSD ? 'Short Deck' : 'Texas';
    }

    // Update blinds info display
    const blindsInfo = document.querySelector('.blinds-info');
    if (blindsInfo) {
        if (isSD && game) {
            blindsInfo.innerHTML = `
                <span>Ante: <span id="sbDisplay">${game.ante}</span></span>
                <span>最小下注: <span id="bbDisplay">${game.minBet}</span></span>
            `;
        } else {
            blindsInfo.innerHTML = `
                <span>SB: <span id="sbDisplay">${game ? game.smallBlind : selectedSmallBlind}</span></span>
                <span>BB: <span id="bbDisplay">${game ? game.bigBlind : selectedBigBlind}</span></span>
            `;
        }
    }
    syncGameSettingsUI();
}

// ============================================================
// Modal Drag Functionality — fixed with getBoundingClientRect
// ============================================================

let dragState = null;

document.addEventListener('mousedown', function(e) {
    const handle = e.target.closest('.modal-drag-handle');
    if (!handle) return;
    const content = document.getElementById('modalContent');
    if (!content) return;

    const rect = content.getBoundingClientRect();

    dragState = {
        content,
        startX: e.clientX,
        startY: e.clientY,
        origLeft: rect.left,
        origTop: rect.top
    };

    content.style.position = 'fixed';
    content.style.cursor = 'grabbing';
    content.style.margin = '0';
    content.style.left = rect.left + 'px';
    content.style.top = rect.top + 'px';
    e.preventDefault();
});

document.addEventListener('mousemove', function(e) {
    if (!dragState) return;
    const dx = e.clientX - dragState.startX;
    const dy = e.clientY - dragState.startY;
    dragState.content.style.left = (dragState.origLeft + dx) + 'px';
    dragState.content.style.top = (dragState.origTop + dy) + 'px';
    dragState.content.style.margin = '0';
    dragState.content.style.transform = 'none';
});

document.addEventListener('mouseup', function() {
    if (dragState) {
        dragState.content.style.cursor = '';
        dragState = null;
    }
});

// Handle player count selection
document.addEventListener('click', function(e) {
    const btn = e.target.closest('.count-btn');
    if (!btn) return;
    document.querySelectorAll('.count-btn').forEach(b => b.classList.remove('selected'));
    btn.classList.add('selected');
});

// Also handle touch events for mobile
document.addEventListener('touchstart', function(e) {
    const handle = e.target.closest('.modal-drag-handle');
    if (!handle) return;
    const content = document.getElementById('modalContent');
    if (!content) return;
    const touch = e.touches[0];
    const rect = content.getBoundingClientRect();
    dragState = {
        content,
        startX: touch.clientX,
        startY: touch.clientY,
        origLeft: rect.left,
        origTop: rect.top
    };
    content.style.position = 'fixed';
    content.style.margin = '0';
    content.style.left = rect.left + 'px';
    content.style.top = rect.top + 'px';
    content.style.cursor = 'grabbing';
}, { passive: false });

document.addEventListener('touchmove', function(e) {
    if (!dragState) return;
    const touch = e.touches[0];
    const dx = touch.clientX - dragState.startX;
    const dy = touch.clientY - dragState.startY;
    dragState.content.style.left = (dragState.origLeft + dx) + 'px';
    dragState.content.style.top = (dragState.origTop + dy) + 'px';
    dragState.content.style.margin = '0';
    dragState.content.style.transform = 'none';
    e.preventDefault();
}, { passive: false });

document.addEventListener('touchend', function() {
    if (dragState) {
        dragState.content.style.cursor = '';
        dragState = null;
    }
});

function showMessage(title, message) {
    const modal = document.getElementById('showdownModal');
    const content = document.getElementById('modalContent');
    content.style.left = '';
    content.style.top = '';
    content.style.margin = '0 auto';
    content.style.transform = 'none';
    document.getElementById('modalTitle').textContent = title;
    document.getElementById('winnerName').textContent = '';
    document.getElementById('winnerAmount').textContent = '';
    document.getElementById('winningHandName').style.display = 'none';
    document.getElementById('modalResults').innerHTML = '';

    const msgDiv = document.createElement('div');
    msgDiv.style.cssText = 'text-align:center;font-size:16px;padding:15px;color:#aaa;';
    msgDiv.textContent = message;
    document.getElementById('modalResults').appendChild(msgDiv);

    modal.classList.add('show');
}

function showAIMessage(name, message) {
    const modal = document.getElementById('showdownModal');
    const content = document.getElementById('modalContent');
    content.style.left = '';
    content.style.top = '';
    content.style.margin = '0 auto';
    content.style.transform = 'none';
    document.getElementById('modalTitle').textContent = '🤖 ' + name;
    document.getElementById('winnerName').textContent = '';
    document.getElementById('winnerAmount').textContent = '';
    document.getElementById('winningHandName').style.display = 'none';
    document.getElementById('modalResults').innerHTML = '';

    const msgDiv = document.createElement('div');
    msgDiv.style.cssText = 'text-align:center;font-size:16px;padding:15px;color:#ffd700;';
    msgDiv.textContent = message;
    document.getElementById('modalResults').appendChild(msgDiv);

    modal.classList.add('show');
}

// ============================================================
// Fullscreen Toggle
// ============================================================

/** Toggle fullscreen mode */
function toggleFullscreen() {
    const btn = document.getElementById('fullscreenBtn');
    if (!document.fullscreenElement && !document.webkitFullscreenElement) {
        const el = document.documentElement;
        if (el.requestFullscreen) {
            el.requestFullscreen().catch(() => {});
        } else if (el.webkitRequestFullscreen) {
            el.webkitRequestFullscreen();
        }
        btn.textContent = '⛶ 退出全屏';
        btn.title = '退出全屏';
    } else {
        if (document.exitFullscreen) {
            document.exitFullscreen().catch(() => {});
        } else if (document.webkitExitFullscreen) {
            document.webkitExitFullscreen();
        }
        btn.textContent = '⛶ 全屏';
        btn.title = '全屏';
    }
}

/** Update fullscreen button state when it changes externally (ESC key, etc.) */
function updateFullscreenBtn() {
    const btn = document.getElementById('fullscreenBtn');
    if (!btn) return;
    const isFs = !!(document.fullscreenElement || document.webkitFullscreenElement);
    btn.textContent = isFs ? '⛶ 退出全屏' : '⛶ 全屏';
    btn.title = isFs ? '退出全屏' : '全屏';
}

// Listen for fullscreen change events (covers ESC key exit)
document.addEventListener('fullscreenchange', updateFullscreenBtn);
document.addEventListener('webkitfullscreenchange', updateFullscreenBtn);

function minimizeAppWindow() {
    window.windowControls?.minimize();
}

async function toggleAppWindowMaximize() {
    if (!window.windowControls) return;
    const maximized = await window.windowControls.toggleMaximize();
    const button = document.getElementById('windowMaximizeBtn');
    if (button) {
        button.textContent = maximized ? '❐' : '□';
        button.title = maximized ? '还原' : '最大化';
        button.setAttribute('aria-label', button.title);
    }
}

function closeAppWindow() {
    window.windowControls?.close();
}

// ============================================================
// Return to Home Screen
// ============================================================

/** Return to the main menu / start screen */
function goHome() {
    if (!game) return;

    // If a hand is in progress, show brief notification
    if (game.phase !== 'idle' && game.getPlayersInHand().length > 0) {
        showDebugToast('返回主页，当前对局已关闭');
    }

    // Close any open modals
    document.getElementById('showdownModal').classList.remove('show');
    hideRaiseSlider();

    // 联网模式：离开房间，回到联网大厅
    if (playMode === 'online') {
        leaveRoom();
        inRoom = false;
        isRoomHost = false;
        resetOnlinePresentationState();
        document.getElementById('createRoomForm').style.display = 'block';
        document.getElementById('joinRoomForm').style.display = 'block';
        document.getElementById('roomActions').style.display = 'none';
        document.getElementById('startGameBtnOnline').style.display = 'none';
        showDebugToast('已离开房间');
    }

    game.resetGame();
    gameStarted = false;
    goToStartScreen();
}

/** 刷新/重新连接 — 不离开当前页面，只重置网络或牌局 */
function doRefresh() {
    if (playMode === 'online') {
        showDebugToast('🔄 正在重新连接...');
        const url = document.getElementById('serverAddress').value.trim();
        if (url) {
            reconnectToServer(url);
        } else {
            showDebugToast('⚠️ 请先输入服务器地址');
        }
    } else if (game) {
        // 本地模式：重置牌局
        showDebugToast('🔄 重新发牌');
        game.resetGame();
        gameStarted = false;
        startNewGame();
    }
}

/** Show a brief floating notification */
function showDebugToast(msg) {
    var t = document.createElement('div');
    t.textContent = msg;
    t.style.cssText = 'position:fixed;bottom:60px;right:20px;z-index:100000;background:rgba(0,0,0,0.85);color:#4caf50;padding:8px 16px;border-radius:6px;font-family:monospace;font-size:12px;transition:opacity 0.5s;';
    document.body.appendChild(t);
    setTimeout(function () { t.style.opacity = '0'; setTimeout(function () { t.remove(); }, 500); }, 2000);
}

// ============================================================
// Keyboard Shortcuts
// ============================================================

document.addEventListener('keydown', function(e) {
    const isOnlineTurn = playMode === 'online' && network.gameState && network.gameState.isYourTurn && !network.pendingAction;
    const isLocalTurn = playMode === 'local' && game && game.isPlayerTurn();
    if (!isOnlineTurn && !isLocalTurn) return;

    switch (e.key.toLowerCase()) {
        case 'f':
            if (!document.getElementById('btnFold').disabled) doAction('fold');
            break;
        case 'c':
            if (!document.getElementById('btnCheck').disabled) doAction('check');
            else if (!document.getElementById('btnCall').disabled) doAction('call');
            break;
        case 'r':
            if (!document.getElementById('btnRaise').disabled) showRaiseSlider();
            break;
        case 'a':
            if (!document.getElementById('btnAllin').disabled) doAction('allin');
            break;
        case 'enter':
            if (raiseSliderVisible) confirmRaise();
            break;
        case 'escape':
            if (raiseSliderVisible) {
                hideRaiseSlider();
            } else if (document.fullscreenElement || document.webkitFullscreenElement) {
                toggleFullscreen();
            }
            break;
    }
});

// ============================================================
// Keyboard Shortcuts Display
// ============================================================

document.addEventListener('DOMContentLoaded', function() {
    document.documentElement.classList.toggle('electron-shell', !!window.windowControls);
    loadAppSettings();
    restoreSavedInputs();
    syncLocalServerEndpoint();

    // Add keyboard hint
    const hints = document.createElement('div');
    hints.style.cssText = 'text-align:center;font-size:11px;color:#555;margin-top:8px;';
    hints.innerHTML = '快捷键: <kbd>F</kbd> 弃牌 <kbd>C</kbd> 过牌/跟注 <kbd>R</kbd> 加注 <kbd>A</kbd> All-in <kbd>Enter</kbd> 确认 <kbd>Esc</kbd> 取消';
    document.getElementById('app').appendChild(hints);

    document.querySelectorAll('.mode-btn').forEach(button => {
        button.classList.toggle('selected', button.dataset.mode === gameMode);
    });
    const roomMode = document.getElementById('roomShortDeck');
    if (roomMode) roomMode.checked = gameMode === 'shortdeck';
    updateStartScreenMode();

    game = gameMode === 'shortdeck'
        ? new ShortDeckGame({ startingStack:selectedStartingStack, ante:selectedSmallBlind, minBet:selectedBigBlind })
        : new PokerGame({ startingStack:selectedStartingStack, smallBlind:selectedSmallBlind, bigBlind:selectedBigBlind });
    setupGameCallbacks(game);
    setupMusicAutoStart(game);
    game.aiDelay = selectedAiDelay;
    game.init(gameMode === 'shortdeck' ? 5 : 9);
    // Ensure player count options reflect initial mode
    updatePlayerCountOptions();
    syncGameSettingsUI();
    const volumeSlider = document.getElementById('volSlider');
    if (volumeSlider) volumeSlider.value = selectedMusicVolume;
    if (typeof bgMusic !== 'undefined') bgMusic.setVolume(musicVolumeFromSlider(selectedMusicVolume));
    if (typeof bgMusic !== 'undefined') bgMusic.setPlaybackMode(musicPlaybackMode);
    // Fetch the first local track while game settings are being chosen. Actual
    // playback still starts only after a user action/game start.
    if (typeof bgMusic !== 'undefined') bgMusic.init();
    updateMusicButton(false);
    updatePlaybackModeButton();

    ['serverAddress', 'playerNameInput'].forEach(id => {
        const input = document.getElementById(id);
        if (input) input.addEventListener('change', saveAppSettings);
    });

    // Hide community area on start screen (absolutely positioned — would show through otherwise)
    const ca = document.getElementById('communityArea');
    if (ca) ca.style.display = 'none';
    const pd = document.getElementById('potDisplay');
    if (pd) pd.style.display = 'none';
});

// ============================================================
// Rules Toggle
// ============================================================

function toggleRules() {
    const content = document.getElementById('rulesContent');
    const btn = document.getElementById('rulesToggle');
    content.classList.toggle('open');
    btn.textContent = content.classList.contains('open')
        ? '📜 收起规则说明'
        : '📜 查看德州扑克规则';
}

function toggleProbPanel() {
    if (humanCardsHidden && getCurrentHumanCards().length === 2) return;
    const content = document.getElementById('probContent');
    const icon = document.getElementById('probToggleIcon');
    content.classList.toggle('collapsed');
    icon.textContent = content.classList.contains('collapsed') ? '▼' : '▲';
}

// ============================================================
// Background Music Controls
// ============================================================

let musicStarted = false;

function musicVolumeFromSlider(value) {
    return Math.max(0, Math.min(.72, (Number(value) || 0) / 100 * .72));
}

function updateMusicButton(playing, state = playing ? 'playing' : 'off') {
    const btn = document.getElementById('musicBtn');
    if (!btn) return;
    const trackName = bgMusic && typeof bgMusic.getCurrentPatternName === 'function'
        ? bgMusic.getCurrentPatternName() : '牌桌音乐';
    if (state === 'loading') btn.textContent = `⏳ 正在加载 ${trackName}`;
    else if (state === 'error') btn.textContent = '⚠️ 音乐加载失败';
    else btn.textContent = playing ? `🎵 ${trackName}` : '🎵 音乐关';
    btn.title = state === 'loading' ? `正在准备：${trackName}`
        : playing ? `正在播放：${trackName}` : '播放背景音乐';
    btn.dataset.playbackState = state;
    btn.classList.toggle('music-on', playing);
}

document.addEventListener('poker-music-state', event => {
    const state = event.detail?.state || 'off';
    updateMusicButton(state !== 'off' && state !== 'error', state);
});

function toggleMusic() {
    try {
        if (!bgMusic.initialized) {
            bgMusic.init();
        }
        const playing = bgMusic.toggle();
        musicStarted = true;
        musicPreference = playing ? 'on' : 'off';
        updateMusicButton(playing, playing ? 'loading' : 'off');
        const slider = document.getElementById('volSlider');
        if (slider) slider.value = selectedMusicVolume;
        saveAppSettings();
    } catch (e) {
        console.log('Music toggle unavailable:', e);
    }
}

function setVolume(val) {
    selectedMusicVolume = Math.max(0, Math.min(100, parseInt(val) || 0));
    const v = musicVolumeFromSlider(selectedMusicVolume);
    bgMusic.setVolume(v);
    saveAppSettings();
}

function updatePlaybackModeButton() {
    const button = document.getElementById('playbackModeBtn');
    if (!button) return;
    const modes = {
        sequential:{ label:'🔁 顺序', title:'当前：顺序播放' },
        'repeat-one':{ label:'🔂 单曲', title:'当前：单曲循环' },
        shuffle:{ label:'🔀 随机', title:'当前：随机播放' }
    };
    const view = modes[musicPlaybackMode] || modes.sequential;
    button.textContent = view.label;
    button.title = `${view.title}；点击切换`;
    button.setAttribute('aria-label', button.title);
}

function cycleMusicPlaybackMode() {
    const modes = ['sequential', 'repeat-one', 'shuffle'];
    musicPlaybackMode = modes[(modes.indexOf(musicPlaybackMode) + 1) % modes.length];
    bgMusic.setPlaybackMode(musicPlaybackMode);
    updatePlaybackModeButton();
    saveAppSettings();
}

function skipSong() {
    try {
        if (!bgMusic.initialized || !bgMusic.ctx) {
            bgMusic.init();
        }
        musicStarted = true;
        musicPreference = 'on';
        bgMusic.next();
        updateMusicButton(true, 'loading');
        saveAppSettings();
    } catch (e) {
        console.log('Skip song unavailable:', e);
    }
}

// Auto-start music on first game start — hooked into startNewHand after game is created
function setupMusicAutoStart(g) {
    const orig = g.startNewHand.bind(g);
    g.startNewHand = function() {
        resetHumanCardsVisibility();
        resetCommunityCardAnimation('local');
        if (!musicStarted) {
            musicStarted = true;
            if (musicPreference === 'off') {
                updateMusicButton(false);
            } else {
                try {
                    bgMusic.init();
                    bgMusic.setVolume(musicVolumeFromSlider(selectedMusicVolume));
                    bgMusic.start();
                    updateMusicButton(true, 'loading');
                    const slider = document.getElementById('volSlider');
                    if (slider) slider.value = selectedMusicVolume;
                } catch (e) {
                    console.log('Audio unavailable (expected on file://):', e);
                }
            }
        }
        const result = orig();
        if (g.humanPlayer?.holeCards?.length === 2) animateHumanCardsDeal();
        return result;
    };
}

/** 轮到玩家行动时的提示音（简短叮声，仅当前玩家听到） */
var _turnAudioCtx = null;
function playTurnAlert() {
    try {
        if (!_turnAudioCtx) _turnAudioCtx = new (window.AudioContext || window.webkitAudioContext)();
        var ctx = _turnAudioCtx;
        var now = ctx.currentTime;
        // 两音叠加：800Hz + 1200Hz，模拟悦耳"叮"
        [800, 1200].forEach(function(freq) {
            var osc = ctx.createOscillator();
            var gain = ctx.createGain();
            osc.type = 'sine';
            osc.frequency.value = freq;
            gain.gain.setValueAtTime(0.08, now);
            gain.gain.exponentialRampToValueAtTime(0.001, now + 0.25);
            osc.connect(gain);
            gain.connect(ctx.destination);
            osc.start(now);
            osc.stop(now + 0.25);
        });
    } catch(e) { /* 静默失败，不影响游戏 */ }
}

// Expose to global for inline handlers
window.game = game;
window.doAction = doAction;
window.showRaiseSlider = showRaiseSlider;
window.hideRaiseSlider = hideRaiseSlider;
window.updateRaiseDisplay = updateRaiseDisplay;
window.confirmRaise = confirmRaise;
window.closeModalAndContinue = closeModalAndContinue;
window.showMessage = showMessage;
window.showAIMessage = showAIMessage;
window.cycleMusicPlaybackMode = cycleMusicPlaybackMode;
window.toggleMusic = toggleMusic;
window.toggleRules = toggleRules;
window.toggleProbPanel = toggleProbPanel;
window.setVolume = setVolume;
window.skipSong = skipSong;
window.toggleFullscreen = toggleFullscreen;
window.minimizeAppWindow = minimizeAppWindow;
window.toggleAppWindowMaximize = toggleAppWindowMaximize;
window.closeAppWindow = closeAppWindow;
window.goHome = goHome;
window.selectPlayMode = selectPlayMode;
window.doConnect = doConnect;
window.doDisconnect = doDisconnect;
window.doCreateRoom = doCreateRoom;
window.doJoinRoom = doJoinRoom;
window.doLeaveRoom = doLeaveRoom;
window.doStartGame = doStartGame;
window.doRefresh = doRefresh;
