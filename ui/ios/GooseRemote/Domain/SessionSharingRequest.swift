import Foundation

struct SessionSharingRequest: Identifiable, Sendable {
    let command: WorkCommand
    let authority: URL
    let deviceId: UUID
    let ownerName: String
    let input: PendingInput?
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
        "This session does not report the provider or service names. Check its AI and tool settings on \(ownerName) before sharing sensitive information."
    }
    func matches(_ identity: CredentialEnvelope) -> Bool {
        authority == identity.authority && deviceId == identity.credentials.device.id
    }
}
