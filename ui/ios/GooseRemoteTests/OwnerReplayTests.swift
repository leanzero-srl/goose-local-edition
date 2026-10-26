import XCTest
@testable import GooseRemote

final class OwnerReplayTests: XCTestCase {
    let owner = UUID(), generation = UUID(), workId = UUID(), run = UUID()
    func entry(_ revision: UInt64 = 42) -> CatalogEntry {
        .init(work: .init(ownerNodeId: owner, kind: .chat, workId: workId), storeGeneration: generation, revision: revision,
              title: "Selected session", workingDir: nil, state: .running, currentRunId: run, capabilities: [.steer, .stop],
              parent: nil, related: [], updatedAt: Date(), lastEvent: nil, archived: false)
    }
    func message(_ text: String) -> JSONValue { .object(["role": .string("assistant"), "content": .array([.object(["type": .string("text"), "text": .string(text)])])]) }
    func selected() -> SelectedWork { .init(entry: entry(), transcript: [], pendingInputs: [], nextPageToken: nil, historyRevision: 80, workCursor: "signed-80", streamRevision: 80) }
    func frame(_ revision: UInt64, type: String, payload: JSONValue) -> WorkFrame {
        .init(resumeCursor: "signed-\(revision)", work: entry().work, storeGeneration: generation, streamEpoch: generation,
              sequence: revision, workRevision: revision, runId: run, type: type, payload: payload)
    }
    func partial(_ revision: UInt64, offset: UInt64, text: String) -> WorkFrame {
        frame(revision, type: "partial_replacement", payload: .object(["message_id": .string("m"), "message_revision": .number(Double(revision)),
             "offset_bytes": .number(Double(offset)), "content": message(text)]))
    }
    func testNoncontiguousGlobalRevisionsReplayPartialExactlyOnceAndCommitReplaces() throws {
        var state = try OwnerReplay.work(partial(84, offset: 0, text: "Hello 🌍"), into: selected())
        XCTAssertEqual(state.transcript[0].text, "Hello 🌍")
        state = try OwnerReplay.work(partial(91, offset: 10, text: " again"), into: state)
        XCTAssertEqual(state.transcript[0].text, "Hello 🌍 again")
        state = try OwnerReplay.work(partial(91, offset: 10, text: " again"), into: state)
        XCTAssertEqual(state.transcript.count, 1)
        XCTAssertEqual(state.transcript[0].text, "Hello 🌍 again")
        let item = HistoryItem(messageId: "m", revision: 98, message: message("Final answer"))
        let encoded = try ControllerCodec.decoder().decode(JSONValue.self, from: ControllerCodec.encoder().encode(item))
        state = try OwnerReplay.work(frame(98, type: "message_upsert", payload: .object(["item": encoded, "previous_message_id": .null])), into: state)
        XCTAssertEqual(state.transcript.map(\.text), ["Final answer"])
        XCTAssertTrue(state.partialMessages.isEmpty)
        XCTAssertEqual(state.workCursor, "signed-98")
        XCTAssertEqual(state.entry.revision, 98)
    }
    func testMissingPartialOffsetWrongGenerationAndTombstoneAreLoud() throws {
        XCTAssertThrowsError(try OwnerReplay.work(partial(84, offset: 3, text: "missing"), into: selected()))
        let wrong = WorkFrame(resumeCursor: "x", work: entry().work, storeGeneration: UUID(), streamEpoch: generation, sequence: 84, workRevision: 84, runId: run, type: "partial_discarded", payload: .object(["message_ids": .array([])]))
        XCTAssertThrowsError(try OwnerReplay.work(wrong, into: selected()))
        XCTAssertThrowsError(try OwnerReplay.work(frame(84, type: "tombstone", payload: .null), into: selected()))
    }
    func testDiscardRemovesOnlyUncommittedPartial() throws {
        var state = try OwnerReplay.work(partial(84, offset: 0, text: "Unfinished"), into: selected())
        state.transcript.append(.init(id: "committed", revision: 40, role: "assistant", text: "Saved answer", timestamp: nil))
        state = try OwnerReplay.work(frame(95, type: "partial_discarded", payload: .object(["message_ids": .array([.string("m"), .string("committed")])])), into: state)
        XCTAssertEqual(state.transcript.map(\.id), ["committed"])
    }
    func testLateHistoryCannotResurrectDeletedOrRenamedMessagesAndOrdersBeforeLiveAnswer() throws {
        var state = selected()
        state.transcript = [.init(id: "live", revision: 95, role: "assistant", text: "Newest", timestamp: Date(timeIntervalSince1970: 300))]
        state = try OwnerReplay.work(frame(120, type: "message_deleted", payload: .object(["message_id": .string("deleted-before-load")])), into: state)
        func dated(_ text: String, _ date: Double) -> JSONValue {
            .object(["role": .string("assistant"), "created": .number(date), "content": .array([.object(["type": .string("text"), "text": .string(text)])])])
        }
        let renamed = HistoryItem(messageId: "renamed", revision: 130, message: dated("New identity", 200))
        let encoded = try ControllerCodec.decoder().decode(JSONValue.self, from: ControllerCodec.encoder().encode(renamed))
        state = try OwnerReplay.work(frame(130, type: "message_upsert", payload: .object(["item": encoded, "previous_message_id": .string("old-id")])), into: state)
        let page = HistoryPage(work: entry().work, storeGeneration: generation, revision: 80,
            items: [.init(messageId: "deleted-before-load", revision: 70, message: dated("Deleted", 100)),
                    .init(messageId: "old-id", revision: 71, message: dated("Old identity", 200)),
                    .init(messageId: "history", revision: 72, message: dated("Older history", 100))], nextPageToken: nil, workCursor: "history-80")
        state = try OwnerReplay.history(page, into: state)
        XCTAssertEqual(state.transcript.map(\.id), ["history", "renamed", "live"])
        XCTAssertEqual(state.workCursor, "signed-130")
    }
    @MainActor func testPendingInputStreamReloadDoesNotCancelItsOwnRequest() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let storage = MemoryCredentials()
        let device = DeviceRecord(deviceId: UUID(), name: "Test", platform: "ios", role: "controller", revision: 1, scopes: [], createdAt: Date(), revokedAt: nil)
        try storage.write(CredentialEnvelope(authority: URL(string: "https://link.example")!, credentials: .init(accessToken: "test", tokenType: "Bearer", expiresAt: .distantFuture, refreshToken: "refresh", device: device)), account: "device")
        let host = HostDescriptor(ownerNodeId: owner, name: "Mac", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
            endpoints: [.init(url: URL(string: "https://100.64.0.1:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))], capabilities: [], registrationState: nil, tlsSpkiSha256: nil)
        let transport = ScriptedTransport([.response(200, try ControllerCodec.encoder().encode(HostPage(revision: 1, hosts: [host])))])
        let reloaded = expectation(description: "Pending input reloaded without cancellation")
        let ownerService = PendingInputOwner(entry: entry(), complete: reloaded)
        let store = ControllerStore(credentials: storage, owner: ownerService, transport: transport, directory: directory)
        await store.synchronize()
        await store.select(entry().work)
        await fulfillment(of: [reloaded], timeout: 3)
        await Task.yield()
        XCTAssertNil(store.selectedError)
        XCTAssertEqual(store.selectedWork?.pendingInputs.first?.prompt, "Current owner input")
        await store.sceneBackground()
    }
    func testEqualTimestampHistoryKeepsOwnerPageOrderAheadOfLiveRows() throws {
        var state = selected()
        let date = Date(timeIntervalSince1970: 200)
        state.transcript = [.init(id: "first", revision: 70, role: "assistant", text: "First", timestamp: date),
                            .init(id: "live", revision: 90, role: "assistant", text: "Live", timestamp: date)]
        state.historyOrder = ["first"]
        let message = JSONValue.object(["role": .string("assistant"), "created": .number(200), "content": .array([.object(["type": .string("text"), "text": .string("Later page")])])])
        let page = HistoryPage(work: entry().work, storeGeneration: generation, revision: 80,
            items: [.init(messageId: "z-second", revision: 72, message: message), .init(messageId: "a-third", revision: 71, message: message)],
            nextPageToken: nil, workCursor: "history-80")
        state = try OwnerReplay.history(page, into: state)
        XCTAssertEqual(state.transcript.map(\.id), ["first", "z-second", "a-third", "live"])
    }
    @MainActor func testCacheReopenPreservesCursorAndOnlySelectedTranscript() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let local = try LocalControllerStore(directory: directory)
        let state = try OwnerReplay.work(partial(84, offset: 0, text: "Durable visible answer"), into: selected())
        try local.cacheCatalog(.init(entries: [entry()], watermark: .init(ownerNodeId: owner, storeGeneration: generation, revision: 80), resumeCursor: "catalog-80"))
        try local.cacheSelected(state)
        try local.saveSyncPending([owner])
        let reopened = try LocalControllerStore(directory: directory)
        XCTAssertEqual(reopened.data.selected?.workCursor, "signed-84")
        XCTAssertEqual(reopened.data.catalogs[owner.uuidString]?.resumeCursor, "catalog-80")
        XCTAssertEqual(reopened.data.syncPending, [owner])
        let bytes = try Data(contentsOf: directory.appendingPathComponent("controller-state.json"))
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes) as? [String: Any])
        XCTAssertFalse(String(describing: json["catalogs"]!).contains("Durable visible answer"))
        try reopened.retainOwners([])
        XCTAssertNil(reopened.data.selected)
        XCTAssertTrue(reopened.data.catalogs.isEmpty)
    }
    func testHistoryWatermarkCanExceedEntryRevisionAndSnapshotCarriesLiveAnswer() async throws {
        let endpoint = MeshEndpoint(url: URL(string: "https://100.64.0.1:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))
        let host = HostDescriptor(ownerNodeId: owner, name: "Mac", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
                                  endpoints: [endpoint], capabilities: [], registrationState: nil, tlsSpkiSha256: nil)
        let snapshot = WorkSnapshot(entry: entry(), pendingInputs: [], historyWatermark: .init(ownerNodeId: owner, storeGeneration: generation, revision: 80), workCursor: "snapshot-80",
                                    liveMessages: [.init(messageId: "live", revision: 42, message: message("Already streaming"))])
        let page = HistoryPage(work: entry().work, storeGeneration: generation, revision: 93, items: [], nextPageToken: nil, workCursor: "history-93")
        let transport = ScriptedTransport([.response(200, try ControllerCodec.encoder().encode(snapshot)), .response(200, try ControllerCodec.encoder().encode(page))])
        let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: transport))
        let result = try await service.selected(work: entry(), host: host, credential: "test")
        XCTAssertEqual(result.historyRevision, 93)
        XCTAssertEqual(result.streamRevision, 80)
        XCTAssertEqual(result.workCursor, "snapshot-80")
        XCTAssertEqual(result.transcript.map(\.text), ["Already streaming"])
    }
    func testMeshEnrollmentUsesControllerRoutesOpaqueKeyAndHeaderCredential() async throws {
        let id = UUID(), device = UUID()
        let join = MeshJoinCredential(enrollmentId: id, authKey: "single-use-test-key", loginServer: URL(string: "https://mesh.example")!, expirySeconds: 600)
        let record = DeviceRecord(deviceId: device, name: "Phone", platform: "ios", role: "controller", revision: 1, scopes: [], createdAt: Date(), revokedAt: nil)
        let transport = ScriptedTransport([.response(200, try ControllerCodec.encoder().encode(join)), .response(200, try ControllerCodec.encoder().encode(record))])
        let api = try ControllerAPI(authority: URL(string: "https://link.example")!, transport: transport)
        _ = try await api.meshJoin(enrollmentId: id, token: "controller-only")
        _ = try await api.confirmMesh(enrollmentId: id, nodeKey: "nodekey:" + String(repeating: "a", count: 64), token: "controller-only")
        let requests = await transport.requests
        XCTAssertEqual(requests.map { $0.url!.path }, ["/v1/controller/mesh/join-key", "/v1/controller/mesh/confirm"])
        for request in requests {
            XCTAssertNil(request.url?.query)
            XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer controller-only")
            let body = String(decoding: request.httpBody!, as: UTF8.self)
            XCTAssertFalse(body.contains("node_secret")); XCTAssertFalse(body.contains("role"))
        }
    }
}

