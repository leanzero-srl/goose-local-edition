import XCTest
import SwiftUI
@testable import GooseRemote

@MainActor final class SessionSharingTests: XCTestCase {
    private func fixture(input: PendingInput? = nil, lostAcknowledgement: Bool = false, sharing: SessionSharingDescriptor? = nil) async throws -> (ControllerStore, SharingOwner, ScriptedTransport, CredentialEnvelope) {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let device = DeviceRecord(deviceId: UUID(), name: "Test phone", platform: "ios", role: "controller", revision: 1, scopes: [], createdAt: Date(), revokedAt: nil)
        let identity = CredentialEnvelope(authority: URL(string: "https://link.example")!, credentials: .init(accessToken: "test-access", tokenType: "Bearer", expiresAt: .distantFuture, refreshToken: "test-refresh", device: device))
        let storage = MemoryCredentials()
        try storage.write(identity, account: "device")
        let hosts = ["First Mac", "Second Mac"].map { name in
            HostDescriptor(ownerNodeId: UUID(), name: name, deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
                           endpoints: [.init(url: URL(string: "https://100.64.0.1:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))], capabilities: [], registrationState: nil, tlsSpkiSha256: nil)
        }
        let entries = hosts.map { host in
            CatalogEntry(work: .init(ownerNodeId: host.id, kind: .chat, workId: UUID()), storeGeneration: UUID(), revision: 42,
                         title: "Owner session", workingDir: nil, state: input == nil ? .running : .waitingInput,
                         currentRunId: input == nil ? UUID() : input!.runId, capabilities: [.send, .steer, .note, .answerInput, .stop],
                         parent: nil, related: [], updatedAt: Date(), lastEvent: nil, archived: false)
        }
        let owner = SharingOwner(entries: entries, input: input, device: device.id, lostAcknowledgement: lostAcknowledgement, sharing: sharing)
        let otherDevice = DeviceRecord(deviceId: UUID(), name: "Different account device", platform: "ios", role: "controller", revision: 1, scopes: [], createdAt: Date(), revokedAt: nil)
        let changed = DeviceCredentials(accessToken: "changed-access", tokenType: "Bearer", expiresAt: .distantFuture, refreshToken: "changed-refresh", device: otherDevice)
        let transport = ScriptedTransport([.response(200, try ControllerCodec.encoder().encode(HostPage(revision: 1, hosts: hosts))),
                                           .response(200, try ControllerCodec.encoder().encode(changed))])
        let store = ControllerStore(credentials: storage, owner: owner, transport: transport, directory: directory)
        await store.synchronize()
        await store.select(entries[0].work)
        XCTAssertNil(store.selectedError)
        return (store, owner, transport, identity)
    }

    func testDeclineEveryDataActionMakesNoTransportWriteAndKeepsDraft() async throws {
        let (store, owner, transport, _) = try await fixture()
        let work = try XCTUnwrap(store.selectedWork?.entry.work)
        store.saveDraft("Private original draft", for: work)
        let before = await transport.requests.count
        for action in [WorkAction.send, .steer, .note] {
            await store.send(action: action)
            XCTAssertEqual(store.sharingRequest?.command.action, action)
            store.declineSharing()
            XCTAssertNil(store.sharingRequest)
            XCTAssertTrue(store.pendingCommands.isEmpty)
            XCTAssertEqual(store.draft(for: work), "Private original draft")
        }
        let submissions = await owner.submissions
        let after = await transport.requests.count
        XCTAssertTrue(submissions.isEmpty)
        XCTAssertEqual(before, after)
        await store.sceneBackground()
    }

    func testConfirmPreservesExactOriginalCommandEvenIfDraftChanges() async throws {
        let (store, owner, _, _) = try await fixture()
        let work = try XCTUnwrap(store.selectedWork?.entry.work)
        for action in [WorkAction.send, .steer, .note] {
            store.saveDraft("Original \(action.rawValue)\n  spacing", for: work)
            await store.send(action: action)
            let request = try XCTUnwrap(store.sharingRequest)
            XCTAssertEqual(request.ownerName, "First Mac")
            XCTAssertTrue(request.disclosure.contains("third-party AI"))
            XCTAssertTrue(request.recipientLimitation.contains("does not report"))
            store.saveDraft("A new unsent draft", for: work)
            await store.confirmSharing(request.id)
            await store.confirmSharing(request.id)
            let submitted = await owner.submissions
            let sent = try XCTUnwrap(submitted.last)
            XCTAssertEqual(try ControllerCodec.encoder().encode(sent), try ControllerCodec.encoder().encode(request.command))
            XCTAssertEqual(store.draft(for: work), "A new unsent draft")
            XCTAssertNil(store.sharingRequest)
        }
        let submissions = await owner.submissions
        XCTAssertEqual(submissions.count, 3)
        await store.sceneBackground()
    }

    func testAnswerDeclineAndConfirmKeepExactInputChoiceAndValue() async throws {
        let choice = InputOption(optionId: "custom", label: "Use this answer", acceptsValue: true)
        let input = PendingInput(inputId: UUID(), runId: nil, prompt: "Which location?", options: [choice], payload: .question(itemId: "question", why: "", recommendedAnswer: "", originRunId: nil))
        let (store, owner, transport, _) = try await fixture(input: input)
        let before = await transport.requests.count
        await store.answer(input: input, option: choice, value: "Private address")
        store.declineSharing()
        let declined = await owner.submissions
        let after = await transport.requests.count
        XCTAssertTrue(declined.isEmpty)
        XCTAssertEqual(before, after)
        XCTAssertTrue(store.pendingCommands.isEmpty)
        await store.answer(input: input, option: choice, value: "Exact answer \n with spaces")
        let request = try XCTUnwrap(store.sharingRequest)
        XCTAssertTrue(request.reviewText.contains(choice.label))
        XCTAssertTrue(request.reviewText.contains("Exact answer \n with spaces"))
        await store.confirmSharing(request.id)
        let submitted = await owner.submissions
        let sent = try XCTUnwrap(submitted.last)
        XCTAssertEqual(try ControllerCodec.encoder().encode(sent), try ControllerCodec.encoder().encode(request.command))
        XCTAssertEqual(sent.payload.inputId, input.id)
        XCTAssertEqual(sent.payload.optionId, choice.id)
        XCTAssertEqual(sent.payload.value, "Exact answer \n with spaces")
        await store.sceneBackground()
    }

    func testChangingOwnerOrRemovingAccountInvalidatesPendingPermission() async throws {
        let (store, owner, _, _) = try await fixture()
        let first = try XCTUnwrap(store.selectedWork?.entry.work)
        store.saveDraft("First owner draft", for: first)
        await store.send(action: .send)
        let firstRequest = try XCTUnwrap(store.sharingRequest)
        let other = try XCTUnwrap(store.entries.first(where: { $0.work.ownerNodeId != first.ownerNodeId }))
        await store.select(other.work)
        XCTAssertNil(store.sharingRequest)
        await store.confirmSharing(firstRequest.id)
        store.saveDraft("Other owner draft", for: other.work)
        await store.send(action: .send)
        let otherRequest = try XCTUnwrap(store.sharingRequest)
        XCTAssertNotEqual(otherRequest.command.work.ownerNodeId, firstRequest.command.work.ownerNodeId)
        try await store.forgetDevice()
        XCTAssertNil(store.sharingRequest)
        await store.confirmSharing(otherRequest.id)
        let submissions = await owner.submissions
        XCTAssertTrue(submissions.isEmpty)
        XCTAssertTrue(store.pendingCommands.isEmpty)
    }

    func testPermissionIsBoundToAuthorityAndDeviceAndRefreshRejectsIdentityChange() async throws {
        let (store, owner, _, identity) = try await fixture()
        let work = try XCTUnwrap(store.selectedWork?.entry.work)
        store.saveDraft("Bound to original identity", for: work)
        await store.send(action: .send)
        let request = try XCTUnwrap(store.sharingRequest)
        XCTAssertTrue(request.matches(identity))
        XCTAssertFalse(request.matches(.init(authority: URL(string: "https://other-link.example")!, credentials: identity.credentials)))
        let otherDevice = DeviceRecord(deviceId: UUID(), name: "Another account", platform: "ios", role: "controller", revision: 1, scopes: [], createdAt: Date(), revokedAt: nil)
        let otherCredentials = DeviceCredentials(accessToken: "other", tokenType: "Bearer", expiresAt: .distantFuture, refreshToken: "other", device: otherDevice)
        XCTAssertFalse(request.matches(.init(authority: identity.authority, credentials: otherCredentials)))
        do {
            _ = try await store.validCredential(forceRefresh: true)
            XCTFail("A credential rotation must not switch enrolled devices")
        } catch { XCTAssertEqual(error as? ControllerError, .malformedResponse) }
        await store.confirmSharing(request.id)
        let submissions = await owner.submissions
        XCTAssertTrue(submissions.isEmpty)
        XCTAssertTrue(store.pendingCommands.isEmpty)
        XCTAssertNotNil(store.errorMessage)
        await store.sceneBackground()
    }

    func testLostAcknowledgementChecksOriginalReceiptWithoutResendOrConsent() async throws {
        let (store, owner, _, _) = try await fixture(lostAcknowledgement: true)
        let work = try XCTUnwrap(store.selectedWork?.entry.work)
        store.saveDraft("Recover exact delivery", for: work)
        await store.send(action: .send)
        let request = try XCTUnwrap(store.sharingRequest)
        await store.confirmSharing(request.id)
        XCTAssertEqual(store.unfinishedCommand?.id, request.id)
        XCTAssertNil(store.sharingRequest)
        await store.send(action: .send)
        XCTAssertNil(store.sharingRequest)
        await store.checkDelivery()
        let submissions = await owner.submissions
        let checks = await owner.receiptChecks
        XCTAssertEqual(submissions.count, 1)
        XCTAssertEqual(checks, [request.id])
        XCTAssertEqual(store.pendingCommands.last?.receipt?.state, .delivered)
        XCTAssertEqual(store.draft(for: work), "")
        XCTAssertNil(store.sharingRequest)
        await store.sceneBackground()
    }

    func testStructuredStaleRevisionRejectionAllowsNewExplicitlyReviewedCommand() async throws {
        let (store, owner, _, identity) = try await fixture()
        let host = try XCTUnwrap(store.hosts.first)
        let wire = StaleRevisionTransport(deviceId: identity.credentials.device.id)
        let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: try XCTUnwrap(host.endpoints.first), transport: wire))
        await owner.useSubmissionService(service)
        let work = try XCTUnwrap(store.selectedWork?.entry.work)
        store.saveDraft("An instruction that must not be retried automatically", for: work)
        await store.send(action: .steer)
        let original = try XCTUnwrap(store.sharingRequest)
        await store.confirmSharing(original.id)
        XCTAssertNil(store.unfinishedCommand)
        XCTAssertTrue(store.pendingCommands.isEmpty)
        XCTAssertEqual(store.draft(for: work), original.reviewText)
        XCTAssertTrue(store.errorMessage?.contains("did not accept") == true)
        let firstRequests = await wire.requests
        XCTAssertEqual(firstRequests.count, 1)
        XCTAssertNil(store.sharingRequest)
        await store.send(action: .steer)
        let fresh = try XCTUnwrap(store.sharingRequest)
        XCTAssertNotEqual(fresh.id, original.id)
        XCTAssertEqual(fresh.command.payload, original.command.payload)
        await store.confirmSharing(fresh.id)
        let requests = await wire.requests
        XCTAssertEqual(requests.map(\.httpMethod), ["POST", "POST"])
        XCTAssertEqual(store.pendingCommands.last?.receipt?.state, .delivered)
        XCTAssertEqual(store.pendingCommands.last?.id, fresh.id)
        await store.sceneBackground()
    }

    func testStreamProgressRefreshesOnlyRevisionAndChangedRunStillRefuses() async throws {
        let (store, owner, _, _) = try await fixture()
        let work = try XCTUnwrap(store.selectedWork?.entry.work)
        await owner.waitForStream()
        store.saveDraft("Keep the approved instruction", for: work)
        await store.send(action: .steer)
        let request = try XCTUnwrap(store.sharingRequest)
        try await owner.publishProgress()
        let progressed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            MainActor.assumeIsolated { store.selectedWork?.entry.revision == 43 }
        }, object: nil)
        await fulfillment(of: [progressed], timeout: 3)
        await store.confirmSharing(request.id)
        let submissions = await owner.submissions
        let sent = try XCTUnwrap(submissions.last)
        XCTAssertEqual(sent.expectedRevision, 43)
        XCTAssertEqual(sent.commandId, request.id)
        XCTAssertEqual(sent.work, request.command.work)
        XCTAssertEqual(sent.expectedRunId, request.command.expectedRunId)
        XCTAssertEqual(sent.expectedStoreGeneration, request.command.expectedStoreGeneration)
        XCTAssertEqual(sent.action, request.command.action)
        XCTAssertEqual(sent.payload, request.command.payload)
        store.saveDraft("Do not send to a replacement run", for: work)
        await store.send(action: .steer)
        let stale = try XCTUnwrap(store.sharingRequest)
        try await owner.publishProgress(changingRun: true)
        let changed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            MainActor.assumeIsolated { store.selectedWork?.entry.revision == 44 }
        }, object: nil)
        await fulfillment(of: [changed], timeout: 3)
        await store.confirmSharing(stale.id)
        let finalSubmissions = await owner.submissions
        XCTAssertEqual(finalSubmissions.count, 1)
        XCTAssertTrue(store.errorMessage?.contains("Nothing was sent") == true)
        await store.sceneBackground()
    }

    func testSessionScreenPresentsSharingSheetWithoutEndingSelection() async throws {
        let details = SessionSharingDescriptor(provenance: "live", provider: .init(name: "Anthropic", origin: "https://api.anthropic.com"),
            tools: [.init(name: "Project documents", transport: "http", origin: "https://tools.example.test")],
            limitations: ["provider_routing_not_reported", "tool_destinations_dynamic"])
        let (store, owner, _, _) = try await fixture(sharing: details)
        let work = try XCTUnwrap(store.selectedWork?.entry.work)
        let scene = try XCTUnwrap(UIApplication.shared.connectedScenes.first as? UIWindowScene)
        let previous = scene.keyWindow
        let window = UIWindow(windowScene: scene)
        let controller = UIHostingController(rootView: NavigationStack { SessionDetailView(work: work).environment(store) })
        window.rootViewController = controller
        window.overrideUserInterfaceStyle = .light
        window.makeKeyAndVisible()
        defer { window.isHidden = true; previous?.makeKey() }
        controller.view.layoutIfNeeded()
        store.saveDraft("Please summarize the project notes.", for: work)
        await store.send(action: .send)
        let request = try XCTUnwrap(store.sharingRequest)
        let presented = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            MainActor.assumeIsolated { controller.presentedViewController?.viewIfLoaded?.window != nil }
        }, object: nil)
        await fulfillment(of: [presented], timeout: 3)
        XCTAssertEqual(store.selectedRef, work)
        XCTAssertEqual(store.sharingRequest?.id, request.id)
        XCTAssertEqual(request.sharing, details)
        XCTAssertTrue(request.recipientLimitation.contains("not a complete list"))
        controller.presentedViewController?.view.layoutIfNeeded()
        let image = UIGraphicsImageRenderer(bounds: window.bounds).image { _ in
            window.drawHierarchy(in: window.bounds, afterScreenUpdates: true)
        }
        let attachment = XCTAttachment(image: image)
        attachment.name = "Session sharing with reported provider and tool"
        attachment.lifetime = .keepAlways
        add(attachment)
        let submissions = await owner.submissions
        XCTAssertTrue(submissions.isEmpty)
        store.declineSharing()
        await store.sceneBackground()
    }

    func testSnapshotCarriesKnownSharingAndOlderOwnersRemainReadable() async throws {
        let (store, _, _, _) = try await fixture()
        defer { Task { await store.sceneBackground() } }
        let entry = try XCTUnwrap(store.selectedWork?.entry)
        let host = try XCTUnwrap(store.hosts.first)
        let snapshot = WorkSnapshot(entry: entry, pendingInputs: [], historyWatermark: .init(ownerNodeId: host.id, storeGeneration: entry.storeGeneration, revision: entry.revision), workCursor: "snapshot", liveMessages: [])
        let page = HistoryPage(work: entry.work, storeGeneration: entry.storeGeneration, revision: entry.revision, items: [], nextPageToken: nil, workCursor: "history")
        let reported = #"{"provenance":"live","provider":{"name":"Anthropic","origin":"https://user:private-password@api.anthropic.com/v1?api_key=private-key#fragment","api_key":"never-decode"},"tools":[{"name":"Documents","transport":"http","origin":"https://tools.example.test:8443/private-path?token=private-token"},{"name":"Local command","transport":"stdio","origin":"file:///private-script"}],"limitations":["provider_routing_not_reported","tool_destinations_dynamic"]}"#
        for descriptor in [nil, reported] as [String?] {
            var body = try XCTUnwrap(JSONSerialization.jsonObject(with: ControllerCodec.encoder().encode(snapshot)) as? [String: Any])
            if let descriptor { body["sharing"] = try JSONSerialization.jsonObject(with: Data(descriptor.utf8)) }
            let wire = ScriptedTransport([.response(200, try JSONSerialization.data(withJSONObject: body)), .response(200, try ControllerCodec.encoder().encode(page))])
            let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: try XCTUnwrap(host.endpoints.first), transport: wire))
            let selected = try await service.selected(work: entry, host: host, credential: "test-access")
            if descriptor == nil {
                XCTAssertNil(selected.sharing)
            } else {
                let sharing = try XCTUnwrap(selected.sharing)
                XCTAssertEqual(sharing.provider?.name, "Anthropic")
                XCTAssertEqual(sharing.provider?.origin, "https://api.anthropic.com")
                XCTAssertEqual(sharing.tools[0].origin, "https://tools.example.test:8443")
                XCTAssertNil(sharing.tools[1].origin)
                let cached = String(decoding: try ControllerCodec.encoder().encode(selected), as: UTF8.self)
                for secret in ["private-password", "private-key", "private-token", "private-path", "private-script", "never-decode"] { XCTAssertFalse(cached.contains(secret)) }
            }
        }
    }

    func testSavedUnavailableAndUnknownProvenanceNeverClaimActiveOrLocalRecipients() throws {
        let unavailable = SharingProvider(name: "Configured driver", origin: nil)
        for provenance in ["saved", "unavailable", "future_source"] {
            let raw = "{\"provenance\":\"\(provenance)\",\"provider\":{\"name\":\"Configured driver\",\"origin\":null},\"tools\":[],\"limitations\":[\"future_limitation\"]}"
            let details = try ControllerCodec.decoder().decode(SessionSharingDescriptor.self, from: Data(raw.utf8))
            XCTAssertEqual(details.provider, unavailable)
            XCTAssertNil(details.provider?.origin)
            XCTAssertFalse(details.sourceDescription.contains("Reported by the active"))
            XCTAssertEqual(details.showsReportedServices, provenance == "saved")
            if provenance == "saved" { XCTAssertTrue(details.sourceDescription.contains("have not been verified")) }
            if provenance == "future_source" { XCTAssertFalse(details.hasKnownProvenance) }
        }
        for origin in ["file:///private/script", "local-model", "javascript:alert(1)", "https://", ""] {
            XCTAssertNil(SessionSharingDescriptor.safeOrigin(origin))
        }
    }

    func testStopNeedsNoSharingPermission() async throws {
        let (store, owner, _, _) = try await fixture()
        await store.send(action: .stop)
        let submissions = await owner.submissions
        XCTAssertEqual(submissions.map(\.action), [.stop])
        XCTAssertNil(store.sharingRequest)
        await store.sceneBackground()
    }
}

