import XCTest
@testable import GooseRemote

final class FailingManagementCredentials: CredentialStorage, @unchecked Sendable {
    let underlying = MemoryCredentials()
    var failWrites = false
    func read<T: Decodable>(_ type: T.Type, account: String) throws -> T? { try underlying.read(type, account: account) }
    func write<T: Encodable>(_ value: T, account: String) throws {
        if failWrites { throw ControllerError.unavailable("Keychain unavailable") }
        try underlying.write(value, account: account)
    }
    func remove(account: String) throws { try underlying.remove(account: account) }
    func removeRevocationRequests() throws { try underlying.removeRevocationRequests() }
}

actor ManagementServiceProbe: OwnerManagementService {
    let device: UUID
    let storage: MemoryCredentials
    var requests: [ManagementCommand] = []
    var loseAcknowledgement = false
    var wrongDevice = false
    var state: ReceiptState = .delivered
    var downloadMissing = false
    var invalidRepository = false
    var changedGeneration = false
    var nodesUnsupported = false
    var nodeReads = 0
    var onManage: (@MainActor @Sendable () -> Void)?
    init(device: UUID, storage: MemoryCredentials) { self.device = device; self.storage = storage }
    func configure(lose: Bool = false, wrongDevice: Bool = false, state: ReceiptState = .delivered) {
        loseAcknowledgement = lose; self.wrongDevice = wrongDevice; self.state = state
    }
    func setOnManage(_ callback: @escaping @MainActor @Sendable () -> Void) { onManage = callback }
    func changeGeneration() { changedGeneration = true }
    func rejectNodes() { nodesUnsupported = true }
    func rejectRepository() { invalidRepository = true }
    func setDownloadMissing() { downloadMissing = true }
    func models(host: HostDescriptor, credential: String) async throws -> ManagedModels { .init(models: [], diskAvailableBytes: 10, diskTotalBytes: 100) }
    func download(repo: String, host: HostDescriptor, credential: String) async throws -> ManagedDownload {
        if downloadMissing { throw ControllerError.remote(404, "download_missing", "No tracked download") }
        return .init(state: "downloading", totalBytes: 100, downloadedBytes: 20, currentFile: "weights", error: nil)
    }
    func providers(host: HostDescriptor, credential: String) async throws -> [ManagedProvider] { [] }
    func nodes(host: HostDescriptor, credential: String) async throws -> ManagedComputeNodes {
        if nodesUnsupported { throw ControllerError.remote(404, "not_found", "Missing route") }
        nodeReads += 1
        let nodes: [ManagedComputeNode] = state == .delivered && requests.last?.action == .nodeCreate ? [.init(id: "test-node", name: "Test", kind: "cloud", model: "claude-test", provider: "anthropic", macs: [], editable: true, definitionRevision: "revision-a")] : []
        return .init(nodes: nodes, providers: [], localModels: [], supportedKinds: ["cloud", "endpoint", "mlx"], capabilities: ["node_create", "node_remove"])
    }
    func manage(_ command: ManagementCommand, host: HostDescriptor, credential: String) async throws -> ManagementReceipt {
        let saved = try storage.read([PendingManagement].self, account: ManagementModel.outboxAccount) ?? []
        guard saved.contains(where: { $0.command.commandId == command.commandId }) else { throw ControllerError.invalid("Sent before durable save") }
        if changedGeneration { throw ControllerError.remote(409, "store_generation_mismatch", "Refresh owner") }
        requests.append(command)
        if invalidRepository { throw ControllerError.remote(400, "invalid_repository", "Expected owner/repo") }
        if let onManage { await onManage() }
        if loseAcknowledgement { throw URLError(.networkConnectionLost) }
        return receipt(command)
    }
    func managementReceipt(id: UUID, host: HostDescriptor, credential: String) async throws -> ManagementReceipt {
        if changedGeneration { throw ControllerError.remote(404, "command_missing", "No receipt in current store") }
        guard let command = requests.first(where: { $0.commandId == id }) else { throw ControllerError.remote(404, "not_found", "Missing") }
        return receipt(command)
    }
    private func receipt(_ command: ManagementCommand) -> ManagementReceipt {
        .init(commandId: command.commandId, ownerNodeId: command.ownerNodeId, authenticatedDeviceId: wrongDevice ? UUID() : device,
              action: command.action, state: state, target: "org/model", reason: nil, updatedAt: Date())
    }
}

