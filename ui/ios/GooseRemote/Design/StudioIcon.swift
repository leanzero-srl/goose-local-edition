import SwiftUI

// Exact paths from the desktop's lucide-react dependency; license in About > Icon license.
enum StudioIconName: String, CaseIterable {
    case messageSquare = "message-square", bot, gauge, terminal, network, settings, laptop, monitor, sun, moon
    case refresh = "refresh-cw", plus, chart = "chart-no-axes-combined", download, cloud, info
    case offline = "wifi-off", pause, send, arrowUp = "arrow-up", question = "circle-help"
    case chevronRight = "chevron-right", check, stop = "square", play, note = "file-text"

    var assetName: String { "Lucide-\(rawValue)" }
}

struct StudioIcon: View {
    let name: StudioIconName
    var size: CGFloat = 20

    init(_ name: StudioIconName, size: CGFloat = 20) {
        self.name = name
        self.size = size
    }

    var body: some View {
        Image(name.assetName).renderingMode(.template).resizable().scaledToFit()
            .frame(width: size, height: size).accessibilityHidden(true)
    }
}

extension Label where Title == Text, Icon == StudioIcon {
    init(_ title: String, icon: StudioIconName) {
        self.init { Text(title) } icon: { StudioIcon(icon) }
    }
}

struct StudioSyncLabel: View {
    let title: String
    let active: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        Label {
            Text(title)
        } icon: {
            if active && !reduceMotion && scenePhase == .active {
                TimelineView(.animation(minimumInterval: 1.0 / 30)) { context in
                    StudioIcon(.refresh)
                        .rotationEffect(.degrees(context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 1.2) / 1.2 * 360))
                }
            } else {
                StudioIcon(.refresh)
            }
        }
    }
}

struct StudioLiveDot: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        Group {
            if !reduceMotion && scenePhase == .active {
                TimelineView(.animation(minimumInterval: 1.0 / 30)) { context in
                    let phase = context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 1.2) / 1.2
                    Circle().frame(width: 5, height: 5).scaleEffect(1 + 0.2 * (1 - cos(phase * 2 * .pi)))
                }
            } else {
                Circle().frame(width: 5, height: 5)
            }
        }
        .frame(width: 8, height: 8).accessibilityHidden(true)
    }
}

struct LucideLicenseView: View {
    private var license: Result<String, Error> {
        Result {
            guard let url = Bundle.main.url(forResource: "Lucide-LICENSE", withExtension: "txt") else {
                throw ControllerError.unavailable("The bundled icon license is missing.")
            }
            return try String(contentsOf: url, encoding: .utf8)
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Lucide icons").font(.title2.bold())
                Text("The same vector icons used by Goose Swarm on desktop.").foregroundStyle(Studio.secondaryInk)
                switch license {
                case .success(let text): Text(text).font(.footnote).textSelection(.enabled)
                case .failure(let error): Text(error.localizedDescription).foregroundStyle(Studio.dangerInk)
                }
            }.padding(20).frame(maxWidth: 760, alignment: .leading)
        }.frame(maxWidth: .infinity).navigationTitle("Icon license").studioScreen()
    }
}
