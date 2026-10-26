import SwiftUI

enum StudioTheme: String, CaseIterable, Identifiable {
    case system, light, dark

    static let storageKey = "studio.theme"
    var id: Self { self }
    var title: String { rawValue.capitalized }
    var colorScheme: ColorScheme? {
        switch self { case .system: nil; case .light: .light; case .dark: .dark }
    }
    var icon: StudioIconName {
        switch self { case .system: .monitor; case .light: .sun; case .dark: .moon }
    }
}

enum StudioMotion {
    // Desktop main.css: duration-120 ease-lz. Reduce Motion removes custom transitions.
    static func transition(reduceMotion: Bool) -> Animation? {
        reduceMotion ? nil : .timingCurve(0, 0, 0.2, 1, duration: 0.12)
    }

    static func pressedScale(isPressed: Bool, reduceMotion: Bool) -> CGFloat {
        isPressed && !reduceMotion ? 0.98 : 1
    }
}

struct StudioThemePicker: View {
    @Binding var selection: StudioTheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: 4) {
            ForEach(StudioTheme.allCases) { theme in
                Button { selection = theme } label: {
                    VStack(spacing: 6) {
                        StudioIcon(theme.icon)
                        Text(theme.title).font(.subheadline.weight(.semibold))
                    }
                    .frame(maxWidth: .infinity, minHeight: 56)
                    .padding(.vertical, 6)
                    .foregroundStyle(selection == theme ? Color.white : Studio.secondaryInk)
                    .background(selection == theme ? Studio.accent : Studio.inset, in: RoundedRectangle(cornerRadius: 6))
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(theme.title) theme")
                .accessibilityAddTraits(selection == theme ? .isSelected : [])
                .accessibilityIdentifier("theme-\(theme.rawValue)")
            }
        }
        .animation(StudioMotion.transition(reduceMotion: reduceMotion), value: selection)
    }
}

struct StudioDisclosureStyle: DisclosureGroupStyle {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func makeBody(configuration: Configuration) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Button {
                withAnimation(StudioMotion.transition(reduceMotion: reduceMotion)) {
                    configuration.isExpanded.toggle()
                }
            } label: {
                HStack {
                    configuration.label
                    Spacer(minLength: 8)
                    StudioIcon(.chevronRight, size: 16)
                        .rotationEffect(.degrees(configuration.isExpanded ? 90 : 0))
                }
                .frame(minHeight: 44)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityValue(configuration.isExpanded ? "Expanded" : "Collapsed")
            if configuration.isExpanded { configuration.content }
        }
    }
}
