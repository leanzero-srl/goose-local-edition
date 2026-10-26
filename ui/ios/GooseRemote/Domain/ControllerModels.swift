import Foundation

enum WorkKind: String, Codable, CaseIterable, Sendable {
    case chat, agentWork = "agent_work", benchmark, shellJob = "shell_job"
    var title: String {
        switch self { case .chat: "Chat"; case .agentWork: "Agent Work"; case .benchmark: "Benchmark"; case .shellJob: "Shell job" }
    }
    var symbol: String {
        switch self { case .chat: "bubble.left.and.bubble.right"; case .agentWork: "sparkles"; case .benchmark: "chart.bar.xaxis"; case .shellJob: "terminal" }
    }
}

struct WorkRef: Codable, Hashable, Identifiable, Sendable {
    let ownerNodeId: UUID
    let kind: WorkKind
    let workId: UUID
    var id: String { "\(ownerNodeId.uuidString)/\(kind.rawValue)/\(workId.uuidString)" }
}

enum WorkState: String, Codable, Sendable {
    case idle, running, waitingInput = "waiting_input", paused, finished, failed, interrupted, deleted
    var title: String {
        switch self { case .waitingInput: "Needs you"; default: rawValue.capitalized }
    }
}

enum WorkAction: String, Codable, CaseIterable, Sendable {
    case send, steer, stop, answerInput = "answer_input", note, pause, resume, tick
    var title: String {
        switch self { case .answerInput: "Answer"; case .send: "Send"; case .steer: "Steer"; default: rawValue.capitalized }
    }
}

struct CatalogEntry: Codable, Identifiable, Sendable {
    let work: WorkRef
    let storeGeneration: UUID
    let revision: UInt64
    let title: String
    let workingDir: String?
    let state: WorkState
    let currentRunId: UUID?
    let capabilities: [WorkAction]
    let parent: WorkRef?
    let related: [WorkRef]
    let updatedAt: Date
    let lastEvent: JSONValue?
    let archived: Bool
    var id: String { work.id }

    func canPerform(_ action: WorkAction) -> Bool {
        guard state != .deleted, capabilities.contains(action) else { return false }
        if action == .steer || action == .stop { return currentRunId != nil }
        if action == .answerInput { return state == .waitingInput }
        return true
    }
}

struct CatalogCursor: Codable, Sendable, Equatable {
    let ownerNodeId: UUID
    let storeGeneration: UUID
    let revision: UInt64
}
struct CatalogPage: Codable, Sendable {
    let entries: [CatalogEntry]
    let watermark: CatalogCursor
    let nextPageToken: String?
}

struct MeshEndpoint: Codable, Hashable, Sendable {
    let url: URL
    let tlsSpkiSha256: String
}

struct HostDescriptor: Codable, Identifiable, Sendable {
    let ownerNodeId: UUID
    let name: String
    let deviceRole: String
    let protocolVersion: UInt32
    let availability: String
    let lastSeenAt: Date?
    let endpoints: [MeshEndpoint]
    let capabilities: [String]
    let registrationState: String?
    let tlsSpkiSha256: String?
    var id: UUID { ownerNodeId }
    var controllerAvailable: Bool {
        deviceRole == "host" && protocolVersion == 1 && availability == "online" && !endpoints.isEmpty
    }
    var statusLabel: String {
        if registrationState == "awaiting_mesh_binding" { return "Mesh setup needed" }
        switch availability { case "online": return "Online"; case "offline": return "Offline"; case "update_required": return "Update needed"; default: return "Unavailable" }
    }
}

struct DeviceRecord: Codable, Identifiable, Sendable {
    let deviceId: UUID
    let name: String
    let platform: String
    let role: String
    let revision: UInt64
    let scopes: [String]
    let createdAt: Date
    let revokedAt: Date?
    var id: UUID { deviceId }
}
struct DeviceCredentials: Codable, Sendable {
    let accessToken: String
    let tokenType: String
    let expiresAt: Date
    let refreshToken: String
    let device: DeviceRecord
}
struct EnrollmentChallenge: Codable, Sendable {
    let challengeId: String
    let expiresAt: Date
}
struct EnrollmentAttempt: Codable, Sendable {
    let enrollmentId: UUID
    let email: String
    let deviceName: String
    var challenge: EnrollmentChallenge?
}
struct CredentialEnvelope: Codable, Sendable {
    let authority: URL
    var credentials: DeviceCredentials
    var refreshRequestId: UUID?
}
struct DevicePage: Codable, Sendable { let revision: UInt64; let devices: [DeviceRecord] }
struct HostPage: Codable, Sendable { let revision: UInt64; let hosts: [HostDescriptor] }
struct RevokeReceipt: Codable, Sendable { let deviceId: UUID; let revision: UInt64; let revokedAt: Date }

struct ControllerContent: Codable, Equatable, Sendable {
    let type: String
    let text: String
    static func text(_ value: String) -> Self { Self(type: "text", text: value) }
}
struct ActionPayload: Codable, Equatable, Sendable {
    var content: [ControllerContent]?
    var inputId: UUID?
    var optionId: String?
    var value: String?
    var text: String?
}
struct WorkCommand: Codable, Sendable {
    let commandId: UUID
    let work: WorkRef
    let expectedStoreGeneration: UUID
    let expectedRevision: UInt64
    let expectedRunId: UUID?
    let action: WorkAction
    let payload: ActionPayload

