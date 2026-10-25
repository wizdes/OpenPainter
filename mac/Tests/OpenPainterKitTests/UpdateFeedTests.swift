// M58: the Sparkle 2 update feed and update archives, tested with a throwaway
// key pair and a throwaway app.
//
// Each test makes a fresh EdDSA (ed25519) key pair inside its own temporary
// folder (CryptoKit; nothing goes to the Keychain, nothing is printed, the
// folder is deleted at the end). Sparkle's own sign_update tool (from the
// Swift package's artifacts) signs fake update archives and the appcast with
// that key. A small HTTP server on 127.0.0.1 serves both with
// "Cache-Control: no-store", so nothing lands in the URL cache. A fake
// "version 1" host app in the temporary folder carries the throwaway public
// key and the app's own Sparkle settings (read from mac/OpenPainter/Info.plist).
//
// Feed checks run Sparkle's background probe (SPUUpdater.checkForUpdateInformation).
// Archive checks run Sparkle's whole update path (SPUUpdater.checkForUpdates):
// download, then Sparkle's installer (Autoupdate, started by Sparkle as a
// user launchd job) validates the archive with SUUpdateValidator before
// extracting, checks the version against the host, and installs over the
// throwaway host app in the temporary folder. Nothing outside that folder is
// installed. The host's Sparkle settings live in a preferences file inside the
// temporary folder (SUDefaultsDomain). Teardown waits for and removes the
// launchd jobs Sparkle submitted, and deletes the host's cache folder.
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

    /// `port` 0 picks a free port.
    init(port: UInt16 = 0) throws {
        let params = NWParameters.tcp
        params.allowLocalEndpointReuse = true
        params.requiredLocalEndpoint = .hostPort(host: .ipv4(.loopback),
                                                 port: port == 0 ? .any : NWEndpoint.Port(rawValue: port)!)
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
        self.port = p
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
            // no-store: URLSession (Sparkle's downloads) keeps nothing in its cache.
            let common = "Cache-Control: no-store\r\nConnection: close\r\n"
            if let body = self.files[path] {
                out = Data("HTTP/1.1 200 OK\r\nContent-Length: \(body.count)\r\nContent-Type: application/octet-stream\r\n\(common)\r\n".utf8)
                if parts.first != "HEAD" { out.append(body) }
            } else {
                out = Data("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\(common)\r\n".utf8)
            }
            c.send(content: out, completion: .contentProcessed { _ in c.cancel() })
        }
    }
}

// MARK: - Sparkle without UI

/// A user driver that shows nothing and records what Sparkle asked. It
/// answers "install" when an update is found and "dismiss" when the update is
/// ready, so Sparkle's installer runs without relaunching anything.
final class SilentUserDriver: NSObject, SPUUserDriver {
    var calls: [String] = []
    var errors: [NSError] = []
    /// Answer to "update found". The probe check never asks.
    var foundChoice: SPUUserUpdateChoice = .dismiss
    var installed = false
    var dismissed = false

