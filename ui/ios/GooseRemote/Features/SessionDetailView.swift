import SwiftUI

struct SessionDetailView: View {
    @Environment(ControllerStore.self) private var store
    let work: WorkRef
    @State private var action: WorkAction = .send
    @State private var confirmStop = false
    var body: some View {
        Group {
            if store.selectedLoading { ProgressView("Loading from owner Mac").frame(maxWidth: .infinity, maxHeight: .infinity) }
            else if let selected = store.selectedWork, selected.entry.work == work {
                content(selected)
            } else if let error = store.selectedError {
                ContentUnavailableView { Label("Session unavailable", systemImage: "wifi.exclamationmark") } description: { Text(error) } actions: {
                    Button("Try again") { Task { await store.select(work) } }.buttonStyle(StudioPrimaryButtonStyle())
                }
            } else { ContentUnavailableView("Session not loaded", systemImage: work.kind.symbol, description: Text("Refresh the session list and try again.")) }
        }
        .studioScreen().navigationTitle(store.selectedWork?.entry.title ?? work.kind.title)
        .navigationBarTitleDisplayMode(.inline)
        .task(id: work) { if store.selectedRef != work || store.selectedError != nil { await store.select(work) } }
        .onDisappear { store.endSelection(work) }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button { Task { await store.select(work) } } label: { Label("Refresh session", systemImage: "arrow.clockwise") }
            }
        }
        .confirmationDialog("Stop this run on \(store.ownerName(work.ownerNodeId))?", isPresented: $confirmStop, titleVisibility: .visible) {
            Button("Stop run", role: .destructive) { Task { await store.send(action: .stop) } }
        } message: { Text("This requests a stop for the exact run currently shown. Closing this screen does not stop work.") }
    }
    private func content(_ selected: SelectedWork) -> some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    if let error = store.selectedError {
                        Notice(title: "Saved view", message: error, symbol: "wifi.exclamationmark")
                    }
                    StudioPanel {
                        Label(store.ownerName(work.ownerNodeId), systemImage: "desktopcomputer").font(.headline)
                        HStack { Text(work.kind.title).font(.subheadline); Spacer(); StatusBadge(text: selected.entry.state.title) }
                        if let directory = selected.entry.workingDir {
                            Text(directory).font(.caption.monospaced()).foregroundStyle(Studio.secondaryInk).textSelection(.enabled)
                        }
                        RelativeSample(date: selected.entry.updatedAt, prefix: "Owner updated")
                        if selected.entry.canPerform(.stop) {
                            Button("Stop run", role: .destructive) { confirmStop = true }.buttonStyle(.bordered)
                                .disabled(store.selectedError != nil || store.sending || store.unfinishedCommand != nil)
                        }
                    }
                    ForEach(selected.pendingInputs) { input in
                        PendingInputView(input: input, supported: selected.entry.canPerform(.answerInput))
                    }
                    if selected.transcript.isEmpty {
                        Notice(title: "No transcript yet", message: "The owner returned no visible messages for this session.", symbol: work.kind.symbol)
                    }
                    ForEach(selected.transcript) { item in
                        StudioPanel {
                            HStack {
                                Text(item.role.capitalized).font(.caption.weight(.bold)).foregroundStyle(Studio.secondaryInk)
                                Spacer()
                                if let timestamp = item.timestamp { Text(timestamp, style: .time).font(.caption).foregroundStyle(Studio.secondaryInk) }
                            }
                            Text(item.text).font(work.kind == .shellJob ? .system(.body, design: .monospaced) : .body).textSelection(.enabled)
                        }
                    }
                    if selected.hasMoreHistory { Button("Load more history") { Task { await store.loadMoreHistory() } }.buttonStyle(.bordered) }
                    if let pending = store.pendingCommands.last(where: { $0.command.work == work }) {
                        StudioPanel {
                            Label(pending.receipt?.state.title ?? "Checking delivery", systemImage: "paperplane").font(.headline)
                            Text(pending.receipt?.reason ?? "Acceptance is separate from delivery. The owner’s receipt determines the result.").font(.subheadline).foregroundStyle(Studio.secondaryInk)
                            if store.unfinishedCommand != nil { Button("Check delivery") { Task { await store.checkDelivery() } }.disabled(store.sending) }
                        }
                    }
                }.padding(16).frame(maxWidth: 900)
            }
            if selected.entry.capabilities.contains(where: { [.send, .steer, .note].contains($0) }) {
                composer(selected.entry)
            } else {
                Text("\(work.kind.title) controls are limited to the capabilities its owner advertises.")
                    .font(.caption).foregroundStyle(Studio.secondaryInk).padding(16)
            }
        }
    }
    private func composer(_ entry: CatalogEntry) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            let actions = [WorkAction.send, .steer, .note].filter { entry.canPerform($0) }
            HStack {
                Picker("Instruction type", selection: $action) {
                    ForEach(actions, id: \.self) { Text($0.title).tag($0) }
                }.pickerStyle(.menu)
                Spacer()
                Text("To \(store.ownerName(work.ownerNodeId))").font(.caption).foregroundStyle(Studio.secondaryInk)
            }
            TextField("Write an instruction…", text: Binding(get: { store.draft(for: work) }, set: { store.saveDraft($0, for: work) }), axis: .vertical)
                .lineLimit(3...8).padding(12).background(Studio.inset, in: RoundedRectangle(cornerRadius: 8))
                .accessibilityLabel("Message draft").accessibilityIdentifier("composer")
            HStack {
                Text(store.canStoreDrafts ? "Draft saved on this device" : "Draft storage unavailable")
                    .font(.caption).foregroundStyle(Studio.secondaryInk)
                Spacer()
                Button { Task { await store.send(action: action) } } label: { Label(action.title, systemImage: "arrow.up") }
                    .buttonStyle(StudioPrimaryButtonStyle()).controlSize(.large)
                    .disabled(store.selectedError != nil || !entry.canPerform(action) || !store.canStoreDrafts || store.sending || store.unfinishedCommand != nil || store.draft(for: work).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(16).background(Studio.surface)
        .onAppear { if !entry.canPerform(action), let supported = [WorkAction.send, .steer, .note].first(where: { entry.canPerform($0) }) { action = supported } }
    }
}
