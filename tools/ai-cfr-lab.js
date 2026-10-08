'use strict';
// A method audit, not a trained ten-player Hold'em policy or a strength result.
const fs=require('node:fs'),path=require('node:path');
const {TabularCfr,audit,kuhnGame}=require('./tabular-cfr');
const args=Object.fromEntries(process.argv.slice(2).map(a=>{const [k,...v]=a.replace(/^--/,'').split('=');return [k,v.join('=')];}));
const iterations=Number(args.iterations)||10000,game=kuhnGame(),start=performance.now(),solver=new TabularCfr(game).run(iterations),policy=solver.policy();
const result={game:'Kuhn poker: two players, three ranks, one-chip bet',method:'full-tree tabular CFR, simultaneous updates, no neural network',iterations,informationSets:solver.nodes.size,elapsedMs:performance.now()-start,...audit(game,policy),knownEquilibriumValue:-1/18,policy,scope:'Method validation only; this policy cannot be used for ten-player Texas Holdem.'};
const out=path.resolve(args.out||'cfr-lab.json');fs.mkdirSync(path.dirname(out),{recursive:true});fs.writeFileSync(out,JSON.stringify(result,null,2)+'\n');
const {policy:_,...summary}=result;console.log(JSON.stringify(summary,null,2));