    func show(_ request: SPUUpdatePermissionRequest, reply: @escaping (SUUpdatePermissionResponse) -> Void) {
        calls.append("permission")
        reply(SUUpdatePermissionResponse(automaticUpdateChecks: false, sendSystemProfile: false))
    }
    func showUserInitiatedUpdateCheck(cancellation: @escaping () -> Void) { calls.append("userCheck") }
    func showUpdateFound(with appcastItem: SUAppcastItem, state: SPUUserUpdateState,
                         reply: @escaping (SPUUserUpdateChoice) -> Void) {
        calls.append("updateFound")
        reply(foundChoice)
    }
    func showUpdateReleaseNotes(with downloadData: SPUDownloadData) {}
    func showUpdateReleaseNotesFailedToDownloadWithError(_ error: any Error) {}
    func showUpdateNotFoundWithError(_ error: any Error, acknowledgement: @escaping () -> Void) {
        calls.append("notFound")
        errors.append(error as NSError)
        acknowledgement()
    }
    func showUpdaterError(_ error: any Error, acknowledgement: @escaping () -> Void) {
        calls.append("error")
        errors.append(error as NSError)
        acknowledgement()
    }
    func showDownloadInitiated(cancellation: @escaping () -> Void) { calls.append("download") }
    func showDownloadDidReceiveExpectedContentLength(_ expectedContentLength: UInt64) {}
    func showDownloadDidReceiveData(ofLength length: UInt64) {}
    func showDownloadDidStartExtractingUpdate() { calls.append("extracting") }
    func showExtractionReceivedProgress(_ progress: Double) {}
    func showReady(toInstallAndRelaunch reply: @escaping (SPUUserUpdateChoice) -> Void) {
        calls.append("readyToInstall")
        reply(.dismiss)
    }
    func showInstallingUpdate(withApplicationTerminated applicationTerminated: Bool,
                              retryTerminatingApplication: @escaping () -> Void) {
        calls.append("installing")
    }
    func showUpdateInstalledAndRelaunched(_ relaunched: Bool, acknowledgement: @escaping () -> Void) {
        calls.append("installed")
        installed = true
        acknowledgement()
    }
    func showUpdateInFocus() {}
    func dismissUpdateInstallation() {
        calls.append("dismissed")
        dismissed = true
    }
}

/// Records what the updater decided. Never relaunches the throwaway app.
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
    func updaterShouldRelaunchApplication(_ updater: SPUUpdater) -> Bool { false }
}

// MARK: - Tests

@MainActor
final class UpdateFeedTests: XCTestCase {
    var dir: URL!
    var server: LocalHTTPServer!
    var hostID: String!
    var updaters: [SPUUpdater] = []

    static let macDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appendingPathComponent("../..").standardized
    /// Sparkle's signing tool from the Swift package (Sparkle docs: "you will
    /// find Sparkle's tools ... in ../artifacts/sparkle/Sparkle/bin/").
    static let signUpdate = macDir
        .appendingPathComponent("build/DerivedData/SourcePackages/artifacts/sparkle/Sparkle/bin/sign_update")
    /// The Info.plist XcodeGen writes from mac/project.yml.
    static let sourceInfoPlist = macDir.appendingPathComponent("OpenPainter/Info.plist")
    /// The Debug app the test scheme builds before testing.
    static let builtApp = macDir.appendingPathComponent("build/DerivedData/Build/Products/Debug/OpenPainter.app")

    /// The Sparkle keys (SU*) of the app's Info.plist, without the feed, the
    /// key and SUEnableAutomaticChecks (throwaway hosts never check on a timer).
    static func appSparkleSettings() throws -> [String: Any] {
        let info = try XCTUnwrap(NSDictionary(contentsOf: sourceInfoPlist) as? [String: Any], sourceInfoPlist.path)
        let skipped: Set<String> = ["SUFeedURL", "SUPublicEDKey", "SUEnableAutomaticChecks"]
        return info.filter { $0.key.hasPrefix("SU") && !skipped.contains($0.key) }
    }