@MainActor final class ManagementTests: XCTestCase {
    private let device = UUID()
    private let owner = UUID()
    private let generation = UUID()
    private var current = true
    private var endpoint: MeshEndpoint { .init(url: URL(string: "https://100.64.0.10:41227")!, tlsSpkiSha256: String(repeating: "a", count: 64)) }
    private var context: ManagementContext {
        let record = DeviceRecord(deviceId: device, name: "Controller", platform: "ios", role: "controller", revision: 1, scopes: ["models.download"], createdAt: Date(), revokedAt: nil)
        let credentials = DeviceCredentials(accessToken: "private-token", tokenType: "Bearer", expiresAt: Date().addingTimeInterval(600), refreshToken: "private-refresh", device: record)
        return .init(host: .init(ownerNodeId: owner, name: "Owner Mac", deviceRole: "host", protocolVersion: 1, availability: "online", lastSeenAt: nil, endpoints: [endpoint], capabilities: [], registrationState: nil, tlsSpkiSha256: nil), identity: .init(authority: URL(string: "https://link.example")!, credentials: credentials), generation: generation)
    }
    private func model(_ probe: ManagementServiceProbe, _ storage: any CredentialStorage) -> ManagementModel {
        .init(service: probe, storage: storage, context: { self.context }, isCurrent: { self.current && $0 == self.device })
    }
    func testDurableFailureSendsNothingAndSuccessfulRetryPersistsFirst() async throws {
        let storage = FailingManagementCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage.underlying)
        let model = model(probe, storage)
        storage.failWrites = true
        await model.perform(.providerConfigure, target: "Provider", payload: .object(["fields": .object(["API_KEY": .string("private-key")])]))
        XCTAssertNil(model.pending)
        var sent = await probe.requests
        XCTAssertTrue(sent.isEmpty)
        storage.failWrites = false
        await model.perform(.providerConfigure, target: "Provider", payload: .object(["fields": .object(["API_KEY": .string("private-key")])]))
        sent = await probe.requests
        XCTAssertEqual(sent.count, 1)
        XCTAssertEqual(model.receipt?.state, .delivered)
        XCTAssertTrue(try XCTUnwrap(storage.read([PendingManagement].self, account: ManagementModel.outboxAccount)).isEmpty)
    }
    func testLostAcknowledgementReopensWithSameCommandAndPayload() async throws {
        let storage = FailingManagementCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage.underlying)
        await probe.configure(lose: true)
        let first = model(probe, storage)
        await first.perform(.download, target: "org/model", payload: .object(["repo_id": .string("org/model")]))
        let original = try XCTUnwrap(first.pending?.command)
        let reopened = model(probe, storage)
        await reopened.load()
        XCTAssertEqual(reopened.pending?.command.commandId, original.commandId)
        storage.failWrites = true
        await reopened.retry()
        var sent = await probe.requests
        XCTAssertEqual(sent.count, 1, "Retry must also require durable storage")
        storage.failWrites = false
        await probe.configure()
        await reopened.retry()
        sent = await probe.requests
        XCTAssertEqual(sent.count, 2)
        XCTAssertEqual(try ControllerCodec.encoder().encode(sent[0]), try ControllerCodec.encoder().encode(sent[1]))
        XCTAssertNil(reopened.pending)
    }
    func testInvalidRepositoryDoesNotPermanentlyBlockOwnerManagement() async throws {
        let storage = MemoryCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage)
        await probe.rejectRepository()
        let model = model(probe, storage)
        await model.perform(.download, target: "invalid", payload: .object(["repo_id": .string("invalid")]))
        XCTAssertNil(model.pending)
        XCTAssertNotNil(model.error)
        XCTAssertTrue(try XCTUnwrap(storage.read([PendingManagement].self, account: ManagementModel.outboxAccount)).isEmpty)
    }
    func testWrongDeviceReceiptCannotClearSavedRequest() async throws {
        let storage = MemoryCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage)
        await probe.configure(wrongDevice: true)
        let model = model(probe, storage)
        await model.perform(.download, target: "org/model", payload: .object(["repo_id": .string("org/model")]))
        XCTAssertNotNil(model.pending)
        XCTAssertNil(model.receipt)
        XCTAssertEqual(try storage.read([PendingManagement].self, account: ManagementModel.outboxAccount)?.count, 1)
    }
    func testLogoutDuringMutationCannotRecreateDeletedCredentials() async throws {
        let storage = MemoryCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage)
        await probe.setOnManage { self.current = false; try? storage.remove(account: ManagementModel.outboxAccount) }
        let model = model(probe, storage)
        await model.perform(.download, target: "org/model", payload: .object(["repo_id": .string("org/model")]))
        XCTAssertNil(try storage.read([PendingManagement].self, account: ManagementModel.outboxAccount))
        XCTAssertNil(model.receipt)
    }
    func testUnknownReceiptRequiresExplicitDismissAndDoesNotResend() async throws {
        let storage = MemoryCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage)
        await probe.configure(state: .outcomeUnknown)
        let model = model(probe, storage)
        await model.perform(.download, target: "org/model", payload: .object(["repo_id": .string("org/model")]))
        XCTAssertNotNil(model.pending)
        await model.dismissUnknownRequest()
        XCTAssertNil(model.pending)
        XCTAssertEqual(model.receipt?.state, .outcomeUnknown)
        let sent = await probe.requests
        XCTAssertEqual(sent.count, 1)
    }
    func testChangedStoreMissingReceiptAllowsExplicitClearWithoutClaimingDelivery() async throws {
        let storage = MemoryCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage)
        await probe.changeGeneration()
        let model = model(probe, storage)
        await model.perform(.download, target: "org/model", payload: .object(["repo_id": .string("org/model")]))
        XCTAssertNotNil(model.pending)
        XCTAssertFalse(model.canDismissRequest)
        await model.checkReceipt()
        XCTAssertTrue(model.canDismissRequest)
        XCTAssertNil(model.receipt, "Missing receipt does not prove rejection or delivery")
        await model.dismissUnknownRequest()
        XCTAssertNil(model.pending)
        XCTAssertTrue(try XCTUnwrap(storage.read([PendingManagement].self, account: ManagementModel.outboxAccount)).isEmpty)
        let sent = await probe.requests
        XCTAssertTrue(sent.isEmpty)
    }
    func testChangingTrackedRepoAndMissingDownloadCannotShowOldProgress() async throws {
        let storage = MemoryCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage)
        let model = model(probe, storage)
        await model.watch(repo: "org/a")
        XCTAssertEqual(model.progress?.downloadedBytes, 20)
        await probe.setDownloadMissing()
        await model.watch(repo: "org/b")
        XCTAssertEqual(model.trackedRepo, "org/b")
        XCTAssertNil(model.progress)
        XCTAssertNil(model.progressAt)
        XCTAssertNotNil(model.error)
    }
    func testExistingCamelCaseDownloadWireAndSecretMaskBoundary() async throws {
        let bytes = Data(#"{"models":[{"id":"org/model","sizeBytes":1234,"complete":false,"missingFiles":2}],"diskAvailableBytes":100,"diskTotalBytes":200}"#.utf8)
        let models = try ControllerCodec.decoder().decode(ManagedModels.self, from: bytes)
        XCTAssertEqual(models.models.first?.missingFiles, 2)
        XCTAssertEqual(models.diskAvailableBytes, 100)
        let secret = Data(#"{"providers":[{"provider_id":"anthropic","display_name":"Anthropic","configured":true,"fields":[{"key":"API_KEY","required":true,"secret":true,"is_set":true,"value":"must-not-reach-ui"}]}]}"#.utf8)
        let transport = ScriptedTransport([.response(200, secret)])
        let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: transport))
        do { _ = try await service.providers(host: context.host, credential: "token"); XCTFail("Unredacted secret accepted") }
        catch { XCTAssertEqual(error as? ControllerError, .malformedResponse) }
        let requests = await transport.requests
        XCTAssertEqual(requests.first?.url?.path, "/v1/controller/management/providers")
        XCTAssertEqual(requests.first?.value(forHTTPHeaderField: "Authorization"), "Bearer token")
    }
    func testComputeNodeWireUsesOwnerRouteAndDecodesRevision() async throws {
        let data = Data(#"{"nodes":[{"id":"test-node","name":"Test","kind":"cloud","model":"claude-test","provider":"anthropic","macs":[],"editable":true,"definition_revision":"revision-a"}],"providers":[{"id":"anthropic","display_name":"Anthropic","models":["claude-test"]}],"local_models":["org/local-model"],"supported_kinds":["cloud","endpoint","mlx"],"capabilities":["node_create","node_remove"]}"#.utf8)
        let transport = ScriptedTransport([.response(200, data)])
        let service = OwnerHTTPControllerService(connections: FixedOwnerConnection(endpoint: endpoint, transport: transport))
        let result = try await service.nodes(host: context.host, credential: "token")
        XCTAssertEqual(result.nodes.first?.definitionRevision, "revision-a")
        XCTAssertEqual(result.providers.first?.displayName, "Anthropic")
        XCTAssertEqual(result.localModels, ["org/local-model"])
        let requests = await transport.requests
        XCTAssertEqual(requests.first?.url?.path, "/v1/controller/management/nodes")
        XCTAssertEqual(requests.first?.value(forHTTPHeaderField: "Authorization"), "Bearer token")
    }
    func testLegacyOwnerNodeRouteExplainsUpgradeWithoutSendingMutation() async throws {
        let storage = MemoryCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage)
        await probe.rejectNodes()
        let model = model(probe, storage)
        await model.load(kind: .nodes)
        XCTAssertNil(model.nodes)
        XCTAssertEqual(model.error, "Update Goose Swarm on this Mac to manage compute nodes from your controller.")
        let sent = await probe.requests
        XCTAssertTrue(sent.isEmpty)
    }
    func testNodeCreateLostAcknowledgementKeepsExactOwnerPayloadAndId() async throws {
        let storage = MemoryCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage)
        await probe.configure(lose: true)
        let first = model(probe, storage)
        let payload: JSONValue = .object(["id": .string("test-node"), "name": .string("Test"), "kind": .string("cloud"), "provider": .string("anthropic"), "model": .string("claude-test")])
        await first.perform(.nodeCreate, target: "Test", payload: payload)
        let original = try XCTUnwrap(first.pending?.command)
        let reopened = model(probe, storage)
        await reopened.load(kind: .nodes)
        XCTAssertEqual(reopened.pending?.command.commandId, original.commandId)
        await probe.configure()
        await reopened.retry()
        let sent = await probe.requests
        XCTAssertEqual(sent.count, 2)
        XCTAssertEqual(try ControllerCodec.encoder().encode(sent[0]), try ControllerCodec.encoder().encode(sent[1]))
        XCTAssertEqual(sent[1].action, .nodeCreate)
        XCTAssertEqual(sent[1].ownerNodeId, owner)
        XCTAssertEqual(reopened.receipt?.state, .delivered)
        XCTAssertNil(reopened.pending)
    }
    func testNodeRemovalRetainsExpectedDefinitionAcrossUnknownDelivery() async throws {
        let storage = MemoryCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage)
        await probe.configure(state: .outcomeUnknown)
        let model = model(probe, storage)
        let payload: JSONValue = .object(["id": .string("test-node"), "expected_definition_revision": .string("revision-a")])
        await model.perform(.nodeRemove, target: "Test", payload: payload)
        XCTAssertNotNil(model.pending)
        let encoded = try ControllerCodec.encoder().encode(XCTUnwrap(model.pending?.command))
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: encoded) as? [String: Any])
        let fields = try XCTUnwrap(json["payload"] as? [String: String])
        XCTAssertEqual(fields, ["id": "test-node", "expected_definition_revision": "revision-a"])
        XCTAssertEqual(json["action"] as? String, "node_remove")
        let sent = await probe.requests
        XCTAssertEqual(sent.count, 1)
    }
    func testNodeDraftOnlyAllowsAdvertisedModelsAndNoExecutionFields() throws {
        let inventory = ManagedComputeNodes(nodes: [], providers: [.init(id: "anthropic", displayName: "Anthropic", models: ["claude-test"])], localModels: ["org/local-model"], supportedKinds: ["cloud", "endpoint", "mlx"], capabilities: ["node_create"])
        var draft = ComputeNodeDraft(id: "test-node", name: " Test ", kind: "cloud", provider: "anthropic", model: "claude-test")
        var encoded = try ControllerCodec.encoder().encode(draft.payload(in: inventory))
        var fields = try XCTUnwrap(JSONSerialization.jsonObject(with: encoded) as? [String: String])
        XCTAssertEqual(fields, ["id": "test-node", "name": "Test", "kind": "cloud", "provider": "anthropic", "model": "claude-test"])
        draft.model = "not-advertised"
        XCTAssertThrowsError(try draft.payload(in: inventory))
        draft.kind = "mlx"; draft.model = "org/local-model"
        encoded = try ControllerCodec.encoder().encode(draft.payload(in: inventory))
        fields = try XCTUnwrap(JSONSerialization.jsonObject(with: encoded) as? [String: String])
        XCTAssertEqual(fields, ["id": "test-node", "name": "Test", "kind": "mlx", "model": "org/local-model"])
        draft.name = "  "
        XCTAssertThrowsError(try draft.payload(in: inventory))
    }

    func testAcceptedNodeCreationRefreshesOnlyAfterFinalDeliveryCheck() async throws {
        let storage = MemoryCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage)
        let model = model(probe, storage)
        await model.load(kind: .nodes)
        XCTAssertEqual(model.nodes?.nodes.count, 0)
        await probe.configure(state: .accepted)
        await model.perform(.nodeCreate, target: "Test", payload: .object(["id": .string("test-node")]))
        XCTAssertEqual(model.receipt?.state, .accepted)
        XCTAssertEqual(model.nodes?.nodes.count, 0)
        var reads = await probe.nodeReads
        XCTAssertEqual(reads, 1)
        await probe.configure(state: .delivered)
        await model.checkReceipt()
        XCTAssertEqual(model.nodes?.nodes.map(\.id), ["test-node"])
        XCTAssertNil(model.pending)
        reads = await probe.nodeReads
        XCTAssertEqual(reads, 2)
        let sent = await probe.requests
        XCTAssertEqual(sent.count, 1, "Receipt lookup must not resend the creation")
    }
    func testAppliedNodeWithFailedInventoryRefreshCannotShowStaleList() async throws {
        let storage = MemoryCredentials()
        let probe = ManagementServiceProbe(device: device, storage: storage)
        let model = model(probe, storage)
        await model.load(kind: .nodes)
        await probe.configure(state: .accepted)
        await model.perform(.nodeRemove, target: "Test", payload: .object(["id": .string("test-node"), "expected_definition_revision": .string("revision-a")]))
        await probe.configure(state: .delivered)
        await probe.rejectNodes()
        await model.checkReceipt()
        XCTAssertEqual(model.receipt?.state, .delivered)
        XCTAssertNil(model.pending)
        XCTAssertNil(model.nodes)
        XCTAssertTrue(model.error?.contains("The Mac applied the request") == true)
    }
    func testNodeDraftRejectsRemovedCapabilityAndRemovedProviderModel() throws {
        let draft = ComputeNodeDraft(id: "test-node", name: "Test", kind: "cloud", provider: "anthropic", model: "claude-test")
        let enabled = ManagedComputeNodes(nodes: [], providers: [.init(id: "anthropic", displayName: "Anthropic", models: ["claude-test"])], localModels: [], supportedKinds: ["cloud"], capabilities: ["node_create"])
        XCTAssertNoThrow(try draft.payload(in: enabled))
        let disabled = ManagedComputeNodes(nodes: [], providers: enabled.providers, localModels: [], supportedKinds: ["cloud"], capabilities: [])
        XCTAssertThrowsError(try draft.payload(in: disabled))
        let disappeared = ManagedComputeNodes(nodes: [], providers: [.init(id: "anthropic", displayName: "Anthropic", models: [])], localModels: [], supportedKinds: ["cloud"], capabilities: ["node_create"])
        XCTAssertThrowsError(try draft.payload(in: disappeared))
    }

}
