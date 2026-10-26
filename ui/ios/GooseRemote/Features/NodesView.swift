import SwiftUI

struct NodesView: View {
    @Environment(ControllerStore.self) private var store
    let connect: () -> Void
    var body: some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Your compute stays here").font(.headline)
                    Text("This device is a controller. Your Macs run sessions and keep their files.").font(.subheadline).foregroundStyle(Studio.secondaryInk)
                }.padding(.vertical, 6)
            }.listRowBackground(Studio.surface)
            if !store.authenticated {
                Section { Button("Connect this device", action: connect).buttonStyle(StudioPrimaryButtonStyle()).controlSize(.large) }.listRowBackground(Studio.surface)
            } else {
                Section { SyncSummary() }.listRowBackground(Studio.surface)
                Section("Macs") {
                    if store.hosts.isEmpty {
                        Text(store.lastSync == nil ? "Sync Link to load registered Macs." : "No Macs have registered controller access for this account.")
                            .foregroundStyle(Studio.secondaryInk)
                    }
                    ForEach(store.hosts) { host in
                        NavigationLink { NodeDetailView(host: host) } label: {
                            VStack(alignment: .leading, spacing: 8) {
                                Label(host.name, systemImage: "desktopcomputer").font(.headline)
                                StatusBadge(text: host.statusLabel, color: host.controllerAvailable ? Studio.positive : Studio.warning)
                                RelativeSample(date: host.lastSeenAt, prefix: "Last seen")
                            }.padding(.vertical, 6)
                        }
                    }
                }.listRowBackground(Studio.surface)
            }
        }.navigationTitle("Nodes").studioScreen()
            .refreshable { store.manualSync() }
            .toolbar { if store.authenticated { Button { store.manualSync() } label: { Label("Sync all Macs", systemImage: "arrow.triangle.2.circlepath") }.disabled(store.syncRunning) } }
    }
}

struct NodeDetailView: View {
    @Environment(ControllerStore.self) private var store
    let host: HostDescriptor
    var body: some View {
        List {
            Section("Connection") {
                LabeledContent("Owner", value: host.name)
                LabeledContent("Status", value: host.statusLabel)
                LabeledContent("Controller protocol", value: String(host.protocolVersion))
                RelativeSample(date: host.lastSeenAt, prefix: "Last seen")
                if !host.controllerAvailable {
                    Text("Open Goose Swarm on this Mac and finish Link hosting setup. Session access requires a verified secure endpoint.")
                        .font(.subheadline).foregroundStyle(Studio.secondaryInk)
                }
            }.listRowBackground(Studio.surface)
            Section("Resources") {
                if let sample = store.resources[host.id] {
                    RelativeSample(date: sample.sampledAt, prefix: "Sampled")
                    LabeledContent("CPU load / core", value: sample.cpuLoadPerCore.map { $0.formatted(.number.precision(.fractionLength(2))) } ?? "Unknown")
                    LabeledContent("Memory available", value: sample.memoryFreePercent.map { "\($0.formatted(.number.precision(.fractionLength(1))))%" } ?? "Unknown")
                    LabeledContent("Memory capacity", value: sample.memoryTotalGB.map { "\($0.formatted(.number.precision(.fractionLength(1)))) GiB" } ?? "Unknown")
                } else {
                    Label("No resource sample received", systemImage: "chart.xyaxis.line")
                    Text("CPU and memory stay unknown until this Mac supplies a timestamped sample.").font(.subheadline).foregroundStyle(Studio.secondaryInk)
                }
                Button("Refresh resource sample") { Task { await store.loadResources(host) } }.disabled(!host.controllerAvailable)
            }.listRowBackground(Studio.surface)
            Section("Manage this Mac") {
                NavigationLink { ManagementAvailabilityView(kind: .models, host: host) } label: { Label("Models & downloads", systemImage: "arrow.down.circle") }
                NavigationLink { ManagementAvailabilityView(kind: .providers, host: host) } label: { Label("Providers", systemImage: "key.horizontal") }
                NavigationLink { ManagementAvailabilityView(kind: .setup, host: host) } label: { Label("Node setup", systemImage: "desktopcomputer.and.arrow.down") }
            }.listRowBackground(Studio.surface)
        }.navigationTitle(host.name).studioScreen()
    }
}

enum ManagementKind {
    case models, providers, setup
    var title: String { switch self { case .models: "Models & downloads"; case .providers: "Providers"; case .setup: "Node setup" } }
    var symbol: String { switch self { case .models: "arrow.down.circle"; case .providers: "key.horizontal"; case .setup: "desktopcomputer.and.arrow.down" } }
    var detail: String {
        switch self {
        case .models: "Model downloads require an owner service that accepts a pinned model revision and reports real progress. This build has no connected download service. Manage models in Goose Swarm on the Mac."
        case .providers: "Remote provider setup is not connected in this build. Configure providers in Goose Swarm on the Mac. Existing provider secrets are never displayed here."
        case .setup: "Install or update Goose Swarm on the Mac, sign in to Link with the same account, and enable controller access. This phone or tablet will not enroll as a compute node."
        }
    }
}
struct ManagementAvailabilityView: View {
    let kind: ManagementKind
    let host: HostDescriptor
    var body: some View {
        ContentUnavailableView { Label(kind.title, systemImage: kind.symbol) } description: {
            Text("\(host.name)\n\n\(kind.detail)")
        }.navigationTitle(kind.title).navigationBarTitleDisplayMode(.inline).studioScreen()
    }
}
