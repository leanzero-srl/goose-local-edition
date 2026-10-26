import Foundation

struct OwnerConnection: Sendable {
    let endpoint: MeshEndpoint
    let transport: any HTTPTransport
}

protocol OwnerConnectionProvider: Sendable {
    // The implementation must bind the worker-authenticated SPKI pin and owner UUID to TLS.
    // Returning an ordinary unpinned session for an untrusted endpoint violates this boundary.
    func connection(for host: HostDescriptor) async throws -> OwnerConnection
}

struct OwnerHTTPControllerService: OwnerControllerService {
    let connections: any OwnerConnectionProvider

    func catalog(host: HostDescriptor, credential: String) async throws -> OwnerCatalogSnapshot {
        var token: String?
        var seen = Set<String>()
        var watermark: CatalogCursor?
        var entries: [CatalogEntry] = []
        var resume = ""
        repeat {
            let query = token.map { [URLQueryItem(name: "page_token", value: $0)] } ?? []
            let page: CatalogPage = try await request("GET", path: "catalog", query: query, host: host, credential: credential, body: Optional<String>.none)
            guard page.watermark.ownerNodeId == host.id,
                  watermark == nil || watermark == page.watermark,
                  !page.resumeCursor.isEmpty,
                  page.entries.allSatisfy({ $0.work.ownerNodeId == host.id && $0.storeGeneration == page.watermark.storeGeneration }) else {
                throw ControllerError.malformedResponse
            }
            watermark = page.watermark
            resume = page.resumeCursor
            entries.append(contentsOf: page.entries)
            token = page.nextPageToken
            if let token, !seen.insert(token).inserted { throw ControllerError.invalid("The owner repeated a catalog page. Refresh the connection.") }
        } while token != nil
        guard let watermark, Set(entries.map(\.id)).count == entries.count else { throw ControllerError.malformedResponse }
        return .init(entries: entries, watermark: watermark, resumeCursor: resume)
    }

    func selected(work: CatalogEntry, host: HostDescriptor, credential: String) async throws -> SelectedWork {
        guard work.work.ownerNodeId == host.id else { throw ControllerError.invalid("The selected session belongs to another Mac.") }
        let query = [URLQueryItem(name: "store_generation", value: work.storeGeneration.uuidString)]
        let snapshot: WorkSnapshot = try await request("GET", path: workPath(work.work), query: query, host: host, credential: credential, body: Optional<String>.none)
        guard snapshot.entry.work == work.work, snapshot.entry.storeGeneration == work.storeGeneration,
              snapshot.historyWatermark.ownerNodeId == host.id, snapshot.historyWatermark.storeGeneration == work.storeGeneration else {
            throw ControllerError.malformedResponse
        }
        let page: HistoryPage = try await request("GET", path: workPath(work.work) + "/history", query: query, host: host, credential: credential, body: Optional<String>.none)
        try validate(page, work: snapshot.entry)
        guard !snapshot.workCursor.isEmpty, snapshot.historyWatermark.revision >= snapshot.entry.revision,
              page.revision >= snapshot.historyWatermark.revision else { throw ControllerError.malformedResponse }
        let committed = Set(page.items.map(\.messageId))
        let partials = snapshot.liveMessages.filter { !committed.contains($0.messageId) }
        guard Set(partials.map(\.messageId)).count == partials.count else { throw ControllerError.malformedResponse }
        return .init(entry: snapshot.entry, transcript: try TranscriptRenderer.items(page.items + partials), pendingInputs: snapshot.pendingInputs,
                     nextPageToken: page.nextPageToken, historyRevision: page.revision, workCursor: snapshot.workCursor,
                     streamRevision: snapshot.historyWatermark.revision, partialMessages: Dictionary(uniqueKeysWithValues: partials.map { ($0.messageId, $0) }),
                     historyOrder: page.items.map(\.messageId), sharing: snapshot.sharing)
    }

