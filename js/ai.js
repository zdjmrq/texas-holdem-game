/**
 * AI Player Logic v4 — Fast & Formidable
 * ========================================
 * Key fixes over v3:
 *   1. ONE Monte Carlo run per decision (was 5-7) — eliminates lag
 *   2. Uses existing ProbabilityCalculator for MC (optimized C-like paths)
 *   3. EV calculated from shared equity — fold equity varies per sizing
 *   4. Poker heuristics for common spots (commitment, traps, value extraction)
 *   5. Opponent ranges + exploitation still active but lightweight
 *
 * FAIR PLAY: AI sees only its cards + community cards. Range estimation
 * uses publicly observable actions only.
 */

// ── Style definitions ──
const AI_STYLES = { TAG:'TAG', LAG:'LAG', SOLID:'SOLID', MANIAC:'MANIAC', TAGFISH:'TAGFISH', CALLING:'CALLING_STATION' };
const AI_NAMES = ['Alex','Blake','Casey','Drew','Emma','Finn','Grace','Hayes','Ivy','Jade'];
const AI_AVATARS = ['😎','🤠','🕶️','🎩','👑','🦊','🐺','🦅','🐯','🐉'];
const POS = { EP:'EP', MP:'MP', LP:'LP', BL:'BL' };

const STYLE_CONFIGS = {
    [AI_STYLES.TAG]:   { vpip:0.20,pfr:0.85,aggression:0.60,threeBetFreq:0.06,bluffFreq:0.08,foldTo3Bet:0.55,cbFreq:0.60,doubleBarrel:0.45,floatFreq:0.15,checkRaiseFreq:0.08,heroCallFreq:0.10,description:'TAG' },
    [AI_STYLES.LAG]:   { vpip:0.30,pfr:0.80,aggression:0.65,threeBetFreq:0.12,bluffFreq:0.14,foldTo3Bet:0.40,cbFreq:0.70,doubleBarrel:0.55,floatFreq:0.25,checkRaiseFreq:0.12,heroCallFreq:0.20,description:'LAG' },
    [AI_STYLES.SOLID]: { vpip:0.24,pfr:0.75,aggression:0.55,threeBetFreq:0.08,bluffFreq:0.10,foldTo3Bet:0.48,cbFreq:0.55,doubleBarrel:0.48,floatFreq:0.18,checkRaiseFreq:0.10,heroCallFreq:0.15,description:'Solid' },
    [AI_STYLES.MANIAC]:{ vpip:0.42,pfr:0.70,aggression:0.75,threeBetFreq:0.18,bluffFreq:0.22,foldTo3Bet:0.28,cbFreq:0.72,doubleBarrel:0.60,floatFreq:0.30,checkRaiseFreq:0.18,heroCallFreq:0.25,description:'Maniac' },
    [AI_STYLES.TAGFISH]:{ vpip:0.18,pfr:0.55,aggression:0.35,threeBetFreq:0.03,bluffFreq:0.04,foldTo3Bet:0.50,cbFreq:0.40,doubleBarrel:0.25,floatFreq:0.08,checkRaiseFreq:0.04,heroCallFreq:0.12,description:'Tight-Passive' },
    [AI_STYLES.CALLING]:{ vpip:0.40,pfr:0.15,aggression:0.22,threeBetFreq:0.02,bluffFreq:0.03,foldTo3Bet:0.15,cbFreq:0.25,doubleBarrel:0.12,floatFreq:0.05,checkRaiseFreq:0.03,heroCallFreq:0.30,description:'Calling Station' }
};

// ═══════════════════════════════════════════════════════════════
// A. OPPONENT RANGE ESTIMATION (lightweight)
// ═══════════════════════════════════════════════════════════════

const RANGE_TEMPLATES = {
    EP_OPEN: {
        pairs:{min:7,all:true,w:1.0}, pairs_low:{min:2,max:6,w:0.25},
        sA:{min:10,all:true,w:0.9}, sA_low:{min:2,max:9,w:0.15},
        sBW:{all:true,w:0.85}, sC:{min:9,gap:1,w:0.4},
        oBW_strong:{all:true,w:0.9}, oBW:{min:11,w:0.5}, sK:{min:10,w:0.25}
    },
    MP_OPEN: {
        pairs:{min:5,all:true,w:1.0}, pairs_low:{min:2,max:4,w:0.5},
        sA:{min:8,all:true,w:0.9}, sA_low:{min:2,max:7,w:0.3},
        sBW:{all:true,w:0.9}, sC:{min:8,gap:1,w:0.6}, sC_wide:{min:9,gap:2,w:0.3},
        oBW_strong:{all:true,w:1.0}, oBW:{min:10,w:0.6}, sK:{min:8,w:0.35}, oA:{min:10,w:0.3}
    },
    CO_OPEN: {
        pairs:{min:2,all:true,w:1.0}, sA:{min:2,all:true,w:0.85},
        sBW:{all:true,w:0.95}, sC:{min:5,gap:1,w:0.7}, sC_wide:{min:7,gap:2,w:0.45},
        sK:{min:6,w:0.4}, sQ:{min:8,w:0.3},
        oBW_strong:{all:true,w:1.0}, oBW:{min:9,w:0.65}, oA:{min:8,w:0.4}
    },
    BTN_OPEN: {
        pairs:{min:2,all:true,w:1.0}, sA:{min:2,all:true,w:0.9},
        sBW:{all:true,w:1.0}, sC:{min:4,gap:1,w:0.8}, sC_wide:{min:6,gap:2,w:0.55},
        s1gap:{min:8,gap:1,w:0.5}, sK:{min:2,w:0.45}, sQ:{min:5,w:0.3}, sAny:{min:10,w:0.2},
        oBW_strong:{all:true,w:1.0}, oBW:{min:8,w:0.7}, oA:{min:4,w:0.45}, oK:{min:9,w:0.3}
    },
    BB_DEFENSE: {
        pairs:{min:2,all:true,w:0.9}, sA:{min:2,all:true,w:0.8},
        sBW:{all:true,w:0.95}, sC:{min:4,gap:1,w:0.7}, sC_wide:{min:6,gap:2,w:0.5},
        sK:{min:4,w:0.4}, sQ:{min:6,w:0.3},
        oBW_strong:{all:true,w:1.0}, oBW:{min:8,w:0.6}, oA:{min:6,w:0.4}, oK:{min:10,w:0.25}
    },
    THREE_BET: {
        pairs:{min:10,all:true,w:0.95}, pairs_med:{min:7,max:9,w:0.35},
        sA_strong:{min:12,all:true,w:0.85}, sA:{min:5,max:11,w:0.2},
        sBW_strong:{min:12,w:0.4}, oBW_strong:{all:true,w:0.55}, sC:{min:10,gap:0,w:0.2}
    },
    FOUR_BET: {
        pairs:{min:12,all:true,w:0.95}, pairs_med:{min:10,max:11,w:0.3}, sA_strong:{min:13,all:true,w:0.8}
    },
    CALL_THREE_BET: {
        pairs:{min:7,all:true,w:0.8}, pairs_low:{min:2,max:6,w:0.3},
        sA:{min:11,all:true,w:0.7}, sBW:{all:true,w:0.5}, sC:{min:9,gap:1,w:0.3}, oBW_strong:{all:true,w:0.55}
    }
};

