// Prints the Sparkle EdDSA public key (base64) for a private key read from
// stdin, so scripts/release.sh can check that the key in ~/keys matches the
// SUPublicEDKey the app ships, before anything is signed or uploaded.
//
// Input: the base64 32-byte ed25519 seed, the format Sparkle's
// `generate_keys -x` writes and `sign_update -f` reads. The private key is
// never printed; on bad input only its decoded length is reported.
import CryptoKit
import Foundation

let input = String(decoding: FileHandle.standardInput.readDataToEndOfFile(), as: UTF8.self)
    .trimmingCharacters(in: .whitespacesAndNewlines)
guard let seed = Data(base64Encoded: input) else {
    FileHandle.standardError.write(Data("sparkle-public-key: the private key is not base64\n".utf8))
    exit(1)
}
guard seed.count == 32, let key = try? Curve25519.Signing.PrivateKey(rawRepresentation: seed) else {
    FileHandle.standardError.write(Data("sparkle-public-key: expected a 32-byte ed25519 seed, got \(seed.count) bytes\n".utf8))
    exit(1)
}
print(key.publicKey.rawRepresentation.base64EncodedString())
