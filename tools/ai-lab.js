'use strict';
// Finite, reproducible experiments. Never changes the installed strategy or starts a service.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const core = require('../js/ai-core');
const probability=require('../js/probability-core');
const {AIPlayer}=require('../js/ai');
const {ServerPokerGame, chooseServerAiAction, createServerAi} = require('../server/server-game');
const args = Object.fromEntries(process.argv.slice(2).map(a => { const [k,...v]=a.replace(/^--/,'').split('='); return [k,v.join('=')]; }));
const baselinePath = args.baseline && path.resolve(args.baseline);
const baseline = baselinePath ? require(baselinePath) : core;
const blocks = Math.min(2000,Math.max(1,Number(args.blocks)||12));
const hands = Math.min(1000,Math.max(1,Number(args.hands)||12));
const seats = Math.min(10,Math.max(2,Number(args.seats)||4));
const seedBase = Number(args.seed)||17001;
const variant = args.variant === 'shortdeck' ? 'shortdeck' : 'standard';
const smallBlind=args['small-blind']===undefined?40:Number(args['small-blind']);
const bigBlind=args['big-blind']===undefined?80:Number(args['big-blind']);
const stackBb=args['stack-bb']===undefined?200:Number(args['stack-bb']);
assert.ok(Number.isSafeInteger(smallBlind)&&smallBlind>=1&&Number.isSafeInteger(bigBlind)&&bigBlind>=smallBlind,'invalid blinds');
assert.ok(Number.isFinite(stackBb)&&stackBb>=2&&stackBb<=2000&&Number.isSafeInteger(stackBb*bigBlind),'invalid stack depth');
const buyIn=stackBb*bigBlind;
const overrides = args.profile ? JSON.parse(fs.readFileSync(args.profile,'utf8')) : {};
const personaOverrides = args.personas ? JSON.parse(fs.readFileSync(args.personas,'utf8')) : null;
for (const [k,v] of Object.entries(overrides)) {
    assert.ok(k in core.DEFAULT_PROFILE && Number.isFinite(v) && v>=0 && v<=1, `invalid profile: ${k}`);
}
const out = path.resolve(args.out || `ai-lab-${variant}-${seedBase}.json`);
fs.mkdirSync(path.dirname(out),{recursive:true});
const log = fs.createWriteStream(out.replace(/\.json$/,'.decisions.jsonl'));
const timings=[], profits=[], personalities={};
const teamTimings={candidate:[],baseline:[]};
const preflopSpots={};
const terminalSpots={};
const riverSolverSpots={};
let decisions=0, played=0, timeouts=0, exact=0, errors=0;
const originalError=console.error;
let observationErrors=0;
console.error=(...items)=>{ if(String(items[0]).includes('AI observation failed')) observationErrors++; originalError(...items); };
const styles = Object.keys(core.PROFILE_OVERRIDES);
function random(seed) { let state=seed>>>0; return ()=>((state=(Math.imul(state,1664525)+1013904223)>>>0)/4294967296); }
function percentile(a,p) { const b=a.slice().sort((x,y)=>x-y); return b[Math.min(b.length-1,Math.floor(b.length*p))]||0; }
function summary(a) { const mean=a.reduce((s,v)=>s+v,0)/a.length; const sd=Math.sqrt(a.reduce((s,v)=>s+(v-mean)**2,0)/Math.max(1,a.length-1)); return {mean,standardError:sd/Math.sqrt(a.length),ci95:[mean-1.96*sd/Math.sqrt(a.length),mean+1.96*sd/Math.sqrt(a.length)]}; }
function rulePolicy(raw,kind,brain){
    assert.ok(['calling','tight','pressure'].includes(kind),'unknown control');
    const ctx=core.normalizedContext(raw),rng=new core.SeededRng(`${ctx.handId}:${ctx.decisionId}:rule-${kind}`);
    const feature=core.startingHandStrength(...ctx.holeCards,ctx.variant,'MP');
    const made=ctx.communityCards.length?probability.evaluate([...ctx.holeCards,...ctx.communityCards],ctx.variant==='shortdeck').rank:0;
    let action=ctx.toCall?'call':'check',amount=Math.min(ctx.toCall,ctx.heroStack);
    if(kind==='tight'){
        const good=ctx.street==='preflop'?feature>.68:made>=2;
        if(ctx.toCall&&!good){action='fold';amount=0;}
        else if(good&&ctx.canRaise){action='raise';amount=Math.max(ctx.minRaiseTo,ctx.currentBet+Math.round(ctx.pot*.55));}
    }else if(kind==='pressure'){
        if(ctx.toCall>ctx.pot*.9&&(ctx.street==='preflop'?feature<.70:made<2)){action='fold';amount=0;}
        else if(ctx.canRaise&&rng.next()<.78){action='raise';amount=Math.max(ctx.minRaiseTo,ctx.currentBet+Math.round(ctx.pot*.8));}
    }
    const d=brain.legalize({action,amount},ctx);
    return {...d,trace:{strategyVersion:`rule-${kind}`,equity:{mean:0,stdDev:0,confidence:0,samples:0},chosen:{action:d.action,amount:d.amount}}};
}
for(let block=0;block<blocks;block++) {
    let blockProfit=0;
    // Complement the team assignment, preserving deck seed and style at each seat.
    for(let rotation=0;rotation<2;rotation++) {
        const players=Array.from({length:seats},(_,seat)=>createServerAi(seat,buyIn,random(seedBase+block+seat)));
        const newer=players.map((_,seat)=>(seat+rotation)%2===0);
        players.forEach((p,seat)=>{
            const style=styles[(seat+block)%styles.length];
            const api=newer[seat]?core:baseline;
            p.aiRef=new AIPlayer(`Lab-${style}`,style,buyIn,seat);
            if(personaOverrides){
                assert.ok(personaOverrides[style] && Number.isFinite(personaOverrides[style].vpip) && Number.isFinite(personaOverrides[style].aggression),`missing persona: ${style}`);
                p.aiRef.config={...p.aiRef.config,...personaOverrides[style]};
            }
            p.aiRef.sharedBrain=new api.UnifiedPokerAI({seatId:seat,style,
                profile:{...p.aiRef.config,...(newer[seat]?overrides:{})},variant});
            p.aiRef.isShortDeck=variant==='shortdeck';
            const original=!newer[seat]&&args.control?ctx=>rulePolicy(ctx,args.control,p.aiRef.sharedBrain):p.aiRef.sharedBrain.decide.bind(p.aiRef.sharedBrain);
            const preflopSeen=new Set(),vpipSeen=new Set(),pfrSeen=new Set();
            p.aiRef.sharedBrain.decide=(ctx)=>{
                const started=performance.now(), decision=original(ctx), ms=performance.now()-started;
                timings.push(ms); decisions++; timeouts+=Number(!!decision.trace.equity.timedOut); exact+=Number(!!decision.trace.equity.exact);
                const team=newer[seat]?'candidate':'baseline';teamTimings[team].push(ms);
                if(decision.trace.riverSolver){const s=riverSolverSpots[team]??={opportunities:0,elapsedMs:0,maxMs:0,check:0,fold:0,call:0,raise:0,allin:0};s.opportunities++;s.elapsedMs+=decision.trace.riverSolver.elapsedMs;s.maxMs=Math.max(s.maxMs,decision.trace.riverSolver.elapsedMs);s[decision.action]++;}
                const finalCall=decision.trace.candidates?.find(c=>c.terminalCall);
                if(finalCall){const s=terminalSpots[`${team}:${game.phase}:${finalCall.terminalCall}`]??={opportunities:0,positiveEv:0,negativeEv:0,call:0,fold:0,raise:0};s.opportunities++;s.positiveEv+=Number(finalCall.ev>0);s.negativeEv+=Number(finalCall.ev<0);s[decision.action==='fold'?'fold':decision.action==='raise'?'raise':'call']++;}
                const stats=personalities[`${team}:${style}`] ||= {decisions:0,fold:0,call:0,check:0,raise:0,allin:0,invested:0,preflopOpportunities:0,vpipHands:0,pfrHands:0,settledHands:0,profitBb:0};
                stats.decisions++; stats[decision.action]++; stats.invested+=decision.action==='raise'?Math.max(0,decision.amount-ctx.heroRoundBet):decision.amount;
                if(game.phase==='preflop') {
                    const pos=core.positionCategory(core.normalizedContext(ctx));
                    const situation=ctx.preflopRaiseCount>=2?'facing3bet':ctx.preflopRaiseCount===1?'facingOpen':'unopened';
                    const spot=preflopSpots[`${team}:${pos}:${situation}`] ||= {opportunities:0,fold:0,call:0,check:0,raise:0,allin:0,aggressive:0};
                    spot.opportunities++;spot[decision.action]++;
                    spot.aggressive+=Number(decision.action==='raise'||decision.action==='allin'&&ctx.heroRoundBet+ctx.heroStack>ctx.currentBet);
                    if(!preflopSeen.has(game.handId)){stats.preflopOpportunities++;preflopSeen.add(game.handId);}
                    if(['call','raise','allin'].includes(decision.action)&&!vpipSeen.has(game.handId)){stats.vpipHands++;vpipSeen.add(game.handId);}
                    if((decision.action==='raise'||decision.action==='allin'&&ctx.heroRoundBet+ctx.heroStack>ctx.currentBet)&&!pfrSeen.has(game.handId)){stats.pfrHands++;pfrSeen.add(game.handId);}
                }
                if(game.handId%64===1) {
                    const {evaluateCards,...publicView}=ctx;
                    log.write(JSON.stringify({block,rotation,seat,style,team:newer[seat]?'candidate':'baseline',ms,publicView,decision})+'\n');
                }
                return decision;
            };
        });
        const game=new ServerPokerGame({startingStack:buyIn,smallBlind,bigBlind,ante:smallBlind,minBet:bigBlind,isShortDeck:variant==='shortdeck'},players,{random:random(seedBase+block)});
        for(let hand=0;hand<hands;hand++) {
            players.forEach(p=>{p.stack=buyIn;}); // Equal configured buy-in each hand; models persist within the match.
            assert.equal(game.startHand(),true);
            let guard=0;
            while(game.phase!=='idle') {
                assert.ok(++guard<300,'stalled hand');
                const idx=game.currentPlayerIdx, legal=game.legalActions(idx), d=chooseServerAiAction(game,idx);
                assert.ok(d && legal.actions.includes(d.action),'illegal action');
                assert.ok(Number.isSafeInteger(d.amount),'fractional wager');
                if(d.action==='raise') assert.equal(d.amount%legal.wagerUnit,0,'off-grid raise');
                const result=game.act(idx,d.action,d.amount);
                if(!result.ok) errors++;
                assert.equal(result.ok,true,result.error);
                assert.equal(observationErrors,0,'opponent observations failed; experiment is invalid');
            }
            assert.equal(players.reduce((s,p)=>s+p.stack,0),buyIn*seats,'chip conservation');
            players.forEach((p,i)=>{const stats=personalities[`${newer[i]?'candidate':'baseline'}:${p.aiRef.style}`];if(stats){stats.settledHands++;stats.profitBb+=(p.stack-buyIn)/bigBlind;}});
            if(game.handId%64===1)log.write(JSON.stringify({type:'public-settlement',block,rotation,handId:game.handId,players:players.map((p,i)=>({seat:i,team:newer[i]?'candidate':'baseline',style:p.aiRef.style,stack:p.stack,profitBb:(p.stack-buyIn)/bigBlind}))})+'\n');
            blockProfit+=players.reduce((s,p,i)=>s+(newer[i]?p.stack-buyIn:0),0)/bigBlind;
            played++;
        }
    }
    // Cluster by deck seed, rather than incorrectly treating dependent actions as independent samples.
    profits.push(blockProfit/(hands*seats));
    if((block+1)%4===0) console.log(JSON.stringify({progress:block+1,blocks,variant,played}));
}
log.end();
const result={schema:2,variant,seedBase,blocks,handsPerRotation:hands,seats,smallBlind,bigBlind,stackBb,played,decisions,baseline:args.control?`rule-${args.control}`:baselinePath?'external':'current-control',profile:overrides,personaOverrides,
    profitBB100:summary(profits.map(v=>v*100)),clusterProfitBB100:profits.map(v=>v*100),
    timing:{averageMs:timings.reduce((s,v)=>s+v,0)/timings.length,p95Ms:percentile(timings,.95),p99Ms:percentile(timings,.99),maxMs:timings.reduce((s,v)=>Math.max(s,v),0)},
    teamTiming:Object.fromEntries(Object.entries(teamTimings).map(([team,t])=>[team,{averageMs:t.reduce((s,v)=>s+v,0)/t.length,p95Ms:percentile(t,.95),maxMs:t.reduce((s,v)=>Math.max(s,v),0)}])),
    timeouts,exact,errors,personalities,preflopSpots,terminalSpots,riverSolverSpots,rssMiB:process.memoryUsage().rss/1048576};
fs.writeFileSync(out,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result));
