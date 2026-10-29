import Foundation

enum OwnerReplay {
    static func refreshed(_ current: SelectedWork, entry: CatalogEntry, pendingInputs: [PendingInput], sharing: SessionSharingDescriptor?) throws -> SelectedWork {
        guard current.entry.work == entry.work, current.entry.storeGeneration == entry.storeGeneration,
              entry.revision >= current.entry.revision else { throw ControllerError.malformedResponse }
        var next = current
        next.entry = entry
        next.pendingInputs = pendingInputs
        next.sharing = sharing
        // Keep the partial's bytes and replay cursor together. Overlaying snapshot
        // partials here would invalidate the next replay frame's byte offset.
        return next
    }
    static func catalog(_ change: CatalogChange, into snapshot: OwnerCatalogSnapshot) throws -> OwnerCatalogSnapshot {
        guard change.cursor.ownerNodeId == snapshot.watermark.ownerNodeId,
              change.cursor.storeGeneration == snapshot.watermark.storeGeneration,
              change.work.ownerNodeId == snapshot.watermark.ownerNodeId, !change.resumeCursor.isEmpty else { throw ControllerError.malformedResponse }
        if change.cursor.revision <= snapshot.watermark.revision { return snapshot }
        var entries = snapshot.entries.filter { $0.work != change.work }
        switch change.type {
        case "upsert":
            guard let entry = change.entry, entry.work == change.work,
                  entry.storeGeneration == snapshot.watermark.storeGeneration, entry.revision == change.cursor.revision else { throw ControllerError.malformedResponse }
            entries.append(entry)
        case "tombstone": guard change.entry == nil else { throw ControllerError.malformedResponse }
        default: throw ControllerError.malformedResponse
        }
        return .init(entries: entries, watermark: change.cursor, resumeCursor: change.resumeCursor)
    }

    static func work(_ frame: WorkFrame, into current: SelectedWork) throws -> SelectedWork {
        guard frame.work == current.entry.work, frame.storeGeneration == current.entry.storeGeneration,
              frame.streamEpoch == current.entry.storeGeneration, frame.workRevision == frame.sequence,
              !frame.resumeCursor.isEmpty else { throw ControllerError.malformedResponse }
        if frame.sequence <= current.streamRevision { return current }
        var next = current
        if frame.sequence <= current.historyRevision,
           ["message_upsert", "message_deleted", "partial_replacement", "partial_discarded"].contains(frame.type) {
            next.streamRevision = frame.sequence
            next.workCursor = frame.resumeCursor
            next.entry.revision = max(next.entry.revision, frame.workRevision)
            return next
        }
        switch frame.type {
        case "message_upsert":
            struct Payload: Decodable { let item: HistoryItem; let previousMessageId: String? }
            let value: Payload = try decode(frame.payload)
            guard value.item.revision <= frame.sequence else { throw ControllerError.malformedResponse }
            if let old = value.previousMessageId {
                next.transcript.removeAll { $0.id == old }; next.partialMessages[old] = nil
                next.messageTombstones[old] = frame.sequence
                if old != value.item.messageId {
                    next.messageRenames[old] = value.item.messageId
                    if let index = next.historyOrder.firstIndex(of: old) { next.historyOrder[index] = value.item.messageId }
                }
            }
            next.partialMessages[value.item.messageId] = nil
            let item = try TranscriptRenderer.items([value.item])
            if let old = next.transcript.firstIndex(where: { $0.id == value.item.messageId }) {
                if next.transcript[old].revision <= value.item.revision {
                    next.transcript.remove(at: old)
                    next.transcript.insert(contentsOf: item, at: old)
                }
            } else { next.transcript.append(contentsOf: item) }
            orderTranscript(&next)
        case "message_deleted":
            struct Payload: Decodable { let messageId: String }
            let value: Payload = try decode(frame.payload)
            next.transcript.removeAll { $0.id == value.messageId && $0.revision <= frame.sequence }
            next.partialMessages[value.messageId] = nil
            next.messageTombstones[value.messageId] = frame.sequence
        case "partial_replacement":
            struct Payload: Decodable { let messageId: String; let messageRevision: UInt64; let offsetBytes: UInt64; let content: JSONValue }
            let value: Payload = try decode(frame.payload)
            guard value.messageRevision == frame.sequence, frame.runId != nil else { throw ControllerError.malformedResponse }
            // A history page may already include the commit of this older replayed partial.
            if next.transcript.contains(where: { $0.id == value.messageId && $0.revision >= value.messageRevision }), next.partialMessages[value.messageId] == nil { break }
            let old = next.partialMessages[value.messageId]
            guard value.offsetBytes == (try old.map { try textBytes($0.message) } ?? 0) else {
                throw ControllerError.replayRequired("Part of the live answer is missing. Refresh to restore it from the owner.")
            }
            let message = try merge(old?.message, chunk: value.content)
            let item = HistoryItem(messageId: value.messageId, revision: value.messageRevision, message: message)
            let display = try TranscriptRenderer.items([item])
            next.partialMessages[value.messageId] = item
            let position = next.transcript.firstIndex { $0.id == value.messageId } ?? next.transcript.count
            next.transcript.removeAll { $0.id == value.messageId }
            next.transcript.insert(contentsOf: display, at: position)
        case "partial_discarded":
            struct Payload: Decodable { let messageIds: [String] }
            let value: Payload = try decode(frame.payload)
            for id in value.messageIds where next.partialMessages[id] != nil {
                next.partialMessages[id] = nil
                next.transcript.removeAll { $0.id == id }
            }
        case "tombstone": throw ControllerError.remote(410, "work_removed", "This session is no longer available on its owner.")
        default:
            struct Payload: Decodable { let entry: CatalogEntry }
            let value: Payload = try decode(frame.payload)
            guard value.entry.work == frame.work, value.entry.storeGeneration == frame.storeGeneration,
                  value.entry.revision == frame.sequence else { throw ControllerError.malformedResponse }
            if value.entry.revision >= next.entry.revision { next.entry = value.entry }
            if next.entry.state != .waitingInput { next.pendingInputs = [] }
        }
        next.entry.revision = max(next.entry.revision, frame.workRevision)
        next.streamRevision = frame.sequence
        next.workCursor = frame.resumeCursor
        return next
    }

