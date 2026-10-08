'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const {aggregateDeals,buildRiverAbstraction}=require('../tools/river-range-abstraction');
const {riverGame}=require('../tools/tabular-cfr');
test('joint range abstraction excludes shared cards and preserves conditional showdown weights',()=>{
 const combos=[{lowMask:3,highMask:0,bucket:0,score:10,weights:[2,1]},{lowMask:12,highMask:0,bucket:1,score:20,weights:[1,3]},{lowMask:5,highMask:0,bucket:1,score:20,weights:[9,9]}];
 const r=aggregateDeals(combos);assert.equal(r.compatiblePairs,2);assert.equal(r.totalWeight,7);
 assert.deepEqual(r.deals,[{scores:[0,1],weight:6/7,showdownSign:-1},{scores:[1,0],weight:1/7,showdownSign:1}]);
});
test('latent showdown utility does not let the policy observe an opponent hand inside an abstract bucket',()=>{
 const g=riverGame({pot:1000,stacks:[500,500],deals:[{scores:[1,1],weight:1,showdownSign:.6}],unit:100,minBet:200,betTargets:[200],includeAllin:false});
 const root=g.deals()[0][0];assert.equal(g.payoff(g.child(g.child(root,'bet:200'),'call')),420);
 assert.equal(g.payoff(g.child(g.child(root,'check'),'check')),300);
});
test('public-range builder has no hidden deal dependency and normalizes mutually legal river hands',()=>{
 const ranks=['Q','9','6','J','2'],suits=['clubs','diamonds','clubs','spades','hearts'],board=ranks.map((rank,i)=>({rank,suit:suits[i]}));
 const a=buildRiverAbstraction({board,buckets:4}),b=buildRiverAbstraction({board,buckets:4,opponentCards:[{rank:'A',suit:'spades'}],actualDeck:['secret'],personality:'MANIAC'});
 assert.deepEqual(a,b);assert.equal(a.combinations,1081);assert.ok(Math.abs(a.deals.reduce((s,d)=>s+d.weight,0)-1)<1e-10);
 assert.ok(a.deals.every(d=>Math.abs(d.showdownSign)<=1));assert.ok(a.compatiblePairs>0);
});
