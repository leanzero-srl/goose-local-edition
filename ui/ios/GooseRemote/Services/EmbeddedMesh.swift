import Foundation
import Network
#if canImport(TailscaleKit)
import TailscaleKit
#endif

struct MeshJoinCredential: Codable, Sendable {
    let enrollmentId: UUID
    let authKey: String
    let loginServer: URL
    let expirySeconds: UInt64
}

struct MeshEnrollmentState: Codable, Sendable {
    let authority: URL
    let deviceId: UUID
    let enrollmentId: UUID
    var join: MeshJoinCredential?
    var loginServer: URL?
    var confirmedNodeKey: String?
}

extension ControllerAPI {
    func meshJoin(enrollmentId: UUID, token: String) async throws -> MeshJoinCredential {
        struct Body: Encodable { let enrollmentId: UUID }
        let value: MeshJoinCredential = try await request("POST", path: "mesh/join-key", token: token, body: Body(enrollmentId: enrollmentId))
        guard value.enrollmentId == enrollmentId, !value.authKey.isEmpty, value.expirySeconds > 0 else { throw ControllerError.malformedResponse }
        _ = try ControllerCodec.authority(value.loginServer.absoluteString)
        return value
    }
    func confirmMesh(enrollmentId: UUID, nodeKey: String, token: String) async throws -> DeviceRecord {
        struct Body: Encodable { let enrollmentId: UUID; let meshNodeKey: String }
        return try await request("POST", path: "mesh/confirm", token: token, body: Body(enrollmentId: enrollmentId, meshNodeKey: nodeKey))
    }
}

protocol ControllerMeshSession: Sendable {
    func prepare(identity: CredentialEnvelope) async throws
    func suspend() async
    func removeLocalIdentity() async throws
}

protocol OwnerEventTransport: HTTPTransport {
    func events(for request: URLRequest) -> AsyncThrowingStream<Data, Error>
}

enum MeshAddress {
    static func contains(_ host: String?) -> Bool {
        guard let host, !host.contains("%") else { return false }
        let value = host.trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
        if let address = IPv4Address(value) {
            let bytes = [UInt8](address.rawValue)
            return bytes[0] == 100 && (64...127).contains(bytes[1])
        }
        if let address = IPv6Address(value) { return address.rawValue.prefix(6) == Data([0xfd, 0x7a, 0x11, 0x5c, 0xa1, 0xe0]) }
        return false
    }
}

