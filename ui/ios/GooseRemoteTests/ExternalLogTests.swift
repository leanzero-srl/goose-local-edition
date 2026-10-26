import XCTest
@testable import GooseRemote

@MainActor final class ExternalLogTests: XCTestCase {
    func testSelectedLogGrowthRefreshesTailAndOlderReadingPausesIt() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let storage = MemoryCredentials()
        let device = DeviceRecord(deviceId: UUID(), name: "Controller", platform: "ios", role: "controller", revision: 1, scopes: [], createdAt: Date(), revokedAt: nil)
        try storage.write(CredentialEnvelope(authority: URL(string: "https://link.example")!, credentials: .init(accessToken: "test", tokenType: "Bearer", expiresAt: .distantFuture, refreshToken: "refresh", device: device)), account: "device")
        let entry = CatalogEntry(work: .init(ownerNodeId: UUID(), kind: .shellJob, workId: UUID()), storeGeneration: UUID(), revision: 80,
            title: "Actual selected job", workingDir: nil, state: .running, currentRunId: UUID(), capabilities: [.stop], parent: nil, related: [], updatedAt: Date(), lastEvent: nil, archived: false)
        let host = HostDescriptor(ownerNodeId: entry.work.ownerNodeId, name: "Mac", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
            endpoints: [.init(url: URL(string: "https://100.64.0.1:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))], capabilities: [], registrationState: nil, tlsSpkiSha256: nil)
        let transport = ScriptedTransport([.response(200, try ControllerCodec.encoder().encode(HostPage(revision: 1, hosts: [host])))])
        let ready = expectation(description: "Selected stream attached")
        let refreshed = expectation(description: "Growth fetched the new tail")
        let owner = ExternalLogOwner(entry: entry, ready: ready, refreshed: refreshed)
        let store = ControllerStore(credentials: storage, owner: owner, transport: transport, directory: root)
        await store.synchronize()
        await store.select(entry.work)
        await fulfillment(of: [ready], timeout: 3)
        XCTAssertEqual(store.selectedWork?.transcript.map(\.id), ["log_100_200"])
        try await owner.grow(to: 84)
        await fulfillment(of: [refreshed], timeout: 3)
        let tailVisible = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in MainActor.assumeIsolated { store.selectedWork?.transcript.first?.id == "log_200_300" } }, object: nil)
        await fulfillment(of: [tailVisible], timeout: 3)
        await store.loadMoreHistory()
        XCTAssertTrue(store.selectedHistoryPaused)
        XCTAssertEqual(store.selectedWork?.transcript.map(\.id), ["log_0_100", "log_100_200", "log_200_300"])
        try await owner.grow(to: 92)
        let metadataApplied = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in MainActor.assumeIsolated { store.selectedWork?.entry.revision == 92 } }, object: nil)
        await fulfillment(of: [metadataApplied], timeout: 3)
        let reads = await owner.selections
        XCTAssertEqual(reads, 2, "Reading older activity must not refetch and replace its transcript")
        XCTAssertEqual(store.selectedWork?.transcript.first?.id, "log_0_100")
        await store.select(entry.work)
        XCTAssertFalse(store.selectedHistoryPaused)
        XCTAssertEqual(store.selectedWork?.transcript.first?.id, "log_300_400")
        await store.sceneBackground()
    }
}

private actor ExternalLogOwner: OwnerControllerService {
    var entry: CatalogEntry
    let ready: XCTestExpectation
    let refreshed: XCTestExpectation
    var selections = 0
    var continuation: AsyncThrowingStream<WorkFrame, Error>.Continuation?
    init(entry: CatalogEntry, ready: XCTestExpectation, refreshed: XCTestExpectation) { self.entry = entry; self.ready = ready; self.refreshed = refreshed }
    func message(_ text: String) -> JSONValue { .object(["role": .string("assistant"), "content": .array([.object(["type": .string("text"), "text": .string(text)])])]) }
    func grow(to revision: UInt64) throws {
        entry.revision = revision
        let value = try ControllerCodec.decoder().decode(JSONValue.self, from: ControllerCodec.encoder().encode(entry))
        continuation?.yield(.init(resumeCursor: "work-\(revision)", work: entry.work, storeGeneration: entry.storeGeneration, streamEpoch: entry.storeGeneration,
            sequence: revision, workRevision: revision, runId: entry.currentRunId, type: "log_updated", payload: .object(["entry": value])))
    }
    func catalog(host: HostDescriptor, credential: String) async throws -> OwnerCatalogSnapshot {
        .init(entries: [entry], watermark: .init(ownerNodeId: entry.work.ownerNodeId, storeGeneration: entry.storeGeneration, revision: entry.revision), resumeCursor: "catalog-80")
    }
    func selected(work: CatalogEntry, host: HostDescriptor, credential: String) async throws -> SelectedWork {
        selections += 1
        let start = selections * 100
        if selections == 2 { refreshed.fulfill() }
        return .init(entry: entry, transcript: [.init(id: "log_\(start)_\(start+100)", revision: entry.revision, role: "assistant", text: "Current log tail", timestamp: nil)],
            pendingInputs: [], nextPageToken: "older-\(start)", historyRevision: entry.revision, workCursor: "work-\(entry.revision)", streamRevision: entry.revision)
    }
    func history(work: CatalogEntry, pageToken: String, host: HostDescriptor, credential: String) async throws -> HistoryPage {
        .init(work: entry.work, storeGeneration: entry.storeGeneration, revision: 84,
            items: [.init(messageId: "log_100_200", revision: 84, message: message("Middle")), .init(messageId: "log_0_100", revision: 84, message: message("Beginning"))], nextPageToken: nil, workCursor: "work-84")
    }
    func workEvents(work: SelectedWork, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<WorkFrame, Error> {
        let (stream, next) = AsyncThrowingStream<WorkFrame, Error>.makeStream()
        continuation = next
        if selections == 1 { ready.fulfill() }
        return stream
    }
    func catalogEvents(cursor: String, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<CatalogChange, Error> { AsyncThrowingStream { _ in } }
    func resources(host: HostDescriptor, credential: String) async throws -> ResourceSample { throw ControllerError.unsupported("unused") }
    func submit(_ command: WorkCommand, host: HostDescriptor, credential: String) async throws -> CommandReceipt { throw ControllerError.unsupported("unused") }
    func receipt(commandId: UUID, host: HostDescriptor, credential: String) async throws -> CommandReceipt { throw ControllerError.unsupported("unused") }
}
