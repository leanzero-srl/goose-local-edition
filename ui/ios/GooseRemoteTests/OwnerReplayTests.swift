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
        let request = scroll.beginOlder()
        XCTAssertFalse(scroll.shouldFollowContent)
        scroll.endOlder(request: request)
        XCTAssertFalse(scroll.shouldFollowContent, "Finishing older history or a same-work refresh must not resume following")
        scroll.jumpToLatest()
        XCTAssertTrue(scroll.shouldFollowContent)
        scroll.userScrolled(nearBottom: true)
        XCTAssertTrue(scroll.shouldFollowContent)
        let before = TranscriptItem(id: "same", revision: 1, role: "assistant", text: "First", timestamp: nil)
        let grown = TranscriptItem(id: "same", revision: 2, role: "assistant", text: "First second", timestamp: nil)
        XCTAssertNotEqual([before], [grown], "The view's content trigger must detect same-ID streamed growth")
    }

    func testJumpReplacesTheBoundReadingAnchorWithTheActualTailTarget() {
        var scroll = SessionScrollState()
        XCTAssertEqual(scroll.position.viewID(type: SessionScrollTarget.self), .latest)
        scroll.position.scrollTo(id: SessionScrollTarget.message("oldest-visible"), anchor: .top)
        scroll.userScrolled(nearBottom: false)
        XCTAssertEqual(scroll.visibleMessageID, "oldest-visible")
        scroll.jumpToLatest()
        XCTAssertEqual(scroll.position.viewID(type: SessionScrollTarget.self), .latest)
        XCTAssertNil(scroll.visibleMessageID, "The old reading ID must not compete with the explicit jump")
        XCTAssertTrue(scroll.shouldFollowContent)
    }

    func testRefreshAndPrependKeepReadingPositionUntilExplicitJump() {
        var scroll = SessionScrollState()
        scroll.position.scrollTo(id: SessionScrollTarget.message("middle-message"), anchor: .top)
        scroll.userScrolled(nearBottom: false)
        let readingPosition = scroll.position
        scroll.followContent()
        XCTAssertEqual(scroll.position, readingPosition)
        let request = scroll.beginOlder()
        scroll.followContent()
        XCTAssertEqual(scroll.position, readingPosition)
        scroll.endOlder(request: request)
        scroll.followContent()
        XCTAssertEqual(scroll.position, readingPosition)
        scroll.jumpToLatest()
        XCTAssertEqual(scroll.position.viewID(type: SessionScrollTarget.self), .latest)
        scroll.position.scrollTo(id: SessionScrollTarget.message("newest-message"), anchor: .top)
        scroll.followContent()
        XCTAssertEqual(scroll.position.viewID(type: SessionScrollTarget.self), .latest, "Same-message growth must follow the actual tail through the same bound position")
    }

    func testPrependAtHeaderRetainsPreviousFirstMessageOnce() {
        var scroll = SessionScrollState()
        scroll.userScrolled(nearBottom: false)
        let request = scroll.beginOlder(transcriptIDs: ["previous-first", "last"])
        scroll.endOlder(request: request, didPrepend: true)
        XCTAssertEqual(scroll.visibleMessageID, "previous-first")
        scroll.position.scrollTo(id: SessionScrollTarget.message("another-row"), anchor: .top)
        scroll.endOlder(request: request, didPrepend: true)
        XCTAssertEqual(scroll.visibleMessageID, "another-row")
    }

    func testPrependAtHeaderReanchorsAnAlreadyBoundFirstMessage() {
        var scroll = SessionScrollState()
        scroll.position = ScrollPosition(id: SessionScrollTarget.message("previous-first"))
        scroll.userScrolled(nearBottom: false)
        let headerPosition = scroll.position
        let request = scroll.beginOlder(transcriptIDs: ["previous-first", "last"], firstMessageBelowTop: true)
        scroll.endOlder(request: request, didPrepend: true)
        XCTAssertEqual(scroll.visibleMessageID, "previous-first")
        XCTAssertNotEqual(scroll.position, headerPosition, "An explicit top anchor must replace the header's tracked identity")
    }

    func testPrependInsideFirstMessageKeepsItsExistingPosition() {
        var scroll = SessionScrollState()
        scroll.position = ScrollPosition(id: SessionScrollTarget.message("first"), anchor: .center)
        scroll.userScrolled(nearBottom: false)
        let readingPosition = scroll.position
        let request = scroll.beginOlder(transcriptIDs: ["first", "last"], firstMessageBelowTop: false)
        scroll.endOlder(request: request, didPrepend: true)
        XCTAssertEqual(scroll.position, readingPosition)
    }

    func testPrependDoesNotReplaceAnExistingMessagePosition() {
        var scroll = SessionScrollState()
        scroll.position.scrollTo(id: SessionScrollTarget.message("middle"), anchor: .center)
        scroll.userScrolled(nearBottom: false)
        let readingPosition = scroll.position
        let request = scroll.beginOlder(transcriptIDs: ["first", "middle", "last"])
        scroll.endOlder(request: request, didPrepend: true)
        XCTAssertEqual(scroll.position, readingPosition)
    }

    func testOlderFallbackDoesNotOverrideFailedLoadJumpScrollOrNewSelection() {
        for interruption in ["failed", "jump", "scroll", "selection"] {
            var scroll = SessionScrollState()
            let request = scroll.beginOlder(transcriptIDs: ["previous-first", "last"])
            switch interruption {
            case "jump": scroll.jumpToLatest()
            case "scroll":
                scroll.position.scrollTo(id: SessionScrollTarget.message("reader-chosen"), anchor: .center)
                scroll.userScrolled(nearBottom: false)
            case "selection": scroll = SessionScrollState()
            default: break
            }
            let expected = scroll.position
            scroll.endOlder(request: request, didPrepend: interruption != "failed")
            XCTAssertEqual(scroll.position, expected, interruption)
        }
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

    @MainActor func testLazyTargetLookupAfterPrependProbe() async throws {
        let baseline = try await runTargetLookupProbe(combined: false, useLastMessage: false)
        let messageControl = try await runTargetLookupProbe(combined: false, useLastMessage: true)
        let combined = try await runTargetLookupProbe(combined: true, useLastMessage: false)
        print("TARGET_LOOKUP baseline=\(baseline) lastMessage=\(messageControl) combined=\(combined)")
        XCTAssertFalse(baseline, "Diagnosis requires reproducing the unresolved standalone target before attributing a cause")
        XCTAssertTrue(messageControl, "The same lazy stack must resolve an ordinary ForEach message as a positive control")
        XCTAssertTrue(combined, "The proposed registration must move the actual tail into the viewport")
    }

    @MainActor func testRepresentativeVariableHeightTailAnchorsAfterPrepend() async throws {
        let top = try await runTargetLookupProbe(combined: true, useLastMessage: false, representative: true, followSizeChanges: false)
        let bottom = try await runTargetLookupProbe(combined: true, useLastMessage: false, representative: true, followSizeChanges: true)
        print("REPRESENTATIVE_TAIL topSizeAnchor=\(top) followingSizeAnchor=\(bottom)")
        XCTAssertTrue(top, "The current production behavior must expose the complete representative final message")
        XCTAssertTrue(bottom, "The size-change comparison must expose the complete representative final message")
    }

    @MainActor private func runTargetLookupProbe(combined: Bool, useLastMessage: Bool, representative: Bool = false, followSizeChanges: Bool = false) async throws -> Bool {
        let model = TargetLookupProbeModel(combined: combined, representative: representative, followSizeChanges: followSizeChanges)
        let scene = try XCTUnwrap(UIApplication.shared.connectedScenes.first as? UIWindowScene)
        let previous = scene.keyWindow
        let window = UIWindow(windowScene: scene)
        let host = UIHostingController(rootView: TargetLookupProbeView(model: model))
        window.rootViewController = host
        window.makeKeyAndVisible()
        defer { window.isHidden = true; previous?.makeKey() }
        host.view.layoutIfNeeded()
        func settle(_ predicate: @escaping () -> Bool) async -> Bool {
            let deadline = ContinuousClock.now.advanced(by: .seconds(3))
            repeat {
                host.view.layoutIfNeeded()
                if predicate() { return true }
                try? await Task.sleep(for: .milliseconds(20))
            } while ContinuousClock.now < deadline
            return predicate()
        }
        func allScrollViews(_ view: UIView) -> [UIScrollView] {
            (view as? UIScrollView).map { [$0] } ?? view.subviews.flatMap(allScrollViews)
        }
        let scroll = try XCTUnwrap(allScrollViews(host.view).first)
        model.viewportHeight = scroll.bounds.height - scroll.adjustedContentInset.top - scroll.adjustedContentInset.bottom
        let initialTailVisible = await settle { model.tailVisible }
        print("TARGET_LOOKUP_INITIAL combined=\(combined) messageControl=\(useLastMessage) visible=\(initialTailVisible) viewport=\(model.viewportHeight)")
        model.firstFrame = .null
        model.scrolling.userScrolled(nearBottom: false)
        model.scrolling.position.scrollTo(id: SessionScrollTarget.message("109"), anchor: .top)
        let movedAway = await settle { abs(model.firstFrame.minY) < 2 }
        XCTAssertTrue(movedAway, "The probe must start from the previous first message, not the independent initial bottom anchor")
        let request = model.scrolling.beginOlder(transcriptIDs: model.rows.map(String.init), firstMessageBelowTop: true)
        model.rows = Array(0..<237)
        model.scrolling.endOlder(request: request, didPrepend: true)
        let prepended = await settle { abs(model.firstFrame.minY) < 2 && model.renderedCount == 237 }
        XCTAssertTrue(prepended, "The target request requires the old first row visibly retained after the237-row prepend")
        guard movedAway && prepended else { return false }
        model.tailFrame = .null
        model.lastFrame = .null
        if useLastMessage {
            model.scrolling.position.scrollTo(id: SessionScrollTarget.message("236"), anchor: .bottom)
        } else {
            model.scrolling.jumpToLatest()
        }
        let reached = await settle { useLastMessage ? model.lastVisible : model.tailVisible }
        print("TARGET_LOOKUP_NATIVE offset=\(scroll.contentOffset.y) contentHeight=\(scroll.contentSize.height) boundsHeight=\(scroll.bounds.height) topInset=\(scroll.adjustedContentInset.top) bottomInset=\(scroll.adjustedContentInset.bottom)")
        print("TARGET_LOOKUP_CASE combined=\(combined) messageControl=\(useLastMessage) reached=\(reached) tailBottom=\(model.tailFrame.maxY) lastBottom=\(model.lastFrame.maxY) viewport=\(model.viewportHeight)")
        return reached
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

@MainActor @Observable private final class TargetLookupProbeModel {
    var scrolling = SessionScrollState()
    var rows = Array(109..<237)
    let combined: Bool
    let representative: Bool
    let followSizeChanges: Bool
    @ObservationIgnored var firstFrame = CGRect.null
    @ObservationIgnored var lastFrame = CGRect.null
    @ObservationIgnored var tailFrame = CGRect.null
    @ObservationIgnored var viewportHeight: CGFloat = 0
    @ObservationIgnored var renderedCount = 0
    init(combined: Bool, representative: Bool = false, followSizeChanges: Bool = false) {
        self.combined = combined
        self.representative = representative
        self.followSizeChanges = followSizeChanges
    }
    func text(_ index: Int) -> String {
        guard representative else {
            return "Message \(index). " + String(repeating: "A variable length synthetic transcript line. ", count: 1 + index % 31)
        }
        return representativeTranscriptShape[index].map { line in
            line.map { length in String(String(repeating: "sample", count: length / 6 + 1).prefix(length)) }.joined(separator: " ")
        }.joined(separator: "\n")
    }
    var tailVisible: Bool { tailFrame.maxY > 0 && tailFrame.maxY <= viewportHeight + 1 }
    var lastVisible: Bool { lastFrame.maxY > 0 && lastFrame.maxY <= viewportHeight + 1 }
}

private struct TargetLookupProbeView: View {
    @Bindable var model: TargetLookupProbeModel
    private var targets: [SessionScrollTarget] { model.rows.map { .message(String($0)) } + [.latest] }
    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    Text("Owner header").frame(height: 100)
                    if model.rows.count == 128 { Text("Load older").frame(height: 40) }
                    if model.combined {
                        ForEach(targets, id: \.self) { target in row(target) }
                    } else {
                        ForEach(model.rows.map { SessionScrollTarget.message(String($0)) }, id: \.self) { target in row(target) }
                        if !model.representative { row(.message("receipt")) }
                        observedTail(Color.clear.frame(height: 1).id(SessionScrollTarget.latest).accessibilityHidden(true))
                    }
                }.scrollTargetLayout().padding(16).frame(maxWidth: 900)
                    .task(id: model.rows.count) { model.renderedCount = model.rows.count }
            }
            .scrollPosition($model.scrolling.position, anchor: .top)
            .defaultScrollAnchor(.bottom, for: .initialOffset)
            .defaultScrollAnchor(model.followSizeChanges && model.scrolling.followsLatest ? .bottom : .top, for: .sizeChanges)
            Text("Composer").frame(height: 120)
        }
    }
    private func row(_ target: SessionScrollTarget) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            switch target {
            case .message(let id):
                if id == "receipt" { Text("Pending command receipt").frame(height: 70) }
                else {
                let index = Int(id)!
                StudioPanel {
                    Text("Assistant").font(.caption.weight(.bold))
                    Text(model.text(index)).textSelection(.enabled)
                }
                .onGeometryChange(for: CGRect.self) { $0.frame(in: .scrollView(axis: .vertical)) } action: { _, frame in
                    if index == 109 { model.firstFrame = frame; model.renderedCount = model.rows.count }
                    if index == 236 { model.lastFrame = frame }
                }
                }
            case .latest:
                if !model.representative { Text("Pending command receipt").frame(height: 70) }
                tail
            }
        }
    }
    private var tail: some View {
        observedTail(Color.clear.frame(height: 1).accessibilityHidden(true))
    }
    private func observedTail<T: View>(_ content: T) -> some View {
        content.onGeometryChange(for: CGRect.self) { geometry in
                let frame = geometry.frame(in: .scrollView(axis: .vertical))
                return CGRect(x: geometry.bounds(of: .scrollView(axis: .vertical))?.height ?? 0, y: frame.minY, width: frame.width, height: frame.height)
            } action: { _, frame in
                model.tailFrame = frame
            }
    }
}

