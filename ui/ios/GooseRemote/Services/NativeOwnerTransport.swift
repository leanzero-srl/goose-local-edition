import CFNetwork
import Foundation
import Network
import Security

// URLSession cannot extend certificate trust for an ATS-protected mesh address.
// Network.framework supplies TLS and WebSocket framing with a mandatory owner verifier.
// ATS remains unchanged for every URLSession request, including the account authority.
final class PinnedOwnerTransport: OwnerEventTransport, @unchecked Sendable {
    private let endpoint: URL
    private let binding: OwnerTLSBinding
    private let proxies: [ProxyConfiguration]
    private let requestTimeout: TimeInterval
    private let queue = DispatchQueue(label: "net.leanzero.goose-remote.owner-transport")
    private let lock = NSLock()
    private var connections: [UUID: NWConnection] = [:]
    private var closed = false
    private static let responseLimit = 8 * 1024 * 1024

    init(configuration: URLSessionConfiguration, endpoint: URL, binding: OwnerTLSBinding) {
        self.endpoint = endpoint
        self.binding = binding
        proxies = configuration.proxyConfigurations.map { proxy in var value = proxy; value.allowFailover = false; return value }
        requestTimeout = configuration.timeoutIntervalForRequest
    }
    private func makeConnection(_ request: URLRequest, websocket: Bool) throws -> (UUID, NWConnection) {
        guard let url = request.url, url.scheme == (websocket ? "wss" : "https"), url.host == endpoint.host,
              url.port == endpoint.port, url.user == nil, url.password == nil, url.fragment == nil,
              request.value(forHTTPHeaderField: "Authorization")?.hasPrefix("Bearer ") == true else { throw ControllerError.invalid("The request does not belong to this verified owner.") }
        // Network.framework bypasses proxies for loopback destinations. Production
        // owner endpoints must be mesh addresses, never an authority-supplied localhost.
        guard proxies.isEmpty || MeshAddress.contains(url.host) else { throw ControllerError.invalid("A mesh proxy request must target an assigned mesh address.") }
        let tls = NWProtocolTLS.Options()
        sec_protocol_options_set_min_tls_protocol_version(tls.securityProtocolOptions, .TLSv12)
        sec_protocol_options_set_tls_server_name(tls.securityProtocolOptions, binding.serverName)
        sec_protocol_options_add_tls_application_protocol(tls.securityProtocolOptions, "http/1.1")
        let binding = binding
        sec_protocol_options_set_verify_block(tls.securityProtocolOptions, { _, trust, complete in
            do { try binding.validate(sec_trust_copy_ref(trust).takeRetainedValue()); complete(true) }
            catch { complete(false) }
        }, queue)
        let parameters = NWParameters(tls: tls, tcp: .init())
        if !proxies.isEmpty {
            let context = NWParameters.PrivacyContext(description: "Embedded controller mesh")
            context.proxyConfigurations = proxies
            parameters.setPrivacyContext(context)
        }
        let target: NWEndpoint
        if websocket {
            let options = NWProtocolWebSocket.Options()
            options.autoReplyPing = true
            options.maximumMessageSize = Self.responseLimit
            options.setAdditionalHeaders((request.allHTTPHeaderFields ?? [:]).map { (name: $0.key, value: $0.value) })
            parameters.defaultProtocolStack.applicationProtocols.insert(options, at: 0)
            target = .url(url)
        } else {
            guard let host = url.host, let port = NWEndpoint.Port(rawValue: UInt16(url.port ?? 443)) else { throw ControllerError.malformedResponse }
            target = .hostPort(host: NWEndpoint.Host(host), port: port)
        }
        let connection = NWConnection(to: target, using: parameters)
        let id = UUID()
        lock.lock(); defer { lock.unlock() }
        guard !closed else { throw URLError(.cancelled) }
        connections[id] = connection
        return (id, connection)
    }
    private func remove(_ id: UUID, connection: NWConnection) {
        connection.cancel()
        lock.lock(); defer { lock.unlock() }
        connections[id] = nil
    }
    private func start(_ connection: NWConnection) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            let pending = OnceContinuation(continuation)
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready: pending.finish(.success(()))
                case .failed(let error), .waiting(let error): pending.finish(.failure(error)); connection.cancel()
                case .cancelled: pending.finish(.failure(CancellationError()))
                default: break
                }
            }
            connection.start(queue: queue)
        }
    }
    private func watchdog(_ connection: NWConnection) -> Task<Void, Never> {
        Task {
            do { try await Task.sleep(for: .seconds(requestTimeout)); connection.cancel() }
            catch { }
        }
    }
    func data(for request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        let (id, connection) = try makeConnection(request, websocket: false)
        defer { remove(id, connection: connection) }
        let timer = watchdog(connection)
        defer { timer.cancel() }
        return try await withTaskCancellationHandler {
            try Task.checkCancellation()
            try await start(connection)
            let bytes = try OwnerHTTPMessage.request(request)
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                connection.send(content: bytes, completion: .contentProcessed { error in
                    if let error { continuation.resume(throwing: error) } else { continuation.resume() }
                })
            }
            var response = Data()
            while true {
                let (data, complete) = try await Self.receive(connection)
                guard response.count + data.count <= Self.responseLimit else { throw ControllerError.unavailable("The owner response exceeds this device’s transport limit.") }
                response.append(data)
                if complete { break }
            }
            return try OwnerHTTPMessage.response(response, url: request.url!)
        } onCancel: { connection.cancel() }
    }
    private static func receive(_ connection: NWConnection) async throws -> (Data, Bool) {
        try await withCheckedThrowingContinuation { continuation in
            connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { data, _, complete, error in
                if let error { continuation.resume(throwing: error) }
                else { continuation.resume(returning: (data ?? Data(), complete)) }
            }
        }
    }
    func events(for request: URLRequest) -> AsyncThrowingStream<Data, Error> {
        AsyncThrowingStream(bufferingPolicy: .bufferingOldest(16)) { continuation in
            let id: UUID, connection: NWConnection
            do { (id, connection) = try makeConnection(request, websocket: true) }
            catch { continuation.finish(throwing: error); return }
            let reader = Task {
                defer { remove(id, connection: connection) }
                let timer = watchdog(connection)
                do {
                    try await start(connection)
                    timer.cancel()
                    while !Task.isCancelled {
                        let data = try await Self.receiveWebSocket(connection)
                        if case .dropped = continuation.yield(data) { throw ControllerError.unavailable("The live view fell behind. Refresh to replay the owner’s history.") }
                    }
                    continuation.finish()
                } catch { timer.cancel(); continuation.finish(throwing: error) }
            }
            continuation.onTermination = { _ in reader.cancel(); connection.cancel() }
        }
    }
    private static func receiveWebSocket(_ connection: NWConnection) async throws -> Data {
        try await withCheckedThrowingContinuation { continuation in
            connection.receiveMessage { data, context, complete, error in
                if let error { continuation.resume(throwing: error); return }
                guard let metadata = context?.protocolMetadata(definition: NWProtocolWebSocket.definition) as? NWProtocolWebSocket.Metadata else {
                    continuation.resume(throwing: ControllerError.malformedResponse); return
                }
                if metadata.opcode == .close {
                    continuation.resume(throwing: OwnerStreamClose.error(metadata.closeCode)); return
                }
                guard complete, metadata.opcode == .text || metadata.opcode == .binary, let data, data.count <= responseLimit else {
                    continuation.resume(throwing: ControllerError.unavailable("The owner stream closed or returned an incompatible frame.")); return
                }
                continuation.resume(returning: data)
            }
        }
    }
    func close() {
        lock.lock()
        closed = true
        let current = Array(connections.values)
        connections.removeAll()
        lock.unlock()
        current.forEach { $0.cancel() }
    }
    deinit { close() }
}

