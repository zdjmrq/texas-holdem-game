'use strict';
// Fixed, finite release acceptance. No parameter search or installed-game writes.
const fs=require('node:fs');
const path=require('node:path');
const crypto=require('node:crypto');
const {spawn}=require('node:child_process');
const args=Object.fromEntries(process.argv.slice(2).map(a=>{const [k,...v]=a.replace(/^--/,'').split('=');return [k,v.join('=')];}));
const root=path.resolve(args.out||'ai-acceptance-results');
const baseline=path.resolve(args.baseline||path.join(__dirname,'../js/ai-core.js'));
const concurrency=Math.min(4,Math.max(1,Number(args.workers)||4));
fs.mkdirSync(root,{recursive:true});
const profile=args.profile?JSON.parse(fs.readFileSync(args.profile,'utf8')):{};
const profilePath=path.join(root,'release.profile.json');fs.writeFileSync(profilePath,JSON.stringify(profile));
const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const sources=['js/ai-core.js','js/ai.js','js/probability-core.js','js/game-rules-core.js','server/server-game.js','server/poker-rules.js','tools/ai-lab.js','tools/ai-acceptance.js'];
const sourceHashes=Object.fromEntries(sources.map(file=>[file,hash(path.join(__dirname,'..',file))]));
const baselineHashes=Object.fromEntries(['ai-core.js','probability-core.js','game-rules-core.js'].map(file=>{const full=path.join(path.dirname(baseline),file);return [file,fs.existsSync(full)?hash(full):null];}));
let jobs=[];
function add(key,variant,blocks,hands,seed,seats=4,control=''){jobs.push({key,variant,blocks,hands,seed,seats,control});}
add('validation-standard','standard',320,32,131001);
add('confirmation-standard-141001','standard',512,32,141001);
add('confirmation-standard-142001','standard',512,32,142001);
add('validation-shortdeck','shortdeck',320,32,131001);
add('stress-standard-10','standard',64,32,151001,10);
add('stress-shortdeck-8','shortdeck',64,32,161001,8);
add('heads-up-standard','standard',128,32,171001,2);
add('six-player-standard','standard',128,32,191001,6);
for(const [i,control] of ['calling','tight','pressure'].entries())for(const variant of ['standard','shortdeck'])add(`control-${control}-${variant}`,variant,96,24,181001+i*1000,4,control);
if(args.suite==='standard-stress')jobs=jobs.filter(job=>job.variant==='standard'&&!job.key.startsWith('validation')&&!job.key.startsWith('confirmation'));
else if(args.suite && args.suite!=='full')throw new Error('suite must be full or standard-stress');
let cursor=0;
const children=new Set();
(async()=>{
 try{
  await Promise.all(Array.from({length:concurrency},async()=>{
   while(cursor<jobs.length){
    const job=jobs[cursor++],output=path.join(root,job.key+'.json'),log=fs.openSync(path.join(root,job.key+'.log'),'w');
    console.log(JSON.stringify({started:job.key,time:new Date().toISOString()}));
    await new Promise((resolve,reject)=>{
     const child=spawn(process.execPath,[path.join(__dirname,'ai-lab.js'),`--baseline=${baseline}`,`--profile=${profilePath}`,`--variant=${job.variant}`,`--blocks=${job.blocks}`,`--hands=${job.hands}`,`--seed=${job.seed}`,`--seats=${job.seats}`,`--out=${output}`,...(job.control?[`--control=${job.control}`]:[])],{stdio:['ignore',log,log]});children.add(child);
     const timer=setTimeout(()=>{child.kill('SIGTERM');reject(new Error(`30-minute limit: ${job.key}`));},1800000);
     child.on('error',reject);child.on('exit',code=>{clearTimeout(timer);children.delete(child);fs.closeSync(log);code===0?resolve():reject(new Error(`${job.key} exit ${code}`));});
    });
    console.log(JSON.stringify({finished:job.key,...JSON.parse(fs.readFileSync(output)).profitBB100}));
   }
  }));
 }catch(error){for(const child of children)child.kill('SIGTERM');throw error;}
 const reports=jobs.map(job=>({...JSON.parse(fs.readFileSync(path.join(root,job.key+'.json'))),experiment:job.key}));
 const standard=reports.filter(r=>r.experiment==='validation-standard'||r.experiment.startsWith('confirmation-standard')).flatMap(r=>r.clusterProfitBB100);
 const mean=standard.length?standard.reduce((s,v)=>s+v,0)/standard.length:0,se=standard.length>1?Math.sqrt(standard.reduce((s,v)=>s+(v-mean)**2,0)/(standard.length-1)/standard.length):0;
 const summary={completedAt:new Date().toISOString(),suite:args.suite||'full',profile,sourceHashes,baselineHashes,totalHands:reports.reduce((s,r)=>s+r.played,0),totalDecisions:reports.reduce((s,r)=>s+r.decisions,0),standardCombined:standard.length?{blocks:standard.length,played:reports.filter(r=>r.experiment==='validation-standard'||r.experiment.startsWith('confirmation-standard')).reduce((s,r)=>s+r.played,0),mean,standardError:se,ci95:[mean-1.96*se,mean+1.96*se]}:null,reports};
 fs.writeFileSync(path.join(root,'summary.json'),JSON.stringify(summary,null,2));console.log('FINITE_ACCEPTANCE_COMPLETE');
})().catch(error=>{console.error(error);process.exitCode=1;});
