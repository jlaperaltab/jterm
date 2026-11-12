import Foundation
import CryptoKit
import CommonCrypto

/// Sobre cifrado del archivo de exportación (contraseña → PBKDF2-SHA256 → AES-256-GCM).
struct ExportEnvelope: Codable {
    var format: String
    var version: Int
    var kdf: String
    var iterations: Int
    var salt: String      // base64
    var data: String      // base64 de nonce + texto cifrado + etiqueta GCM
}

enum ExportCryptoError: LocalizedError {
    case badPassphrase, unsupported, derivation
    var errorDescription: String? {
        switch self {
        case .badPassphrase: return "Contraseña incorrecta o archivo dañado."
        case .unsupported: return "Formato de archivo no compatible."
        case .derivation: return "No se pudo derivar la clave de cifrado."
        }
    }
}

enum ExportCrypto {
    static let format = "jterm-encrypted"
    static let iterations = 600_000   // recomendación OWASP para PBKDF2-HMAC-SHA256

    private static func derive(_ passphrase: String, salt: Data, iterations: Int) throws -> SymmetricKey {
        let pw = Array(passphrase.utf8)
        guard !pw.isEmpty, !salt.isEmpty else { throw ExportCryptoError.derivation }
        var derived = [UInt8](repeating: 0, count: 32)
        let status = pw.withUnsafeBufferPointer { pwp in
            salt.withUnsafeBytes { sp in
                CCKeyDerivationPBKDF(CCPBKDFAlgorithm(kCCPBKDF2),
                                     UnsafeRawPointer(pwp.baseAddress!).assumingMemoryBound(to: Int8.self), pw.count,
                                     sp.bindMemory(to: UInt8.self).baseAddress!, salt.count,
                                     CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA256), UInt32(iterations),
                                     &derived, 32)
            }
        }
        guard status == kCCSuccess else { throw ExportCryptoError.derivation }
        return SymmetricKey(data: derived)
    }

    static func encrypt(_ plain: Data, passphrase: String) throws -> Data {
        var salt = Data(count: 16)
        let ok = salt.withUnsafeMutableBytes { SecRandomCopyBytes(kSecRandomDefault, 16, $0.baseAddress!) }
        guard ok == errSecSuccess else { throw ExportCryptoError.derivation }
        let key = try derive(passphrase, salt: salt, iterations: iterations)
        let sealed = try AES.GCM.seal(plain, using: key)
        guard let combined = sealed.combined else { throw ExportCryptoError.derivation }
        let env = ExportEnvelope(format: format, version: 2, kdf: "pbkdf2-sha256", iterations: iterations,
                                 salt: salt.base64EncodedString(), data: combined.base64EncodedString())
        let enc = JSONEncoder(); enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        return try enc.encode(env)
    }

    /// ¿El archivo es un sobre cifrado de jTerm?
    static func envelope(from data: Data) -> ExportEnvelope? {
        guard let env = try? JSONDecoder().decode(ExportEnvelope.self, from: data), env.format == format else { return nil }
        return env
    }

    static func decrypt(_ env: ExportEnvelope, passphrase: String) throws -> Data {
        guard env.kdf == "pbkdf2-sha256", (1...10_000_000).contains(env.iterations),
              let salt = Data(base64Encoded: env.salt), let blob = Data(base64Encoded: env.data) else { throw ExportCryptoError.unsupported }
        let key = try derive(passphrase, salt: salt, iterations: env.iterations)
        do { return try AES.GCM.open(try AES.GCM.SealedBox(combined: blob), using: key) }
        catch { throw ExportCryptoError.badPassphrase }
    }
}
