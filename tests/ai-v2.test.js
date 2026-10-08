'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const core=require('../js/ai-core');
const probability=require('../js/probability-core');
const c=(rank,suit='spades')=>({rank,suit,value:({J:11,Q:12,K:13,A:14})[rank]||Number(rank)});
const evaluateCards=(cards,variant)=>probability.evaluate(cards,variant==='shortdeck');
test('river range enumeration gives exact nuts and board ties, with no sampling error',()=>{
    const base={phase:'river',activeOpponentSeats:[1],numOpponents:1,evaluateCards};
    const nuts=core.normalizedContext({...base,holeCards:[c('A'),c('K')],communityCards:[c('Q'),c('J'),c('10'),c('2','hearts'),c('3','clubs')]});
    const a=core.estimateRangeEquity(nuts,new Map(),new core.SeededRng(1));
    assert.equal(a.mean,1); assert.equal(a.exact,true); assert.equal(a.standardError,0); assert.equal(a.samples,990);
    const tie=core.normalizedContext({...base,holeCards:[c('2','hearts'),c('3','clubs')],communityCards:[c('A'),c('K'),c('Q'),c('J'),c('10')]});
    assert.equal(core.estimateRangeEquity(tie,new Map(),new core.SeededRng(2)).mean,.5);
});
test('draws require a future card and hole-card contribution; nut blockers are distinct',()=>{
    const h=[c('A'),c('K','hearts')], board=[c('Q'),c('J'),c('4'),c('2','clubs')];
    assert.equal(core.analyzeTexture(board,h,'standard').hasFlushDraw,true);
    const river=core.analyzeTexture([...board,c('3','clubs')],h,'standard');
    assert.equal(river.hasFlushDraw,false); assert.equal(river.hasStraightDraw,false); assert.ok(river.nutBlocker>.5);
    const publicDraw=core.analyzeTexture([c('Q'),c('J'),c('10'),c('9')],[c('2','clubs'),c('3','hearts')],'standard');
    assert.equal(publicDraw.hasStraightDraw,false); assert.equal(publicDraw.hasFlushDraw,false);
    assert.equal(core.analyzeTexture([c('7'),c('8','hearts'),c('K','clubs')],[c('6','clubs'),c('9','hearts')],'shortdeck').openEnded,true);
});
test('public flop raises update board-specific range weights and preserve historical boards',()=>{
    const belief=new core.OpponentBelief(1);
    const set=[c('9','hearts'),c('9','clubs')], air=[c('A','hearts'),c('K','clubs')];
    const before=belief.comboWeight(...set,'standard')/belief.comboWeight(...air,'standard');
    const board=[c('9'),c('4','clubs'),c('2','hearts')];
    belief.observe({action:'raise',street:'flop',meta:{communityCards:board,betFraction:1,potBefore:400,toCallBefore:200}});
    const after=belief.comboWeight(...set,'standard')/belief.comboWeight(...air,'standard');
    assert.ok(after>before*2,`${before} -> ${after}`);
    board[0]=c('A'); assert.equal(belief.history[0].board[0].rank,'9');
    belief.beginHand(); assert.equal(belief.history.length,0); assert.equal(belief.preflopSnapshot,null);
});
test('all-in opponents cannot be bluffed off and side-pot eligibility caps the available reward',()=>{
    const brain=new core.UnifiedPokerAI({seatId:0});
    const context=core.normalizedContext({heroSeat:0,stack:300,pot:2300,bigBlind:100,currentBet:0,maxRaiseTo:300,minRaiseTo:100,phase:'river',
        activeOpponentSeats:[1,2],publicPlayers:[{seat:0,committed:100,stack:300},{seat:1,committed:1000,stack:0,allIn:true},{seat:2,committed:1000,stack:200}],canRaise:true});
    assert.equal(core.contestablePot(context,300),900);
    const response=brain.responseForRaise(context,200,.3,{wetness:0},brain.activeReads(context));
    assert.equal(response.allFold,0); assert.ok(response.anyCall>0);
    assert.ok(brain.raiseTargets(context,{wetness:0}).every(t=>t<=200));
});
test('normalized observations strip opponent private information and explicit persona parameters survive',()=>{
    const ctx=core.normalizedContext({publicPlayers:[{seat:1,stack:10,holeCards:[c('A')],deck:[c('K')]}]});
    assert.equal(ctx.publicPlayers[0].holeCards,undefined); assert.equal(ctx.publicPlayers[0].deck,undefined);
    const brain=new core.UnifiedPokerAI({style:'TAG',profile:{riskAversion:.4}});
    assert.equal(brain.profile.riskAversion,.4);
    assert.notEqual(core.PROFILE_OVERRIDES.TAG.aggression,core.PROFILE_OVERRIDES.MANIAC.aggression);
});

