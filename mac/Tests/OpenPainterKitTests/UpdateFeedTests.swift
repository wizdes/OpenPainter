// M58: the Sparkle 2 update feed, tested with a throwaway key pair.
//
// Each test makes a fresh EdDSA (ed25519) key pair inside its own temporary
// folder (CryptoKit; nothing goes to the Keychain, nothing is printed, the
// folder is deleted at the end). Sparkle's own sign_update tool (from the
// Swift package's artifacts) signs a fake "version 2" update archive and the
// appcast with that key. A small HTTP server on 127.0.0.1 serves both. A fake
// "version 1" host bundle carries the throwaway public key, and Sparkle's real
// SPUUpdater checks the feed (SURequireSignedFeed and
// SUVerifyUpdateBeforeExtraction on, like the app).
//
// What is checked:
// - a correctly signed feed is accepted and its update is found; the archive
//   the feed points to passes Sparkle's EdDSA check (sign_update --verify);
// - a feed signed with another key, and a feed changed after signing, are
//   rejected by the updater (SUAppcastParseError);
// - an archive signed with another key fails Sparkle's EdDSA check.
// The final install step (Sparkle's Autoupdate installer, a launchd job) is
// not run here: it would replace an app bundle and start helper processes.
import AppKit
import CryptoKit
import Network
@testable import OpenPainterKit
import Sparkle
import XCTest

// MARK: - A tiny HTTP server on 127.0.0.1

final class LocalHTTPServer: @unchecked Sendable {
    private let listener: NWListener
    private let queue = DispatchQueue(label: "OpenPainterTests.LocalHTTPServer")
    private var files: [String: Data] = [:]
    private var log: [String] = []
    private(set) var port: UInt16 = 0

    init() throws {
        let params = NWParameters.tcp
        params.requiredLocalEndpoint = .hostPort(host: .ipv4(.loopback), port: .any)
        listener = try NWListener(using: params)
        let ready = DispatchSemaphore(value: 0)
        listener.stateUpdateHandler = { state in
            switch state {
            case .ready, .failed, .cancelled: ready.signal()
            default: break
            }
        }
        listener.newConnectionHandler = { [weak self] c in self?.serve(c) }
        listener.start(queue: queue)
        guard ready.wait(timeout: .now() + 5) == .success, let p = listener.port?.rawValue, p != 0 else {
            listener.cancel()
            throw NSError(domain: "LocalHTTPServer: listener did not start", code: 1)
        }
        port = p
    }

    func url(_ path: String) -> URL { URL(string: "http://127.0.0.1:\(port)\(path)")! }

    func put(_ path: String, _ data: Data) { queue.sync { files[path] = data } }

    /// Request paths served so far, in order.
    var requests: [String] { queue.sync { log } }

    func stop() { listener.cancel() }

    private func serve(_ c: NWConnection) {
        c.start(queue: queue)
        c.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] data, _, _, _ in
            guard let self else { c.cancel(); return }
            let text = String(decoding: data ?? Data(), as: UTF8.self)
            let parts = text.split(separator: "\r\n").first?.split(separator: " ") ?? []
            let path = parts.count > 1 ? String(parts[1]) : ""
            self.log.append(path)
            var out: Data
            if let body = self.files[path] {
                out = Data("HTTP/1.1 200 OK\r\nContent-Length: \(body.count)\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n".utf8)
                if parts.first != "HEAD" { out.append(body) }
            } else {
                out = Data("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".utf8)
            }
            c.send(content: out, completion: .contentProcessed { _ in c.cancel() })
        }
    }
}

// MARK: - Sparkle without UI