actor EmbeddedMeshConnectionProvider: OwnerConnectionProvider, ControllerMeshSession {
    private let credentials: any CredentialStorage
    private let authorityTransport: any HTTPTransport
    private let directory: URL
    private var transports: [UUID: (MeshEndpoint, PinnedOwnerTransport)] = [:]
    private var lifecycle = UUID()
    #if canImport(TailscaleKit)
    private var node: TailscaleNode?
    #endif

    init(credentials: any CredentialStorage = KeychainStore(), authorityTransport: any HTTPTransport = SecureHTTPTransport(), directory: URL) {
        self.credentials = credentials
        self.authorityTransport = authorityTransport
        self.directory = directory
    }

    func prepare(identity: CredentialEnvelope) async throws {
        #if canImport(TailscaleKit)
        await suspend()
        let operation = lifecycle
        let deviceId = identity.credentials.device.deviceId
        var state = try credentials.read(MeshEnrollmentState.self, account: "mesh") ??
            MeshEnrollmentState(authority: identity.authority, deviceId: deviceId, enrollmentId: UUID())
        guard state.authority == identity.authority, state.deviceId == deviceId else {
            throw ControllerError.invalid("Remove the previous device’s local access before connecting another account.")
        }
        try credentials.write(state, account: "mesh")
        let api = try ControllerAPI(authority: identity.authority, transport: authorityTransport)
        if state.confirmedNodeKey == nil && state.join == nil {
            state.join = try await api.meshJoin(enrollmentId: state.enrollmentId, token: identity.credentials.accessToken)
            state.loginServer = state.join?.loginServer
            guard operation == lifecycle else { throw CancellationError() }
            try credentials.write(state, account: "mesh")
        }
        guard let loginServer = state.loginServer else { throw ControllerError.invalid("Mesh enrollment has no verified authority. Remove local access and enroll again.") }
        _ = try ControllerCodec.authority(loginServer.absoluteString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.protectionKey: FileProtectionType.complete])
        try FileManager.default.setAttributes([.protectionKey: FileProtectionType.complete], ofItemAtPath: directory.path)
        var excluded = directory
        var values = URLResourceValues(); values.isExcludedFromBackup = true
        try excluded.setResourceValues(values)
        let current = try TailscaleNode(config: Configuration(hostName: "goose-controller-" + deviceId.uuidString.lowercased(), path: directory.path,
            authKey: state.join?.authKey, controlURL: loginServer.absoluteString, ephemeral: false), logger: nil)
        node = current
        do {
            try await withTaskCancellationHandler {
                try await current.up()
            } onCancel: {
                // Cancelling detached up() alone leaves its Go call alive. Closing the node unblocks it.
                Task { try? await current.close() }
            }
            guard operation == lifecycle, !Task.isCancelled else { throw CancellationError() }
            let key = try Self.nodeKey(status: await current.statusJSON())
            if let confirmed = state.confirmedNodeKey, confirmed != key { throw ControllerError.invalid("The local mesh identity changed. Explicit reenrollment is required.") }
            let confirmed = try await api.confirmMesh(enrollmentId: state.enrollmentId, nodeKey: key, token: identity.credentials.accessToken)
            guard operation == lifecycle, confirmed.deviceId == deviceId, confirmed.role == "controller", confirmed.revokedAt == nil else { throw ControllerError.malformedResponse }
            state.confirmedNodeKey = key
            state.join = nil
            try credentials.write(state, account: "mesh")
        } catch {
            if operation == lifecycle { await suspend() }
            throw error
        }
        #else
        throw ControllerError.unavailable("This build is missing its embedded mesh framework. No owner connection was opened.")
        #endif
    }

    static func nodeKey(status: Data) throws -> String {
        struct Status: Decodable {
            struct Peer: Decodable { let PublicKey: String }
            let BackendState: String
            let selfPeer: Peer
            enum CodingKeys: String, CodingKey { case BackendState; case selfPeer = "Self" }
        }
        let value = try JSONDecoder().decode(Status.self, from: status)
        let key = value.selfPeer.PublicKey
        guard value.BackendState == "Running", key.hasPrefix("nodekey:"), key.count == 72,
              key.dropFirst(8).allSatisfy({ "0123456789abcdef".contains($0) }) else { throw ControllerError.invalid("The embedded mesh has not established its own node identity.") }
        return key
    }

    func connection(for host: HostDescriptor) async throws -> OwnerConnection {
        guard host.controllerAvailable, let endpoint = host.endpoints.first, MeshAddress.contains(endpoint.url.host) else { throw ControllerError.unavailable("This Mac has no verified mesh endpoint.") }
        #if canImport(TailscaleKit)
        guard let node else { throw ControllerError.unavailable("Connect this device’s embedded mesh before opening a Mac.") }
        if let cached = transports[host.id], cached.0 == endpoint { return .init(endpoint: endpoint, transport: cached.1) }
        if let cached = transports.removeValue(forKey: host.id) { cached.1.close() }
        let operation = lifecycle
        let configuration = URLSessionConfiguration.ephemeral
        try await MeshPeerReadiness.waitForOwner(endpoint: endpoint.url, timeout: configuration.timeoutIntervalForRequest) {
            try await node.statusJSON()
        }
        guard operation == lifecycle, !Task.isCancelled else { throw CancellationError() }
        try await configuration.proxyVia(node)
        guard operation == lifecycle, !Task.isCancelled else { throw CancellationError() }
        guard !configuration.proxyConfigurations.isEmpty else { throw ControllerError.unavailable("The embedded mesh proxy is unavailable.") }
        let transport = PinnedOwnerTransport(configuration: configuration, endpoint: endpoint.url,
            binding: .init(owner: host.id, pin: endpoint.tlsSpkiSha256))
        transports[host.id] = (endpoint, transport)
        return .init(endpoint: endpoint, transport: transport)
        #else
        throw ControllerError.unavailable("This build has no embedded mesh framework.")
        #endif
    }
    func suspend() async {
        lifecycle = UUID()
        transports.values.forEach { $0.1.close() }
        transports.removeAll()
        #if canImport(TailscaleKit)
        let current = node
        node = nil
        try? await current?.close()
        #endif
    }
    func removeLocalIdentity() async throws {
        await suspend()
        try credentials.remove(account: "mesh")
        if FileManager.default.fileExists(atPath: directory.path) { try FileManager.default.removeItem(at: directory) }
    }
}