class OpponentRange {
    constructor(seatIndex, totalPlayers) {
        this.seatIndex = seatIndex; this.totalPlayers = totalPlayers;
        this.template = null; this.narrowingFactor = 1.0;
        this.actionHistory = []; this._rangeWidth = 0.35;
    }
    initFromAction(action, isBlind) {
        const posFromBtn = (this.seatIndex + this.totalPlayers) % this.totalPlayers;
        let t;
        if (action === '4bet') t = RANGE_TEMPLATES.FOUR_BET;
        else if (action === '3bet') t = RANGE_TEMPLATES.THREE_BET;
        else if (action === 'call3bet') t = RANGE_TEMPLATES.CALL_THREE_BET;
        else if (isBlind) t = RANGE_TEMPLATES.BB_DEFENSE;
        else if (posFromBtn === 0) t = RANGE_TEMPLATES.BTN_OPEN;
        else if (posFromBtn === this.totalPlayers - 1) t = RANGE_TEMPLATES.CO_OPEN;
        else if (posFromBtn <= 2) t = RANGE_TEMPLATES.MP_OPEN;
        else t = RANGE_TEMPLATES.EP_OPEN;
        this.template = t;
        this.actionHistory.push({ action, isBlind });
        this.narrowingFactor = 1.0;
        this._rangeWidth = 0.30;
    }
    narrowByAction(action, aggressionLevel) {
        this.actionHistory.push({ action });
        switch (action) {
            case 'bet': case 'raise': case 'cbet': this.narrowingFactor *= (0.50 + aggressionLevel * 0.18); break;
            case 'call': this.narrowingFactor *= (0.68 + aggressionLevel * 0.10); break;
            case 'check': this.narrowingFactor *= 0.82; break;
            case 'fold': this.narrowingFactor = 0; break;
            case 'check_raise': this.narrowingFactor *= 0.40; break;
            case 'allin': this.narrowingFactor *= 0.35; break;
            default: this.narrowingFactor *= 0.88;
        }
        this.narrowingFactor = Math.max(0.04, Math.min(1.0, this.narrowingFactor));
        this._rangeWidth = 0.30 * this.narrowingFactor;
    }
    reset() { this.template = null; this.narrowingFactor = 1.0; this.actionHistory = []; this._rangeWidth = 0.35; }
    get rangeWidth() { return this._rangeWidth; }
}

// ═══════════════════════════════════════════════════════════════
// E. PLAYER MODEL (for exploitation)
// ═══════════════════════════════════════════════════════════════

class PlayerModel {
    constructor(seatIndex) {
        this.seatIndex = seatIndex; this.handsSeen = 0;
        this.seenHandIds = new Set();
        this.handFlags = new Map();
        this.vpipHands = 0; this.pfrHands = 0;
        this.pf = { fold:0,call:0,raise:0 }; this.postf = { fold:0,call:0,raise:0 };
        this.postflopFacedBet = 0; this.postflopFolds = 0;
        this.postflopCalls = 0; this.postflopAggressive = 0;
        this.cbetOpp = 0; this.cbetMade = 0; this.facedCbet = 0; this.foldedToCbet = 0;
        this.faced3bet = 0; this.foldedTo3bet = 0;
    }
    recordAction(action, street, handId, meta = {}) {
        const a = (action||'').toLowerCase();
        if (a.includes('blind') || a.includes('ante')) return;
        const id = handId ?? ('sample-' + (this.pf.fold + this.pf.call + this.pf.raise + this.postf.fold + this.postf.call + this.postf.raise));
        if (!this.seenHandIds.has(id)) {
            this.seenHandIds.add(id);
            this.handsSeen++;
        }
        if (!this.handFlags.has(id)) this.handFlags.set(id, {
            vpip:false, pfr:false, preflopFold:false
        });
        const flags = this.handFlags.get(id);
        const aggressive = meta.aggressive === true || a.includes('raise') || a === 'bet';
        const called = a.includes('call') || (a.includes('allin') && !aggressive);
        const folded = a.includes('fold');

        if (street === 'preflop') {
            if ((called || aggressive) && !flags.vpip) {
                flags.vpip = true;
                this.vpipHands++;
                this.pf.call++;
            }
            if (aggressive && !flags.pfr) {
                flags.pfr = true;
                this.pfrHands++;
                this.pf.raise++;
                if (this.pf.call > 0) this.pf.call--;
            }
            if (folded && !flags.vpip && !flags.preflopFold) {
                flags.preflopFold = true;
                this.pf.fold++;
            }
        } else {
            if (aggressive) this.postflopAggressive++;
            if (Number(meta.toCallBefore) > 0) {
                this.postflopFacedBet++;
                if (folded) { this.postflopFolds++; this.postf.fold++; }
                else if (aggressive) this.postf.raise++;
                else if (called) { this.postflopCalls++; this.postf.call++; }
            }
        }

        if (meta.isCbetOpportunity) this.recordCbet(aggressive);
        if (meta.facedCbet) this.recordFacedCbet(folded);
        if (meta.faced3bet) this.recordFaced3bet(folded);
    }
    recordCbet(made) { this.cbetOpp++; if (made) this.cbetMade++; }
    recordFacedCbet(folded) { this.facedCbet++; if (folded) this.foldedToCbet++; }
    recordFaced3bet(folded) { this.faced3bet++; if (folded) this.foldedTo3bet++; }

