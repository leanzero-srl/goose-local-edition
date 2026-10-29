#!/bin/bash
# Development-only physical diagnosis. No archive, export, upload, install or launch.
set -euo pipefail
cd /Users/mihaiperdum/.codex/worktrees/link-controller-plan/goose
final_commit="${1:?Pass the reviewed diagnostic source commit}"
final_commit=$(git rev-parse --verify "$final_commit^{commit}")
expected_tree=$(git rev-parse "$final_commit:ui/ios")
artifact="/tmp/goose-phone-lifecycle-${final_commit:0:12}"
profile_uuid=da27f4b6-6c1a-4495-871b-da7890d58fac
profile="/Users/mihaiperdum/Library/Developer/Xcode/UserData/Provisioning Profiles/$profile_uuid.mobileprovision"
git diff --exit-code -- ui/ios
test "$(git rev-parse HEAD:ui/ios)" = "$expected_tree"
test ! -e "$artifact"
python3 - "$profile" <<'PY'
import sys,subprocess,plistlib
p=plistlib.loads(subprocess.check_output(['security','cms','-D','-i',sys.argv[1]],stderr=subprocess.DEVNULL))
assert p['TeamIdentifier']==['ZZ8MTZ6NRZ']
assert p['Entitlements'].get('get-task-allow') is True
assert p['Entitlements']['application-identifier']=='ZZ8MTZ6NRZ.net.leanzero.goose-remote'
assert '00008150-000640A81442401C' in [d.upper() for d in p.get('ProvisionedDevices',[])]
print('Explicit development profile includes the paired physical phone and correct bundle/team.')
PY
python3 ui/ios/scripts/verify-mesh-framework.py /tmp/goose-libtailscale-device-build/Build/Products/Release-iphoneos/TailscaleKit.framework
printf 'NATIVE_SOURCE_COMMIT %s\nNATIVE_SOURCE_TREE %s\n' "$final_commit" "$expected_tree"
xcodebuild -project ui/ios/GooseRemote.xcodeproj -scheme GooseRemote -configuration Release -destination 'generic/platform=iOS' -derivedDataPath /tmp/goose-remote-device-derived -allowProvisioningUpdates CODE_SIGN_STYLE=Automatic DEVELOPMENT_TEAM=ZZ8MTZ6NRZ 'CODE_SIGN_IDENTITY=Apple Development' GOOSE_TAILSCALE_FRAMEWORK_DIR=/tmp/goose-libtailscale-device-build/Build/Products/Release-iphoneos 'GOOSE_MESH_LDFLAGS=-framework TailscaleKit' build
git diff --exit-code -- ui/ios
test "$(git rev-parse HEAD:ui/ios)" = "$expected_tree"
mkdir "$artifact"
ditto /tmp/goose-remote-device-derived/Build/Products/Release-iphoneos/GooseRemote.app "$artifact/GooseRemote.app"
if [[ -d /tmp/goose-remote-device-derived/Build/Products/Release-iphoneos/GooseRemote.app.dSYM ]]; then
  ditto /tmp/goose-remote-device-derived/Build/Products/Release-iphoneos/GooseRemote.app.dSYM "$artifact/GooseRemote.app.dSYM"
fi
python3 - "$artifact" "$final_commit" "$expected_tree" <<'PYVERIFY'
import sys,subprocess,plistlib,re,hashlib,json,datetime
from pathlib import Path
artifact=Path(sys.argv[1]);app=artifact/'GooseRemote.app';framework=app/'Frameworks/TailscaleKit.framework'
def run(*args):return subprocess.check_output(args,text=True,stderr=subprocess.STDOUT)
def uuid(p):return re.search(r'UUID: ([0-9A-F-]+)',run('dwarfdump','--uuid',str(p))).group(1)
run('codesign','--verify','--deep','--strict',str(app))
assert 'TeamIdentifier=ZZ8MTZ6NRZ' in run('codesign','-dvv',str(app))
assert 'TailscaleKit.framework/TailscaleKit' in run('otool','-L',str(app/'GooseRemote'))
p=plistlib.loads(subprocess.check_output(['security','cms','-D','-i',str(app/'embedded.mobileprovision')],stderr=subprocess.DEVNULL))
assert p['TeamIdentifier']==['ZZ8MTZ6NRZ']
assert p['Entitlements']['application-identifier']=='ZZ8MTZ6NRZ.net.leanzero.goose-remote'
assert p['Entitlements'].get('get-task-allow') is True
assert '00008150-000640A81442401C' in [d.upper() for d in p.get('ProvisionedDevices',[])]
e=plistlib.loads(subprocess.check_output(['codesign','-d','--entitlements',':-',str(app)],stderr=subprocess.DEVNULL))
assert e['application-identifier']=='ZZ8MTZ6NRZ.net.leanzero.goose-remote'
assert e.get('get-task-allow') is True
app_uuid=uuid(app/'GooseRemote')
assert app_uuid==uuid(artifact/'GooseRemote.app.dSYM/Contents/Resources/DWARF/GooseRemote')
assert uuid(framework/'TailscaleKit')=='59CE7913-265C-3FDF-8885-616B358E6352'
for actual,source in [(app/'PrivacyInfo.xcprivacy',Path('ui/ios/GooseRemote/Resources/PrivacyInfo.xcprivacy')),(framework/'PrivacyInfo.xcprivacy',Path('ui/ios/Dependencies/TailscaleKit/PrivacyInfo.xcprivacy'))]:
 assert actual.read_bytes()==source.read_bytes()
r={'captured_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'purpose':'development-only lifecycle diagnosis; temporary redacted MeshLifecycle events; NOT final acceptance candidate','source_commit':sys.argv[2],'native_tree':sys.argv[3],'app':str(app),'app_sha256':hashlib.sha256((app/'GooseRemote').read_bytes()).hexdigest(),'app_uuid':app_uuid,'matching_dsym':True,'strict_signature':True,'team':'ZZ8MTZ6NRZ','actual_profile_uuid':p['UUID'],'phone_included':True,'profile_and_app_get_task_allow':True,'mesh_linked':True,'pinned_framework_uuid_matches':True,'privacy_manifests_match':True,'archive_created':False,'exported':False,'uploaded':False,'installed':False}
(artifact/'verification.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r))
PYVERIFY
