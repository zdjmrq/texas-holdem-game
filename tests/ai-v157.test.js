'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const core=require('../js/ai-core');
const probability=require('../js/probability-core');
const stableShortdeck=require('../js/ai-shortdeck-stable');
const {createAIPlayers,AIPlayer,STYLE_CONFIGS}=require('../js/ai');
const {ServerPokerGame,createServerAi,chooseServerAiAction}=require('../server/server-game');
const card=(rank,suit='spades')=>({rank,suit,value:({J:11,Q:12,K:13,A:14})[rank]||+rank});
function spot(n,hand,open=false,blind=80,seed='inspect'){
 const ps=Array.from({length:n},(_,seat)=>({seat,stack:200*blind,roundBet:open&&seat===0?3*blind:seat===1?blind/2:seat===2?blind:0,
   committed:open&&seat===0?3*blind:seat===1?blind/2:seat===2?blind:0,positionFromButton:seat,isSmallBlind:seat===1,isBigBlind:seat===2}));
 return {evaluateCards:cards=>probability.evaluate(cards,false),variant:'standard',heroSeat:3,handId:1,decisionId:'inspect',seed,
   holeCards:[card(hand[0]),card(hand[1],'hearts')],phase:'preflop',playerCount:n,positionFromButton:3,
   activeOpponentSeats:ps.filter(p=>p.seat!==3).map(p=>p.seat),publicPlayers:ps,stack:200*blind,effectiveStack:200*blind,
   pot:(open?4.5:1.5)*blind,currentBet:(open?3:1)*blind,toCall:(open?3:1)*blind,heroRoundBet:0,bigBlind:blind,wagerUnit:blind/2,
   minRaiseTo:(open?5:2)*blind,maxRaiseTo:200*blind,canRaise:true,canCheck:false,legalActions:['fold','call','raise','allin'],preflopRaiseCount:open?1:0,timeBudgetMs:500};
}
function brain(open=false){const b=new core.UnifiedPokerAI({seatId:3,style:'SOLID',profile:STYLE_CONFIGS.SOLID});
 if(open)b.observeAction({seatId:0,handId:1,street:'preflop',action:'raise',meta:{communityCards:[],potBefore:120,toCallBefore:80,betFraction:1.2,preflopRaiseCountBefore:0}});return b;}
