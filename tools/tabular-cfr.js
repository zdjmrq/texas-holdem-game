'use strict';
// Offline, finite two-player zero-sum extensive-form CFR. No neural network.
// Information sets belong to the acting player, never to a sampled full deal.
// Algorithm reference: OpenSpiel docs/algorithms.md (CFR); independently implemented.
const assert=require('node:assert/strict');

class TabularCfr {
    constructor(game){this.game=game;this.nodes=new Map();this.iterations=0;}
    node(state){
        const player=this.game.player(state),key=this.game.key(state,player),actions=this.game.actions(state);
        let node=this.nodes.get(key);
        if(!node){node={player,actions,regrets:actions.map(()=>0),average:actions.map(()=>0)};this.nodes.set(key,node);}
        assert.equal(node.player,player);assert.deepEqual(node.actions,actions,'information-set action menus must agree');
        return {key,node};
    }
    strategy(node){const pos=node.regrets.map(r=>Math.max(0,r)),sum=pos.reduce((a,b)=>a+b,0);return pos.map(r=>sum?r/sum:1/pos.length);}
    iteration(){
        const changes=new Map(),strategyCache=new Map();
        const visit=(state,r0,r1,chance)=>{
            if(this.game.terminal(state))return this.game.payoff(state);
            if(this.game.player(state)===-1)return this.game.chance(state).reduce((v,[action,p])=>v+p*visit(this.game.child(state,action),r0,r1,chance*p),0);
            const {key,node}=this.node(state),player=node.player;
            let sigma=strategyCache.get(key);if(!sigma){sigma=this.strategy(node);strategyCache.set(key,sigma);}
            const values=node.actions.map((action,i)=>visit(this.game.child(state,action),player===0?r0*sigma[i]:r0,player===1?r1*sigma[i]:r1,chance));
            const value=values.reduce((v,x,i)=>v+sigma[i]*x,0),sign=player===0?1:-1;
            let delta=changes.get(key);if(!delta){delta={regrets:sigma.map(()=>0),average:sigma.map(()=>0)};changes.set(key,delta);}
            for(let i=0;i<sigma.length;i++){
                delta.regrets[i]+=chance*(player===0?r1:r0)*sign*(values[i]-value);
                delta.average[i]+=chance*(player===0?r0:r1)*sigma[i];
            }
            return value;
        };
        for(const [state,p] of this.game.deals())visit(state,1,1,p);
        // Apply simultaneously: a different private deal cannot observe updates
        // from an earlier deal within this iteration.
        for(const [key,d] of changes){const n=this.nodes.get(key);for(let i=0;i<n.actions.length;i++){n.regrets[i]+=d.regrets[i];n.average[i]+=d.average[i];}}
        this.iterations++;
    }
    run(iterations){assert.ok(Number.isSafeInteger(iterations)&&iterations>=1&&iterations<=100000,'finite iteration count required');for(let i=0;i<iterations;i++)this.iteration();return this;}
    policy(){return Object.fromEntries([...this.nodes].map(([key,node])=>{const sum=node.average.reduce((a,b)=>a+b,0);return [key,{player:node.player,actions:node.actions,probabilities:node.average.map(s=>sum?s/sum:1/node.actions.length)}];}));}
}

function probabilities(game,state,policy){const actions=game.actions(state),n=policy[game.key(state,game.player(state))];assert.ok(n,'policy misses an information set');assert.deepEqual(n.actions,actions);return n.probabilities;}
function policyValue(game,policy,replies,respondingPlayer){
    const value=state=>{
        if(game.terminal(state))return game.payoff(state);
        const player=game.player(state);
        if(player===-1)return game.chance(state).reduce((v,[a,p])=>v+p*value(game.child(state,a)),0);
        const actions=game.actions(state);
        if(replies&&player===respondingPlayer){const a=replies.get(game.key(state,player));assert.ok(a!==undefined,'missing future best response');return value(game.child(state,a));}
        const sigma=probabilities(game,state,policy);return actions.reduce((v,a,i)=>v+sigma[i]*value(game.child(state,a)),0);
    };
    return game.deals().reduce((v,[s,p])=>v+p*value(s),0);
}

function bestResponse(game,policy,player){
    const sets=new Map();
    const discover=(state,weight,depth)=>{
        if(game.terminal(state))return;
        const actor=game.player(state);
        if(actor===-1){for(const [a,p] of game.chance(state))discover(game.child(state,a),weight*p,depth+1);return;}
        const actions=game.actions(state);
        if(actor===player){
            const key=game.key(state,player);let group=sets.get(key);
            if(!group){group={depth,records:[]};sets.set(key,group);}
            assert.equal(group.depth,depth,'best-response auditor requires equal-depth perfect-recall information sets');
            group.records.push({state,weight});for(const a of actions)discover(game.child(state,a),weight,depth+1);
        }else{const sigma=probabilities(game,state,policy);actions.forEach((a,i)=>discover(game.child(state,a),weight*sigma[i],depth+1));}
    };
    for(const [s,p] of game.deals())discover(s,p,0);
    const replies=new Map(),sign=player===0?1:-1;
    const downstream=state=>{
        if(game.terminal(state))return sign*game.payoff(state);
        const actor=game.player(state);
        if(actor===-1)return game.chance(state).reduce((v,[a,p])=>v+p*downstream(game.child(state,a)),0);
        if(actor===player){const a=replies.get(game.key(state,player));assert.ok(a!==undefined);return downstream(game.child(state,a));}
        const sigma=probabilities(game,state,policy);return game.actions(state).reduce((v,a,i)=>v+sigma[i]*downstream(game.child(state,a)),0);
    };
    for(const [key,group] of [...sets].sort((a,b)=>b[1].depth-a[1].depth)){
        const actions=game.actions(group.records[0].state),values=actions.map(a=>group.records.reduce((v,r)=>v+r.weight*downstream(game.child(r.state,a)),0));
        let best=0;for(let i=1;i<values.length;i++)if(values[i]>values[best])best=i;replies.set(key,actions[best]);
    }
    return {value:sign*policyValue(game,policy,replies,player),actions:Object.fromEntries(replies)};
}