    func history(work: CatalogEntry, pageToken: String, host: HostDescriptor, credential: String) async throws -> HistoryPage {
        guard work.work.ownerNodeId == host.id else { throw ControllerError.invalid("The session belongs to another Mac.") }
        let query = [URLQueryItem(name: "store_generation", value: work.storeGeneration.uuidString), URLQueryItem(name: "page_token", value: pageToken)]
        let page: HistoryPage = try await request("GET", path: workPath(work.work) + "/history", query: query, host: host, credential: credential, body: Optional<String>.none)
        try validate(page, work: work)
        guard page.nextPageToken != pageToken else { throw ControllerError.malformedResponse }
        return page
    }
    func resources(host: HostDescriptor, credential: String) async throws -> ResourceSample {
        let response: ResourceSnapshot = try await request("GET", path: "resources", host: host, credential: credential, body: Optional<String>.none)
        return response.display
    }
    func submit(_ command: WorkCommand, host: HostDescriptor, credential: String) async throws -> CommandReceipt {
        guard command.work.ownerNodeId == host.id else { throw ControllerError.invalid("The command belongs to another Mac.") }
        let receipt: CommandReceipt = try await request("POST", path: workPath(command.work) + "/commands", host: host, credential: credential, body: command)
        guard receipt.work == command.work, receipt.commandId == command.commandId else { throw ControllerError.malformedResponse }
        return receipt
    }
    func receipt(commandId: UUID, host: HostDescriptor, credential: String) async throws -> CommandReceipt {
        let receipt: CommandReceipt = try await request("GET", path: "commands/\(commandId.uuidString)", host: host, credential: credential, body: Optional<String>.none)
        guard receipt.work.ownerNodeId == host.id, receipt.commandId == commandId else { throw ControllerError.malformedResponse }
        return receipt
    }
    private func workPath(_ work: WorkRef) -> String { "work/\(work.kind.rawValue)/\(work.workId.uuidString)" }
    private func validate(_ page: HistoryPage, work: CatalogEntry) throws {
        guard page.work == work.work, page.storeGeneration == work.storeGeneration,
              !page.workCursor.isEmpty else { throw ControllerError.invalid("The session changed while history was loading. Refresh it before continuing.") }
    }