    override func setUp() async throws {
        _ = NSApplication.shared
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("OpenPainterUpdateTests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        server = try LocalHTTPServer()
        // A new bundle id per test. Sparkle keeps the host's settings in the
        // domain SUDefaultsDomain names: an absolute path inside the test
        // folder (CFPreferences accepts paths), so its plist is deleted with
        // the folder and never lands in ~/Library/Preferences.
        hostID = "dev.yili.OpenPainterUpdateTest.\(UUID().uuidString)"
        XCTAssertTrue(FileManager.default.isExecutableFile(atPath: Self.signUpdate.path),
                      "Sparkle's sign_update is missing at \(Self.signUpdate.path); build the project first")
    }

    override func tearDown() async throws {
        updaters.removeAll()
        // Let Sparkle finish and release its objects (and their defaults)
        // before the domain is removed, so nothing writes it back later.
        _ = waitUntil(0.5) { false }
        server?.stop()
        if let dir { try? FileManager.default.removeItem(at: dir) }
        guard let hostID else { return }
        // Sparkle's installer and progress agent run as user launchd jobs
        // named after the host. Wait (up to 15 s) until they have exited,
        // since they also write the host's preferences, then remove them.
        for label in ["\(hostID)-sparkle-updater", "\(hostID)-sparkle-progress"] {
            _ = waitUntil(15) { !Self.launchdJobIsRunning(label) }
            XCTAssertFalse(Self.launchdJobIsRunning(label), "\(label) still running")
            _ = Self.launchctl(["remove", label])
        }
        // The "first failed feed check" date lives in the test runner's domain.
        UserDefaults.standard.removeObject(forKey: "SUInitialFailedFeedSigningValidationDate_\(hostID)")
        let caches = try FileManager.default.url(for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: false)
        try? FileManager.default.removeItem(at: caches.appendingPathComponent(hostID))
    }

    // MARK: Helpers

    @discardableResult
    static func launchctl(_ args: [String]) -> String {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/launchctl")
        p.arguments = args
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = FileHandle.nullDevice
        guard (try? p.run()) != nil else { return "" }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        return String(decoding: data, as: UTF8.self)
    }

    /// True while a user launchd job with this label has a running process.
    static func launchdJobIsRunning(_ label: String) -> Bool {
        launchctl(["list", label]).contains("\"PID\" =")
    }

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

    /// A minimal app bundle: Info.plist (the app's Sparkle settings plus
    /// `extra`) and an executable script.
    func makeApp(at url: URL, version: String, extra: [String: Any]) throws {
        let macos = url.appendingPathComponent("Contents/MacOS")
        try FileManager.default.createDirectory(at: macos, withIntermediateDirectories: true)
        var info: [String: Any] = [
            "CFBundleIdentifier": hostID!, "CFBundleName": "OpenPainterUpdateTest",
            "CFBundleExecutable": "OpenPainterUpdateTest", "CFBundlePackageType": "APPL",
            "CFBundleVersion": version, "CFBundleShortVersionString": "\(version).0",
            "LSMinimumSystemVersion": "15.0", "SUEnableAutomaticChecks": false,
            "SUDefaultsDomain": dir.appendingPathComponent("sparkle-defaults").path,
        ]
        info.merge(try Self.appSparkleSettings()) { _, new in new }
        info.merge(extra) { _, new in new }
        let plist = try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
        try plist.write(to: url.appendingPathComponent("Contents/Info.plist"))
        let exe = macos.appendingPathComponent("OpenPainterUpdateTest")
        try Data("#!/bin/sh\nexit 0\n".utf8).write(to: exe)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: exe.path)
    }

    var hostURL: URL { dir.appendingPathComponent("host/OpenPainterUpdateTest.app") }

    /// The "version 1" host that Sparkle updates, holding `publicKey`.
    func makeHost(publicKey: String) throws -> Bundle {
        try makeApp(at: hostURL, version: "1", extra: [
            "SUFeedURL": server.url("/appcast.xml").absoluteString,
            "SUPublicEDKey": publicKey,
        ])
        return try XCTUnwrap(Bundle(url: hostURL))
    }

    /// The host's CFBundleVersion on disk now (it changes when Sparkle installs).
    func hostVersion(_ key: String = "CFBundleVersion", at url: URL? = nil) -> String? {
        (NSDictionary(contentsOf: (url ?? hostURL).appendingPathComponent("Contents/Info.plist")) as? [String: Any])?[key] as? String
    }

    /// Zips an update app with CFBundleVersion `version` that keeps `publicKey`
    /// (Sparkle refuses an update that drops the key), serves it, and returns
    /// (archive, signature by `key`, or nil when unsigned).
    func makeUpdate(version: String = "2", publicKey: String, signedBy key: KeyPair?) throws -> (URL, String?) {
        let app = dir.appendingPathComponent("update-\(version)/OpenPainterUpdateTest.app")
        try makeApp(at: app, version: version, extra: [
            "SUFeedURL": server.url("/appcast.xml").absoluteString,
            "SUPublicEDKey": publicKey,
        ])
        let zip = dir.appendingPathComponent("OpenPainterUpdateTest-\(version).zip")
        let ditto = try run(URL(fileURLWithPath: "/usr/bin/ditto"), ["-c", "-k", "--keepParent", app.path, zip.path])
        XCTAssertEqual(ditto.status, 0, ditto.out)
        server.put("/OpenPainterUpdateTest.zip", try Data(contentsOf: zip))
        guard let key else { return (zip, nil) }
        let signed = try run(Self.signUpdate, ["-f", key.file.path, "-p", zip.path])
        XCTAssertEqual(signed.status, 0, signed.out)
        let signature = signed.out.trimmingCharacters(in: .whitespacesAndNewlines)
        XCTAssertEqual(Data(base64Encoded: signature)?.count, 64, "an ed25519 signature is 64 bytes")
        return (zip, signature)
    }

