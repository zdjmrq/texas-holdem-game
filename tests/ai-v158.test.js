'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const core=require('../js/ai-core'),probability=require('../js/probability-core');
const {STYLE_CONFIGS}=require('../js/ai');
const card=(rank,suit)=>({rank,suit,value:({J:11,Q:12,K:13,A:14})[rank]||+rank});
const board=[card('Q','clubs'),card('9','diamonds'),card('6','clubs'),card('J','spades'),card('2','hearts')];
function river(){return core.normalizedContext({variant:'standard',phase:'river',heroSeat:0,handId:1,decisionId:'test',holeCards:[card('A','spades'),card('Q','hearts')],communityCards:board,
 publicPlayers:[{seat:0,stack:5000,committed:500},{seat:1,stack:5000,committed:500}],activeOpponentSeats:[1],stack:5000,effectiveStack:5000,pot:1000,bigBlind:200,wagerUnit:100,currentBet:0,toCall:0,heroRoundBet:0,minRaiseTo:200,maxRaiseTo:5000,canCheck:true,canRaise:true,legalActions:['check','raise','allin'],evaluateCards:cards=>probability.evaluate(cards,false)});}
test('joint caller payments preserve covariance, ties and hero side-pot cap',()=>{
 const ctx=river();ctx.publicPlayers.push({seat:2,stack:100,committed:500,roundBet:0});
 const info={mean:.5,responseSamples:{seats:['1','2'],relations:[[1,-1],[-1,1]],weights:[1,1]}};
 const r={opponents:[{seat:1,call:.5,fold:.5},{seat:2,call:.5,fold:.5}]};
 // Short strong caller 2 competes for only the first 100, not the extra 900 side pot.
 assert.ok(Math.abs(core.jointAdditionalReturn(ctx,1000,info,r)-(.25*100+.5*900+.25*100)/1.5)<1e-10);
 const tie={mean:.5,responseSamples:{seats:['1'],relations:[[0]],weights:[1]}};
 assert.equal(core.jointAdditionalReturn(ctx,1000,tie,{opponents:[{seat:1,forceContinue:true}]}),500);
 ctx.publicPlayers[0].stack=50;
 assert.equal(core.jointAdditionalReturn(ctx,50,tie,{opponents:[{seat:1,forceContinue:true}]}),25);
});
test('conditional exact river equity preserves the original unequal combination weights',()=>{
 const info={mean:.1,responseSamples:{seats:['1'],relations:[[1],[-1]],weights:[9,1],selectionScores:[[.2],[.9]]}};
 assert.equal(core.conditionalCalledEquity(info,{opponents:[{seat:1,forceContinue:true}]}),.9);
});
test('joint caller return matches exhaustive independent action branches with unequal stacks',()=>{
 const ctx=river();ctx.publicPlayers=[{seat:0,committed:50,stack:800,roundBet:50},{seat:1,committed:20,stack:70,roundBet:20},{seat:2,committed:50,stack:1200,roundBet:50},{seat:3,committed:0,stack:320,roundBet:0}];
 const seats=['1','2','3'],ps=[.2,.7,.45],r={opponents:seats.map((seat,i)=>({seat,call:ps[i],fold:1-ps[i]}))};
 for(const relations of [[-1,1,0],[0,0,0],[1,-1,1],[-1,-1,-1],[1,1,1]]){
  let amount=0,mass=0;
  for(let mask=1;mask<8;mask++){
   const calls=ps.map((p,i)=>!!(mask&(1<<i))),q=ps.reduce((a,p,i)=>a*(calls[i]?p:1-p),1);mass+=q;
   for(let j=0;j<3;j++)if(calls[j]){
    const p=ctx.publicPlayers[j+1],end=Math.min(700,p.committed+p.stack,p.committed+700-p.roundBet);
    for(let level=p.committed+1;level<=end;level++){
     const eligible=calls.map((called,i)=>called&&ctx.publicPlayers[i+1].committed+ctx.publicPlayers[i+1].stack>=level);
     const beaten=relations.some((v,i)=>eligible[i]&&v===-1);
     const ties=relations.filter((v,i)=>eligible[i]&&v===0).length;
     if(!beaten)amount+=q/(ties+1);
    }
   }
  }
  const eq={mean:.5,responseSamples:{seats,relations:[relations],weights:[3]}};
  assert.ok(Math.abs(core.jointAdditionalReturn(ctx,700,eq,r)-amount/mass)<1e-8);
 }
});
test('postflop continuation selects strong current-board combinations and excludes future-card information',()=>{
 const b=new core.UnifiedPokerAI({seatId:0,profile:{postflopSelectionWeight:.65}}),ctx=river(),reads=b.activeReads(ctx);
 const eq={mean:.5,responseSamples:{seats:['1'],relations:[[1],[-1]],selectionScores:[[.1],[.95]],weights:[1,1]}};
 const response=b.postflopResponse(ctx,800,'raise',reads,eq,core.analyzeTexture(board,ctx.holeCards));
 assert.ok(core.conditionalCalledEquity(eq,response)<.3,'a caller must have a stronger subset than the entire range');
 assert.ok(Math.abs(response.allFold+response.anyCall+response.anyRaise-1)<1e-12);
 const forced={...ctx,publicPlayers:ctx.publicPlayers.map(p=>p.seat===1?{...p,allIn:true,stack:0}:p)};
 const r=b.postflopResponse(forced,800,'raise',reads,eq,{});assert.equal(r.allFold,0);assert.equal(r.anyRaise,0);assert.equal(r.anyCall,1);
 assert.equal(core.conditionalCalledEquity(eq,r),.5);
});
test('real exact river simulation exports weighted hypothetical combinations without sampling private hands',()=>{
 const ctx=river();ctx.sampleResponses=true;
 const b=new core.UnifiedPokerAI({seatId:0});b.activeReads(ctx);
 const eq=core.estimateRangeEquity(ctx,b.beliefs,new core.SeededRng('exact'));
 assert.equal(eq.exact,true);assert.equal(eq.responseSamples.relations.length,eq.samples);assert.equal(eq.responseSamples.weights.length,eq.samples);
 const fixed={opponents:[{seat:1,forceContinue:true}]};assert.ok(Math.abs(core.conditionalCalledEquity(eq,fixed)-eq.mean)<1e-12);
 const flop={...ctx,street:'flop',communityCards:board.slice(0,3),mcSamples:120};
 const sampled=core.estimateRangeEquity(flop,b.beliefs,new core.SeededRng('flop'));
 assert.ok(sampled.responseSamples.selectionScores.every(row=>row.every(n=>n>=0&&n<=1)));
});
test('professional styles retain differences without the deliberately passive first-in limps',()=>{
 const profiles=Object.keys(core.PROFILE_OVERRIDES).map(style=>new core.UnifiedPokerAI({style,profile:{professionalWeight:1}}).profile);
 assert.ok(profiles.every(p=>p.vpip>=.2&&p.vpip<=.33&&p.pfr>=.9&&p.aggression>=.54));assert.ok(new Set(profiles.map(p=>p.vpip)).size>3);
});
test('target style discipline preserves names/styles and leaves other initial configurations unchanged',()=>{
 for(const style of Object.keys(core.PROFILE_OVERRIDES)){
  const b=new core.UnifiedPokerAI({style,profile:{...STYLE_CONFIGS[style],competenceWeight:1}});
  const target={...river(),phase:'river',startingStack:5000,tablePlayerCount:10};
  const d=b.decide(target);assert.equal(d.trace.parameters.competenceWeight,1);assert.equal(b.style,style);
  if(style==='TAG'||style==='LAG'||style==='SOLID')assert.equal(b.profile.pfr,STYLE_CONFIGS[style].pfr);
  else assert.ok(b.profile.pfr>=.82);
  const general=b.decide({...target,startingStack:20000,tablePlayerCount:4});
  assert.equal(general.trace.parameters.competenceWeight,0);assert.equal(b.profile.pfr,STYLE_CONFIGS[style].pfr);
 }
});
test('conditional risk counts continuing opponents and removes risk on the all-fold branch',()=>{
 const b=new core.UnifiedPokerAI({seatId:0}),ctx=river();ctx.numOpponents=9;ctx.canCheck=false;ctx.toCall=200;ctx.currentBet=200;ctx.legalActions=['fold','call','raise'];ctx.preflopParticipants=true;
 const response={allFold:.6,anyCall:.4,anyRaise:0,expectedCallers:2,fraction:.5,credibility:.5,opponents:[{seat:1,forceContinue:true}]};
 b.preflopResponse=()=>response;b.raiseTargets=()=>[600];b.riskPenalty=(c)=>c.numOpponents*10;
 const info={mean:.5,stdDev:.1,standardError:.01,samples:240,rangeConfidence:.3,responseSamples:{seats:['1'],relations:[[1],[-1]],weights:[1,1]}};
 const texture=core.analyzeTexture(board,ctx.holeCards),old=b.buildPostflopCandidates(ctx,info,texture,[]),fixed=b.buildPostflopCandidates({...ctx,conditionalRiskWeight:1},info,texture,[]);
 assert.equal(old.find(c=>c.action==='call').risk,90);assert.equal(fixed.find(c=>c.action==='call').risk,20);
 assert.equal(old.find(c=>c.action==='raise').risk,90);assert.equal(fixed.find(c=>c.action==='raise').risk,8);
 response.allFold=0;response.anyCall=1;assert.equal(b.buildPostflopCandidates({...ctx,conditionalRiskWeight:1},info,texture,[]).find(c=>c.action==='raise').risk,20);
});
test('target precision doubles or quadruples fixed samples only for the requested initial table',()=>{
 const ctx={...river(),phase:'flop',street:'flop',communityCards:board.slice(0,3),startingStack:5000,tablePlayerCount:10,timeBudgetMs:160};
 const baseline=new core.UnifiedPokerAI({seatId:0,profile:{targetPrecisionWeight:0}}).decide(ctx);assert.equal(baseline.trace.equity.samples,420);
 for(const weight of [1/3,1]){
  const b=new core.UnifiedPokerAI({seatId:0,profile:{targetPrecisionWeight:weight,conditionalRiskWeight:1}}),d=b.decide(ctx);
  assert.equal(d.trace.equity.samples,weight===1?1680:840);assert.equal(d.trace.equity.timedOut,false);assert.equal(d.trace.parameters.conditionalRiskWeight,1);
  const general=b.decide({...ctx,tablePlayerCount:4,startingStack:20000});assert.equal(general.trace.equity.samples,420);assert.equal(general.trace.parameters.targetPrecisionWeight,0);assert.equal(general.trace.parameters.conditionalRiskWeight,0);
 }
});
test('terminal calls realize all contestable equity and retain future factors while betting remains possible',()=>{
 const b=new core.UnifiedPokerAI({seatId:0}),ctx={...river(),street:'flop',communityCards:board.slice(0,3),pot:3000,heroStack:1000,toCall:1000,currentBet:1000,canCheck:false,canRaise:false,legalActions:['fold','call'],terminalRealizationWeight:1,
  publicPlayers:[{seat:0,committed:1000,stack:1000,roundBet:0},{seat:1,committed:2000,stack:0,roundBet:1000,allIn:true}]};
 b.riskPenalty=()=>0;const info={mean:.3,stdDev:.1,standardError:.01,samples:240},texture={wetness:0,hasStrongDraw:false};
 assert.equal(b.buildPostflopCandidates(ctx,info,texture,[]).find(c=>c.action==='allin').ev,200);
 assert.equal(b.realizationFactor({...ctx,heroStack:5000},texture,[],1000),1,'an all-in opponent cannot force a later fold');
 const live={...ctx,heroStack:5000,activeOpponentSeats:[1,2],numOpponents:2,publicPlayers:[...ctx.publicPlayers,{seat:2,committed:0,stack:5000,roundBet:0}]};
 assert.ok(b.realizationFactor(live,texture,[],1000)<1,'a remaining live opponent can still bet');
 assert.equal(b.realizationFactor(live,texture,[],5000),1,'hero all-in guarantees its equity reaches showdown');
 assert.ok(b.realizationFactor({...live,publicPlayers:ctx.publicPlayers},texture,[],1000)<1,'missing public seats are not assumed all-in');
 assert.equal(b.realizationFactor({...live,street:'river'},texture,[],1000),1);
 const target=new core.UnifiedPokerAI({seatId:0,profile:{terminalRealizationWeight:1}});
 assert.equal(target.decide({...river(),startingStack:5000,tablePlayerCount:10}).trace.parameters.terminalRealizationWeight,1);
 assert.equal(target.decide({...river(),startingStack:20000,tablePlayerCount:4}).trace.parameters.terminalRealizationWeight,0);
});
test('ranked continuation keeps weighted marginal rates and ties, without using simulated future outcomes',()=>{
 const ctx={...river(),rankedContinuationWeight:1},b=new core.UnifiedPokerAI({seatId:0}),reads=b.activeReads(ctx);
 const eq={mean:.9,responseSamples:{seats:['1'],relations:[[1],[1],[-1]],currentScores:[[10],[10],[20]],currentDraws:[[0],[0],[0]],weights:[1,8,1]}};
 const old=b.responseForRaise(ctx,600,eq.mean,{},reads),r=b.rankedPostflopResponse(ctx,600,'raise',reads,eq,{}),q=r.opponents[0].continuationProbabilities;
 assert.deepEqual(eq.responseSamples.currentPercentiles,[[.45],[.45],[.95]]);assert.equal(q[0],q[1]);assert.ok(q[2]>q[0]);
 assert.ok(Math.abs((q[0]+8*q[1]+q[2])/10-old.opponents[0].call/(1-old.opponents[0].raise))<1e-4);
 assert.ok(Math.abs(r.allFold+r.anyCall+r.anyRaise-1)<1e-12);
 eq.responseSamples.relations=[[-1],[-1],[1]];assert.deepEqual(b.rankedPostflopResponse(ctx,600,'raise',reads,eq,{}).opponents[0].continuationProbabilities,q);
 const forced={...ctx,publicPlayers:ctx.publicPlayers.map(p=>p.seat===1?{...p,allIn:true,stack:0}:p)};
 const f=b.rankedPostflopResponse(forced,600,'raise',reads,eq,{});assert.equal(f.allFold,0);assert.equal(f.anyRaise,0);assert.equal(f.anyCall,1);
});
test('ranked continuation uses present board scores and is limited to the requested initial configuration',()=>{
 const raw={...river(),startingStack:5000,tablePlayerCount:10,phase:'flop',street:'flop',communityCards:board.slice(0,3),timeBudgetMs:180};
 const b=new core.UnifiedPokerAI({seatId:0,profile:{rankedContinuationWeight:.65}}),d=b.decide(raw);assert.equal(d.trace.parameters.rankedContinuationWeight,.65);assert.equal(d.trace.equity.samples,420);
 assert.equal(b.decide({...raw,tablePlayerCount:4,startingStack:20000}).trace.parameters.rankedContinuationWeight,0);
 const c=core.normalizedContext({...raw,sampleResponses:true});c.sampleResponses=true;c.rankedContinuationWeight=1;
 const info=core.estimateRangeEquity(c,b.beliefs,new core.SeededRng('current-board'));
 assert.equal(info.responseSamples.currentScores.length,info.samples);assert.ok(info.responseSamples.currentScores.every(row=>row.every(Number.isFinite)));assert.ok(info.responseSamples.currentDraws.every(row=>row.every(v=>v>=0&&v<=1)));
});

