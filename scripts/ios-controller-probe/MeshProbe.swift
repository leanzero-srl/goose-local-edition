import Darwin
import Foundation
import SwiftUI
import TailscaleKit
import UIKit

#if !targetEnvironment(simulator)
#error("This fixture probe is only for the iOS Simulator.")
#endif

struct MeshFixtureConfig: Decodable, Sendable {
    let schemaVersion: Int
    let fixture: Bool
    let runID: String
    let controllerID: String
    let controlURL: String
    let authKey: String
    let healthURL: URL
    let eventsURL: URL
    let bearerToken: String
    let expectedNonce: String
    let expectedOwnerID: String
    let expectedEpoch: String
    let siblingControllerID: String?
    let siblingBearerToken: String?
    let crashCheckpoint: String?

    func validate() throws {
        guard schemaVersion == 1, fixture,
              Self.isFixtureRandom(runID),
              ["fixture-phone", "fixture-ipad"].contains(controllerID),
              authKey.hasPrefix("goose-ios-disposable-"),
              Self.isFixtureRandom(String(authKey.dropFirst("goose-ios-disposable-".count))),
              !bearerToken.isEmpty, !bearerToken.contains("\r"), !bearerToken.contains("\n"),
              Self.isFixtureRandom(expectedNonce), expectedOwnerID == "fixture-owner",
              Self.isFixtureRandom(expectedEpoch)
        else { throw MeshProbeError("Configuration is not the explicit disposable fixture contract") }
        guard let control = URLComponents(string: controlURL), control.scheme == "http",
              control.host == "127.0.0.1", let port = control.port, (1...65535).contains(port),
              control.user == nil, control.password == nil, control.query == nil,
              control.fragment == nil, ["", "/"].contains(control.path)
        else { throw MeshProbeError("Control URL must be a literal loopback HTTP address with a port") }
        guard let health = URLComponents(url: healthURL, resolvingAgainstBaseURL: false),
              let events = URLComponents(url: eventsURL, resolvingAgainstBaseURL: false),
              health.scheme == "https", events.scheme == "wss",
              health.host == events.host, health.port == events.port,
              let peerPort = health.port, (1...65535).contains(peerPort),
              health.path == "/health", events.path == "/events",
              health.user == nil, health.password == nil, health.query == nil, health.fragment == nil,
              events.user == nil, events.password == nil, events.query == nil, events.fragment == nil,
              let host = health.host, Self.isMeshIPv4(host)
        else { throw MeshProbeError("Peer URLs must be HTTPS and WSS on the same literal mesh IPv4 authority") }
        switch (siblingControllerID, siblingBearerToken) {
        case (nil, nil): break
        case (let siblingID?, let siblingToken?):
            guard ["fixture-phone", "fixture-ipad"].contains(siblingID), siblingID != controllerID,
                  !siblingToken.isEmpty, siblingToken != bearerToken,
                  !siblingToken.contains("\r"), !siblingToken.contains("\n") else {
                throw MeshProbeError("Sibling ownership control must use the other disposable controller and token")
            }
        default: throw MeshProbeError("Sibling controller ID and token must be supplied together")
        }
        if let crashCheckpoint {
            guard crashCheckpoint == "after-command-transport-before-journal", siblingControllerID != nil else {
                throw MeshProbeError("The deliberate crash checkpoint requires the command fixture checks")
            }
        }
    }

    private static func isFixtureRandom(_ value: String) -> Bool {
        value.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil
    }

    private static func isMeshIPv4(_ value: String) -> Bool {
        let parts = value.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 4 else { return false }
        let octets = parts.compactMap { Int($0) }
        guard octets.count == 4, octets.allSatisfy({ (0...255).contains($0) }),
              zip(parts, octets).allSatisfy({ String($0.0) == String($0.1) }) else { return false }
        return octets[0] == 100 && (64...127).contains(octets[1])
    }
}

struct MeshProbeError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

struct DiscardMeshLogs: LogSink {
    let logFileHandle: Int32? = -1
    func log(_ message: String) {}
}

final class RejectFixtureRedirects: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(_ session: URLSession, task: URLSessionTask,
                    willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest,
                    completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}

struct MeshHealth: Codable {
    let type: String
    let runID: String
    let nonce: String
    let ownerID: String
    let epoch: String
    let deviceID: String
}

struct MeshEvent: Codable {
    struct Payload: Codable {
        let nonce: String
        let deviceID: String
        let command: MeshCommand?
    }
    let type: String
    let runID: String
    let ownerID: String
    let epoch: String
    let sequence: Int
    let payload: Payload
}

