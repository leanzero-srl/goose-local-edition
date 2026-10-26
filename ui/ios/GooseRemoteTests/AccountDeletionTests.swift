import XCTest
@testable import GooseRemote

private struct RejectConfirmingStorage: CredentialStorage {
    let base: MemoryCredentials
    func read<T: Decodable>(_ type: T.Type, account: String) throws -> T? { try base.read(type, account: account) }
    func write<T: Encodable>(_ value: T, account: String) throws {
        if let deletion = value as? SavedAccountDeletion, deletion.phase == .confirming {
            throw ControllerError.credentialStore(-50)
        }
        try base.write(value, account: account)
    }
    func remove(account: String) throws { try base.remove(account: account) }
    func removeRevocationRequests() throws { try base.removeRevocationRequests() }
}

private struct FailingDeletionMesh: ControllerMeshSession {
    func prepare(identity: CredentialEnvelope) async throws {}
    func suspend() async {}
    func removeLocalIdentity() async throws { throw ControllerError.unavailable("Mesh cleanup failed") }
}

@MainActor
final class AccountDeletionTests: XCTestCase {
    private let authority = URL(string: "https://link.example/leanzero-link")!
    private let requestId = UUID(uuidString: "ADADADAD-1111-4111-8111-111111111111")!
    private let deviceId = UUID(uuidString: "DEDEDEDE-2222-4222-8222-222222222222")!
    private let expiry = Date(timeIntervalSince1970: 4_102_444_800)
    private func directory() -> URL { FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString) }
    private func seed(_ storage: any CredentialStorage, phase: SavedAccountDeletion.Phase? = nil) throws -> SavedAccountDeletion {
        let device = DeviceRecord(deviceId: deviceId, name: "My iPhone", platform: "ios", role: "controller", revision: 1,
                                  scopes: ["devices.manage"], createdAt: Date(), revokedAt: nil)
        let credential = DeviceCredentials(accessToken: "ordinary-access", tokenType: "Bearer", expiresAt: expiry,
                                           refreshToken: "ordinary-refresh", device: device)
        try storage.write(CredentialEnvelope(authority: authority, credentials: credential), account: "device")
        let saved = SavedAccountDeletion(authority: authority, requestId: requestId, deviceId: deviceId,
                                        phase: phase ?? .requestingCode, challenge: phase == .requestingCode ? nil : challenge())
        if phase != nil { try storage.write(saved, account: ControllerStore.deletionAccount) }
        return saved
    }
    private func challenge() -> AccountDeletionChallenge { .init(requestId: requestId, deletionToken: "purpose-only-recovery", expiresAt: expiry) }
    private func receipt(_ status: AccountDeletionReceipt.Status, id: UUID? = nil) -> AccountDeletionReceipt {
        .init(requestId: id ?? requestId, status: status, pending: status == .pending ? ["mesh"] : [],
              completedAt: status == .deleted ? Date() : nil, retained: ["security_revocation_record", "historical_service_records"])
    }
    private func reply<T: Encodable>(_ value: T) throws -> ScriptedTransport.Reply { .response(200, try ControllerCodec.encoder().encode(value)) }
    private func failure(_ status: Int, _ code: String) -> ScriptedTransport.Reply {
        .response(status, Data("{\"error\":{\"code\":\"\(code)\",\"message\":\"Not accepted\"}}".utf8))
    }

    func testActualKeychainRevocationCleanupPreservesDeletionReceiptAndOtherRecords() throws {
        let storage = KeychainStore(service: "net.leanzero.goose-remote.tests.deletion." + UUID().uuidString)
        let revoke = "revoke-" + UUID().uuidString
        defer {
            for name in [revoke, ControllerStore.deletionAccount, "other-record"] { try? storage.remove(account: name) }
        }
        try storage.write("old device metadata", account: revoke)
        try storage.write("receipt", account: ControllerStore.deletionAccount)
        try storage.write("unrelated", account: "other-record")
        try storage.removeRevocationRequests()
        XCTAssertNil(try storage.read(String.self, account: revoke))
        XCTAssertEqual(try storage.read(String.self, account: ControllerStore.deletionAccount), "receipt")
        XCTAssertEqual(try storage.read(String.self, account: "other-record"), "unrelated")
    }

    func testWireUsesNormalAccessOnlyForCodeAndPurposeTokenForConfirmAndStatus() async throws {
        let saved = SavedAccountDeletion(authority: authority, requestId: requestId, deviceId: deviceId, phase: .awaitingCode, challenge: challenge())
        let transport = ScriptedTransport([try reply(challenge()), try reply(receipt(.pending)), try reply(receipt(.deleted))])
        let api = try ControllerAPI(authority: authority, transport: transport)
        _ = try await api.requestAccountDeletionCode(requestId: requestId, token: "ordinary-access")
        _ = try await api.confirmAccountDeletion(saved, code: "012345")
        _ = try await api.accountDeletionStatus(saved)
        let requests = await transport.requests
        XCTAssertEqual(requests.map { $0.url!.absoluteString }, ["request-code", "confirm", "status"].map { authority.absoluteString + "/v1/controller/account/deletion/" + $0 })
        XCTAssertEqual(requests.map(\.httpMethod), ["POST", "POST", "POST"])
        XCTAssertEqual(requests.map { $0.value(forHTTPHeaderField: "Authorization") }, ["Bearer ordinary-access", "Bearer purpose-only-recovery", "Bearer purpose-only-recovery"])
        let bodies = try requests.map { try JSONSerialization.jsonObject(with: $0.httpBody!) as! [String: String] }
        XCTAssertEqual(bodies, [["request_id": requestId.uuidString], ["code": "012345"], [:]])
        XCTAssertTrue(requests.allSatisfy { $0.url!.query == nil })
    }

    func testInterruptedCodeRequestReusesSavedIdBeforeReceivingToken() async throws {
        let storage = MemoryCredentials(), dir = directory()
        defer { try? FileManager.default.removeItem(at: dir) }
        _ = try seed(storage)
        let firstTransport = ScriptedTransport([.disconnected])
        let first = ControllerStore(credentials: storage, transport: firstTransport, directory: dir)
        await first.requestAccountDeletionCode()
        let saved = try XCTUnwrap(storage.read(SavedAccountDeletion.self, account: ControllerStore.deletionAccount))
        XCTAssertEqual(saved.phase, .requestingCode)
        XCTAssertNil(saved.challenge)
        let result = AccountDeletionChallenge(requestId: saved.requestId, deletionToken: "recovered-token", expiresAt: expiry)
        let nextTransport = ScriptedTransport([try reply(result)])
        let restarted = ControllerStore(credentials: storage, transport: nextTransport, directory: dir)
        await restarted.requestAccountDeletionCode()
        let initialRequests = await firstTransport.requests, repeatedRequests = await nextTransport.requests
        XCTAssertEqual(initialRequests.first?.httpBody, repeatedRequests.first?.httpBody)
        XCTAssertEqual(restarted.accountDeletion?.challenge?.deletionToken, "recovered-token")
        XCTAssertEqual(restarted.accountDeletion?.phase, .awaitingCode)
        XCTAssertTrue(restarted.authenticated)
    }

    func testWrongCodeKeepsNormalCredentialsAndDrafts() async throws {
        let storage = MemoryCredentials(), dir = directory()
        defer { try? FileManager.default.removeItem(at: dir) }
        _ = try seed(storage, phase: .awaitingCode)
        let work = WorkRef(ownerNodeId: UUID(), kind: .chat, workId: UUID())
        try LocalControllerStore(directory: dir).saveDraft("Keep this draft", work: work)
        let store = ControllerStore(credentials: storage, transport: ScriptedTransport([failure(401, "invalid_code")]), directory: dir)
        store.deletionCode = "000000"
        await store.confirmAccountDeletion()
        XCTAssertTrue(store.authenticated)
        XCTAssertEqual(store.accountDeletion?.phase, .awaitingCode)
        XCTAssertNotNil(try storage.read(CredentialEnvelope.self, account: "device"))
        XCTAssertEqual(store.draft(for: work), "Keep this draft")
        XCTAssertFalse(store.accountDeletionCompleted)
    }

    func testLostConfirmationRecoversPendingAcrossLogoutAndFinishesWithoutNormalCredentials() async throws {
        let storage = MemoryCredentials(), dir = directory()
        defer { try? FileManager.default.removeItem(at: dir) }
        _ = try seed(storage, phase: .awaitingCode)
        let work = WorkRef(ownerNodeId: UUID(), kind: .chat, workId: UUID())
        try LocalControllerStore(directory: dir).saveDraft("Private draft", work: work)
        try storage.write("private pending provider value", account: ManagementModel.outboxAccount)
        let revoke = "revoke-" + UUID().uuidString
        try storage.write("old device metadata", account: revoke)
        let first = ControllerStore(credentials: storage, transport: ScriptedTransport([.disconnected]), directory: dir)
        first.deletionCode = "012345"
        await first.confirmAccountDeletion()
        XCTAssertFalse(first.authenticated)
        XCTAssertEqual(first.accountDeletion?.phase, .confirming)
        XCTAssertNotNil(try storage.read(CredentialEnvelope.self, account: "device"))
        var expiredCodeReceipt = try XCTUnwrap(storage.read(SavedAccountDeletion.self, account: ControllerStore.deletionAccount))
        expiredCodeReceipt.challenge = .init(requestId: requestId, deletionToken: "purpose-only-recovery", expiresAt: Date(timeIntervalSince1970: 1))
        try storage.write(expiredCodeReceipt, account: ControllerStore.deletionAccount)
        let pendingTransport = ScriptedTransport([try reply(receipt(.pending))])
        let reopened = ControllerStore(credentials: storage, transport: pendingTransport, directory: dir)
        await reopened.checkAccountDeletion()
        XCTAssertNil(reopened.envelope)
        XCTAssertTrue(reopened.entries.isEmpty)
        XCTAssertEqual(reopened.accountDeletion?.phase, .pending)
        XCTAssertNil(try storage.read(CredentialEnvelope.self, account: "device"))
        XCTAssertNil(try storage.read(String.self, account: ManagementModel.outboxAccount))
        XCTAssertNil(try storage.read(String.self, account: revoke))
        XCTAssertTrue(try LocalControllerStore(directory: dir).data.drafts.isEmpty)
        try await reopened.forgetDevice()
        XCTAssertEqual(try storage.read(SavedAccountDeletion.self, account: ControllerStore.deletionAccount)?.challenge?.deletionToken, "purpose-only-recovery")
        let finishTransport = ScriptedTransport([try reply(receipt(.deleted))])
        let finished = ControllerStore(credentials: storage, transport: finishTransport, directory: dir)
        await finished.checkAccountDeletion()
        XCTAssertTrue(finished.accountDeletionCompleted)
        XCTAssertNil(finished.accountDeletion)
        XCTAssertNil(try storage.read(SavedAccountDeletion.self, account: ControllerStore.deletionAccount))
        let requests = await finishTransport.requests
        XCTAssertEqual(requests.count, 1)
        XCTAssertTrue(requests[0].url!.path.hasSuffix("/status"))
        XCTAssertEqual(requests[0].value(forHTTPHeaderField: "Authorization"), "Bearer purpose-only-recovery")
    }

    func testConfirmationCannotLeaveDeviceWithoutDurableRecovery() async throws {
        let memory = MemoryCredentials(), dir = directory()
        defer { try? FileManager.default.removeItem(at: dir) }
        _ = try seed(memory, phase: .awaitingCode)
        let transport = ScriptedTransport([])
        let store = ControllerStore(credentials: RejectConfirmingStorage(base: memory), transport: transport, directory: dir)
        store.deletionCode = "012345"
        await store.confirmAccountDeletion()
        let requests = await transport.requests
        XCTAssertTrue(requests.isEmpty)
        XCTAssertTrue(store.authenticated)
        XCTAssertEqual(store.accountDeletion?.phase, .awaitingCode)
        XCTAssertNotNil(store.errorMessage)
    }

    func testUnacceptedConfirmationRestoresCodeEntryAndOriginalSession() async throws {
        let storage = MemoryCredentials(), dir = directory()
        defer { try? FileManager.default.removeItem(at: dir) }
        _ = try seed(storage, phase: .confirming)
        let store = ControllerStore(credentials: storage, transport: ScriptedTransport([failure(409, "deletion_not_confirmed")]), directory: dir)
        XCTAssertFalse(store.authenticated)
        await store.checkAccountDeletion()
        XCTAssertTrue(store.authenticated)
        XCTAssertEqual(store.accountDeletion?.phase, .awaitingCode)
        XCTAssertNotNil(try storage.read(CredentialEnvelope.self, account: "device"))
    }

    func testLocalCleanupFailureKeepsReceiptAndRevokesLocalCredentialsBeforeRetry() async throws {
        let storage = MemoryCredentials(), dir = directory()
        defer { try? FileManager.default.removeItem(at: dir) }
        _ = try seed(storage, phase: .awaitingCode)
        let first = ControllerStore(credentials: storage, transport: ScriptedTransport([try reply(receipt(.deleted))]), directory: dir, mesh: FailingDeletionMesh())
        first.deletionCode = "012345"
        await first.confirmAccountDeletion()
        XCTAssertFalse(first.authenticated)
        XCTAssertNil(try storage.read(CredentialEnvelope.self, account: "device"))
        XCTAssertEqual(first.accountDeletion?.phase, .deleted)
        XCTAssertFalse(first.accountDeletionCompleted)
        XCTAssertNotNil(try storage.read(SavedAccountDeletion.self, account: ControllerStore.deletionAccount))
        let restarted = ControllerStore(credentials: storage, transport: ScriptedTransport([try reply(receipt(.deleted))]), directory: dir)
        await restarted.checkAccountDeletion()
        XCTAssertTrue(restarted.accountDeletionCompleted)
        XCTAssertNil(try storage.read(SavedAccountDeletion.self, account: ControllerStore.deletionAccount))
    }

    func testWrongReceiptCannotEraseAccountOrRecovery() async throws {
        let storage = MemoryCredentials(), dir = directory()
        defer { try? FileManager.default.removeItem(at: dir) }
        _ = try seed(storage, phase: .confirming)
        let store = ControllerStore(credentials: storage, transport: ScriptedTransport([try reply(receipt(.deleted, id: UUID()))]), directory: dir)
        await store.checkAccountDeletion()
        XCTAssertEqual(store.accountDeletion?.phase, .confirming)
        XCTAssertNotNil(try storage.read(CredentialEnvelope.self, account: "device"))
        XCTAssertNotNil(try storage.read(SavedAccountDeletion.self, account: ControllerStore.deletionAccount))
        XCTAssertFalse(store.accountDeletionCompleted)
    }
}