enum OwnerStreamClose {
    static func error(_ code: NWProtocolWebSocket.CloseCode) -> ControllerError {
        let value: UInt16
        switch code { case .applicationCode(let code), .privateCode(let code): value = code; case .protocolCode(let code): value = code.rawValue; @unknown default: value = 1006 }
        switch value {
        case 4401: return .remote(401, "credential_expired", "The controller credential expired. Reconnect to refresh access.")
        case 4403: return .remote(403, "authorization_denied", "This device is no longer allowed to view this Mac.")
        case 4413: return .remote(503, "authorization_unavailable", "The owner could not renew access. Reconnect when Link is available.")
        default: return .unavailable("The owner stream closed (\(value)). Reconnect to replay missed changes.")
        }
    }
}

private final class OnceContinuation<Value: Sendable>: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Value, Error>?
    init(_ continuation: CheckedContinuation<Value, Error>) { self.continuation = continuation }
    func finish(_ result: Result<Value, Error>) {
        lock.lock()
        let pending = continuation
        continuation = nil
        lock.unlock()
        pending?.resume(with: result)
    }
}

enum OwnerHTTPMessage {
    static func request(_ request: URLRequest) throws -> Data {
        guard let url = request.url, let method = request.httpMethod, ["GET", "POST"].contains(method) else { throw ControllerError.invalid("Unsupported owner request method.") }
        let message = CFHTTPMessageCreateRequest(nil, method as CFString, url as CFURL, kCFHTTPVersion1_1).takeRetainedValue()
        guard let host = url.host else { throw ControllerError.malformedResponse }
        let authority = host + (url.port.map { ":\($0)" } ?? "")
        CFHTTPMessageSetHeaderFieldValue(message, "Host" as CFString, authority as CFString)
        for (name, value) in request.allHTTPHeaderFields ?? [:] {
            guard !name.contains("\r"), !name.contains("\n"), !value.contains("\r"), !value.contains("\n") else { throw ControllerError.invalid("Invalid request header.") }
            CFHTTPMessageSetHeaderFieldValue(message, name as CFString, value as CFString)
        }
        CFHTTPMessageSetHeaderFieldValue(message, "Connection" as CFString, "close" as CFString)
        CFHTTPMessageSetHeaderFieldValue(message, "Accept-Encoding" as CFString, "identity" as CFString)
        if let body = request.httpBody {
            CFHTTPMessageSetBody(message, body as CFData)
            CFHTTPMessageSetHeaderFieldValue(message, "Content-Length" as CFString, String(body.count) as CFString)
        }
        guard let serialized = CFHTTPMessageCopySerializedMessage(message)?.takeRetainedValue() else { throw ControllerError.malformedResponse }
        return serialized as Data
    }
    static func response(_ bytes: Data, url: URL) throws -> (Data, HTTPURLResponse) {
        guard let separator = bytes.range(of: Data("\r\n\r\n".utf8)), separator.upperBound <= 64 * 1024 else { throw ControllerError.malformedResponse }
        guard let rawHeaders = String(data: bytes[..<separator.lowerBound], encoding: .isoLatin1) else { throw ControllerError.malformedResponse }
        let names = rawHeaders.components(separatedBy: "\r\n").dropFirst().map { $0.split(separator: ":", maxSplits: 1).first?.lowercased() ?? "" }
        guard names.filter({ $0 == "content-length" }).count <= 1, names.filter({ $0 == "transfer-encoding" }).count <= 1 else { throw ControllerError.malformedResponse }
        let message = CFHTTPMessageCreateEmpty(nil, false).takeRetainedValue()
        let valid = bytes.withUnsafeBytes { buffer in CFHTTPMessageAppendBytes(message, buffer.bindMemory(to: UInt8.self).baseAddress!, bytes.count) }
        guard valid, CFHTTPMessageIsHeaderComplete(message),
              let fields = CFHTTPMessageCopyAllHeaderFields(message)?.takeRetainedValue() as? [String: String] else { throw ControllerError.malformedResponse }
        let status = CFHTTPMessageGetResponseStatusCode(message)
        guard (200...599).contains(status), let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: fields) else { throw ControllerError.malformedResponse }
        var body = Data(bytes[separator.upperBound...])
        if let encoding = response.value(forHTTPHeaderField: "Content-Encoding"), encoding.lowercased() != "identity" { throw ControllerError.malformedResponse }
        if let transfer = response.value(forHTTPHeaderField: "Transfer-Encoding") {
            guard transfer.lowercased() == "chunked", response.value(forHTTPHeaderField: "Content-Length") == nil else { throw ControllerError.malformedResponse }
            body = try decodeChunks(body)
        } else if let length = response.value(forHTTPHeaderField: "Content-Length") {
            guard let count = Int(length), count >= 0, count == body.count else { throw ControllerError.malformedResponse }
        }
        return (body, response)
    }
    static func decodeChunks(_ data: Data) throws -> Data {
        var position = data.startIndex
        var output = Data()
        while position < data.endIndex {
            guard let line = data[position...].range(of: Data("\r\n".utf8)),
                  let text = String(data: data[position..<line.lowerBound], encoding: .ascii),
                  let sizeText = text.split(separator: ";", maxSplits: 1).first,
                  !sizeText.isEmpty, sizeText.allSatisfy({ $0.isHexDigit }), let size = Int(sizeText, radix: 16) else { throw ControllerError.malformedResponse }
            position = line.upperBound
            if size == 0 {
                if data[position...] == Data("\r\n".utf8) { return output }
                guard let trailerEnd = data[position...].range(of: Data("\r\n\r\n".utf8)), trailerEnd.upperBound == data.endIndex else { throw ControllerError.malformedResponse }
                return output
            }
            guard size <= data.endIndex - position, data.endIndex - position - size >= 2,
                  data[(position + size)..<(position + size + 2)] == Data("\r\n".utf8) else { throw ControllerError.malformedResponse }
            output.append(data[position..<(position + size)])
            position += size + 2
        }
        throw ControllerError.malformedResponse
    }
}
