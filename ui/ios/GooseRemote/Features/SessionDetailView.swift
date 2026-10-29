import SwiftUI
#if DEBUG
import OSLog
private let sessionScrollLogger = Logger(subsystem: "net.leanzero.GooseRemote", category: "SessionScrollTrace")
#endif

enum SessionScrollTarget: Hashable {
    case message(String)
    case latest
}

private extension TranscriptItem {
    var sessionScrollTarget: SessionScrollTarget { .message(id) }
}

struct SessionTailMeasurement: Equatable {
    let overflowPixels: Int
    let contentHeight: CGFloat
}

struct SessionScrollState {
    var position = ScrollPosition(id: SessionScrollTarget.latest, anchor: .bottom)
    private(set) var followsLatest = true
    private(set) var loadingOlder = false
    private var olderRequest: UUID?
    private var olderFallbackID: String?
    private var lastTailCorrection: SessionTailMeasurement?
    func trace(_ reason: String) {
#if DEBUG
        let target: String
        switch position.viewID(type: SessionScrollTarget.self) {
        case .latest: target = "latest"
        case .message: target = "message"
        case nil: target = "none"
        }
        sessionScrollLogger.notice("\(reason, privacy: .public) target=\(target, privacy: .public) userPositioned=\(position.isPositionedByUser) follows=\(followsLatest) loading=\(loadingOlder) fallback=\(olderFallbackID != nil)")
#endif
    }
    mutating func userScrolled(nearBottom: Bool) {
        trace("userScrolled before nearBottom=\(nearBottom)")
        defer { trace("userScrolled after") }
        followsLatest = nearBottom
        olderFallbackID = nil
    }
    @discardableResult mutating func beginOlder(transcriptIDs: [String] = [], firstMessageBelowTop: Bool = false) -> UUID {
        trace("beginOlder before count=\(transcriptIDs.count) firstBelowTop=\(firstMessageBelowTop)")
        defer { trace("beginOlder after") }
        let request = UUID()
        olderRequest = request
        loadingOlder = true
        followsLatest = false
        let visibleID = visibleMessageID
        olderFallbackID = firstMessageBelowTop || visibleID.map(transcriptIDs.contains) != true ? transcriptIDs.first : nil
        return request
    }
    mutating func endOlder(request: UUID, didPrepend: Bool = false) {
        trace("endOlder before sameRequest=\(olderRequest == request) didPrepend=\(didPrepend)")
        defer { trace("endOlder after") }
        guard olderRequest == request else { return }
        loadingOlder = false
        olderRequest = nil
        if didPrepend, !followsLatest, let olderFallbackID {
            position.scrollTo(id: SessionScrollTarget.message(olderFallbackID), anchor: .top)
        }
        olderFallbackID = nil
    }
    mutating func jumpToLatest() {
        trace("jump before")
        defer { trace("jump after") }
        followsLatest = true
        lastTailCorrection = nil
        olderFallbackID = nil
        position.scrollTo(id: SessionScrollTarget.latest, anchor: .bottom)
    }
    mutating func followContent() {
        trace("followContent before")
        defer { trace("followContent after") }
        if shouldFollowContent { position.scrollTo(id: SessionScrollTarget.latest, anchor: .bottom) }
    }
    @discardableResult mutating func revealClippedTail(overflowPixels: Int, contentHeight: CGFloat, userScrolling: Bool) -> Bool {
        if overflowPixels == 0 { lastTailCorrection = nil }
        guard overflowPixels > 0, !userScrolling, shouldFollowContent else { return false }
        let measurement = SessionTailMeasurement(overflowPixels: overflowPixels, contentHeight: contentHeight)
        guard measurement != lastTailCorrection else { return false }
        trace("tailCorrection overflow=\(overflowPixels) height=\(contentHeight)")
        lastTailCorrection = measurement
        followContent()
        return true
    }
    var visibleMessageID: String? {
        guard case .message(let id) = position.viewID(type: SessionScrollTarget.self) else { return nil }
        return id
    }
    var shouldFollowContent: Bool { followsLatest && !loadingOlder }
}

