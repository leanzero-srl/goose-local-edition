#!/usr/bin/env python3
"""Deterministic Xcode project generator; Python standard library only."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / "GooseRemote.xcodeproj"
OBJECTS = {}
def ident(name):
    return hashlib.sha256(name.encode()).hexdigest()[:24].upper()
def put(name, body):
    key = ident(name)
    OBJECTS[key] = body
    return key
def quote(value):
    return json.dumps(str(value))
def array(values):
    return "(" + ", ".join(values) + ")"
def settings(values):
    return "{ " + " ".join(f"{k} = {quote(v)};" for k, v in values.items()) + " }"
def file(path, filetype):
    return put("file:" + path, f'isa = PBXFileReference; lastKnownFileType = {filetype}; path = {quote(path)}; sourceTree = "<group>";')
def phase(name, isa, files):
    builds = [put("build:" + name + ":" + f, f"isa = PBXBuildFile; fileRef = {f};") for f in files]
    return put(name, f"isa = {isa}; buildActionMask = 2147483647; files = {array(builds)}; runOnlyForDeploymentPostprocessing = 0;")

app_files = [file(str(p.relative_to(ROOT)), "sourcecode.swift") for p in sorted((ROOT / "GooseRemote").rglob("*.swift"))]
test_files = [file(str(p.relative_to(ROOT)), "sourcecode.swift") for p in sorted((ROOT / "GooseRemoteTests").rglob("*.swift"))]
assets = file("GooseRemote/Resources/Assets.xcassets", "folder.assetcatalog")
privacy = file("GooseRemote/Resources/PrivacyInfo.xcprivacy", "text.xml")
info = file("GooseRemote/Resources/Info.plist", "text.plist.xml")
app_product = put("appProduct", 'isa = PBXFileReference; explicitFileType = wrapper.application; path = GooseRemote.app; sourceTree = BUILT_PRODUCTS_DIR;')
test_product = put("testProduct", 'isa = PBXFileReference; explicitFileType = wrapper.cfbundle; path = GooseRemoteTests.xctest; sourceTree = BUILT_PRODUCTS_DIR;')
product_group = put("products", f'isa = PBXGroup; name = Products; sourceTree = "<group>"; children = {array([app_product,test_product])};')
main_group = put("mainGroup", f'isa = PBXGroup; sourceTree = "<group>"; children = {array(app_files + test_files + [assets, privacy, info, product_group])};')
app_sources = phase("appSources", "PBXSourcesBuildPhase", app_files)
test_sources = phase("testSources", "PBXSourcesBuildPhase", test_files)
resources = phase("resources", "PBXResourcesBuildPhase", [assets, privacy])
frameworks = phase("frameworks", "PBXFrameworksBuildPhase", [])
embed = put("meshEmbed", 'isa = PBXShellScriptBuildPhase; buildActionMask = 2147483647; files = (); inputPaths = (); outputPaths = (); alwaysOutOfDate = 1; runOnlyForDeploymentPostprocessing = 0; shellPath = /bin/sh; shellScript = "\\\"$SRCROOT/scripts/embed-mesh-framework.sh\\\"";')

common = {"SDKROOT":"iphoneos", "IPHONEOS_DEPLOYMENT_TARGET":"18.1", "SWIFT_VERSION":"5.0", "CLANG_ENABLE_MODULES":"YES", "ENABLE_STRICT_OBJC_MSGSEND":"YES", "CLANG_ENABLE_OBJC_ARC":"YES", "SWIFT_STRICT_CONCURRENCY":"complete", "TARGETED_DEVICE_FAMILY":"1,2", "SUPPORTED_PLATFORMS":"iphoneos iphonesimulator", "ENABLE_USER_SCRIPT_SANDBOXING":"NO"}
app = {"PRODUCT_BUNDLE_IDENTIFIER":"net.leanzero.goose-remote", "PRODUCT_NAME":"GooseRemote", "INFOPLIST_FILE":"GooseRemote/Resources/Info.plist", "GENERATE_INFOPLIST_FILE":"NO", "MARKETING_VERSION":"1.0.0", "CURRENT_PROJECT_VERSION":"1", "CODE_SIGN_STYLE":"Automatic", "CODE_SIGN_ENTITLEMENTS":"GooseRemote/Resources/GooseRemote.entitlements", "LD_RUNPATH_SEARCH_PATHS":"$(inherited) @executable_path/Frameworks", "FRAMEWORK_SEARCH_PATHS":"$(inherited) $(GOOSE_TAILSCALE_FRAMEWORK_DIR)", "OTHER_LDFLAGS":"$(inherited) $(GOOSE_MESH_LDFLAGS)", "ASSETCATALOG_COMPILER_GLOBAL_ACCENT_COLOR_NAME":"AccentColor"}
tests = {"PRODUCT_BUNDLE_IDENTIFIER":"net.leanzero.goose-remote.tests", "PRODUCT_NAME":"GooseRemoteTests", "GENERATE_INFOPLIST_FILE":"YES", "TEST_HOST":"$(BUILT_PRODUCTS_DIR)/GooseRemote.app/$(BUNDLE_EXECUTABLE_FOLDER_PATH)/GooseRemote", "BUNDLE_LOADER":"$(TEST_HOST)"}
def configs(name, own):
    refs = []
    for mode in ["Debug", "Release"]:
        vals = dict(own)
        vals.update({"SWIFT_OPTIMIZATION_LEVEL":"-Onone" if mode == "Debug" else "-O", "DEBUG_INFORMATION_FORMAT":"dwarf" if mode == "Debug" else "dwarf-with-dsym", "ENABLE_TESTABILITY":"YES" if mode == "Debug" else "NO", "ONLY_ACTIVE_ARCH":"YES" if mode == "Debug" else "NO", "SWIFT_ACTIVE_COMPILATION_CONDITIONS":"DEBUG" if mode == "Debug" else ""})
        refs.append(put(name + mode, f'isa = XCBuildConfiguration; name = {mode}; buildSettings = {settings(vals)};'))
    return put(name + "Configs", f"isa = XCConfigurationList; buildConfigurations = {array(refs)}; defaultConfigurationIsVisible = 0; defaultConfigurationName = Release;")
project_configs = configs("project", common)
app_configs = configs("app", app)
test_configs = configs("test", tests)
app_id = ident("appTarget")
project_id = ident("project")
proxy = put("proxy", f'isa = PBXContainerItemProxy; containerPortal = {project_id}; proxyType = 1; remoteGlobalIDString = {app_id}; remoteInfo = GooseRemote;')
dependency = put("testDependency", f'isa = PBXTargetDependency; target = {app_id}; targetProxy = {proxy};')
put("appTarget", f'isa = PBXNativeTarget; buildConfigurationList = {app_configs}; buildPhases = {array([app_sources, frameworks, resources, embed])}; buildRules = (); dependencies = (); name = GooseRemote; productName = GooseRemote; productReference = {app_product}; productType = "com.apple.product-type.application";')
test_id = put("testTarget", f'isa = PBXNativeTarget; buildConfigurationList = {test_configs}; buildPhases = {array([test_sources])}; buildRules = (); dependencies = ({dependency}); name = GooseRemoteTests; productName = GooseRemoteTests; productReference = {test_product}; productType = "com.apple.product-type.bundle.unit-test";')
put("project", f'isa = PBXProject; attributes = {{ BuildIndependentTargetsInParallel = YES; LastUpgradeCheck = 2710; TargetAttributes = {{ {app_id} = {{ CreatedOnToolsVersion = 27.1; }}; {test_id} = {{ CreatedOnToolsVersion = 27.1; TestTargetID = {app_id}; }}; }}; }}; buildConfigurationList = {project_configs}; compatibilityVersion = "Xcode 14.0"; developmentRegion = en; hasScannedForEncodings = 0; knownRegions = (en, Base); mainGroup = {main_group}; productRefGroup = {product_group}; projectDirPath = ""; projectRoot = ""; targets = {array([app_id,test_id])};')
PROJECT.mkdir(parents=True, exist_ok=True)
(PROJECT / "project.pbxproj").write_text("// !$*UTF8*$!\n{\n archiveVersion = 1;\n classes = {};\n objectVersion = 56;\n objects = {\n" + "\n".join(f"  {k} = {{ {v} }};" for k,v in sorted(OBJECTS.items())) + f"\n }};\n rootObject = {project_id};\n}}\n")
scheme = PROJECT / "xcshareddata/xcschemes/GooseRemote.xcscheme"
scheme.parent.mkdir(parents=True, exist_ok=True)
def reference(id, name):
    return f'<BuildableReference BuildableIdentifier="primary" BlueprintIdentifier="{id}" BuildableName="{name}" BlueprintName="{name.split(".")[0]}" ReferencedContainer="container:GooseRemote.xcodeproj"/>'
scheme.write_text(f'''<?xml version="1.0" encoding="UTF-8"?>
<Scheme LastUpgradeVersion="2710" version="1.7">
<BuildAction parallelizeBuildables="YES" buildImplicitDependencies="YES"><BuildActionEntries><BuildActionEntry buildForTesting="YES" buildForRunning="YES" buildForProfiling="YES" buildForArchiving="YES" buildForAnalyzing="YES">{reference(app_id,'GooseRemote.app')}</BuildActionEntry></BuildActionEntries></BuildAction>
<TestAction buildConfiguration="Debug" selectedDebuggerIdentifier="Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier="Xcode.IDEFoundation.Launcher.LLDB" shouldUseLaunchSchemeArgsEnv="YES"><Testables><TestableReference skipped="NO">{reference(test_id,'GooseRemoteTests.xctest')}</TestableReference></Testables></TestAction>
<LaunchAction buildConfiguration="Debug" selectedDebuggerIdentifier="Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier="Xcode.IDEFoundation.Launcher.LLDB" launchStyle="0" useCustomWorkingDirectory="NO" ignoresPersistentStateOnLaunch="NO" debugDocumentVersioning="YES" debugServiceExtension="internal" allowLocationSimulation="YES"><BuildableProductRunnable runnableDebuggingMode="0">{reference(app_id,'GooseRemote.app')}</BuildableProductRunnable></LaunchAction>
<ProfileAction buildConfiguration="Release" shouldUseLaunchSchemeArgsEnv="YES" savedToolIdentifier="" useCustomWorkingDirectory="NO" debugDocumentVersioning="YES"><BuildableProductRunnable runnableDebuggingMode="0">{reference(app_id,'GooseRemote.app')}</BuildableProductRunnable></ProfileAction>
<AnalyzeAction buildConfiguration="Debug"/><ArchiveAction buildConfiguration="Release" revealArchiveInOrganizer="YES"/>
</Scheme>
''')
print(PROJECT)
