'use strict';
const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
const {CompiledCfr}=require('../js/compiled-cfr'),reference=require('../tools/tabular-cfr'),river=require('../js/river-solver'),core=require('../js/ai-core'),prob=require('../js/probability-core');
const board=[['K','clubs'],['8','diamonds'],['3','hearts'],['2','spades'],['J','clubs']].map(([rank,suit])=>({rank,suit}));
const deals=Array.from({length:6},(_,i)=>({scores:[i,5-i],weight:(i+1)/21,showdownSign:Math.sign(i-2.5)}));
function context(overrides={}){return {variant:'standard',phase:'river',heroSeat:0,handId:1,decisionId:'cfr',seed:'cfr',holeCards:[{rank:'A',suit:'spades'},{rank:'Q',suit:'hearts'}],communityCards:board,
 publicPlayers:[{seat:0,stack:4500,committed:500,roundBet:0,positionFromButton:1},{seat:1,stack:4500,committed:500,roundBet:0,positionFromButton:0}],activeOpponentSeats:[1],stack:4500,effectiveStack:4500,pot:1000,bigBlind:200,wagerUnit:100,currentBet:0,toCall:0,heroRoundBet:0,minRaiseTo:200,maxRaiseTo:4500,canCheck:true,canRaise:true,legalActions:['check','raise','allin'],evaluateCards:cards=>prob.evaluate(cards,false),tablePlayerCount:10,startingStack:5000,playerCount:10,...overrides};}
