'use strict';
// Exact best-response audit of the bounded runtime game, not a table win rate.
const fs=require('node:fs'),path=require('node:path'),core=require('../js/ai-core'),river=require('../js/river-solver'),{CompiledCfr}=require('../js/compiled-cfr'),{audit}=require('./tabular-cfr');
const args=Object.fromEntries(process.argv.slice(2).map(a=>{const [k,...v]=a.replace(/^--/,'').split('=');return [k,v.join('=')];}));
const textures={dry:[['K','clubs'],['8','diamonds'],['3','hearts'],['2','spades'],['J','clubs']],wet:[['Q','clubs'],['9','diamonds'],['6','clubs'],['J','spades'],['2','hearts']],paired:[['K','clubs'],['K','diamonds'],['8','hearts'],['4','spades'],['2','clubs']],flush:[['A','clubs'],['9','clubs'],['6','clubs'],['J','spades'],['2','hearts']]};
const name=args.case||'dry';if(!textures[name])throw Error('unknown case');const board=textures[name].map(([rank,suit])=>({rank,suit}));
const start=performance.now(),abstract=river.ranges({board,deck:core.buildDeck('standard'),weights:()=>[1,1]}),buildMs=performance.now()-start,results=[];
for(const [position,options] of Object.entries({first:{pot:1000,stacks:[4500,4500]},checked:{pot:1000,stacks:[4500,4500],checkedBefore:true},facing:{pot:1600,stacks:[4500,3900],paid:[0,600],currentBet:600,minRaiseTo:1200}})){
 const game=river.game({...options,minBet:200,unit:100,deals:abstract.deals}),solver=new CompiledCfr(game),checkpoints=[];
 for(const target of [200,800,10000]){const tick=performance.now();solver.run(target-solver.iterations);const solveMs=performance.now()-tick;checkpoints.push({iterations:target,solveMs,...audit(game,solver.policy())});}
 results.push({position,informationSets:solver.infos.length,treeNodes:solver.infoIndex.length,checkpoints});
}
const result={case:name,buildMs,elapsedMs:performance.now()-start,results,scope:'Exact consistent-information-set best responses in a six-bucket, heads-up river game; independent public ranges; not ten-player NLHE exploitability or actual table strength.'},out=path.resolve(args.out||`river-runtime-audit-${name}.json`);fs.mkdirSync(path.dirname(out),{recursive:true});fs.writeFileSync(out,JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result,null,2));