    // Bayesian priors keep a few unusual hands from turning the opponent model
    // into an extreme label. Observations gradually outweigh the population base.
    get vpip() { return (this.vpipHands+2.5)/(this.handsSeen+10); }
    get pfr() { return (this.pfrHands+1.8)/(this.handsSeen+10); }
    get foldFreq() { return (this.postflopFolds+2)/(this.postflopFacedBet+5); }
    get foldTo3betFreq() { return (this.foldedTo3bet+2.2)/(this.faced3bet+4); }
    get foldToCbetFreq() { return (this.foldedToCbet+2)/(this.facedCbet+4); }
    get passivity() { return (this.postflopCalls+2)/(this.postflopCalls+this.postflopAggressive+5); }
    get tightness() { return (1-this.vpip)*0.5+this.foldFreq*0.5; }
    get isTight() { return this.vpip<0.18; }
    get isLoose() { return this.vpip>0.40; }
    get foldsTooMuch() { return this.foldFreq>0.55; }
    get callsTooMuch() { return this.foldFreq<0.25&&this.passivity>0.6; }
}

// ═══════════════════════════════════════════════════════════════
// BOARD TEXTURE
// ═══════════════════════════════════════════════════════════════

function analyzeBoardTexture(cc, hole, isShortDeck = false) {
    if (!cc||cc.length<3) return { wetness:0,paired:false,monotone:false,hasFlushDraw:false,hasStraightDraw:false,openEnded:false,gutshot:false,highCard:0,connectedness:0 };
    const all=[...hole,...cc];
    const ranks={}; cc.forEach(c=>{ranks[c.rank]=(ranks[c.rank]||0)+1;});
    const paired=Object.values(ranks).some(c=>c>=2);
    const trips=Object.values(ranks).some(c=>c>=3);
    const suits={}; all.forEach(c=>{suits[c.suit]=(suits[c.suit]||0)+1;});
    const bSuits={}; cc.forEach(c=>{bSuits[c.suit]=(bSuits[c.suit]||0)+1;});
    const monotone=Object.values(bSuits).some(c=>c>=3);
    const hasFlushDraw=Object.values(suits).some(c=>c===4);
    const hasFlushDrawBoard=Object.values(bSuits).some(c=>c>=2);
    const rawVals=[...new Set(all.map(c=>c.value))];
    const vals=[...rawVals];
    if (rawVals.includes(14)) vals.push(isShortDeck ? 5 : 1);
    const valueSet = new Set(vals);
    const firstStraightLow = isShortDeck ? 5 : 1;
    const drawMissing = new Set();
    for (let low = firstStraightLow; low <= 10; low++) {
        const window = [low, low+1, low+2, low+3, low+4];
        const present = window.filter(value => valueSet.has(value));
        if (present.length === 4) {
            const missing = window.find(value => !valueSet.has(value));
            if (missing !== undefined) drawMissing.add(missing);
        }
    }
    const hasStraightDraw = drawMissing.size > 0;
    const oe = drawMissing.size >= 2;
    const gs = hasStraightDraw && !oe;
    const highCard=Math.max(...cc.map(c=>c.value));
    const sv=cc.map(c=>c.value).sort((a,b)=>a-b);
    let conn=0;
    for(let i=1;i<sv.length;i++){const d=sv[i]-sv[i-1];if(d<=2)conn+=(3-d)/3;}
    conn=conn/Math.max(1,cc.length-1);
    let wet=0;
    if(hasFlushDrawBoard)wet+=0.3;
    if(monotone)wet+=0.35;
    wet+=conn*0.4;
    if(paired)wet-=0.15;
    if (hasStraightDraw) wet += oe ? 0.18 : 0.10;
    return { wetness:Math.max(0,Math.min(1,wet)),paired,trips,monotone,hasFlushDraw,hasFlushDrawBoard,hasStraightDraw,openEnded:oe,gutshot:gs,highCard,connectedness:conn };
}

// ═══════════════════════════════════════════════════════════════
// POSITION & PREFLOP STRENGTH
// ═══════════════════════════════════════════════════════════════

function getPositionCategory(seatIndex, totalPlayers) {
    const n=totalPlayers; if(n<=2)return POS.LP;
    const pfb=(seatIndex+n)%n;
    if(pfb===0||pfb===n-1)return POS.LP;
    if(pfb===1||pfb===2)return POS.BL;
    if(pfb===3)return POS.EP;
    return POS.MP;
}

function getPositionCategoryFromButton(distance, totalPlayers, isSmallBlind, isBigBlind, isShortDeck = false) {
    if (totalPlayers <= 2) return distance === 0 ? POS.LP : POS.BL;
    if (isSmallBlind || isBigBlind) return POS.BL;
    if (!isShortDeck && (distance === 1 || distance === 2)) return POS.BL;
    if (distance === 0 || distance === totalPlayers - 1) return POS.LP;
    if (isShortDeck ? distance <= 2 : distance === 3) return POS.EP;
    return POS.MP;
}

