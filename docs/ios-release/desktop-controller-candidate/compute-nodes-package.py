import hashlib,json,os,pathlib,stat,subprocess,sys
repo=pathlib.Path(os.environ['REPO']); stage=pathlib.Path(os.environ['STAGE']); source=stage/'source'; sha=os.environ['SOURCE_COMMIT']
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def files(root):
 out={}
 for folder,dirs,names in os.walk(root,followlinks=False):
  for name in dirs+names:
   p=pathlib.Path(folder)/name; s=p.lstat(); rel=str(p.relative_to(root))
   if p.is_symlink():out[rel]={'link':os.readlink(p),'mode':stat.S_IMODE(s.st_mode)}
   elif p.is_file():out[rel]={'sha256':digest(p),'mode':stat.S_IMODE(s.st_mode)}
   elif p.is_dir():out[rel]={'directory':True,'mode':stat.S_IMODE(s.st_mode)}
 return out
mode=sys.argv[1]
if mode=='source':
 manifest={}
 for row in subprocess.check_output(['git','-C',str(repo),'ls-tree','-rz',sha]).split(b'\0'):
  if not row:continue
  meta,path=row.split(b'\t',1); perms,kind,oid=meta.decode().split(); name=os.fsdecode(path)
  if kind!='blob':raise RuntimeError('Unsupported tracked entry '+name)
  p=source/name; data=os.readlink(p).encode() if perms=='120000' else p.read_bytes()
  assert hashlib.sha1(b'blob '+str(len(data)).encode()+b'\0'+data).hexdigest()==oid,name
  if perms=='100755':assert p.stat().st_mode & stat.S_IXUSR,name
  manifest[name]={'oid':oid,'mode':perms}
 target=stage/'runtime-source-manifest.json'
 result={'sourceCommit':sha,'sourceTree':subprocess.check_output(['git','-C',str(repo),'rev-parse',sha+'^{tree}'],text=True).strip(),'files':manifest}
 if target.exists():assert json.loads(target.read_text())==result,'Source manifest changed'
 else:target.write_text(json.dumps(result,sort_keys=True,indent=2)+'\n')
 print('EXACT_COMMITTED_SOURCE_PASS',len(manifest))
elif mode=='baseline':
 receipt_revision='1623113ce86a681c758df1f5779f2b8eadbe4f1e'
 receipt_path='docs/ios-release/desktop-controller-candidate/notarization-43a8589d652f.json'
 raw=subprocess.check_output(['git','-C',str(repo),'show',receipt_revision+':'+receipt_path]); receipt=json.loads(raw)
 assert receipt['sourceCommit']=='43a8589d652f72c145f57cbe667b0b9ec2a71976'
 assert receipt['notaryStatus']=='Accepted' and receipt['notarized']
 baseline=pathlib.Path(os.environ['BASELINE_APP']); assert str(baseline)==receipt['stapledApp']
 saved=baseline.parent/'after-notarization.json'
 assert digest(saved)==receipt['postStapleFingerprintSha256']
 actual=stage/'baseline-fingerprint.json'
 subprocess.run([sys.executable,str(source/'docs/ios-release/desktop-controller-candidate/bundle-fingerprint.py'),str(baseline),str(actual)],check=True)
 assert digest(actual)==receipt['postStapleFingerprintSha256'],'Baseline differs from committed notarized43a receipt'
 (stage/'baseline-notarization-receipt.json').write_bytes(raw)
 print('PINNED_NOTARIZED_43A_BASELINE_PASS')
elif mode=='payload':
 baseline=pathlib.Path(os.environ['BASELINE_APP'])/'Contents/Resources'
 paths=['bundled-mcps','bin/tailscale','bin/tailscaled']
 got={name:files(baseline/name) if (baseline/name).is_dir() else {'sha256':digest(baseline/name),'mode':stat.S_IMODE((baseline/name).lstat().st_mode)} for name in paths}
 dest=stage/'baseline-runtime-payload.json'
 if dest.exists():assert json.loads(dest.read_text())==got,'Approved baseline payload changed'
 else:dest.write_text(json.dumps(got,sort_keys=True,indent=2)+'\n')
 print('BASELINE_RUNTIME_PAYLOAD_PASS')
elif mode in ['copied','packaged']:
 expected=json.loads((stage/'baseline-runtime-payload.json').read_text())
 root=source/'ui/desktop' if mode=='copied' else source/'ui/desktop/out/Goose Swarm-darwin-arm64/Goose Swarm.app/Contents/Resources'
 assert files(root/'bundled-mcps')==expected['bundled-mcps'],'Bundled MCP payload bytes/modes/links differ'
 binaries=root/'src/bin' if mode=='copied' else root/'bin'
 for name in ['tailscale','tailscaled']:
  assert digest(binaries/name)==expected['bin/'+name]['sha256'],name
  assert stat.S_IMODE((binaries/name).lstat().st_mode)==expected['bin/'+name]['mode'],name
 print('COPIED_PAYLOAD_EQUAL_PASS',mode)
