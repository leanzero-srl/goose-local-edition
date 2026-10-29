import XCTest
@testable import GooseRemote

final class MemoryCredentials: CredentialStorage, @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String: Data] = [:]
    func read<T: Decodable>(_ type: T.Type, account: String) throws -> T? {
        lock.lock(); defer { lock.unlock() }
        guard let value = values[account] else { return nil }
        return try ControllerCodec.decoder().decode(type, from: value)
    }
    func write<T: Encodable>(_ value: T, account: String) throws {
        lock.lock(); defer { lock.unlock() }
        values[account] = try ControllerCodec.encoder().encode(value)
    }
    func remove(account: String) throws {
        lock.lock(); defer { lock.unlock() }
        values[account] = nil
    }
    func removeRevocationRequests() throws {
        lock.lock(); defer { lock.unlock() }
        values = values.filter { !($0.key.hasPrefix("revoke-") && UUID(uuidString: String($0.key.dropFirst(7))) != nil) }
    }
}

actor ScriptedTransport: HTTPTransport {
    enum Reply: Sendable { case response(Int, Data), disconnected }
    var replies: [Reply]
    private(set) var requests: [URLRequest] = []
    init(_ replies: [Reply]) { self.replies = replies }
    func data(for request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        requests.append(request)
        guard !replies.isEmpty else { throw URLError(.badServerResponse) }
        switch replies.removeFirst() {
        case .disconnected: throw URLError(.networkConnectionLost)
        case .response(let status, let data):
            return (data, HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: "HTTP/1.1", headerFields: [:])!)
        }
    }
}

final class ControllerTests: XCTestCase {
    private let owner = UUID(uuidString: "11111111-1111-4111-8111-111111111111")!
    private let workId = UUID(uuidString: "22222222-2222-4222-8222-222222222222")!
    private let generation = UUID(uuidString: "33333333-3333-4333-8333-333333333333")!
    private let run = UUID(uuidString: "44444444-4444-4444-8444-444444444444")!
    private let deviceId = UUID(uuidString: "55555555-5555-4555-8555-555555555555")!
    private func entry(kind: WorkKind = .chat, state: WorkState = .running, runId: UUID? = UUID(uuidString: "44444444-4444-4444-8444-444444444444"), capabilities: [WorkAction] = [.send, .steer, .stop]) -> CatalogEntry {
        .init(work: .init(ownerNodeId: owner, kind: kind, workId: workId), storeGeneration: generation, revision: 42,
              title: "Real owner session", workingDir: nil, state: state, currentRunId: runId, capabilities: capabilities,
              parent: nil, related: [], updatedAt: Date(), lastEvent: nil, archived: false)
    }
    private func credentials(expiry: Date, token: String = "test-access", refresh: String = "test-refresh") -> DeviceCredentials {
        .init(accessToken: token, tokenType: "Bearer", expiresAt: expiry, refreshToken: refresh,
              device: .init(deviceId: deviceId, name: "Test controller", platform: "ios", role: "controller", revision: 1,
                            scopes: ["catalog.read"], createdAt: Date(timeIntervalSince1970: 1_700_000_000), revokedAt: nil))
    }

    func testRealDeviceBoundKeychainRoundTrip() throws {
        let store = KeychainStore()
        let account = "test-" + UUID().uuidString
        defer { try? store.remove(account: account) }
        XCTAssertNil(try store.read(String.self, account: account))
        do {
            try store.write("private-test-value", account: account)
            XCTAssertEqual(try store.read(String.self, account: account), "private-test-value")
            try store.write("rotated-test-value", account: account)
            XCTAssertEqual(try store.read(String.self, account: account), "rotated-test-value")
            try store.remove(account: account)
            XCTAssertNil(try store.read(String.self, account: account))
        } catch ControllerError.credentialStore(let status) {
            XCTFail("Actual Keychain OSStatus: \(status)")
        }
    }