struct SessionDetailView: View {
    @Environment(ControllerStore.self) private var store
    @Environment(\.displayScale) private var displayScale
    let work: WorkRef
    #if DEBUG
    @State private var diagnosticViewID = UUID()
    #endif
    @State private var action: WorkAction = .send
    @State private var confirmStop = false
    @State private var scrolling = SessionScrollState()
    @State private var userScrolling = false
    @State private var nearBottom = true
    @State private var firstMessageBelowTop = false
    @State private var tailOverflowPixels = 0
    @State private var tailContentHeight: CGFloat = 0
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
        .onDisappear { traceView("view disappear"); store.endSelection(work) }
        .onChange(of: work) { _, _ in traceView("work reset before"); scrolling = SessionScrollState(); firstMessageBelowTop = false; tailOverflowPixels = 0; tailContentHeight = 0; traceView("work reset after") }
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
    private func traceView(_ reason: String) {
#if DEBUG
        let items = store.selectedWork?.transcript ?? []
        let index = scrolling.visibleMessageID.flatMap { id in items.firstIndex(where: { $0.id == id }) } ?? -1
        sessionScrollLogger.notice("\(reason, privacy: .public) view=\(diagnosticViewID.uuidString, privacy: .public) selectedNil=\(store.selectedWork == nil) selectedMatches=\(store.selectedWork?.entry.work == work) refMatches=\(store.selectedRef == work) count=\(items.count) index=\(index) userScrolling=\(userScrolling)")
        scrolling.trace(reason)
#endif
    }
    private var tracedPosition: Binding<ScrollPosition> {
#if DEBUG
        Binding(get: { scrolling.position }, set: { value, transaction in
            traceView("binding before")
            $scrolling.position.transaction(transaction).wrappedValue = value
            traceView("binding after")
        })
#else
        $scrolling.position
#endif
    }
    private func content(_ selected: SelectedWork) -> some View {
        VStack(spacing: 0) {
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
                            let request = scrolling.beginOlder(transcriptIDs: selected.transcript.map(\.id), firstMessageBelowTop: firstMessageBelowTop)
                            let firstMessageID = selected.transcript.first?.id
                            let historyReset = store.selectedHistoryReset
                            Task {
                                await store.loadMoreHistory()
                                let current = store.selectedWork
                                let didPrepend = !Task.isCancelled && store.selectedRef == work
                                    && store.selectedHistoryReset == historyReset && current?.entry.work == work
                                    && firstMessageID != nil && current?.transcript.first?.id != firstMessageID
                                    && current?.transcript.contains(where: { $0.id == firstMessageID }) == true
                                scrolling.endOlder(request: request, didPrepend: didPrepend)
                            }
                        }.buttonStyle(StudioSecondaryButtonStyle()).disabled(store.selectedLoading)
                            .accessibilityIdentifier("load-older-history")
                    }
                    ForEach(selected.pendingInputs) { input in
                        PendingInputView(input: input, supported: selected.entry.canPerform(.answerInput))
                    }
                    if selected.transcript.isEmpty {
                        Notice(title: "No visible messages", message: selected.hasMoreHistory ? "There are no visible messages in this page. Load older history to continue." : "The owner returned no visible messages for this session.", icon: work.kind.icon)
                    }
                    ForEach(selected.transcript, id: \.sessionScrollTarget) { item in
                        StudioPanel {
                            HStack {
                                Text(item.role.capitalized).font(.caption.weight(.bold)).foregroundStyle(Studio.secondaryInk)
                                Spacer()
                                if let timestamp = item.timestamp { Text(timestamp, style: .time).font(.caption).foregroundStyle(Studio.secondaryInk) }
                            }
                            Text(item.text).font(work.kind == .shellJob ? .system(.body, design: .monospaced) : .body).textSelection(.enabled)
                        }
                            .onGeometryChange(for: Bool?.self) { geometry in
                                guard item.id == selected.transcript.first?.id else { return nil }
                                return geometry.frame(in: .scrollView(axis: .vertical)).minY >= 0
                            } action: { _, belowTop in
                                if let belowTop {
                                    firstMessageBelowTop = belowTop
                                }
                            }
                    }
                    if let pending = store.pendingCommands.last(where: { $0.command.work == work }) {
                        StudioPanel {
                            Label(pending.receipt?.state.title ?? "Checking delivery", icon: .send).font(.headline)
                            Text(pending.receipt?.reason ?? "Acceptance is separate from delivery. The owner’s receipt determines the result.").font(.subheadline).foregroundStyle(Studio.secondaryInk)
                            if store.unfinishedCommand != nil { Button("Check delivery") { Task { await store.checkDelivery() } }.disabled(store.sending) }
                        }
                    }
                    Color.clear.frame(height: 1).id(SessionScrollTarget.latest).accessibilityHidden(true)
                        .onGeometryChange(for: Int.self) { geometry in
                            guard let bounds = geometry.bounds(of: .scrollView(axis: .vertical)) else { return 0 }
                            let overflow = geometry.frame(in: .scrollView(axis: .vertical)).maxY - bounds.height
                            return max(0, Int((overflow * displayScale).rounded(.up)))
                        } action: { _, overflow in
                            tailOverflowPixels = overflow
                            scrolling.revealClippedTail(overflowPixels: overflow, contentHeight: tailContentHeight, userScrolling: userScrolling)
                        }
                }.scrollTargetLayout().padding(16).frame(maxWidth: 900)
            }
            .scrollPosition(tracedPosition, anchor: .top)
            .onAppear { traceView("scroll appear") }
            .onDisappear { traceView("scroll disappear") }
            .defaultScrollAnchor(.bottom, for: .initialOffset)
            .defaultScrollAnchor(.top, for: .sizeChanges)
            .onScrollGeometryChange(for: Bool.self) { geometry in
                geometry.contentSize.height - geometry.visibleRect.maxY <= 80
            } action: { _, isNearBottom in
                nearBottom = isNearBottom
                if userScrolling { scrolling.userScrolled(nearBottom: isNearBottom) }
            }
            .onScrollGeometryChange(for: CGFloat.self) { $0.contentSize.height } action: { _, height in
                tailContentHeight = height
                scrolling.revealClippedTail(overflowPixels: tailOverflowPixels, contentHeight: tailContentHeight, userScrolling: userScrolling)
            }
            .onScrollPhaseChange { _, phase in
                traceView("phase \(String(describing: phase))")
                userScrolling = phase == .tracking || phase == .interacting || phase == .decelerating
                if userScrolling { scrolling.userScrolled(nearBottom: nearBottom) }
                else if phase == .idle { scrolling.revealClippedTail(overflowPixels: tailOverflowPixels, contentHeight: tailContentHeight, userScrolling: false) }
            }
            .task(id: selected.transcript) {
                traceView("content task before yield count=\(selected.transcript.count)")
                await Task.yield()
                traceView("content task resumed cancelled=\(Task.isCancelled)")
                if !Task.isCancelled, !userScrolling { scrolling.followContent() }
            }
            .onChange(of: store.selectedHistoryReset) { _, _ in
                traceView("history reset")
                scrolling.jumpToLatest()
            }
            .overlay(alignment: .bottomTrailing) {
                if !scrolling.followsLatest {
                    Button {
                        traceView("jump tapped paused=\(store.selectedHistoryPaused)")
                        Task {
                            if store.selectedHistoryPaused {
                                await store.select(work)
                                guard store.selectedError == nil, store.selectedRef == work else { return }
                            }
                            scrolling.jumpToLatest()
                        }
                    } label: { Label("Jump to latest", systemImage: "arrow.down") }
                    .buttonStyle(StudioPrimaryButtonStyle()).padding(16)
                    .accessibilityIdentifier("jump-to-latest")
                    .accessibilityHint("Scrolls to the newest message and follows live updates")
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