    func changes(cursor: String, host: HostDescriptor, credential: String) async throws -> CatalogChanges {
        try await request("GET", path: "catalog/changes", query: [URLQueryItem(name: "cursor", value: cursor)], host: host, credential: credential, body: Optional<String>.none)
    }
    func catalogEvents(cursor: String, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<CatalogChange, Error> {
        try await stream(path: "catalog/stream", cursor: cursor, host: host, credential: credential)
    }
    func workEvents(work: SelectedWork, host: HostDescriptor, credential: String) async throws -> AsyncThrowingStream<WorkFrame, Error> {
        guard work.entry.work.ownerNodeId == host.id else { throw ControllerError.malformedResponse }
        return try await stream(path: workPath(work.entry.work) + "/stream", cursor: work.workCursor, host: host, credential: credential,
                                generation: work.entry.storeGeneration)
    }
    private func stream<T: Decodable & Sendable>(path: String, cursor: String, host: HostDescriptor, credential: String,
                                                 generation: UUID? = nil) async throws -> AsyncThrowingStream<T, Error> {
        guard !cursor.isEmpty, host.controllerAvailable else { throw ControllerError.invalid("A verified replay cursor is required.") }
        let connection = try await connections.connection(for: host)
        try validate(connection, host: host)
        guard let transport = connection.transport as? any OwnerEventTransport else { throw ControllerError.unavailable("The owner transport has no secure stream support.") }
        var parts = URLComponents(url: connection.endpoint.url.appendingPathComponent("v1/controller").appendingPathComponent(path), resolvingAgainstBaseURL: false)
        parts?.scheme = "wss"
        parts?.queryItems = [URLQueryItem(name: "cursor", value: cursor)]
        if let generation { parts?.queryItems?.append(.init(name: "store_generation", value: generation.uuidString)) }
        guard let url = parts?.url else { throw ControllerError.malformedResponse }
        var request = URLRequest(url: url)
        request.setValue("Bearer \(credential)", forHTTPHeaderField: "Authorization")
        let events = transport.events(for: request)
        return AsyncThrowingStream(bufferingPolicy: .bufferingOldest(16)) { continuation in
            let task = Task {
                do {
                    for try await data in events {
                        let event = try ControllerCodec.decoder().decode(T.self, from: data)
                        if case .dropped = continuation.yield(event) { throw ControllerError.unavailable("The live view needs replay. Refresh to continue.") }
                    }
                    continuation.finish()
                } catch { continuation.finish(throwing: error) }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }
    private func validate(_ connection: OwnerConnection, host: HostDescriptor) throws {
        guard host.endpoints.contains(connection.endpoint), connection.endpoint.url.scheme == "https",
              connection.endpoint.url.user == nil, connection.endpoint.url.password == nil,
              connection.endpoint.url.query == nil, connection.endpoint.url.fragment == nil else { throw ControllerError.invalid("The connection does not match the owner’s registered endpoint.") }
    }
    func request<T: Decodable, Body: Encodable>(_ method: String, path: String, query: [URLQueryItem] = [],
                                                       host: HostDescriptor, credential: String, body: Body?) async throws -> T {
        guard host.controllerAvailable else { throw ControllerError.unavailable("This Mac has no verified controller connection.") }
        let connection = try await connections.connection(for: host)
        try validate(connection, host: host)
        var components = URLComponents(url: connection.endpoint.url.appendingPathComponent("v1/controller").appendingPathComponent(path), resolvingAgainstBaseURL: false)
        components?.queryItems = query.isEmpty ? nil : query
        guard let url = components?.url else { throw ControllerError.invalid("The owner endpoint is invalid.") }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue("Bearer \(credential)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if let body {
            request.httpBody = try ControllerCodec.encoder().encode(body)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        let (data, response) = try await connection.transport.data(for: request)
        guard (200..<300).contains(response.statusCode) else {
            if let error = try? ControllerCodec.decoder().decode(OwnerErrorEnvelope.self, from: data) {
                throw ControllerError.remote(response.statusCode, error.error.code, error.error.message)
            }
            throw ControllerError.remote(response.statusCode, "owner_error", "The owner could not complete the request (HTTP \(response.statusCode)).")
        }
        do { return try ControllerCodec.decoder().decode(T.self, from: data) }
        catch { throw ControllerError.malformedResponse }
    }
}
private struct OwnerErrorEnvelope: Decodable {
    struct Detail: Decodable { let code: String; let message: String }
    let error: Detail
}

enum TranscriptRenderer {
    static func items(_ values: [HistoryItem]) throws -> [TranscriptItem] {
        try values.compactMap { value in
            guard case .object(let message) = value.message, case .string(let role) = message["role"],
                  case .array(let blocks) = message["content"] else { throw ControllerError.malformedResponse }
            if case .object(let metadata) = message["metadata"], case .bool(false) = metadata["userVisible"] { return nil }
            let text = try blocks.compactMap { block -> String? in
                guard case .object(let fields) = block, case .string(let type) = fields["type"] else { throw ControllerError.malformedResponse }
                switch type {
                case "text":
                    guard case .string(let text) = fields["text"] else { throw ControllerError.malformedResponse }
                    return text
                case "thinking", "redactedThinking": return nil
                case "systemNotification": if case .string(let message) = fields["msg"] { return message }; return "System notification"
                case "toolRequest": return "Tool request · Open the owner Mac for its full details."
                case "toolResponse": return "Tool response · Open the owner Mac for its full details."
                case "image": return "Image attachment · Open the owner Mac to view this attachment."
                case "toolConfirmationRequest", "actionRequired": return "Input requested · See the current request above."
                default: return "Unsupported message content (\(type)). Update the app or view it on the owner Mac."
                }
            }.joined(separator: "\n\n")
            guard !text.isEmpty else { return nil }
            let timestamp: Date?
            if let created = message["created"]?.doubleValue { timestamp = Date(timeIntervalSince1970: created) } else { timestamp = nil }
            return TranscriptItem(id: value.messageId, revision: value.revision, role: role, text: text, timestamp: timestamp)
        }
    }
}
