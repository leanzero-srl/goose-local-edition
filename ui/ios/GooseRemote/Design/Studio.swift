import SwiftUI

enum Studio {
    static let canvas = Color("Canvas")
    static let surface = Color("Surface")
    static let inset = Color("Inset")
    static let ink = Color("Ink")
    static let secondaryInk = Color("SecondaryInk")
    static let border = Color("Border")
    static let accent = Color("AccentColor")
    static let accentInk = Color("AccentInk")
    static let positive = Color("Positive")
    static let warning = Color("Warning")
    static let danger = Color("Danger")
    static func kind(_ kind: WorkKind) -> Color {
        switch kind { case .chat: accent; case .agentWork: Color("Agent"); case .benchmark: Color("Benchmark"); case .shellJob: Color("Shell") }
    }
}

struct StudioPanel<Content: View>: View {
    @ViewBuilder let content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 12) { content }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(16)
            .background(Studio.surface, in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(Studio.border, lineWidth: 1))
    }
}

struct StatusBadge: View {
    let text: String
    var color: Color = Studio.accent
    var body: some View {
        Text(text).font(.caption.weight(.semibold))
            .foregroundStyle(.white).padding(.horizontal, 8).padding(.vertical, 5)
            .background(color, in: Capsule())
            .fixedSize(horizontal: false, vertical: true)
    }
}

struct Notice: View {
    let title: String
    let message: String
    var symbol = "info.circle"
    var color = Studio.accent
    var body: some View {
        StudioPanel {
            Label(title, systemImage: symbol).font(.headline).foregroundStyle(color)
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
                Text("\(prefix) \(date, style: .relative) ago").font(.caption).foregroundStyle(Studio.secondaryInk)
            }
        } else { Text("\(prefix): not available").font(.caption).foregroundStyle(Studio.secondaryInk) }
    }
}

extension View {
    func studioScreen() -> some View {
        self.foregroundStyle(Studio.ink).background(Studio.canvas).scrollContentBackground(.hidden)
    }
}

struct StudioPrimaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.font(.body.weight(.semibold))
            .foregroundStyle(isEnabled ? Color.white : Studio.secondaryInk)
            .padding(.horizontal, 16).padding(.vertical, 12).frame(minHeight: 44)
            .background(isEnabled ? Studio.accent : Studio.inset, in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(isEnabled ? Studio.accent : Studio.border, lineWidth: 1))
            .scaleEffect(configuration.isPressed ? 0.98 : 1)
    }
}
