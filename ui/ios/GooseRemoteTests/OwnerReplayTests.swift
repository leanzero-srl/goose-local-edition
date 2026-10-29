import SwiftUI
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
        XCTAssertEqual(state.transcript.map(\.id), ["z-second", "a-third", "first", "live"])
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
        let page = HistoryPage(work: entry().work, storeGeneration: generation, revision: 80, items: [], nextPageToken: "older-80", workCursor: "history-80")
        let transport = ScriptedTransport([.response(200, try ControllerCodec.encoder().encode(snapshot)), .response(200, try ControllerCodec.encoder().encode(page))])
        let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: transport))
        let result = try await service.selected(work: entry(), host: host, credential: "test")
        XCTAssertEqual(result.historyRevision, 80)
        XCTAssertEqual(result.streamRevision, 80)
        XCTAssertEqual(result.workCursor, "snapshot-80")
        XCTAssertEqual(result.transcript.map(\.text), ["Already streaming"])
        let requests = await transport.requests
        XCTAssertEqual(requests.count, 2, "Opening a chat must not drain older pages")
        XCTAssertTrue(requests[1].url!.path.hasSuffix("/history/latest"))
        let query = URLComponents(url: requests[1].url!, resolvingAgainstBaseURL: false)!.queryItems!
        XCTAssertEqual(query.first { $0.name == "cursor" }?.value, "snapshot-80")
        XCTAssertEqual(result.nextPageToken, "older-80")
    }

    func testLatestHistoryRejectsDifferentSnapshotRevision() async throws {
        let endpoint = MeshEndpoint(url: URL(string: "https://100.64.0.1:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))
        let host = HostDescriptor(ownerNodeId: owner, name: "Mac", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
            endpoints: [endpoint], capabilities: [], registrationState: nil, tlsSpkiSha256: nil)
        let snapshot = WorkSnapshot(entry: entry(), pendingInputs: [], historyWatermark: .init(ownerNodeId: owner, storeGeneration: generation, revision: 80), workCursor: "snapshot-80", liveMessages: [])
        for revision: UInt64 in [79, 81] {
            let page = HistoryPage(work: entry().work, storeGeneration: generation, revision: revision, items: [], nextPageToken: nil, workCursor: "history")
            let wire = ScriptedTransport([.response(200, try ControllerCodec.encoder().encode(snapshot)), .response(200, try ControllerCodec.encoder().encode(page))])
            let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: wire))
            do { _ = try await service.selected(work: entry(), host: host, credential: "test"); XCTFail("Mixed revisions accepted") }
            catch { XCTAssertEqual(error as? ControllerError, .malformedResponse) }
        }
    }

    func testExternalSelectionKeepsTailRouteAndBackwardTokenIsOpaque() async throws {
        let external = CatalogEntry(work: .init(ownerNodeId: owner, kind: .shellJob, workId: workId), storeGeneration: generation, revision: 42,
            title: "External", workingDir: nil, state: .running, currentRunId: run, capabilities: [.stop], parent: nil, related: [], updatedAt: Date(), lastEvent: nil, archived: false)
        let endpoint = MeshEndpoint(url: URL(string: "https://100.64.0.1:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))
        let host = HostDescriptor(ownerNodeId: owner, name: "Mac", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
            endpoints: [endpoint], capabilities: [], registrationState: nil, tlsSpkiSha256: nil)
        let snapshot = WorkSnapshot(entry: external, pendingInputs: [], historyWatermark: .init(ownerNodeId: owner, storeGeneration: generation, revision: 80), workCursor: "snapshot-80", liveMessages: [])
        let page = HistoryPage(work: external.work, storeGeneration: generation, revision: 93, items: [], nextPageToken: "opaque/+?=&", workCursor: "history-93")
        let older = HistoryPage(work: external.work, storeGeneration: generation, revision: 93, items: [], nextPageToken: nil, workCursor: "history-93")
        let wire = ScriptedTransport([.response(200, try ControllerCodec.encoder().encode(snapshot)), .response(200, try ControllerCodec.encoder().encode(page)), .response(200, try ControllerCodec.encoder().encode(older))])
        let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: wire))
        let selected = try await service.selected(work: external, host: host, credential: "test")
        _ = try await service.history(work: external, pageToken: XCTUnwrap(selected.nextPageToken), host: host, credential: "test")
        let requests = await wire.requests
        XCTAssertTrue(requests[1].url!.path.hasSuffix("/history"))
        XCTAssertTrue(requests[2].url!.path.hasSuffix("/history"))
        XCTAssertEqual(URLComponents(url: requests[2].url!, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "page_token" }?.value, "opaque/+?=&")
    }

    func testOpaqueWebSocketFailureOnlyResetsAfterTypedCursorExpiry() async throws {
        let endpoint = MeshEndpoint(url: URL(string: "https://100.64.0.1:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))
        let host = HostDescriptor(ownerNodeId: owner, name: "Mac", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
            endpoints: [endpoint], capabilities: [], registrationState: nil, tlsSpkiSha256: nil)
        for expired in [false, true] {
            let response = expired ? Data(#"{"error":{"code":"snapshot_required","message":"Expired cursor"}}"#.utf8) : Data(#"{"changes":[]}"#.utf8)
            let wire = ScriptedTransport([.response(expired ? 410 : 200, response)])
            let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: ClosedStreamTransport(wire: wire)))
            let events = try await service.workEvents(work: selected(), host: host, credential: "test")
            do {
                for try await _ in events {}
                XCTFail("A failed stream must be reported")
            } catch {
                if expired { XCTAssertEqual(error as? ControllerError, .remote(410, "snapshot_required", "Expired cursor")) }
                else { XCTAssertEqual((error as? URLError)?.code, .networkConnectionLost) }
            }
            let requests = await wire.requests
            XCTAssertTrue(requests[0].url!.path.hasSuffix("/changes"))
            XCTAssertEqual(URLComponents(url: requests[0].url!, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "cursor" }?.value, "signed-80")
        }
    }

    func testEstablishedStreamAndTypedAuthorizationCloseDoNotProbeOldCursor() async throws {
        let endpoint = MeshEndpoint(url: URL(string: "https://100.64.0.1:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))
        let host = HostDescriptor(ownerNodeId: owner, name: "Mac", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
            endpoints: [endpoint], capabilities: [], registrationState: nil, tlsSpkiSha256: nil)
        let refusal = ControllerError.remote(403, "authorization_denied", "Revoked device")
        for established in [false, true] {
            let wire = ScriptedTransport([])
            let first = established ? try ControllerCodec.encoder().encode(partial(84, offset: 0, text: "Started")) : nil
            let transport = ClosedStreamTransport(wire: wire, first: first, failure: established ? nil : refusal)
            let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: transport))
            let events = try await service.workEvents(work: selected(), host: host, credential: "test")
            do { for try await _ in events {}; XCTFail("Stream failure was swallowed") }
            catch {
                if established { XCTAssertEqual((error as? URLError)?.code, .networkConnectionLost) }
                else { XCTAssertEqual(error as? ControllerError, refusal) }
            }
            let requests = await wire.requests
            XCTAssertTrue(requests.isEmpty, "Do not probe an old connection cursor or override an authoritative refusal")
        }
    }

    func testThreeBackwardPagesPreserveOwnerTiesDespiteReverseLexicalIDs() throws {
        var state = selected()
        let tied = JSONValue.object(["role": .string("assistant"), "created": .number(200), "content": .array([.object(["type": .string("text"), "text": .string("Tied row")])])])
        for ids in [["z-new", "a-new"], ["z-middle", "a-middle"], ["z-old", "a-old"]] {
            state = try OwnerReplay.history(.init(work: entry().work, storeGeneration: generation, revision: 80,
                items: ids.map { .init(messageId: $0, revision: 70, message: tied) }, nextPageToken: "older", workCursor: "page-80"), into: state)
        }
        XCTAssertEqual(state.transcript.map(\.id), ["z-old", "a-old", "z-middle", "a-middle", "z-new", "a-new"])
        XCTAssertEqual(state.historyOrder, state.transcript.map(\.id))
    }

    func testRefreshKeepsLoadedHistoryAndPartialCursorCoherentWhileReplayCatchesUp() async throws {
        var state = try OwnerReplay.work(partial(84, offset: 0, text: "Hello"), into: selected())
        state.nextPageToken = "older-at-80"
        state.historyOrder = ["older"]
        state.transcript.insert(.init(id: "older", revision: 70, role: "user", text: "Earlier page", timestamp: nil), at: 0)
        let endpoint = MeshEndpoint(url: URL(string: "https://100.64.0.1:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))
        let host = HostDescriptor(ownerNodeId: owner, name: "Mac", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
            endpoints: [endpoint], capabilities: [], registrationState: nil, tlsSpkiSha256: nil)
        let snapshot = WorkSnapshot(entry: entry(100), pendingInputs: [], historyWatermark: .init(ownerNodeId: owner, storeGeneration: generation, revision: 100), workCursor: "snapshot-100",
            liveMessages: [.init(messageId: "m", revision: 100, message: message("Hello newer snapshot"))])
        let wire = ScriptedTransport([.response(200, try ControllerCodec.encoder().encode(snapshot))])
        let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: wire))
        state = try await service.refreshSelected(state, host: host, credential: "test")
        let requests = await wire.requests
        XCTAssertEqual(requests.count, 1, "Same-work refresh needs only snapshot metadata")
        XCTAssertEqual(state.historyRevision, 80)
        XCTAssertEqual(state.nextPageToken, "older-at-80")
        XCTAssertEqual(state.workCursor, "signed-84")
        XCTAssertEqual(state.transcript.map(\.text), ["Earlier page", "Hello"])
        state = try OwnerReplay.work(partial(90, offset: 5, text: " newer"), into: state)
        XCTAssertEqual(state.transcript.last?.text, "Hello newer")
        XCTAssertEqual(state.entry.revision, 100)
        state = try OwnerReplay.work(frame(95, type: "message_deleted", payload: .object(["message_id": .string("older")])), into: state)
        XCTAssertEqual(state.transcript.map(\.id), ["m"])
    }

    func testReplayBeforeFixedHistoryCannotResurrectAnUnloadedDeletedMessage() throws {
        var state = selected()
        state.streamRevision = 60
        let item = HistoryItem(messageId: "already-deleted-at-snapshot", revision: 70, message: message("Must stay absent"))
        let encoded = try ControllerCodec.decoder().decode(JSONValue.self, from: ControllerCodec.encoder().encode(item))
        state = try OwnerReplay.work(frame(70, type: "message_upsert", payload: .object(["item": encoded])), into: state)
        XCTAssertTrue(state.transcript.isEmpty)
        XCTAssertEqual(state.workCursor, "signed-70")
        XCTAssertEqual(state.historyRevision, 80)
    }

    func testScrollPolicyFollowsGrowthOnlyAtBottomAndKeepsOlderReading() {
        var scroll = SessionScrollState()
        XCTAssertTrue(scroll.shouldFollowContent)
        scroll.userScrolled(nearBottom: false)
        XCTAssertFalse(scroll.shouldFollowContent)
        scroll.beginOlder()
        XCTAssertFalse(scroll.shouldFollowContent)
        scroll.endOlder()
        XCTAssertFalse(scroll.shouldFollowContent, "Finishing older history or a same-work refresh must not resume following")
        scroll.jumpToLatest()
        XCTAssertTrue(scroll.shouldFollowContent)
        scroll.userScrolled(nearBottom: true)
        XCTAssertTrue(scroll.shouldFollowContent)
        let before = TranscriptItem(id: "same", revision: 1, role: "assistant", text: "First", timestamp: nil)
        let grown = TranscriptItem(id: "same", revision: 2, role: "assistant", text: "First second", timestamp: nil)
        XCTAssertNotEqual([before], [grown], "The view's content trigger must detect same-ID streamed growth")
    }

    func testJumpReplacesTheBoundReadingAnchorWithTheBottomEdge() {
        var scroll = SessionScrollState()
        XCTAssertEqual(scroll.position.edge, .bottom)
        scroll.position.scrollTo(id: "oldest-visible", anchor: .top)
        scroll.userScrolled(nearBottom: false)
        XCTAssertEqual(scroll.position.viewID(type: String.self), "oldest-visible")
        scroll.jumpToLatest()
        XCTAssertEqual(scroll.position.edge, .bottom)
        XCTAssertNil(scroll.position.viewID(type: String.self), "The old reading ID must not compete with the explicit jump")
        XCTAssertTrue(scroll.shouldFollowContent)
    }

    func testRefreshAndPrependKeepReadingPositionUntilExplicitJump() {
        var scroll = SessionScrollState()
        scroll.position.scrollTo(id: "middle-message", anchor: .top)
        scroll.userScrolled(nearBottom: false)
        let readingPosition = scroll.position
        scroll.followContent()
        XCTAssertEqual(scroll.position, readingPosition)
        scroll.beginOlder()
        scroll.followContent()
        XCTAssertEqual(scroll.position, readingPosition)
        scroll.endOlder()
        scroll.followContent()
        XCTAssertEqual(scroll.position, readingPosition)
        scroll.jumpToLatest()
        XCTAssertEqual(scroll.position.edge, .bottom)
        scroll.position.scrollTo(id: "newest-message", anchor: .top)
        scroll.followContent()
        XCTAssertEqual(scroll.position.edge, .bottom, "Same-message growth must follow through the same bound position")
    }

    @MainActor func testSameWorkRefreshAndDeclinedSendRetainLoadedOlderHistory() async throws {
        let (store, owner) = try await historyStore()
        let work = entry().work
        await store.select(work)
        await store.loadMoreHistory()
        XCTAssertEqual(store.selectedWork?.transcript.map(\.id), ["older", "newest"])
        let reset = store.selectedHistoryReset
        await store.select(work)
        XCTAssertEqual(store.selectedWork?.transcript.map(\.id), ["older", "newest"])
        XCTAssertNil(store.selectedWork?.nextPageToken)
        XCTAssertEqual(store.selectedWork?.workCursor, "work-80")
        XCTAssertEqual(store.selectedHistoryReset, reset)
        store.saveDraft("Review before sending", for: work)
        await store.send(action: .steer)
        XCTAssertNotNil(store.sharingRequest)
        store.declineSharing()
        XCTAssertEqual(store.selectedHistoryReset, reset)
        XCTAssertEqual(store.selectedWork?.transcript.map(\.id), ["older", "newest"])
        let selections = await owner.selections
        XCTAssertEqual(selections, 2, "Declining sharing must not reload or jump the transcript")
        await store.sceneBackground()
    }

    @MainActor func testExpiredOlderPageResetsToLatestWithVisibleExplanation() async throws {
        let (store, _) = try await historyStore(expireOlder: true)
        await store.select(entry().work)
        let reset = store.selectedHistoryReset
        await store.loadMoreHistory()
        XCTAssertNotEqual(store.selectedHistoryReset, reset)
        XCTAssertNotNil(store.selectedHistoryNotice)
        XCTAssertEqual(store.selectedWork?.transcript.map(\.id), ["newest"])
        XCTAssertNil(store.selectedError)
        await store.sceneBackground()
    }

    @MainActor private func historyStore(expireOlder: Bool = false) async throws -> (ControllerStore, HistoryRefreshOwner) {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let storage = MemoryCredentials()
        let device = DeviceRecord(deviceId: UUID(), name: "Test", platform: "ios", role: "controller", revision: 1, scopes: [], createdAt: Date(), revokedAt: nil)
        try storage.write(CredentialEnvelope(authority: URL(string: "https://link.example")!, credentials: .init(accessToken: "test", tokenType: "Bearer", expiresAt: .distantFuture, refreshToken: "refresh", device: device)), account: "device")
        let host = HostDescriptor(ownerNodeId: owner, name: "Mac", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil,
            endpoints: [.init(url: URL(string: "https://100.64.0.1:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64))], capabilities: [], registrationState: nil, tlsSpkiSha256: nil)
        let transport = ScriptedTransport([.response(200, try ControllerCodec.encoder().encode(HostPage(revision: 1, hosts: [host])))])
        let owner = HistoryRefreshOwner(entry: entry(80), expireOlder: expireOlder)
        let store = ControllerStore(credentials: storage, owner: owner, transport: transport, directory: directory)
        await store.synchronize()
        return (store, owner)
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
        var current = entry
        if selections > 1 { current.revision = 84 }
        return .init(entry: current, transcript: [], pendingInputs: inputs, nextPageToken: nil, historyRevision: 80, workCursor: "work-80", streamRevision: 80)
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

private struct ClosedStreamTransport: OwnerEventTransport {
    let wire: ScriptedTransport
    var first: Data? = nil
    var failure: ControllerError? = nil
    func data(for request: URLRequest) async throws -> (Data, HTTPURLResponse) { try await wire.data(for: request) }
    func events(for request: URLRequest) -> AsyncThrowingStream<Data, Error> {
        AsyncThrowingStream {
            if let first { $0.yield(first) }
            $0.finish(throwing: failure ?? URLError(.networkConnectionLost) as Error)
        }
    }
}

private actor HistoryRefreshOwner: OwnerControllerService {
    let entry: CatalogEntry
    let expireOlder: Bool
    var selections = 0
    init(entry: CatalogEntry, expireOlder: Bool) { self.entry = entry; self.expireOlder = expireOlder }
    func catalog(host: HostDescriptor, credential: String) async throws -> OwnerCatalogSnapshot {
        .init(entries: [entry], watermark: .init(ownerNodeId: entry.work.ownerNodeId, storeGeneration: entry.storeGeneration, revision: 80), resumeCursor: "catalog-80")
    }
    func selected(work: CatalogEntry, host: HostDescriptor, credential: String) async throws -> SelectedWork {
        selections += 1
        return .init(entry: entry, transcript: [.init(id: "newest", revision: 80, role: "assistant", text: "Latest page", timestamp: nil)],
            pendingInputs: [], nextPageToken: "older-80", historyRevision: 80, workCursor: "work-80", streamRevision: 80, historyOrder: ["newest"])
    }
    func history(work: CatalogEntry, pageToken: String, host: HostDescriptor, credential: String) async throws -> HistoryPage {
        if expireOlder { throw ControllerError.remote(410, "snapshot_required", "Expired page") }
        return .init(work: entry.work, storeGeneration: entry.storeGeneration, revision: 80,
            items: [.init(messageId: "older", revision: 70, message: .object(["role": .string("user"), "content": .array([.object(["type": .string("text"), "text": .string("Earlier page")])])]))], nextPageToken: nil, workCursor: "work-80")
    }
    func catalogEvents(cursor: String, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<CatalogChange, Error> { AsyncThrowingStream { _ in } }
    func workEvents(work: SelectedWork, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<WorkFrame, Error> { AsyncThrowingStream { _ in } }
    func resources(host: HostDescriptor, credential: String) async throws -> ResourceSample { throw ControllerError.unsupported("unused") }
    func submit(_ command: WorkCommand, host: HostDescriptor, credential: String) async throws -> CommandReceipt { throw ControllerError.unsupported("unused") }
    func receipt(commandId: UUID, host: HostDescriptor, credential: String) async throws -> CommandReceipt { throw ControllerError.unsupported("unused") }
}
