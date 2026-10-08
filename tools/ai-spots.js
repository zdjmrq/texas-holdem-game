'use strict';
// Finite, independently dealt preflop audit. Public ranges never use actual opponent cards.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const core=require('../js/ai-core'),probability=require('../js/probability-core'),{STYLE_CONFIGS}=require('../js/ai');
const args=Object.fromEntries(process.argv.slice(2).map(a=>{const [k,...v]=a.replace(/^--/,'').split('=');return [k,v.join('=')];}));
const baseline=args.baseline?require(path.resolve(args.baseline)):null;
const out=path.resolve(args.out||'ai-spots.json'),variant=args.variant==='shortdeck'?'shortdeck':'standard';
const personas=args.personas?JSON.parse(fs.readFileSync(args.personas,'utf8')):STYLE_CONFIGS;
const deck=core.buildDeck(variant),step=Math.max(1,Number(args.step)||1),results=[],examples=[];
let decisions=0,violations=0;
for(const n of variant==='shortdeck'?[2,4,6,8]:[2,4,6,10])for(const position of ['first','button']){
 if(n===2&&position==='first')continue;
 const hero=position==='button'?0:variant==='shortdeck'?1:3;
 for(const open of [false,true])for(const style of Object.keys(personas)){
  const row={n,position,open,style,candidate:{fold:0,call:0,check:0,raise:0,allin:0},baseline:{fold:0,call:0,check:0,raise:0,allin:0},hands:0};
  let combo=0;
  for(let a=0;a<deck.length;a++)for(let b=a+1;b<deck.length;b++){
   if(combo++%step!==0&&!(deck[a].value===14&&deck[b].value===14))continue;
   const opener=hero===0?(n===2?1:3):0;
   const ps=Array.from({length:n},(_,seat)=>({seat,stack:16000,roundBet:open&&seat===opener?240:variant==='shortdeck'?seat===0?80:40:seat===1?40:seat===2?80:0,
     positionFromButton:seat,isSmallBlind:variant==='standard'&&seat===1,isBigBlind:variant==='standard'&&seat===2}));
   if(n===2&&variant==='standard'){ps[0].roundBet=40;ps[1].roundBet=open?240:80;}
   if(hero!==opener&&open)ps[opener].roundBet=240;
   ps.forEach(p=>p.committed=p.roundBet);
   const heroBet=ps[hero].roundBet,currentBet=open?240:80,toCall=currentBet-heroBet;
   const raw={evaluateCards:cards=>probability.evaluate(cards,variant==='shortdeck'),variant,heroSeat:hero,handId:combo,decisionId:'audit',seed:'v157-independent-spots',
    holeCards:[deck[a],deck[b]],phase:'preflop',playerCount:n,positionFromButton:hero,isSmallBlind:variant==='standard'&&n===2&&hero===0,
    activeOpponentSeats:ps.filter(p=>p.seat!==hero).map(p=>p.seat),publicPlayers:ps,stack:16000,effectiveStack:16000,pot:ps.reduce((s,p)=>s+p.roundBet,0),
    currentBet,toCall,heroRoundBet:heroBet,bigBlind:80,wagerUnit:40,minRaiseTo:open?400:160,maxRaiseTo:16000+heroBet,canRaise:true,canCheck:toCall===0,
    legalActions:toCall>0?['fold','call','raise','allin']:['check','raise','allin'],preflopRaiseCount:open?1:0,timeBudgetMs:180};
   for(const [team,api] of [['candidate',core],['baseline',baseline]]){
    if(!api)continue;
    const brain=new api.UnifiedPokerAI({seatId:hero,style,profile:personas[style]});
    if(open)brain.observeAction({seatId:opener,handId:combo,street:'preflop',action:'raise',meta:{communityCards:[],potBefore:120,toCallBefore:80,betFraction:1.2,preflopRaiseCountBefore:0,positionFromButton:opener,playerCount:n}});
    const d=brain.decide(raw);row[team][d.action]++;decisions++;
    assert.ok(raw.legalActions.includes(d.action));assert.ok(Number.isSafeInteger(d.amount));if(d.action==='raise')assert.equal(d.amount%40,0);
    if(deck[a].value===14&&deck[b].value===14&&['fold','check','call'].includes(d.action)&&!open&&variant==='standard'){if(team==='candidate')violations++;examples.push({team,n,position,style,action:d.action,hand:raw.holeCards});}
   }
   row.hands++;
  }
  results.push(row);console.log(JSON.stringify({n,position,open,style,hands:row.hands}));
 }
}
fs.mkdirSync(path.dirname(out),{recursive:true});fs.writeFileSync(out,JSON.stringify({variant,step,decisions,premiumOpenViolations:violations,results,examples},null,2));
assert.equal(violations,0,'premium opening audit failed');console.log('FINITE_SPOTS_COMPLETE');