struct MeshCommand: Codable, Equatable, Sendable {
    let commandID: String
    let kind: String
    let state: String
    let deviceID: String
    let deliveryCount: Int
    let acceptedAt: String
}

struct MeshCommandResponse: Codable {
    let type: String
    let runID: String
    let ownerID: String
    let epoch: String
    let nonce: String
    let command: MeshCommand
    let duplicate: Bool
}

struct MeshCommandRequest: Encodable {
    struct Payload: Codable { let text: String }
    let commandID: String
    let kind: String
    let payload: Payload
    let dropAcknowledgement: Bool
}

struct MeshCommandJournal: Codable {
    let runID: String
    let controllerID: String
    let commandID: String
    let kind: String
    let text: String
    let replayCursor: Int
    var interruption: [String: String]?
}

enum MeshCheckState: String, Encodable { case pending, running, passed, failed, skipped, unproven }

struct MeshCheck: Encodable, Identifiable {
    let id: String
    let title: String
    var state: MeshCheckState = .pending
    var observations: [String: String] = [:]
    var updatedAt: Date?
}

struct MeshReceipt: Encodable {
    let schemaVersion = 1
    let scope = "Disposable simulator userspace mesh transport fixture"
    let startedAt: Date
    let measuredAt: Date
    let finished: Bool
    let passed: Bool
    let runID: String?
    let controllerID: String?
    let systemVersion: String
    let deviceFamily: String
    let simulatorModel: String?
    let frameworkSHA256: String?
    let sourceSHA256: String?
    let healthEndpoint: String?
    let eventsEndpoint: String?
    let commandTestingEnabled: Bool
    let requestedCrashCheckpoint: String?
    let checks: [MeshCheck]
}

@MainActor
final class MeshProbeModel: ObservableObject {
    @Published private(set) var checks = [
        MeshCheck(id: "config", title: "Validate injected fixture configuration"),
        MeshCheck(id: "node", title: "Join disposable mesh with fresh state"),
        MeshCheck(id: "proxy", title: "Configure the node's SOCKS transport"),
        MeshCheck(id: "health", title: "Read HTTPS owner identity and random nonce"),
        MeshCheck(id: "websocket", title: "Receive the WSS identity envelope"),
        MeshCheck(id: "wrong-token", title: "Reject a wrong token on the same endpoint"),
        MeshCheck(id: "no-proxy", title: "Direct URLSession must not obtain the nonce"),
        MeshCheck(id: "close", title: "Close and discard this controller node"),
        MeshCheck(id: "state", title: "Remove this probe's ephemeral node state")
    ]
    @Published private(set) var receiptError: String?
    @Published private(set) var finished = false
    private let startedAt = Date()
    private var config: MeshFixtureConfig?
    private var node: TailscaleNode?
    private var session: URLSession?
    private var socket: URLSessionWebSocketTask?
    private var stateURL: URL?
    private var didRun = false
    private let redirectDelegate = RejectFixtureRedirects()

    var passed: Bool { finished && checks.allSatisfy { $0.state == .passed } && receiptError == nil }
    var summary: String {
        guard finished else { return "Mesh probe running" }
        if passed { return "Mesh probe passed" }
        if checks.contains(where: { $0.state == .failed }) || receiptError != nil { return "Mesh probe failed" }
        return "Mesh probe has unproven checks"
    }

