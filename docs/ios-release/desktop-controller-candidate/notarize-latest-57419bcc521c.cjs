const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const {spawnSync}=require('node:child_process');
const stage='/tmp/goose-controller-corrected-signed-57419bcc521c';
const app=path.join(stage,'Goose Swarm.app'),zip=path.join(stage,'Goose-Swarm-signed.zip');
const statePath=path.join(stage,'notary-receipt.json');
const zipSha='b1703dfda06d48ac052a5f43fdb3c952785fc4a04075bd84f0f25a56127c7872';
const receiptSha='c3b52012c537fba7fac03bc093b4db9150e4597aa57e0f61f1683112b9e6d064';
const helpers='/Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose/docs/ios-release/desktop-controller-candidate';
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const read=p=>JSON.parse(fs.readFileSync(p));
const clean=s=>{for(const k of ['APPLE_ID','APPLE_ID_PASSWORD'])if(process.env[k])s=s.split(process.env[k]).join('[redacted]');return s;};
const save=r=>{fs.writeFileSync(statePath+'.pending',JSON.stringify(r,null,2)+'\n',{mode:0o600});fs.renameSync(statePath+'.pending',statePath);};
function run(cmd,args,label){const secure=cmd==='xcrun'&&args[0]==='notarytool';const p=spawnSync(secure?'python3':cmd,secure?['/tmp/goose-notary-secure-prompt.py',cmd,...args]:args,{encoding:'utf8',maxBuffer:16*1024*1024});const output=clean((p.stdout||'')+(p.stderr||''));fs.writeFileSync(path.join(stage,label+'.log'),output,{mode:0o600});if(p.error||p.status!==0)throw new Error(label+' failed; inspect redacted '+label+'.log');return p.stdout;}
function main(){
 const action=process.argv[2];assert(['submit','info','staple'].includes(action));
 assert.equal(sha(zip),zipSha);assert.equal(sha(path.join(stage,'receipt.json')),receiptSha);
 const v=read(path.join(stage,'receipt.json'));assert.equal(v.sourceCommit,'57419bcc521ce6f3ef0cc00dde59b735415496c3');
 assert.equal(v.signedArchiveSha256,zipSha);assert.equal(v.signedCliSha256,'cdc357f9743cabda6b4f2a7852a5e7654be13bb7a9f5c359b99855cdd9a2c51f');
 assert.equal(sha(path.join(app,'Contents/Resources/bin/goose')),v.signedCliSha256);
 assert.equal(sha(path.join(stage,'after-signing.json')),v.signedFingerprintSha256);
 let r=fs.existsSync(statePath)?read(statePath):{sourceCommit:v.sourceCommit,sourceTree:v.sourceTree,version:'3.0.134',uploadArchive:zip,uploadArchiveSha256:zipSha,preUploadReceiptSha256:receiptSha,notarized:false,uploaded:false,installed:false};
 assert.equal(r.uploadArchiveSha256,zipSha);assert.equal(r.preUploadReceiptSha256,receiptSha);
 if(action==='staple'){
  assert.equal(r.notaryStatus,'Accepted');assert(r.notarySubmissionId);
  run('xcrun',['stapler','staple',app],'notary-staple');
  run('codesign',['--verify','--deep','--strict','--test-requirement','=anchor apple generic and certificate leaf[subject.OU] = "ZZ8MTZ6NRZ"',app],'notary-signature');
  run('xcrun',['stapler','validate',app],'notary-staple-validation');
  run('/usr/sbin/spctl',['--assess','--type','execute','--verbose=2',app],'notary-gatekeeper');
  const post=path.join(stage,'after-notarization.json');run('python3',[path.join(helpers,'bundle-fingerprint.py'),app,post],'notary-fingerprint');
  const before=read(path.join(stage,'after-signing.json')),after=read(post);
  const changed=[...new Set([...Object.keys(before),...Object.keys(after)])].filter(k=>JSON.stringify(before[k])!==JSON.stringify(after[k]));
  assert(changed.every(k=>k==='Contents/CodeResources'),'Unexpected post-staple bundle mutation');
  assert.equal(sha(path.join(app,'Contents/Resources/bin/goose')),v.signedCliSha256);assert.equal(sha(zip),zipSha);
  Object.assign(r,{notarized:true,stapledApp:app,notarizedAt:new Date().toISOString(),notarizationChanged:changed,postStapleFingerprintSha256:sha(post),postStapleCliSha256:v.signedCliSha256,preUploadArchivePreserved:true,validation:{strictSignature:true,team:'ZZ8MTZ6NRZ',stapler:true,gatekeeper:true},distributionArchiveCreated:false});save(r);
 }else{
  assert.equal(process.env.APPLE_TEAM_ID,'ZZ8MTZ6NRZ');assert(process.env.APPLE_ID&&process.env.APPLE_ID_PASSWORD);
  const auth=['--apple-id',process.env.APPLE_ID,'--team-id','ZZ8MTZ6NRZ'];
  if(action==='submit'){
   assert(!fs.existsSync(statePath),'Existing attempt: reconcile receipt; never submit twice');
   assert.equal(process.env.GOOSE_APPROVED_APPLE_ZIP_SHA256,zipSha);
   r.notarySubmissionAttemptedAt=new Date().toISOString();
   fs.writeFileSync(statePath,JSON.stringify(r,null,2)+'\n',{mode:0o600,flag:'wx'});
   const result=JSON.parse(run('xcrun',['notarytool','submit',zip,...auth,'--output-format','json'],'notary-submit'));
   assert(result.id,'Unknown submission outcome: reconcile history');Object.assign(r,{notarySubmissionId:result.id,notaryStatus:result.status||'In Progress',uploaded:true});save(r);
  }else{
   assert(r.notarySubmissionId,'No submission id: reconcile history, never resubmit');
   const result=JSON.parse(run('xcrun',['notarytool','info',r.notarySubmissionId,...auth,'--output-format','json'],'notary-info'));
   assert.equal(result.id,r.notarySubmissionId);r.notaryStatus=result.status;r.lastCheckedAt=new Date().toISOString();save(r);
   if(['Accepted','Invalid','Rejected'].includes(result.status))run('xcrun',['notarytool','log',r.notarySubmissionId,...auth,'--output-format','json'],'notary-apple-log');
  }
 }
 console.log(JSON.stringify(r));
}
try{main();}catch(e){console.error(clean(e.message||String(e)));process.exit(1);}
