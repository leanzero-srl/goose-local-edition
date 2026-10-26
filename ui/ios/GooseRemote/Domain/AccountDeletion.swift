import Foundation

struct AccountDeletionChallenge: Codable, Sendable {
    let requestId: UUID
    let deletionToken: String
    let expiresAt: Date
}

struct AccountDeletionReceipt: Codable, Sendable {
    enum Status: String, Codable, Sendable { case pending, deleted }
    let requestId: UUID
    let status: Status
    let pending: [String]
    let completedAt: Date?
    let retained: [String]
    var retentionSummary: [String] {
        retained.map {
            switch $0 {
            case "security_revocation_record": "A minimal security record keeps revoked credentials from working."
            case "historical_service_records": "Historical service records follow their existing retention."
            case "shared_contact_record": "A shared email contact record may remain for other LeanZero services; Link membership is removed."
            default: "Link reports an additional retained record category. Update this app to review its details."
            }
        }
    }
}

struct SavedAccountDeletion: Codable, Sendable {
    enum Phase: String, Codable, Sendable { case requestingCode, awaitingCode, confirming, pending, deleted }
    let authority: URL
    let requestId: UUID
    let deviceId: UUID
    var phase: Phase
    var challenge: AccountDeletionChallenge?
    var receipt: AccountDeletionReceipt?
    var blocksAccountAccess: Bool { [.confirming, .pending, .deleted].contains(phase) }
    var canCancel: Bool { !blocksAccountAccess }
}

extension ControllerAPI {
    func requestAccountDeletionCode(requestId: UUID, token: String) async throws -> AccountDeletionChallenge {
        struct Body: Encodable { let requestId: UUID }
        let challenge: AccountDeletionChallenge = try await request("POST", path: "account/deletion/request-code", token: token, body: Body(requestId: requestId))
        guard challenge.requestId == requestId, !challenge.deletionToken.isEmpty else { throw ControllerError.malformedResponse }
        return challenge
    }
    func confirmAccountDeletion(_ saved: SavedAccountDeletion, code: String) async throws -> AccountDeletionReceipt {
        struct Body: Encodable { let code: String }
        guard let token = saved.challenge?.deletionToken else { throw ControllerError.invalid("Request a deletion code first.") }
        let receipt: AccountDeletionReceipt = try await request("POST", path: "account/deletion/confirm", token: token, body: Body(code: code))
        try validateDeletion(receipt, requestId: saved.requestId)
        return receipt
    }
    func accountDeletionStatus(_ saved: SavedAccountDeletion) async throws -> AccountDeletionReceipt {
        struct Body: Encodable {}
        guard let token = saved.challenge?.deletionToken else { throw ControllerError.invalid("Request a deletion code first.") }
        let receipt: AccountDeletionReceipt = try await request("POST", path: "account/deletion/status", token: token, body: Body())
        try validateDeletion(receipt, requestId: saved.requestId)
        return receipt
    }
    private func validateDeletion(_ receipt: AccountDeletionReceipt, requestId: UUID) throws {
        guard receipt.requestId == requestId,
              receipt.status != .deleted || (receipt.completedAt != nil && receipt.pending.isEmpty) else {
            throw ControllerError.malformedResponse
        }
    }
}