    func run() async {
        guard !didRun else { return }
        didRun = true
        var active = "config"
        do {
            update(active, .running)
            let documents = try FileManager.default.url(for: .documentDirectory, in: .userDomainMask,
                                                        appropriateFor: nil, create: true)
            let loaded = try JSONDecoder().decode(MeshFixtureConfig.self,
                from: Data(contentsOf: documents.appendingPathComponent("mesh-config.json")))
            try loaded.validate()
            config = loaded
            if loaded.siblingControllerID != nil {
                checks.insert(contentsOf: [
                    MeshCheck(id: "command-journal", title: "Persist command identity before sending"),
                    MeshCheck(id: "lost-ack", title: "Observe the deliberately lost acknowledgement"),
                    MeshCheck(id: "command-status", title: "Recover acceptance from the owner's command status"),
                    MeshCheck(id: "command-retry", title: "Retry the same command without a second delivery"),
                    MeshCheck(id: "body-conflict", title: "Reject a changed body for the same command ID"),
                    MeshCheck(id: "token-ownership", title: "Reject command access with the sibling token"),
                    MeshCheck(id: "replay", title: "Replay the missed command event after reconnecting")
                ], at: 5)
            }
            update(active, .passed, ["runID": loaded.runID, "controllerID": loaded.controllerID,
                                     "controlURL": loaded.controlURL])

            active = "node"
            update(active, .running)
            let freshState = FileManager.default.temporaryDirectory
                .appendingPathComponent("mesh-probe-\(UUID().uuidString)", isDirectory: true)
            try FileManager.default.createDirectory(at: freshState, withIntermediateDirectories: false)
            stateURL = freshState
            let hostName = "\(loaded.controllerID)-\(UUID().uuidString.prefix(8).lowercased())"
            let created = try TailscaleNode(config: Configuration(hostName: hostName,
                path: freshState.path, authKey: loaded.authKey, controlURL: loaded.controlURL,
                ephemeral: true), logger: DiscardMeshLogs())
            node = created
            try await awaitReadiness(created)
            let addresses = try await created.addrs()
            guard addresses.ip4 != nil || addresses.ip6 != nil else {
                throw MeshProbeError("Node became ready without reporting any assigned address")
            }
            var nodeObservations = ["hostname": hostName, "ephemeral": "true", "freshState": "true"]
            if let ip4 = addresses.ip4 { nodeObservations["IPv4"] = ip4 }
            if let ip6 = addresses.ip6 { nodeObservations["IPv6"] = ip6 }
            update(active, .passed, nodeObservations)

            active = "proxy"
            update(active, .running)
            let settings = transportConfiguration()
            try await settings.proxyVia(created)
            guard settings.proxyConfigurations.count == 1 else {
                throw MeshProbeError("Expected one actual SOCKS proxy configuration")
            }
            let proxied = URLSession(configuration: settings, delegate: redirectDelegate, delegateQueue: nil)
            session = proxied
            update(active, .passed, ["proxyConfigurations": String(settings.proxyConfigurations.count),
                                     "TLS": "URLSession default trust", "redirects": "rejected"])

            active = "health"
            update(active, .running)
            let (healthData, healthResponse) = try await proxied.data(for: request(loaded.healthURL, config: loaded))
            let healthStatus = try status(healthResponse)
            update(active, .running, ["HTTPStatus": String(healthStatus), "byteCount": String(healthData.count)])
            guard healthStatus == 200 else { throw MeshProbeError("Health returned HTTP \(healthStatus)") }
            let health = try JSONDecoder().decode(MeshHealth.self, from: healthData)
            let healthValues = ["HTTPStatus": String(healthStatus), "type": health.type,
                "runID": health.runID, "nonce": health.nonce, "ownerID": health.ownerID,
                "epoch": health.epoch, "deviceID": health.deviceID]
            update(active, .running, healthValues)
            guard health.type == "health", health.runID == loaded.runID,
                  health.nonce == loaded.expectedNonce, health.ownerID == loaded.expectedOwnerID,
                  health.epoch == loaded.expectedEpoch, health.deviceID == loaded.controllerID
            else { throw MeshProbeError("Health identity or nonce does not match this fixture") }
            update(active, .passed, healthValues)

            active = "websocket"
            update(active, .running)
            let stream = proxied.webSocketTask(with: request(loaded.eventsURL, config: loaded))
            socket = stream
            stream.resume()
            let event = try await receiveEvent(stream)
            let eventValues = eventObservations(event)
            update(active, .running, eventValues)
            try validate(event, config: loaded)
            guard event.type == "connected", event.sequence > 0 else {
                throw MeshProbeError("Initial event must be connected with a positive sequence")
            }
            update(active, .passed, eventValues)
            stream.cancel(with: .normalClosure, reason: nil)
            socket = nil
            if loaded.siblingControllerID != nil {
                await runCommandChecks(config: loaded, session: proxied, cursor: event.sequence)
            }

            active = "wrong-token"
            update(active, .running)
            var rejectedRequest = request(loaded.healthURL, config: loaded)
            rejectedRequest.setValue("Bearer fixture-wrong-\(UUID().uuidString)", forHTTPHeaderField: "Authorization")
            let (rejectedData, rejectedResponse) = try await proxied.data(for: rejectedRequest)
            let rejectedStatus = try status(rejectedResponse)
            let rejectedNonce = rejectedData.range(of: Data(loaded.expectedNonce.utf8)) != nil
            let rejectedValues = ["HTTPStatus": String(rejectedStatus), "noncePresent": String(rejectedNonce)]
            update(active, .running, rejectedValues)
            guard rejectedStatus == 401, !rejectedNonce else {
                throw MeshProbeError("Wrong-token request must return 401 without the nonce")
            }
            update(active, .passed, rejectedValues)

            active = "no-proxy"
            update(active, .running)
            let directSettings = transportConfiguration()
            directSettings.proxyConfigurations = []
            let direct = URLSession(configuration: directSettings, delegate: redirectDelegate, delegateQueue: nil)
            defer { direct.invalidateAndCancel() }
            let directResult: (Data, URLResponse)?
            do {
                directResult = try await direct.data(for: request(loaded.healthURL, config: loaded))
            } catch {
                if Task.isCancelled || (error as NSError).code == NSURLErrorCancelled { throw error }
                directResult = nil
                update(active, .passed, errorObservations(error).merging([
                    "nonceObtained": "false", "proxyConfigurations": "0"
                ]) { _, new in new })
            }
            if let (data, response) = directResult {
                let noncePresent = data.range(of: Data(loaded.expectedNonce.utf8)) != nil
                let values = ["HTTPStatus": String(try status(response)), "nonceObtained": String(noncePresent),
                              "proxyConfigurations": "0", "byteCount": String(data.count)]
                update(active, .running, values)
                guard !noncePresent else {
                    throw MeshProbeError("The direct session obtained the nonce; mesh route isolation is not proved")
                }
                update(active, .passed, values)
            }
        } catch {
            update(active, .failed, errorObservations(error))
        }
        await cleanUp()
        for index in checks.indices where checks[index].state == .pending {
            checks[index].state = .skipped
            checks[index].observations = ["reason": "A prerequisite failed before this check ran"]
            checks[index].updatedAt = Date()
        }
        finished = true
        saveReceipt()
    }

