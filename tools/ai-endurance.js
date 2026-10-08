'use strict';
// Finite real-stack table lifecycle audit; chip results are not a strength rating.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {ServerPokerGame,createServerAi,chooseServerAiAction}=require('../server/server-game');
const core=require('../js/ai-core');
const args=Object.fromEntries(process.argv.slice(2).map(a=>{const [k,...v]=a.replace(/^--/,'').split('=');return [k,v.join('=')];}));
const tables=Math.max(1,Math.min(300,Number(args.tables)||48)),limit=Math.max(1,Math.min(1000,Number(args.hands)||128));
const rows=[];let decisions=0,hands=0,timeouts=0;const timing=[],sampling={};const precisionWeights=new Set(),rankedWeights=new Set(),jointWeights=new Set();
const overrides=args.profile?JSON.parse(fs.readFileSync(args.profile,'utf8')):null;let riverSolves=0;
if(overrides)for(const [k,v] of Object.entries(overrides))assert.ok(k in core.DEFAULT_PROFILE&&Number.isFinite(v)&&v>=0&&v<=1,'invalid profile');
const random=seed=>{let s=seed>>>0;return ()=>((s=(Math.imul(s,1664525)+1013904223)>>>0)/4294967296);};
const patterns=[[25,25,25,25,25,25,25,25,25,25],[10,25,50,100,10,25,50,100,25,25],[15,15,25,25,35,35,50,50,100,100]];
for(let table=0;table<tables;table++){
 const rng=random((Number(args.seed)||241001)+table),depths=patterns[table%patterns.length];
 const ps=Array.from({length:10},(_,seat)=>createServerAi(seat,depths[seat]*200,rng));
 if(overrides)ps.forEach((p,seat)=>{p.aiRef.config={...p.aiRef.config,...overrides};p.aiRef.sharedBrain=new core.UnifiedPokerAI({seatId:seat,style:p.aiRef.style,profile:p.aiRef.config});});
 const refs=ps.map(p=>p.aiRef),styles=refs.map(p=>p.style),chips=ps.reduce((s,p)=>s+p.stack,0);
 const game=new ServerPokerGame({startingStack:5000,smallBlind:100,bigBlind:200,isShortDeck:false},ps,{random:rng});
 const activeCounts=new Set();let played=0,actions=0;
 for(let hand=0;hand<limit;hand++){
  if(ps.filter(p=>p.stack>0).length<2)break;
  assert.ok(game.startHand());activeCounts.add(game.inHand().length);let guard=0;
  while(game.phase!=='idle'){
   assert.ok(++guard<300,'stalled hand');const seat=game.currentPlayerIdx,legal=game.legalActions(seat),start=performance.now(),d=chooseServerAiAction(game,seat);
   timing.push(performance.now()-start);const trace=ps[seat].aiRef.lastDecisionTrace;timeouts+=Number(!!trace.equity.timedOut);riverSolves+=Number(!!trace.riverSolver);precisionWeights.add(trace.parameters.targetPrecisionWeight);rankedWeights.add(trace.parameters.rankedContinuationWeight);jointWeights.add(trace.parameters.jointCallReturnWeight);
   const counts=sampling[game.phase]??={decisions:0,minSamples:Infinity,maxSamples:0,exact:0};counts.decisions++;counts.minSamples=Math.min(counts.minSamples,trace.equity.samples);counts.maxSamples=Math.max(counts.maxSamples,trace.equity.samples);counts.exact+=Number(!!trace.equity.exact);
   assert.ok(legal.actions.includes(d.action));assert.ok(Number.isSafeInteger(d.amount));if(d.action==='raise')assert.equal(d.amount%100,0);
   assert.equal(ps[seat].aiRef.lastDecisionTrace.parameters.strategyFocus,'ten-player-25bb');assert.ok(game.act(seat,d.action,d.amount).ok);decisions++;actions++;
  }
  assert.equal(ps.reduce((s,p)=>s+p.stack,0),chips);assert.deepEqual(ps.map(p=>p.aiRef),refs);assert.deepEqual(ps.map(p=>p.aiRef.style),styles);
  played++;hands++;
 }
 rows.push({table,depths,played,actions,activeCounts:[...activeCounts].sort((a,b)=>a-b),survivors:ps.filter(p=>p.stack>0).length,
  finalStacks:ps.map(p=>p.stack),styles,models:refs.map(p=>p.sharedBrain.models.size)});
 if((table+1)%4===0)console.log(JSON.stringify({tables:table+1,hands,decisions}));
}
timing.sort((a,b)=>a-b);const out=path.resolve(args.out||'ai-endurance.json');fs.mkdirSync(path.dirname(out),{recursive:true});
fs.writeFileSync(out,JSON.stringify({tables,hands,decisions,errors:0,timeouts,riverSolves,profile:overrides,precisionWeights:[...precisionWeights],rankedWeights:[...rankedWeights],jointWeights:[...jointWeights],sampling,timing:{averageMs:timing.reduce((s,v)=>s+v,0)/timing.length,p95Ms:timing[Math.floor(timing.length*.95)],maxMs:timing.at(-1)},rows},null,2));
console.log('FINITE_TABLE_LIFECYCLE_COMPLETE');
