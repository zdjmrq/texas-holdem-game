'use strict';
const fs=require('node:fs');
const path=require('node:path');
const {spawn}=require('node:child_process');
const args=Object.fromEntries(process.argv.slice(2).map(a=>{const [k,...v]=a.replace(/^--/,'').split('=');return [k,v.join('=')];}));
const root=path.resolve(args.out||'ai-results');
fs.mkdirSync(root,{recursive:true});
const baseline=path.resolve(args.baseline||path.join(__dirname,'../js/ai-core.js'));
const concurrency=Math.min(4,Math.max(1,Number(args.workers)||4));
const candidates={default:{},balancedEvidence:{rangeEvidenceWeight:.70,calledRangeDiscount:.055},cautiousEvidence:{rangeEvidenceWeight:.45,calledRangeDiscount:.08},conditionalCalibrated:{conditionalCallerEquityWeight:1,rangePercentileWeight:1}};
for(const [name,profile] of Object.entries(candidates)) fs.writeFileSync(path.join(root,`${name}.profile.json`),JSON.stringify(profile));
function experiment(name,stage,variant,blocks,hands,seed,seats=4,control=''){return {name,stage,variant,blocks,hands,seed,seats,control,key:`${stage}-${name}-${variant}-${seats}`};}
async function runAll(jobs){
 let cursor=0;
 const children=new Set();
 try{
 await Promise.all(Array.from({length:concurrency},async()=>{
  while(cursor<jobs.length){
   const job=jobs[cursor++], output=path.join(root,`${job.key}.json`), logfile=fs.openSync(path.join(root,`${job.key}.log`),'w');
   console.log(JSON.stringify({started:job.key,time:new Date().toISOString()}));
   await new Promise((resolve,reject)=>{
    const child=spawn(process.execPath,[path.join(__dirname,'ai-lab.js'),`--baseline=${baseline}`,`--profile=${path.join(root,`${job.name}.profile.json`)}`,`--variant=${job.variant}`,`--blocks=${job.blocks}`,`--hands=${job.hands}`,`--seed=${job.seed}`,`--seats=${job.seats}`,`--out=${output}`,...(job.control?[`--control=${job.control}`]:[])],{stdio:['ignore',logfile,logfile]});children.add(child);
    const timer=setTimeout(()=>{child.kill('SIGTERM');reject(new Error(`30-minute limit: ${job.key}`));},1800000);
    child.on('error',reject); child.on('exit',code=>{clearTimeout(timer);children.delete(child);fs.closeSync(logfile);code===0?resolve():reject(new Error(`${job.key} exit ${code}`));});
   });
   console.log(JSON.stringify({finished:job.key,...JSON.parse(fs.readFileSync(output)).profitBB100}));
  }
 }));
 }catch(error){for(const child of children)child.kill('SIGTERM');throw error;}
}
(async()=>{
 const train=Object.keys(candidates).flatMap(name=>['standard','shortdeck'].map(v=>experiment(name,'tuning',v,64,32,17001)));
 await runAll(train);
 // Tune separately: short-deck gains must not conceal standard hold'em losses.
 // Neither held-out seed family is used to select the nominee.
 const variants=['standard','shortdeck'];
 const ranked=Object.fromEntries(variants.map(v=>[v,Object.keys(candidates).map(name=>({name,score:JSON.parse(fs.readFileSync(path.join(root,`tuning-${name}-${v}-4.json`))).profitBB100.mean})).sort((a,b)=>b.score-a.score)]));
 const nominee=Object.fromEntries(variants.map(v=>[v,ranked[v][0].name]));
 fs.writeFileSync(path.join(root,'selection.json'),JSON.stringify({ranked,nominee},null,2));
 const validate=variants.flatMap(v=>[...new Set(['default',nominee[v]])].flatMap(name=>[experiment(name,'validation',v,320,32,31001),experiment(name,'confirmation',v,128,32,41001)]));
 await runAll(validate);
 const interval=a=>{
  const mean=a.reduce((s,v)=>s+v,0)/a.length,se=Math.sqrt(a.reduce((s,v)=>s+(v-mean)**2,0)/(a.length-1)/a.length);
  return {mean,standardError:se,ci95:[mean-1.96*se,mean+1.96*se]};
 };
 const comparisons=variants.filter(v=>nominee[v]!=='default').map(variant=>{
  const families=['validation','confirmation'].map(stage=>{
   const read=name=>JSON.parse(fs.readFileSync(path.join(root,`${stage}-${name}-${variant}-4.json`))).clusterProfitBB100;
   const a=read(nominee[variant]),b=read('default');return {stage,delta:a.map((v,i)=>v-b[i])};
  });
  return {variant,...interval(families.flatMap(f=>f.delta)),families:families.map(f=>({stage:f.stage,...interval(f.delta)}))};
 });
 const selected=Object.fromEntries(variants.map(v=>{
  const c=comparisons.find(c=>c.variant===v);return [v,c&&c.ci95[0]>0&&c.families.every(f=>f.ci95[1]>=0)?nominee[v]:'default'];
 }));
 fs.writeFileSync(path.join(root,'promotion.json'),JSON.stringify({nominee,selected,comparisons,criterion:'per variant: pooled held-out paired 95% interval above zero; neither family significantly worse',profiles:Object.fromEntries(variants.map(v=>[v,candidates[selected[v]]]))},null,2));
 const stress=[experiment(selected.standard,'stress','standard',64,32,51001,10),experiment(selected.shortdeck,'stress','shortdeck',64,32,61001,8),experiment(selected.standard,'heads-up','standard',128,32,71001,2)];
 const controls=['calling','tight','pressure'].flatMap((control,i)=>variants.map(v=>experiment(selected[v],`control-${control}`,v,96,24,81001+i*1000,4,control)));
 await runAll([...stress,...controls]);
 const reports=[...train,...validate,...stress,...controls].map(j=>({...JSON.parse(fs.readFileSync(path.join(root,`${j.key}.json`))),experiment:j.key}));
 fs.writeFileSync(path.join(root,'summary.json'),JSON.stringify({completedAt:new Date().toISOString(),selected,nominee,comparisons,ranked,totalHands:reports.reduce((s,r)=>s+r.played,0),totalDecisions:reports.reduce((s,r)=>s+r.decisions,0),reports},null,2));
 console.log('FINITE_TOURNAMENT_COMPLETE');
})().catch(e=>{console.error(e);process.exitCode=1;});