    private func transportConfiguration() -> URLSessionConfiguration {
        let value = URLSessionConfiguration.ephemeral
        value.timeoutIntervalForRequest = 15
        value.timeoutIntervalForResource = 20
        value.requestCachePolicy = .reloadIgnoringLocalCacheData
        value.urlCache = nil
        value.httpCookieStorage = nil
        return value
    }

    private func runCommandChecks(config: MeshFixtureConfig, session: URLSession, cursor: Int) async {
        var active = "command-journal"
        do {
            update(active, .running)
            let journalURL = try FileManager.default.url(for: .documentDirectory, in: .userDomainMask,
                appropriateFor: nil, create: true).appendingPathComponent("mesh-command-\(config.controllerID).json")
            let journal: MeshCommandJournal
            let reused: Bool
            if FileManager.default.fileExists(atPath: journalURL.path) {
                let saved = try JSONDecoder().decode(MeshCommandJournal.self, from: Data(contentsOf: journalURL))
                if saved.runID == config.runID {
                    guard saved.controllerID == config.controllerID,
                          UUID(uuidString: saved.commandID) != nil, saved.kind == "fixture-command",
                          saved.text == "Mesh probe command for \(config.controllerID)", saved.replayCursor > 0 else {
                        throw MeshProbeError("Persisted command journal is invalid for this controller")
                    }
                    journal = saved
                    reused = true
                } else {
                    journal = newJournal(config: config, cursor: cursor)
                    reused = false
                }
            } else {
                journal = newJournal(config: config, cursor: cursor)
                reused = false
            }
            try persist(journal, at: journalURL)
            let readBack = try JSONDecoder().decode(MeshCommandJournal.self, from: Data(contentsOf: journalURL))
            guard readBack.commandID == journal.commandID, readBack.runID == config.runID else {
                throw MeshProbeError("Durable command readback does not match the command being sent")
            }
            update(active, .passed, ["commandID": journal.commandID, "replayCursor": String(journal.replayCursor),
                                     "reusedPersistedIdentity": String(reused), "journalReadBack": "matched"])

            let endpoint = commandEndpoint(config, commandID: journal.commandID)
            var recovered: MeshCommandResponse?
            if reused {
                guard let index = checks.firstIndex(where: { $0.id == "lost-ack" }) else {
                    throw MeshProbeError("Command checks are missing from the receipt")
                }
                checks.insert(MeshCheck(id: "command-recovery", title: "Reconcile the saved command before resubmitting"), at: index)
                active = "command-recovery"
                update(active, .running, ["commandID": journal.commandID, "mutationSentThisLaunch": "false"])
                let (data, response) = try await session.data(for: request(endpoint, config: config))
                let code = try status(response)
                update(active, .running, ["HTTPStatus": String(code)])
                switch code {
                case 200:
                    let existing = try JSONDecoder().decode(MeshCommandResponse.self, from: data)
                    try validate(existing, journal: journal, config: config)
                    guard !existing.duplicate else { throw MeshProbeError("Recovery status lookup reported a duplicate submission") }
                    recovered = existing
                    update(active, .passed, commandObservations(existing.command).merging([
                        "result": "Owner confirms the saved command was already accepted"
                    ]) { _, new in new })
                case 404:
                    update(active, .passed, ["result": "Owner reports no accepted command; continue with the same saved identity"])
                default:
                    throw MeshProbeError("Cannot reconcile the saved command: HTTP \(code)")
                }
            }

            active = "lost-ack"
            update(active, .running)
            if recovered != nil {
                var history = ["source": "no current-launch acknowledgement loss",
                    "reason": "Owner acceptance predates this launch; recovery succeeded independently",
                    "priorInterruptionRecorded": String(journal.interruption != nil)]
                if let interruption = journal.interruption {
                    for (key, value) in interruption { history["prior_\(key)"] = value }
                }
                update(active, .unproven, history)
            } else {
                let first = try commandRequest(config: config, journal: journal, dropAcknowledgement: true)
                var interruption: [String: String]?
                do {
                    let (_, response) = try await session.data(for: first)
                    update(active, .running, ["HTTPStatus": String(try status(response)), "acknowledgementReceived": "true"])
                } catch {
                    if Task.isCancelled || (error as NSError).code == NSURLErrorCancelled { throw error }
                    interruption = errorObservations(error)
                }
                if let interruption {
                    if config.crashCheckpoint == "after-command-transport-before-journal" {
                        let beforeCrash = try JSONDecoder().decode(MeshCommandJournal.self, from: Data(contentsOf: journalURL))
                        guard beforeCrash.interruption == nil else {
                            throw MeshProbeError("The deliberate crash checkpoint requires an unrecorded acknowledgement result")
                        }
                        update(active, .unproven, ["checkpoint": "after-command-transport-before-journal",
                            "commandID": journal.commandID, "journalInterruptionPresent": "false",
                            "plannedProcessExitCode": "86", "reason": "Deliberate process death before recording acknowledgement outcome"])
                        guard receiptError == nil else { throw MeshProbeError("Cannot persist the deliberate crash checkpoint receipt") }
                        Darwin._exit(86)
                    }
                    var interruptedJournal = journal
                    interruptedJournal.interruption = interruption
                    try persist(interruptedJournal, at: journalURL)
                    update(active, .passed, interruption.merging(["source": "this launch", "acknowledgementReceived": "false"])
                        { _, new in new })
                } else {
                    update(active, .failed, ["reason": "The first command received HTTP headers; this launch did not observe a lost acknowledgement"])
                }
            }

            active = "command-status"
            update(active, .running)
            let accepted: MeshCommandResponse
            if let recovered {
                accepted = recovered
                update(active, .running, ["HTTPStatus": "200", "source": "current-launch status query before resubmission"])
            } else {
                let (statusData, statusResponse) = try await session.data(for: request(endpoint, config: config))
                let statusCode = try status(statusResponse)
                update(active, .running, ["HTTPStatus": String(statusCode), "source": "current-launch query after initial send"])
                guard statusCode == 200 else { throw MeshProbeError("Command status did not recover an accepted command") }
                accepted = try JSONDecoder().decode(MeshCommandResponse.self, from: statusData)
            }
            update(active, .running, commandObservations(accepted.command))
            try validate(accepted, journal: journal, config: config)
            guard !accepted.duplicate else { throw MeshProbeError("Status lookup was incorrectly described as a duplicate submission") }
            update(active, .passed, commandObservations(accepted.command))

            active = "command-retry"
            update(active, .running)
            let retry = try commandRequest(config: config, journal: journal, dropAcknowledgement: false)
            let (retryData, retryResponse) = try await session.data(for: retry)
            let retryCode = try status(retryResponse)
            update(active, .running, ["HTTPStatus": String(retryCode)])
            guard retryCode == 200 else { throw MeshProbeError("Duplicate command did not return HTTP 200") }
            let duplicate = try JSONDecoder().decode(MeshCommandResponse.self, from: retryData)
            update(active, .running, commandObservations(duplicate.command).merging(["duplicate": String(duplicate.duplicate)])
                { _, new in new })
            try validate(duplicate, journal: journal, config: config)
            guard duplicate.duplicate, duplicate.command == accepted.command else {
                throw MeshProbeError("Retry did not retain the original command and its single delivery")
            }
            update(active, .passed)

            active = "body-conflict"
            update(active, .running)
            let conflicting = try commandRequest(config: config, journal: journal, dropAcknowledgement: false,
                text: journal.text + " changed")
            let (_, conflictResponse) = try await session.data(for: conflicting)
            let conflictCode = try status(conflictResponse)
            update(active, .running, ["HTTPStatus": String(conflictCode)])
            guard conflictCode == 409 else { throw MeshProbeError("Changed command body did not return HTTP 409") }
            update(active, .passed)

            active = "token-ownership"
            update(active, .running)
            guard let siblingToken = config.siblingBearerToken, let siblingID = config.siblingControllerID else {
                throw MeshProbeError("The sibling ownership control is missing")
            }
            var siblingPost = retry
            siblingPost.setValue("Bearer \(siblingToken)", forHTTPHeaderField: "Authorization")
            let (_, siblingPostResponse) = try await session.data(for: siblingPost)
            let siblingPostCode = try status(siblingPostResponse)
            var siblingGet = request(endpoint, config: config)
            siblingGet.setValue("Bearer \(siblingToken)", forHTTPHeaderField: "Authorization")
            let (siblingData, siblingGetResponse) = try await session.data(for: siblingGet)
            let siblingGetCode = try status(siblingGetResponse)
            let commandExposed = siblingData.range(of: Data(journal.commandID.utf8)) != nil
            update(active, .running, ["siblingControllerID": siblingID, "POSTStatus": String(siblingPostCode),
                "GETStatus": String(siblingGetCode), "commandIDExposed": String(commandExposed),
                "scope": "token ownership conflict; not simultaneous two-device contention"])
            guard siblingPostCode == 409, siblingGetCode == 404, !commandExposed else {
                throw MeshProbeError("Sibling token did not receive the expected command ownership denials")
            }
            let (finalData, finalResponse) = try await session.data(for: request(endpoint, config: config))
            guard try status(finalResponse) == 200 else { throw MeshProbeError("Owner status readback failed after conflicts") }
            let finalStatus = try JSONDecoder().decode(MeshCommandResponse.self, from: finalData)
            try validate(finalStatus, journal: journal, config: config)
            guard finalStatus.command == accepted.command else {
                throw MeshProbeError("Rejected requests changed the accepted command or its delivery count")
            }
            update(active, .passed, ["ownerDeliveryCountAfterConflicts": String(finalStatus.command.deliveryCount)])

            active = "replay"
            update(active, .running)
            guard var replayURL = URLComponents(url: config.eventsURL, resolvingAgainstBaseURL: false) else {
                throw MeshProbeError("Could not construct the replay URL")
            }
            replayURL.queryItems = [URLQueryItem(name: "after", value: String(journal.replayCursor))]
            guard let replayEndpoint = replayURL.url else { throw MeshProbeError("Replay URL is invalid") }
            let replay = session.webSocketTask(with: request(replayEndpoint, config: config))
            socket = replay
            replay.resume()
            let missed = try await receiveEvent(replay)
            update(active, .running, eventObservations(missed).merging(["requestedAfter": String(journal.replayCursor)])
                { _, new in new })
            try validate(missed, config: config)
            guard missed.type == "command_accepted", missed.sequence > journal.replayCursor,
                  missed.payload.command == accepted.command else {
                throw MeshProbeError("Reconnect did not replay the original accepted command after its saved cursor")
            }
            let connected = try await receiveEvent(replay)
            try validate(connected, config: config)
            guard connected.type == "connected", connected.sequence > missed.sequence else {
                throw MeshProbeError("Reconnect did not follow the replay with an increasing connected envelope")
            }
            update(active, .passed, ["replayedCommandID": accepted.command.commandID,
                "replayedSequence": String(missed.sequence), "connectedSequence": String(connected.sequence)])
            replay.cancel(with: .normalClosure, reason: nil)
            socket = nil
        } catch { update(active, .failed, errorObservations(error)) }
    }