/// A user driver that shows nothing. The probe check used here never asks it
/// for anything; any call that would need an answer is recorded and declined.
final class SilentUserDriver: NSObject, SPUUserDriver {
    var calls: [String] = []
    func show(_ request: SPUUpdatePermissionRequest, reply: @escaping (SUUpdatePermissionResponse) -> Void) {
        calls.append("permission")
        reply(SUUpdatePermissionResponse(automaticUpdateChecks: false, sendSystemProfile: false))
    }
    func showUserInitiatedUpdateCheck(cancellation: @escaping () -> Void) { calls.append("userCheck") }
    func showUpdateFound(with appcastItem: SUAppcastItem, state: SPUUserUpdateState,
                         reply: @escaping (SPUUserUpdateChoice) -> Void) {
        calls.append("updateFound")
        reply(.dismiss)
    }
    func showUpdateReleaseNotes(with downloadData: SPUDownloadData) {}
    func showUpdateReleaseNotesFailedToDownloadWithError(_ error: any Error) {}
    func showUpdateNotFoundWithError(_ error: any Error, acknowledgement: @escaping () -> Void) {
        calls.append("notFound")
        acknowledgement()
    }
    func showUpdaterError(_ error: any Error, acknowledgement: @escaping () -> Void) {
        calls.append("error")
        acknowledgement()
    }
    func showDownloadInitiated(cancellation: @escaping () -> Void) {
        calls.append("download")
        cancellation()
    }
    func showDownloadDidReceiveExpectedContentLength(_ expectedContentLength: UInt64) {}
    func showDownloadDidReceiveData(ofLength length: UInt64) {}
    func showDownloadDidStartExtractingUpdate() {}
    func showExtractionReceivedProgress(_ progress: Double) {}
    func showReady(toInstallAndRelaunch reply: @escaping (SPUUserUpdateChoice) -> Void) {
        calls.append("readyToInstall")
        reply(.dismiss)
    }
    func showInstallingUpdate(withApplicationTerminated applicationTerminated: Bool,
                              retryTerminatingApplication: @escaping () -> Void) {}
    func showUpdateInstalledAndRelaunched(_ relaunched: Bool, acknowledgement: @escaping () -> Void) { acknowledgement() }
    func showUpdateInFocus() {}
    func dismissUpdateInstallation() {}
}

/// Records what the updater decided.
final class FeedDelegate: NSObject, SPUUpdaterDelegate {
    var found: SUAppcastItem?
    var notFound: NSError?
    var aborted: NSError?
    var finished = false
    var finishError: NSError?

    func updater(_ updater: SPUUpdater, didFindValidUpdate item: SUAppcastItem) { found = item }
    func updaterDidNotFindUpdate(_ updater: SPUUpdater, error: any Error) { notFound = error as NSError }
    func updater(_ updater: SPUUpdater, didAbortWithError error: any Error) { aborted = error as NSError }
    func updater(_ updater: SPUUpdater, didFinishUpdateCycleFor updateCheck: SPUUpdateCheck, error: (any Error)?) {
        finished = true
        finishError = error as NSError?
    }
}

// MARK: - Tests

@MainActor
final class UpdateFeedTests: XCTestCase {
    var dir: URL!
    var server: LocalHTTPServer!
    var hostID: String!

    /// Sparkle's signing tool from the Swift package (Sparkle docs: "you will
    /// find Sparkle's tools ... in ../artifacts/sparkle/Sparkle/bin/").
    static let signUpdate = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appendingPathComponent("../../build/DerivedData/SourcePackages/artifacts/sparkle/Sparkle/bin/sign_update")
        .standardized

    override func setUp() async throws {
        _ = NSApplication.shared
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("OpenPainterUpdateTests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        server = try LocalHTTPServer()
        // One fixed id, so at most one (emptied) preferences file stays behind.
        hostID = "dev.yili.OpenPainterUpdateTest"
        XCTAssertTrue(FileManager.default.isExecutableFile(atPath: Self.signUpdate.path),
                      "Sparkle's sign_update is missing at \(Self.signUpdate.path); build the project first")
    }

    override func tearDown() async throws {
        server?.stop()
        if let dir { try? FileManager.default.removeItem(at: dir) }
        if let hostID {
            // Sparkle keeps per-host settings and a "first failed feed check"
            // date; remove both so no test state stays behind.
            UserDefaults.standard.removePersistentDomain(forName: hostID)
            UserDefaults.standard.removeObject(forKey: "SUInitialFailedFeedSigningValidationDate_\(hostID)")
        }
    }

    // MARK: Helpers

    struct KeyPair {
        let file: URL
        let publicKey: String
    }

    /// A throwaway key pair. The private key (the 32-byte ed25519 seed, base64,
    /// the format sign_update -f reads) is written only inside the test folder.
    func makeKeyPair(_ name: String) throws -> KeyPair {
        let key = Curve25519.Signing.PrivateKey()
        let keys = dir.appendingPathComponent("keys")
        try FileManager.default.createDirectory(at: keys, withIntermediateDirectories: true)
        let file = keys.appendingPathComponent("\(name).key")
        try Data(key.rawRepresentation.base64EncodedString().utf8).write(to: file)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        return KeyPair(file: file, publicKey: key.publicKey.rawRepresentation.base64EncodedString())
    }

    @discardableResult
    func run(_ tool: URL, _ args: [String]) throws -> (status: Int32, out: String) {
        let p = Process()
        p.executableURL = tool
        p.arguments = args
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = pipe
        try p.run()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        return (p.terminationStatus, String(decoding: data, as: UTF8.self))
    }

    /// A minimal app bundle: Info.plist plus an executable script.
    func makeApp(at url: URL, version: String, extra: [String: Any] = [:]) throws {
        let macos = url.appendingPathComponent("Contents/MacOS")
        try FileManager.default.createDirectory(at: macos, withIntermediateDirectories: true)
        var info: [String: Any] = [
            "CFBundleIdentifier": hostID!, "CFBundleName": "OpenPainterUpdateTest",
            "CFBundleExecutable": "OpenPainterUpdateTest", "CFBundlePackageType": "APPL",
            "CFBundleVersion": version, "CFBundleShortVersionString": "\(version).0",
            "LSMinimumSystemVersion": "15.0",
        ]
        info.merge(extra) { _, new in new }
        let plist = try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
        try plist.write(to: url.appendingPathComponent("Contents/Info.plist"))
        let exe = macos.appendingPathComponent("OpenPainterUpdateTest")
        try Data("#!/bin/sh\nexit 0\n".utf8).write(to: exe)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: exe.path)
    }

