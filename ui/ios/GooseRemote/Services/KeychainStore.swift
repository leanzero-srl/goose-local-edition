import Foundation
import Security

protocol CredentialStorage: Sendable {
    func read<T: Decodable>(_ type: T.Type, account: String) throws -> T?
    func write<T: Encodable>(_ value: T, account: String) throws
    func remove(account: String) throws
}

struct KeychainStore: CredentialStorage {
    private let service = "net.leanzero.goose-remote.controller"
    private func query(_ account: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
         kSecAttrAccount as String: account, kSecAttrSynchronizable as String: false]
    }
    func read<T: Decodable>(_ type: T.Type, account: String) throws -> T? {
        var query = query(account)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data else { throw ControllerError.credentialStore(status) }
        return try ControllerCodec.decoder().decode(type, from: data)
    }
    func write<T: Encodable>(_ value: T, account: String) throws {
        let data = try ControllerCodec.encoder().encode(value)
        let attributes: [String: Any] = [kSecValueData as String: data, kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly]
        let status = SecItemUpdate(query(account) as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            let status = SecItemAdd(query(account).merging(attributes) { _, new in new } as CFDictionary, nil)
            guard status == errSecSuccess else { throw ControllerError.credentialStore(status) }
        } else if status != errSecSuccess { throw ControllerError.credentialStore(status) }
    }
    func remove(account: String) throws {
        let status = SecItemDelete(query(account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw ControllerError.credentialStore(status) }
    }
}