    static func history(_ page: HistoryPage, into current: SelectedWork) throws -> SelectedWork {
        guard page.work == current.entry.work, page.storeGeneration == current.entry.storeGeneration,
              page.revision == current.historyRevision else { throw ControllerError.malformedResponse }
        var next = current
        var olderOrder: [String] = []
        for item in page.items {
            var id = item.messageId
            var visited = Set<String>()
            while let replacement = next.messageRenames[id] {
                guard visited.insert(id).inserted else { throw ControllerError.malformedResponse }
                id = replacement
            }
            if !next.historyOrder.contains(id), !olderOrder.contains(id) { olderOrder.append(id) }
        }
        next.historyOrder.insert(contentsOf: olderOrder, at: 0)
        for message in try TranscriptRenderer.items(page.items) {
            if let deletedAt = next.messageTombstones[message.id], message.revision <= deletedAt { continue }
            if let index = next.transcript.firstIndex(where: { $0.id == message.id }) {
                if next.transcript[index].revision < message.revision { next.transcript[index] = message }
            } else { next.transcript.append(message) }
        }
        orderTranscript(&next)
        next.nextPageToken = page.nextPageToken
        return next
    }

    private static func orderTranscript(_ work: inout SelectedWork) {
        let order = Dictionary(work.historyOrder.enumerated().map { ($0.element, $0.offset) }, uniquingKeysWith: min)
        // Pages carry the owner's row order. Stable sorting keeps that order when
        // timestamps tie, ahead of newer live rows not yet in this history snapshot.
        let external = work.entry.work.kind != .chat
        work.transcript.sort {
            if external, let first = logOffset($0.id), let second = logOffset($1.id), first != second { return first < second }
            let first = $0.timestamp ?? .distantFuture, second = $1.timestamp ?? .distantFuture
            if first != second { return first < second }
            return (order[$0.id] ?? .max) < (order[$1.id] ?? .max)
        }
    }

    private static func logOffset(_ id: String) -> UInt64? {
        let parts = id.split(separator: "_")
        guard parts.count == 3, parts[0] == "log", let start = UInt64(parts[1]), let end = UInt64(parts[2]), end >= start else { return nil }
        return start
    }

    private static func decode<T: Decodable>(_ value: JSONValue) throws -> T {
        try ControllerCodec.decoder().decode(T.self, from: ControllerCodec.encoder().encode(value))
    }
    private static func textBytes(_ message: JSONValue) throws -> UInt64 {
        guard case .object(let fields) = message, case .array(let blocks) = fields["content"] else { throw ControllerError.malformedResponse }
        return try blocks.reduce(0) { count, block in
            guard case .object(let value) = block, case .string(let type) = value["type"] else { throw ControllerError.malformedResponse }
            if type != "text" { return count }
            guard case .string(let text) = value["text"] else { throw ControllerError.malformedResponse }
            return count + UInt64(text.utf8.count)
        }
    }
    private static func merge(_ previous: JSONValue?, chunk: JSONValue) throws -> JSONValue {
        guard case .object(var fields) = chunk, case .array(let blocks) = fields["content"] else { throw ControllerError.malformedResponse }
        guard let previous else { return chunk }
        guard case .object(let old) = previous, old["role"] == fields["role"], case .array(var accumulated) = old["content"] else { throw ControllerError.malformedResponse }
        for block in blocks {
            if case .object(let value) = block, value["type"] == .string("text"), case .string(let text) = value["text"],
               case .object(var last) = accumulated.last, last["type"] == .string("text"), case .string(let prior) = last["text"] {
                last["text"] = .string(prior + text)
                accumulated[accumulated.count - 1] = .object(last)
            } else { accumulated.append(block) }
        }
        fields["content"] = .array(accumulated)
        return .object(fields)
    }
}