function getPreflopStrength(c1,c2,posCat,config) {
    const h=Math.max(c1.value,c2.value),l=Math.min(c1.value,c2.value),s=c1.suit===c2.suit,g=h-l,ip=c1.rank===c2.rank;
    let bs=0,playable=true;
    if(ip){const v=c1.value;if(v>=13)bs=0.90;else if(v>=12)bs=0.82;else if(v>=11)bs=0.72;else if(v>=10)bs=0.62;else if(v>=9)bs=0.52;else if(v>=8)bs=0.44;else if(v>=7)bs=0.38;else if(v>=5)bs=0.30;else bs=0.20;}
    else if(h===14&&l>=13)bs=s?0.85:0.78;else if(h===14&&l===12)bs=s?0.72:0.62;
    else if(h===14&&l===11)bs=s?0.62:0.52;else if(h===14&&l===10)bs=s?0.52:0.42;
    else if(h>=13&&l>=12)bs=s?0.65:0.55;else if(h>=13&&l>=11)bs=s?0.52:0.40;
    else if(h>=13&&l>=10)bs=s?0.42:0.30;else if(h>=12&&l>=11)bs=s?0.48:0.38;
    else if(h>=12&&l>=10)bs=s?0.38:0.28;
    else if(h===14&&s&&l>=8)bs=0.38;else if(h===14&&s&&l>=5)bs=0.28;else if(h===14&&s)bs=0.22;
    else if(s&&g<=2&&h>=12)bs=0.42;else if(s&&g<=1&&h>=10)bs=0.35;else if(s&&g<=1&&h>=8)bs=0.28;
    else if(s&&g<=2&&h>=9)bs=0.24;else if(s&&g<=1&&h>=6)bs=0.18;
    else if(!s&&g<=2&&h>=13)bs=0.26;else if(!s&&g<=1&&h>=11)bs=0.22;
    else if(h>=14&&l>=7)bs=s?0.26:0.18;else if(h>=14)bs=s?0.18:0.12;
    else if(h>=13&&s&&l>=8)bs=0.22;else if(h>=12&&s&&l>=8)bs=0.18;
    else if(s)bs=0.12;else bs=0.06;
    let pm;
    switch(posCat){case POS.EP:pm=0.85;if(bs<0.30)playable=false;break;case POS.MP:pm=0.95;if(bs<0.22&&!s)playable=false;break;case POS.LP:pm=1.10;break;case POS.BL:pm=1.0;break;default:pm=1.0;}
    let st=bs*pm+(config.vpip-0.25)*0.6;
    if(!playable)st*=0.5;
    return {strength:Math.max(0.02,Math.min(0.95,st)),playable,baseStrength:bs};
}

// ═══════════════════════════════════════════════════════════════
// AIPlayer CLASS — Fast hybrid decision engine
// ═══════════════════════════════════════════════════════════════

class AIPlayer {
    constructor(name, style, stack=20000, index=0) {
        this.name=name; this.style=style; this.config=STYLE_CONFIGS[style];
        this.stack=stack; this.holeCards=[]; this.chipsInPot=0;
        this.folded=false; this.isAllIn=false; this.hasActed=false;
        this.lastAction=null; this.position=index; this.numPlayers=6;
        this.bigBlind=80;
        this.isShortDeck=false;
        this.opponentRanges={}; this.playerModels={};
        // Cache for Monte Carlo results within a single decision
        this._cachedEquity = null;
        this._cacheKey = '';
        this.streetPlan = null;
    }

    // ═══════════════════════════════════════════════════════════
    // MAIN DECISION — fast path with single Monte Carlo
    // ═══════════════════════════════════════════════════════════

