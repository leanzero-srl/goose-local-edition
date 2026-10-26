import CryptoKit
import Foundation
import Security

struct OwnerTLSBinding: Sendable {
    let owner: UUID
    let pin: String
    var serverName: String { "owner-\(owner.uuidString.lowercased()).link" }

    func validate(_ trust: SecTrust, now: Date = Date()) throws {
        guard pin.count == 64, pin.allSatisfy({ "0123456789abcdef".contains($0) }),
              let certificates = SecTrustCopyCertificateChain(trust) as? [SecCertificate],
              let leaf = certificates.first else { throw ControllerError.invalid("The owner certificate is missing.") }
        let certificate = try DERCertificate(data: SecCertificateCopyData(leaf) as Data)
        let digest = SHA256.hash(data: certificate.subjectPublicKeyInfo).map { String(format: "%02x", $0) }.joined()
        guard digest == pin else { throw ControllerError.invalid("This Mac’s certificate does not match its registered identity.") }
        guard certificate.dnsNames.contains(serverName), certificate.serverAuthentication,
              now >= certificate.notBefore, now < certificate.notAfter else {
            throw ControllerError.invalid("This Mac’s certificate is expired or does not identify the selected owner.")
        }
        // Only this worker-bound leaf is an anchor; public roots cannot bypass the pin.
        guard SecTrustSetPolicies(trust, SecPolicyCreateSSL(true, serverName as CFString)) == errSecSuccess,
              SecTrustSetAnchorCertificates(trust, [leaf] as CFArray) == errSecSuccess,
              SecTrustSetAnchorCertificatesOnly(trust, true) == errSecSuccess,
              SecTrustSetNetworkFetchAllowed(trust, false) == errSecSuccess,
              SecTrustSetVerifyDate(trust, now as CFDate) == errSecSuccess else {
            throw ControllerError.invalid("The owner’s TLS certificate could not be verified.")
        }
        var error: CFError?
        guard SecTrustEvaluateWithError(trust, &error) else {
            throw ControllerError.invalid("The owner’s TLS certificate could not be verified (\(error.map { CFErrorGetCode($0) } ?? 0)).")
        }
    }
}

// Security exposes raw keys, not SPKI DER, on iOS. Read the original X.509 fields;
// hashing SecKeyCopyExternalRepresentation would not match the authority’s SPKI pin.
struct DERCertificate {
    let subjectPublicKeyInfo: Data
    let notBefore: Date
    let notAfter: Date
    let dnsNames: [String]
    let serverAuthentication: Bool

    init(data: Data) throws {
        let root = try DERNode.single(data)
        guard root.tag == 0x30 else { throw ControllerError.malformedResponse }
        let certificate = try root.children()
        guard certificate.count == 3, certificate[0].tag == 0x30 else { throw ControllerError.malformedResponse }
        let fields = try certificate[0].children()
        let start = fields.first?.tag == 0xa0 ? 1 : 0
        guard fields.count >= start + 6, fields[start + 5].tag == 0x30 else { throw ControllerError.malformedResponse }
        subjectPublicKeyInfo = fields[start + 5].encoded
        let validity = try fields[start + 3].children()
        guard validity.count == 2 else { throw ControllerError.malformedResponse }
        notBefore = try validity[0].date()
        notAfter = try validity[1].date()
        var names: [String] = []
        var serverAuth = false
        var seenOIDs = Set<Data>()
        for field in fields.dropFirst(start + 6) where field.tag == 0xa3 {
            let container = try DERNode.single(field.content)
            guard container.tag == 0x30 else { throw ControllerError.malformedResponse }
            for ext in try container.children() {
                let parts = try ext.children()
                guard parts.count >= 2, parts.count <= 3, parts[0].tag == 0x06,
                      parts.last?.tag == 0x04, seenOIDs.insert(parts[0].content).inserted else { throw ControllerError.malformedResponse }
                let value = try DERNode.single(parts.last!.content)
                guard value.tag == 0x30 else { continue }
                if parts[0].content == Data([0x55, 0x1d, 0x11]) {
                    for name in try value.children() where name.tag == 0x82 {
                        guard let dns = String(data: name.content, encoding: .ascii) else { throw ControllerError.malformedResponse }
                        names.append(dns.lowercased())
                    }
                }
                if parts[0].content == Data([0x55, 0x1d, 0x25]) {
                    serverAuth = try value.children().contains { $0.tag == 0x06 && $0.content == Data([0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01]) }
                }
            }
        }
        dnsNames = names
        serverAuthentication = serverAuth
    }
}

private struct DERNode {
    let tag: UInt8
    let content: Data
    let encoded: Data
    static func single(_ data: Data) throws -> Self {
        var position = 0
        let result = try read([UInt8](data), position: &position)
        guard position == data.count else { throw ControllerError.malformedResponse }
        return result
    }
    static func read(_ bytes: [UInt8], position: inout Int) throws -> Self {
        let start = position
        guard bytes.count - position >= 2 else { throw ControllerError.malformedResponse }
        let tag = bytes[position]
        position += 1
        let first = bytes[position]
        position += 1
        var length = Int(first)
        if first & 0x80 != 0 {
            let width = Int(first & 0x7f)
            guard width > 0, width <= 4, bytes.count - position >= width, bytes[position] != 0 else { throw ControllerError.malformedResponse }
            length = 0
            for _ in 0..<width { length = (length << 8) | Int(bytes[position]); position += 1 }
            guard length >= 128 else { throw ControllerError.malformedResponse }
        }
        guard length <= bytes.count - position else { throw ControllerError.malformedResponse }
        let content = Data(bytes[position..<(position + length)])
        position += length
        return Self(tag: tag, content: content, encoded: Data(bytes[start..<position]))
    }
    func children() throws -> [Self] {
        let bytes = [UInt8](content)
        var position = 0
        var result: [Self] = []
        while position < bytes.count { result.append(try Self.read(bytes, position: &position)) }
        return result
    }
    func date() throws -> Date {
        guard let text = String(data: content, encoding: .ascii),
              (tag == 0x17 && text.count == 13) || (tag == 0x18 && text.count == 15), text.hasSuffix("Z") else { throw ControllerError.malformedResponse }
        var full = text
        if tag == 0x17 {
            guard let year = Int(text.prefix(2)) else { throw ControllerError.malformedResponse }
            full = (year >= 50 ? "19" : "20") + text
        }
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.dateFormat = "yyyyMMddHHmmss'Z'"
        formatter.isLenient = false
        guard let result = formatter.date(from: full), formatter.string(from: result) == full else { throw ControllerError.malformedResponse }
        return result
    }
}