test('terminal call decisions follow an independent pot-odds oracle across prices and equities',()=>{
 const brain=new core.UnifiedPokerAI({seatId:0});brain.plan={continuityBonus:()=>-100000};
 for(const pot of [200,1000,3000,10000])for(const cost of [100,500,2000])for(const equity of [.05,.2,.3,.5,.8,.99]){
  const ctx={...river(),street:'flop',communityCards:board.slice(0,3),pot,heroStack:cost,toCall:cost,currentBet:cost,heroRoundBet:0,
   legalActions:['fold','call'],canCheck:false,canRaise:false,terminalCallWeight:1,
   publicPlayers:[{seat:0,committed:pot/2,stack:cost,roundBet:0},{seat:1,committed:pot/2,stack:cost,roundBet:cost}]};
  const info={mean:equity,stdDev:.49,standardError:.09,rangeConfidence:.1};
  const candidates=brain.buildPostflopCandidates(ctx,info,{wetness:1},[]),call=candidates.find(c=>c.terminalCall);
  const expected=(pot+cost)*equity-cost;
  assert.ok(Math.abs(call.ev-expected)<1e-9);assert.equal(call.risk,0);assert.equal(call.planBonus,0);
  const selected=brain.selectCandidate(candidates,ctx,new core.SeededRng('terminal'));
  assert.equal(selected.action,expected>1e-9?'allin':'fold');
 }
});

