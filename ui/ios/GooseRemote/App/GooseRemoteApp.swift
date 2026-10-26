import SwiftUI
import Observation

@main
struct GooseRemoteApp: App {
    @AppStorage(StudioTheme.storageKey) private var theme: StudioTheme = .system
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var store: ControllerStore = {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("GooseRemote", isDirectory: true)
        let mesh = EmbeddedMeshConnectionProvider(directory: base.appendingPathComponent("mesh", isDirectory: true))
        return ControllerStore(owner: OwnerHTTPControllerService(connections: mesh), directory: base, mesh: mesh)
    }()
    @Environment(\.scenePhase) private var scenePhase
    var body: some Scene {
        WindowGroup {
            RootView().environment(store).tint(Studio.accentInk)
                .preferredColorScheme(theme.colorScheme)
                .transaction { if reduceMotion { $0.disablesAnimations = true } }
                .task {
                    if store.accountDeletion?.blocksAccountAccess == true { await store.checkAccountDeletion() }
                    else if store.authenticated { store.manualSync() }
                }
                .onChange(of: scenePhase) { _, phase in
                    if phase == .active { store.sceneActive() }
                    if phase == .background { Task { await store.sceneBackground() } }
                }
        }
    }
}

enum AppSection: String, CaseIterable, Identifiable {
    case sessions, nodes, settings
    var id: Self { self }
    var title: String { rawValue.capitalized }
    var icon: StudioIconName {
        switch self { case .sessions: .messageSquare; case .nodes: .network; case .settings: .settings }
    }
}

@MainActor @Observable
final class RootNavigation {
    var section: AppSection = .sessions
    var settingsPath: [SettingsDestination] = []

    func showSettingsRoot() {
        settingsPath = []
        section = .settings
    }
}

struct RootView: View {
    @Environment(ControllerStore.self) private var store
    @State private var navigation: RootNavigation
    init(navigation: RootNavigation = RootNavigation()) {
        _navigation = State(initialValue: navigation)
    }
    var body: some View {
        @Bindable var store = store
        @Bindable var navigation = navigation
        TabView(selection: $navigation.section) {
            Tab(value: AppSection.sessions) {
                SessionsView(connect: navigation.showSettingsRoot)
            } label: { Label("Sessions", image: AppSection.sessions.icon.assetName) }
            Tab(value: AppSection.nodes) {
                NavigationStack { NodesView(connect: navigation.showSettingsRoot) }
            } label: { Label("Nodes", image: AppSection.nodes.icon.assetName) }
            Tab(value: AppSection.settings) {
                NavigationStack(path: $navigation.settingsPath) { SettingsView() }
            } label: { Label("Settings", image: AppSection.settings.icon.assetName) }
        }
        .toolbarBackground(Studio.surface, for: .tabBar)
        .toolbarBackground(.visible, for: .tabBar)
        .onChange(of: store.accountDeletion?.phase, initial: true) { _, phase in
            if phase != nil { navigation.showSettingsRoot() }
        }
        .alert("Could not complete the request", isPresented: Binding(get: { store.errorMessage != nil }, set: { if !$0 { store.errorMessage = nil } })) {
            Button("OK") { store.errorMessage = nil }
        } message: { Text(store.errorMessage ?? "") }
    }
}