    decide(gameState) {
        this.hasActed = true;
        const { communityCards, pot, currentBet, toCall, numOpponentsActive, bigBlind } = gameState;
        if (bigBlind) this.bigBlind = bigBlind;

        const isPF = !communityCards || communityCards.length === 0;
        const stack = Math.max(0, Number(gameState.stack ?? this.stack) || 0);
        this.stack = stack;
        const effectiveStack = Math.max(0, Number(gameState.effectiveStack ?? stack) || stack);
        const ownBet = Math.max(0, Number(gameState.yourBet) || 0);
        const maxRaiseTo = Math.max(ownBet, Number(gameState.maxRaiseTo) || ownBet + stack);
        const minRaiseTo = Math.max(0, Number(gameState.minRaiseTo) || this.bigBlind);
        const canRaise = gameState.canRaise !== false && maxRaiseTo > currentBet;
        const posCat = Number.isFinite(gameState.positionFromButton)
            ? getPositionCategoryFromButton(gameState.positionFromButton, this.numPlayers,
                gameState.isSmallBlind, gameState.isBigBlind, this.isShortDeck)
            : getPositionCategory(this.position, this.numPlayers);
        const rng = Math.random();
        const spr = pot > 0 ? effectiveStack / pot : 999;
        const potOdds = toCall > 0 ? toCall / (pot + toCall) : 0;
        this._currentOpponents = Math.max(1, numOpponentsActive || 1);

        // ── Blind specials ──
        if (isPF && gameState.isBigBlind && toCall === 0 && gameState.canCheck) {
            const pf = isPF ? getPreflopStrength(this.holeCards[0], this.holeCards[1], posCat, this.config) : null;
            if (canRaise && pf && pf.strength > 0.55 && rng < this.config.aggression * 0.30) {
                return this._legalize(this._act('raise', Math.round(this.bigBlind * 3)), gameState);
            }
            return this._act('check', 0);
        }
        if (isPF && gameState.isSmallBlind && toCall === 0 && gameState.canCheck) {
            const pf = isPF ? getPreflopStrength(this.holeCards[0], this.holeCards[1], posCat, this.config) : null;
            if (canRaise && pf && pf.strength > 0.58 && rng < 0.25 && numOpponentsActive <= 3) {
                return this._legalize(this._act('raise', Math.round(this.bigBlind * 3.5)), gameState);
            }
            return this._act('check', 0);
        }

        // ── Preflop ──
        if (isPF) return this._legalize(this._preflopDecide(gameState, posCat, spr, rng), gameState);

        // ── Postflop: one equity estimate for all EV calculations ──
        const equity = this._estimateEquity(communityCards, numOpponentsActive);
        const texture = analyzeBoardTexture(communityCards, this.holeCards, this.isShortDeck);
        const street = communityCards.length; // 3,4,5

        // Equity stays objective. Personality affects sizing/mixing, never the
        // cards' mathematical chance of winning.
        const str = Math.max(0.01, Math.min(0.99, equity));

        // A planned check-raise now survives the first action and is actually
        // executed when the action returns on the same street. Abandon it when
        // the price is excessive or the betting rights are closed.
        if (this.streetPlan && this.streetPlan.street !== street) this.streetPlan = null;
        if (toCall > 0 && this.streetPlan?.type === 'check_raise') {
            const plan = this.streetPlan;
            this.streetPlan = null;
            if (canRaise && str >= plan.minEquity && toCall <= Math.max(this.bigBlind * 4, pot * 1.25)) {
                const target = Math.min(maxRaiseTo, Math.max(minRaiseTo,
                    ownBet + toCall + Math.round((pot + toCall) * (texture.wetness > 0.45 ? 0.82 : 0.68))));
                return this._legalize(this._act(target >= maxRaiseTo ? 'allin' : 'raise',
                    target >= maxRaiseTo ? stack : target), gameState);
            }
        }

        // ── Build candidate actions ──
        const candidates = [];

        if (toCall > 0) candidates.push({ action:'fold', amount:0, invest:0, ev:0 });

        // Check
        if (toCall === 0 && gameState.canCheck) {
            candidates.push({ action:'check', amount:0, invest:0, ev:str * pot });
        }

        // Call
        if (toCall > 0 && stack > 0) {
            const callInvest = Math.min(toCall, stack);
            const evCall = str * (pot + callInvest) - callInvest;
            candidates.push({
                action: callInvest >= stack ? 'allin' : 'call',
                amount: callInvest >= stack ? stack : callInvest,
                invest: callInvest,
                ev: evCall
            });
        }

        if (canRaise) {
            const raiseTargets = this._getRaiseSizes(pot, toCall, maxRaiseTo, isPF, street, {
                ...gameState, ownBet, minRaiseTo, maxRaiseTo
            });
            for (const target of raiseTargets) {
                if (target <= currentBet || target <= ownBet) continue;
                const invest = target - ownBet;
                const opponentCall = Math.max(0, target - currentBet);
                const fe = this._estimateFoldEq(invest, pot, toCall, street);
                const calledEv = str * (pot + invest + opponentCall) - invest;
                const ev = fe * pot + (1 - fe) * calledEv;
                const isAllIn = target >= maxRaiseTo;
                candidates.push({ action: isAllIn ? 'allin' : 'raise', amount: isAllIn ? stack : target, invest, ev });
                if (isAllIn) break;
            }
        }

        // Continue the betting lead with a range-aware frequency. Multi-way pots
        // and wet boards reduce air barrels; strong made hands and real draws keep
        // betting. This distinguishes a genuine prior-street aggressor from blinds.
        let decision = null;
        if (toCall === 0 && gameState.isPreviousStreetAggressor) {
            const frequency = street === 3 ? this.config.cbFreq
                : street === 4 ? this.config.doubleBarrel : this.config.doubleBarrel * 0.55;
            const multiwayFactor = Math.pow(0.68, Math.max(0, this._currentOpponents - 1));
            const blocker = street === 5 ? this._riverBlockerScore(communityCards) : 0;
            const hasRealSemiBluff = texture.hasFlushDraw || texture.openEnded;
            const rangeCandidate = str >= 0.42 || hasRealSemiBluff || (str < 0.24 && blocker >= 0.75);
            if (rangeCandidate && rng < frequency * multiwayFactor) {
                const bets = candidates.filter(candidate => candidate.action === 'raise')
                    .sort((a, b) => a.amount - b.amount);
                const preferred = texture.wetness > 0.45 ? bets[Math.min(1, bets.length - 1)] : bets[0];
                if (preferred && preferred.ev >= -pot * 0.10) decision = preferred;
            }
        }

        // Delayed c-bet: the preflop aggressor checked through the flop and now
        // gets a clean turn stab. Use a lower frequency than a flop c-bet, favor
        // useful scare cards / picked-up draws, and reduce it sharply multi-way.
        if (!decision && toCall === 0 && street === 4 && gameState.isDelayedCBetCandidate) {
            const turnCard = communityCards[3];
            const turnPairedBoard = communityCards.slice(0, 3)
                .some(card => card.rank === turnCard?.rank);
            const pressureCard = (turnCard?.value || 0) >= 11 || turnPairedBoard;
            const realDraw = texture.hasFlushDraw || texture.hasStraightDraw;
            const rangeCandidate = str >= 0.48 || realDraw || (str < 0.28 && pressureCard);
            const multiwayFactor = Math.pow(0.62, Math.max(0, this._currentOpponents - 1));
            const frequency = this.config.cbFreq * 0.48 * multiwayFactor;
            if (rangeCandidate && rng < frequency) {
                const bets = candidates.filter(candidate => candidate.action === 'raise' ||
                    (candidate.action === 'allin' && (str >= 0.60 || (realDraw && spr <= 1.25))))
                    .sort((a, b) => a.invest - b.invest);
                const preferred = texture.wetness > 0.48 && bets.length > 1 ? bets[1] : bets[0];
                if (preferred && preferred.ev >= -pot * 0.06) decision = preferred;
            }
        }

        // ── Apply poker heuristics ──
        if (!decision) decision = this._applyHeuristics(candidates, str, pot, toCall, texture, street, spr, rng);
        if (decision.action === 'check' && str > 0.68 && texture.wetness > 0.28 &&
            rng < this.config.checkRaiseFreq * 2.4) {
            this.streetPlan = { type:'check_raise', street, minEquity:Math.max(0.60, str - 0.12) };
        }

        return this._legalize(this._act(decision.action, decision.amount), gameState);
    }

    // ═══════════════════════════════════════════════════════════
    // EQUITY (single Monte Carlo run)
    // ═══════════════════════════════════════════════════════════

    _estimateEquity(communityCards, numOpponents) {
        // Use the optimized ProbabilityCalculator when available
        const calc = (this.isShortDeck && typeof SDProbability !== 'undefined')
            ? SDProbability : ProbabilityCalculator;
        if (typeof calc !== 'undefined' && calc.calculateWinProb) {
            try {
                const result = calc.calculateWinProb(
                    this.holeCards, communityCards, Math.max(1, numOpponents), 500
                );
                return Number.isFinite(result.equityProb)
                    ? result.equityProb
                    : result.winProb + result.tieProb / (Math.max(1, numOpponents) + 1);
            } catch(e) { /* fall through */ }
        }
        // Fast fallback: use hand rank
        if (typeof evaluateHand !== 'undefined') {
            try {
                const all = [...this.holeCards, ...communityCards];
                const hand = evaluateHand(all);
                if (hand) {
                    // Map rank 0-9 to approximate win probability
                    // This is a rough approximation
                    const baseMap = [0.15, 0.30, 0.42, 0.50, 0.58, 0.62, 0.68, 0.75, 0.85, 0.95];
                    const base = baseMap[Math.min(9, hand.rank)] || 0.5;
                    // Discount for multiple opponents
                    return Math.pow(base, 1 + numOpponents * 0.3);
                }
            } catch(e) {}
        }
        return 0.35; // Complete fallback
    }

