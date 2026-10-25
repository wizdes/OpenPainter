// Help > Check for Updates… (M58): Sparkle 2 (https://sparkle-project.org/documentation/).
//
// How it works:
// - The app's Info.plist (mac/project.yml) holds Sparkle's settings: SUFeedURL
//   (the appcast M60 publishes), SUPublicEDKey (the EdDSA public key that
//   checks every update), SURequireSignedFeed and SUVerifyUpdateBeforeExtraction
//   (the appcast and each archive must carry a valid EdDSA signature; see the
//   "Signing feeds" part of Sparkle's documentation).
// - SUPublicEDKey stays empty until M60: the release key pair is a credential
//   and needs the user's OK. While it is empty the updater never starts, and
//   Check for Updates says "Updates are not set up yet".
// - With a key, Sparkle's standard controller starts at launch (its own
//   scheduled checks and update windows) and the menu item calls its
//   checkForUpdates:.
import AppKit
import Sparkle

/// Target of Help > Check for Updates… .
@MainActor @objc public protocol UpdateMenuActions {
    func checkForUpdates(_ sender: Any?)
}

/// Sparkle's settings as the app's Info.plist gives them.
public struct UpdateSettings: Equatable, Sendable {
    /// The feed M60 publishes. Until then nothing is served there.
    public static let placeholderFeed = "https://yili.dev/projects/openpainter/appcast.xml"

    public let feedURL: URL?
    /// Base64 EdDSA (ed25519) public key; empty until M60.
    public let publicKey: String

    public init(feedURL: URL?, publicKey: String) {
        self.feedURL = feedURL
        self.publicKey = publicKey.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Reads SUFeedURL and SUPublicEDKey from an Info.plist dictionary.
    public init(info: [String: Any]) {
        let feed = (info["SUFeedURL"] as? String).flatMap { URL(string: $0) }
        self.init(feedURL: feed, publicKey: info["SUPublicEDKey"] as? String ?? "")
    }

    /// The running app's settings.
    public static var main: UpdateSettings { UpdateSettings(info: Bundle.main.infoDictionary ?? [:]) }

    /// Sparkle can run only with a feed and a public key.
    public var isSetUp: Bool { feedURL != nil && !publicKey.isEmpty }
}

@MainActor
public final class UpdateMenuTarget: NSObject, UpdateMenuActions {
    public static let shared = UpdateMenuTarget()

    public static let notSetUpTitle = "Updates are not set up yet"
    public static let notSetUpDetail =
        "This copy of OpenPainter has no update signing key yet, so it cannot check for new versions."

    public var settings: UpdateSettings = .main
    /// Shows an alert. Tests replace it so no modal window runs.
    public var present: (NSAlert) -> Void = { _ = $0.runModal() }
    private(set) var controller: SPUStandardUpdaterController?

    /// Starts Sparkle (scheduled checks) when the settings are complete. Does
    /// nothing while the public key is empty. Safe to call twice.
    public func startIfSetUp() {
        guard settings.isSetUp, controller == nil else { return }
        controller = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil)
    }

    /// True once Sparkle's controller exists.
    public var isRunning: Bool { controller != nil }

    @objc public func checkForUpdates(_ sender: Any?) {
        guard settings.isSetUp else {
            present(Self.notSetUpAlert())
            return
        }
        startIfSetUp()
        controller?.checkForUpdates(sender)
    }

    /// The alert Check for Updates shows while the public key is empty.
    public static func notSetUpAlert() -> NSAlert {
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = notSetUpTitle
        alert.informativeText = notSetUpDetail
        alert.addButton(withTitle: "OK")
        return alert
    }
}
