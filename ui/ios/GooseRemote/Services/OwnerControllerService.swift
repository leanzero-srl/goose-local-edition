import Foundation

struct OwnerCatalogSnapshot: Sendable {
    let entries: [CatalogEntry]
    let watermark: CatalogCursor
}

protocol OwnerControllerService: Sendable {
    func catalog(host: HostDescriptor, credential: String) async throws -> OwnerCatalogSnapshot
    func selected(work: CatalogEntry, host: HostDescriptor, credential: String) async throws -> SelectedWork
    func history(work: CatalogEntry, pageToken: String, host: HostDescriptor, credential: String) async throws -> HistoryPage
    func resources(host: HostDescriptor, credential: String) async throws -> ResourceSample
    func submit(_ command: WorkCommand, host: HostDescriptor, credential: String) async throws -> CommandReceipt
    func receipt(commandId: UUID, host: HostDescriptor, credential: String) async throws -> CommandReceipt
}

struct UnavailableOwnerControllerService: OwnerControllerService {
    private var error: ControllerError { .unavailable("The secure connection to this Mac is not available in this build. No command was sent.") }
    func catalog(host: HostDescriptor, credential: String) async throws -> OwnerCatalogSnapshot { throw error }
    func selected(work: CatalogEntry, host: HostDescriptor, credential: String) async throws -> SelectedWork { throw error }
    func history(work: CatalogEntry, pageToken: String, host: HostDescriptor, credential: String) async throws -> HistoryPage { throw error }
    func resources(host: HostDescriptor, credential: String) async throws -> ResourceSample { throw error }
    func submit(_ command: WorkCommand, host: HostDescriptor, credential: String) async throws -> CommandReceipt { throw error }
    func receipt(commandId: UUID, host: HostDescriptor, credential: String) async throws -> CommandReceipt { throw error }
}

struct PendingCommand: Codable, Identifiable, Sendable {
    let command: WorkCommand
    let createdAt: Date
    var receipt: CommandReceipt?
    var id: UUID { command.commandId }
}
struct LocalControllerData: Codable {
    var drafts: [String: String] = [:]
    var pendingCommands: [PendingCommand] = []
}

@MainActor
final class LocalControllerStore {
    private let url: URL
    private(set) var data: LocalControllerData
    init(directory: URL) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true,
                                               attributes: [.protectionKey: FileProtectionType.complete])
        var excluded = directory
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try excluded.setResourceValues(values)
        url = directory.appendingPathComponent("controller-state.json")
        if FileManager.default.fileExists(atPath: url.path) {
            data = try ControllerCodec.decoder().decode(LocalControllerData.self, from: Data(contentsOf: url))
        } else {
            data = LocalControllerData()
        }
    }
    func saveDraft(_ text: String, work: WorkRef) throws {
        var next = data
        next.drafts[work.id] = text.isEmpty ? nil : text
        try save(next)
    }
    func record(_ command: PendingCommand) throws {
        var next = data
        if let index = next.pendingCommands.firstIndex(where: { $0.id == command.id }) { next.pendingCommands[index] = command }
        else { next.pendingCommands.append(command) }
        try save(next)
    }
    func clear() throws { try save(LocalControllerData()) }
    private func save(_ next: LocalControllerData) throws {
        try ControllerCodec.encoder().encode(next).write(to: url, options: [.atomic, .completeFileProtection])
        data = next
    }
}