    _adjustForTexture(equity, cc) {
        return equity;
    }

    // ═══════════════════════════════════════════════════════════
    // FOLD EQUITY (lightweight estimation)
    // ═══════════════════════════════════════════════════════════

    /** Estimate probability opponents fold to a raise of given size */
    _estimateFoldEq(raiseAmt, pot, toCall, street) {
        // Get average opponent fold tendency
        let avgFoldFreq = 0.40;
        let avgRangeWidth = 0.30;
        let modelCount = 0;
        for (const [seat, model] of Object.entries(this.playerModels)) {
            if (model.handsSeen >= 3) {
                avgFoldFreq = avgFoldFreq * 0.7 + model.foldFreq * 0.3; // Weight towards observed
                modelCount++;
            }
        }
        for (const r of Object.values(this.opponentRanges)) {
            avgRangeWidth = avgRangeWidth * 0.5 + r.rangeWidth * 0.5;
        }

        // Base fold rate from player tendencies
        let foldRate = avgFoldFreq;

        // Range adjustment: tighter ranges fold less
        if (avgRangeWidth < 0.15) foldRate -= 0.10;

        // Size pressure: bigger bets get more folds
        const potRatio = pot > 0 ? raiseAmt / pot : 1;
        foldRate += Math.min(0.35, potRatio * 0.22);

        // Exploitation adjustments
        const exp = this._getExploitProfile();
        if (exp.foldsTooMuch) foldRate += 0.10;
        if (exp.callsTooMuch) foldRate -= 0.12;

        // Street factor
        if (street === 3) foldRate -= 0.02; // Flop: less fold equity
        if (street === 5) foldRate += 0.04;  // River: more fold equity

        const individualFold = Math.max(0.05, Math.min(0.85, foldRate));
        // A bluff succeeds only if every live opponent folds. This prevents the
        // old heads-up fold estimate from making multi-way bluffs look magical.
        return Math.pow(individualFold, Math.max(1, this._currentOpponents || 1));
    }

    _getExploitProfile() {
        let foldsTooMuch = false, callsTooMuch = false;
        for (const model of Object.values(this.playerModels)) {
            if (model.handsSeen >= 5) {
                if (model.foldsTooMuch) foldsTooMuch = true;
                if (model.callsTooMuch) callsTooMuch = true;
            }
        }
        return { foldsTooMuch, callsTooMuch };
    }

    // ═══════════════════════════════════════════════════════════
    // RAISE SIZE GENERATION
    // ═══════════════════════════════════════════════════════════

    _getRaiseSizes(pot, toCall, maxRaiseTo, isPF, street, gs = {}) {
        const sizes = [];
        const ownBet = Math.max(0, Number(gs.ownBet) || 0);
        const minRaiseTo = Math.max(0, Number(gs.minRaiseTo) || this.bigBlind);
        const maxTarget = Math.max(ownBet, Number(gs.maxRaiseTo) || maxRaiseTo);
        // A short stack may legally raise only by going all-in below the normal
        // minimum. Evaluate that real target directly; inflating it to the
        // theoretical minimum distorts both the investment and the call EV.
        if (maxTarget < minRaiseTo) return [maxTarget];
        if (isPF) {
            const base = Math.max(this.bigBlind, Number(gs.currentBet) || 0);
            [2.2, 2.7, 3.3, 4.0].forEach(m => {
                const target = Math.max(minRaiseTo, Math.round(base * m));
                sizes.push(Math.min(target, maxTarget));
            });
        } else {
            const texture = analyzeBoardTexture(gs.communityCards || [], this.holeCards, this.isShortDeck);
            const fracs = street === 5
                ? [0.35, 0.70, 1.10]
                : texture.wetness > 0.45 ? [0.55, 0.75, 0.95] : [0.30, 0.55, 0.75];
            const potAfterCall = Math.max(this.bigBlind, pot + toCall);
            for (const f of fracs) {
                const target = ownBet + toCall + Math.round(potAfterCall * f);
                sizes.push(Math.min(Math.max(minRaiseTo, target), maxTarget));
            }
        }
        if (sizes.length === 0) sizes.push(maxTarget);
        if (!sizes.includes(maxTarget)) sizes.push(maxTarget);
        return [...new Set(sizes)].sort((a,b)=>a-b);
    }

    // ═══════════════════════════════════════════════════════════
    // POKER HEURISTICS — layers real poker wisdom over EV math
    // ═══════════════════════════════════════════════════════════