test('terminal classification requires known public seats and distinguishes a closing river call from a possible squeeze',()=>{
 const brain=new core.UnifiedPokerAI({seatId:0}),ctx={...river(),toCall:100,currentBet:100,heroStack:5000,canCheck:false,terminalCallWeight:1,riverCallWeight:1};
 ctx.publicPlayers[1].roundBet=100;assert.equal(brain.terminalCallState(ctx),'river-close');
 ctx.publicPlayers.push({seat:2,stack:5000,roundBet:0});ctx.activeOpponentSeats=[1,2];assert.equal(brain.terminalCallState(ctx),null);
 ctx.publicPlayers[2].roundBet=100;assert.equal(brain.terminalCallState(ctx),'river-close');
 ctx.street='turn';assert.equal(brain.terminalCallState(ctx),null);
 ctx.publicPlayers.slice(1).forEach(p=>{p.stack=0;p.allIn=true;});assert.equal(brain.terminalCallState(ctx),'allin');
 ctx.activeOpponentSeats.push(3);assert.equal(brain.terminalCallState(ctx),null);
});

test('terminal call bypasses a fixed starting range gate while still rejecting a bad price',()=>{
 const brain=new core.UnifiedPokerAI({seatId:0});brain.selectCandidate=(cs)=>cs;
 const ctx={...river(),street:'preflop',communityCards:[],toCall:100,heroStack:100,heroRoundBet:0,currentBet:500,canCheck:false,canRaise:false,
  holeCards:[card('7','spades'),card('2','hearts')],legalActions:['fold','call'],preflopRaiseCount:3,terminalCallWeight:1};
 const result=brain.preflopDecision(ctx,new core.SeededRng('cheap-short-call'),brain.activeReads(ctx));
 assert.ok(result.candidates.some(c=>c.terminalCall),'the actual price, not a fixed 4-bet range, controls a forced final call');
 const target=new core.UnifiedPokerAI({seatId:0,profile:{terminalCallWeight:1,riverCallWeight:1}});
 assert.equal(target.decide({...river(),startingStack:5000,tablePlayerCount:10}).trace.parameters.terminalCallWeight,1);
 assert.equal(target.decide({...river(),startingStack:20000,tablePlayerCount:4}).trace.parameters.terminalCallWeight,0);
});