private actor SharingOwner: OwnerControllerService {
    var entries: [CatalogEntry]
    let input: PendingInput?
    let device: UUID
    let lostAcknowledgement: Bool
    let sharing: SessionSharingDescriptor?
    private(set) var submissions: [WorkCommand] = []
    private(set) var receiptChecks: [UUID] = []
    private var submissionService: (any OwnerControllerService)?
    func useSubmissionService(_ service: any OwnerControllerService) { submissionService = service }
    private var frames: AsyncThrowingStream<WorkFrame, Error>.Continuation?
    private var streamReady: CheckedContinuation<Void, Never>?
    func waitForStream() async {
        if frames == nil { await withCheckedContinuation { streamReady = $0 } }
    }
    func publishProgress(changingRun: Bool = false) throws {
        entries[0].revision += 1
        if changingRun { entries[0].currentRunId = UUID() }
        let entry = entries[0]
        let payload: JSONValue
        if changingRun {
            payload = .object(["entry": try ControllerCodec.decoder().decode(JSONValue.self, from: ControllerCodec.encoder().encode(entry))])
        } else {
            payload = .object(["message_id": .string("live"), "message_revision": .unsignedInteger(entry.revision), "offset_bytes": .unsignedInteger(0),
                               "content": .object(["role": .string("assistant"), "content": .array([.object(["type": .string("text"), "text": .string("Working on the task")])])])])
        }
        frames?.yield(.init(resumeCursor: "work-\(entry.revision)", work: entry.work, storeGeneration: entry.storeGeneration, streamEpoch: entry.storeGeneration,
                            sequence: entry.revision, workRevision: entry.revision, runId: entry.currentRunId,
                            type: changingRun ? "state_changed" : "partial_replacement", payload: payload))
    }
    init(entries: [CatalogEntry], input: PendingInput?, device: UUID, lostAcknowledgement: Bool, sharing: SessionSharingDescriptor?) {
        self.entries = entries; self.input = input; self.device = device; self.lostAcknowledgement = lostAcknowledgement; self.sharing = sharing
    }
    func catalog(host: HostDescriptor, credential: String) async throws -> OwnerCatalogSnapshot {
        let entry = entries.first { $0.work.ownerNodeId == host.id }!
        return .init(entries: [entry], watermark: .init(ownerNodeId: host.id, storeGeneration: entry.storeGeneration, revision: entry.revision))
    }
    func selected(work: CatalogEntry, host: HostDescriptor, credential: String) async throws -> SelectedWork {
        let entry = entries.first { $0.work == work.work }!
        return .init(entry: entry, transcript: [], pendingInputs: input.map { [$0] } ?? [], nextPageToken: nil, historyRevision: entry.revision, sharing: sharing)
    }
    func submit(_ command: WorkCommand, host: HostDescriptor, credential: String) async throws -> CommandReceipt {
        submissions.append(command)
        if let submissionService { return try await submissionService.submit(command, host: host, credential: credential) }
        if lostAcknowledgement { throw URLError(.networkConnectionLost) }
        return delivered(command)
    }
    func receipt(commandId: UUID, host: HostDescriptor, credential: String) async throws -> CommandReceipt {
        receiptChecks.append(commandId)
        return delivered(submissions.first { $0.commandId == commandId }!)
    }
    private func delivered(_ command: WorkCommand) -> CommandReceipt {
        .init(commandId: command.commandId, work: command.work, authenticatedDeviceId: device, revision: 43, state: .delivered,
              runId: command.expectedRunId, messageId: nil, inputId: command.payload.inputId, checkpointId: nil, reason: nil, updatedAt: Date())
    }
    func catalogEvents(cursor: String, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<CatalogChange, Error> { AsyncThrowingStream { _ in } }
    func workEvents(work: SelectedWork, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<WorkFrame, Error> {
        let (stream, continuation) = AsyncThrowingStream<WorkFrame, Error>.makeStream()
        frames = continuation
        streamReady?.resume(); streamReady = nil
        return stream
    }
    func history(work: CatalogEntry, pageToken: String, host: HostDescriptor, credential: String) async throws -> HistoryPage { throw ControllerError.unsupported("unused") }
    func resources(host: HostDescriptor, credential: String) async throws -> ResourceSample { throw ControllerError.unsupported("unused") }
}

private actor StaleRevisionTransport: HTTPTransport {
    let deviceId: UUID
    private(set) var requests: [URLRequest] = []
    init(deviceId: UUID) { self.deviceId = deviceId }
    func data(for request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        requests.append(request)
        if requests.count == 1 {
            let body = Data(#"{"error":{"code":"stale_revision","message":"work changed; refresh before issuing this command","retryable":false}}"#.utf8)
            return (body, HTTPURLResponse(url: request.url!, statusCode: 409, httpVersion: "HTTP/1.1", headerFields: [:])!)
        }
        let command = try ControllerCodec.decoder().decode(WorkCommand.self, from: XCTUnwrap(request.httpBody))
        let receipt = CommandReceipt(commandId: command.commandId, work: command.work, authenticatedDeviceId: deviceId, revision: command.expectedRevision,
                                     state: .delivered, runId: command.expectedRunId, messageId: nil, inputId: nil, checkpointId: nil, reason: nil, updatedAt: Date())
        return (try ControllerCodec.encoder().encode(receipt), HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: [:])!)
    }
}