    private func newJournal(config: MeshFixtureConfig, cursor: Int) -> MeshCommandJournal {
        MeshCommandJournal(runID: config.runID, controllerID: config.controllerID,
            commandID: UUID().uuidString, kind: "fixture-command",
            text: "Mesh probe command for \(config.controllerID)", replayCursor: cursor)
    }

    private func persist(_ journal: MeshCommandJournal, at url: URL) throws {
        try JSONEncoder().encode(journal).write(to: url, options: .atomic)
    }

    private func commandEndpoint(_ config: MeshFixtureConfig, commandID: String? = nil) -> URL {
        let commands = config.healthURL.deletingLastPathComponent().appendingPathComponent("commands")
        if let commandID { return commands.appendingPathComponent(commandID) }
        return commands
    }

    private func commandRequest(config: MeshFixtureConfig, journal: MeshCommandJournal,
                                dropAcknowledgement: Bool, text: String? = nil) throws -> URLRequest {
        var value = request(commandEndpoint(config), config: config)
        value.httpMethod = "POST"
        value.setValue("application/json", forHTTPHeaderField: "Content-Type")
        value.httpBody = try JSONEncoder().encode(MeshCommandRequest(commandID: journal.commandID, kind: journal.kind,
            payload: .init(text: text ?? journal.text), dropAcknowledgement: dropAcknowledgement))
        return value
    }

