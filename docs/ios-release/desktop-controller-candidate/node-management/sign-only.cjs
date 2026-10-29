const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const {execFileSync}=require('node:child_process');
const {signAsync}=require('@electron/osx-sign');
const repo='/Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose';
const helpers=path.join(repo,'docs/ios-release/desktop-controller-candidate');
const validator=require('/tmp/goose-controller-signing-metadata-validator.cjs');
const {compareSignedCli,compareStripped}=require('/tmp/goose-node-signature-equivalence.cjs');
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const load=p=>JSON.parse(fs.readFileSync(p));
const run=(c,a)=>execFileSync(c,a,{stdio:'inherit'});
const capture=(c,a)=>execFileSync(c,a,{encoding:'utf8',stdio:['ignore','pipe','pipe'],maxBuffer:16*1024*1024});
const write=(p,v)=>{fs.writeFileSync(p+'.pending',JSON.stringify(v,null,2)+'\n',{mode:0o600});fs.renameSync(p+'.pending',p);};
function requireSupportedThinMachO(file,relative) {
 const fd=fs.openSync(file,'r'),header=Buffer.alloc(8);
 try { assert.equal(fs.readSync(fd,header,0,8,0),8,'Truncated Mach-O: '+relative); }
 finally { fs.closeSync(fd); }
 assert(header.readUInt32LE(0)===0xfeedfacf && [0x0100000c,0x01000007].includes(header.readUInt32LE(4)),
  'Unsupported Mach-O architecture for signing equivalence: '+relative+' (header '+header.toString('hex')+'); thin arm64 or x86_64 required');
}
function compareAllChangedMachO(classification,baseline,app) {
 const results={};
 for(const relative of classification.machO) {
  requireSupportedThinMachO(path.join(baseline,relative),relative);
  requireSupportedThinMachO(path.join(app,relative),relative);
  results[relative]=compareSignedCli(path.join(baseline,relative),path.join(app,relative));
 }
 assert.equal(Object.keys(results).length,classification.machO.length);
 return results;
}
function helperCorruptionControl(baseline,app,relative) {
 requireSupportedThinMachO(path.join(baseline,relative),relative);
 requireSupportedThinMachO(path.join(app,relative),relative);
 const temporary=fs.mkdtempSync('/tmp/goose-helper-negative-');
 try {
  const original=path.join(temporary,'original'),signed=path.join(temporary,'signed');
  fs.copyFileSync(path.join(baseline,relative),original);fs.copyFileSync(path.join(app,relative),signed);
  execFileSync('codesign',['--remove-signature',original]);execFileSync('codesign',['--remove-signature',signed]);
  const a=fs.readFileSync(original),b=fs.readFileSync(signed);
  compareStripped(a,b);
  let cursor=32,textOffset;
  const end=32+b.readUInt32LE(20);
  for(let i=0;i<b.readUInt32LE(16);i++) {
   const command=b.readUInt32LE(cursor),size=b.readUInt32LE(cursor+4);
   assert(size>=8 && cursor+size<=end,'Invalid helper load commands');
   if(command===0x19) {
    const sections=b.readUInt32LE(cursor+64);assert(72+80*sections<=size,'Invalid helper sections');
    for(let j=0;j<sections;j++) {
     const section=cursor+72+80*j,name=b.subarray(section,section+16).toString().replace(/\0+$/,'');
     const segment=b.subarray(section+16,section+32).toString().replace(/\0+$/,'');
     if(name==='__text' && segment==='__TEXT') {
      assert.equal(textOffset,undefined,'Duplicate text section');
      assert(b.readBigUInt64LE(section+40)>0n,'Empty helper text');textOffset=b.readUInt32LE(section+48);
     }
    }
   }
   cursor+=size;
  }
  assert(Number.isInteger(textOffset) && textOffset>=end && textOffset<b.length,'No helper code byte found');
  b[textOffset]^=1;
  assert.throws(()=>compareStripped(a,b),/differs beyond/, 'Modified helper code accepted');
  return {path:relative,positiveComparisonPassed:true,corruptedTextOffset:textOffset,nonSignatureCodeChangeRejected:true};
 } finally {fs.rmSync(temporary,{recursive:true,force:true});}
}
async function main(){
 const receiptPath=process.argv[2],input=process.argv[3],stage=process.argv[4];
 assert(receiptPath&&input&&stage,'Build receipt, unsigned ZIP, fresh stage required');
 assert(path.resolve(stage).startsWith('/tmp/goose-controller-corrected-signed-'),'Unexpected staging root');
 assert(!fs.existsSync(stage),'Fresh stage required, never re-sign existing artifact');
 const b=load(receiptPath);assert.equal(b.build_job.exit,0);assert.equal(b.owner_gates.exit,0);assert.equal(b.app_version,'3.0.134');assert.equal(sha(input),b.unsigned_archive_sha256);
 assert.equal(capture('git',['-C',repo,'rev-parse',b.source_commit+'^{tree}']).trim(),b.source_tree);
 assert.equal(sha('/tmp/goose-controller-signing-metadata-validator.cjs'),'e4171080243765e9ba37669b00dfc75bb26a67aaa4da4925d614b78a9a41528d');
 assert.equal(sha('/tmp/goose-node-signature-equivalence.cjs'),'91b9f66db0fcd8c624c6fe4fb51cd3ac0f345a5c8cc01701eae097cfabd232a2');
 fs.mkdirSync(stage,{mode:0o700});fs.mkdirSync(path.join(stage,'baseline'));
 run('ditto',['-x','-k',input,path.join(stage,'baseline')]);
 const baseline=path.join(stage,'baseline/Goose Swarm.app'),app=path.join(stage,'Goose Swarm.app');
 const snapshot=(root,name)=>{const p=path.join(stage,name);run('python3',[path.join(helpers,'bundle-fingerprint.py'),root,p]);return load(p);};
 const cli='Contents/Resources/bin/goose',main='Contents/MacOS/Goose Swarm';
 assert.equal(sha(path.join(baseline,cli)),b.cli_sha256);
 assert.equal(capture('/usr/libexec/PlistBuddy',['-c','Print CFBundleShortVersionString',path.join(baseline,'Contents/Info.plist')]).trim(),b.app_version);
 const before=snapshot(baseline,'before-signing.json');run('ditto',[baseline,app]);assert.deepEqual(snapshot(app,'copied-before-signing.json'),before);
 for(const p of Object.keys(before))if(before[p].kind==='file'){
  const candidate=fs.lstatSync(path.join(app,p)),installed=path.join('/Applications/Goose Swarm.app',p);
  if(fs.existsSync(installed)){const real=fs.lstatSync(installed);assert(candidate.dev!==real.dev||candidate.ino!==real.ino,'Shared installed inode: '+p);}
 }
 const entitlements=path.join(stage,'entitlements.plist');fs.writeFileSync(entitlements,capture('git',['-C',repo,'show',b.source_commit+':ui/desktop/entitlements.plist']));
 await signAsync({app,platform:'darwin',arch:'arm64',identity:'Developer ID Application: Mihai Perdum (ZZ8MTZ6NRZ)',keychain:path.join(process.env.HOME,'Library/Keychains/goose-signing.keychain-db'),preAutoEntitlements:false,preEmbedProvisioningProfile:false,optionsForFile:()=>({entitlements,hardenedRuntime:true})});
 const signed=snapshot(app,'after-signing.json'),classification=validator.classify(before,signed,baseline,app),negativeControls=validator.negativeControls(before,signed,baseline,app,classification);
 const signatures=validator.verify(classification,app);
 const machOEquivalence=compareAllChangedMachO(classification,baseline,app);
 const cliEquivalence=machOEquivalence[cli] || compareSignedCli(path.join(baseline,cli),path.join(app,cli));
 const mainEquivalence=machOEquivalence[main] || compareSignedCli(path.join(baseline,main),path.join(app,main));
 const helper='Contents/Frameworks/Goose Swarm Helper.app/Contents/MacOS/Goose Swarm Helper';
 assert(classification.machO.includes(helper),'Expected actual changed helper positive control');
 const machONegativeControl=helperCorruptionControl(baseline,app,helper);
 const result={sourceCommit:b.source_commit,sourceTree:b.source_tree,buildReceiptSha256:sha(receiptPath),inputArchiveSha256:sha(input),inputCliSha256:b.cli_sha256,signedCliSha256:sha(path.join(app,cli)),appVersion:b.app_version,app,team:'ZZ8MTZ6NRZ',classification,negativeControls,signatures,cliEquivalence,mainEquivalence,machOEquivalence,machONegativeControl,signedFingerprintSha256:sha(path.join(stage,'after-signing.json')),signingScriptSha256:sha(__filename),machOComparatorSha256:sha('/tmp/goose-node-signature-equivalence.cjs'),signedAt:new Date().toISOString(),notarized:false,uploaded:false,installed:false};
 write(path.join(stage,'receipt.json'),result);
 const zip=path.join(stage,'Goose-Swarm-signed.zip');run('ditto',['-c','-k','--sequesterRsrc','--keepParent',app,zip]);
 const round=path.join(stage,'zip-roundtrip');fs.mkdirSync(round);run('ditto',['-x','-k',zip,round]);
 const roundApp=path.join(round,'Goose Swarm.app'),archive=snapshot(roundApp,'zip-roundtrip-fingerprint.json');assert.deepEqual(Object.keys(signed).sort(),Object.keys(archive).sort());
 const dropped=[];
 for(const [p,item] of Object.entries(signed)){
  if(JSON.stringify(item)===JSON.stringify(archive[p]))continue;
  assert((p==='.'&&item.kind==='directory')||item.kind==='symlink','Unexpected archive change: '+p);
  assert(Object.hasOwn(item.xattrs,'com.apple.provenance'));const expected=structuredClone(item);delete expected.xattrs['com.apple.provenance'];assert.deepEqual(archive[p],expected);dropped.push(p);
 }
 run('codesign',['--verify','--deep','--strict','--test-requirement','=anchor apple generic and certificate leaf[subject.OU] = "ZZ8MTZ6NRZ"',roundApp]);
 Object.assign(result,{signedArchive:zip,signedArchiveSha256:sha(zip),archiveProvenanceOnlyDrops:dropped,archiveStrictSignatureVerified:true});write(path.join(stage,'receipt.json'),result);
 console.log(JSON.stringify({receiptPath:path.join(stage,'receipt.json'),signedArchive:zip,signedArchiveSha256:result.signedArchiveSha256,signedCliSha256:result.signedCliSha256,notarized:false,uploaded:false,installed:false}));
}
module.exports={requireSupportedThinMachO,compareAllChangedMachO,helperCorruptionControl};
if(require.main===module)main().catch(e=>{console.error(String(e.message||e));process.exit(1);});
