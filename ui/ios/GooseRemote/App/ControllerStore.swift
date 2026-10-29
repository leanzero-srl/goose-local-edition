import Foundation
import Observation

struct NodeSyncResult: Identifiable {
    let id: UUID
    let name: String
    var state: State
    enum State { case waiting, syncing, complete(Int), unavailable(String), failed(String) }
}

enum ConnectionState: Equatable {
    case signedOut, loading, connected, partial, stale(String)
    var title: String {
        switch self { case .signedOut: "Not connected"; case .loading: "Connecting"; case .connected: "Synchronized"; case .partial: "Some Macs unavailable"; case .stale: "Connection needs attention" }
    }
}

@MainActor @Observable
final class ControllerStore {
    var authorityText = Bundle.main.object(forInfoDictionaryKey: "LeanZeroLinkURL") as? String ?? ""
    var email = ""
    var deviceName = ""
    var code = ""
    var errorMessage: String?
    var authBusy = false
    var deletionCode = ""
    private(set) var accountDeletion: SavedAccountDeletion?
    private(set) var accountDeletionCompleted = false
    private(set) var accountDeletionRetention: [String] = []
    static let deletionAccount = "account-deletion"
    private(set) var envelope: CredentialEnvelope?
    private(set) var enrollment: EnrollmentAttempt?
    private(set) var connection: ConnectionState = .signedOut
    private(set) var hosts: [HostDescriptor] = []
    private(set) var devices: [DeviceRecord] = []
    private(set) var entries: [CatalogEntry] = []
    private(set) var selectedWork: SelectedWork?
    private(set) var selectedError: String?
    private(set) var selectedLoading = false
    private(set) var selectedHistoryPaused = false
    private(set) var selectedHistoryNotice: String?
    private(set) var selectedHistoryReset = UUID()
    private(set) var syncResults: [NodeSyncResult] = []
    private(set) var lastSync: Date?
    private(set) var syncRunning = false
    private(set) var resources: [UUID: ResourceSample] = [:]
    private(set) var draftRevision = 0
    private(set) var pendingRevision = 0
    private(set) var sending = false
    private(set) var sharingRequest: SessionSharingRequest?
    var selectedRef: WorkRef?

    @ObservationIgnored private let credentials: any CredentialStorage
    @ObservationIgnored private let owner: any OwnerControllerService
    @ObservationIgnored private let local: LocalControllerStore?
    @ObservationIgnored private var localDirectory: URL?
    @ObservationIgnored private let transport: any HTTPTransport
    @ObservationIgnored private let mesh: (any ControllerMeshSession)?
    @ObservationIgnored private var syncTask: Task<Void, Never>?
    @ObservationIgnored private var syncRequest = UUID()
    @ObservationIgnored private var meshSuspension: Task<Void, Never>?
    @ObservationIgnored private var catalogTasks: [UUID: Task<Void, Never>] = [:]
    @ObservationIgnored private var workTask: Task<Void, Never>?
    @ObservationIgnored private var reconnectTask: Task<Void, Never>?
    @ObservationIgnored private var foreground = true
    @ObservationIgnored private var selectedRequest = UUID()
    @ObservationIgnored private var refreshTask: Task<CredentialEnvelope, Error>?

    @ObservationIgnored private var accessEpoch = UUID()
    @ObservationIgnored private var recoveryTask: Task<Void, Never>?
    @ObservationIgnored private var credentialSyncTask: Task<Void, Never>?
    @ObservationIgnored private var recoveryAttempted = false

    struct SavedEnrollment: Codable { let authority: URL; let attempt: EnrollmentAttempt }
    struct SavedRevoke: Codable { let device: DeviceRecord; let requestId: UUID }

    init(credentials: any CredentialStorage = KeychainStore(), owner: any OwnerControllerService = UnavailableOwnerControllerService(),
         transport: any HTTPTransport = SecureHTTPTransport(), directory: URL? = nil, mesh: (any ControllerMeshSession)? = nil) {
        self.credentials = credentials
        self.owner = owner
        self.transport = transport
        self.mesh = mesh
        do {
            let base = try directory ?? FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                .appendingPathComponent("GooseRemote", isDirectory: true)
            localDirectory = base
            local = try LocalControllerStore(directory: base)
        } catch {
            local = nil
            errorMessage = "Local drafts could not be opened. Sending is disabled until storage is available."
        }
        do {
            accountDeletion = try credentials.read(SavedAccountDeletion.self, account: Self.deletionAccount)
            envelope = try credentials.read(CredentialEnvelope.self, account: "device")
            if let envelope { authorityText = envelope.authority.absoluteString; deviceName = envelope.credentials.device.name }
            else if let saved = try credentials.read(SavedEnrollment.self, account: "enrollment") {
                enrollment = saved.attempt
                authorityText = saved.authority.absoluteString
                email = saved.attempt.email
                deviceName = saved.attempt.deviceName
            }
        } catch { errorMessage = error.localizedDescription }
        if authenticated, let local {
            entries = local.data.catalogs.values.flatMap(\.entries)
            selectedWork = local.data.selected
            selectedError = selectedWork == nil ? nil : "Saved view. Reconnect to verify this session with its owner."
            if !entries.isEmpty { connection = .stale("Showing saved session metadata until your Macs reconnect.") }
        }
    }

