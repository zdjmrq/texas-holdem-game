'use strict';
// Finite offline method study. Results describe this bounded abstract game,
// not the strength of the desktop Bot or a solved ten-player strategy.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {buildRiverAbstraction}=require('./river-range-abstraction'),{TabularCfr,riverGame,audit}=require('./tabular-cfr');
const args=Object.fromEntries(process.argv.slice(2).map(a=>{const [k,...v]=a.replace(/^--/,'').split('=');return [k,v.join('=')];}));
const textures={dry:[['K','clubs'],['8','diamonds'],['3','hearts'],['2','spades'],['J','clubs']],wet:[['Q','clubs'],['9','diamonds'],['6','clubs'],['J','spades'],['2','hearts']],paired:[['K','clubs'],['K','diamonds'],['8','hearts'],['4','spades'],['2','clubs']],flush:[['A','clubs'],['9','clubs'],['6','clubs'],['J','spades'],['2','hearts']]};
const name=args.case||'dry';assert.ok(textures[name]);const iterations=Number(args.iterations)||1000;assert.ok(Number.isSafeInteger(iterations)&&iterations>=100&&iterations<=10000);
const board=textures[name].map(([rank,suit])=>({rank,suit})),events=[{seatId:1,handId:1,street:'preflop',action:'raise',meta:{preflopRaiseCountBefore:0,positionFromButton:0,playerCount:10,potBefore:300,toCallBefore:200,betFraction:1.5}},
 {seatId:0,handId:1,street:'preflop',action:'call',meta:{positionFromButton:1,playerCount:10,potBefore:700,toCallBefore:300,betFraction:.6}}];
const start=performance.now(),abstract=buildRiverAbstraction({board,publicEvents:events,buckets:6}),buildMs=performance.now()-start;
const game=riverGame({pot:1000,stacks:[4500,4500],deals:abstract.deals,unit:100,minBet:200,maxRaises:1});
const solver=new TabularCfr(game),checkpoints=[];
for(const target of [...new Set([Math.floor(iterations/4),iterations])]){solver.run(target-solver.iterations);checkpoints.push({iterations:solver.iterations,elapsedMs:performance.now()-start,...audit(game,solver.policy())});}
const policy=solver.policy(),root=Object.fromEntries(Object.entries(policy).filter(([key])=>key.endsWith(':')));
const result={case:name,board,iterations,buildMs,elapsedMs:performance.now()-start,informationSets:solver.nodes.size,compatiblePairs:abstract.compatiblePairs,abstractDeals:abstract.deals.length,checkpoints,root,policy,
 scope:'Heads-up river only; six private strength buckets; public action priors, exact card-removal joint weights and conditional showdown values; at most one raise. Method study, not ten-player GTO or delivered Bot strength.'};
const out=path.resolve(args.out||`river-cfr-${name}.json`);fs.mkdirSync(path.dirname(out),{recursive:true});fs.writeFileSync(out,JSON.stringify(result,null,2)+'\n');
const {policy:_,root:__,...summary}=result;console.log(JSON.stringify(summary,null,2));
