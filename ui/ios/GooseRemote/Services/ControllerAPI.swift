import Foundation

protocol HTTPTransport: Sendable {
    func data(for request: URLRequest) async throws -> (Data, HTTPURLResponse)
}

private final class RejectRedirectDelegate: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}

final class SecureHTTPTransport: HTTPTransport, @unchecked Sendable {
    private let session: URLSession
    init() {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.urlCache = nil
        configuration.httpCookieStorage = nil
        configuration.urlCredentialStorage = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        session = URLSession(configuration: configuration, delegate: RejectRedirectDelegate(), delegateQueue: nil)
    }
    func data(for request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        let (data, response) = try await session.data(for: request)
        guard let response = response as? HTTPURLResponse else { throw ControllerError.malformedResponse }
        return (data, response)
    }
}

private struct APIErrorEnvelope: Decodable {
    struct Detail: Decodable { let code: String; let message: String }
    let error: Detail
}

struct ControllerAPI: Sendable {
    let authority: URL
    let transport: any HTTPTransport
    init(authority: URL, transport: any HTTPTransport = SecureHTTPTransport()) throws {
        self.authority = try ControllerCodec.authority(authority.absoluteString)
        self.transport = transport
    }
    func request<T: Decodable, Body: Encodable>(_ method: String, path: String, token: String? = nil, body: Body?) async throws -> T {
        var request = URLRequest(url: authority.appendingPathComponent("v1/controller").appendingPathComponent(path))
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if let token { request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization") }
        if let body {
            request.httpBody = try ControllerCodec.encoder().encode(body)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        let (data, response) = try await transport.data(for: request)
        guard (200..<300).contains(response.statusCode) else {
            if let envelope = try? ControllerCodec.decoder().decode(APIErrorEnvelope.self, from: data) {
                throw ControllerError.remote(response.statusCode, envelope.error.code, envelope.error.message)
            }
            throw ControllerError.remote(response.statusCode, "invalid_response", "Link could not complete the request (HTTP \(response.statusCode)).")
        }
        do { return try ControllerCodec.decoder().decode(T.self, from: data) }
        catch { throw ControllerError.malformedResponse }
    }
    func requestCode(_ attempt: EnrollmentAttempt) async throws -> EnrollmentChallenge {
        struct Body: Encodable { let email: String; let enrollmentId: UUID }
        return try await request("POST", path: "auth/request-code", body: Body(email: attempt.email, enrollmentId: attempt.enrollmentId))
    }
    func verify(attempt: EnrollmentAttempt, code: String, platform: String) async throws -> DeviceCredentials {
        guard let challenge = attempt.challenge else { throw ControllerError.invalid("Request a sign-in code first.") }
        struct Body: Encodable { let challengeId: String; let enrollmentId: UUID; let code: String; let deviceName: String; let platform: String }
        let credentials: DeviceCredentials = try await request("POST", path: "auth/verify", body: Body(challengeId: challenge.challengeId,
            enrollmentId: attempt.enrollmentId, code: code, deviceName: attempt.deviceName, platform: platform))
        try validate(credentials)
        return credentials
    }
    func refresh(_ envelope: CredentialEnvelope) async throws -> DeviceCredentials {
        guard let requestId = envelope.refreshRequestId else { throw ControllerError.invalid("Refresh request was not saved.") }
        struct Body: Encodable { let deviceId: UUID; let refreshToken: String; let requestId: UUID }
        let credentials: DeviceCredentials = try await request("POST", path: "auth/refresh", body: Body(deviceId: envelope.credentials.device.deviceId,
            refreshToken: envelope.credentials.refreshToken, requestId: requestId))
        try validate(credentials)
        guard credentials.device.deviceId == envelope.credentials.device.deviceId else { throw ControllerError.malformedResponse }
        return credentials
    }
    func hosts(token: String) async throws -> HostPage {
        try await request("GET", path: "hosts", token: token, body: Optional<String>.none)
    }
    func devices(token: String) async throws -> DevicePage {
        try await request("GET", path: "devices", token: token, body: Optional<String>.none)
    }
    func revoke(device: DeviceRecord, requestId: UUID, token: String) async throws -> RevokeReceipt {
        struct Body: Encodable { let requestId: UUID; let expectedRevision: UInt64 }
        return try await request("POST", path: "devices/\(device.deviceId.uuidString)/revoke", token: token,
                                 body: Body(requestId: requestId, expectedRevision: device.revision))
    }
    private func validate(_ credentials: DeviceCredentials) throws {
        guard credentials.tokenType == "Bearer", credentials.device.role == "controller", credentials.device.revokedAt == nil,
              !credentials.accessToken.isEmpty, !credentials.refreshToken.isEmpty else { throw ControllerError.malformedResponse }
    }
}