    /// The "version 1" host that Sparkle updates, holding `publicKey`.
    func makeHost(publicKey: String) throws -> Bundle {
        let url = dir.appendingPathComponent("host/OpenPainterUpdateTest.app")
        try makeApp(at: url, version: "1", extra: [
            "SUFeedURL": server.url("/appcast.xml").absoluteString,
            "SUPublicEDKey": publicKey,
            "SURequireSignedFeed": true,
            "SUVerifyUpdateBeforeExtraction": true,
            "SUEnableAutomaticChecks": false,
        ])
        return try XCTUnwrap(Bundle(url: url))
    }

    /// Zips a "version 2" app, serves it, and returns (archive file, signature by `key`).
    func makeUpdate(signedBy key: KeyPair) throws -> (URL, String) {
        let app = dir.appendingPathComponent("update/OpenPainterUpdateTest.app")
        try makeApp(at: app, version: "2")
        let zip = dir.appendingPathComponent("OpenPainterUpdateTest-2.zip")
        let ditto = try run(URL(fileURLWithPath: "/usr/bin/ditto"), ["-c", "-k", "--keepParent", app.path, zip.path])
        XCTAssertEqual(ditto.status, 0, ditto.out)
        let signed = try run(Self.signUpdate, ["-f", key.file.path, "-p", zip.path])
        XCTAssertEqual(signed.status, 0, signed.out)
        let signature = signed.out.trimmingCharacters(in: .whitespacesAndNewlines)
        XCTAssertEqual(Data(base64Encoded: signature)?.count, 64, "an ed25519 signature is 64 bytes")
        server.put("/OpenPainterUpdateTest-2.zip", try Data(contentsOf: zip))
        return (zip, signature)
    }

    /// Writes the appcast for one update, signs it with `feedKey` (sign_update
    /// adds the signature block), applies `tamper`, and serves it.
    func publishFeed(archive: URL, signature: String, feedKey: KeyPair, tamper: ((String) -> String)? = nil) throws {
        let length = try FileManager.default.attributesOfItem(atPath: archive.path)[.size] as! NSNumber
        let xml = """
            <?xml version="1.0" encoding="utf-8"?>
            <rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
              <channel>
                <title>OpenPainter test feed</title>
                <item>
                  <title>Version 2</title>
                  <sparkle:version>2</sparkle:version>
                  <sparkle:shortVersionString>2.0</sparkle:shortVersionString>
                  <sparkle:minimumSystemVersion>15.0</sparkle:minimumSystemVersion>
                  <enclosure url="\(server.url("/OpenPainterUpdateTest-2.zip").absoluteString)" length="\(length)"
                             type="application/octet-stream" sparkle:edSignature="\(signature)"/>
                </item>
              </channel>
            </rss>

            """
        let feed = dir.appendingPathComponent("appcast.xml")
        try Data(xml.utf8).write(to: feed)
        let signed = try run(Self.signUpdate, ["-f", feedKey.file.path, "--disable-signing-warning", feed.path])
        XCTAssertEqual(signed.status, 0, signed.out)
        var text = try String(contentsOf: feed, encoding: .utf8)
        XCTAssertTrue(text.contains("<!-- sparkle-signatures:"), "sign_update embeds the feed signature")
        if let tamper { text = tamper(text) }
        server.put("/appcast.xml", Data(text.utf8))
    }

