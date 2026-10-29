#!/bin/bash
set -euo pipefail
stage=/tmp/goose-controller-incremental-43a8589d652f
app="$stage/Goose Swarm.app"
root=/Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose
[[ ! -e "$stage/Goose-Swarm-signed.zip" ]]
codesign --verify --deep --strict "$app"
ditto -c -k --sequesterRsrc --keepParent "$app" "$stage/Goose-Swarm-signed.zip"
mkdir "$stage/roundtrip"
ditto -x -k "$stage/Goose-Swarm-signed.zip" "$stage/roundtrip"
codesign --verify --deep --strict "$stage/roundtrip/Goose Swarm.app"
python3 "$root/docs/ios-release/desktop-controller-candidate/bundle-fingerprint.py" "$stage/roundtrip/Goose Swarm.app" "$stage/roundtrip-fingerprint.json"
cmp "$stage/after-signing.json" "$stage/roundtrip-fingerprint.json"
python3 - <<'PY'
import pathlib,json,hashlib,datetime,subprocess
p=pathlib.Path('/tmp/goose-controller-incremental-43a8589d652f');r=json.loads((p/'receipt.json').read_text());app=p/'Goose Swarm.app'
h=lambda f:hashlib.sha256(f.read_bytes()).hexdigest()
meta=subprocess.check_output(['codesign','-dv','--verbose=4',str(app)],stderr=subprocess.STDOUT,text=True);assert 'TeamIdentifier=ZZ8MTZ6NRZ' in meta
out={'sourceCommit':r['cliSourceSha'],'sourceTree':'cfc2004805b779ee7e13e287c0385b46a98ddd35','inputCliSha256':r['inputCliSha256'],'signedCliSha256':r['signedCliSha256'],'appVersion':'3.0.134','app':str(app),'team':'ZZ8MTZ6NRZ','baselineArchiveSha256':r['baselineArchiveSha256'],'baselineSourceCommit':r['desktopSourceSha'],'classification':'new whole signed desktop bundle using unchanged proven Electron resources; new source-pinned CLI','signedFingerprintSha256':h(p/'after-signing.json'),'signedArchive':str(p/'Goose-Swarm-signed.zip'),'signedArchiveSha256':h(p/'Goose-Swarm-signed.zip'),'cliEquivalence':r['cliEquivalence'],'mainEquivalence':r['mainEquivalence'],'changed':r['changed'],'signedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'notarized':False,'uploaded':False,'installed':False,'archiveStrictSignatureVerified':True,'archiveFingerprintExact':True,'sourceGateJob':'laptop-20261029-111601-1870','stampedBuildJob':'laptop-20261029-113026-440f','signJob':'laptop-20261029-113206-e91a','runtimeManifestSha256':h(pathlib.Path('/tmp/goose-cold-send-runtime-manifest.json'))}
(p/'release-receipt.json').write_text(json.dumps(out,indent=2)+'\n');print(json.dumps({k:out[k] for k in ['sourceCommit','signedArchive','signedArchiveSha256','signedCliSha256','signedFingerprintSha256','notarized']}))
PY
