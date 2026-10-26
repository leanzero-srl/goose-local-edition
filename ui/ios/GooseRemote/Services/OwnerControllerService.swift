import Foundation

struct OwnerCatalogSnapshot: Codable, Sendable {
    let entries: [CatalogEntry]
    let watermark: CatalogCursor
    var resumeCursor: String = ""
}

protocol OwnerControllerService: Sendable {
    func catalog(host: HostDescriptor, credential: String) async throws -> OwnerCatalogSnapshot
    func selected(work: CatalogEntry, host: HostDescriptor, credential: String) async throws -> SelectedWork
    func history(work: CatalogEntry, pageToken: String, host: HostDescriptor, credential: String) async throws -> HistoryPage
    func resources(host: HostDescriptor, credential: String) async throws -> ResourceSample
    func submit(_ command: WorkCommand, host: HostDescriptor, credential: String) async throws -> CommandReceipt
    func receipt(commandId: UUID, host: HostDescriptor, credential: String) async throws -> CommandReceipt
    func changes(cursor: String, host: HostDescriptor, credential: String) async throws -> CatalogChanges
    func catalogEvents(cursor: String, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<CatalogChange, Error>
    func workEvents(work: SelectedWork, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<WorkFrame, Error>
}

extension OwnerControllerService {
    func changes(cursor: String, host: HostDescriptor, credential: String) async throws -> CatalogChanges { throw ControllerError.unsupported("Catalog replay is unavailable.") }
    func catalogEvents(cursor: String, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<CatalogChange, Error> { throw ControllerError.unsupported("Live catalog is unavailable.") }
    func workEvents(work: SelectedWork, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<WorkFrame, Error> { throw ControllerError.unsupported("Live transcript is unavailable.") }
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
    var catalogs: [String: OwnerCatalogSnapshot] = [:]
    var selected: SelectedWork?
    var syncPending: [UUID] = []
    enum CodingKeys: String, CodingKey { case drafts, pendingCommands, catalogs, selected, syncPending }
    init() {}
    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        drafts = try values.decode([String: String].self, forKey: .drafts)
        pendingCommands = try values.decode([PendingCommand].self, forKey: .pendingCommands)
        catalogs = try values.decodeIfPresent([String: OwnerCatalogSnapshot].self, forKey: .catalogs) ?? [:]
        selected = try values.decodeIfPresent(SelectedWork.self, forKey: .selected)
        syncPending = try values.decodeIfPresent([UUID].self, forKey: .syncPending) ?? []
    }
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
    func cacheCatalog(_ snapshot: OwnerCatalogSnapshot) throws {
        var next = data
        next.catalogs[snapshot.watermark.ownerNodeId.uuidString] = snapshot
        try save(next)
    }
    func retainOwners(_ owners: Set<UUID>) throws {
        var next = data
        next.catalogs = next.catalogs.filter { owners.contains($0.value.watermark.ownerNodeId) }
        if let selected = next.selected, !owners.contains(selected.entry.work.ownerNodeId) { next.selected = nil }
        try save(next)
    }
    func removeOwner(_ owner: UUID) throws {
        var next = data
        next.catalogs[owner.uuidString] = nil
        if next.selected?.entry.work.ownerNodeId == owner { next.selected = nil }
        try save(next)
    }
    func cacheSelected(_ selected: SelectedWork?) throws { var next = data; next.selected = selected; try save(next) }
    func saveSyncPending(_ owners: [UUID]) throws { var next = data; next.syncPending = owners; try save(next) }
    private func save(_ next: LocalControllerData) throws {
        try ControllerCodec.encoder().encode(next).write(to: url, options: [.atomic, .completeFileProtection])
        data = next
    }
}