    var authenticated: Bool { envelope != nil && accountDeletion?.blocksAccountAccess != true }
    var canDeleteAccount: Bool {
        authenticated && envelope?.credentials.device.scopes.contains("devices.manage") == true && !authBusy && !syncRunning && !sending && refreshTask == nil
    }
    var canRequestCode: Bool {
        !authBusy && accountDeletion == nil && (try? ControllerCodec.authority(authorityText)) != nil && email.contains("@") && !deviceName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
    var canStoreDrafts: Bool { local != nil }
    var pendingCommands: [PendingCommand] { _ = pendingRevision; return local?.data.pendingCommands ?? [] }
    var unfinishedCommand: PendingCommand? {
        pendingCommands.last { $0.command.work == selectedRef && $0.receipt?.state != .delivered && $0.receipt?.state != .rejected }
    }
    func managementModel(for host: HostDescriptor) throws -> ManagementModel {
        guard let service = owner as? any OwnerManagementService else { throw ControllerError.unsupported("This version cannot manage remote Macs.") }
        return ManagementModel(service: service, storage: credentials, context: { [self] in
            guard let currentHost = hosts.first(where: { $0.id == host.id }), currentHost.controllerAvailable else {
                throw ControllerError.unavailable("Reconnect this Mac before managing it.")
            }
            let identity = try await validCredential()
            let snapshot: OwnerCatalogSnapshot
            if let saved = local?.data.catalogs[host.id.uuidString] { snapshot = saved }
            else { snapshot = try await owner.catalog(host: currentHost, credential: identity.credentials.accessToken) }
            guard authenticated, envelope?.credentials.device.id == identity.credentials.device.id, snapshot.watermark.ownerNodeId == host.id else { throw ControllerError.malformedResponse }
            return ManagementContext(host: currentHost, identity: identity, generation: snapshot.watermark.storeGeneration)
        }, isCurrent: { [weak self] device in self?.authenticated == true && self?.envelope?.credentials.device.id == device })
    }
    func ownerName(_ id: UUID) -> String { hosts.first { $0.id == id }?.name ?? "Unknown owner" }
    func draft(for work: WorkRef) -> String { _ = draftRevision; return local?.data.drafts[work.id] ?? "" }
    func saveDraft(_ text: String, for work: WorkRef) {
        do {
            guard let local else { throw ControllerError.unavailable("Draft storage is unavailable. Your message has not been saved.") }
            try local.saveDraft(text, work: work)
            draftRevision += 1
        } catch { errorMessage = error.localizedDescription }
    }

    func requestCode(platform: String) async {
        guard !authBusy, !authenticated, accountDeletion == nil else { return }
        authBusy = true
        defer { authBusy = false }
        do {
            let authority = try ControllerCodec.authority(authorityText)
            let name = deviceName.trimmingCharacters(in: .whitespacesAndNewlines)
            let email = email.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !name.isEmpty, email.contains("@") else { throw ControllerError.invalid("Enter your email and a name for this device.") }
            var attempt: EnrollmentAttempt
            if let existing = enrollment, existing.email == email, existing.deviceName == name,
               existing.challenge == nil || existing.challenge!.expiresAt > Date(),
               let saved = try credentials.read(SavedEnrollment.self, account: "enrollment"), saved.authority == authority {
                attempt = existing
            } else { attempt = .init(enrollmentId: UUID(), email: email, deviceName: name) }
            try credentials.write(SavedEnrollment(authority: authority, attempt: attempt), account: "enrollment")
            enrollment = attempt
            let api = try ControllerAPI(authority: authority, transport: transport)
            attempt.challenge = try await api.requestCode(attempt)
            try credentials.write(SavedEnrollment(authority: authority, attempt: attempt), account: "enrollment")
            enrollment = attempt
            errorMessage = nil
        } catch { errorMessage = error.localizedDescription }
    }

    func verify(platform: String) async {
        guard !authBusy, accountDeletion == nil, let enrollment else { return }
        authBusy = true
        defer { authBusy = false }
        do {
            guard let saved = try credentials.read(SavedEnrollment.self, account: "enrollment"), saved.attempt.enrollmentId == enrollment.enrollmentId else {
                throw ControllerError.invalid("Request a new sign-in code.")
            }
            let api = try ControllerAPI(authority: saved.authority, transport: transport)
            let received = try await api.verify(attempt: enrollment, code: code, platform: platform)
            let next = CredentialEnvelope(authority: saved.authority, credentials: received)
            // A newly verified device cannot inherit transcript caches from a removed Keychain.
            try local?.clear()
            try credentials.remove(account: ManagementModel.outboxAccount)
            // A failed secure write leaves the enrollment receipt retryable with the same code/id.
            try credentials.write(next, account: "device")
            envelope = next
            recoveryAttempted = false
            accountDeletionCompleted = false
            code = ""
            self.enrollment = nil
            try credentials.remove(account: "enrollment")
            errorMessage = nil
            manualSync()
        } catch { errorMessage = error.localizedDescription }
    }

    func validCredential(forceRefresh: Bool = false) async throws -> CredentialEnvelope {
        guard authenticated, let current = envelope else { throw ControllerError.unavailable("Connect this device to Link first.") }
        if let refreshTask { return try await refreshTask.value }
        if !forceRefresh, current.refreshRequestId == nil && current.credentials.expiresAt.timeIntervalSince1970 > ceil(Date().timeIntervalSince1970) { return current }
        let task = Task { @MainActor [self] in
            var pending = current
            if pending.refreshRequestId == nil {
                pending.refreshRequestId = UUID()
                try credentials.write(pending, account: "device")
                envelope = pending
            }
            let api = try ControllerAPI(authority: pending.authority, transport: transport)
            let rotated = try await api.refresh(pending)
            guard authenticated, envelope?.credentials.device.id == pending.credentials.device.id else { throw CancellationError() }
            let next = CredentialEnvelope(authority: pending.authority, credentials: rotated)
            try credentials.write(next, account: "device")
            envelope = next
            accessEpoch = UUID()
            stopStreams()
            // A replacement token is not proof that its scopes still authorize cached content.
            try clearAuthorizedViews()
            queueCredentialSync()
            return next
        }
        refreshTask = task
        let epoch = accessEpoch
        defer { refreshTask = nil }
        do { return try await task.value }
        catch {
            if Self.isAccessFailure(error), epoch == accessEpoch {
                accessEpoch = UUID()
                stopStreams()
                connection = .stale(error.localizedDescription)
                selectedError = error.localizedDescription
                errorMessage = error.localizedDescription
                try clearAuthorizedViews()
            }
            throw error
        }
    }

    func manualSync() {
        guard foreground, syncTask == nil, authenticated else { return }
        stopStreams()
        syncRunning = true
        connection = .loading
        let request = UUID()
        syncRequest = request
        MeshLifecycleDiagnostics.record("sync-begin", operation: request)
        syncTask = Task { @MainActor [self] in
            defer {
                MeshLifecycleDiagnostics.record("sync-end", operation: request)
                if syncRequest == request { syncTask = nil; syncRunning = false }
            }
            var epoch = accessEpoch
            do {
                MeshLifecycleDiagnostics.record("sync-wait-shutdown", operation: request)
                await meshSuspension?.value
                try Task.checkCancellation()
                guard foreground, syncRequest == request else { return }
                let identity = try await validCredential()
                try Task.checkCancellation()
                guard foreground, syncRequest == request else { return }
                epoch = accessEpoch
                MeshLifecycleDiagnostics.record("sync-prepare-begin", operation: request)
                try await mesh?.prepare(identity: identity)
                MeshLifecycleDiagnostics.record("sync-prepare-end", operation: request)
                try Task.checkCancellation()
                guard foreground, syncRequest == request else { return }
                let api = try ControllerAPI(authority: identity.authority, transport: transport)
                MeshLifecycleDiagnostics.record("sync-hosts-begin", operation: request)
                let page = try await api.hosts(token: identity.credentials.accessToken)
                guard !Task.isCancelled, epoch == accessEpoch else { throw CancellationError() }
                MeshLifecycleDiagnostics.record("sync-hosts-end", operation: request)
                guard Set(page.hosts.map(\.id)).count == page.hosts.count else { throw ControllerError.malformedResponse }
                hosts = page.hosts
                try local?.retainOwners(Set(page.hosts.map(\.id)))
                try local?.saveSyncPending(page.hosts.map(\.id))
                syncResults = page.hosts.map { .init(id: $0.id, name: $0.name, state: .waiting) }
                entries.removeAll { entry in !page.hosts.contains { $0.id == entry.work.ownerNodeId } }
                for host in page.hosts {
                    guard let index = syncResults.firstIndex(where: { $0.id == host.id }) else { continue }
                    guard host.controllerAvailable else {
                        syncResults[index].state = .unavailable(host.statusLabel)
                        continue
                    }
                    syncResults[index].state = .syncing
                    do {
                        let result = try await synchronizedCatalog(host: host, credential: identity.credentials.accessToken, epoch: epoch)
                        guard !Task.isCancelled, epoch == accessEpoch else { return }
                        guard result.watermark.ownerNodeId == host.id,
                              result.entries.allSatisfy({ $0.work.ownerNodeId == host.id && $0.storeGeneration == result.watermark.storeGeneration }) else {
                            throw ControllerError.malformedResponse
                        }
                        entries.removeAll { $0.work.ownerNodeId == host.id }
                        entries.append(contentsOf: result.entries)
                        try local?.cacheCatalog(result)
                        try local?.saveSyncPending((local?.data.syncPending ?? []).filter { $0 != host.id })
                        syncResults[index].state = .complete(result.entries.count)
                        startCatalogStream(host: host)
                    } catch {
                        guard !Task.isCancelled, epoch == accessEpoch else { return }
                        syncResults[index].state = .failed(error.localizedDescription)
                        if Self.isAccessFailure(error) { streamFailed(error, epoch: epoch); return }
                    }
                }
                lastSync = Date()
                connection = syncResults.contains { result in
                    if case .complete = result.state { return false }
                    return true
                } ? .partial : .connected
                errorMessage = nil
                if let selectedRef { await select(selectedRef) }
            } catch {
                guard !Task.isCancelled, epoch == accessEpoch else { return }
                if Self.isAccessFailure(error) { streamFailed(error, epoch: epoch); return }
                connection = .stale(error.localizedDescription)
                errorMessage = error.localizedDescription
            }
        }
    }
    func synchronize() async { manualSync(); await syncTask?.value }

    func select(_ work: WorkRef?, resetHistory: Bool = false) async {
        let previous = work != nil && selectedRef == work ? selectedWork : nil
        sharingRequest = nil
        workTask?.cancel(); workTask = nil
        selectedRequest = UUID()
        selectedLoading = false
        selectedRef = work
        selectedHistoryPaused = false
        selectedWork = nil
        if previous == nil { selectedHistoryNotice = nil }
        selectedError = nil
        guard let work, let entry = entries.first(where: { $0.work == work }), let host = hosts.first(where: { $0.id == work.ownerNodeId }) else { return }
        selectedWork = previous
        if selectedWork == nil, let saved = local?.data.selected, saved.entry.work == work { selectedWork = saved }
        let request = UUID()
        selectedRequest = request
        selectedLoading = true
        defer { if selectedRequest == request { selectedLoading = false } }
        do {
            let identity = try await validCredential()
            let epoch = accessEpoch
            let result: SelectedWork
            if let previous, work.kind == .chat, previous.entry.storeGeneration == entry.storeGeneration,
               !previous.workCursor.isEmpty, !resetHistory {
                result = try await owner.refreshSelected(previous, host: host, credential: identity.credentials.accessToken)
            } else {
                result = try await owner.selected(work: entry, host: host, credential: identity.credentials.accessToken)
            }
            guard epoch == accessEpoch, !Task.isCancelled else { return }
            guard result.entry.work == work, result.entry.storeGeneration == entry.storeGeneration else { throw ControllerError.malformedResponse }
            if selectedRequest == request {
                try local?.cacheSelected(result)
                selectedWork = result
                if resetHistory || previous.map({ $0.entry.storeGeneration != result.entry.storeGeneration }) == true {
                    selectedHistoryNotice = "The owner’s saved history changed or expired. Showing the latest messages; older history can be loaded again."
                    selectedHistoryReset = UUID()
                }
                startWorkStream(host: host, request: request)
            }
        } catch {
            if selectedRequest == request {
                if Self.isAccessFailure(error) { streamFailed(error, epoch: accessEpoch) }
                else { selectedError = error.localizedDescription }
            }
        }
    }

    func send(action: WorkAction) async {
        guard selectedError == nil, let work = selectedWork else { return }
        do {
            let command = try WorkCommand.make(entry: work.entry, action: action, text: draft(for: work.entry.work))
            await requestDelivery(command)
        } catch { errorMessage = error.localizedDescription }
    }

    func answer(input: PendingInput, option: InputOption, value: String?) async {
        guard selectedError == nil, let current = selectedWork, current.pendingInputs.contains(input) else {
            errorMessage = "This input is no longer current. Refresh the session."
            return
        }
        do {
            let command = try WorkCommand.answer(entry: current.entry, input: input, option: option, value: value)
            await requestDelivery(command, input: input)
        } catch { errorMessage = error.localizedDescription }
    }

    private func requestDelivery(_ command: WorkCommand, input: PendingInput? = nil) async {
        guard !sending, unfinishedCommand == nil else { return }
        guard [.send, .steer, .note, .answerInput].contains(command.action) else {
            sharingRequest = nil
            await deliver(command)
            return
        }
        guard sharingRequest == nil, authenticated, let identity = envelope,
              let host = hosts.first(where: { $0.id == command.work.ownerNodeId }) else { return }
        sharingRequest = .init(command: command, authority: identity.authority,
                               deviceId: identity.credentials.device.id, ownerName: host.name, input: input, sharing: selectedWork?.sharing)
    }

    func declineSharing() { sharingRequest = nil }

    func confirmSharing(_ id: UUID) async {
        guard let request = sharingRequest, request.id == id else { return }
        sharingRequest = nil
        do {
            _ = try confirmedSharingCommand(request)
            await deliver(request.command, sharing: request)
        } catch { errorMessage = error.localizedDescription }
    }

    private func confirmedSharingCommand(_ request: SessionSharingRequest) throws -> WorkCommand {
        guard authenticated, let identity = envelope, request.matches(identity),
              selectedError == nil, selectedRef == request.command.work,
              let current = selectedWork, current.entry.work == request.command.work,
              current.entry.storeGeneration == request.command.expectedStoreGeneration,
              current.entry.revision >= request.command.expectedRevision,
              current.entry.currentRunId == request.command.expectedRunId,
              current.entry.canPerform(request.command.action),
              request.input.map({ current.pendingInputs.contains($0) }) ?? true else {
            throw ControllerError.invalid("The account or session changed. Nothing was sent. Review the session and try again.")
        }
        // Stream progress changes the concurrency revision, not the instruction the user approved.
        return .init(commandId: request.command.commandId, work: request.command.work,
                     expectedStoreGeneration: request.command.expectedStoreGeneration, expectedRevision: current.entry.revision,
                     expectedRunId: request.command.expectedRunId, action: request.command.action, payload: request.command.payload)
    }

    private func deliver(_ command: WorkCommand, sharing: SessionSharingRequest? = nil) async {
        guard !sending, unfinishedCommand == nil, let host = hosts.first(where: { $0.id == command.work.ownerNodeId }) else { return }
        sending = true
        defer { sending = false }
        do {
            guard let local else { throw ControllerError.unavailable("Command recovery storage is unavailable. Nothing was sent.") }
            let identity = try await validCredential()
            let command = try sharing.map(confirmedSharingCommand) ?? command
            var pending = PendingCommand(command: command, createdAt: Date())
            try local.record(pending)
            pendingRevision += 1
            let receipt: CommandReceipt
            do {
                receipt = try await owner.submit(command, host: host, credential: identity.credentials.accessToken)
            } catch ControllerError.remote(409, "stale_revision", _) {
                // The owner checks this before accepting/journaling a new command, so no receipt exists.
                try local.removeUnacceptedCommand(command.commandId)
                pendingRevision += 1
                if selectedRef == command.work { await select(command.work) }
                throw ControllerError.invalid("The owner did not accept this action because the session changed. Review the refreshed session and try again. Your draft has been kept.")
            }
            guard receipt.commandId == command.commandId, receipt.work == command.work else { throw ControllerError.malformedResponse }
            pending.receipt = receipt
            try local.record(pending)
            pendingRevision += 1
            if receipt.state == .delivered { clearDeliveredDraft(pending) }
            errorMessage = Self.rejectionMessage(receipt)
            if receipt.state == .delivered, selectedRef == command.work { await select(command.work) }
        } catch { errorMessage = error.localizedDescription }
    }

    func checkDelivery() async {
        guard !sending, var pending = unfinishedCommand, let host = hosts.first(where: { $0.id == pending.command.work.ownerNodeId }), let local else { return }
        sending = true
        defer { sending = false }
        do {
            let identity = try await validCredential()
            let receipt = try await owner.receipt(commandId: pending.id, host: host, credential: identity.credentials.accessToken)
            guard receipt.commandId == pending.id, receipt.work == pending.command.work else { throw ControllerError.malformedResponse }
            pending.receipt = receipt
            try local.record(pending)
            pendingRevision += 1
            if receipt.state == .delivered { clearDeliveredDraft(pending) }
            errorMessage = Self.rejectionMessage(receipt)
        } catch { errorMessage = error.localizedDescription }
    }
    private static func rejectionMessage(_ receipt: CommandReceipt) -> String? {
        guard receipt.state == .rejected else { return nil }
        return "The owner rejected this action. \(receipt.reason ?? "Review the session before trying again.")"
    }

    private func clearDeliveredDraft(_ pending: PendingCommand) {
        let sent = pending.command.payload.content?.first?.text ?? pending.command.payload.text
        if let sent, draft(for: pending.command.work) == sent { saveDraft("", for: pending.command.work) }
    }

    func loadResources(_ host: HostDescriptor) async {
        var epoch = accessEpoch
        do {
            let identity = try await validCredential()
            epoch = accessEpoch
            let sample = try await owner.resources(host: host, credential: identity.credentials.accessToken)
            guard authenticated, envelope?.credentials.device.id == identity.credentials.device.id, identity.credentials.accessToken == envelope?.credentials.accessToken else { return }
            guard epoch == accessEpoch else { return }
            resources[host.id] = sample
        } catch {
            guard epoch == accessEpoch else { return }
            if Self.isAccessFailure(error) { streamFailed(error, epoch: epoch) }
            else { errorMessage = error.localizedDescription }
        }
    }
    func loadMoreHistory() async {
        guard !selectedLoading, let current = selectedWork, let token = current.nextPageToken,
              let host = hosts.first(where: { $0.id == current.entry.work.ownerNodeId }) else { return }
        let request = selectedRequest
        let wasPaused = selectedHistoryPaused
        if current.entry.work.kind != .chat { selectedHistoryPaused = true }
        selectedLoading = true
        defer { if selectedRequest == request { selectedLoading = false } }
        do {
            let identity = try await validCredential()
            let epoch = accessEpoch
            let page = try await owner.history(work: current.entry, pageToken: token, host: host, credential: identity.credentials.accessToken)
            guard epoch == accessEpoch else { return }
            guard page.work == current.entry.work, page.storeGeneration == current.entry.storeGeneration, page.revision == current.historyRevision,
                  page.nextPageToken != token else { throw ControllerError.malformedResponse }
            if selectedRequest == request, let current = selectedWork {
                let latest = try OwnerReplay.history(page, into: current)
                try local?.cacheSelected(latest)
                selectedWork = latest
            }
        } catch {
            guard selectedRequest == request else { return }
            selectedHistoryPaused = wasPaused
            if Self.needsFreshHistory(error) { await select(current.entry.work, resetHistory: true) }
            else {
                if Self.isAccessFailure(error) { streamFailed(error, epoch: accessEpoch) }
                else { errorMessage = error.localizedDescription }
            }
        }
    }

    func loadDevices() async {
        do {
            let identity = try await validCredential()
            let epoch = accessEpoch
            let page = try await ControllerAPI(authority: identity.authority, transport: transport).devices(token: identity.credentials.accessToken)
            guard authenticated, envelope?.credentials.device.id == identity.credentials.device.id, identity.credentials.accessToken == envelope?.credentials.accessToken else { return }
            guard epoch == accessEpoch else { return }
            devices = page.devices
        } catch { errorMessage = error.localizedDescription }
    }
    func revoke(_ device: DeviceRecord) async {
        guard !authBusy else { return }
        authBusy = true
        defer { authBusy = false }
        do {
            let identity = try await validCredential()
            let key = "revoke-\(device.id.uuidString)"
            let saved = try credentials.read(SavedRevoke.self, account: key) ?? SavedRevoke(device: device, requestId: UUID())
            try credentials.write(saved, account: key)
            let receipt = try await ControllerAPI(authority: identity.authority, transport: transport).revoke(device: saved.device, requestId: saved.requestId, token: identity.credentials.accessToken)
            guard receipt.deviceId == device.id else { throw ControllerError.malformedResponse }
            try credentials.remove(account: key)
            if device.id == identity.credentials.device.id { try await forgetDevice() }
            else { await loadDevices() }
        } catch { errorMessage = error.localizedDescription }
    }
    func forgetDevice() async throws {
        guard !syncRunning, refreshTask == nil, !sending else { throw ControllerError.invalid("Wait for the current request to finish before removing local access.") }
        if accountDeletion?.canCancel == true { try cancelAccountDeletion() }
        try await removeLocalAccess()
    }
    private func removeLocalAccess() async throws {
        sharingRequest = nil
        stopStreams()
        selectedRequest = UUID()
        accessEpoch = UUID()
        recoveryTask?.cancel(); recoveryTask = nil
        credentialSyncTask?.cancel(); credentialSyncTask = nil
        envelope = nil
        enrollment = nil
        code = ""; email = ""
        hosts = []; entries = []; devices = []; resources = [:]; syncResults = []
        selectedWork = nil; selectedRef = nil; selectedError = nil; lastSync = nil
        draftRevision += 1; pendingRevision += 1
        connection = .signedOut
        var failure: Error?
        for account in ["device", "enrollment", "mesh", ManagementModel.outboxAccount] {
            do { try credentials.remove(account: account) }
            catch { if failure == nil { failure = error } }
        }
        do { try credentials.removeRevocationRequests() }
        catch { if failure == nil { failure = error } }
        do {
            if let local { try local.clear() }
            else if let localDirectory {
                let state = localDirectory.appendingPathComponent("controller-state.json")
                if FileManager.default.fileExists(atPath: state.path) { try FileManager.default.removeItem(at: state) }
            } else { throw ControllerError.unavailable("Local storage is unavailable. Unlock this device and retry cleanup.") }
        } catch { if failure == nil { failure = error } }
        await meshSuspension?.value
        do { try await mesh?.removeLocalIdentity() }
        catch { if failure == nil { failure = error } }
        if let failure { throw failure }
    }

    func requestAccountDeletionCode() async {
        guard canDeleteAccount, accountDeletion?.blocksAccountAccess != true else { return }
        authBusy = true
        defer { authBusy = false }
        do {
            let identity = try await validCredential()
            let saved: SavedAccountDeletion
            if let previous = accountDeletion, previous.phase == .requestingCode,
               previous.authority == identity.authority, previous.deviceId == identity.credentials.device.id {
                saved = previous
            } else {
                saved = .init(authority: identity.authority, requestId: UUID(), deviceId: identity.credentials.device.id, phase: .requestingCode)
            }
            try saveDeletion(saved)
            accountDeletionCompleted = false
            let api = try ControllerAPI(authority: saved.authority, transport: transport)
            var next = saved
            next.challenge = try await api.requestAccountDeletionCode(requestId: saved.requestId, token: identity.credentials.accessToken)
            next.phase = .awaitingCode
            try saveDeletion(next)
            deletionCode = ""
            errorMessage = nil
        } catch {
            if case ControllerError.remote(_, let code, _) = error,
               ["mail_failed", "deletion_challenge_expired", "deletion_challenge_changed"].contains(code) {
                do { try cancelAccountDeletion() } catch { errorMessage = error.localizedDescription; return }
            }
            errorMessage = error.localizedDescription
        }
    }

    func cancelAccountDeletion() throws {
        guard accountDeletion?.canCancel == true else { return }
        try credentials.remove(account: Self.deletionAccount)
        accountDeletion = nil
        deletionCode = ""
    }

    func confirmAccountDeletion() async {
        guard canDeleteAccount, var saved = accountDeletion, saved.phase == .awaitingCode else { return }
        let code = deletionCode.trimmingCharacters(in: .whitespacesAndNewlines)
        guard code.count == 6, code.utf8.allSatisfy({ (48...57).contains($0) }) else {
            errorMessage = "Enter the six-digit deletion code from your email."
            return
        }
        authBusy = true
        defer { authBusy = false }
        do {
            // An interrupted confirmation must recover by receipt before normal access resumes.
            saved.phase = .confirming
            try saveDeletion(saved)
            stopStreams()
            selectedRequest = UUID()
            let api = try ControllerAPI(authority: saved.authority, transport: transport)
            let receipt = try await api.confirmAccountDeletion(saved, code: code)
            try await acceptDeletion(receipt, saved: saved)
            errorMessage = nil
        } catch {
            if case ControllerError.remote(401, "invalid_code", _) = error {
                saved.phase = .awaitingCode
                do { try saveDeletion(saved) } catch { errorMessage = error.localizedDescription; return }
            }
            errorMessage = error.localizedDescription
        }
    }

    func checkAccountDeletion() async {
        guard !authBusy, let saved = accountDeletion, saved.blocksAccountAccess else { return }
        authBusy = true
        defer { authBusy = false }
        do {
            let receipt = try await ControllerAPI(authority: saved.authority, transport: transport).accountDeletionStatus(saved)
            try await acceptDeletion(receipt, saved: saved)
            errorMessage = nil
        } catch {
            if case ControllerError.remote(409, "deletion_not_confirmed", _) = error, saved.phase == .confirming {
                var next = saved
                next.phase = .awaitingCode
                do { try saveDeletion(next) } catch { errorMessage = error.localizedDescription; return }
            }
            errorMessage = error.localizedDescription
        }
    }

    private func saveDeletion(_ saved: SavedAccountDeletion) throws {
        try credentials.write(saved, account: Self.deletionAccount)
        accountDeletion = saved
    }
    private func acceptDeletion(_ receipt: AccountDeletionReceipt, saved: SavedAccountDeletion) async throws {
        var next = saved
        next.phase = receipt.status == .deleted ? .deleted : .pending
        next.receipt = receipt
        // Keep the recovery receipt until every local cleanup step has succeeded.
        try saveDeletion(next)
        deletionCode = ""
        try await removeLocalAccess()
        if receipt.status == .deleted {
            try credentials.remove(account: Self.deletionAccount)
            accountDeletion = nil
            accountDeletionCompleted = true
            accountDeletionRetention = receipt.retentionSummary
        }
    }

    private func synchronizedCatalog(host: HostDescriptor, credential: String, epoch: UUID) async throws -> OwnerCatalogSnapshot {
        guard var snapshot = local?.data.catalogs[host.id.uuidString], !snapshot.resumeCursor.isEmpty else {
            return try await owner.catalog(host: host, credential: credential)
        }
        do {
            while true {
                let changes = try await owner.changes(cursor: snapshot.resumeCursor, host: host, credential: credential)
                guard epoch == accessEpoch, !Task.isCancelled else { throw CancellationError() }
                guard changes.cursor.ownerNodeId == host.id, changes.cursor.storeGeneration == snapshot.watermark.storeGeneration,
                      changes.cursor.revision >= snapshot.watermark.revision, !changes.resumeCursor.isEmpty else { throw ControllerError.malformedResponse }
                let previous = snapshot.watermark.revision
                for change in changes.changes { snapshot = try OwnerReplay.catalog(change, into: snapshot) }
                guard changes.cursor.revision >= snapshot.watermark.revision,
                      !changes.hasMore || changes.cursor.revision > previous else { throw ControllerError.malformedResponse }
                snapshot = .init(entries: snapshot.entries, watermark: changes.cursor, resumeCursor: changes.resumeCursor)
                try local?.cacheCatalog(snapshot)
                if !changes.hasMore { return snapshot }
            }
        } catch ControllerError.remote(410, _, _) {
            return try await owner.catalog(host: host, credential: credential)
        }
    }

    private func startCatalogStream(host: HostDescriptor) {
        guard foreground, let cached = local?.data.catalogs[host.id.uuidString] else { return }
        catalogTasks[host.id]?.cancel()
        catalogTasks[host.id] = Task { @MainActor [weak self] in
            guard let self else { return }
            var epoch = accessEpoch
            do {
                let identity = try await validCredential()
                epoch = accessEpoch
                guard !Task.isCancelled else { return }
                let events = try await owner.catalogEvents(cursor: cached.resumeCursor, host: host, credential: identity.credentials.accessToken)
                for try await change in events {
                    guard !Task.isCancelled, epoch == accessEpoch, let current = local?.data.catalogs[host.id.uuidString] else { return }
                    let next = try OwnerReplay.catalog(change, into: current)
                    try local?.cacheCatalog(next)
                    entries.removeAll { $0.work.ownerNodeId == host.id }
                    entries.append(contentsOf: next.entries)
                    if change.type == "tombstone", selectedRef == change.work {
                        workTask?.cancel(); selectedWork = nil
                        try local?.cacheSelected(nil)
                        selectedError = "This session is no longer visible on its owner."
                    }
                }
                if !Task.isCancelled, epoch == accessEpoch { scheduleReconnect("The live catalog disconnected.") }
            } catch {
                if !Task.isCancelled { streamFailed(error, epoch: epoch) }
            }
        }
    }

    private func refreshSelectedLog(host: HostDescriptor, request: UUID) async throws {
        guard !selectedHistoryPaused, let selected = selectedWork else { return }
        let identity = try await validCredential()
        let epoch = accessEpoch
        let latest = try await owner.selected(work: selected.entry, host: host, credential: identity.credentials.accessToken)
        guard selectedRequest == request, epoch == accessEpoch, !Task.isCancelled, !selectedHistoryPaused else { return }
        guard latest.entry.work == selected.entry.work, latest.entry.storeGeneration == selected.entry.storeGeneration,
              latest.streamRevision >= selected.streamRevision else { throw ControllerError.malformedResponse }
        try local?.cacheSelected(latest)
        selectedWork = latest
    }

    private func startWorkStream(host: HostDescriptor, request: UUID) {
        guard foreground, let current = selectedWork else { return }
        workTask = Task { @MainActor [weak self] in
            guard let self else { return }
            var epoch = accessEpoch
            do {
                let identity = try await validCredential()
                epoch = accessEpoch
                guard !Task.isCancelled, selectedRequest == request else { return }
                let events = try await owner.workEvents(work: current, host: host, credential: identity.credentials.accessToken)
                for try await frame in events {
                    guard !Task.isCancelled, epoch == accessEpoch, selectedRequest == request, let selected = selectedWork else { return }
                    let next = try OwnerReplay.work(frame, into: selected)
                    try local?.cacheSelected(next)
                    selectedWork = next
                    if frame.type == "log_updated", next.entry.work.kind != .chat, !selectedHistoryPaused {
                        try await refreshSelectedLog(host: host, request: request)
                    }
                    if frame.type == "pending_input", frame.sequence > selected.entry.revision {
                        Task { @MainActor [weak self] in
                            guard let self, selectedRequest == request, selectedRef == frame.work, foreground else { return }
                            await select(frame.work)
                        }
                        return
                    }
                    if frame.type == "command_receipt", unfinishedCommand != nil { await checkDelivery() }
                }
                if !Task.isCancelled, epoch == accessEpoch { scheduleReconnect("The live session disconnected.") }
            } catch {
                if !Task.isCancelled, epoch == accessEpoch, selectedRequest == request {
                    if Self.needsFreshHistory(error) {
                        Task { @MainActor [weak self] in
                            guard let self, selectedRequest == request, selectedRef == current.entry.work else { return }
                            await select(current.entry.work, resetHistory: true)
                        }
                        return
                    }
                    if !Self.isAccessFailure(error) { selectedError = error.localizedDescription }
                    streamFailed(error, epoch: epoch)
                }
            }
        }
    }

    private static func needsFreshHistory(_ error: Error) -> Bool {
        if case ControllerError.replayRequired = error { return true }
        if case ControllerError.remote(410, "snapshot_required", _) = error { return true }
        return false
    }

    private static func isAccessFailure(_ error: Error) -> Bool {
        if case ControllerError.remote(let status, _, _) = error { return status == 401 || status == 403 }
        return false
    }

    private func clearAuthorizedViews() throws {
        selectedRequest = UUID()
        entries = []; resources = [:]; devices = []
        selectedWork = nil; selectedLoading = false; sharingRequest = nil
        // Keep only navigation intent; a fresh owner read must supply every visible field.
        try local?.retainOwners([])
    }

    private func queueCredentialSync() {
        guard foreground, authenticated, credentialSyncTask == nil, !hosts.isEmpty || syncTask != nil else { return }
        let pendingSync = syncTask
        credentialSyncTask = Task { @MainActor [weak self] in
            await pendingSync?.value
            guard let self, !Task.isCancelled else { return }
            _ = try? await refreshTask?.value
            guard !Task.isCancelled else { return }
            credentialSyncTask = nil
            guard !Task.isCancelled, foreground, authenticated else { return }
            manualSync()
        }
    }

    private func streamFailed(_ error: Error, epoch: UUID) {
        guard epoch == accessEpoch, authenticated else { return }
        guard Self.isAccessFailure(error) else { scheduleReconnect(error.localizedDescription); return }
        let mayRecover = !recoveryAttempted || (envelope?.credentials.expiresAt.timeIntervalSince1970 ?? .infinity) <= ceil(Date().timeIntervalSince1970)
        let recovering = foreground && mayRecover && recoveryTask == nil
        accessEpoch = UUID()
        stopStreams()
        connection = recovering ? .loading : .stale(error.localizedDescription)
        selectedError = recovering ? nil : error.localizedDescription
        errorMessage = recovering ? nil : error.localizedDescription
        do { try clearAuthorizedViews() }
        catch {
            connection = .stale(error.localizedDescription)
            selectedError = error.localizedDescription
            errorMessage = error.localizedDescription
            return
        }
        guard recovering else { return }
        selectedLoading = selectedRef != nil
        recoveryAttempted = true
        let deniedEpoch = accessEpoch
        recoveryTask = Task { @MainActor [weak self] in
            guard let self else { return }
            defer { recoveryTask = nil }
            do {
                _ = try await validCredential(forceRefresh: true)
            } catch {
                guard deniedEpoch == accessEpoch, authenticated else { return }
                selectedLoading = false
                connection = .stale(error.localizedDescription)
                selectedError = error.localizedDescription
                errorMessage = error.localizedDescription
            }
        }
    }

    private func scheduleReconnect(_ reason: String) {
        connection = .stale(reason)
        guard foreground, authenticated, reconnectTask == nil else { return }
        reconnectTask = Task { @MainActor [weak self] in
            // Reconnect delay limits network churn; it never controls work on the owner.
            do { try await Task.sleep(for: .seconds(3)) } catch { return }
            guard let self, !Task.isCancelled else { return }
            reconnectTask = nil
            manualSync()
        }
    }
    private func stopStreams() {
        catalogTasks.values.forEach { $0.cancel() }; catalogTasks.removeAll()
        workTask?.cancel(); workTask = nil
        reconnectTask?.cancel(); reconnectTask = nil
    }
    func endSelection(_ work: WorkRef) {
        guard selectedRef == work else { return }
        sharingRequest = nil
        selectedRequest = UUID()
        workTask?.cancel(); workTask = nil
        selectedRef = nil; selectedWork = nil; selectedError = nil
    }
    func sceneActive() {
        MeshLifecycleDiagnostics.record("scene-active")
        foreground = true
        Task { @MainActor [weak self] in
            guard let self, foreground else { return }
            if accountDeletion?.blocksAccountAccess == true { await checkAccountDeletion() }
            else if authenticated { manualSync() }
        }
    }
    func sceneBackground() async {
        MeshLifecycleDiagnostics.record("scene-background")
        sharingRequest = nil
        foreground = false
        stopStreams()
        credentialSyncTask?.cancel(); credentialSyncTask = nil
        syncTask?.cancel(); syncTask = nil
        syncRequest = UUID()
        syncRunning = false
        accessEpoch = UUID()
        selectedRequest = UUID()
        selectedLoading = false
        selectedError = selectedWork == nil ? nil : "Reconnect to refresh this saved view."
        let pending = meshSuspension
        let mesh = mesh
        let suspension = Task {
            await pending?.value
            MeshLifecycleDiagnostics.record("background-shutdown-begin")
            await mesh?.suspend()
            MeshLifecycleDiagnostics.record("background-shutdown-end")
        }
        meshSuspension = suspension
        await suspension.value
    }
}