    private func validate(_ response: MeshCommandResponse, journal: MeshCommandJournal,
                          config: MeshFixtureConfig) throws {
        guard response.type == "command", response.runID == config.runID,
              response.ownerID == config.expectedOwnerID, response.epoch == config.expectedEpoch,
              response.nonce == config.expectedNonce, response.command.commandID == journal.commandID,
              response.command.kind == journal.kind, response.command.state == "accepted",
              response.command.deviceID == config.controllerID, response.command.deliveryCount == 1,
              !response.command.acceptedAt.isEmpty else {
            throw MeshProbeError("Command identity, state or single delivery count does not match the fixture")
        }
    }

    private func commandObservations(_ command: MeshCommand) -> [String: String] {
        ["commandID": command.commandID, "kind": command.kind, "state": command.state,
         "deviceID": command.deviceID, "deliveryCount": String(command.deliveryCount), "acceptedAt": command.acceptedAt]
    }

    private func request(_ url: URL, config: MeshFixtureConfig) -> URLRequest {
        var value = URLRequest(url: url)
        value.setValue("Bearer \(config.bearerToken)", forHTTPHeaderField: "Authorization")
        value.setValue("application/json", forHTTPHeaderField: "Accept")
        return value
    }

    private func status(_ response: URLResponse) throws -> Int {
        guard let http = response as? HTTPURLResponse else { throw MeshProbeError("Response is not HTTP") }
        return http.statusCode
    }