    func testWorkIdentityIncludesOwnerAndKind() {
        let original = entry().work
        XCTAssertNotEqual(original.id, WorkRef(ownerNodeId: UUID(), kind: .chat, workId: workId).id)
        XCTAssertNotEqual(original.id, WorkRef(ownerNodeId: owner, kind: .shellJob, workId: workId).id)
    }
    func testTypedActionsPreserveOwnerRunGenerationAndRevision() throws {
        let id = UUID()
        let command = try WorkCommand.make(entry: entry(), action: .steer, text: "Keep the API compatible", id: id)
        XCTAssertEqual(command.commandId, id)
        XCTAssertEqual(command.work.ownerNodeId, owner)
        XCTAssertEqual(command.expectedRunId, run)
        XCTAssertEqual(command.expectedStoreGeneration, generation)
        XCTAssertEqual(command.expectedRevision, 42)
        XCTAssertEqual(command.payload.content, [.text("Keep the API compatible")])
        let json = try JSONSerialization.jsonObject(with: ControllerCodec.encoder().encode(command)) as! [String: Any]
        XCTAssertNotNil(json["command_id"])
        XCTAssertEqual(json["action"] as? String, "steer")
        XCTAssertNil(json["device_id"])
    }
    func testStaleRunAndUnsupportedActionNeverBecomeSend() throws {
        XCTAssertThrowsError(try WorkCommand.make(entry: entry(state: .idle, runId: nil), action: .stop, text: ""))
        XCTAssertThrowsError(try WorkCommand.make(entry: entry(kind: .shellJob, capabilities: [.stop]), action: .steer, text: "run something"))
        XCTAssertThrowsError(try WorkCommand.make(entry: entry(state: .deleted), action: .send, text: "hello"))
        XCTAssertThrowsError(try WorkCommand.make(entry: entry(), action: .send, text: " \n "))
    }
    func testAnswerRequiresExactRunAndCurrentChoice() throws {
        let choice = InputOption(optionId: "allow", label: "Allow once", acceptsValue: false)
        let input = PendingInput(inputId: UUID(), runId: run, prompt: "Allow?", options: [choice], payload: .permission(requestId: "request", toolName: "developer__shell", arguments: .object(["command": .string("pwd")])))
        let current = entry(state: .waitingInput, capabilities: [.answerInput])
        let command = try WorkCommand.answer(entry: current, input: input, option: choice, value: nil)
        XCTAssertEqual(command.expectedRunId, run)
        XCTAssertEqual(command.payload.inputId, input.id)
        XCTAssertEqual(command.payload.optionId, "allow")
        XCTAssertThrowsError(try WorkCommand.answer(entry: entry(state: .waitingInput, runId: UUID(), capabilities: [.answerInput]), input: input, option: choice, value: nil))
        XCTAssertThrowsError(try WorkCommand.answer(entry: current, input: input, option: choice, value: "unpermitted value"))
        XCTAssertThrowsError(try WorkCommand.answer(entry: current, input: input, option: .init(optionId: "invented", label: "Invented", acceptsValue: false), value: nil))
    }
    func testIdleSendCarriesNoRun() throws {
        let command = try WorkCommand.make(entry: entry(state: .idle, runId: nil), action: .send, text: "Start the work")
        XCTAssertNil(command.expectedRunId)
        XCTAssertEqual(command.expectedRevision, 42)
    }
    func testAuthorityRejectsCredentialsInURLAndNonHTTPS() throws {
        for bad in ["http://link.example", "https://user:password@link.example", "https://link.example/?token=secret", "https://link.example/#secret", "file:///tmp/foo"] {
            XCTAssertThrowsError(try ControllerCodec.authority(bad), bad)
        }
        XCTAssertEqual(try ControllerCodec.authority("https://link.example:9443/").host, "link.example")
    }
    func testAuthorityBasePathPreservesRealLinkRouteAndRejectsTraversal() async throws {
        let base = try ControllerCodec.authority("https://worksmacstudio.tailfc4700.ts.net/leanzero-link/")
        let transport = ScriptedTransport([.response(200, Data(#"{"revision":1,"hosts":[]}"#.utf8))])
        _ = try await ControllerAPI(authority: base, transport: transport).hosts(token: "test")
        let requests = await transport.requests
        XCTAssertEqual(requests.first?.url?.absoluteString, "https://worksmacstudio.tailfc4700.ts.net/leanzero-link/v1/controller/hosts")
        for suffix in ["//other", "/%2e%2e/other", "/foo%2fbar", "/foo%5cbar", "/%252e%252e"] {
            XCTAssertThrowsError(try ControllerCodec.authority("https://link.example" + suffix), suffix)
        }
    }
    func testFractionalRFC3339AndOpaqueChallenge() throws {
        let data = Data(#"{"challenge_id":"opaque-account.abcdef","expires_at":"2026-10-26T11:00:00.123Z"}"#.utf8)
        let challenge = try ControllerCodec.decoder().decode(EnrollmentChallenge.self, from: data)
        XCTAssertEqual(challenge.challengeId, "opaque-account.abcdef")
        XCTAssertEqual(challenge.expiresAt.timeIntervalSince1970, 1793012400.123, accuracy: 0.001)
    }
    func testUnverifiedHostsCannotExposeControllerAccess() throws {
        let data = Data("""
        {"owner_node_id":"\(owner.uuidString)","name":"Mac","device_role":"host","protocol_version":1,
         "availability":"update_required","last_seen_at":null,"endpoints":[],"capabilities":[],
         "registration_state":"awaiting_mesh_binding","tls_spki_sha256":null}
        """.utf8)
        let host = try ControllerCodec.decoder().decode(HostDescriptor.self, from: data)
        XCTAssertFalse(host.controllerAvailable)
        XCTAssertEqual(host.statusLabel, "Mesh setup needed")
    }
    func testAPIUsesHeaderBearerAndExactControllerNamespace() async throws {
        let transport = ScriptedTransport([.response(200, Data(#"{"revision":1,"hosts":[]}"#.utf8))])
        let api = try ControllerAPI(authority: URL(string: "https://link.example")!, transport: transport)
        let result = try await api.hosts(token: "sensitive-bearer")
        XCTAssertTrue(result.hosts.isEmpty)
        let requests = await transport.requests
        XCTAssertEqual(requests.count, 1)
        XCTAssertEqual(requests[0].url?.absoluteString, "https://link.example/v1/controller/hosts")
        XCTAssertEqual(requests[0].value(forHTTPHeaderField: "Authorization"), "Bearer sensitive-bearer")
        XCTAssertNil(requests[0].url?.query)
        XCTAssertNil(requests[0].value(forHTTPHeaderField: "Origin"))
    }
    func testRemoteFailureIsNotSuccessfulEmptyCatalog() async throws {
        let transport = ScriptedTransport([.response(503, Data(#"{"error":{"code":"registry_unavailable","message":"Authority unavailable","retryable":true}}"#.utf8))])
        do {
            _ = try await ControllerAPI(authority: URL(string: "https://link.example")!, transport: transport).hosts(token: "x")
            XCTFail("An unavailable registry cannot become an empty host list")
        } catch { XCTAssertEqual(error as? ControllerError, .remote(503, "registry_unavailable", "Authority unavailable")) }
    }
    @MainActor func testDraftAndPendingCommandSurviveReopening() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = try LocalControllerStore(directory: directory)
        let ref = entry().work
        try store.saveDraft("Unsent local draft", work: ref)
        let command = try WorkCommand.make(entry: entry(), action: .send, text: "Do work")
        try store.record(.init(command: command, createdAt: Date()))
        let recovered = try LocalControllerStore(directory: directory)
        XCTAssertEqual(recovered.data.drafts[ref.id], "Unsent local draft")
        XCTAssertEqual(recovered.data.pendingCommands.count, 1)
        XCTAssertEqual(recovered.data.pendingCommands[0].id, command.commandId)
        XCTAssertNil(recovered.data.pendingCommands[0].receipt)
        XCTAssertThrowsError(try Data("broken json".utf8).write(to: directory.appendingPathComponent("missing/child")))
        try Data("broken json".utf8).write(to: directory.appendingPathComponent("controller-state.json"))
        XCTAssertThrowsError(try LocalControllerStore(directory: directory))
    }
    @MainActor func testLostRefreshACKReusesOriginalRequestAndTokenAfterRelaunch() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let storage = MemoryCredentials()
        let original = CredentialEnvelope(authority: URL(string: "https://link.example")!, credentials: credentials(expiry: .distantPast))
        try storage.write(original, account: "device")
        let replacement = credentials(expiry: .distantFuture, token: "rotated-access", refresh: "rotated-refresh")
        let transport = ScriptedTransport([.disconnected, .response(200, try ControllerCodec.encoder().encode(replacement))])
        let first = ControllerStore(credentials: storage, transport: transport, directory: directory)
        do { _ = try await first.validCredential(); XCTFail("Lost response must remain unresolved") } catch {}
        let persisted = try storage.read(CredentialEnvelope.self, account: "device")!
        XCTAssertNotNil(persisted.refreshRequestId)
        XCTAssertEqual(persisted.credentials.refreshToken, original.credentials.refreshToken)
        let relaunched = ControllerStore(credentials: storage, transport: transport, directory: directory)
        let result = try await relaunched.validCredential()
        XCTAssertEqual(result.credentials.refreshToken, "rotated-refresh")
        XCTAssertNil(result.refreshRequestId)
        let requests = await transport.requests
        XCTAssertEqual(requests.count, 2)
        XCTAssertEqual(requests[0].httpBody, requests[1].httpBody)
        XCTAssertFalse(String(data: requests[0].httpBody!, encoding: .utf8)!.contains("node_secret"))
    }
    func testUnavailableOwnerCannotReportSuccess() async throws {
        let hostData = Data("""
        {"owner_node_id":"\(owner.uuidString)","name":"Mac","device_role":"host","protocol_version":1,"availability":"online","last_seen_at":null,"endpoints":[],"capabilities":[]}
        """.utf8)
        let host = try ControllerCodec.decoder().decode(HostDescriptor.self, from: hostData)
        do { _ = try await UnavailableOwnerControllerService().catalog(host: host, credential: "test"); XCTFail("Missing transport returned success") }
        catch { XCTAssertTrue(error.localizedDescription.contains("not available")) }
    }
}

struct FixedOwnerConnection: OwnerConnectionProvider {
    let endpoint: MeshEndpoint
    let transport: any HTTPTransport
    func connection(for host: HostDescriptor) async throws -> OwnerConnection { .init(endpoint: endpoint, transport: transport) }
}

final class OwnerWireTests: XCTestCase {
    private let owner = UUID(uuidString: "11111111-1111-4111-8111-111111111111")!
    private let work = UUID(uuidString: "22222222-2222-4222-8222-222222222222")!
    private let generation = UUID(uuidString: "33333333-3333-4333-8333-333333333333")!
    private let endpoint = MeshEndpoint(url: URL(string: "https://100.64.0.10:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))
    private func host() -> HostDescriptor {
        .init(ownerNodeId: owner, name: "Owner Mac", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
              endpoints: [endpoint], capabilities: ["catalog.read", "history.read"], registrationState: nil, tlsSpkiSha256: nil)
    }
    private func page(owner: UUID? = nil, revision: UInt64 = 7, token: String? = nil) throws -> Data {
        try ControllerCodec.encoder().encode(CatalogPage(entries: [], watermark: .init(ownerNodeId: owner ?? self.owner, storeGeneration: generation, revision: revision), nextPageToken: token, resumeCursor: "signed-test-cursor"))
    }
    func testCatalogPaginationKeepsExactOwnerWatermarkAndOpaqueToken() async throws {
        let token = "opaque/+?=&"
        let transport = ScriptedTransport([.response(200, try page(token: token)), .response(200, try page())])
        let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: transport))
        let result = try await service.catalog(host: host(), credential: "device-token")
        XCTAssertEqual(result.watermark.ownerNodeId, owner)
        let requests = await transport.requests
        XCTAssertEqual(requests.count, 2)
        let query = URLComponents(url: requests[1].url!, resolvingAgainstBaseURL: false)!.queryItems
        XCTAssertEqual(query, [URLQueryItem(name: "page_token", value: token)])
        XCTAssertEqual(requests[1].value(forHTTPHeaderField: "Authorization"), "Bearer device-token")
        XCTAssertFalse(requests[1].url!.absoluteString.contains("device-token"))
    }
    func testChangedWatermarkAndWrongOwnerAreRejected() async throws {
        for replies in [[ScriptedTransport.Reply.response(200, try page(token: "next")), .response(200, try page(revision: 8))], [.response(200, try page(owner: UUID()))]] {
            let transport = ScriptedTransport(replies)
            let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: transport))
            do { _ = try await service.catalog(host: host(), credential: "x"); XCTFail("Mixed owner/snapshot accepted") }
            catch { XCTAssertEqual(error as? ControllerError, .malformedResponse) }
        }
    }
    func testRepeatedPageTokenStopsOnLackOfProgress() async throws {
        let transport = ScriptedTransport([.response(200, try page(token: "same")), .response(200, try page(token: "same"))])
        let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: transport))
        do { _ = try await service.catalog(host: host(), credential: "x"); XCTFail("Repeated page accepted") }
        catch { XCTAssertTrue(error.localizedDescription.contains("repeated")) }
        let requests = await transport.requests
        XCTAssertEqual(requests.count, 2)
    }
    func testUnregisteredEndpointGetsNoBearerRequest() async throws {
        let transport = ScriptedTransport([])
        let other = MeshEndpoint(url: URL(string: "https://another.example")!, tlsSpkiSha256: endpoint.tlsSpkiSha256)
        let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: other, transport: transport))
        do { _ = try await service.catalog(host: host(), credential: "secret"); XCTFail("Unregistered endpoint accepted") } catch {}
        let requests = await transport.requests
        XCTAssertTrue(requests.isEmpty)
    }
    func testVisibleTranscriptPreservesMessageIdentityAndOmitsHiddenReasoning() throws {
        let visible = HistoryItem(messageId: "stable-id", revision: 9, message: .object([
            "role": .string("assistant"), "created": .number(1_700_000_000),
            "content": .array([.object(["type": .string("text"), "text": .string("Visible answer")]),
                               .object(["type": .string("thinking"), "thinking": .string("Private reasoning")])]),
            "metadata": .object(["userVisible": .bool(true)])]))
        let hidden = HistoryItem(messageId: "hidden", revision: 10, message: .object([
            "role": .string("assistant"), "content": .array([.object(["type": .string("text"), "text": .string("Hidden agent data")])]),
            "metadata": .object(["userVisible": .bool(false)])]))
        let result = try TranscriptRenderer.items([visible, hidden])
        XCTAssertEqual(result.count, 1)
        XCTAssertEqual(result[0].id, "stable-id")
        XCTAssertEqual(result[0].revision, 9)
        XCTAssertEqual(result[0].text, "Visible answer")
    }
    func testUnknownResourceCapacityStaysUnknownAndLoadIsNotPercent() throws {
        let data = Data(#"{"sampled_at":"2026-10-26T11:00:00Z","load":{"own":{"cpu_load_per_core":1.25,"mem_free_pct":null,"mem_total_gb":null}}}"#.utf8)
        let sample = try ControllerCodec.decoder().decode(ResourceSnapshot.self, from: data).display
        XCTAssertEqual(sample.cpuLoadPerCore, 1.25)
        XCTAssertNil(sample.memoryFreePercent)
        XCTAssertNil(sample.memoryTotalGB)
    }
    func testPendingInputPreservesRunAndOptions() throws {
        let inputId = UUID(), runId = UUID()
        let data = Data("""
        {"input_id":"\(inputId)","run_id":"\(runId)","prompt":"Allow this action?","options":[{"option_id":"allow","label":"Allow once","accepts_value":false}],"payload":{"kind":"permission","request_id":"original-request","tool_name":"developer__shell","arguments":{"command":"pwd"}}}
        """.utf8)
        let input = try ControllerCodec.decoder().decode(PendingInput.self, from: data)
        XCTAssertEqual(input.id, inputId)
        XCTAssertEqual(input.runId, runId)
        XCTAssertEqual(input.options[0].id, "allow")
        XCTAssertFalse(input.options[0].acceptsValue)
    }
}

@MainActor final class CredentialRecoveryTests: XCTestCase {
    private func eventually(_ check: @escaping @MainActor () -> Bool, file: StaticString = #filePath, line: UInt = #line) async {
        let done = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in MainActor.assumeIsolated { check() } }, object: nil)
        await fulfillment(of: [done], timeout: 5)
    }

    private func fixture(deniedRefresh: Bool = false, denyFreshOwner: Bool = false, droppedScopes: Bool = false, expiry: Date = Date().addingTimeInterval(60)) throws -> (ControllerStore, RecoveryOwner, RecoveryAuthority, URL) {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let storage = MemoryCredentials()
        let device = DeviceRecord(deviceId: UUID(), name: "Test iPad", platform: "ios", role: "controller", revision: 9,
                                  scopes: ["catalog.read", "history.read"], createdAt: Date(), revokedAt: nil)
        let original = DeviceCredentials(accessToken: "revision-9", tokenType: "Bearer", expiresAt: expiry, refreshToken: "refresh-9", device: device)
        try storage.write(CredentialEnvelope(authority: URL(string: "https://link.example")!, credentials: original), account: "device")
        let hosts = (0..<2).map { index in HostDescriptor(ownerNodeId: UUID(), name: "Mac \(index)", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
            endpoints: [.init(url: URL(string: "https://100.64.0.1:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))], capabilities: [], registrationState: nil, tlsSpkiSha256: nil) }
        let authority = RecoveryAuthority(original: original, hosts: hosts, denied: deniedRefresh, droppedScopes: droppedScopes)
        let owner = RecoveryOwner(hosts: hosts, denyFresh: denyFreshOwner)
        return (ControllerStore(credentials: storage, owner: owner, transport: authority, directory: directory), owner, authority, directory)
    }

    func testSharedExpiryDenialsRenewOnceAndRestoreAuthorizedSelection() async throws {
        let expiry = Date().addingTimeInterval(3)
        let (store, owner, authority, directory) = try fixture(expiry: expiry)
        defer { try? FileManager.default.removeItem(at: directory) }
        await store.synchronize()
        let work = try XCTUnwrap(store.entries.first?.work)
        await store.select(work)
        await owner.waitForStreams()
        await eventually { Date() >= expiry }
        await owner.denyOriginalStreams()
        await eventually { store.envelope?.credentials.accessToken == "revision-10" && store.selectedWork?.entry.work == work && !store.syncRunning }
        XCTAssertEqual(store.entries.count, 2)
        XCTAssertEqual(store.selectedWork?.transcript.first?.text, "authorized revision-10")
        XCTAssertEqual(store.selectedRef, work)
        let refreshes = await authority.refreshes
        XCTAssertEqual(refreshes, 1)
        let disk = try LocalControllerStore(directory: directory)
        XCTAssertEqual(disk.data.selected?.transcript.first?.text, "authorized revision-10")
        await store.sceneBackground()
    }

    func testRevokedRefreshPurgesVisibleAndPersistedData() async throws {
        let (store, owner, authority, directory) = try fixture(deniedRefresh: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        await store.synchronize()
        let work = try XCTUnwrap(store.entries.first?.work)
        await store.select(work)
        await owner.waitForStreams()
        await owner.denyOriginalStreams()
        await eventually { store.errorMessage == "Device revoked" }
        XCTAssertTrue(store.entries.isEmpty)
        XCTAssertNil(store.selectedWork)
        XCTAssertEqual(store.selectedRef, work)
        let disk = try LocalControllerStore(directory: directory)
        XCTAssertTrue(disk.data.catalogs.isEmpty)
        XCTAssertNil(disk.data.selected)
        let refreshes = await authority.refreshes
        XCTAssertEqual(refreshes, 1)
        await store.sceneBackground()
    }

    func testFreshOwnerDenialCannotStartAnotherRotation() async throws {
        let (store, owner, authority, directory) = try fixture(denyFreshOwner: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        await store.synchronize()
        await store.select(try XCTUnwrap(store.entries.first?.work))
        await owner.waitForStreams()
        await owner.denyOriginalStreams()
        await eventually { store.envelope?.credentials.accessToken == "revision-10" && !store.syncRunning && store.entries.isEmpty }
        await store.synchronize()
        let refreshes = await authority.refreshes
        XCTAssertEqual(refreshes, 1)
        XCTAssertNil(store.selectedWork)
        await store.sceneBackground()
    }

    func testDroppedScopesCannotRestoreCachedViews() async throws {
        let (store, owner, authority, directory) = try fixture(droppedScopes: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        await store.synchronize()
        await store.select(try XCTUnwrap(store.entries.first?.work))
        await owner.waitForStreams()
        await owner.denyOriginalStreams()
        await eventually { store.errorMessage == "Scope removed" }
        XCTAssertTrue(store.entries.isEmpty)
        XCTAssertNil(store.selectedWork)
        XCTAssertTrue(try LocalControllerStore(directory: directory).data.catalogs.isEmpty)
        let refreshes = await authority.refreshes
        XCTAssertEqual(refreshes, 1)
        await store.sceneBackground()
    }

    func testOrdinaryRotationReconnectsStreamsAndIgnoresLateOldDenial() async throws {
        let (store, owner, authority, directory) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        await owner.holdOriginalOpening()
        await store.synchronize()
        let work = try XCTUnwrap(store.entries.first?.work)
        await store.select(work)
        await owner.waitForStreams()
        _ = try await store.validCredential(forceRefresh: true)
        await eventually { store.selectedWork?.transcript.first?.text == "authorized revision-10" && !store.syncRunning }
        await owner.releaseOldOpening()
        await owner.denyOriginalStreams()
        await Task.yield()
        XCTAssertEqual(store.entries.count, 2)
        XCTAssertEqual(store.selectedWork?.entry.work, work)
        let refreshes = await authority.refreshes
        XCTAssertEqual(refreshes, 1)
        await store.sceneBackground()
    }

    func testExpiryDuringResourceReadReconnectsExistingStreams() async throws {
        let expiry = Date().addingTimeInterval(3)
        let (store, owner, authority, directory) = try fixture(expiry: expiry)
        defer { try? FileManager.default.removeItem(at: directory) }
        await store.synchronize()
        let work = try XCTUnwrap(store.entries.first?.work)
        let host = try XCTUnwrap(store.hosts.first)
        await store.select(work)
        await owner.waitForStreams()
        await eventually { Date() >= expiry }
        await store.loadResources(host)
        await eventually { store.selectedWork?.transcript.first?.text == "authorized revision-10" && !store.syncRunning }
        XCTAssertEqual(store.resources[host.id]?.cpuLoadPerCore, 0.5)
        XCTAssertEqual(store.selectedRef, work)
        let refreshes = await authority.refreshes
        XCTAssertEqual(refreshes, 1)
        await store.sceneBackground()
    }

    func testRevokedOrdinaryRefreshClearsPreviouslyAuthorizedViews() async throws {
        let (store, owner, _, directory) = try fixture(deniedRefresh: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        await store.synchronize()
        await store.select(try XCTUnwrap(store.entries.first?.work))
        await owner.waitForStreams()
        do { _ = try await store.validCredential(forceRefresh: true); XCTFail("Revoked device refreshed") } catch {}
        XCTAssertTrue(store.entries.isEmpty)
        XCTAssertNil(store.selectedWork)
        XCTAssertTrue(try LocalControllerStore(directory: directory).data.catalogs.isEmpty)
        await store.sceneBackground()
    }

    func testAuthorityDenialWithoutRotationPurgesSavedViewsAndStopsAfterOneRecovery() async throws {
        let (store, owner, authority, directory) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        await store.synchronize()
        let work = try XCTUnwrap(store.entries.first?.work)
        await store.select(work)
        await owner.waitForStreams()
        await authority.rejectHostReads()
        await store.synchronize()
        await eventually { store.envelope?.credentials.accessToken == "revision-10" && !store.syncRunning && store.entries.isEmpty }
        XCTAssertNil(store.selectedWork)
        XCTAssertEqual(store.selectedRef, work)
        XCTAssertTrue(try LocalControllerStore(directory: directory).data.catalogs.isEmpty)
        let refreshes = await authority.refreshes
        XCTAssertEqual(refreshes, 1)
        await store.sceneBackground()
    }

    func testRemovedHostCannotKeepSelectedCachedTranscript() async throws {
        let (store, _, authority, directory) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        await store.synchronize()
        await store.select(try XCTUnwrap(store.entries.first?.work))
        await authority.removeHosts()
        await store.synchronize()
        XCTAssertTrue(store.entries.isEmpty)
        XCTAssertNil(store.selectedWork)
        XCTAssertNil(try LocalControllerStore(directory: directory).data.selected)
        await store.sceneBackground()
    }

    func testUnavailableRefreshAfterDenialNeverRestoresCachedContent() async throws {
        let (store, owner, authority, directory) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        await store.synchronize()
        await store.select(try XCTUnwrap(store.entries.first?.work))
        await owner.waitForStreams()
        await authority.failRefresh()
        await owner.denyOriginalStreams()
        await eventually { store.errorMessage == "Authority unavailable" }
        XCTAssertTrue(store.entries.isEmpty)
        XCTAssertNil(store.selectedWork)
        XCTAssertTrue(try LocalControllerStore(directory: directory).data.catalogs.isEmpty)
        let refreshes = await authority.refreshes
        XCTAssertEqual(refreshes, 1)
        await store.sceneBackground()
    }

    func testBackgroundRotationDoesNotReconnect() async throws {
        let (store, owner, authority, directory) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        await store.synchronize()
        await owner.waitForCatalogs()
        await store.sceneBackground()
        _ = try await store.validCredential(forceRefresh: true)
        await Task.yield()
        let reads = await authority.hostReads
        XCTAssertEqual(reads, 1)
        XCTAssertTrue(store.entries.isEmpty)
    }
}

private actor RecoveryAuthority: HTTPTransport {
    let original: DeviceCredentials
    let hosts: [HostDescriptor]
    let denied: Bool
    let droppedScopes: Bool
    var refreshes = 0
    var hostReads = 0
    var rejectHosts = false
    var hideHosts = false
    var unavailable = false
    func rejectHostReads() { rejectHosts = true }
    func removeHosts() { hideHosts = true }
    func failRefresh() { unavailable = true }
    init(original: DeviceCredentials, hosts: [HostDescriptor], denied: Bool, droppedScopes: Bool) {
        self.original = original; self.hosts = hosts; self.denied = denied; self.droppedScopes = droppedScopes
    }
    func data(for request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        let status: Int
        let data: Data
        if request.url!.path.hasSuffix("auth/refresh") {
            refreshes += 1
            if unavailable {
                status = 503; data = Data(#"{"error":{"code":"unavailable","message":"Authority unavailable"}}"#.utf8)
            } else if denied {
                status = 403; data = Data(#"{"error":{"code":"revoked","message":"Device revoked"}}"#.utf8)
            } else {
                status = 200
                let d = original.device
                let device = DeviceRecord(deviceId: d.id, name: d.name, platform: d.platform, role: d.role, revision: 10,
                                          scopes: droppedScopes ? [] : d.scopes, createdAt: d.createdAt, revokedAt: nil)
                data = try ControllerCodec.encoder().encode(DeviceCredentials(accessToken: "revision-10", tokenType: "Bearer", expiresAt: Date().addingTimeInterval(900), refreshToken: "refresh-10", device: device))
            }
        } else {
            hostReads += 1
            if rejectHosts || (droppedScopes && request.value(forHTTPHeaderField: "Authorization") == "Bearer revision-10") {
                status = 403; data = Data(#"{"error":{"code":"scope_denied","message":"Scope removed"}}"#.utf8)
            } else {
                status = 200; data = try ControllerCodec.encoder().encode(HostPage(revision: 1, hosts: hideHosts ? [] : hosts))
            }
        }
        return (data, HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: nil)!)
    }
}

private actor RecoveryOwner: OwnerControllerService {
    let hosts: [HostDescriptor]
    let denyFresh: Bool
    let generation = UUID()
    var entries: [UUID: CatalogEntry] = [:]
    var catalogs: [String: [AsyncThrowingStream<CatalogChange, Error>.Continuation]] = [:]
    var works: [String: [AsyncThrowingStream<WorkFrame, Error>.Continuation]] = [:]
    var holdOpening = false
    var opening: CheckedContinuation<Void, Error>?
    func holdOriginalOpening() { holdOpening = true }
    func releaseOldOpening() {
        opening?.resume(throwing: ControllerError.remote(403, "forbidden", "Obsolete stream denial"))
        opening = nil
    }
    init(hosts: [HostDescriptor], denyFresh: Bool) {
        self.hosts = hosts; self.denyFresh = denyFresh
        for host in hosts {
            entries[host.id] = .init(work: .init(ownerNodeId: host.id, kind: .chat, workId: UUID()), storeGeneration: generation, revision: 80,
                title: "Authorized session", workingDir: nil, state: .running, currentRunId: UUID(), capabilities: [], parent: nil, related: [], updatedAt: Date(), lastEvent: nil, archived: false)
        }
    }
    func waitForCatalogs() async {
        let deadline = Date().addingTimeInterval(5)
        while (catalogs["revision-9"]?.count ?? 0) < hosts.count && Date() < deadline { await Task.yield() }
        XCTAssertEqual(catalogs["revision-9"]?.count, hosts.count)
    }
    func waitForStreams() async {
        await waitForCatalogs()
        let deadline = Date().addingTimeInterval(5)
        while works["revision-9"]?.isEmpty != false && Date() < deadline { await Task.yield() }
        XCTAssertFalse(works["revision-9"]?.isEmpty ?? true)
    }
    func denyOriginalStreams() {
        let denial = ControllerError.remote(403, "forbidden", "This device is no longer allowed to view this Mac.")
        catalogs["revision-9"]?.forEach { $0.finish(throwing: denial) }
        works["revision-9"]?.forEach { $0.finish(throwing: denial) }
    }
    func catalog(host: HostDescriptor, credential: String) async throws -> OwnerCatalogSnapshot {
        if denyFresh && credential == "revision-10" { throw ControllerError.remote(403, "forbidden", "Still denied") }
        return .init(entries: [entries[host.id]!], watermark: .init(ownerNodeId: host.id, storeGeneration: generation, revision: 80), resumeCursor: "catalog-80")
    }
    func changes(cursor: String, host: HostDescriptor, credential: String) async throws -> CatalogChanges { throw ControllerError.remote(410, "snapshot_required", "Reload") }
    func selected(work: CatalogEntry, host: HostDescriptor, credential: String) async throws -> SelectedWork {
        .init(entry: work, transcript: [.init(id: "message", revision: 80, role: "assistant", text: "authorized \(credential)", timestamp: nil)], pendingInputs: [], nextPageToken: nil, historyRevision: 80, workCursor: "work-80", streamRevision: 80)
    }
    func catalogEvents(cursor: String, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<CatalogChange, Error> {
        let (stream, continuation) = AsyncThrowingStream<CatalogChange, Error>.makeStream()
        catalogs[credential, default: []].append(continuation)
        if holdOpening && credential == "revision-9" {
            holdOpening = false
            try await withCheckedThrowingContinuation { opening = $0 }
        }
        return stream
    }
    func workEvents(work: SelectedWork, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<WorkFrame, Error> {
        let (stream, continuation) = AsyncThrowingStream<WorkFrame, Error>.makeStream()
        works[credential, default: []].append(continuation)
        return stream
    }
    func history(work: CatalogEntry, pageToken: String, host: HostDescriptor, credential: String) async throws -> HistoryPage { throw ControllerError.unsupported("unused") }
    func resources(host: HostDescriptor, credential: String) async throws -> ResourceSample { .init(sampledAt: Date(), cpuLoadPerCore: 0.5, memoryFreePercent: 50, memoryTotalGB: 64) }
    func submit(_ command: WorkCommand, host: HostDescriptor, credential: String) async throws -> CommandReceipt { throw ControllerError.unsupported("unused") }
    func receipt(commandId: UUID, host: HostDescriptor, credential: String) async throws -> CommandReceipt { throw ControllerError.unsupported("unused") }
}
