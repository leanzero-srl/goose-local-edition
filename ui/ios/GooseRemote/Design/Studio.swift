import SwiftUI

enum Studio {
    static let canvas = Color("Canvas")
    static let surface = Color("Surface")
    static let inset = Color("Inset")
    static let ink = Color("Ink")
    static let secondaryInk = Color("SecondaryInk")
    static let mutedInk = Color("MutedInk")
    static let border = Color("Border")
    static let accent = Color("AccentColor")
    static let accentInk = Color("AccentInk")
    static let accentHover = Color("AccentHover")
    static let positive = Color("Positive")
    static let warning = Color("Warning")
    static let danger = Color("Danger")
    static let positiveInk = Color("PositiveInk")
    static let warningInk = Color("WarningInk")
    static let dangerInk = Color("DangerInk")
    static func kind(_ kind: WorkKind) -> Color {
        switch kind { case .chat: accentInk; case .agentWork: Color("Agent"); case .benchmark: Color("Benchmark"); case .shellJob: Color("Shell") }
    }
}

struct StudioPanel<Content: View>: View {
    @ViewBuilder let content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 12) { content }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(16)
            .background(Studio.surface, in: RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(Studio.border, lineWidth: 1))
    }
}

struct StatusBadge: View {
    let text: String
    var color: Color = Studio.accent
    var active = false
    var body: some View {
        HStack(spacing: 5) {
            if active { StudioLiveDot() }
            Text(text).font(.caption.weight(.semibold))
        }
            .foregroundStyle(.white).padding(.horizontal, 8).padding(.vertical, 5)
            .background(color, in: Capsule())
            .fixedSize(horizontal: false, vertical: true)
    }
}

struct Notice: View {
    let title: String
    let message: String
    var icon: StudioIconName = .info
    var color = Studio.accentInk
    var body: some View {
        StudioPanel {
            Label(title, icon: icon).font(.headline).foregroundStyle(color)
            Text(message).font(.subheadline).foregroundStyle(Studio.secondaryInk)
        }
    }
}

struct RelativeSample: View {
    let date: Date?
    let prefix: String
    var body: some View {
        if let date {
            TimelineView(.periodic(from: .now, by: 60)) { _ in
                Text("\(prefix) \(date, style: .relative) ago").font(.caption).foregroundStyle(Studio.mutedInk)
            }
        } else { Text("\(prefix): not available").font(.caption).foregroundStyle(Studio.mutedInk) }
    }
}

extension View {
    func studioScreen() -> some View {
        self.foregroundStyle(Studio.ink).background(Studio.canvas).scrollContentBackground(.hidden)
    }
}

struct StudioPrimaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.font(.body.weight(.semibold))
            .foregroundStyle(isEnabled ? Color.white : Studio.secondaryInk)
            .padding(.horizontal, 16).padding(.vertical, 12).frame(minHeight: 44)
            .background(isEnabled ? (configuration.isPressed ? Studio.accentHover : Studio.accent) : Studio.inset, in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).stroke(isEnabled ? Studio.accent : Studio.border, lineWidth: 1))
            .scaleEffect(StudioMotion.pressedScale(isPressed: configuration.isPressed, reduceMotion: reduceMotion))
            .animation(StudioMotion.transition(reduceMotion: reduceMotion), value: configuration.isPressed)
    }
}

struct StudioSecondaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func makeBody(configuration: Configuration) -> some View {
        configuration.label.font(.body.weight(.semibold))
            .foregroundStyle(!isEnabled ? Studio.mutedInk : configuration.role == .destructive ? Studio.dangerInk : Studio.accentInk)
            .padding(.horizontal, 14).padding(.vertical, 10).frame(minHeight: 44)
            .background(configuration.isPressed ? Studio.inset : Studio.surface, in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).stroke(Studio.border, lineWidth: 1))
            .scaleEffect(StudioMotion.pressedScale(isPressed: configuration.isPressed, reduceMotion: reduceMotion))
            .animation(StudioMotion.transition(reduceMotion: reduceMotion), value: configuration.isPressed)
    }
}
