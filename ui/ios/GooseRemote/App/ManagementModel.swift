import Foundation
import Observation

@MainActor @Observable
final class ManagementModel {
    private(set) var models: ManagedModels?
    private(set) var providers: [ManagedProvider] = []
    private(set) var nodes: ManagedComputeNodes?
    private(set) var progress: ManagedDownload?
    private(set) var progressAt: Date?
    private(set) var trackedRepo = ""
    private(set) var pending: PendingManagement?
    private(set) var receipt: ManagementReceipt?
    private(set) var busy = false
    private(set) var receiptMissing = false
    private var refreshedNodeCommand: UUID?
    var canDismissRequest: Bool { receipt?.state == .outcomeUnknown || receiptMissing }
    var error: String?
    private let service: any OwnerManagementService
    private let storage: any CredentialStorage
    private let context: @MainActor () async throws -> ManagementContext
    private let isCurrent: @MainActor (UUID) -> Bool
    static let outboxAccount = "management-pending"

    init(service: any OwnerManagementService, storage: any CredentialStorage,
         context: @escaping @MainActor () async throws -> ManagementContext,
         isCurrent: @escaping @MainActor (UUID) -> Bool) {
        self.service = service; self.storage = storage; self.context = context; self.isCurrent = isCurrent
    }
    func load(kind: ManagementKind = .models) async {
        guard !busy else { return }
        busy = true; defer { busy = false }
        do {
            let current = try await context()
            let items = try storage.read([PendingManagement].self, account: Self.outboxAccount) ?? []
            pending = items.first { $0.deviceId == current.identity.credentials.device.id && $0.command.ownerNodeId == current.host.id }
            if kind == .nodes {
                let next = try await service.nodes(host: current.host, credential: current.identity.credentials.accessToken)
                guard isCurrent(current.identity.credentials.device.id) else { throw CancellationError() }
                nodes = next
            } else if kind == .providers {
                let next = try await service.providers(host: current.host, credential: current.identity.credentials.accessToken)
                guard isCurrent(current.identity.credentials.device.id) else { throw CancellationError() }
                providers = next
            } else {
                let next = try await service.models(host: current.host, credential: current.identity.credentials.accessToken)
                guard isCurrent(current.identity.credentials.device.id) else { throw CancellationError() }
                models = next
            }
            error = nil
        } catch {
            if kind == .nodes {
                nodes = nil
                if case ControllerError.remote(let status, _, _) = error, [404, 501].contains(status) {
                    self.error = "Update Goose Swarm on this Mac to manage compute nodes from your controller."
                    return
                }
            }
            self.error = error.localizedDescription
        }
    }
    func watch(repo: String) async {
        selectRepo(repo)
        await refreshProgress()
    }
    func refreshProgress() async {
        guard !trackedRepo.isEmpty else { return }
        let repo = trackedRepo
        do {
            let current = try await context()
            let next = try await service.download(repo: repo, host: current.host, credential: current.identity.credentials.accessToken)
            guard trackedRepo == repo, isCurrent(current.identity.credentials.device.id) else { return }
            progress = next; progressAt = Date(); error = nil
        } catch {
            guard trackedRepo == repo else { return }
            progress = nil; progressAt = nil
            self.error = error.localizedDescription
        }
    }
    private func selectRepo(_ repo: String) {
        let next = repo.trimmingCharacters(in: .whitespacesAndNewlines)
        if next != trackedRepo { progress = nil; progressAt = nil }
        trackedRepo = next
    }
    func perform(_ action: ManagementAction, target: String, payload: JSONValue) async {
        guard !busy else { return }
        busy = true; defer { busy = false }
        receipt = nil
        do {
            let current = try await context()
            var items = try storage.read([PendingManagement].self, account: Self.outboxAccount) ?? []
            guard !items.contains(where: { $0.deviceId == current.identity.credentials.device.id && $0.command.ownerNodeId == current.host.id }) else {
                throw ControllerError.invalid("Check the previous request before sending another command to this Mac.")
            }
            let command = ManagementCommand(commandId: UUID(), ownerNodeId: current.host.id, expectedStoreGeneration: current.generation, action: action, payload: payload)
            let next = PendingManagement(command: command, deviceId: current.identity.credentials.device.id, target: target)
            items.append(next)
            // Provider fields may contain secrets. The whole retry envelope stays device-bound in Keychain.
            try storage.write(items, account: Self.outboxAccount)
            pending = next; receipt = nil; receiptMissing = false
            try await send(next, current: current)
            if [.download, .downloadPause, .downloadResume, .downloadCancel].contains(action) { selectRepo(target) }
            error = nil
        } catch { self.error = error.localizedDescription }
    }
    func retry() async {
        guard !busy, let pending else { return }
        busy = true; defer { busy = false }
        do {
            let current = try await context()
            guard pending.deviceId == current.identity.credentials.device.id, pending.command.ownerNodeId == current.host.id else { throw ControllerError.invalid("Reconnect the original controller before retrying.") }
            let items = try storage.read([PendingManagement].self, account: Self.outboxAccount) ?? []
            guard items.contains(where: { $0.command.commandId == pending.command.commandId }) else { throw ControllerError.unavailable("The saved request is no longer available. Nothing was sent.") }
            try storage.write(items, account: Self.outboxAccount)
            try await send(pending, current: current)
            error = nil
        } catch { self.error = error.localizedDescription }
    }
    func checkReceipt() async {
        guard !busy, let pending else { return }
        busy = true; defer { busy = false }
        do {
            let current = try await context()
            guard pending.deviceId == current.identity.credentials.device.id, pending.command.ownerNodeId == current.host.id else { throw CancellationError() }
            let next = try await service.managementReceipt(id: pending.command.commandId, host: current.host, credential: current.identity.credentials.accessToken)
            try accept(next, pending: pending)
            try await refreshNodes(after: next, current: current)
            error = nil
        } catch {
            if case ControllerError.remote(404, "command_missing", _) = error, isCurrent(pending.deviceId) { receiptMissing = true }
            self.error = error.localizedDescription
        }
    }
    func dismissUnknownRequest() async {
        guard !busy, let pending, canDismissRequest else { return }
        busy = true; defer { busy = false }
        do {
            let current = try await context()
            guard isCurrent(pending.deviceId), pending.deviceId == current.identity.credentials.device.id,
                  pending.command.ownerNodeId == current.host.id else { throw CancellationError() }
            var items = try storage.read([PendingManagement].self, account: Self.outboxAccount) ?? []
            items.removeAll { $0.command.commandId == pending.command.commandId }
            try storage.write(items, account: Self.outboxAccount)
            self.pending = nil
            error = nil
        } catch { self.error = error.localizedDescription }
    }
    private func send(_ pending: PendingManagement, current: ManagementContext) async throws {
        guard isCurrent(pending.deviceId) else { throw CancellationError() }
        do {
            let next = try await service.manage(pending.command, host: current.host, credential: current.identity.credentials.accessToken)
            try accept(next, pending: pending)
            try await refreshNodes(after: next, current: current)
        } catch {
            // These typed validation failures happen before admission for this immutable payload.
            if case ControllerError.remote(400, let code, _) = error,
               ["invalid_repository", "invalid_request"].contains(code), isCurrent(pending.deviceId) {
                var items = try storage.read([PendingManagement].self, account: Self.outboxAccount) ?? []
                items.removeAll { $0.command.commandId == pending.command.commandId }
                try storage.write(items, account: Self.outboxAccount)
                self.pending = nil
            }
            throw error
        }
    }
    private func refreshNodes(after receipt: ManagementReceipt, current: ManagementContext) async throws {
        guard receipt.state == .delivered, [.nodeCreate, .nodeRemove].contains(receipt.action),
              refreshedNodeCommand != receipt.commandId else { return }
        do {
            let next = try await service.nodes(host: current.host, credential: current.identity.credentials.accessToken)
            guard isCurrent(current.identity.credentials.device.id) else { throw CancellationError() }
            nodes = next
            refreshedNodeCommand = receipt.commandId
        } catch {
            guard isCurrent(current.identity.credentials.device.id) else { throw CancellationError() }
            nodes = nil
            throw ControllerError.unavailable("The Mac applied the request, but its node list could not be refreshed. Return to Compute nodes and refresh. \(error.localizedDescription)")
        }
    }
    private func accept(_ next: ManagementReceipt, pending: PendingManagement) throws {
        guard isCurrent(pending.deviceId) else { throw CancellationError() }
        guard next.commandId == pending.command.commandId, next.ownerNodeId == pending.command.ownerNodeId,
              next.authenticatedDeviceId == pending.deviceId, next.action == pending.command.action else { throw ControllerError.malformedResponse }
        if next.state == .delivered || next.state == .rejected {
            var items = try storage.read([PendingManagement].self, account: Self.outboxAccount) ?? []
            items.removeAll { $0.command.commandId == next.commandId }
            try storage.write(items, account: Self.outboxAccount)
            self.pending = nil
        }
        receiptMissing = false
        receipt = next
    }
}
