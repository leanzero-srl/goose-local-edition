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
                Section { NavigationLink { AddMacView() } label: { Label("Add a Mac", icon: .plus) } }.listRowBackground(Studio.surface)
                Section("Macs") {
                    if store.hosts.isEmpty {
                        Text(store.lastSync == nil ? "Sync Link to load registered Macs." : "No Macs have registered controller access for this account.")
                            .foregroundStyle(Studio.secondaryInk)
                    }
                    ForEach(store.hosts) { host in
                        NavigationLink { NodeDetailView(host: host) } label: {
                            VStack(alignment: .leading, spacing: 8) {
                                Label(host.name, icon: .laptop).font(.headline)
                                StatusBadge(text: host.statusLabel, color: host.controllerAvailable ? Studio.positive : Studio.warning)
                                RelativeSample(date: host.lastSeenAt, prefix: "Last seen")
                            }.padding(.vertical, 6)
                        }
                    }
                }.listRowBackground(Studio.surface)
            }
        }.navigationTitle("Nodes").studioScreen()
            .refreshable { await store.synchronize() }
            .toolbar { if store.authenticated { Button { store.manualSync() } label: { StudioSyncLabel(title: "Sync all Macs", active: store.syncRunning) }.disabled(store.syncRunning) } }
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
                    Label("No resource sample received", icon: .chart)
                    Text("CPU and memory stay unknown until this Mac supplies a timestamped sample.").font(.subheadline).foregroundStyle(Studio.secondaryInk)
                }
                Button("Refresh resource sample") { Task { await store.loadResources(host) } }.disabled(!host.controllerAvailable)
            }.listRowBackground(Studio.surface)
            Section("Manage this Mac") {
                NavigationLink { ManagementView(kind: .models, host: host) } label: { Label("Models & downloads", icon: .download) }
                NavigationLink { ManagementView(kind: .providers, host: host) } label: { Label("Providers", icon: .cloud) }
                NavigationLink { ManagementView(kind: .nodes, host: host) } label: { Label("Compute nodes", icon: .laptop) }
            }.listRowBackground(Studio.surface)
        }.navigationTitle(host.name).studioScreen()
    }
}

enum ManagementKind {
    case models, providers, nodes
    var title: String { switch self { case .models: "Models & downloads"; case .providers: "Providers"; case .nodes: "Compute nodes" } }
}