elif mode=='pins':
 desktop=source/'ui/desktop'; pins=json.loads((desktop/'scripts/bundled-mcps.json').read_text()); payload=desktop/'bundled-mcps'
 assert not payload.is_symlink()
 assert json.loads((payload/'manifest.json').read_text())==pins
 for pin in pins:
  revision=pin['commit']
  for path in [desktop/'scripts/mcp-locks'/f"{pin['id']}.json",desktop/'scripts/mcp-patches'/f"{pin['id']}.patch"]:
   if path.exists():revision+=':'+digest(path)
  assert (payload/pin['id']/'.revision').read_text()==revision,pin['id']
  assert (payload/pin['id']/pin['entry']).is_file(),pin['id']
 print('BUNDLED_MCP_PINS_PASS')
elif mode=='record-sdk-build':
 target=pathlib.Path('/tmp/goose-node-management-sdk-built.json'); assert not target.exists()
 names=subprocess.check_output(['git','-C',str(repo),'ls-files','--cached','--others','--exclude-standard','ui/sdk','crates/goose/acp-schema.json','crates/goose/acp-meta.json'],text=True).splitlines()
 target.write_text(json.dumps({'files':files(repo/'ui/sdk/dist'),'sourceFiles':{name:digest(repo/name) for name in names if (repo/name).is_file()}},sort_keys=True,indent=2)+'\n')
 print('SDK_BUILD_OUTPUT_RECORDED',digest(target))
elif mode=='snapshot-sdk':
 target=pathlib.Path(os.environ['SDK_MANIFEST']); assert not target.exists()
 build=pathlib.Path('/tmp/goose-node-management-sdk-built.json'); built=json.loads(build.read_text())
 job=os.environ['SDK_GATE_JOB']; assert pathlib.Path(job).name==job
 jobdir=pathlib.Path.home()/'.cache/node-jobs'/job
 assert (jobdir/'exit').read_text().strip()=='0'
 assert 'NODE_MANAGEMENT_SDK_GATES_PASS' in (jobdir/'log').read_text()
 assert files(repo/'ui/sdk/dist')==built['files'],'SDK dist changed after successful build'
 for name,expected in built['sourceFiles'].items():assert digest(repo/name)==expected,name
 assert not subprocess.check_output(['git','-C',str(repo),'diff',sha,'--','ui/sdk','crates/goose/acp-schema.json','crates/goose/acp-meta.json'])
 target.write_text(json.dumps({'sourceCommit':sha,'sdkGateJob':job,'buildRecordSha256':digest(build),'files':built['files']},sort_keys=True,indent=2)+'\n')
 print('SDK_DISTRIBUTION_PINNED',digest(target))
elif mode=='sdk':
 manifest=json.loads(pathlib.Path(os.environ['SDK_MANIFEST']).read_text())
 assert manifest['sourceCommit']==sha
 assert files(source/'ui/sdk/dist')==manifest['files'],'Gated SDK dist differs'
 print('GATED_SDK_DIST_PASS')
elif mode=='cli':
 cli=pathlib.Path(os.environ['CLI']); assert digest(cli)==os.environ['CLI_SHA256']; assert sha.encode() in cli.read_bytes()
 assert (cli.parent/'source-commit.txt').read_text().strip()==sha
 assert (cli.parent/'source-tree.txt').read_text().strip()==subprocess.check_output(['git','-C',str(repo),'rev-parse',sha+'^{tree}'],text=True).strip()
 print('STAMPED_CLI_PASS')
elif mode=='receipt':
 app=source/'ui/desktop/out/Goose Swarm-darwin-arm64/Goose Swarm.app'; cli=app/'Contents/Resources/bin/goose'
 assert digest(cli)==os.environ['CLI_SHA256'],'Packaged CLI differs'
 assert (app/'Contents/Resources/app.asar').is_file()
 for name in ['tailscale','tailscaled']:assert digest(app/'Contents/Resources/bin'/name)==digest(source/'ui/desktop/src/bin'/name)
 receipt={'sourceCommit':sha,'sourceTree':json.loads((stage/'runtime-source-manifest.json').read_text())['sourceTree'],'cliSha256':digest(cli),'app':str(app),'asarSha256':digest(app/'Contents/Resources/app.asar'),'unsignedArchive':str(stage/'Goose-Swarm-unsigned.zip'),'unsignedArchiveSha256':digest(stage/'Goose-Swarm-unsigned.zip'),'sdkManifestSha256':digest(pathlib.Path(os.environ['SDK_MANIFEST'])),'sourceManifestSha256':digest(stage/'runtime-source-manifest.json'),'baselinePayloadManifestSha256':digest(stage/'baseline-runtime-payload.json'),'signedForDistribution':False,'installed':False,'uploaded':False}
 (stage/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print('FRESH_ELECTRON_PACKAGE_READY',str(app))
else:raise RuntimeError(mode)
