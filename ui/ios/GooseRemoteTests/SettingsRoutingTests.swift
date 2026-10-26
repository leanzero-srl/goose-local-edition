import SwiftUI
import XCTest
@testable import GooseRemote

@MainActor final class SettingsRoutingTests: XCTestCase {
    func testExplicitConnectReturnsFromOldSettingsDetailToSignInRoot() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let store = ControllerStore(credentials: MemoryCredentials(), directory: directory)
        let navigation = RootNavigation()
        navigation.section = .settings
        let scene = try XCTUnwrap(UIApplication.shared.connectedScenes.first as? UIWindowScene)
        let previous = scene.keyWindow
        let window = UIWindow(windowScene: scene)
        let host = UIHostingController(rootView: RootView(navigation: navigation).environment(store))
        window.rootViewController = host
        window.overrideUserInterfaceStyle = .light
        window.makeKeyAndVisible()
        defer { window.isHidden = true; previous?.makeKey() }
        await waitForTitle("Settings", under: host)
        navigation.settingsPath = [.iconLicense]
        await waitForTitle("Icon license", under: host)
        navigation.section = .sessions
        await waitForTitle("Sessions", under: host)
        XCTAssertEqual(navigation.settingsPath, [.iconLicense])
        navigation.showSettingsRoot()
        await waitForTitle("Settings", under: host)
        XCTAssertTrue(navigation.settingsPath.isEmpty)
        XCTAssertFalse(store.authenticated)
        let screenshot = UIGraphicsImageRenderer(bounds: window.bounds).image { _ in
            window.drawHierarchy(in: window.bounds, afterScreenUpdates: true)
        }
        let attachment = XCTAttachment(image: screenshot)
        attachment.name = "Connect returns to Settings sign-in"
        attachment.lifetime = .keepAlways
        add(attachment)
        navigation.settingsPath = [.privacy]
        await waitForTitle("Connection & privacy", under: host)
        let privacy = UIGraphicsImageRenderer(bounds: window.bounds).image { _ in
            window.drawHierarchy(in: window.bounds, afterScreenUpdates: true)
        }
        let policyAttachment = XCTAttachment(image: privacy)
        policyAttachment.name = "Privacy policy and support links"
        policyAttachment.lifetime = .keepAlways
        add(policyAttachment)
    }

    private func waitForTitle(_ title: String, under root: UIViewController) async {
        let shown = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            MainActor.assumeIsolated { self.visibleNavigation(root)?.navigationBar.topItem?.title == title }
        }, object: nil)
        await fulfillment(of: [shown], timeout: 3)
    }

    private func visibleNavigation(_ root: UIViewController) -> UINavigationController? {
        if let navigation = root as? UINavigationController { return navigation }
        if let tabs = root as? UITabBarController, let selected = tabs.selectedViewController { return visibleNavigation(selected) }
        for child in root.children {
            if let result = visibleNavigation(child) { return result }
        }
        return nil
    }
}
