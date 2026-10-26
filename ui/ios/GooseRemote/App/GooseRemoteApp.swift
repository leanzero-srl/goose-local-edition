import SwiftUI

@main
struct GooseRemoteApp: App {
    @State private var store = ControllerStore()
    @Environment(\.scenePhase) private var scenePhase
    var body: some Scene {
        WindowGroup {
            RootView().environment(store).tint(Studio.accent)
                .task { if store.authenticated { store.manualSync() } }
                .onChange(of: scenePhase) { _, phase in
                    if phase == .active && store.authenticated { store.manualSync() }
                }
        }
    }
}

enum AppSection: String, CaseIterable, Identifiable {
    case sessions, nodes, settings
    var id: Self { self }
    var title: String { rawValue.capitalized }
    var symbol: String {
        switch self { case .sessions: "bubble.left.and.bubble.right"; case .nodes: "desktopcomputer"; case .settings: "gearshape" }
    }
}

struct RootView: View {
    @Environment(ControllerStore.self) private var store
    @State private var section: AppSection = .sessions
    var body: some View {
        @Bindable var store = store
        TabView(selection: $section) {
            Tab("Sessions", systemImage: AppSection.sessions.symbol, value: AppSection.sessions) {
                SessionsView(connect: { section = .settings })
            }
            Tab("Nodes", systemImage: AppSection.nodes.symbol, value: AppSection.nodes) {
                NavigationStack { NodesView(connect: { section = .settings }) }
            }
            Tab("Settings", systemImage: AppSection.settings.symbol, value: AppSection.settings) {
                NavigationStack { SettingsView() }
            }
        }
        .alert("Could not complete the request", isPresented: Binding(get: { store.errorMessage != nil }, set: { if !$0 { store.errorMessage = nil } })) {
            Button("OK") { store.errorMessage = nil }
        } message: { Text(store.errorMessage ?? "") }
    }
}
