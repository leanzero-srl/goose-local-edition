import Foundation
import XCTest
@testable import GooseRemote

final class MeshPeerReadinessTests: XCTestCase {
    private let endpoint = URL(string: "https://100.64.0.1:10002")!
    private let starting = Data(#"{"BackendState":"Running","Peer":{}}"#.utf8)
    private let ready = Data(#"{"BackendState":"Running","Peer":{"nodekey:owner":{"Online":true,"TailscaleIPs":["100.64.0.1"]}}}"#.utf8)

    func testRunningWithoutOwnerDoesNotBeginDialAndActualPeerReleasesWait() async throws {
        let sequence = StatusSequence(values: [starting, ready])
        try await MeshPeerReadiness.waitForOwner(endpoint: endpoint, timeout: 2) { await sequence.next() }
        let calls = await sequence.calls
        XCTAssertEqual(calls, 2)
    }
    func testOtherOnlinePeerAndOfflineOwnerCannotSatisfyReadiness() throws {
        let other = Data(#"{"BackendState":"Running","Peer":{"other":{"Online":true,"TailscaleIPs":["100.64.0.9"]},"owner":{"Online":false,"TailscaleIPs":["100.64.0.1"]}}}"#.utf8)
        XCTAssertFalse(try MeshPeerReadiness.containsOwner(other, endpoint: endpoint))
        XCTAssertTrue(try MeshPeerReadiness.containsOwner(ready, endpoint: endpoint))
        let ipv6 = Data(#"{"BackendState":"Running","Peer":{"owner":{"Online":true,"TailscaleIPs":["fd7a:115c:a1e0::1"]}}}"#.utf8)
        XCTAssertTrue(try MeshPeerReadiness.containsOwner(ipv6, endpoint: URL(string: "https://[fd7a:115c:a1e0:0:0:0:0:1]:10002")!))
    }
    func testNoOwnerEndsAtTransportDeadlineWithoutDialFallback() async {
        let data = starting
        do { try await MeshPeerReadiness.waitForOwner(endpoint: endpoint, timeout: 0) { data }; XCTFail("Missing owner reported ready") }
        catch { XCTAssertTrue(error.localizedDescription.contains("has not appeared")) }
    }
}

private actor StatusSequence {
    let values: [Data]
    private(set) var calls = 0
    init(values: [Data]) { self.values = values }
    func next() -> Data { defer { calls += 1 }; return values[min(calls, values.count - 1)] }
}