test('compiled CFR matches the independent full-tree oracle and Kuhn equilibrium',()=>{
 for(const game of [reference.kuhnGame(),reference.riverGame({pot:1000,stacks:[4500,4500],deals}),river.game({pot:1600,paid:[100,600],currentBet:600,minRaiseTo:1100,stacks:[4300,3800],minBet:200,deals})]){
  const slow=new reference.TabularCfr(game).run(400).policy(),fast=new CompiledCfr(game).run(400).policy();assert.deepEqual(Object.keys(fast).sort(),Object.keys(slow).sort());
  for(const [key,node] of Object.entries(slow)){assert.deepEqual(fast[key].actions,node.actions);node.probabilities.forEach((v,i)=>assert.ok(Math.abs(v-fast[key].probabilities[i])<1e-10));}
 }
 const g=reference.kuhnGame(),a=reference.audit(g,new CompiledCfr(g).run(10000).policy());assert.ok(Math.abs(a.value+1/18)<.001);assert.ok(a.exploitability<.005);
});
test('runtime river public ranges reproduce the independent card-removal abstraction',()=>{
 const slow=require('../tools/river-range-abstraction').buildRiverAbstraction({board,buckets:6}),fast=river.ranges({board,deck:core.buildDeck('standard'),weights:()=>[1,1]});
 assert.equal(fast.combinations,1081);assert.deepEqual(fast.bucketByScore,slow.bucketByScore);
 const keyed=new Map(slow.deals.map(d=>[d.scores.join(':'),d]));for(const d of fast.deals){const s=keyed.get(d.scores.join(':'));assert.ok(Math.abs(d.weight-s.weight)<1e-12);assert.ok(Math.abs(d.showdownSign-s.showdownSign)<1e-12);}
});
test('river state adapter accounts for prior bets, closes a check and enforces integer raise sizes',()=>{
 const g=river.game({pot:1600,paid:[100,600],currentBet:600,minRaiseTo:1100,stacks:[4300,3800],minBet:200,deals});const s=g.deals()[0][0];
 assert.equal(g.payoff(g.child(s,'fold')),-550);assert.equal(g.payoff(g.child(s,'call')),-1050); // delta from folding = -500
 for(const a of g.actions(s).filter(a=>a.includes(':'))){const amount=Number(a.split(':')[1]);assert.ok(amount>=1100);assert.equal(amount%100,0);}
 const checked=river.game({pot:1000,stacks:[4500,4500],checkedBefore:true,deals});assert.ok(checked.terminal(checked.child(checked.deals()[0][0],'check')));
 const first=river.game({pot:1000,stacks:[4500,4500],deals});assert.equal(first.terminal(first.child(first.deals()[0][0],'check')),false);
 const odd=river.game({pot:1000,stacks:[1473,4500],deals});assert.ok(odd.actions(odd.deals()[0][0]).includes('bet:1473'));
});
test('real Bot river solve uses public observations and ignores hidden cards, deck and personalities',()=>{
 const make=extra=>{const brain=new core.UnifiedPokerAI({seatId:0,profile:{riverCfrWeight:1}});brain.observeAction({seatId:1,handId:1,street:'river',action:'check',meta:{communityCards:board}});return brain.decide(context(extra));};
 const a=make(),b=make({opponentCards:[{rank:'A',suit:'clubs'},{rank:'A',suit:'hearts'}],deck:[{rank:'2',suit:'diamonds'}],opponentPersonality:'MANIAC',publicPlayers:context().publicPlayers.map(p=>({...p,holeCards:[{rank:'A',suit:'clubs'}],style:'MANIAC'}))});
 assert.ok(a.trace.riverSolver);assert.equal(a.trace.riverSolver.iterations,800);assert.equal(a.trace.riverSolver.checkedBefore,true);assert.deepEqual(a.trace.riverSolver.policy,b.trace.riverSolver.policy);assert.deepEqual([a.action,a.amount],[b.action,b.amount]);
 const g=river.game({pot:1000,stacks:[4500,4500],deals:[{scores:[2,1],weight:.5,showdownSign:1},{scores:[2,4],weight:.5,showdownSign:-1}]});assert.equal(g.key(g.deals()[0][0],0),g.key(g.deals()[1][0],0));assert.notEqual(g.key(g.deals()[0][0],1),g.key(g.deals()[1][0],1));
});
test('own public range is separate from opponent beliefs and resets between hands',()=>{
 const b=new core.UnifiedPokerAI({seatId:0,profile:{riverCfrWeight:1}}),event={seatId:0,handId:1,street:'preflop',action:'raise',meta:{positionFromButton:0,playerCount:10}};b.observeAction(event);
 assert.ok(b.publicSelfBelief.history.length);assert.equal(b.beliefs.has('0'),false);b.beginHand(2);assert.equal(b.publicSelfBelief.history.length,0);
 const off=new core.UnifiedPokerAI({seatId:0,profile:{riverCfrWeight:0}});off.observeAction(event);assert.equal(off.publicSelfBelief,undefined);
 const a=require('../js/ai').AIPlayer;const wrapper=new a('Test','SOLID',5000,0);wrapper.sharedBrain=b;wrapper.recordOpponentAction(0,'raise','preflop',2,event.meta);assert.equal(b.publicSelfBelief.history.length,1);assert.equal(wrapper.playerModels[0],undefined);
});
test('river CFR is scoped to the ten-player preset, and rejects side pots and all-in terminals',()=>{
 for(const extra of [{startingStack:10000},{tablePlayerCount:6},{phase:'turn',communityCards:board.slice(0,4)},{activeOpponentSeats:[1,2]},{publicPlayers:[{seat:0,stack:4500,committed:500},{seat:1,stack:0,committed:500,allIn:true}]},{publicPlayers:[{seat:0,stack:4500,committed:500},{seat:1,stack:4300,committed:700}]}]){const b=new core.UnifiedPokerAI({seatId:0,profile:{riverCfrWeight:1}}),ctx=core.normalizedContext(context(extra));ctx.strategyFocus=extra.startingStack||extra.tablePlayerCount?'general':'ten-player-25bb';assert.equal(b.solveRiver(ctx,new core.SeededRng(1),b.activeReads(ctx)),null);}
 assert.equal(core.DEFAULT_PROFILE.riverCfrWeight,0);
 assert.equal(core.VARIANT_PROFILE_OVERRIDES.standard.riverCfrWeight,1);
 const production=new core.UnifiedPokerAI({seatId:0}).decide(context());assert.ok(production.trace.riverCfrEnabled);assert.ok(production.trace.riverSolver);
 const disabled=new core.UnifiedPokerAI({seatId:0,profile:{riverCfrWeight:0}}).decide(context());assert.equal(disabled.trace.riverCfrEnabled,false);assert.equal(disabled.trace.riverSolver,undefined);
});
test('browser and Node execute identical compiled solver and Bot policy',()=>{
 const sandbox={console,performance};sandbox.globalThis=sandbox;vm.createContext(sandbox);for(const file of ['game-rules-core.js','probability-core.js','ai-shortdeck-stable.js','compiled-cfr.js','river-solver.js','ai-core.js'])vm.runInContext(fs.readFileSync(path.join(__dirname,'../js',file),'utf8'),sandbox,{filename:file});
 const a=new core.UnifiedPokerAI({seatId:0,profile:{riverCfrWeight:1}}).decide(context()),b=new sandbox.PokerAICore.UnifiedPokerAI({seatId:0,profile:{riverCfrWeight:1}}).decide(context());assert.deepEqual([a.action,a.amount],[b.action,b.amount]);assert.deepEqual(a.trace.riverSolver.policy,JSON.parse(JSON.stringify(b.trace.riverSolver.policy)));
});
test('covering an odd opponent stack stays terminal after legal raise-grid rounding',()=>{
 const saved=river.solve;
 try{
  const raw=context({publicPlayers:[{seat:0,stack:4500,committed:500,roundBet:0},{seat:1,stack:1423,committed:500,roundBet:0}]}),score=prob.evaluate([...raw.holeCards,...board],false).score;
  river.solve=()=>({bucketByScore:{[score]:5},policy:{'0:5:':{player:0,actions:['bet:1423'],probabilities:[1]}},rootValues:{'0:5:':[500]},iterations:800,informationSets:1,treeNodes:1,elapsedMs:0});
  const brain=new core.UnifiedPokerAI({seatId:0}),ctx=core.normalizedContext(raw);ctx.strategyFocus='ten-player-25bb';const selected=brain.solveRiver(ctx,new core.SeededRng(1),brain.activeReads(ctx)),legal=brain.legalize(selected.decision,ctx);
  assert.equal(legal.action,'raise');assert.equal(legal.amount,1500);assert.equal(legal.amount%100,0);assert.ok(legal.amount>=1423,'must cover villain rather than leave 23 chips');
 }finally{river.solve=saved;}
});