    _applyHeuristics(candidates, strength, pot, toCall, texture, street, spr, rng) {
        // Sort by EV
        candidates.sort((a,b) => b.ev - a.ev);
        const best = candidates[0];

        // ── Heuristic 1: Very strong hands should build the pot ──
        if (strength > 0.72) {
            const raiseOpt = candidates.find(c => c.action === 'raise' || c.action === 'allin');
            if (raiseOpt && raiseOpt.ev > 0 && best.action !== 'raise' && best.action !== 'allin') {
                // Override: force aggression with very strong hands
                if (rng < 0.70 + this.config.aggression * 0.2) return raiseOpt;
            }
            // If EV says fold with very strong hand (shouldn't happen), call instead
            if (best.action === 'fold') {
                const callOpt = candidates.find(c => c.action === 'call' || c.action === 'check');
                if (callOpt) return callOpt;
            }
        }

        // ── Heuristic 2: Strong hands + SPR low → commit ──
        if (strength > 0.55 && spr < 3.5) {
            const pushOpt = candidates.find(c => c.action === 'allin');
            const raiseOpt = candidates.find(c => c.action === 'raise');
            // Don't fold with decent equity in low SPR pots
            if ((pushOpt || raiseOpt) && best.action === 'fold') {
                return pushOpt || raiseOpt;
            }
        }

        // ── Heuristic 3: Wet board + strong draw → semi-bluff more ──
        if (texture.wetness > 0.4 && (texture.hasFlushDraw || texture.openEnded) && strength > 0.35) {
            const semiBluffOpt = candidates.find(c => c.action === 'raise' && (c.invest || c.amount) <= pot * 0.8);
            if (semiBluffOpt && rng < this.config.aggression * 0.45 && toCall === 0) {
                return semiBluffOpt;
            }
        }

        // ── Heuristic 4: Dry board, strong hand → slow-play sometimes ──
        if (strength > 0.65 && texture.wetness < 0.2 && street === 3 && rng < 0.15 && toCall === 0) {
            const checkOpt = candidates.find(c => c.action === 'check');
            if (checkOpt) return checkOpt; // Trap on dry flop
        }

        // ── Heuristic 5: Don't fold to tiny bets ──
        if (toCall > 0 && toCall <= this.bigBlind && strength > 0.20 && best.action === 'fold') {
            const callOpt = candidates.find(c => c.action === 'call');
            if (callOpt) return callOpt;
        }

        // ── Heuristic 6: River thin value ──
        if (street === 5 && strength > 0.52 && strength < 0.70 && toCall === 0) {
            // Check if we should bet for thin value
            const exp = this._getExploitProfile();
            if (exp.callsTooMuch && rng < 0.60) {
                const valueBet = candidates.find(c => c.action === 'raise' && (c.invest || c.amount) <= pot * 0.6);
                if (valueBet) return valueBet;
            }
        }

        // ── Heuristic 7: Check-raise with monsters on draw-heavy boards ──
        if (toCall === 0 && strength > 0.70 && texture.wetness > 0.3 && rng < 0.25) {
            const checkOpt = candidates.find(c => c.action === 'check');
            if (checkOpt) return checkOpt; // Trap to check-raise
        }

        // ── Heuristic 8: Calling station special ──
        if (this.style === AI_STYLES.CALLING && best.action === 'fold' && toCall <= this.bigBlind * 4) {
            const callOpt = candidates.find(c => c.action === 'call');
            if (callOpt) return callOpt;
        }

        // ── Heuristic 9: Maniac aggression override ──
        if (this.style === AI_STYLES.MANIAC && rng < 0.12 && toCall === 0) {
            const aggOpt = candidates.find(c => c.action === 'raise' || c.action === 'allin');
            if (aggOpt && aggOpt.ev > 0) return aggOpt;
        }

        // ── Heuristic 10: Exploit tight players with more bluffs ──
        const exp = this._getExploitProfile();
        if (exp.foldsTooMuch && best.action === 'check' && toCall === 0 && rng < 0.30) {
            const blockerBonus = this._riverBlockerScore();
            const bluffOpt = candidates.find(c => c.action === 'raise' && (c.invest || c.amount) <= pot * (0.7 + blockerBonus * 0.25));
            if (bluffOpt) return bluffOpt;
        }

        // ── Mixing for balance ──
        if (candidates.length >= 2 && best.ev > 0) {
            const second = candidates[1];
            if (second.ev > 0) {
                const gap = (best.ev - second.ev) / Math.max(1, Math.abs(best.ev));
                if (gap < 0.25 && rng < 0.06) return second; // Occasionally mix
            }
        }

        return best;
    }

    // ═══════════════════════════════════════════════════════════
    // PREFLOP (separated for clarity, fast path)
    // ═══════════════════════════════════════════════════════════

    _preflopDecide(gs, posCat, spr, rng) {
        const { pot, currentBet, toCall, canCheck, numOpponentsActive } = gs;
        const c1=this.holeCards[0],c2=this.holeCards[1];
        let range;
        if (this.isShortDeck && typeof this.evaluatePreFlop === 'function') {
            const sd = this.evaluatePreFlop();
            const positional = posCat === POS.LP ? 1.08 : posCat === POS.EP ? 0.92 : 1;
            range = { strength: Math.min(0.95, sd.strength * positional), playable: sd.strength >= 0.16 };
        } else range=getPreflopStrength(c1,c2,posCat,this.config);
        const str=range.strength;
        const raiseCount = Math.max(0, Number(gs.preflopRaiseCount) || 0);
        const stack = Math.max(0, Number(gs.stack ?? this.stack) || 0);
        const ownBet = Math.max(0, Number(gs.yourBet) || 0);
        const maxRaiseTo = Math.max(ownBet, Number(gs.maxRaiseTo) || ownBet + stack);
        const minRaiseTo = Math.max(this.bigBlind, Number(gs.minRaiseTo) || this.bigBlind);
        const canRaise = gs.canRaise !== false && maxRaiseTo > currentBet;
        const stackBB = stack / Math.max(1, this.bigBlind);
        const callAmt = Math.min(toCall, stack);

        // Short stacks use a disciplined push/fold/reshove strategy instead of
        // making tiny raises that leave an unusable SPR behind.
        if (stackBB <= 12) {
            const pushThreshold = posCat === POS.LP ? 0.36 : posCat === POS.BL ? 0.42 : 0.48;
            if (canRaise && str >= pushThreshold + Math.max(0, numOpponentsActive - 1) * 0.025)
                return this._act('allin', stack);
            const potOdds = callAmt > 0 ? callAmt / Math.max(1, pot + callAmt) : 0;
            if (callAmt > 0 && str >= potOdds + 0.12) return this._act(callAmt >= stack ? 'allin' : 'call', callAmt);
            return this._act(canCheck ? 'check' : 'fold', 0);
        }

        // No voluntary raise yet: distinguish a blind bring-in from a real open.
        if (raiseCount === 0) {
            const openThreshold = posCat === POS.LP ? 0.20 : posCat === POS.BL ? 0.27 : posCat === POS.MP ? 0.29 : 0.34;
            const styleShift = (0.25 - this.config.vpip) * 0.35;
            if (canRaise && range.playable && str >= openThreshold + styleShift) {
                const mult = posCat === POS.LP ? 2.2 : 2.5;
                const target = Math.min(maxRaiseTo, Math.max(minRaiseTo,
                    Math.round(this.bigBlind * (mult + Math.max(0, numOpponentsActive - 2) * 0.12))));
                return this._act(target >= maxRaiseTo ? 'allin' : 'raise', target >= maxRaiseTo ? stack : target);
            }
            if (toCall > 0 && range.playable && str >= openThreshold - 0.08 && toCall <= this.bigBlind)
                return this._act(callAmt >= stack ? 'allin' : 'call', callAmt);
            return this._act(canCheck ? 'check' : 'fold', 0);
        }

        // Facing an open / 3-bet / 4-bet.
        if (str > (raiseCount >= 2 ? 0.72 : 0.62)) {
            if (canRaise && rng < 0.72 + this.config.aggression * 0.18) {
                const multiplier = posCat === POS.LP ? 3.0 : 4.0;
                const target = Math.min(maxRaiseTo, Math.max(minRaiseTo, Math.round(currentBet * multiplier)));
                return this._act(target >= maxRaiseTo ? 'allin' : 'raise', target >= maxRaiseTo ? stack : target);
            }
            return this._act(callAmt >= stack ? 'allin' : 'call', callAmt);
        }
        const potOdds = callAmt / Math.max(1, pot + callAmt);
        if (str > Math.max(0.36, potOdds + (posCat === POS.LP ? 0.08 : 0.13))) {
            return this._act(callAmt >= stack ? 'allin' : 'call', callAmt);
        }
        // Polarized 3-bet bluff: suited ace/king blockers, position, and an
        // opponent profile that actually folds. Never use this multi-way.
        const high = Math.max(c1.value, c2.value);
        const blocker = high >= 13 && c1.suit === c2.suit;
        if (canRaise && raiseCount === 1 && numOpponentsActive === 1 && posCat === POS.LP &&
            blocker && str > 0.15 && rng < this.config.threeBetFreq) {
            const target = Math.min(maxRaiseTo, Math.max(minRaiseTo, Math.round(currentBet * 3)));
            return this._act(target >= maxRaiseTo ? 'allin' : 'raise', target >= maxRaiseTo ? stack : target);
        }
        return this._act('fold',0);
    }

