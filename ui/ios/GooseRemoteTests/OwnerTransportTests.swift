import CryptoKit
import Foundation
import Network
import Security
import XCTest
@testable import GooseRemote

final class OwnerTransportTests: XCTestCase {
    let owner = UUID(uuidString: "11111111-1111-4111-8111-111111111111")!
    func certificate(_ name: String) throws -> Data {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: name, withExtension: "der", subdirectory: "Fixtures"))
        return try Data(contentsOf: url)
    }
    func pin() throws -> String {
        SHA256.hash(data: try DERCertificate(data: certificate("valid")).subjectPublicKeyInfo).map { String(format: "%02x", $0) }.joined()
    }
    func trust(_ name: String) throws -> SecTrust {
        let cert = try XCTUnwrap(SecCertificateCreateWithData(nil, certificate(name) as CFData))
        var result: SecTrust?
        XCTAssertEqual(SecTrustCreateWithCertificates(cert, SecPolicyCreateSSL(true, nil), &result), errSecSuccess)
        return try XCTUnwrap(result)
    }
    func transport(port: Int = 42385, owner: UUID? = nil, pin: String? = nil) throws -> PinnedOwnerTransport {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 8
        return PinnedOwnerTransport(configuration: config, endpoint: URL(string: "https://127.0.0.1:\(port)")!,
            binding: .init(owner: owner ?? self.owner, pin: try pin ?? self.pin()))
    }
    func request(port: Int = 42385, path: String = "/", websocket: Bool = false) -> URLRequest {
        var result = URLRequest(url: URL(string: "\(websocket ? "wss" : "https")://127.0.0.1:\(port)\(path)")!)
        result.setValue("Bearer disposable-test-token", forHTTPHeaderField: "Authorization")
        return result
    }
    func testCertificateBindingRejectsWrongKeyOwnerDatesAndEKU() throws {
        XCTAssertEqual(try pin(), "4379a9ee08e0d4f37685ce498c090125088f046413298a5d2cce99b60a127616", "Independent OpenSSL SPKI hash")
        let binding = OwnerTLSBinding(owner: owner, pin: try pin())
        try binding.validate(trust("valid"))
        try binding.validate(trust("renewed"))
        XCTAssertNotEqual(try certificate("valid"), try certificate("renewed"))
        for name in ["expired", "future", "client-only", "wrong-owner"] { XCTAssertThrowsError(try binding.validate(trust(name)), name) }
        XCTAssertThrowsError(try OwnerTLSBinding(owner: UUID(), pin: pin()).validate(trust("valid")))
        XCTAssertThrowsError(try OwnerTLSBinding(owner: owner, pin: String(repeating: "0", count: 64)).validate(trust("valid")))
        XCTAssertThrowsError(try binding.validate(trust("valid"), now: Date(timeIntervalSince1970: 1)))
    }
    func testDERRejectsTruncatedTrailingAndNonCanonicalLengths() throws {
        let der = try certificate("valid")
        XCTAssertThrowsError(try DERCertificate(data: der.dropLast()))
        XCTAssertThrowsError(try DERCertificate(data: der + Data([0])))
        XCTAssertThrowsError(try DERCertificate(data: Data([0x30, 0x80, 0, 0])))
        XCTAssertThrowsError(try DERCertificate(data: Data([0x30, 0x81, 0x01, 0])))
    }
    func testActualHTTPSAcceptsPinnedOwnerAndRefusesRedirect() async throws {
        let transport = try transport()
        defer { transport.close() }
        let (data, response) = try await transport.data(for: request())
        XCTAssertEqual(response.statusCode, 200)
        XCTAssertTrue(String(decoding: data, as: UTF8.self).contains("native-owner-tls"))
        let (_, redirect) = try await transport.data(for: request(path: "/redirect"))
        XCTAssertEqual(redirect.statusCode, 302)
        let renewed = try self.transport(port: 42390)
        defer { renewed.close() }
        let (_, renewal) = try await renewed.data(for: request(port: 42390))
        XCTAssertEqual(renewal.statusCode, 200, "Renewing the leaf with the same pinned key must work")
    }
    func testActualHTTPSRefusesWrongPinWrongOwnerExpiredFutureAndClientOnly() async throws {
        let transports = [try transport(pin: String(repeating: "0", count: 64)), try transport(owner: UUID()),
                          try transport(port: 42386), try transport(port: 42387), try transport(port: 42388), try transport(port: 42389)]
        let ports = [42385, 42385, 42386, 42387, 42388, 42389]
        for (transport, port) in zip(transports, ports) {
            defer { transport.close() }
            do { _ = try await transport.data(for: request(port: port)); XCTFail("Untrusted owner accepted on \(port)") } catch {}
        }
    }
    func testActualWSSAcceptsPinnedOwnerAndWrongPinReceivesNoFrame() async throws {
        let valid = try transport()
        defer { valid.close() }
        var iterator = valid.events(for: request(websocket: true)).makeAsyncIterator()
        let received = try await iterator.next()
        XCTAssertTrue(String(decoding: try XCTUnwrap(received), as: UTF8.self).contains("native-owner-tls"))
        let wrong = try transport(pin: String(repeating: "f", count: 64))
        defer { wrong.close() }
        var rejected = wrong.events(for: request(websocket: true)).makeAsyncIterator()
        do { _ = try await rejected.next(); XCTFail("Wrong pin received a websocket frame") } catch {}
    }
    func testActualWebSocketPreservesAuthorizationCloseCodes() async throws {
        for (code, status) in [(4401, 401), (4403, 403), (4413, 503)] {
            let connection = try transport()
            defer { connection.close() }
            var stream = connection.events(for: request(path: "/close/\(code)", websocket: true)).makeAsyncIterator()
            do { _ = try await stream.next(); XCTFail("Authorization close must terminate the stream") }
            catch {
                guard case ControllerError.remote(let received, _, _) = error else { XCTFail("Lost close code \(code): \(error)"); continue }
                XCTAssertEqual(received, status)
            }
        }
    }
    func testProxyConfiguredTransportRejectsLoopbackDestination() async throws {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 2
        config.proxyConfigurations = [ProxyConfiguration(socksv5Proxy: .hostPort(host: "127.0.0.1", port: 9))]
        let connection = PinnedOwnerTransport(configuration: config, endpoint: URL(string: "https://127.0.0.1:42385")!, binding: .init(owner: owner, pin: try pin()))
        defer { connection.close() }
        do { _ = try await connection.data(for: request()); XCTFail("Loopback bypass reached direct TLS") }
        catch { XCTAssertEqual(error as? ControllerError, .invalid("A mesh proxy request must target an assigned mesh address.")) }
        XCTAssertTrue(MeshAddress.contains("100.64.0.1"))
        XCTAssertTrue(MeshAddress.contains("[fd7a:115c:a1e0::1]"))
        for address in ["100.63.255.255", "100.128.0.1", "127.0.0.1", "fd7a:115c:a1e1::1", "owner.example"] { XCTAssertFalse(MeshAddress.contains(address)) }
    }
    func testHTTPFramingUsesByteLengthsAndRejectsAmbiguousOrTruncatedBodies() throws {
        let url = URL(string: "https://owner.test")!
        let chunked = Data("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n🌍\r\n0\r\n\r\n".utf8)
        XCTAssertEqual(String(decoding: try OwnerHTTPMessage.response(chunked, url: url).0, as: UTF8.self), "🌍")
        for raw in ["HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort",
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 5\r\n\r\n0\r\n\r\n",
                    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\ncontent-length: 1\r\n\r\n",
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nshort\r\n",
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffff\r\n"] {
            XCTAssertThrowsError(try OwnerHTTPMessage.response(Data(raw.utf8), url: url))
        }
        var request = URLRequest(url: URL(string: "https://owner.test/v1?cursor=opaque%2Bvalue")!)
        request.httpMethod = "POST"
        request.httpBody = Data("🌍".utf8)
        request.setValue("Bearer test", forHTTPHeaderField: "Authorization")
        let serialized = String(decoding: try OwnerHTTPMessage.request(request), as: UTF8.self)
        XCTAssertTrue(serialized.contains("Content-Length: 4"))
        XCTAssertTrue(serialized.contains("Connection: close"))
        XCTAssertTrue(serialized.contains("Authorization: Bearer test"))
        XCTAssertFalse(serialized.components(separatedBy: "\r\n")[0].contains("Bearer"))
    }
    func testEmbeddedStatusUsesActualNodeIdentityAndRejectsIncompleteStatus() throws {
        let key = "nodekey:" + String(repeating: "a", count: 64)
        XCTAssertEqual(try EmbeddedMeshConnectionProvider.nodeKey(status: Data("{\"BackendState\":\"Running\",\"Self\":{\"PublicKey\":\"\(key)\"}}".utf8)), key)
        for status in ["{}", "{\"BackendState\":\"NeedsLogin\",\"Self\":{\"PublicKey\":\"\(key)\"}}", "{\"BackendState\":\"Running\",\"Self\":{\"PublicKey\":\"forged\"}}"] {
            XCTAssertThrowsError(try EmbeddedMeshConnectionProvider.nodeKey(status: Data(status.utf8)))
        }
    }
}