    private func receiveEvent(_ stream: URLSessionWebSocketTask) async throws -> MeshEvent {
        try await withThrowingTaskGroup(of: MeshEvent.self) { group in
            group.addTask {
                let message = try await stream.receive()
                let data: Data
                switch message {
                case .string(let text): data = Data(text.utf8)
                case .data(let bytes): data = bytes
                @unknown default: throw MeshProbeError("Unsupported WebSocket message format")
                }
                return try JSONDecoder().decode(MeshEvent.self, from: data)
            }
            group.addTask {
                try await Task.sleep(for: .seconds(20))
                stream.cancel(with: .goingAway, reason: nil)
                throw MeshProbeError("Fixture WebSocket did not deliver its expected envelope within 20 seconds")
            }
            defer { group.cancelAll() }
            guard let first = try await group.next() else { throw MeshProbeError("WebSocket receive had no result") }
            return first
        }
    }

    private func validate(_ event: MeshEvent, config: MeshFixtureConfig) throws {
        guard event.runID == config.runID, event.ownerID == config.expectedOwnerID,
              event.epoch == config.expectedEpoch, event.payload.nonce == config.expectedNonce,
              event.payload.deviceID == config.controllerID else {
            throw MeshProbeError("Event identity or nonce does not match this controller and fixture")
        }
    }

    private func eventObservations(_ event: MeshEvent) -> [String: String] {
        ["type": event.type, "runID": event.runID, "ownerID": event.ownerID, "epoch": event.epoch,
         "sequence": String(event.sequence), "nonce": event.payload.nonce, "deviceID": event.payload.deviceID]
    }

    private func awaitReadiness(_ created: TailscaleNode) async throws {
        try await withTaskCancellationHandler {
            try await withThrowingTaskGroup(of: Void.self) { group in
                group.addTask { try await created.up() }
                group.addTask {
                    try await Task.sleep(for: .seconds(30))
                    await self.closeControllerNode()
                    throw MeshProbeError("Disposable node did not become ready within 30 seconds")
                }
                defer { group.cancelAll() }
                _ = try await group.next()
            }
        } onCancel: {
            Task { await self.closeControllerNode() }
        }
    }

    private func closeControllerNode() async {
        guard let closing = node else { return }
        node = nil
        update("close", .running)
        do {
            try await closing.close()
            update("close", .passed, ["result": "local controller close() returned successfully"])
        } catch { update("close", .failed, errorObservations(error)) }
    }

    private func cleanUp() async {
        socket?.cancel(with: .normalClosure, reason: nil)
        socket = nil
        session?.invalidateAndCancel()
        session = nil
        await closeControllerNode()
        if let removing = stateURL {
            guard checks.first(where: { $0.id == "close" })?.state == .passed else {
                update("state", .skipped, ["reason": "Node close did not succeed; state retained for diagnosis"])
                return
            }
            update("state", .running)
            do {
                try FileManager.default.removeItem(at: removing)
                guard !FileManager.default.fileExists(atPath: removing.path) else {
                    throw MeshProbeError("Ephemeral node state is still present")
                }
                stateURL = nil
                update("state", .passed, ["stateDirectoryExists": "false"])
            } catch { update("state", .failed, errorObservations(error)) }
        }
    }