test('production short-deck brains retain the validated stable policy across local, worker and server adapters',()=>{
 assert.ok(new core.UnifiedPokerAI({variant:'shortdeck'}) instanceof stableShortdeck.UnifiedPokerAI);
 const ps=[createServerAi(0,5000,()=>.1,'shortdeck'),createServerAi(1,5000,()=>.9,'shortdeck')];
 assert.ok(ps.every(p=>p.aiRef.sharedBrain instanceof stableShortdeck.UnifiedPokerAI));
 const game=new ServerPokerGame({startingStack:5000,ante:100,minBet:200,isShortDeck:true},ps);
 assert.ok(game.startHand());assert.ok(chooseServerAiAction(game,game.currentPlayerIdx));
 assert.ok(ps.every(p=>p.aiRef.sharedBrain instanceof stableShortdeck.UnifiedPokerAI));
});
test('full tables open premium hands without treating every unacted seat as a certain caller',()=>{
 for(const n of [6,10])for(const seed of ['inspect','premium-2','premium-3']){
  const d=brain().decide(spot(n,['A','A'],false,80,seed));
  assert.equal(d.action,'raise',`${n}/${seed}: ${d.action}`);assert.ok(d.amount>=160&&d.amount<=320);
  const defended=brain(true).decide(spot(n,['A','A'],true,80,seed));assert.notEqual(defended.action,'fold');
 }
});
test('a medium pair facing an ordinary opening retains a profitable flat rather than automatic three-betting',()=>{
 for(const n of [4,6]){const d=brain(true).decide(spot(n,['9','9'],true));assert.equal(d.action,'call');
   assert.ok(d.trace.candidates.find(c=>c.action==='call').calledEquity> d.trace.equity.mean);}
});
test('nominal blinds scale the same standard preflop decision and utility',()=>{
 const results=[2,20,80,200,800].map(blind=>({blind,d:brain().decide(spot(6,['A','K'],false,blind))}));
 for(const {blind,d} of results){assert.equal(d.action,results[0].d.action);assert.equal(d.amount/blind,results[0].d.amount/results[0].blind);
   assert.equal(d.trace.equity.mean,results[0].d.trace.equity.mean);assert.ok(Math.abs(d.trace.chosen.utility/blind-results[0].d.trace.chosen.utility/results[0].blind)<.004);}
});
test('position respects full-ring UTG, six-max HJ, blinds and heads-up button',()=>{
 const pos=(count,d,flags={})=>core.positionCategory({playerCount:count,positionFromButton:d,...flags});
 assert.equal(pos(10,3),'EP');assert.equal(pos(10,5),'EP');assert.equal(pos(10,6),'MP');assert.equal(pos(10,9),'LP');
 assert.equal(pos(6,3),'EP');assert.equal(pos(6,4),'MP');assert.equal(pos(6,5),'LP');assert.equal(pos(6,1,{isSmallBlind:true}),'BL');
 assert.equal(pos(2,0,{isSmallBlind:true}),'LP');assert.equal(pos(2,1,{isBigBlind:true}),'BL');
});
test('the ten-player 5000 100/200 preset opens normally and tracks current depth rather than always assuming 25 BB',()=>{
 const raw=spot(10,['A','A'],false,200);raw.startingStack=5000;raw.tablePlayerCount=10;raw.stack=5000;raw.effectiveStack=5000;raw.maxRaiseTo=5000;
 raw.publicPlayers.forEach(p=>p.stack=5000-p.roundBet);
 const d=brain().decide(raw);assert.equal(d.trace.parameters.strategyFocus,'ten-player-25bb');assert.equal(d.action,'raise');assert.ok(d.amount===400||d.amount===500);
 const other=brain().decide({...raw,startingStack:20000});assert.equal(other.trace.parameters.strategyFocus,'general');
 const ctx=core.normalizedContext(raw),b=brain();ctx.preflopParticipants=true;ctx.strategyFocus='ten-player-25bb';
 assert.ok(b.raiseTargets(ctx,{}).every(n=>n<=500));
 const deep={...ctx,effectiveStack:20000,heroStack:20000,maxRaiseTo:20000};assert.ok(b.raiseTargets(deep,{}).some(n=>n>500));
 const veryShort={...ctx,effectiveStack:2000,heroStack:2000,maxRaiseTo:2000};assert.ok(b.raiseTargets(veryShort,{}).includes(2000));
});
test('an all-in hero does not pay a fictional further raise penalty on either street',()=>{
 const b=brain(),ctx=core.normalizedContext({heroSeat:0,stack:5000,effectiveStack:5000,pot:300,bigBlind:200,currentBet:200,toCall:200,minRaiseTo:400,maxRaiseTo:5000,
   publicPlayers:[{seat:0,stack:5000},{seat:1,stack:10000,roundBet:200}],activeOpponentSeats:[1],canRaise:true});
 const reads=b.activeReads(ctx),eq={mean:.7,responseSamples:{seats:['1'],relations:[[1]],strengths:[[.8]]}};
 assert.equal(b.preflopResponse(ctx,5000,'raise',reads,eq).anyRaise,0);
 assert.equal(b.responseForRaise({...ctx,street:'flop'},5000,.7,{wetness:0},reads).anyRaise,0);
});
test('public raise levels update three-bet and four-bet ranges without misclassifying all-in calls',()=>{
 const belief=new core.OpponentBelief(1);
 belief.observe({action:'raise',street:'preflop',meta:{preflopRaiseCountBefore:1,aggressive:true}});
 assert.equal(belief.history[0].action,'3bet');assert.equal(belief.rangeWidth,.115);
 belief.observe({action:'raise',street:'preflop',meta:{preflopRaiseCountBefore:2,aggressive:true}});
 assert.equal(belief.history[1].action,'4bet');assert.equal(belief.rangeWidth,.055);
 belief.observe({action:'allin',street:'preflop',meta:{preflopRaiseCountBefore:3,aggressive:false}});
 assert.equal(belief.history[2].action,'call');
});
test('sample-dependent callers weight conditional equity by the chance that the sample continues',()=>{
 const info={mean:.5,responseSamples:{seats:['1'],relations:[[-1],[1]],strengths:[[.9],[.2]]}};
 const response={opponents:[{seat:1,continuationCutoff:.55}]};
 const hi=1/(1+Math.exp(-(.9-.55)*24)),lo=1/(1+Math.exp(-(.2-.55)*24));
 const expected=Math.max(.005,lo)/(Math.min(.995,hi)+Math.max(.005,lo));
 assert.ok(Math.abs(core.conditionalCalledEquity(info,response)-expected)<1e-12);
 assert.ok(core.conditionalCalledEquity(info,response)<.01);
});
test('observed opening and three-bet ranges learn from public opportunities with separate priors',()=>{
 const tight=new core.BehavioralModel(1),loose=new core.BehavioralModel(1);
 for(let h=1;h<=80;h++)for(const [model,action] of [[tight,h%20===0?'raise':'fold'],[loose,h%2===0?'raise':'fold']]){
  model.observe({handId:h,street:'preflop',action,meta:{preflopRaiseCountBefore:0,toCallBefore:80,positionFromButton:0,playerCount:6}});
 }
 assert.ok(tight.preflopRangeEstimate('open','LP',.32).width<.1);
 assert.ok(loose.preflopRangeEstimate('open','LP',.32).width>.45);
 assert.equal(tight.preflopRangeEstimate('3bet','LP',.115).confidence,0);
 const make=model=>{const belief=new core.OpponentBelief(1);belief.observe({action:'raise',street:'preflop',meta:{positionFromButton:0,playerCount:6}});
  belief.percentileWeight=1;belief.preflopSharpness=1;belief.adaptiveRangeWeight=.65;belief.publicStats=model;return belief;};
 const a=make(tight),b=make(loose),aa=[card('A'),card('A','hearts')],aq=[card('A'),card('Q','hearts')];
 assert.ok(a.comboWeight(...aa,'standard')/a.comboWeight(...aq,'standard')>b.comboWeight(...aa,'standard')/b.comboWeight(...aq,'standard'));
 const belief=new core.OpponentBelief(1);belief.observe({action:'call',street:'preflop'});assert.equal(belief.capped,true);
 belief.observe({action:'raise',street:'preflop',meta:{preflopRaiseCountBefore:1}});assert.equal(belief.capped,false);
});
test('new tables randomize style independently of names while a new hand preserves style and brain',()=>{
 const a=createAIPlayers(6,16000,()=>.01),b=createAIPlayers(6,16000,()=>.99);
 assert.deepEqual(a.map(p=>p.name),b.map(p=>p.name));assert.notDeepEqual(a.map(p=>p.style),b.map(p=>p.style));
 assert.equal(new Set(a.map(p=>p.style)).size,6);
 const first=createServerAi(1,16000,()=>.01),other=createServerAi(1,16000,()=>.99);
 assert.equal(first.name,other.name);assert.notEqual(first.aiRef.style,other.aiRef.style);
 const players=[first,createServerAi(0,16000),createServerAi(2,16000)];
 const game=new ServerPokerGame({startingStack:16000,smallBlind:40,bigBlind:80},players);
 const refs=players.map(p=>p.aiRef),styles=refs.map(p=>p.style);
 for(let hand=0;hand<2;hand++){
  players.forEach(p=>p.stack=16000);assert.ok(game.startHand());let guard=0;
  while(game.phase!=='idle'){assert.ok(++guard<100);const idx=game.currentPlayerIdx;const actions=game.legalActions(idx).actions;
   assert.ok(game.act(idx,actions.includes('fold')?'fold':'check').ok);}
  assert.deepEqual(players.map(p=>p.aiRef),refs);assert.deepEqual(players.map(p=>p.aiRef.style),styles);
 }
});
test('server decision input ignores other hole cards, actual deck order and opponent personality',()=>{
 const players=Array.from({length:6},(_,s)=>createServerAi(s,16000));
 const game=new ServerPokerGame({startingStack:16000,smallBlind:40,bigBlind:80},players,{random:()=>.37});game.startHand();
 const idx=game.currentPlayerIdx,own=players[idx].aiRef,views=[];
 own.sharedBrain.decide=raw=>{views.push(core.normalizedContext(raw));return {action:'fold',amount:0,trace:{}};};
 chooseServerAiAction(game,idx);
 game.deck.reverse();players.forEach((p,s)=>{if(s!==idx){p.holeCards=[card('A'),card('K')];p.aiRef.style='MANIAC';p.aiRef.config={bluffFreq:1};}});
 chooseServerAiAction(game,idx);
 const publicData=views.map(({evaluateCards,...data})=>data);
 assert.deepEqual(publicData[0],publicData[1]);
 const observed=JSON.stringify(views[1]);assert.ok(!observed.includes('MANIAC'));assert.ok(!observed.includes('bluffFreq'));
 const left=new core.UnifiedPokerAI({seatId:idx}),right=new core.UnifiedPokerAI({seatId:idx});
 assert.deepEqual(left.decide(views[0]),right.decide(views[1]));
});