function audit(game,policy){const value=policyValue(game,policy),br0=bestResponse(game,policy,0),br1=bestResponse(game,policy,1);return {value,bestResponseValues:[br0.value,br1.value],nashConv:br0.value+br1.value,exploitability:(br0.value+br1.value)/2};}

function kuhnGame(){
    const starts=[];for(let a=0;a<3;a++)for(let b=0;b<3;b++)if(a!==b)starts.push([{cards:[a,b],history:''},1/6]);
    return {
        deals:()=>starts,
        terminal:s=>['pp','bf','bc','pbf','pbc'].includes(s.history),
        player:s=>s.history.length%2,
        key:(s,p)=>`${p}:${s.cards[p]}:${s.history}`,
        actions:s=>s.history.endsWith('b')?['f','c']:['p','b'],
        child:(s,a)=>({cards:s.cards,history:s.history+a}),
        payoff:s=>s.history==='bf'?1:s.history==='pbf'?-1:(s.cards[0]>s.cards[1]?1:-1)*(s.history.endsWith('c')?2:1)
    };
}

// Bounded heads-up river abstraction. Joint score probabilities must be built
// from public ranges; never pass the table's actual opponent cards. Equal score
// buckets lose some blocker information, so this is a research game, not GTO NLHE.
function riverGame({pot,stacks=[5000,5000],deals,unit=100,minBet=200,fractions=[.33,.67,1],betTargets,includeAllin=true,maxRaises=1}){
    assert.ok(pot>0&&stacks.every(s=>s>0)&&unit>0&&minBet>0);
    assert.ok(Number.isSafeInteger(maxRaises)&&maxRaises>=0&&maxRaises<=3);
    assert.ok(deals.length>0&&deals.every(d=>d.scores.length===2&&d.scores.every(Number.isFinite)&&d.weight>=0&&
        (d.showdownSign===undefined||Number.isFinite(d.showdownSign)&&Math.abs(d.showdownSign)<=1)));
    const mass=deals.reduce((s,d)=>s+d.weight,0);assert.ok(mass>0);
    const ceiling=Math.min(...stacks),starts=deals.filter(d=>d.weight>0).map(d=>[{scores:d.scores,showdownSign:d.showdownSign,actor:0,paid:[0,0],currentBet:0,lastRaise:minBet,raises:0,checks:0,history:[],end:null,winner:null},d.weight/mass]);
    const terminal=s=>s.end!==null;
    const actions=s=>{
        const call=s.currentBet-s.paid[s.actor];
        const menu=call>0?['fold','call']:['check'];
        if(s.currentBet<ceiling&&(call===0||s.raises<maxRaises)){
            const minimum=call>0?s.currentBet+s.lastRaise:minBet;
            const afterCall=pot+s.paid[0]+s.paid[1]+call;
            const targets=betTargets&&call===0?betTargets.slice():fractions.map(f=>s.currentBet+Math.round(afterCall*f/unit)*unit);
            if(includeAllin)targets.push(ceiling);
            for(const amount of [...new Set(targets.map(t=>Math.min(ceiling,Math.max(minimum,t))))].sort((a,b)=>a-b))
                if(amount>s.currentBet&&(amount===ceiling||amount%unit===0))menu.push(`${call>0?'raise':'bet'}:${amount}`);
        }
        return menu;
    };
    const child=(s,a)=>{
        assert.ok(actions(s).includes(a),'illegal bounded river action');
        const n={...s,paid:s.paid.slice(),history:[...s.history,a],actor:1-s.actor};
        if(a==='fold'){n.end='fold';n.winner=1-s.actor;}
        else if(a==='call'){n.paid[s.actor]=s.currentBet;n.end='showdown';}
        else if(a==='check'){n.checks++;if(n.checks===2)n.end='showdown';}
        else{const amount=Number(a.split(':')[1]);n.paid[s.actor]=amount;n.lastRaise=amount-s.currentBet;n.currentBet=amount;n.checks=0;n.raises+=Number(s.currentBet>0);}
        return n;
    };
    return {deals:()=>starts,terminal,player:s=>s.actor,key:(s,p)=>`${p}:${s.scores[p]}:${s.history.join('|')}`,actions,child,
        payoff:s=>{
            assert.ok(terminal(s));
            if(s.end==='showdown'&&s.showdownSign!==undefined){assert.equal(s.paid[0],s.paid[1]);return s.showdownSign*(pot/2+s.paid[0]);}
            const winner=s.end==='fold'?s.winner:s.scores[0]===s.scores[1]?null:s.scores[0]>s.scores[1]?0:1;
            return winner===0?pot/2+s.paid[1]:winner===1?-pot/2-s.paid[0]:(s.paid[1]-s.paid[0])/2;
        }};
}

module.exports={TabularCfr,audit,bestResponse,policyValue,kuhnGame,riverGame};
