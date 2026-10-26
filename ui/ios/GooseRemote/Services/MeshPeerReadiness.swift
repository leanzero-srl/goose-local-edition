import Foundation
import Network

enum MeshPeerReadiness {
    private struct Status: Decodable {
        struct NodeStatus: Decodable {
            let Online: Bool?
            let TailscaleIPs: [String]?
        }
        let BackendState: String
        let Peer: [String: NodeStatus]?
    }

    static func containsOwner(_ data: Data, endpoint: URL) throws -> Bool {
        guard MeshAddress.contains(endpoint.host), let host = endpoint.host else { throw ControllerError.malformedResponse }
        let status = try JSONDecoder().decode(Status.self, from: data)
        guard status.BackendState == "Running" else {
            throw ControllerError.unavailable("The embedded mesh is not connected. Reconnect Link before opening this Mac.")
        }
        let target = host.trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
        return status.Peer?.values.contains { peer in
            peer.Online == true && peer.TailscaleIPs?.contains { address in
                if let first = IPv4Address(target), let second = IPv4Address(address) { return first == second }
                if let first = IPv6Address(target), let second = IPv6Address(address) { return first == second }
                return false
            } == true
        } == true
    }

    static func waitForOwner(endpoint: URL, timeout: TimeInterval, status: @Sendable () async throws -> Data) async throws {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: .seconds(timeout))
        while true {
            try Task.checkCancellation()
            if try containsOwner(await status(), endpoint: endpoint) { return }
            guard clock.now < deadline else {
                throw ControllerError.unavailable("This Mac has not appeared in the mesh yet. Check that it is connected and try again.")
            }
            // Headscale can report Running before delivering the first peer map after resume.
            try await Task.sleep(for: .milliseconds(100))
        }
    }
}
