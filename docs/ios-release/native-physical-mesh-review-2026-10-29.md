# Independent physical-phone mesh lifecycle review — 2026-10-29

Verdict: HOLD on claiming the physical stall fixed. HIGH confidence in the cancellation/resume liveness defect in source; MEDIUM confidence it explains the physical incident; exact native/Go blocked await remains unobserved. A cancellation-generation and serialized-shutdown repair is justified as a safety net, not yet the proven physical root-cause fix.

Read-only review: no product edits, physical-device actions, deployments, credential copies, re-enrollment, auth changes, or heavy tests. Coordinated with native_expiry_recovery, which owns any repair. Primary incident report /tmp/goose-physical-mesh-diagnosis.md. Parent observed actual55Gabee/557laptop/165workhorse catalogs after user OTP, a brief phone lock, then >3minutes busy fleet and selected-work error that the embedded mesh must be connected. Installed15a3625e41; relevant EmbeddedMesh/App lifecycle code is unchanged from current source. Parent/author's safe authority inspection shows device333b2a27-9958-4195-8341-2256696e8287 revision1 unrevoked with14scopes and confirmed enrollmented48f07d-04da-4a89-999c-d287db99d22c. No need for reenrollment is established.

## Primary source read

Complete EmbeddedMesh.swift and NativeOwnerTransport.swift; ControllerStore manualSync/select/stream/cancellation/active/background ownership; GooseRemoteApp scene callbacks; MeshPeerReadiness.swift; EmbeddedMeshTransportTests.swift. Pinned framework wrapper /tmp/goose-libtailscale-ios-d1dc45d/swift/TailscaleKit/TailscaleNode.swift and URLSession+Tailscale.swift; C/Go TsnetUp/TsnetClose/Loopback/StatusJSON in tailscale.go; actual tailscale.comv1.104.1 Server.Dial/Up/Loopback/Close, netstack.Close, tsdial.Close and SOCKS dialing. Verified the examined tsnet.Close implementation against the cached module ZIP, not an inferred older upstream version.

## Confirmed liveness defect

ControllerStore.sceneBackground cancels syncTask but retains the task reference and syncRunning=true until its deferred cleanup runs. sceneActive captures that task and awaits its value before manualSync; manualSync itself refuses while syncTask is nonnil. Cancellation is cooperative: any pending operation that does not finish on cancellation therefore blocks all resumed fleet synchronization indefinitely. This is a deterministic source property, independently of which concrete phone operation was slow.

Mesh prepare always starts with await suspend. suspend immediately changes lifecycle, closes owner transports, clears them, sets node=nil and then awaits oldNode.close. Thus nil-node plus busy fleet is consistent with either a newprepare awaiting shutdown or a canceled oldsync preventing foregroundprepare. Old completed syncResults remain until prepare and authority hosts both succeed, so completed owner rows do not locate current progress. The exact displayed message is the node==nil guard in connection(for:), not missing framework, revoked authority, peer discovery or TLS failure.

A second ordering hazard is visible: prepare captures its lifecycle token AFTER awaiting suspend. If it is canceled/superseded during that await, it can resume and borrow the newer lifecycle token before any cancellation check, creating/starting a node until the later up cancellation handler runs. The repair should capture operation ownership before suspension and reject stale/canceled work before creating a replacement node, rather than relying only on the post-up guard.

## Bridge/transport findings and rejected guesses

TailscaleNode.up runs tailscale_up in a detached Task; Swift cancellation alone does not end it. The app explicitly schedules current.close on up cancellation. TailscaleNode.close is an async actor method but invokes synchronous tailscale_close on that actor, so cancellation cannot preempt an ongoing close and further methods on the same actor cannot execute until it returns.

Go TsnetClose deletes the server handle under servers.mu, RELEASES the mutex, then invokes tsnet.Server.Close. Therefore a global server-map lock held throughout shutdown is NOT this code's deadlock. Server.close similarly releases s.mu after closing listeners and before heavy shutdown, so the historical s.mu/gVisor callback deadlock is already avoided in this exact dependency. Do not patch either based on a remembered upstream bug.

Server.close contains netstack.Close, LocalBackend.Shutdown, network-monitor/dialer shutdown and waits. netstack.Close calls ipstack.Close, ipstack.Wait and injectWG.Wait. These provide potentially blocking shutdown points but no actual phone stack identifies one. Its5second context only bounds some log/local-API shutdown work, not the entire close. Do not infer the whole close must return within5seconds.

Owner HTTP start/read cancellation handlers call NWConnection.cancel and have transport deadlines; WSS cancellation closes its connection. MeshPeerReadiness checks Task cancellation per poll; its statusJSON bridge has a10second Go context. SOCKS TCP dialing has a5second context. These weaken the hypothesis of a simple unbounded owner-connect retry; they do not prove every callback completes after iOS suspension. The actual app routes owner requests through SOCKS+Network.framework, not OutgoingConnection/TsnetDial, so editing the unused direct-dial wrapper would not address this user path.

The existing embedded test does two orderly node reconstruction cycles after requests; it does not lock a physical phone, cancel in-flightprepare/reads, or overlap pending shutdown with resume. Its success cannot establish this lifecycle behavior.

## Safe repair shape and proof

Retire canceled sync ownership on background with a new sync generation; fence EVERY publication and deferred cleanup so a late old task cannot clear replacement syncTask/syncRunning or publish old catalogs/errors. Foreground should not wait indefinitely on a retired owner read. Do not merely nil syncTask without generation fences: the old defer currently unconditionally clears shared fields.

