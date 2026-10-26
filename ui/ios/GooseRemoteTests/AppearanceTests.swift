import SwiftUI
import XCTest
@testable import GooseRemote

@MainActor
final class AppearanceTests: XCTestCase {
    func testThemeChoiceSurvivesASeparatePreferencesRead() throws {
        let suite = "AppearanceTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let preference = AppStorage(wrappedValue: StudioTheme.system, StudioTheme.storageKey, store: defaults)
        XCTAssertEqual(preference.wrappedValue, .system)
        for theme in StudioTheme.allCases {
            preference.wrappedValue = theme
            let reopened = AppStorage(wrappedValue: StudioTheme.system, StudioTheme.storageKey,
                                      store: try XCTUnwrap(UserDefaults(suiteName: suite)))
            XCTAssertEqual(reopened.wrappedValue, theme)
        }
    }

    func testSystemThemeLeavesAppearanceToTheDevice() {
        XCTAssertNil(StudioTheme.system.colorScheme)
        XCTAssertEqual(StudioTheme.light.colorScheme, .light)
        XCTAssertEqual(StudioTheme.dark.colorScheme, .dark)
    }

    func testReduceMotionRemovesCustomTransitionAndPressScale() {
        XCTAssertNil(StudioMotion.transition(reduceMotion: true))
        XCTAssertNotNil(StudioMotion.transition(reduceMotion: false))
        XCTAssertEqual(StudioMotion.pressedScale(isPressed: true, reduceMotion: true), 1)
        XCTAssertEqual(StudioMotion.pressedScale(isPressed: false, reduceMotion: false), 1)
        XCTAssertLessThan(StudioMotion.pressedScale(isPressed: true, reduceMotion: false), 1)
    }

    func testDesktopIconsAndLicenseAreInTheApplicationBundle() throws {
        for icon in StudioIconName.allCases {
            XCTAssertNotNil(UIImage(named: icon.assetName), "Missing desktop icon: \(icon.assetName)")
        }
        XCTAssertNotNil(UIImage(named: "GooseBrand"))
        let url = try XCTUnwrap(Bundle.main.url(forResource: "Lucide-LICENSE", withExtension: "txt"))
        let license = try String(contentsOf: url, encoding: .utf8)
        XCTAssertTrue(license.hasPrefix("ISC License"))
        XCTAssertTrue(license.contains("Lucide Contributors 2026"))
        XCTAssertTrue(license.contains("The MIT License (MIT) (for portions derived from Feather)"))
    }

    func testTextAndSolidButtonContrastAcrossBothThemes() throws {
        let texts = ["Ink", "SecondaryInk", "MutedInk", "AccentInk", "PositiveInk", "WarningInk", "DangerInk", "Agent", "Benchmark", "Shell"]
        for style in [UIUserInterfaceStyle.light, .dark] {
            let traits = UITraitCollection(userInterfaceStyle: style)
            for surface in ["Canvas", "Surface", "Inset"] {
                let background = try color(surface, traits)
                for text in texts {
                    XCTAssertGreaterThanOrEqual(contrast(try color(text, traits), background), 4.5,
                                                "\(text) on \(surface), \(style)")
                }
            }
            for fill in ["AccentColor", "AccentHover", "Positive", "Warning", "Danger"] {
                XCTAssertGreaterThanOrEqual(contrast(try color(fill, traits), .white), 4.5,
                                            "White text on \(fill), \(style)")
            }
        }
    }

    private func color(_ name: String, _ traits: UITraitCollection) throws -> UIColor {
        try XCTUnwrap(UIColor(named: name)).resolvedColor(with: traits)
    }

    private func contrast(_ first: UIColor, _ second: UIColor) -> Double {
        let values = [luminance(first), luminance(second)].sorted()
        return (values[1] + 0.05) / (values[0] + 0.05)
    }

    private func luminance(_ color: UIColor) -> Double {
        var red: CGFloat = 0, green: CGFloat = 0, blue: CGFloat = 0, alpha: CGFloat = 0
        XCTAssertTrue(color.getRed(&red, green: &green, blue: &blue, alpha: &alpha))
        let linear = [red, green, blue].map { value in
            let component = Double(value)
            return component <= 0.04045 ? component / 12.92 : pow((component + 0.055) / 1.055, 2.4)
        }
        return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
    }
}
