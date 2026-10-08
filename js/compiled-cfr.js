/* Finite tabular CFR, compiled once into typed arrays. No learned model. */
(function(root,factory){const api=factory();if(typeof module==='object'&&module.exports)module.exports=api;if(root)root.PokerCompiledCfr=api;})(typeof globalThis!=='undefined'?globalThis:this,function(){
    'use strict';
    class CompiledCfr {
        constructor(game){
            this.game=game;this.iterations=0;this.infos=[];this.roots=[];
            const infos=new Map(),records=[];let slots=0;
            const visit=state=>{
                const index=records.length,record={info:-1,children:[],payoff:0};records.push(record);
                if(game.terminal(state)){record.payoff=game.payoff(state);return index;}
                const player=game.player(state);if(player!==0&&player!==1)throw Error('compile deals before CFR: two players, no internal chance nodes');
                const key=game.key(state,player),actions=game.actions(state);let info=infos.get(key);
                if(info===undefined){info=this.infos.length;infos.set(key,info);this.infos.push({key,player,actions,offset:slots});slots+=actions.length;}
                const old=this.infos[info];if(old.player!==player||JSON.stringify(old.actions)!==JSON.stringify(actions))throw Error('inconsistent information set');
                record.info=info;record.children=actions.map(a=>visit(game.child(state,a)));return index;
            };
            for(const [state,p] of game.deals()){if(!(p>=0&&Number.isFinite(p)))throw Error('invalid deal probability');this.roots.push([visit(state),p]);}
            const count=records.length;this.infoIndex=Int32Array.from(records,r=>r.info);this.childOffset=new Int32Array(count);this.childCount=new Int32Array(count);
            const children=[];records.forEach((r,i)=>{this.childOffset[i]=children.length;this.childCount[i]=r.children.length;children.push(...r.children);});this.children=Int32Array.from(children);
            this.values=Float64Array.from(records,r=>r.payoff);this.r0=new Float64Array(count);this.r1=new Float64Array(count);this.chance=new Float64Array(count);
            this.regrets=new Float64Array(slots);this.average=new Float64Array(slots);this.sigma=new Float64Array(slots);this.delta=new Float64Array(slots);
            this.roots.forEach(([i,p])=>{this.chance[i]=p;});
            for(let i=0;i<count;i++)for(let j=0;j<this.childCount[i];j++)this.chance[this.children[this.childOffset[i]+j]]=this.chance[i];
        }
        iteration(){
            const {infos,regrets,average,sigma,delta,infoIndex,childOffset,childCount,children,values,r0,r1,chance}=this;
            for(const info of infos){let sum=0;for(let j=0;j<info.actions.length;j++)sum+=Math.max(0,regrets[info.offset+j]);for(let j=0;j<info.actions.length;j++)sigma[info.offset+j]=sum?Math.max(0,regrets[info.offset+j])/sum:1/info.actions.length;}
            this.roots.forEach(([i])=>{r0[i]=1;r1[i]=1;});delta.fill(0);
            for(let i=0;i<infoIndex.length;i++){
                if(infoIndex[i]<0)continue;const info=infos[infoIndex[i]],offset=info.offset;
                for(let j=0;j<childCount[i];j++){const child=children[childOffset[i]+j],s=sigma[offset+j];r0[child]=r0[i]*(info.player===0?s:1);r1[child]=r1[i]*(info.player===1?s:1);average[offset+j]+=chance[i]*(info.player===0?r0[i]:r1[i])*s;}
            }
            for(let i=infoIndex.length-1;i>=0;i--){
                if(infoIndex[i]<0)continue;const info=infos[infoIndex[i]],offset=info.offset;let v=0;
                for(let j=0;j<childCount[i];j++)v+=sigma[offset+j]*values[children[childOffset[i]+j]];values[i]=v;
                const weight=chance[i]*(info.player===0?r1[i]:-r0[i]);
                for(let j=0;j<childCount[i];j++)delta[offset+j]+=weight*(values[children[childOffset[i]+j]]-v);
            }
            for(let j=0;j<regrets.length;j++)regrets[j]+=delta[j];this.iterations++;
        }
        run(n){if(!Number.isSafeInteger(n)||n<1||n>100000)throw Error('finite iteration count required');for(let i=0;i<n;i++)this.iteration();return this;}
        policy(){return Object.fromEntries(this.infos.map(info=>{let sum=0;for(let j=0;j<info.actions.length;j++)sum+=this.average[info.offset+j];return [info.key,{player:info.player,actions:info.actions,probabilities:info.actions.map((_,j)=>sum?this.average[info.offset+j]/sum:1/info.actions.length)}];}));}
    }
    return {CompiledCfr};
});