Track one mesh shutdown operation and serialize replacement node construction behind its completion; all callers must join the same shutdown, not observe node=nil and conclude shutdown is complete while another close is pending. Maintain operation/lifecycle identity across awaits and check Task cancellation before constructing/up/confirming/publishing nodes. Late failure cleanup must only close the node owned by that operation. Preserve the existing saved identity and authority confirmation; do not relax the mesh guard, TLS binding, scopes or token checks. A truly hung native close still needs direct diagnosis; allowing a replacement to reuse its state directory while oldclose runs is not an acceptable workaround.

Focused proofs: latch an owner read that ignores cancellation, background then active, show a fresh sync can progress and releasing oldread cannot overwrite it; latch oldclose and show replacement cannot start until that exact close completes; cancel prepare while awaiting shutdown and show it cannot create/confirm a node after retirement; overlap background/active/background and verify latest intent owns mesh+sync state; preserve identity and authorized owner HTTP/WSS after orderly close/recreate through the real embedded harness. Finally install only under parent authorization and reproduce actual physical lock/unlock with same account and live catalogs, capturing lifecycle phase start/end if the stall recurs. No success claim based solely on latch tests or simulator.

Precise physical cause remains unproven because no supported phone log/stack was available in the incident report. Report this limit rather than attributing the stall to a guessed Go mutex.

## Candidate repair review — 10:04, uncommitted diff

Reviewed ControllerStore, EmbeddedMesh and three new ControllerLifecycleTests. The store now retires canceled sync ownership immediately, uses a request UUID for deferred cleanup, changes access/selection epochs, and waits for a meshSuspension chain rather than a canceled predecessor request. Existing post-host/catalog cancellation and epoch checks prevent retired requests publishing results; old deferred completion cannot clear a replacement's busy/task state. Background manualSync is rejected. The provider claims its lifecycle before waiting for close, joins a shared closingTask, and checks cancellation before constructing the replacement. This prevents the observed source overlap; it does not make a blocked native close finish.

Confirmed remaining boundary sent to author: prepare checks ownership after up but awaits statusJSON and then calls confirmMesh without rechecking ownership. Background/replacement can retire the operation during statusJSON. The post-confirm check prevents local publication but not a stale authority confirmation. Add a pre-confirm operation/cancellation check; do not claim this is the physical stall's cause.

Rejected suspicion: closeNode's tasks form a predecessor chain, not a self-wait cycle. All are actor-isolated and capture the previous task before assigning the new one. Cancellation's direct current.close uses the same TailscaleNode actor, so concurrent closes serialize; a duplicate error is swallowed and does not demonstrate a replacement-state overlap. A canceled oldprepare cannot close a newer node through its catch because the lifecycle check gates cleanup.

Evidence limitation: the three tests use a fake ControllerMeshSession. They exercise store sync retirement, background refusal and waiting for suspend; they do not execute the provider's closingTask or cancellation during native status. Reviewer independently read joblaptop-20261029-100320-26a8:34 tests, one failure in testBackgroundRejectsManualSyncUntilForegroundReturns; all16 credential-recovery tests passed. Candidate is NOT yet test-green. Actual physical lock/unlock remains the final acceptance gate regardless of subsequent unit results.

### Revised candidate — post-status guard and diagnostic privacy

Author added the operation/lifecycle/cancellation guard immediately after statusJSON and before confirmMesh; this resolves the confirmed stale-confirm dispatch boundary. Reviewed temporary MeshLifecycle logging: every stage is a literal string and the optional operation is a freshly generated UUID, not device/enrollment/node/work identity. It logs no credentials, URL, content, error payload or user name. This is appropriate for parent's requested physical investigation and must be removed before final release unless explicitly retained.

The added real-provider cancellation test exercises entry with an already-canceled task and proves no enrollment/storage side effect; it does not exercise cancellation DURING pending close/status, nor shared-close joining. A test portability defect was sent to author: cancellation check is under canImport(TailscaleKit), but the unconditional test expects CancellationError even when the framework is absent; that configuration returns unavailable. Either gate this test or deliberately make the cancellation check common to both configurations. Logger.info's persistence should also match the intended physical capture path; it may be absent from later collection.

## Final source verdict for revised candidate

PASS as a cancellation/shutdown safety net; combined tests and actual physical acceptance remain pending. Verified the pre-cancel check is now outside canImport, diagnostics use notice, and queueCredentialSync checks cancellation after both awaits before clearing its shared slot. The latter correctly prevents a retired queue callback from clearing a newer queue after background/resume. Post-status ownership check remains present. No confirmed remaining source blocker was found in this reviewed diff.

This source verdict does not upgrade the physical incident's cause from unobserved to proven. Shared shutdown deliberately waits for real native close; a genuinely hung native close still blocks. The new pre-canceled-provider test does not prove cancellation during close/status, and store latches still use a fake provider. Actual physical same-identity lock/unlock and visible catalog/session recovery must decide the incident verdict. No claim of cold-start recovery is made: parent attempted UI dismissal but has not established the app actually terminated.

## Exact committed test receipt

Lifecycle source commit94b1b2d6de was included in combined joblaptop-20261029-100921-a1fd. Reviewer independently read the result:63 tests, zero failures, TEST SUCCEEDED; lifecycle/auth/controller35 plus scroll28. Result bundle /tmp/goose-latest-native-derived/Logs/Test/Test-GooseRemote-2026.10.29_10-09-22-+0200.xcresult. This replaces the earlier failed candidate test status without erasing that receipt. Lifecycle source/test verdict PASS as a safety net; physical lock/unlock acceptance is still pending. No tests were rerun by reviewer.
