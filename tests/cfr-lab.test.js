'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const {TabularCfr,audit,kuhnGame,policyValue,riverGame}=require('../tools/tabular-cfr');
test('full-tree CFR reproduces Kuhn poker value and limits a consistent-information-set best response',()=>{
 const game=kuhnGame(),solver=new TabularCfr(game).run(5000),policy=solver.policy(),r=audit(game,policy);
 assert.ok(Math.abs(r.value+1/18)<.005);assert.ok(r.exploitability<.015);assert.ok(r.nashConv>=-1e-12);assert.equal(solver.nodes.size,12);
 for(const n of Object.values(policy))assert.ok(Math.abs(n.probabilities.reduce((a,b)=>a+b,0)-1)<1e-12);
});
test('opponent private ranks cannot separate an acting player information set',()=>{
 const g=kuhnGame();assert.equal(g.key({cards:[0,1],history:''},0),g.key({cards:[0,2],history:''},0));
 assert.equal(g.key({cards:[0,2],history:'p'},1),g.key({cards:[1,2],history:'p'},1));
 const s=new TabularCfr(g);s.iteration();assert.equal(s.nodes.size,12);
 assert.throws(()=>s.run(0));assert.throws(()=>s.run(Infinity));
});
test('best-response audit detects a weak policy rather than a deal-specific clairvoyant response',()=>{
 const g=kuhnGame(),s=new TabularCfr(g);s.iteration();const uniform=s.policy(),r=audit(g,uniform);
 assert.ok(r.exploitability>.3);assert.equal(policyValue(g,uniform),r.value);
});
test('bounded river tree reproduces Kuhn payoff and convergence as an independent game implementation',()=>{
 const deals=[];for(let a=0;a<3;a++)for(let b=0;b<3;b++)if(a!==b)deals.push({scores:[a,b],weight:1});
 const game=riverGame({pot:2,stacks:[5,5],deals,unit:1,minBet:1,betTargets:[1],includeAllin:false,maxRaises:0});
 const root=game.deals()[0][0],bet=game.child(root,'bet:1'),fold=game.child(bet,'fold'),call=game.child(bet,'call');
 assert.equal(game.payoff(fold),1);assert.equal(game.payoff(call),-2);
 const result=audit(game,new TabularCfr(game).run(5000).policy());
 assert.ok(Math.abs(result.value+1/18)<.005);assert.ok(result.exploitability<.015);
});
test('bounded river tree respects integer sizes, raise increments, unequal effective caps and split pots',()=>{
 const g=riverGame({pot:1000,stacks:[1500,800],deals:[{scores:[10,10],weight:1}],unit:100,minBet:200,maxRaises:1});
 const root=g.deals()[0][0];assert.deepEqual(g.actions(root),['check','bet:300','bet:700','bet:800']);
 const bet=g.child(root,'bet:300');assert.ok(g.actions(bet).includes('raise:800'));
 assert.ok(g.actions(bet).filter(a=>a.includes(':')).every(a=>Number(a.split(':')[1])>=600));
 const raise=g.child(bet,'raise:800');assert.deepEqual(g.actions(raise),['fold','call']);
 assert.equal(g.payoff(g.child(raise,'call')),0);
 assert.equal(g.payoff(g.child(raise,'fold')),-800,'fold loses half the existing pot and only hero contribution');
});
