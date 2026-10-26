import Foundation

enum ControllerCodec {
    static func encoder() -> JSONEncoder {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }
    static func decoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .custom { decoder in
            let value = try decoder.singleValueContainer().decode(String.self)
            let formatter = ISO8601DateFormatter()
            formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            if let date = formatter.date(from: value) { return date }
            formatter.formatOptions = [.withInternetDateTime]
            guard let date = formatter.date(from: value) else {
                throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Expected RFC3339 timestamp"))
            }
            return date
        }
        return decoder
    }
    private static func safeBasePath(_ path: String) -> Bool {
        guard !path.contains("//") else { return false }
        return path.split(separator: "/").allSatisfy { segment in
            guard let decoded = String(segment).removingPercentEncoding else { return false }
            return decoded != "." && decoded != ".." && !decoded.contains("/") && !decoded.contains("\\") && !decoded.contains("%")
        }
    }
    static func authority(_ value: String) throws -> URL {
        guard let url = URL(string: value.trimmingCharacters(in: .whitespacesAndNewlines)),
              url.scheme == "https", let host = url.host, !host.isEmpty,
              url.user == nil, url.password == nil, url.query == nil, url.fragment == nil,
              let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
              safeBasePath(components.percentEncodedPath) else {
            throw ControllerError.invalid("Enter the HTTPS base address of your Link service, without credentials, a query, or a fragment.")
        }
        return url
    }
}