    private func errorObservations(_ error: Error) -> [String: String] {
        let value = error as NSError
        var message = value.localizedDescription
        if let config {
            message = message.replacingOccurrences(of: config.bearerToken, with: "[redacted]")
            message = message.replacingOccurrences(of: config.authKey, with: "[redacted]")
            if let siblingToken = config.siblingBearerToken {
                message = message.replacingOccurrences(of: siblingToken, with: "[redacted]")
            }
        }
        return ["errorDomain": value.domain, "errorCode": String(value.code), "error": message]
    }

    private func update(_ id: String, _ state: MeshCheckState, _ observations: [String: String] = [:]) {
        guard let index = checks.firstIndex(where: { $0.id == id }) else { preconditionFailure("Unknown probe check") }
        checks[index].state = state
        checks[index].observations.merge(observations) { _, new in new }
        checks[index].updatedAt = Date()
        saveReceipt()
    }

    private func saveReceipt() {
        do {
            let device = UIDevice.current
            let receipt = MeshReceipt(startedAt: startedAt, measuredAt: Date(), finished: finished,
                passed: finished && checks.allSatisfy { $0.state == .passed },
                runID: config?.runID, controllerID: config?.controllerID,
                systemVersion: device.systemVersion,
                deviceFamily: device.userInterfaceIdiom == .pad ? "iPad" : "iPhone",
                simulatorModel: ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"],
                frameworkSHA256: Bundle.main.object(forInfoDictionaryKey: "ProbeFrameworkSHA256") as? String,
                sourceSHA256: Bundle.main.object(forInfoDictionaryKey: "ProbeSourceSHA256") as? String,
                healthEndpoint: config?.healthURL.absoluteString, eventsEndpoint: config?.eventsURL.absoluteString,
                commandTestingEnabled: config?.siblingControllerID != nil,
                requestedCrashCheckpoint: config?.crashCheckpoint,
                checks: checks)
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            encoder.dateEncodingStrategy = .iso8601
            let documents = try FileManager.default.url(for: .documentDirectory, in: .userDomainMask,
                                                        appropriateFor: nil, create: true)
            try encoder.encode(receipt).write(to: documents.appendingPathComponent("mesh-receipt.json"), options: .atomic)
            receiptError = nil
        } catch { receiptError = error.localizedDescription }
    }
}

@main
struct GooseMeshProbeApp: App {
    @StateObject private var model = MeshProbeModel()

    var body: some Scene {
        WindowGroup {
            NavigationStack {
                List {
                    Section {
                        Text(model.summary)
                            .font(.headline)
                        Text("Disposable simulator fixture. HTTPS and WSS use the embedded node's SOCKS proxy with ordinary TLS validation.")
                            .foregroundStyle(.secondary)
                    }
                    Section("Observed checks") {
                        ForEach(model.checks) { check in
                            VStack(alignment: .leading, spacing: 6) {
                                Label(check.title, systemImage: icon(check.state))
                                Text(check.state.rawValue.uppercased()).font(.caption.bold()).foregroundStyle(color(check.state))
                                ForEach(check.observations.keys.sorted(), id: \.self) { key in
                                    if let value = check.observations[key] {
                                        Text("\(key): \(value)").font(.caption.monospaced()).textSelection(.enabled)
                                    }
                                }
                            }
                            .accessibilityElement(children: .combine)
                        }
                    }
                    Section("Receipt") {
                        Text("Documents/mesh-receipt.json updates after every check.")
                        if let error = model.receiptError { Text(error).foregroundStyle(.red) }
                    }
                }
                .navigationTitle("Goose Mesh Probe")
                .task { await model.run() }
            }
        }
    }

    private func icon(_ state: MeshCheckState) -> String {
        switch state {
        case .pending: "circle"
        case .running: "arrow.triangle.2.circlepath"
        case .passed: "checkmark.circle.fill"
        case .failed: "xmark.circle.fill"
        case .skipped: "minus.circle"
        case .unproven: "questionmark.circle"
        }
    }

    private func color(_ state: MeshCheckState) -> Color {
        switch state {
        case .passed: .green
        case .failed: .red
        case .running: .blue
        case .unproven: .orange
        case .pending, .skipped: .secondary
        }
    }
}