    static func make(entry: CatalogEntry, action: WorkAction, text: String, id: UUID = UUID()) throws -> Self {
        guard entry.canPerform(action) else { throw ControllerError.unsupported("This session does not support \(action.title.lowercased()) in its current state.") }
        guard [.send, .steer, .note, .stop, .pause, .resume, .tick].contains(action) else {
            throw ControllerError.unsupported("An answer must name the current input and selected option.")
        }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if [.send, .steer, .note].contains(action), trimmed.isEmpty { throw ControllerError.invalid("Write a message first.") }
        let payload: ActionPayload
        switch action {
        case .send, .steer: payload = .init(content: [.text(text)])
        case .note: payload = .init(text: text)
        default: payload = .init()
        }
        return Self(commandId: id, work: entry.work, expectedStoreGeneration: entry.storeGeneration,
                    expectedRevision: entry.revision, expectedRunId: entry.currentRunId, action: action, payload: payload)
    }

    static func answer(entry: CatalogEntry, input: PendingInput, option: InputOption, value: String?, id: UUID = UUID()) throws -> Self {
        guard entry.canPerform(.answerInput), entry.currentRunId == input.runId,
              input.options.contains(where: { $0.optionId == option.optionId && $0.acceptsValue == option.acceptsValue }),
              value == nil || option.acceptsValue else {
            throw ControllerError.invalid("This input no longer matches the active run or permitted choices. Refresh the session.")
        }
        return .init(commandId: id, work: entry.work, expectedStoreGeneration: entry.storeGeneration,
                     expectedRevision: entry.revision, expectedRunId: input.runId, action: .answerInput,
                     payload: .init(inputId: input.id, optionId: option.id, value: value))
    }
}

enum ReceiptState: String, Codable, Sendable {
    case accepted, waiting, dispatching, delivered, rejected, outcomeUnknown = "outcome_unknown"
    var title: String {
        switch self { case .outcomeUnknown: "Delivery unknown"; case .accepted: "Accepted by owner"; case .delivered: "Delivered to session"; default: rawValue.capitalized }
    }
}
struct CommandReceipt: Codable, Sendable {
    let commandId: UUID
    let work: WorkRef
    let authenticatedDeviceId: UUID
    let revision: UInt64
    let state: ReceiptState
    let runId: UUID?
    let messageId: String?
    let inputId: UUID?
    let checkpointId: String?
    let reason: String?
    let updatedAt: Date
}

// The selected-work adapter maps the canonical Goose history contract into these display values.
// No synthetic transcript is produced when decoding or transport fails.
struct TranscriptItem: Identifiable, Sendable {
    let id: String
    let revision: UInt64
    let role: String
    let text: String
    let timestamp: Date?
}
struct SelectedWork: Sendable {
    let entry: CatalogEntry
    let transcript: [TranscriptItem]
    let pendingInputs: [PendingInput]
    let nextPageToken: String?
    let historyRevision: UInt64
    var hasMoreHistory: Bool { nextPageToken != nil }
}
struct PendingInput: Codable, Identifiable, Sendable {
    let inputId: UUID
    let runId: UUID
    let prompt: String
    let options: [InputOption]
    var id: UUID { inputId }
}
struct InputOption: Codable, Identifiable, Sendable {
    let optionId: String
    let label: String
    let acceptsValue: Bool
    var id: String { optionId }
}
struct WorkSnapshot: Codable, Sendable {
    let entry: CatalogEntry
    let pendingInputs: [PendingInput]
    let historyWatermark: CatalogCursor
}
struct HistoryItem: Codable, Sendable {
    let messageId: String
    let revision: UInt64
    let message: JSONValue
}
struct HistoryPage: Codable, Sendable {
    let work: WorkRef
    let storeGeneration: UUID
    let revision: UInt64
    let items: [HistoryItem]
    let nextPageToken: String?
}
struct ResourceSample: Sendable {
    let sampledAt: Date
    let cpuLoadPerCore: Double?
    let memoryFreePercent: Double?
    let memoryTotalGB: Double?
}
struct ResourceSnapshot: Codable, Sendable {
    struct Load: Codable, Sendable {
        struct Own: Codable, Sendable {
            let cpuLoadPerCore: Double?
            let memFreePct: Double?
            let memTotalGb: Double?
        }
        let own: Own
    }
    let load: Load
    let sampledAt: Date
    var display: ResourceSample {
        .init(sampledAt: sampledAt, cpuLoadPerCore: load.own.cpuLoadPerCore,
              memoryFreePercent: load.own.memFreePct, memoryTotalGB: load.own.memTotalGb)
    }
}

enum JSONValue: Codable, Hashable, Sendable {
    case string(String), number(Double), bool(Bool), object([String: JSONValue]), array([JSONValue]), null
    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(Double.self) { self = .number(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode([JSONValue].self) { self = .array(v) }
        else { self = .object(try c.decode([String: JSONValue].self)) }
    }
    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .string(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .bool(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .null: try c.encodeNil()
        }
    }
}

enum ControllerError: Error, LocalizedError, Equatable {
    case invalid(String), unsupported(String), unavailable(String), remote(Int, String, String), malformedResponse, credentialStore(Int32)
    var errorDescription: String? {
        switch self {
        case .invalid(let text), .unsupported(let text), .unavailable(let text): return text
        case .remote(_, _, let message): return message
        case .malformedResponse: return "The server returned an incompatible response. Refresh or update the owner Mac."
        case .credentialStore: return "Secure device storage could not be accessed. Unlock this device and try again."
        }
    }
}
