import Foundation
import Network
import XCTest
#if canImport(TailscaleKit)
import TailscaleKit
#endif
@testable import GooseRemote

final class EmbeddedMeshTransportTests: XCTestCase {
    struct Fixture: Decodable {
        let fixture: Bool
        let controlURL: URL
        let authKey: String
        let endpoint: URL
        let nonce: String
    }
    func testActualEmbeddedMeshPinnedHTTPWSSAndRecreatedNode() async throws {
        guard let url = Bundle(for: Self.self).url(forResource: "mesh-runtime", withExtension: "json", subdirectory: "Fixtures") else {
            throw XCTSkip("Run build.sh with --mesh-fixture to exercise the real embedded mesh.")
        }
        #if canImport(TailscaleKit)
        let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: url))
        guard fixture.fixture, fixture.controlURL.host == "127.0.0.1", fixture.controlURL.scheme == "http",
              fixture.authKey.hasPrefix("disposable-native-"), fixture.nonce.count == 64 else { throw ControllerError.invalid("Refusing a nonlocal mesh fixture.") }
        setenv("TS_NO_LOGS_NO_SUPPORT", "true", 1)
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("native-mesh-test-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.protectionKey: FileProtectionType.complete])
        defer { try? FileManager.default.removeItem(at: directory) }
        let owner = UUID(uuidString: "11111111-1111-4111-8111-111111111111")!
        let pin = "4379a9ee08e0d4f37685ce498c090125088f046413298a5d2cce99b60a127616"
        func request(websocket: Bool = false) -> URLRequest {
            var parts = URLComponents(url: fixture.endpoint, resolvingAgainstBaseURL: false)!
            if websocket { parts.scheme = "wss"; parts.path = "/events" }
            var request = URLRequest(url: parts.url!)
            request.setValue("Bearer disposable-test-token", forHTTPHeaderField: "Authorization")
            return request
        }
        func makeNode(authKey: String?) throws -> TailscaleNode {
            try TailscaleNode(config: .init(hostName: "native-pinned-controller", path: directory.path, authKey: authKey,
                                           controlURL: fixture.controlURL.absoluteString, ephemeral: false), logger: nil)
        }
        var firstKey: String?
        for iteration in 0..<2 {
            // testcontrol requires its key on every registration, including a known node.
            // Real Headscale resume without a fresh key is a separate provider integration gate.
            let node = try makeNode(authKey: fixture.authKey)
            do {
                try await node.up()
                let key = try EmbeddedMeshConnectionProvider.nodeKey(status: await node.statusJSON())
                if let firstKey { XCTAssertEqual(key, firstKey, "Node reconstruction must preserve its enrolled identity") } else { firstKey = key }
                let configuration = URLSessionConfiguration.ephemeral
                configuration.timeoutIntervalForRequest = 10
                try await configuration.proxyVia(node)
                let transport = PinnedOwnerTransport(configuration: configuration, endpoint: fixture.endpoint, binding: .init(owner: owner, pin: pin))
                do {
                    let (data, response) = try await transport.data(for: request())
                    XCTAssertEqual(response.statusCode, 200)
                    XCTAssertTrue(String(decoding: data, as: UTF8.self).contains(fixture.nonce))
                    var stream = transport.events(for: request(websocket: true)).makeAsyncIterator()
                    let frame = try await stream.next()
                    XCTAssertTrue(String(decoding: try XCTUnwrap(frame), as: UTF8.self).contains(fixture.nonce))
                } catch { transport.close(); throw error }
                transport.close()
                let wrong = PinnedOwnerTransport(configuration: configuration, endpoint: fixture.endpoint, binding: .init(owner: UUID(), pin: pin))
                do { _ = try await wrong.data(for: request()); XCTFail("Mesh membership cannot replace owner identity validation") } catch {}
                wrong.close()
                let brokenConfiguration = URLSessionConfiguration.ephemeral
                brokenConfiguration.timeoutIntervalForRequest = 2
                brokenConfiguration.proxyConfigurations = [ProxyConfiguration(socksv5Proxy: .hostPort(host: "127.0.0.1", port: 9))]
                let broken = PinnedOwnerTransport(configuration: brokenConfiguration, endpoint: fixture.endpoint, binding: .init(owner: owner, pin: pin))
                do { _ = try await broken.data(for: request()); XCTFail("Dead proxy reached the owner") } catch {}
                broken.close()
                try await node.close()
            } catch { try? await node.close(); throw error }
        }
        let directConfig = URLSessionConfiguration.ephemeral
        directConfig.timeoutIntervalForRequest = 2
        directConfig.timeoutIntervalForResource = 3
        let direct = PinnedOwnerTransport(configuration: directConfig, endpoint: fixture.endpoint, binding: .init(owner: owner, pin: pin))
        defer { direct.close() }
        do {
            let (data, _) = try await direct.data(for: request())
            XCTAssertFalse(String(decoding: data, as: UTF8.self).contains(fixture.nonce), "The mesh endpoint escaped the embedded transport")
        } catch {}
        #else
        XCTFail("A configured embedded-mesh test must link TailscaleKit")
        #endif
    }
}