// Integer word lengths and newline boundaries only; no original text, IDs or timestamps.
private let representativeTranscriptShape: [[[Int]]] = [
    [[3,3,8,1,4,4,6,2,5,9,9,10,3,1,7,11,7,11,5,3,4,7,4,2,75,4,3,2,3,5,4,11,7,5,5,5,6],[0],[7,4,1,5,7,6,4,5,5,7,2],[1,4,2,7,3,5,9,10,2,3,2,5,3,5],[1,3,9,4,4,3,6,5,6,3,3,2,8],[1,6,2,4,3,8,3,2,4,2,4,2,5,5,5,3,4,11,3,12,4,4,2,5,2,3,15,4,8,3,12],[1,8,1,2,5,2,2,7,4,2,3,6,5,6,3,2],[1,3,7,5,4,5,2,7,4,2,9,4,3,5],[1,8,7,8,2,8,5,5,7],[1,3,4,7,3,8,4,4,7,5,7],[1,13,9,6,1,3,3,6,7,2,3,5],[1,4,4,3,4],[0],[6,4,4,4,1,5,5,4,2,16,4,5,9,10,7,5,7,4,10,4,5,5,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,5,4,3,2,6,5,5,5,2,10,2,12,3,4,2,7,2,3,7,4,2,4,5,1,1,7,4,4,11,2,1,8,2,8,3,7,5,4,2,8,3,8,3,4,3,11,2,3,6,2,3,13,2,7,2,3,11,6,4,8,5,3],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,1,18,2,8,6,3,4,4,4,3,3,8,5,1,4,6,4,8,3],[0],[13,8,2,3,6,10,3,2,6,8,10,6,4,4,7,5,5,8,12,1,6,1,3,1,10,8],[0],[9,4,7,1,3,6,5,2,2,5,3,3,1,5,2,5,2,2,3,10,2,7,7,1,8,5,6,5,8,5,4,3,4,2,4,3,3,9,3,2,5,6,5,4,3,3,14,6,3,3,7,10,3,10,3,10,6,2,6],[0],[6,11,1,6],[2,10,2,9,2,3,4,7,9,8,2,3,5,2,2,5,4,3,3,2,10,4,6,1,4,2,2,6,4,4,5,11,6,1,7,12,11,2,8],[2,5,3,1,8,3,6,4,2,4,8,3,3,4,3,10,3,3,8,5,3,4,2,4,3,8,3,2,4,1,2,1,12,2,3,6,2,3,4,4,5,3,5,6,10,3,5,10,5,2,5,2,5,5,3,3,7,4,2,6,5,6,5,6,3,7,7,8,3,4,6],[2,9,10,1,5,10,3,6,4,3,3,4,4,9,4,3,7,3,3,15,5,2,3,8,2,8,5,6,3,8],[0],[10,4,4,5,2,2,5,1,7,8,3,3,7,5,7,4,2,3,6,5,2,3,4,3,2,8,3,3,3,7,10,3,2,8,3,3,9,4,7,8,9,2,6,4,3,15,9],[0],[3,6,2,5,6,4,6,2,7,2,3,6,6,4,2,3,5,3,6,4,8,10]],
    [[3,6,2,8,5,3,1,5,3,4,3,3,6,4,1,4,3,5,12,3,7,8,2,8,4,4,2,1,7,3,7,1,4,4,3,6,5,4,4,4,3,13,2,7,9]],
    [[5,7,6,3,5,3,3,6,1,3,4,4,2,11,3,15,6,4,2,3,3,3,3,5,2,4,7,3,3,11,5,4,14,6,2,3,10,3,3,6,3,4,9,1,2,7,6,11,4,10,2,1,5,3,5,2,5,3,3,4,3,5,4,3,3,5,5,6,2,7,5,2,3,11,10,2,7,3],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,3,7,3,4,1,6,3,5,4,1,7,5,9,3,4,3,3,3,1,3,6,5,8,3,5,7,3,2,4,7,11,8,3,3,4,6,5,2,3,9,3,4,6,6,3,4,8,3,6,4,7,2,5,3,3,4,7],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,6,5,1,5,2,7,3,4,4,5,7,2,3,4,2,2,3,4,8,4,3,4,11,6,2,3,4,5,6,3,4,6,5,1,4,6,2,3,5,5,3,2,3,5,7,5,6,3,6,5,9,3],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[7,3,3,5,4,2,7,6],[0],[5,5,3,9,7,2,3,6,3,6,3,7,8,1,4,6,2,8,4,4,2,1,7,7,3,4,3,3,7,6,4,3,8,5,4,5],[0],[5,15,3,2,6,2,4,7,7,1,4,2,2,8,5,4,3,4,2,3,4,2,2,6,2,8,1,4,6,3,3,4,1,5,2,7,2,3,6,2,3,11,7,2,2,5,6,4,3,11,2,3,8,1,14,2,3,10,3,3,5,1,3,5,11,4,10,2,1,6,3,5,2,5,3,3,4,3,5,4,3,3,5,7,2,4,3,5,3,3,4,4,7,10,4,7,4,2,2,1,4,2,3,1,5,12,2,6],[0],[3,3,5,4,3,5,5,1,5,8,3,8,2,4,4,4,2,4,4,3,4,4,3,3,4,4,3,5,2,2,10,3,11,3,6,3,3,4,10]],
    [[5,5,5,4,4,3,4,3,4,4,2,4,7,4,2,4,9,3,10,9,5,4,6,3,2,4,1,3,2,8,4,3,6,3,7,3,4,5,9,9,8,4,5,4,2,3,8]],
    [[5,6,3,6,4,4,4,6,5,1,5,2,7,3,4,4,5,7,3,4,6,5,1,5,2,7,3,4,4,5,7,2,3,4,2,2,3,4,8,4,3,4,11,6,2,3,4,7,4,6,4,5]],
    [[3,2,6,3,3,7,11,5,7,9,10],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[7,5,7,3,2,4,2,1,7,7,3,8,11,3,6,3,11,3,19,4],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,6,3,2,5,5,11],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,6,6,3,2,5,3,2,3,3,5,5,8,1,11,3,6,6,5,1,4,3,5,3],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,6,3,2,5,3,10,5,5,1,5,6],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,4,3,8,3,5,6,7,4,3,4,4,7,4,7,3,1,6,4,1,3,4,3,3,6,7,9],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[2,15,8,3,2,5,3,6,3,1,4,27,5,3,1,11,5,3,1,9,6],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,9,8,2,2,3,3,8,7,3,4,6,2,3,2,3,2,3,7,5,3,4,7,6],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,3,4,2,1,3,4,10,3,4,5,3,2,6,7,3,7,9,5],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,2,3,3,4,7,7],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,8,6,3,3,4,2,8,10,45,7,4,4,1,17,3,3,6,3,6,3,8,7,2,9],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,4,4,5,3,3,4,4,3,3,17,2,4,3,4,2,3,6,4,1,6,4,9,5,2,5,4,7,1,6,1,5,2,6,3,1,6,5,1,7,6,3,8,4,2,2,3,6,5,2,9],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,5,6,9,3,9,2,3,2,1,7,5,3,1,2,6,3,4,3,3,3,8,3,3,7,5,2,4,11,2,29,2,6,4,6,5,7,6,6,2,3,5,3,2,3,2,2,3,5,1,5,3,3,3,10,7,7,5,2,3,8,2,2,7],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,3,4,3,5,4,3,2,3,3,7,9,3,3,5,3,3,6,4,2,9],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[1,3,4,5,5,5,3,3,8,5,1,4,3,3,5,3,8,5,2,2,3,10,6,8,3,8,3,5,3,5,7,6,5,5,2,3,4],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[2,7,8,3,10,5,5,1,15,1,2,3,2,2,3,6,7],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[6,3,4,2,3,7,5,6,1,11,2,4,3,5,7],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,6,6,4,2,7,4,6,5,3,3,5,8,4,3,7,8,9],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[5,3,2,9,1,8,3,7,2,7,3,4,2,5,5,4,13],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,6,6,6,8,3,3,6,4,7,4,1,4,6,9,13,6,2,3,3,4,4,8,1,2,6,7,5,4,5,5,5,2,4,6,2,5,7],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,3,6,6,3,4,3,3,5,4,5,6,3,2,3,2,8,7,3,5,3,8,9,5,1,3,9,6,4,4,6,4,8,2,4,4,12,3,5,9,4,3,3,9,10],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,9,6,3,5,8,3,3,9,4,3,1,6,3,6,2,5,7,7,3,3,19,3,6,4,2,4,2,29,1,3,2,4,4,3,7,6,8,6,4,8,4,2,1,4,4,5,5],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,4,3,12],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,2,3,7,1,5,3,7,5,12,3,2,4,3,6,9,4,6,3],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,2,4,8,2,4,3,6,6,4,7,3,3,8,4],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[6,5,3,2,5,2,4,8,4,7,1,9,3,4,9,5,3,5,5,9,10],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,8,4,5,3,5,5,3,5,6,6,3,2,3,3,6,9,6,4,1,3,12,4,1,5,3,5,3,5,9,6,3,5,2,1,9,5,3,2,5,1,3,4,10,9],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[7,4,3,4,7,4,2,1,9,4,4,3,2,4,3,13,4,5,8,7,2,8,10,5,1,4,4,4,2,3,4,3,5,3,3,7,7,5,2,3,5],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,4,9,7,5,6,3,3,6,5,7,1,1,3,8,5,9,9,1,11,6,6,3,3,6,8,2,6,5,2,3,2,3,4,7,2,8,6],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,8,2,5,4,4,8,5,2,1,4,4,5,4,1,1,10,6,2,2,3,7,3,2,3,3,4,4,4,1,6,5,4,5,2,4],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,1,4,4,61],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,4,7,3,5,3,8,3,3,3,3,7,2,24,3,5,4,2,8,2,8,2,4,6],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,6,5,10,3,3,9,9,7,8,1,3,2,4,3,5,4],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,7,3,3,6,3,2,5,3,7,14],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,4,3,9,2,42,3,5,3,5,5,14],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[2,7,5,2,4,5,3,14,3,7,4],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,4,10,3,3,7,5,2,4,2,6,5,4,2,3,7,5,3,1,9,3,12,7,7,3,3,2,8,4,5,1,4,7,4,4,1,5,2,3,5,6,3,2,5,3,9,6],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,2,5,3,9,5,9],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,4,2,6,5,2,3,6,4,1,4,1,6,6,2,7,2,6,3,2,5,4,3,10,8,7,3,1,4,4,10,9],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,7,3,1,4,4,3,3,15,3,2,3,3,4,7,3,5,7,13,3,2,3,3,4,3,4,3,13,6],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,9,5,2,2,39,1,3,2,3,3,6],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,2,3,3,9,5,3,9,3,4,5,3,2,9,5,5,3,4,7,13],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,2,4,3,9,5,4],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,2,3,3,6,9,5,3,5,3,3,7,13],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,2,4,8,3,4,3,4,5,8,2,3,3,4,5,4],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,2,5,3,13,8,4,1,3,7,9,5,7,4,27,9],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,2,3,3,9,5,3,9],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,2,4,8,4,3,4,3,9,4,6],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,2,4,8,4,3,3,3,6,9,4,6],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,9,5,3,5,7,3,2,5,3,13,3,4,2,4,3],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,9,5,3,5,7,3,3,8,2,6,3,2,4,3,2,9,5,5,6,4,3,7,13],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,5,3,8,3,5,1,6,6,2,7,2,3,2,4,7,3,6,3,4,8,6,2,3,6,4,3,7],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[1,4,4,7,3,1,3,3,5,4,3,6,5,3,3,7,5,4,3,9,9,7,4,4,4,7,8,4,5,3,2,5,3,8,5],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[3,5,2,4,1,4,5,4,2,3,5,5,4,7,3,10,5,2,6,9,5,3,2,6,4,5,2,3,6,4,3,5,2,3],[0],[0]],
    [[4,7,1,4,3,5,3,3,3,4,8]],
    [[4,8,1,4,3,5,3,3,3,4,8]],
    [[4,9,3,7,3,8,3,4,5,3,2,11,3,6,5,1,6,2,5,5,11,3,8,10,20,2,5,7,4,2,3,6,7,4,3,6,4,3,7,5,2,1,6,3,1,8],[0],[5,4,3,4,4,2,4,9],[1,4,3,6,1,3,2,4,3,5,10,4,3,2,8,9,2,4,4,3,4,5],[1,4,3,6,1,4,4,5,4,2,3,10,3,8,4,5,1,8,7,4,5,5,4,4,8,4,2,2,3,5],[1,4,3,5,5,5,1,3,2,5,13,7,3,8,4,10,2,7,2,8,6,9,5,5,10,10,4,4,4,3,9,5,1,2,4,2,3,8,3,3,4,5,3,2,9,3,2,2,5,4],[1,9,2,3,4,5,3,6,3,2,3,4,11,2,8,5,2,4,4,3,5,5],[0],[9,6,3,3,7,6,4,5,5,2,4,3,8,3,3,6,6,6],[0],[6,3,4,5,9,9,7,4,7],[1,3,7,3,4,3,9,2,13,2,6,4,3,7,3,8,11,2,4,8,4,6,5,2,8,4,2,5,3,6,2,4],[1,9,10,10,9,7,2,7,2,3,9,3,3,5,12,7,6],[1,3,3,2,1,6,9,5,3,2,8,4,4,3,7,4,5,1,2,3,6,6,2,3,6,2,3,11,3,8,3,9,2,7,3,2,8,2,5,6,10],[0],[7,3,2,3,8,4,2,29,4,3,5,4,3,5,5]],
    [[3,4,4,6,2,5,3,2,1,5,4,2,2,4,5,1,5,6,4,9,1,9,4,15,5,3,5,4,3,4,8,4,10,9,9,7,6,2,5,4,2,6,4,7,9,8,5,4,4,5,5,4,3,5,7,19,3,20,8,9,13,6,7,11,7,4,2,2,4,13]],
    [[3,4,4,6,9,6,5,5,2,4,3,5,4,4,1,10,4,5,3,4,2,6,4,6,3,6,7,7,4,26,1,4,2,2,3,4,5,3,7,26,6,3,8,3,6,2,3,7,7,4,1,9,8,3,3,8,16,6,5,4,4,28,6,3,6,4,6],[0],[6,5,2,3,5,4,2,1,9,2,11,6]]
]