    /// Writes the appcast for one update offered as `feedVersion`, signs it
    /// with `feedKey` (sign_update adds the signature block), applies
    /// `tamper`, and serves it. A nil `signature` leaves the enclosure unsigned.
    func publishFeed(archive: URL, signature: String?, feedKey: KeyPair, feedVersion: String = "2",
                     tamper: ((String) -> String)? = nil) throws {
        let length = try FileManager.default.attributesOfItem(atPath: archive.path)[.size] as! NSNumber
        let edSignature = signature.map { " sparkle:edSignature=\"\($0)\"" } ?? ""
        let xml = """
            <?xml version="1.0" encoding="utf-8"?>
            <rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
              <channel>
                <title>OpenPainter test feed</title>
                <item>
                  <title>Version \(feedVersion)</title>
                  <sparkle:version>\(feedVersion)</sparkle:version>
                  <sparkle:shortVersionString>\(feedVersion).0</sparkle:shortVersionString>
                  <sparkle:minimumSystemVersion>15.0</sparkle:minimumSystemVersion>
                  <enclosure url="\(server.url("/OpenPainterUpdateTest.zip").absoluteString)" length="\(length)"
                             type="application/octet-stream"\(edSignature)/>
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

    func makeUpdater(_ host: Bundle, _ driver: SilentUserDriver, _ delegate: FeedDelegate) throws -> SPUUpdater {
        let updater = SPUUpdater(hostBundle: host, applicationBundle: host, userDriver: driver, delegate: delegate)
        updaters.append(updater)
        try updater.start()
        return updater
    }

    /// Runs Sparkle's background probe check against the host and waits for the cycle to end.
    func check(_ host: Bundle) throws -> (FeedDelegate, SilentUserDriver) {
        let driver = SilentUserDriver()
        let delegate = FeedDelegate()
        try makeUpdater(host, driver, delegate).checkForUpdateInformation()
        XCTAssertTrue(waitUntil(30) { delegate.finished }, "the update check finished")
        return (delegate, driver)
    }

    /// Runs Sparkle's full update path (download, installer validation,
    /// install into the throwaway host) and waits until Sparkle reports an
    /// install or an error and the cycle ends.
    func update(_ host: Bundle) throws -> (FeedDelegate, SilentUserDriver) {
        let driver = SilentUserDriver()
        driver.foundChoice = .install
        let delegate = FeedDelegate()
        try makeUpdater(host, driver, delegate).checkForUpdates()
        let done = waitUntil(90) { (driver.installed || !driver.errors.isEmpty) && delegate.finished }
        XCTAssertTrue(done, "the update finished; Sparkle asked: \(driver.calls)")
        return (delegate, driver)
    }

    /// Every error and underlying error, flattened, for message checks.
    func describe(_ errors: [NSError]) -> String {
        var out: [String] = []
        func walk(_ e: NSError) {
            out.append("\(e.domain) \(e.code): \(e.localizedDescription)")
            if let u = e.userInfo[NSUnderlyingErrorKey] as? NSError { walk(u) }
            for m in e.userInfo[NSMultipleUnderlyingErrorsKey] as? [NSError] ?? [] { walk(m) }
        }
        errors.forEach(walk)
        return out.joined(separator: " | ")
    }

    // MARK: Feed

    func testCorrectlySignedFeedIsAccepted() throws {
        let key = try makeKeyPair("release")
        let (zip, signature) = try makeUpdate(publicKey: key.publicKey, signedBy: key)
        try publishFeed(archive: zip, signature: signature, feedKey: key)
        let (result, driver) = try check(try makeHost(publicKey: key.publicKey))

        let item = try XCTUnwrap(result.found, "Sparkle found the signed update: \(String(describing: result.finishError))")
        XCTAssertEqual(item.versionString, "2")
        XCTAssertEqual(item.signingValidationStatus, .succeeded, "the feed signature was checked and passed")
        XCTAssertNil(result.aborted)
        XCTAssertEqual(driver.calls, [], "the probe check shows no UI")
        XCTAssertEqual(server.requests, ["/appcast.xml"])
    }

    func testFeedSignedWithAnotherKeyIsRejected() throws {
        let key = try makeKeyPair("release")
        let other = try makeKeyPair("attacker")
        let (zip, signature) = try makeUpdate(publicKey: key.publicKey, signedBy: key)
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
        let (zip, signature) = try makeUpdate(publicKey: key.publicKey, signedBy: key)
        try publishFeed(archive: zip, signature: signature, feedKey: key) {
            $0.replacingOccurrences(of: "<title>Version 2</title>", with: "<title>Version 2!</title>")
        }
        let (result, _) = try check(try makeHost(publicKey: key.publicKey))

        XCTAssertNil(result.found, "a changed feed offers no update")
        let error = try XCTUnwrap(result.aborted ?? result.finishError, "the check failed")
        XCTAssertEqual(error.code, Int(SUError.appcastParseError.rawValue), "\(error)")
    }

    /// Sparkle 2.10 lets a failing feed through once its first failure is
    /// older than SUSignedFeedFailureExpirationInterval (20 days when unset).
    /// The app sets it to 0, so a bad feed stays rejected.
    func testBadFeedIsStillRejectedAfterTwentyDays() throws {
        let key = try makeKeyPair("release")
        let other = try makeKeyPair("attacker")
        let (zip, signature) = try makeUpdate(publicKey: key.publicKey, signedBy: key)
        try publishFeed(archive: zip, signature: signature, feedKey: other)
        let host = try makeHost(publicKey: key.publicKey)
        // The date Sparkle stores at the first failure (in the updater's own
        // domain, keyed by the host id), set 21 days back.
        UserDefaults.standard.set(Date(timeIntervalSinceNow: -21 * 86_400),
                                  forKey: "SUInitialFailedFeedSigningValidationDate_\(hostID!)")
        let (result, _) = try check(host)

        XCTAssertNil(result.found, "a wrongly signed feed offers no update, however old the first failure")
        let error = try XCTUnwrap(result.aborted ?? result.finishError, "the check failed")
        XCTAssertEqual(error.code, Int(SUError.appcastParseError.rawValue), "\(error)")
    }

    // MARK: Archives (Sparkle's installer)

    func testSignedUpdateIsValidatedAndInstalledIntoTheThrowawayApp() throws {
        let key = try makeKeyPair("release")
        let (zip, signature) = try makeUpdate(publicKey: key.publicKey, signedBy: key)
        try publishFeed(archive: zip, signature: signature, feedKey: key)
        let host = try makeHost(publicKey: key.publicKey)
        let (_, driver) = try update(host)

        XCTAssertEqual(driver.errors, [], describe(driver.errors))
        XCTAssertTrue(driver.installed, "Sparkle reported the install: \(driver.calls)")
        XCTAssertEqual(hostVersion(), "2", "Sparkle's installer replaced the throwaway v1 app with v2")
        XCTAssertTrue(server.requests.contains("/OpenPainterUpdateTest.zip"))
    }

    func testUpdateSignedWithAnotherKeyIsRejectedByTheInstaller() throws {
        let key = try makeKeyPair("release")
        let other = try makeKeyPair("attacker")
        let (zip, badSignature) = try makeUpdate(publicKey: key.publicKey, signedBy: other)
        try publishFeed(archive: zip, signature: badSignature, feedKey: key)
        let (_, driver) = try update(try makeHost(publicKey: key.publicKey))

        XCTAssertFalse(driver.installed)
        XCTAssertEqual(hostVersion(), "1", "the app was not replaced")
        let text = describe(driver.errors)
        XCTAssertTrue(text.contains("signature validation before unarchiving failed"), text)
    }

    func testUnsignedUpdateIsRejected() throws {
        let key = try makeKeyPair("release")
        let (zip, _) = try makeUpdate(publicKey: key.publicKey, signedBy: nil)
        try publishFeed(archive: zip, signature: nil, feedKey: key)
        let (_, driver) = try update(try makeHost(publicKey: key.publicKey))

        XCTAssertFalse(driver.installed)
        XCTAssertEqual(hostVersion(), "1", "the app was not replaced")
        XCTAssertFalse(driver.errors.isEmpty, "Sparkle refused the unsigned archive: \(driver.calls)")
        print("M58 unsigned update rejected: \(describe(driver.errors))")
    }

    /// The feed offers "3" but the archive holds CFBundleVersion 0: the
    /// installer refuses to go below the installed version 1.
    func testDowngradeIsRefused() throws {
        let key = try makeKeyPair("release")
        let (zip, signature) = try makeUpdate(version: "0", publicKey: key.publicKey, signedBy: key)
        try publishFeed(archive: zip, signature: signature, feedKey: key, feedVersion: "3")
        let (_, driver) = try update(try makeHost(publicKey: key.publicKey))

        XCTAssertFalse(driver.installed)
        XCTAssertEqual(hostVersion(), "1", "the app was not replaced")
        let text = describe(driver.errors)
        XCTAssertTrue(text.contains("downgrade"), text)
    }

    // MARK: The app's own settings

    /// M60: the release key's public half (the private half is
    /// OPENPAINTER_SPARKLE_PRIVATE_KEY in ~/keys).
    static let releasePublicKey = "Q1P9GEU6X00XfYERTrMaaVhfra04UVYC2IuWAK0TJoc="

    static let expectedAppSettings: [String: AnyHashable] = [
        "SUFeedURL": UpdateSettings.placeholderFeed,
        "SUPublicEDKey": releasePublicKey,
        "SUEnableAutomaticChecks": true,
        "SURequireSignedFeed": true,
        "SUVerifyUpdateBeforeExtraction": true,
        "SUSignedFeedFailureExpirationInterval": 0,
    ]

    func checkAppSettings(_ url: URL) throws -> [String: Any] {
        let info = try XCTUnwrap(NSDictionary(contentsOf: url) as? [String: Any], url.path)
        for (key, want) in Self.expectedAppSettings {
            XCTAssertEqual(info[key] as? AnyHashable, want, "\(key) in \(url.path)")
        }
        return info
    }

    /// The Info.plist XcodeGen writes and the one inside the built app both
    /// carry the feed, the release key, scheduled checks and the signed-feed
    /// rules. (M58 expected an empty key here: a placeholder for M60, updated
    /// with the user's OK.)
    func testAppInfoPlistHasTheFeedAndTheReleaseKey() throws {
        let source = try checkAppSettings(Self.sourceInfoPlist)
        XCTAssertTrue(UpdateSettings(info: source).isSetUp)
        let built = try checkAppSettings(Self.builtApp.appendingPathComponent("Contents/Info.plist"))
        let settings = UpdateSettings(info: built)
        XCTAssertTrue(settings.isSetUp)
        XCTAssertEqual(settings.publicKey, Self.releasePublicKey)
        XCTAssertEqual(settings.feedURL?.absoluteString, UpdateSettings.placeholderFeed)
        XCTAssertEqual(Data(base64Encoded: settings.publicKey)?.count, 32, "an ed25519 public key is 32 bytes")
    }

    /// A copy of the built app's Info.plist as a throwaway host: own bundle id,
    /// own defaults file, the feed on the local server, no timer checks.
    func makeBuiltAppHost(_ name: String, key: String?) throws -> Bundle {
        var info = try checkAppSettings(Self.builtApp.appendingPathComponent("Contents/Info.plist"))
        info["CFBundleIdentifier"] = hostID!
        info["CFBundleExecutable"] = "OpenPainterUpdateTest"
        info["SUDefaultsDomain"] = dir.appendingPathComponent("sparkle-defaults").path
        info["SUEnableAutomaticChecks"] = false
        info["SUFeedURL"] = server.url("/appcast.xml").absoluteString
        if let key { info["SUPublicEDKey"] = key }
        let url = dir.appendingPathComponent("\(name)/OpenPainter.app")
        try FileManager.default.createDirectory(at: url.appendingPathComponent("Contents/MacOS"), withIntermediateDirectories: true)
        let plist = try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
        try plist.write(to: url.appendingPathComponent("Contents/Info.plist"))
        try Data("#!/bin/sh\nexit 0\n".utf8).write(to: url.appendingPathComponent("Contents/MacOS/OpenPainterUpdateTest"))
        return try XCTUnwrap(Bundle(url: url))
    }

    /// Sparkle starts with the built app's own settings (the release key).
    func testSparkleStartsWithTheAppsReleaseKey() throws {
        let host = try makeBuiltAppHost("release-key", key: nil)
        let updater = SPUUpdater(hostBundle: host, applicationBundle: host, userDriver: SilentUserDriver(), delegate: nil)
        updaters.append(updater)
        XCTAssertNoThrow(try updater.start())
    }

    /// The M58 guard still holds: Sparkle itself refuses to start with an
    /// empty key, so even a direct start cannot check an unsigned feed. The
    /// host is a copy of the built app's Info.plist with the key emptied.
    func testSparkleRefusesToStartWithAnEmptyKey() throws {
        var info = try checkAppSettings(Self.builtApp.appendingPathComponent("Contents/Info.plist"))
        info["CFBundleIdentifier"] = hostID!
        info["CFBundleExecutable"] = "OpenPainterUpdateTest"
        info["SUPublicEDKey"] = ""
        let url = dir.appendingPathComponent("empty-key/OpenPainter.app")
        try FileManager.default.createDirectory(at: url.appendingPathComponent("Contents/MacOS"), withIntermediateDirectories: true)
        let plist = try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
        try plist.write(to: url.appendingPathComponent("Contents/Info.plist"))
        try Data("#!/bin/sh\nexit 0\n".utf8).write(to: url.appendingPathComponent("Contents/MacOS/OpenPainterUpdateTest"))
        let host = try XCTUnwrap(Bundle(url: url))
        let updater = SPUUpdater(hostBundle: host, applicationBundle: host, userDriver: SilentUserDriver(), delegate: nil)
        XCTAssertThrowsError(try updater.start()) { error in
            let e = error as NSError
            XCTAssertEqual(e.domain, SUSparkleErrorDomain)
            XCTAssertEqual(e.code, Int(SUError.noPublicDSAFoundError.rawValue), "\(e)")
            XCTAssertTrue(e.localizedDescription.contains("EdDSA"), e.localizedDescription)
        }
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

    // MARK: A real release (M60, run by scripts/release.sh)

    /// What scripts/release.sh passes (as TEST_RUNNER_OP_RELEASE_*): the
    /// folder that holds the release DMG, the public appcast and a second copy
    /// of the appcast whose download URL points at this test's local server.
    /// Both feeds are signed with the real release key; this test never sees
    /// the private key. Without these variables the release tests skip.
    struct ReleaseInput {
        let dir: URL
        let version: String
        let build: String
        let dmgName: String
        let publicURL: String

        static func fromEnvironment() throws -> ReleaseInput {
            let env = ProcessInfo.processInfo.environment
            guard let dir = env["OP_RELEASE_DIR"], let version = env["OP_RELEASE_VERSION"],
                  let build = env["OP_RELEASE_BUILD"], let publicURL = env["OP_RELEASE_DMG_URL"]
            else { throw XCTSkip("set by scripts/release.sh only (OP_RELEASE_DIR is unset)") }
            return ReleaseInput(dir: URL(fileURLWithPath: dir), version: version, build: build,
                                dmgName: "OpenPainter-\(version).dmg", publicURL: publicURL)
        }

        func read(_ name: String) throws -> Data { try Data(contentsOf: dir.appendingPathComponent(name)) }
    }

    /// Serves the release files on the local server. The local appcast was
    /// signed for a URL on a port the release script picked, so the server
    /// must listen there: the script passes OP_RELEASE_LOCAL_PORT and the
    /// local feed names http://127.0.0.1:<port>/<dmg>.
    func serveRelease(_ release: ReleaseInput) throws {
        let env = ProcessInfo.processInfo.environment
        let port = try XCTUnwrap(env["OP_RELEASE_LOCAL_PORT"].flatMap(UInt16.init), "OP_RELEASE_LOCAL_PORT")
        server.stop()
        server = try LocalHTTPServer(port: port)
        server.put("/appcast.xml", try release.read("appcast.xml"))
        server.put("/appcast-local.xml", try release.read("check/appcast-local.xml"))
        server.put("/\(release.dmgName)", try release.read(release.dmgName))
    }

    /// The published appcast, unchanged, with the real public key: Sparkle
    /// accepts its signature and offers this version and build.
    func testReleaseFeedIsAcceptedWithTheReleaseKey() throws {
        let release = try ReleaseInput.fromEnvironment()
        try serveRelease(release)
        let host = try makeBuiltAppHost("release-feed", key: nil)
        let (result, driver) = try check(host)

        let item = try XCTUnwrap(result.found, "Sparkle found the release: \(String(describing: result.finishError))")
        XCTAssertEqual(item.signingValidationStatus, .succeeded, "the feed signature was checked with the release key")
        XCTAssertEqual(item.versionString, release.build)
        XCTAssertEqual(item.displayVersionString, release.version)
        XCTAssertEqual(item.fileURL?.absoluteString, release.publicURL)
        XCTAssertEqual(item.minimumSystemVersion, "15.0.0")
        let size = try XCTUnwrap(FileManager.default.attributesOfItem(
            atPath: release.dir.appendingPathComponent(release.dmgName).path)[.size] as? NSNumber)
        XCTAssertEqual(item.contentLength, size.uint64Value)
        XCTAssertNil(result.aborted)
        XCTAssertEqual(driver.calls, [])
        print("M60 release feed accepted: version \(item.displayVersionString) build \(item.versionString)")
    }

    /// The local copy of the feed (same item, download from this server):
    /// Sparkle downloads the real DMG, its installer checks the EdDSA
    /// signature with the real key before unpacking, and installs the release
    /// app over a throwaway "version 1" OpenPainter.app in the test folder.
    func testReleaseDMGIsValidatedAndInstalledBySparkle() throws {
        let release = try ReleaseInput.fromEnvironment()
        try serveRelease(release)
        let host = try makeBuiltAppHost("release-install", key: nil)
        let info = host.bundleURL.appendingPathComponent("Contents/Info.plist")
        let plist = NSMutableDictionary(contentsOf: info)!
        plist["CFBundleVersion"] = "1"
        plist["CFBundleShortVersionString"] = "0.0.1"
        plist["SUFeedURL"] = server.url("/appcast-local.xml").absoluteString
        XCTAssertTrue(plist.write(to: info, atomically: true))
        let (_, driver) = try update(try XCTUnwrap(Bundle(url: host.bundleURL)))

        XCTAssertEqual(driver.errors, [], describe(driver.errors))
        XCTAssertTrue(driver.installed, "Sparkle reported the install: \(driver.calls)")
        XCTAssertEqual(hostVersion(at: host.bundleURL), release.build)
        XCTAssertEqual(hostVersion("CFBundleShortVersionString", at: host.bundleURL), release.version)
        XCTAssertEqual(hostVersion("CFBundleIdentifier", at: host.bundleURL), "dev.yili.OpenPainter")
        XCTAssertTrue(server.requests.contains("/\(release.dmgName)"), "\(server.requests)")
        let verify = try run(URL(fileURLWithPath: "/usr/bin/codesign"),
                             ["--verify", "--deep", "--strict", host.bundleURL.path])
        XCTAssertEqual(verify.status, 0, "the installed app's signature: \(verify.out)")
        print("M60 release DMG installed by Sparkle: build \(hostVersion(at: host.bundleURL) ?? "?")")
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
