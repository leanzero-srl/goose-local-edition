import SwiftUI

struct SessionsView: View {
    @Environment(ControllerStore.self) private var store
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var filter: WorkKind?
    let connect: () -> Void
    var filteredEntries: [CatalogEntry] {
        store.entries.filter { !$0.archived && $0.state != .deleted && (filter == nil || $0.work.kind == filter) }
            .sorted { $0.updatedAt > $1.updatedAt }
    }
    var body: some View {
        if sizeClass == .regular {
            NavigationSplitView {
                list
                    .navigationTitle("Sessions")
                    .navigationSplitViewColumnWidth(min: 300, ideal: 360, max: 460)
            } detail: {
                if let selected = store.selectedRef { SessionDetailView(work: selected) }
                else { ContentUnavailableView("Choose a session", systemImage: "bubble.left.and.bubble.right", description: Text("Read and control work on the Mac that owns it.")).studioScreen() }
            }
        } else {
            NavigationStack {
                list.navigationTitle("Sessions")
                    .navigationDestination(for: WorkRef.self) { SessionDetailView(work: $0) }
            }
        }
    }
    private var list: some View {
        List {
            Section {
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "point.3.connected.trianglepath.dotted").font(.title2).foregroundStyle(Studio.accent)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Goose Swarm").font(.headline)
                        Text("Work stays on its Mac.").font(.subheadline).foregroundStyle(Studio.secondaryInk)
                    }
                }.padding(.vertical, 6)
            }.listRowBackground(Studio.surface)
            if !store.authenticated {
                Section {
                    VStack(alignment: .leading, spacing: 16) {
                        Text("Your Macs, within reach").font(.title2.bold())
                        Text("Connect to Link to find sessions, follow progress, and send instructions to their original owner.")
                            .foregroundStyle(Studio.secondaryInk)
                        Button("Connect this device", action: connect).buttonStyle(StudioPrimaryButtonStyle()).controlSize(.large)
                    }.padding(.vertical, 16)
                }.listRowBackground(Studio.surface)
            } else {
                Section {
                    SyncSummary()
                }.listRowBackground(Studio.surface)
                Section {
                    Picker("Session type", selection: $filter) {
                        Text("All types").tag(Optional<WorkKind>.none)
                        ForEach(WorkKind.allCases, id: \.self) { kind in Label(kind.title, systemImage: kind.symbol).tag(Optional(kind)) }
                    }
                    if filteredEntries.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            Text(emptyTitle).font(.headline)
                            Text(emptyMessage).font(.subheadline).foregroundStyle(Studio.secondaryInk)
                        }.padding(.vertical, 16)
                    }
                    ForEach(filteredEntries) { entry in
                        if sizeClass == .regular {
                            Button { Task { await store.select(entry.work) } } label: { SessionRow(entry: entry) }
                                .buttonStyle(.plain)
                                .listRowBackground(store.selectedRef == entry.work ? Studio.inset : Studio.surface)
                        } else {
                            NavigationLink(value: entry.work) { SessionRow(entry: entry) }
                        }
                    }
                } header: { Text("Sessions on your Macs") }.listRowBackground(Studio.surface)
            }
        }
        .listStyle(.insetGrouped).studioScreen()
        .refreshable { await store.synchronize() }
        .toolbar {
            if store.authenticated {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { store.manualSync() } label: { Label("Sync all Macs", systemImage: "arrow.triangle.2.circlepath") }
                        .disabled(store.syncRunning).accessibilityIdentifier("manualSync")
                }
            }
        }
    }
    private var emptyTitle: String {
        if store.syncRunning { return "Checking your Macs" }
        if store.hosts.isEmpty && store.lastSync != nil { return "No Macs registered" }
        if !store.syncResults.isEmpty && store.syncResults.allSatisfy({ if case .complete = $0.state { return true }; return false }) { return "No sessions found" }
        return "Sessions are unavailable"
    }
    private var emptyMessage: String {
        if store.syncRunning { return "Sessions appear after their owner responds." }
        if store.hosts.isEmpty && store.lastSync != nil { return "Enable Link hosting in Goose Swarm on a Mac using the same account." }
        return "Check each Mac’s connection in Nodes. A Mac must support controller access before its sessions can be loaded."
    }
}

struct SessionRow: View {
    @Environment(ControllerStore.self) private var store
    let entry: CatalogEntry
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: entry.work.kind.symbol).foregroundStyle(Studio.kind(entry.work.kind)).frame(width: 24)
                Text(entry.title.isEmpty ? "Untitled \(entry.work.kind.title.lowercased())" : entry.title).font(.headline).lineLimit(3)
            }
            HStack(alignment: .top) {
                Text(entry.work.kind.title).font(.caption.weight(.semibold)).foregroundStyle(Studio.kind(entry.work.kind))
                Spacer(minLength: 8)
                StatusBadge(text: entry.state.title, color: statusColor)
            }
            Label(store.ownerName(entry.work.ownerNodeId), systemImage: "desktopcomputer").font(.subheadline).foregroundStyle(Studio.secondaryInk)
            RelativeSample(date: entry.updatedAt, prefix: "Updated")
        }.padding(.vertical, 8).accessibilityElement(children: .combine)
    }
    private var statusColor: Color {
        switch entry.state { case .failed: Studio.danger; case .waitingInput, .interrupted: Studio.warning; case .finished: Studio.positive; default: Studio.accent }
    }
}

struct SyncSummary: View {
    @Environment(ControllerStore.self) private var store
    var body: some View {
        DisclosureGroup {
            ForEach(store.syncResults) { result in
                VStack(alignment: .leading, spacing: 4) {
                    Text(result.name).font(.subheadline.weight(.semibold))
                    switch result.state {
                    case .waiting: Text("Waiting").font(.caption)
                    case .syncing: ProgressView("Loading sessions")
                    case .complete(let count): Text("\(count) sessions synchronized").font(.caption).foregroundStyle(Studio.positive)
                    case .unavailable(let message): Text(message).font(.caption).foregroundStyle(Studio.warning)
                    case .failed(let message): Text(message).font(.caption).foregroundStyle(Studio.danger)
                    }
                }.padding(.vertical, 4)
            }
            if case .stale(let message) = store.connection { Text(message).font(.subheadline).foregroundStyle(Studio.danger) }
        } label: {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    if store.syncRunning { ProgressView().controlSize(.small) }
                    Label(store.syncRunning ? "Syncing fleet" : store.connection.title, systemImage: "network")
                        .font(.subheadline.weight(.semibold))
                }
                RelativeSample(date: store.lastSync, prefix: "Last checked")
            }
        }
    }
}
