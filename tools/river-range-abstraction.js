'use strict';
// Offline public-range abstraction for bounded river CFR. Not used by the game.
const assert=require('node:assert/strict');
const core=require('../js/ai-core'),probability=require('../js/probability-core');
const key=c=>`${c.rank}-${c.suit}`;

function aggregateDeals(combos){
    const cells=new Map();let compatiblePairs=0,total=0;
    for(const a of combos)for(const b of combos){
        if((a.lowMask&b.lowMask)!==0||(a.highMask&b.highMask)!==0)continue;
        const weight=a.weights[0]*b.weights[1];if(weight<=0)continue;
        const id=`${a.bucket}:${b.bucket}`;let cell=cells.get(id);
        if(!cell){cell={scores:[a.bucket,b.bucket],weight:0,signedWeight:0};cells.set(id,cell);}
        cell.weight+=weight;cell.signedWeight+=weight*Math.sign(a.score-b.score);compatiblePairs++;total+=weight;
    }
    assert.ok(total>0,'the public ranges have no mutually legal deal');
    return {compatiblePairs,totalWeight:total,deals:[...cells.values()].map(c=>({scores:c.scores,weight:c.weight/total,showdownSign:c.signedWeight/c.weight}))};
}

function buildRiverAbstraction({board,seats=[0,1],publicEvents=[],buckets=8}){
    assert.equal(board.length,5);assert.equal(new Set(board.map(key)).size,5);assert.equal(seats.length,2);
    assert.ok(Number.isSafeInteger(buckets)&&buckets>=2&&buckets<=16);
    const beliefs=seats.map(s=>new core.OpponentBelief(s)),models=seats.map(s=>new core.BehavioralModel(s));
    beliefs.forEach(b=>{b.evidenceWeight=.45;b.percentileWeight=1;b.preflopSharpness=1;b.adaptiveRangeWeight=.65;});
    // Only public event fields are accepted, even if an offline log includes
    // additional debugging fields. No actual hero or villain hole cards input.
    for(const raw of publicEvents){
        const index=seats.findIndex(s=>String(s)===String(raw.seatId));if(index<0)continue;
        const m=raw.meta||{},event={seatId:raw.seatId,handId:raw.handId,street:raw.street,action:raw.action,
            meta:{communityCards:m.communityCards,betFraction:m.betFraction,toCallBefore:m.toCallBefore,potBefore:m.potBefore,
                aggressive:m.aggressive,preflopRaiseCountBefore:m.preflopRaiseCountBefore,positionFromButton:m.positionFromButton,playerCount:m.playerCount}};
        models[index].observe(event);beliefs[index].publicStats=models[index];beliefs[index].observe(event,models[index]);
    }
    const known=new Set(board.map(key)),deck=core.buildDeck('standard').filter(c=>!known.has(key(c))),cache=new Map(),combos=[];
    for(let i=0;i<deck.length;i++)for(let j=i+1;j<deck.length;j++){
        const lowMask=(i<32?1<<i:0)|(j<32?1<<j:0),highMask=(i>=32?1<<(i-32):0)|(j>=32?1<<(j-32):0);
        combos.push({indices:[i,j],lowMask,highMask,score:probability.evaluate([deck[i],deck[j],...board],false).score,
            weights:beliefs.map(b=>b.comboWeight(deck[i],deck[j],'standard',cache)),bucket:0});
    }
    const sorted=combos.slice().sort((a,b)=>a.score-b.score),mass=sorted.reduce((s,c)=>s+c.weights[0]+c.weights[1],0);assert.ok(mass>0);
    const bucketByScore={};let before=0;
    for(let i=0;i<sorted.length;){
        let j=i+1,group=sorted[i].weights[0]+sorted[i].weights[1];
        while(j<sorted.length&&sorted[j].score===sorted[i].score){group+=sorted[j].weights[0]+sorted[j].weights[1];j++;}
        const bucket=Math.min(buckets-1,Math.floor((before+group/2)/mass*buckets));
        for(let k=i;k<j;k++)sorted[k].bucket=bucket;bucketByScore[sorted[i].score]=bucket;before+=group;i=j;
    }
    const joint=aggregateDeals(combos);
    return {...joint,board,seats,buckets,combinations:combos.length,bucketByScore,
        scope:'Independent product of inferred public ranges conditioned on card removal; equal-score ties preserved. Buckets discard some private blocker information. No actual table deal input.'};
}
module.exports={aggregateDeals,buildRiverAbstraction};