    // ═══════════════════════════════════════════════════════════
    // OPPONENT TRACKING
    // ═══════════════════════════════════════════════════════════

    updateOpponentRange(seatIdx, action, isBlind) {
        if (seatIdx===this.position) return;
        if (!this.opponentRanges[seatIdx]) this.opponentRanges[seatIdx]=new OpponentRange(seatIdx,this.numPlayers);
        const r=this.opponentRanges[seatIdx];
        if (r.actionHistory.length===0) r.initFromAction(action,isBlind);
        else r.narrowByAction(action,this.config.aggression);
    }

    recordOpponentAction(seatIdx, action, street, handId, meta = {}) {
        if (seatIdx===this.position) return;
        if (!this.playerModels[seatIdx]) this.playerModels[seatIdx]=new PlayerModel(seatIdx);
        this.playerModels[seatIdx].recordAction(action,street,handId,meta);
    }

    reset() {
        this.holeCards=[]; this.chipsInPot=0; this.folded=false;
        this.isAllIn=false; this.hasActed=false; this.lastAction=null;
        this.streetPlan=null;
        for (const r of Object.values(this.opponentRanges)) r.reset();
    }

    _act(action, amount) {
        amount=Math.round(amount||0);
        this.lastAction={action,amount};
        return this.lastAction;
    }

    _legalize(decision, gs) {
        const ownBet = Math.max(0, Number(gs.yourBet) || 0);
        const stack = Math.max(0, Number(gs.stack ?? this.stack) || 0);
        const toCall = Math.max(0, Number(gs.toCall) || 0);
        const currentBet = Math.max(0, Number(gs.currentBet) || 0);
        const minRaiseTo = Math.max(0, Number(gs.minRaiseTo) || this.bigBlind);
        const maxRaiseTo = Math.max(ownBet, Number(gs.maxRaiseTo) || ownBet + stack);
        const canRaise = gs.canRaise !== false && maxRaiseTo > currentBet;
        let action = decision?.action || (toCall > 0 ? 'fold' : 'check');
        let amount = Math.max(0, Math.round(Number(decision?.amount) || 0));

        if (action === 'check' && toCall > 0) action = 'fold';
        if (action === 'call') {
            amount = Math.min(toCall, stack);
            if (amount >= stack) action = 'allin';
        }
        if (action === 'raise') {
            if (!canRaise) return this._act(toCall > 0 ? (toCall >= stack ? 'allin' : 'call') : 'check', Math.min(toCall, stack));
            amount = Math.min(maxRaiseTo, Math.max(minRaiseTo, amount));
            if (amount >= maxRaiseTo) return this._act('allin', stack);
        }
        if (action === 'allin' && ownBet + stack > currentBet && !canRaise) {
            return this._act(toCall >= stack ? 'allin' : (toCall > 0 ? 'call' : 'check'), Math.min(toCall, stack));
        }
        return this._act(action, action === 'allin' ? stack : amount);
    }

    _riverBlockerScore(communityCards = []) {
        if (!this.holeCards || this.holeCards.length !== 2) return 0;
        const boardSuits = {};
        for (const card of communityCards) boardSuits[card.suit] = (boardSuits[card.suit] || 0) + 1;
        const flushSuit = Object.keys(boardSuits).find(suit => boardSuits[suit] >= 3);
        if (flushSuit) {
            if (this.holeCards.some(card => card.suit === flushSuit && card.value === 14)) return 1;
            if (this.holeCards.some(card => card.suit === flushSuit && card.value === 13) &&
                communityCards.some(card => card.suit === flushSuit && card.value === 14)) return 0.8;
        }
        const hasAce = this.holeCards.some(card => card.value === 14);
        const hasKing = this.holeCards.some(card => card.value === 13);
        return hasAce ? 1 : hasKing ? 0.5 : 0;
    }
}

// ═══════════════════════════════════════════════════════════════
// FACTORY
// ═══════════════════════════════════════════════════════════════

function createAIPlayers(count, startingStack = 20000) {
    const players=[];
    const styles=[...Object.values(AI_STYLES)];
    for (let i=styles.length-1;i>0;i--){const j=Math.floor(Math.random()*(i+1));[styles[i],styles[j]]=[styles[j],styles[i]];}
    for (let i=0;i<count;i++) {
        const style=styles[i%styles.length];
        const stack=Math.max(1, Math.floor(Number(startingStack) || 20000));
        const p=new AIPlayer(AI_NAMES[i%AI_NAMES.length],style,stack,i);
        p.avatar=AI_AVATARS[i%AI_AVATARS.length];
        players.push(p);
    }
    return players;
}
