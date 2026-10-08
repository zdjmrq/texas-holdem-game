/* Public-range, bounded heads-up river subgame. Never receives a table deal. */
(function(root,factory){const node=typeof module==='object'&&module.exports;const api=factory(node?require('./compiled-cfr'):root.PokerCompiledCfr,node?require('./probability-core'):root.PokerProbabilityCore);if(node)module.exports=api;if(root)root.PokerRiverSolver=api;})(typeof globalThis!=='undefined'?globalThis:this,function(compiled,probability){
    'use strict';
    const key=c=>`${c.rank}-${c.suit}`;
    function ranges({board,deck,weights,buckets=6}){
        if(board.length!==5||new Set(board.map(key)).size!==5)throw Error('five distinct public cards required');
        const known=new Set(board.map(key)),available=deck.filter(c=>!known.has(key(c))),combos=[];
        for(let i=0;i<available.length;i++)for(let j=i+1;j<available.length;j++){
            const cards=[available[i],available[j]],w=weights(...cards);
            if(w.length!==2||w.some(v=>!Number.isFinite(v)||v<0))throw Error('invalid public range');
            combos.push({cards,low:(i<32?1<<i:0)|(j<32?1<<j:0),high:(i>=32?1<<(i-32):0)|(j>=32?1<<(j-32):0),score:probability.evaluate([...cards,...board],false).score,weights:w,bucket:0});
        }
        const sorted=combos.slice().sort((a,b)=>a.score-b.score),mass=sorted.reduce((s,c)=>s+c.weights[0]+c.weights[1],0);if(!(mass>0))throw Error('empty range');
        const bucketByScore={};let before=0;
        for(let i=0;i<sorted.length;){let j=i+1,group=sorted[i].weights[0]+sorted[i].weights[1];while(j<sorted.length&&sorted[j].score===sorted[i].score){group+=sorted[j].weights[0]+sorted[j].weights[1];j++;}
            const bucket=Math.min(buckets-1,Math.floor((before+group/2)/mass*buckets));for(let k=i;k<j;k++)sorted[k].bucket=bucket;bucketByScore[sorted[i].score]=bucket;before+=group;i=j;
        }
        // Exact card removal for the JOINT public ranges. Do not remove the
        // actual hero's cards here: that would reveal them to villain's policy.
        const cells=new Float64Array(buckets*buckets),signed=new Float64Array(cells.length);let total=0;
        for(const a of combos)for(const b of combos){if((a.low&b.low)||(a.high&b.high))continue;const w=a.weights[0]*b.weights[1],cell=a.bucket*buckets+b.bucket;cells[cell]+=w;signed[cell]+=w*Math.sign(a.score-b.score);total+=w;}
        if(!(total>0))throw Error('no compatible private deals');
        const deals=[];for(let i=0;i<cells.length;i++)if(cells[i]>0)deals.push({scores:[Math.floor(i/buckets),i%buckets],weight:cells[i]/total,showdownSign:signed[i]/cells[i]});
        return {deals,bucketByScore,combinations:combos.length};
    }
    function game({pot,stacks,paid=[0,0],currentBet=0,minRaiseTo=200,unit=100,minBet=200,checkedBefore=false,canRaise=true,deals,maxRaises=1}){
        const base=pot-paid[0]-paid[1],ceiling=Math.min(stacks[0]+paid[0],stacks[1]+paid[1]);
        if(base<=0||paid.some(x=>x<0)||stacks.some(x=>x<=0)||currentBet!==Math.max(...paid)||currentBet>ceiling)throw Error('unsupported river pot or stacks');
        const starts=deals.map(d=>[{scores:d.scores,sign:d.showdownSign,actor:0,paid:paid.slice(),bet:currentBet,lastRaise:currentBet?minRaiseTo-currentBet:minBet,raises:0,checks:Number(checkedBefore),history:[],end:null},d.weight]);
        const actions=s=>{
            const call=s.bet-s.paid[s.actor],menu=call?['fold','call']:['check'];
            if(s.bet<ceiling&&(s.history.length||canRaise)&&(!call||s.raises<maxRaises)){
                const minimum=call?s.bet+s.lastRaise:minBet,afterCall=base+s.paid[0]+s.paid[1]+call;
                const targets=[.33,.67,1].map(f=>s.bet+Math.round(afterCall*f/unit)*unit);targets.push(ceiling);
                for(const target of [...new Set(targets.map(t=>Math.min(ceiling,Math.max(Math.ceil(minimum/unit)*unit,t))))].sort((a,b)=>a-b))if(target>s.bet&&(target===ceiling||target%unit===0))menu.push(`${call?'raise':'bet'}:${target}`);
            }
            return menu;
        };
        const child=(s,a)=>{const n={...s,actor:1-s.actor,paid:s.paid.slice(),history:[...s.history,a]};if(a==='fold'){n.end='fold';n.winner=1-s.actor;}else if(a==='call'){n.paid[s.actor]=s.bet;n.end='showdown';}else if(a==='check'){n.checks++;if(n.checks===2)n.end='showdown';}else{const target=Number(a.split(':')[1]);n.paid[s.actor]=target;n.lastRaise=target-s.bet;n.raises+=Number(s.bet>0);n.bet=target;n.checks=0;}return n;};
        return {deals:()=>starts,terminal:s=>s.end!==null,player:s=>s.actor,key:(s,p)=>`${p}:${s.scores[p]}:${s.history.join('|')}`,actions,child,payoff:s=>s.end==='fold'?(s.winner===0?base/2+s.paid[1]:-base/2-s.paid[0]):s.sign*(base/2+s.paid[0])};
    }
    function solve(options){
        const start=performance.now(),abstract=ranges(options),g=game({...options,deals:abstract.deals}),solver=new compiled.CompiledCfr(g).run(options.iterations||800),policy=solver.policy();
        // Evaluate the AVERAGE policy, not the final iteration's strategy.
        const values=solver.values.slice();for(let i=solver.infoIndex.length-1;i>=0;i--){if(solver.infoIndex[i]<0)continue;const node=policy[solver.infos[solver.infoIndex[i]].key];let value=0;for(let j=0;j<solver.childCount[i];j++)value+=node.probabilities[j]*values[solver.children[solver.childOffset[i]+j]];values[i]=value;}
        const rootValues={},rootMass={};for(const [i,p] of solver.roots){const info=solver.infos[solver.infoIndex[i]],key=info.key;rootMass[key]=(rootMass[key]||0)+p;rootValues[key]||=info.actions.map(()=>0);for(let j=0;j<info.actions.length;j++)rootValues[key][j]+=p*values[solver.children[solver.childOffset[i]+j]];}
        for(const key of Object.keys(rootValues))rootValues[key]=rootValues[key].map(v=>v/rootMass[key]+(options.pot-(options.paid?.[0]||0)-(options.paid?.[1]||0))/2+(options.paid?.[0]||0));
        return {policy,rootValues,bucketByScore:abstract.bucketByScore,iterations:solver.iterations,informationSets:solver.infos.length,treeNodes:solver.infoIndex.length,elapsedMs:performance.now()-start,scope:'heads-up river; six strength buckets; exact public-range card removal; at most one future raise; blockers within a bucket are averaged'};
    }
    return {ranges,game,solve};
});
