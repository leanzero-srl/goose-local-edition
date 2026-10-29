import SwiftUI

struct SessionScrollState {
    private(set) var followsLatest = true
    private(set) var loadingOlder = false
    mutating func userScrolled(nearBottom: Bool) { followsLatest = nearBottom }
    mutating func beginOlder() { loadingOlder = true; followsLatest = false }
    mutating func endOlder() { loadingOlder = false }
    mutating func jumpToLatest() { followsLatest = true }
    var shouldFollowContent: Bool { followsLatest && !loadingOlder }
}

struct SessionDetailView: View {
    @Environment(ControllerStore.self) private var store
    let work: WorkRef
    @State private var action: WorkAction = .send
    @State private var confirmStop = false
    @State private var scrolling = SessionScrollState()
    @State private var visibleMessage: String?
    @State private var userScrolling = false
    @State private var nearBottom = true
    private let latestAnchor = "session-latest"
    var body: some View {
        Group {
            if store.selectedLoading && store.selectedWork == nil { ProgressView("Loading from owner Mac").frame(maxWidth: .infinity, maxHeight: .infinity) }
            else if let selected = store.selectedWork, selected.entry.work == work {
                content(selected)
            } else if let error = store.selectedError {
                ContentUnavailableView { Label("Session unavailable", icon: .offline) } description: { Text(error) } actions: {
                    Button("Try again") { Task { await store.select(work) } }.buttonStyle(StudioPrimaryButtonStyle())
                }
            } else {
                ContentUnavailableView { Label("Session not loaded", icon: work.kind.icon) }
                description: { Text("Refresh the session list and try again.") }
            }
        }
        .sheet(item: Binding(get: { store.sharingRequest }, set: { if $0 == nil { store.declineSharing() } })) { request in
            SessionSharingView(request: request).environment(store)
        }
        .studioScreen().navigationTitle(store.selectedWork?.entry.title ?? work.kind.title)
        .navigationBarTitleDisplayMode(.inline)
        .task(id: work) { if store.selectedRef != work || store.selectedError != nil { await store.select(work) } }
        .onDisappear { store.endSelection(work) }
        .onChange(of: work) { _, _ in scrolling = SessionScrollState(); visibleMessage = nil }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    if work.kind != .chat { scrolling.jumpToLatest() }
                    Task { await store.select(work) }
                } label: { StudioSyncLabel(title: "Refresh session", active: store.selectedLoading) }
            }
        }
        .confirmationDialog("Stop this run on \(store.ownerName(work.ownerNodeId))?", isPresented: $confirmStop, titleVisibility: .visible) {
            Button("Stop run", role: .destructive) { Task { await store.send(action: .stop) } }
        } message: { Text("This requests a stop for the exact run currently shown. Closing this screen does not stop work.") }
    }
    private func content(_ selected: SelectedWork) -> some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 16) {
                        if let error = store.selectedError {
                            Notice(title: "Saved view", message: error, icon: .offline)
                        }
                        if let notice = store.selectedHistoryNotice {
                            Notice(title: "History refreshed", message: notice, icon: work.kind.icon)
                        }
                        StudioPanel {
                            Label(store.ownerName(work.ownerNodeId), icon: .laptop).font(.headline)
                            HStack {
                                Text(work.kind.title).font(.subheadline)
                                Spacer()
                                StatusBadge(text: selected.entry.state.title, active: selected.entry.state == .running && store.connection == .connected && store.selectedError == nil && !store.selectedHistoryPaused)
                            }
                            if let directory = selected.entry.workingDir {
                                Text(directory).font(.caption.monospaced()).foregroundStyle(Studio.secondaryInk).textSelection(.enabled)
                            }
                            RelativeSample(date: selected.entry.updatedAt, prefix: "Owner updated")
                            if selected.entry.canPerform(.stop) {
                                Button(role: .destructive) { confirmStop = true } label: { Label("Stop run", icon: .stop) }.buttonStyle(StudioSecondaryButtonStyle())
                                    .disabled(store.selectedError != nil || store.sending || store.unfinishedCommand != nil)
                            }
                        }
                        if store.selectedHistoryPaused {
                            Notice(title: "Reading older activity", message: "Live log updates are paused to keep your place. Refresh to return to the latest activity.", icon: .pause)
                        }
                        if selected.hasMoreHistory {
                            Button(work.kind == .chat ? "Load older messages" : "Load older activity") {
                                scrolling.beginOlder()
                                Task { await store.loadMoreHistory(); scrolling.endOlder() }
                            }.buttonStyle(StudioSecondaryButtonStyle()).disabled(store.selectedLoading)
                                .accessibilityIdentifier("load-older-history")
                        }
                        ForEach(selected.pendingInputs) { input in
                            PendingInputView(input: input, supported: selected.entry.canPerform(.answerInput))
                        }
                        if selected.transcript.isEmpty {
                            Notice(title: "No visible messages", message: selected.hasMoreHistory ? "There are no visible messages in this page. Load older history to continue." : "The owner returned no visible messages for this session.", icon: work.kind.icon)
                        }
                        ForEach(selected.transcript) { item in
                            StudioPanel {
                                HStack {
                                    Text(item.role.capitalized).font(.caption.weight(.bold)).foregroundStyle(Studio.secondaryInk)
                                    Spacer()
                                    if let timestamp = item.timestamp { Text(timestamp, style: .time).font(.caption).foregroundStyle(Studio.secondaryInk) }
                                }
                                Text(item.text).font(work.kind == .shellJob ? .system(.body, design: .monospaced) : .body).textSelection(.enabled)
                            }.id(item.id)
                        }
                        if let pending = store.pendingCommands.last(where: { $0.command.work == work }) {
                            StudioPanel {
                                Label(pending.receipt?.state.title ?? "Checking delivery", icon: .send).font(.headline)
                                Text(pending.receipt?.reason ?? "Acceptance is separate from delivery. The owner’s receipt determines the result.").font(.subheadline).foregroundStyle(Studio.secondaryInk)
                                if store.unfinishedCommand != nil { Button("Check delivery") { Task { await store.checkDelivery() } }.disabled(store.sending) }
                            }
                        }
                        Color.clear.frame(height: 1).id(latestAnchor).accessibilityHidden(true)
                    }.scrollTargetLayout().padding(16).frame(maxWidth: 900)
                }
                .scrollPosition(id: $visibleMessage, anchor: .top)
                .defaultScrollAnchor(.bottom, for: .initialOffset)
                .defaultScrollAnchor(.top, for: .sizeChanges)
                .onScrollGeometryChange(for: Bool.self) { geometry in
                    geometry.contentSize.height - geometry.visibleRect.maxY <= 80
                } action: { _, isNearBottom in
                    nearBottom = isNearBottom
                    if userScrolling { scrolling.userScrolled(nearBottom: isNearBottom) }
                }
                .onScrollPhaseChange { _, phase in
                    userScrolling = phase == .tracking || phase == .interacting || phase == .decelerating
                    if userScrolling { scrolling.userScrolled(nearBottom: nearBottom) }
                }
                .task(id: selected.transcript) {
                    await Task.yield()
                    if !Task.isCancelled, !userScrolling, scrolling.shouldFollowContent { proxy.scrollTo(latestAnchor, anchor: .bottom) }
                }
                .onChange(of: store.selectedHistoryReset) { _, _ in
                    scrolling.jumpToLatest()
                    proxy.scrollTo(latestAnchor, anchor: .bottom)
                }
                .overlay(alignment: .bottomTrailing) {
                    if !scrolling.followsLatest {
                        Button {
                            Task {
                                if store.selectedHistoryPaused {
                                    await store.select(work)
                                    guard store.selectedError == nil, store.selectedRef == work else { return }
                                }
                                scrolling.jumpToLatest()
                                await Task.yield()
                                proxy.scrollTo(latestAnchor, anchor: .bottom)
                            }
                        } label: { Label("Jump to latest", systemImage: "arrow.down") }
                        .buttonStyle(StudioPrimaryButtonStyle()).padding(16)
                        .accessibilityIdentifier("jump-to-latest")
                        .accessibilityHint("Scrolls to the newest message and follows live updates")
                    }
                }
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
                Button { Task { await store.send(action: action) } } label: { Label(action.title, icon: .arrowUp) }
                    .buttonStyle(StudioPrimaryButtonStyle()).controlSize(.large)
                    .disabled(store.selectedError != nil || !entry.canPerform(action) || !store.canStoreDrafts || store.sending || store.sharingRequest != nil || store.unfinishedCommand != nil || store.draft(for: work).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(16).background(Studio.surface)
        .onAppear { if !entry.canPerform(action), let supported = [WorkAction.send, .steer, .note].first(where: { entry.canPerform($0) }) { action = supported } }
    }
}
