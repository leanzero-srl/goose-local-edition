import Foundation

struct ManagedModel: Decodable, Identifiable, Sendable {
    let id: String
    let sizeBytes: UInt64
    let complete: Bool
    let missingFiles: UInt32
}
struct ManagedModels: Decodable, Sendable {
    let models: [ManagedModel]
    let diskAvailableBytes: UInt64
    let diskTotalBytes: UInt64
}
struct ManagedDownload: Decodable, Sendable {
    let state: String
    let totalBytes: UInt64
    let downloadedBytes: UInt64
    let currentFile: String?
    let error: String?
}
struct ManagedProvider: Decodable, Identifiable, Sendable {
    struct Field: Decodable, Identifiable, Sendable {
        let key: String
        let required: Bool
        let secret: Bool
        let isSet: Bool
        let value: String?
        var id: String { key }
    }
    let providerId: String
    let displayName: String
    let configured: Bool
    let fields: [Field]
    var id: String { providerId }
}
struct ManagedComputeNode: Decodable, Identifiable, Sendable {
    let id: String
    let name: String
    let kind: String
    let model: String?
    let provider: String?
    let macs: [String]
    let editable: Bool
    let definitionRevision: String
}
struct ManagedNodeProvider: Decodable, Identifiable, Sendable {
    let id: String
    let displayName: String
    let models: [String]
}
struct ManagedComputeNodes: Decodable, Sendable {
    let nodes: [ManagedComputeNode]
    let providers: [ManagedNodeProvider]
    let localModels: [String]
    let supportedKinds: [String]
    let capabilities: [String]
    let localModelsError: String?
}
enum ManagementAction: String, Codable, Sendable {
    case download, downloadCancel = "download_cancel", downloadPause = "download_pause", downloadResume = "download_resume"
    case providerConfigure = "provider_configure", providerCreate = "provider_create"
    case nodeCreate = "node_create", nodeRemove = "node_remove"
}
struct ManagementCommand: Codable, Sendable {
    let commandId: UUID
    let ownerNodeId: UUID
    let expectedStoreGeneration: UUID
    let action: ManagementAction
    let payload: JSONValue
}
struct ManagementReceipt: Codable, Sendable {
    let commandId: UUID
    let ownerNodeId: UUID
    let authenticatedDeviceId: UUID
    let action: ManagementAction
    let state: ReceiptState
    let target: String
    let reason: String?
    let updatedAt: Date
}
struct PendingManagement: Codable, Sendable {
    let command: ManagementCommand
    let deviceId: UUID
    let target: String
}
struct ManagementContext: Sendable {
    let host: HostDescriptor
    let identity: CredentialEnvelope
    let generation: UUID
}
protocol OwnerManagementService: Sendable {
    func models(host: HostDescriptor, credential: String) async throws -> ManagedModels
    func download(repo: String, host: HostDescriptor, credential: String) async throws -> ManagedDownload
    func providers(host: HostDescriptor, credential: String) async throws -> [ManagedProvider]
    func nodes(host: HostDescriptor, credential: String) async throws -> ManagedComputeNodes
    func manage(_ command: ManagementCommand, host: HostDescriptor, credential: String) async throws -> ManagementReceipt
    func managementReceipt(id: UUID, host: HostDescriptor, credential: String) async throws -> ManagementReceipt
}
extension OwnerHTTPControllerService: OwnerManagementService {
    func models(host: HostDescriptor, credential: String) async throws -> ManagedModels {
        try await request("GET", path: "management/models", host: host, credential: credential, body: Optional<String>.none)
    }
    func download(repo: String, host: HostDescriptor, credential: String) async throws -> ManagedDownload {
        struct Response: Decodable { let progress: ManagedDownload }
        let response: Response = try await request("GET", path: "management/downloads", query: [.init(name: "repo_id", value: repo)], host: host, credential: credential, body: Optional<String>.none)
        return response.progress
    }
    func providers(host: HostDescriptor, credential: String) async throws -> [ManagedProvider] {
        struct Response: Decodable { let providers: [ManagedProvider] }
        let response: Response = try await request("GET", path: "management/providers", host: host, credential: credential, body: Optional<String>.none)
        guard response.providers.allSatisfy({ $0.fields.allSatisfy { !$0.secret || $0.value == nil } }) else { throw ControllerError.malformedResponse }
        return response.providers
    }
    func nodes(host: HostDescriptor, credential: String) async throws -> ManagedComputeNodes {
        try await request("GET", path: "management/nodes", host: host, credential: credential, body: Optional<String>.none)
    }
    func manage(_ command: ManagementCommand, host: HostDescriptor, credential: String) async throws -> ManagementReceipt {
        guard command.ownerNodeId == host.id else { throw ControllerError.invalid("This command belongs to another Mac.") }
        let response: ManagementReceipt = try await request("POST", path: "management/commands", host: host, credential: credential, body: command)
        guard response.commandId == command.commandId, response.ownerNodeId == host.id, response.action == command.action else { throw ControllerError.malformedResponse }
        return response
    }
    func managementReceipt(id: UUID, host: HostDescriptor, credential: String) async throws -> ManagementReceipt {
        let response: ManagementReceipt = try await request("GET", path: "management/commands/\(id.uuidString)", host: host, credential: credential, body: Optional<String>.none)
        guard response.commandId == id, response.ownerNodeId == host.id else { throw ControllerError.malformedResponse }
        return response
    }
}