    /// Runs Sparkle's background probe check against the host and waits for the cycle to end.
    func check(_ host: Bundle) throws -> (FeedDelegate, SilentUserDriver) {
        let driver = SilentUserDriver()
        let delegate = FeedDelegate()
        let updater = SPUUpdater(hostBundle: host, applicationBundle: host, userDriver: driver, delegate: delegate)
        try updater.start()
        updater.checkForUpdateInformation()
        XCTAssertTrue(waitUntil(30) { delegate.finished }, "the update check finished")
        return (delegate, driver)
    }

    /// Sparkle's EdDSA check of an archive (the same ed25519 verify the installer runs).
    func verifies(_ archive: URL, _ signature: String, publicKeyOf key: KeyPair) throws -> Bool {
        try run(Self.signUpdate, ["--verify", "-f", key.file.path, archive.path, signature]).status == 0
    }

    // MARK: Cases

    func testCorrectlySignedFeedAndUpdateAreAccepted() throws {
        let key = try makeKeyPair("release")
        let (zip, signature) = try makeUpdate(signedBy: key)
        try publishFeed(archive: zip, signature: signature, feedKey: key)
        let (result, driver) = try check(try makeHost(publicKey: key.publicKey))

        let item = try XCTUnwrap(result.found, "Sparkle found the signed update: \(String(describing: result.finishError))")
        XCTAssertEqual(item.versionString, "2")
        XCTAssertEqual(item.signingValidationStatus, .succeeded, "the feed signature was checked and passed")
        XCTAssertNil(result.aborted)
        XCTAssertEqual(driver.calls, [], "the probe check shows no UI")
        XCTAssertEqual(server.requests.first, "/appcast.xml")

        // The archive the signed feed names, fetched from the local server,
        // carries a signature that Sparkle's EdDSA check accepts.
        let fileURL = try XCTUnwrap(item.fileURL)
        XCTAssertEqual(fileURL.host, "127.0.0.1")
        let enclosure = try XCTUnwrap(item.propertiesDictionary["enclosure"] as? [String: Any])
        let feedSignature = try XCTUnwrap(enclosure["sparkle:edSignature"] as? String)
        XCTAssertEqual(feedSignature, signature)
        let downloaded = dir.appendingPathComponent("downloaded.zip")
        try Data(contentsOf: fileURL).write(to: downloaded)
        XCTAssertEqual(try Data(contentsOf: downloaded), try Data(contentsOf: zip))
        XCTAssertTrue(try verifies(downloaded, feedSignature, publicKeyOf: key))
    }

    func testFeedSignedWithAnotherKeyIsRejected() throws {
        let key = try makeKeyPair("release")
        let other = try makeKeyPair("attacker")
        let (zip, signature) = try makeUpdate(signedBy: key)
        try publishFeed(archive: zip, signature: signature, feedKey: other)
        let (result, _) = try check(try makeHost(publicKey: key.publicKey))

        XCTAssertNil(result.found, "a wrongly signed feed offers no update")
        let error = try XCTUnwrap(result.aborted ?? result.finishError, "the check failed")
        XCTAssertEqual(error.domain, SUSparkleErrorDomain)
        XCTAssertEqual(error.code, Int(SUError.appcastParseError.rawValue), "\(error)")
        XCTAssertTrue(error.localizedDescription.contains("improperly signed"), error.localizedDescription)
    }

    func testFeedChangedAfterSigningIsRejected() throws {
        let key = try makeKeyPair("release")
        let (zip, signature) = try makeUpdate(signedBy: key)
        try publishFeed(archive: zip, signature: signature, feedKey: key) {
            $0.replacingOccurrences(of: "<title>Version 2</title>", with: "<title>Version 2!</title>")
        }
        let (result, _) = try check(try makeHost(publicKey: key.publicKey))

        XCTAssertNil(result.found, "a changed feed offers no update")
        let error = try XCTUnwrap(result.aborted ?? result.finishError, "the check failed")
        XCTAssertEqual(error.code, Int(SUError.appcastParseError.rawValue), "\(error)")
    }