test('validated variant presets switch with the rules while explicit lab options stay authoritative',()=>{
    const brain=new core.UnifiedPokerAI({style:'TAG'});
    const context={holeCards:[c('A'),c('K','hearts')],phase:'preflop',handId:1,heroSeat:0,stack:1000,pot:120,bigBlind:80,toCall:0,canCheck:true,legalActions:['check'],activeOpponentSeats:[]};
    assert.equal(brain.decide({...context,variant:'standard'}).trace.parameters.rangeEvidenceWeight,.45);
    assert.equal(brain.profile.calledRangeDiscount,.08);
    assert.equal(brain.decide({...context,variant:'shortdeck'}).trace.parameters.rangeEvidenceWeight,.70);
    assert.equal(brain.profile.calledRangeDiscount,.055);
    assert.ok(Object.isFrozen(brain.profile));
    const override=new core.UnifiedPokerAI({profile:{rangeEvidenceWeight:.31,calledRangeDiscount:.02,riskAversion:.4}});
    for(const variant of ['standard','shortdeck','standard']){
        override.decide({...context,variant});
        assert.equal(override.profile.rangeEvidenceWeight,.31);assert.equal(override.profile.calledRangeDiscount,.02);assert.equal(override.profile.riskAversion,.4);
    }
});
test('a short all-in winning the main pot does not erase equity in a separate side pot',()=>{
    const ctx=core.normalizedContext({heroSeat:0,pot:1400,activeOpponentSeats:[1,2],
        publicPlayers:[{seat:0,committed:500,stack:1000},{seat:1,committed:100,stack:0,allIn:true},
            {seat:2,committed:500,stack:1000},{seat:3,committed:300,stack:0,folded:true}]});
    // Main pot: 4*100 at 10%; upper layers: 3*200 + 2*200 at 80%.
    assert.equal(core.contestableReturn(ctx,0,{mean:.1,subsetEquities:{'2':.8}}),840);
    assert.equal(core.contestableReturn({...ctx,publicPlayers:[]},100,{mean:.5}),750);
});
test('unacted opponents remain unconditioned instead of all being assigned strong opening ranges',()=>{
    const ctx=core.normalizedContext({phase:'preflop',holeCards:[c('A'),c('Q','hearts')],activeOpponentSeats:[1,2,3],numOpponents:3,evaluateCards});
    const observed=new Map(ctx.activeOpponentSeats.map(s=>[String(s),new core.OpponentBelief(s)]));
    const a=core.estimateRangeEquity(ctx,observed,new core.SeededRng('no-action'));
    const b=core.estimateRangeEquity(ctx,new Map(),new core.SeededRng('no-action'));
    assert.equal(a.mean,b.mean); assert.equal(a.samples,b.samples);
    const belief=observed.get('1'); belief.observe({action:'raise',street:'preflop',meta:{betFraction:.7}});
    assert.ok(belief.comboWeight(c('A'),c('A','hearts'),'standard')>belief.comboWeight(c('7'),c('2','hearts'),'standard')*5);
});
test('all-in calls remain calls in range updates and behavior statistics',()=>{
    const a=new core.OpponentBelief(1),b=new core.OpponentBelief(1);
    const meta={aggressive:false,toCallBefore:100,potBefore:400,betFraction:.25,communityCards:[c('9'),c('4','clubs'),c('2','hearts')]};
    a.observe({action:'allin',street:'flop',meta}); b.observe({action:'call',street:'flop',meta});
    assert.equal(a.history[0].action,'call');assert.equal(a.comboWeight(c('A'),c('K','clubs'),'standard'),b.comboWeight(c('A'),c('K','clubs'),'standard'));
    const model=new core.BehavioralModel(1);model.observe({handId:1,street:'preflop',action:'allin',meta});
    assert.equal(model.vpipHands.size,1);assert.equal(model.pfrHands.size,0);
    model.observe({handId:2,street:'flop',action:'allin',meta});assert.equal(model.postflopCalls,1);assert.equal(model.postflopRaises,0);
});
test('preflop and postflop fold observations remain separate and affect the corresponding street',()=>{
    const brain=new core.UnifiedPokerAI({seatId:0,profile:{preflopResponseWeight:1}});
    for(let hand=1;hand<=20;hand++) {
        brain.observeAction({seatId:1,handId:hand,action:'fold',street:'preflop',meta:{toCallBefore:80,betFraction:.7}});
        brain.observeAction({seatId:1,handId:hand,action:'call',street:'flop',meta:{toCallBefore:80,betFraction:.7}});
    }
    const model=brain.ensureOpponent(1).model;assert.ok(model.foldRateFor(.7,'preflop')>model.foldRateFor(.7)+.5);
});
test('range width uses actual combination percentiles rather than a linear hand-score threshold',()=>{
    for(const variant of ['standard','shortdeck']) {
        const deck=core.buildDeck(variant),cutoff=core.startingRangeCutoff(.24,variant);
        let count=0,total=0;
        for(let i=0;i<deck.length;i++)for(let j=i+1;j<deck.length;j++){
            total++;count+=Number(core.startingHandStrength(deck[i],deck[j],variant,'MP')>=cutoff);
        }
        assert.ok(count/total>=.23&&count/total<.32,`${variant}: ${count/total}`);
        assert.ok(core.startingRangeCutoff(.1,variant)>core.startingRangeCutoff(.4,variant));
    }
});
test('conditional caller equity excludes folded winners and divides ties correctly',()=>{
    const info={mean:0,responseSamples:{seats:['1','2'],relations:[[-1,1]]}};
    const response={opponents:[{seat:1,fold:.8,call:.2,raise:0},{seat:2,fold:.5,call:.5,raise:0}]};
    // Hero wins iff the better hand folds and the worse hand calls: .8*.5 / (1-.8*.5).
    assert.ok(Math.abs(core.conditionalCalledEquity(info,response)-2/3)<1e-12);
    info.responseSamples.relations=[[0,0]];
    // Single caller (.1+.4) wins half, double caller (.1) wins a third.
    assert.ok(Math.abs(core.conditionalCalledEquity(info,response)-17/36)<1e-12);
    response.opponents[0]={seat:1,fold:0,call:1,raise:0};info.responseSamples.relations=[[-1,1]];
    assert.equal(core.conditionalCalledEquity(info,response),0);
});
test('expected future calls weight each stack separately and never add chips from an all-in seat',()=>{
    const ctx=core.normalizedContext({currentBet:100,publicPlayers:[{seat:1,roundBet:100,stack:0,allIn:true},{seat:2,roundBet:0,stack:1100}]});
    const response={anyRaise:0,anyCall:1,expectedCallers:1.1,opponents:[{seat:1,fold:0,call:1,raise:0},{seat:2,fold:.9,call:.1,raise:0}]};
    // Seat 2 has paid zero: its full 200 call is new money, not only the 100 raise increment.
    assert.ok(Math.abs(core.expectedAdditionalCalls(ctx,200,response)-20)<1e-12);
    ctx.publicPlayers[0]={seat:1,roundBet:0,stack:110};response.opponents[0]={seat:1,fold:.1,call:.9,raise:0};response.anyCall=.91;
    assert.ok(Math.abs(core.expectedAdditionalCalls(ctx,500,response)-149/.91)<1e-12);
    response.anyRaise=.75;response.anyCall=.2275;response.opponents.forEach(p=>{p.call*=.5;p.fold*=.5;p.raise=.5;});
    assert.ok(Math.abs(core.expectedAdditionalCalls(ctx,500,response)-149/.91)<1e-12);
});
