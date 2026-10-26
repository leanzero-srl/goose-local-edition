import Foundation

struct SessionSharingRequest: Identifiable, Sendable {
    let command: WorkCommand
    let authority: URL
    let deviceId: UUID
    let ownerName: String
    let input: PendingInput?
    var sharing: SessionSharingDescriptor? = nil
    var id: UUID { command.commandId }
    var actionTitle: String { command.action.title }
    var reviewText: String {
        if let input, let choice = input.options.first(where: { $0.id == command.payload.optionId }) {
            return [input.prompt, choice.label, command.payload.value].compactMap { $0 }.joined(separator: "\n\n")
        }
        return command.payload.content?.map(\.text).joined(separator: "\n") ?? command.payload.text ?? ""
    }
    var disclosure: String {
        "This sends your message or answer to \(ownerName). To carry out this action, that Mac may share it and session context with its configured AI providers and external tool services, including third-party AI."
    }
    var recipientLimitation: String {
        guard let sharing, sharing.hasKnownProvenance else {
            return "This session does not report sharing details this app can verify. Check its AI and tool settings on \(ownerName) before sharing sensitive information."
        }
        return "This is a session snapshot. Providers may route requests onward, and tools can contact other services. Settings can change; this is not a complete list of recipients."
    }
    func matches(_ identity: CredentialEnvelope) -> Bool {
        authority == identity.authority && deviceId == identity.credentials.device.id
    }
}


struct SessionSharingDescriptor: Codable, Sendable, Equatable {
    let provenance: String
    let provider: SharingProvider?
    let tools: [SharingTool]
    let limitations: [String]

    var hasKnownProvenance: Bool { ["live", "saved", "unavailable"].contains(provenance) }
    var showsReportedServices: Bool { provenance == "live" || provenance == "saved" }
    var sourceDescription: String {
        switch provenance {
        case "live": "Reported by the active session when loaded."
        case "saved": "Saved configuration only. Active connections have not been verified."
        case "unavailable": "The owner could not report this session’s active services."
        default: "This app cannot interpret the owner’s sharing details."
        }
    }

    static func safeOrigin(_ value: String?) -> String? {
        guard let value, let parsed = URLComponents(string: value),
              let scheme = parsed.scheme?.lowercased(), ["http", "https"].contains(scheme),
              let host = parsed.host, !host.isEmpty else { return nil }
        var origin = URLComponents()
        origin.scheme = scheme
        origin.host = host
        origin.port = parsed.port
        return origin.url?.absoluteString
    }
}

struct SharingProvider: Codable, Sendable, Equatable {
    let name: String
    let origin: String?
    init(name: String, origin: String?) {
        self.name = name
        self.origin = SessionSharingDescriptor.safeOrigin(origin)
    }
    enum CodingKeys: String, CodingKey { case name, origin }
    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        self.init(name: try values.decode(String.self, forKey: .name),
                  origin: try values.decodeIfPresent(String.self, forKey: .origin))
    }
}

struct SharingTool: Codable, Sendable, Equatable {
    let name: String
    let transport: String
    let origin: String?
    init(name: String, transport: String, origin: String?) {
        self.name = name
        self.transport = transport
        self.origin = transport == "http" ? SessionSharingDescriptor.safeOrigin(origin) : nil
    }
    enum CodingKeys: String, CodingKey { case name, transport, origin }
    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        self.init(name: try values.decode(String.self, forKey: .name), transport: try values.decode(String.self, forKey: .transport),
                  origin: try values.decodeIfPresent(String.self, forKey: .origin))
    }
}