    func testUpdateSignedWithAnotherKeyIsRejected() throws {
        let key = try makeKeyPair("release")
        let other = try makeKeyPair("attacker")
        let (zip, badSignature) = try makeUpdate(signedBy: other)
        try publishFeed(archive: zip, signature: badSignature, feedKey: key)
        let (result, _) = try check(try makeHost(publicKey: key.publicKey))

        // The feed itself is genuine, so the item is offered...
        let item = try XCTUnwrap(result.found, "\(String(describing: result.finishError))")
        // ...but its archive fails the EdDSA check against the app's key, so
        // Sparkle's installer would refuse it before extracting.
        let downloaded = dir.appendingPathComponent("downloaded.zip")
        try Data(contentsOf: try XCTUnwrap(item.fileURL)).write(to: downloaded)
        XCTAssertFalse(try verifies(downloaded, badSignature, publicKeyOf: key))
        XCTAssertTrue(try verifies(downloaded, badSignature, publicKeyOf: other), "the signature itself is well formed")
    }

    // MARK: The app's own settings

    /// The app ships the placeholder feed, an empty public key and signed-feed
    /// enforcement (mac/project.yml writes mac/OpenPainter/Info.plist).
    func testAppInfoPlistHasThePlaceholderFeedAndNoKey() throws {
        let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .appendingPathComponent("../../OpenPainter/Info.plist").standardized
        let info = try XCTUnwrap(NSDictionary(contentsOf: url) as? [String: Any], url.path)
        XCTAssertEqual(info["SUFeedURL"] as? String, UpdateSettings.placeholderFeed)
        XCTAssertEqual(info["SUPublicEDKey"] as? String, "", "the release key is added at M60 with the user's OK")
        XCTAssertEqual(info["SURequireSignedFeed"] as? Bool, true)
        XCTAssertEqual(info["SUVerifyUpdateBeforeExtraction"] as? Bool, true)
        let settings = UpdateSettings(info: info)
        XCTAssertFalse(settings.isSetUp)
        XCTAssertEqual(settings.feedURL?.absoluteString, UpdateSettings.placeholderFeed)
    }

    /// With no key, Check for Updates… is on, says "Updates are not set up
    /// yet" and never starts Sparkle.
    func testCheckForUpdatesSaysNotSetUpWhileTheKeyIsEmpty() throws {
        let target = UpdateMenuTarget()
        target.settings = UpdateSettings(info: ["SUFeedURL": UpdateSettings.placeholderFeed, "SUPublicEDKey": " "])
        var shown: [NSAlert] = []
        target.present = { shown.append($0) }
        target.startIfSetUp()
        XCTAssertFalse(target.isRunning)
        target.checkForUpdates(nil)
        XCTAssertEqual(shown.map(\.messageText), ["Updates are not set up yet"])
        XCTAssertEqual(shown.first?.informativeText, UpdateMenuTarget.notSetUpDetail)
        XCTAssertEqual(shown.first?.buttons.map(\.title), ["OK"])
        XCTAssertFalse(target.isRunning, "the updater stays off")

        // The menu item is on and goes to the shared target.
        let fx = try ShellFixture()
        defer { fx.tearDown() }
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: fx.workspace.menuTarget)
        let item = try XCTUnwrap(menu.item(withTitle: "Help")?.submenu?.item(withTitle: "Check for Updates…"))
        XCTAssertEqual(item.action, #selector(UpdateMenuActions.checkForUpdates(_:)))
        XCTAssertTrue(item.target === UpdateMenuTarget.shared)

        if let out = ProcessInfo.processInfo.environment["OP_M58_PNG_DIR"] {
            try renderAlert(UpdateMenuTarget.notSetUpAlert(), to: URL(fileURLWithPath: out))
        }
    }

    /// Writes the alert in light and dark (visual check only, outside the repo).
    func renderAlert(_ alert: NSAlert, to dir: URL) throws {
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        alert.layout()
        for (name, look) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let window = alert.window
            window.appearance = NSAppearance(named: look)
            let view = try XCTUnwrap(window.contentView)
            view.appearance = NSAppearance(named: look)
            view.layoutSubtreeIfNeeded()
            let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            let ctx = try XCTUnwrap(NSGraphicsContext(bitmapImageRep: rep))
            // The alert window's own background is not part of its content
            // view, so paint the window color first, then the view.
            NSAppearance(named: look)!.performAsCurrentDrawingAppearance {
                NSGraphicsContext.saveGraphicsState()
                NSGraphicsContext.current = ctx
                NSColor.windowBackgroundColor.setFill()
                view.bounds.fill()
                NSGraphicsContext.restoreGraphicsState()
                view.displayIgnoringOpacity(view.bounds, in: ctx)
            }
            let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
            try png.write(to: dir.appendingPathComponent("updates-not-set-up-\(name).png"))
        }
    }
}
