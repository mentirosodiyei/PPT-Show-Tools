// Signs releases so the apps only install updates made by the author.
//
//   swift tools/release-sign.swift keygen        create the key (once) and print the public key
//   swift tools/release-sign.swift pubkey        print the public key
//   swift tools/release-sign.swift sign FILE     write FILE.sig (Ed25519 signature, base64)
//   swift tools/release-sign.swift verify FILE   check FILE.sig
//
// The private key lives in ~/Library/Application Support/PPT Show Tools Release/
// (readable only by you), never in the repository. Keep a backup: without it,
// new versions cannot be installed by the apps' updater, only by hand.
import CryptoKit
import Foundation

// PPTSHOWTOOLS_RELEASE_KEY_DIR lets the test suite use its own throw-away key.
let folder = ProcessInfo.processInfo.environment["PPTSHOWTOOLS_RELEASE_KEY_DIR"].map { URL(fileURLWithPath: $0) }
    ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/PPT Show Tools Release")
let keyFile = folder.appendingPathComponent("release-signing-key.txt")

func die(_ message: String) -> Never { FileHandle.standardError.write(Data((message + "\n").utf8)); exit(1) }

func loadKey() -> Curve25519.Signing.PrivateKey {
    guard let text = try? String(contentsOf: keyFile, encoding: .utf8),
          let raw = Data(base64Encoded: text.trimmingCharacters(in: .whitespacesAndNewlines)),
          let key = try? Curve25519.Signing.PrivateKey(rawRepresentation: raw) else {
        die("No signing key at \(keyFile.path). Run: swift tools/release-sign.swift keygen")
    }
    return key
}

let args = CommandLine.arguments.dropFirst()
switch (args.first, args.dropFirst().first) {
case ("keygen", nil):
    if FileManager.default.fileExists(atPath: keyFile.path) { die("A key already exists at \(keyFile.path); not replacing it.") }
    try! FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
    let key = Curve25519.Signing.PrivateKey()
    FileManager.default.createFile(atPath: keyFile.path, contents: Data(key.rawRepresentation.base64EncodedString().utf8), attributes: [.posixPermissions: 0o600])
    print(key.publicKey.rawRepresentation.base64EncodedString())
case ("pubkey", nil):
    print(loadKey().publicKey.rawRepresentation.base64EncodedString())
case ("sign", let file?):
    guard let data = FileManager.default.contents(atPath: file) else { die("Cannot read \(file)") }
    let signature = try! loadKey().signature(for: data)
    try! Data(signature.base64EncodedString().utf8).write(to: URL(fileURLWithPath: file + ".sig"))
    print("Signed \(file) → \(file).sig")
case ("verify", let file?):
    guard let data = FileManager.default.contents(atPath: file),
          let sigText = try? String(contentsOfFile: file + ".sig", encoding: .utf8),
          let signature = Data(base64Encoded: sigText.trimmingCharacters(in: .whitespacesAndNewlines)) else { die("Cannot read \(file) or its .sig") }
    if loadKey().publicKey.isValidSignature(signature, for: data) { print("Valid signature") } else { die("INVALID signature") }
default:
    die("Usage: swift tools/release-sign.swift keygen | pubkey | sign FILE | verify FILE")
}