private actor PendingInputOwner: OwnerControllerService {
    let entry: CatalogEntry
    let complete: XCTestExpectation
    var selections = 0
    var streams = 0
    init(entry: CatalogEntry, complete: XCTestExpectation) { self.entry = entry; self.complete = complete }
    func catalog(host: HostDescriptor, credential: String) async throws -> OwnerCatalogSnapshot {
        .init(entries: [entry], watermark: .init(ownerNodeId: entry.work.ownerNodeId, storeGeneration: entry.storeGeneration, revision: 80), resumeCursor: "catalog-80")
    }
    func selected(work: CatalogEntry, host: HostDescriptor, credential: String) async throws -> SelectedWork {
        try Task.checkCancellation()
        selections += 1
        let inputs: [PendingInput] = selections == 1 ? [] : [.init(inputId: UUID(), runId: entry.currentRunId!, prompt: "Current owner input", options: [], payload: .permission(requestId: "request", toolName: "developer__shell", arguments: .object(["command": .string("pwd")])))]
        if selections > 1 { complete.fulfill() }
        return .init(entry: entry, transcript: [], pendingInputs: inputs, nextPageToken: nil, historyRevision: 80, workCursor: "work-80", streamRevision: 80)
    }
    func catalogEvents(cursor: String, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<CatalogChange, Error> { AsyncThrowingStream { _ in } }
    func workEvents(work: SelectedWork, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<WorkFrame, Error> {
        streams += 1
        let first = streams == 1
        var next = entry; next.revision = 84
        let payload = try ControllerCodec.decoder().decode(JSONValue.self, from: ControllerCodec.encoder().encode(next))
        return AsyncThrowingStream { continuation in
            if first {
                continuation.yield(.init(resumeCursor: "work-84", work: entry.work, storeGeneration: entry.storeGeneration, streamEpoch: entry.storeGeneration, sequence: 84, workRevision: 84, runId: entry.currentRunId, type: "pending_input", payload: .object(["entry": payload])))
            }
        }
    }
    func history(work: CatalogEntry, pageToken: String, host: HostDescriptor, credential: String) async throws -> HistoryPage { throw ControllerError.unsupported("unused test method") }
    func resources(host: HostDescriptor, credential: String) async throws -> ResourceSample { throw ControllerError.unsupported("unused test method") }
    func submit(_ command: WorkCommand, host: HostDescriptor, credential: String) async throws -> CommandReceipt { throw ControllerError.unsupported("unused test method") }
    func receipt(commandId: UUID, host: HostDescriptor, credential: String) async throws -> CommandReceipt { throw ControllerError.unsupported("unused test method") }
}
