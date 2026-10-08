'use strict';
// Runs a sealed, finite list of experiments with at most four independent workers.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),{spawn}=require('node:child_process');
const args=Object.fromEntries(process.argv.slice(2).map(a=>{const [k,...v]=a.replace(/^--/,'').split('=');return [k,v.join('=')];}));
if(!args.plan||!args.baseline)throw new Error('--plan and --baseline required');
const plan=JSON.parse(fs.readFileSync(args.plan,'utf8')),root=path.resolve(args.out||'ai-matrix-results'),baseline=path.resolve(args.baseline);
const workers=Math.max(1,Math.min(4,Number(args.workers)||4));fs.mkdirSync(root,{recursive:true});
const hash=f=>crypto.createHash('sha256').update(fs.readFileSync(f)).digest('hex');
const sourceFiles=['js/ai-core.js','js/ai-shortdeck-stable.js','js/ai.js','js/shortdeck.js','js/engine.js','js/game.js','js/probability-core.js','js/game-rules-core.js','server/server-game.js','server/poker-rules.js','tools/ai-lab.js','tools/ai-matrix.js'];
const sourceHashes=Object.fromEntries(sourceFiles.map(f=>[f,hash(path.join(__dirname,'..',f))]));
const baselineHashes=Object.fromEntries(['ai-core.js','probability-core.js','game-rules-core.js'].map(f=>[f,hash(path.join(path.dirname(baseline),f))]));
if(!Array.isArray(plan)||!plan.length||plan.length>200)throw new Error('invalid finite plan');
const keys=new Set();for(const job of plan){if(!/^[a-z0-9-]+$/.test(job.key)||keys.has(job.key))throw new Error('invalid key');keys.add(job.key);}
fs.writeFileSync(path.join(root,'plan.json'),JSON.stringify(plan,null,2));
let cursor=0;const children=new Set();
(async()=>{
 try{await Promise.all(Array.from({length:workers},async()=>{
  while(cursor<plan.length){
   const job=plan[cursor++],out=path.join(root,job.key+'.json'),profile=path.join(root,job.key+'.profile.json');
   fs.writeFileSync(profile,JSON.stringify(job.profile||{}));
   const params=[`--baseline=${baseline}`,`--out=${out}`,`--profile=${profile}`];
   for(const k of ['variant','seats','blocks','hands','seed','small-blind','big-blind','stack-bb','control','personas'])if(job[k]!==undefined)params.push(`--${k}=${job[k]}`);
   const fd=fs.openSync(path.join(root,job.key+'.log'),'w');console.log(JSON.stringify({started:job.key,time:new Date().toISOString()}));
   await new Promise((resolve,reject)=>{
    const child=spawn(process.execPath,[path.join(__dirname,'ai-lab.js'),...params],{stdio:['ignore',fd,fd]});children.add(child);
    const timer=setTimeout(()=>{child.kill();reject(new Error(`30 minute deadline: ${job.key}`));},1800000);
    child.on('error',reject);child.on('exit',code=>{clearTimeout(timer);children.delete(child);fs.closeSync(fd);code===0?resolve():reject(new Error(`${job.key} exit ${code}`));});
   });console.log(JSON.stringify({finished:job.key,...JSON.parse(fs.readFileSync(out)).profitBB100}));
  }
 }));}catch(error){for(const child of children)child.kill();throw error;}
 const reports=plan.map(j=>({...JSON.parse(fs.readFileSync(path.join(root,j.key+'.json'))),experiment:j.key}));
 const finalHashes=Object.fromEntries(sourceFiles.map(f=>[f,hash(path.join(__dirname,'..',f))]));
 if(JSON.stringify(sourceHashes)!==JSON.stringify(finalHashes))throw new Error('sources changed during experiments');
 const summary={completedAt:new Date().toISOString(),workers,sourceHashes,baselineHashes,totalHands:reports.reduce((s,r)=>s+r.played,0),totalDecisions:reports.reduce((s,r)=>s+r.decisions,0),reports};
 fs.writeFileSync(path.join(root,'summary.json'),JSON.stringify(summary,null,2));console.log('FINITE_MATRIX_COMPLETE');
})().catch(error=>{console.error(error);process.exitCode=1;});
