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
    var authorityText = ""
    var email = ""
    var deviceName = ""
    var code = ""
    var errorMessage: String?
    var authBusy = false
    private(set) var envelope: CredentialEnvelope?
    private(set) var enrollment: EnrollmentAttempt?
    private(set) var connection: ConnectionState = .signedOut
    private(set) var hosts: [HostDescriptor] = []
    private(set) var devices: [DeviceRecord] = []
    private(set) var entries: [CatalogEntry] = []
    private(set) var selectedWork: SelectedWork?
    private(set) var selectedError: String?
    private(set) var selectedLoading = false
    private(set) var syncResults: [NodeSyncResult] = []
    private(set) var lastSync: Date?
    private(set) var syncRunning = false
    private(set) var resources: [UUID: ResourceSample] = [:]
    private(set) var draftRevision = 0
    private(set) var pendingRevision = 0
    private(set) var sending = false
    var selectedRef: WorkRef?

    @ObservationIgnored private let credentials: any CredentialStorage
    @ObservationIgnored private let owner: any OwnerControllerService
    @ObservationIgnored private let local: LocalControllerStore?
    @ObservationIgnored private let transport: any HTTPTransport
    @ObservationIgnored private var syncTask: Task<Void, Never>?
    @ObservationIgnored private var selectedRequest = UUID()
    @ObservationIgnored private var refreshTask: Task<CredentialEnvelope, Error>?

    struct SavedEnrollment: Codable { let authority: URL; let attempt: EnrollmentAttempt }
    struct SavedRevoke: Codable { let device: DeviceRecord; let requestId: UUID }

    init(credentials: any CredentialStorage = KeychainStore(), owner: any OwnerControllerService = UnavailableOwnerControllerService(),
         transport: any HTTPTransport = SecureHTTPTransport(), directory: URL? = nil) {
        self.credentials = credentials
        self.owner = owner
        self.transport = transport
        do {
            let base = try directory ?? FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                .appendingPathComponent("GooseRemote", isDirectory: true)
            local = try LocalControllerStore(directory: base)
        } catch {
            local = nil
            errorMessage = "Local drafts could not be opened. Sending is disabled until storage is available."
        }
        do {
            envelope = try credentials.read(CredentialEnvelope.self, account: "device")
            if let envelope { authorityText = envelope.authority.absoluteString; deviceName = envelope.credentials.device.name }
            else if let saved = try credentials.read(SavedEnrollment.self, account: "enrollment") {
                enrollment = saved.attempt
                authorityText = saved.authority.absoluteString
                email = saved.attempt.email
                deviceName = saved.attempt.deviceName
            }
        } catch { errorMessage = error.localizedDescription }
    }

    var authenticated: Bool { envelope != nil }
    var canRequestCode: Bool {
        !authBusy && (try? ControllerCodec.authority(authorityText)) != nil && email.contains("@") && !deviceName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
    var canStoreDrafts: Bool { local != nil }
    var pendingCommands: [PendingCommand] { _ = pendingRevision; return local?.data.pendingCommands ?? [] }
    var unfinishedCommand: PendingCommand? {
        pendingCommands.last { $0.command.work == selectedRef && $0.receipt?.state != .delivered && $0.receipt?.state != .rejected }
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
        guard !authBusy, !authenticated else { return }
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
        guard !authBusy, let enrollment else { return }
        authBusy = true
        defer { authBusy = false }
        do {
            guard let saved = try credentials.read(SavedEnrollment.self, account: "enrollment"), saved.attempt.enrollmentId == enrollment.enrollmentId else {
                throw ControllerError.invalid("Request a new sign-in code.")
            }
            let api = try ControllerAPI(authority: saved.authority, transport: transport)
            let received = try await api.verify(attempt: enrollment, code: code, platform: platform)
            let next = CredentialEnvelope(authority: saved.authority, credentials: received)
            // A failed secure write leaves the enrollment receipt retryable with the same code/id.
            try credentials.write(next, account: "device")
            envelope = next
            code = ""
            self.enrollment = nil
            try credentials.remove(account: "enrollment")
            errorMessage = nil
            manualSync()
        } catch { errorMessage = error.localizedDescription }
    }

    func validCredential() async throws -> CredentialEnvelope {
        guard let current = envelope else { throw ControllerError.unavailable("Connect this device to Link first.") }
        if let refreshTask { return try await refreshTask.value }
        if current.refreshRequestId == nil && current.credentials.expiresAt > Date() { return current }
        let task = Task { @MainActor [self] in
            var pending = current
            if pending.refreshRequestId == nil {
                pending.refreshRequestId = UUID()
                try credentials.write(pending, account: "device")
                envelope = pending
            }
            let api = try ControllerAPI(authority: pending.authority, transport: transport)
            let rotated = try await api.refresh(pending)
            let next = CredentialEnvelope(authority: pending.authority, credentials: rotated)
            try credentials.write(next, account: "device")
            envelope = next
            return next
        }
        refreshTask = task
        defer { refreshTask = nil }
        return try await task.value
    }

    func manualSync() {
        guard syncTask == nil, authenticated else { return }
        syncRunning = true
        connection = .loading
        syncTask = Task { @MainActor [self] in
            defer { syncTask = nil; syncRunning = false }
            do {
                let identity = try await validCredential()
                let api = try ControllerAPI(authority: identity.authority, transport: transport)
                let page = try await api.hosts(token: identity.credentials.accessToken)
                hosts = page.hosts
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
                        let result = try await owner.catalog(host: host, credential: identity.credentials.accessToken)
                        guard result.watermark.ownerNodeId == host.id,
                              result.entries.allSatisfy({ $0.work.ownerNodeId == host.id && $0.storeGeneration == result.watermark.storeGeneration }) else {
                            throw ControllerError.malformedResponse
                        }
                        entries.removeAll { $0.work.ownerNodeId == host.id }
                        entries.append(contentsOf: result.entries)
                        syncResults[index].state = .complete(result.entries.count)
                    } catch { syncResults[index].state = .failed(error.localizedDescription) }
                }
                lastSync = Date()
                connection = syncResults.contains { result in
                    if case .complete = result.state { return false }
                    return true
                } ? .partial : .connected
                errorMessage = nil
            } catch {
                connection = .stale(error.localizedDescription)
                errorMessage = error.localizedDescription
            }
        }
    }

    func select(_ work: WorkRef?) async {
        selectedRequest = UUID()
        selectedLoading = false
        selectedRef = work
        selectedWork = nil
        selectedError = nil
        guard let work, let entry = entries.first(where: { $0.work == work }), let host = hosts.first(where: { $0.id == work.ownerNodeId }) else { return }
        let request = UUID()
        selectedRequest = request
        selectedLoading = true
        defer { if selectedRequest == request { selectedLoading = false } }
        do {
            let identity = try await validCredential()
            let result = try await owner.selected(work: entry, host: host, credential: identity.credentials.accessToken)
            guard result.entry.work == work, result.entry.storeGeneration == entry.storeGeneration else { throw ControllerError.malformedResponse }
            if selectedRequest == request { selectedWork = result }
        } catch { if selectedRequest == request { selectedError = error.localizedDescription } }
    }

    func send(action: WorkAction) async {
        guard let work = selectedWork else { return }
        do {
            let command = try WorkCommand.make(entry: work.entry, action: action, text: draft(for: work.entry.work))
            await deliver(command)
        } catch { errorMessage = error.localizedDescription }
    }

    func answer(input: PendingInput, option: InputOption, value: String?) async {
        guard let current = selectedWork, current.pendingInputs.contains(where: { $0.id == input.id && $0.runId == input.runId }) else {
            errorMessage = "This input is no longer current. Refresh the session."
            return
        }
        do {
            let command = try WorkCommand.answer(entry: current.entry, input: input, option: option, value: value)
            await deliver(command)
        } catch { errorMessage = error.localizedDescription }
    }

    private func deliver(_ command: WorkCommand) async {
        guard !sending, unfinishedCommand == nil, let host = hosts.first(where: { $0.id == command.work.ownerNodeId }) else { return }
        sending = true
        defer { sending = false }
        do {
            guard let local else { throw ControllerError.unavailable("Command recovery storage is unavailable. Nothing was sent.") }
            let identity = try await validCredential()
            var pending = PendingCommand(command: command, createdAt: Date())
            try local.record(pending)
            pendingRevision += 1
            let receipt = try await owner.submit(command, host: host, credential: identity.credentials.accessToken)
            guard receipt.commandId == command.commandId, receipt.work == command.work else { throw ControllerError.malformedResponse }
            pending.receipt = receipt
            try local.record(pending)
            pendingRevision += 1
            if receipt.state == .delivered { clearDeliveredDraft(pending) }
            errorMessage = nil
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
            errorMessage = nil
        } catch { errorMessage = error.localizedDescription }
    }
    private func clearDeliveredDraft(_ pending: PendingCommand) {
        let sent = pending.command.payload.content?.first?.text ?? pending.command.payload.text
        if let sent, draft(for: pending.command.work) == sent { saveDraft("", for: pending.command.work) }
    }

    func loadResources(_ host: HostDescriptor) async {
        do {
            let identity = try await validCredential()
            resources[host.id] = try await owner.resources(host: host, credential: identity.credentials.accessToken)
        } catch { errorMessage = error.localizedDescription }
    }
    func loadMoreHistory() async {
        guard !selectedLoading, let current = selectedWork, let token = current.nextPageToken,
              let host = hosts.first(where: { $0.id == current.entry.work.ownerNodeId }) else { return }
        let request = selectedRequest
        selectedLoading = true
        defer { if selectedRequest == request { selectedLoading = false } }
        do {
            let identity = try await validCredential()
            let page = try await owner.history(work: current.entry, pageToken: token, host: host, credential: identity.credentials.accessToken)
            guard page.work == current.entry.work, page.storeGeneration == current.entry.storeGeneration, page.revision == current.historyRevision,
                  page.nextPageToken != token else { throw ControllerError.malformedResponse }
            let messages = try TranscriptRenderer.items(page.items)
            let combined = current.transcript + messages
            guard Set(combined.map(\.id)).count == combined.count else { throw ControllerError.malformedResponse }
            if selectedRequest == request {
                selectedWork = .init(entry: current.entry, transcript: combined, pendingInputs: current.pendingInputs,
                                     nextPageToken: page.nextPageToken, historyRevision: page.revision)
            }
        } catch { errorMessage = error.localizedDescription }
    }

    func loadDevices() async {
        do {
            let identity = try await validCredential()
            devices = try await ControllerAPI(authority: identity.authority, transport: transport).devices(token: identity.credentials.accessToken).devices
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
            if device.id == identity.credentials.device.id { try forgetDevice() }
            else { await loadDevices() }
        } catch { errorMessage = error.localizedDescription }
    }
    func forgetDevice() throws {
        guard !syncRunning, refreshTask == nil, !sending else { throw ControllerError.invalid("Wait for the current request to finish before removing local access.") }
        try local?.clear()
        try credentials.remove(account: "device")
        try credentials.remove(account: "enrollment")
        envelope = nil
        enrollment = nil
        code = ""
        hosts = []; entries = []; devices = []; resources = [:]; syncResults = []
        selectedWork = nil; selectedRef = nil; lastSync = nil
        draftRevision += 1; pendingRevision += 1
        connection = .signedOut
    }
}
