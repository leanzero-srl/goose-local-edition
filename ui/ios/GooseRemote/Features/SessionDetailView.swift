import SwiftUI
import UIKit

enum SessionScrollTarget: Hashable {
    case message(String)
    case latest
}

struct SessionScrollState {
    var position = ScrollPosition(id: SessionScrollTarget.latest, anchor: .bottom)
    private(set) var followsLatest = true
    private(set) var loadingOlder = false
    private var olderRequest: UUID?
    private var olderFallbackID: String?
    private(set) var requestGeneration: UInt64 = 0
    mutating func userScrolled(nearBottom: Bool) {
        followsLatest = nearBottom
        olderFallbackID = nil
    }
    @discardableResult mutating func beginOlder(transcriptIDs: [String] = [], firstMessageBelowTop: Bool = false) -> UUID {
        let request = UUID()
        olderRequest = request
        loadingOlder = true
        followsLatest = false
        let visibleID = visibleMessageID
        olderFallbackID = firstMessageBelowTop || visibleID.map(transcriptIDs.contains) != true ? transcriptIDs.first : nil
        return request
    }
    mutating func endOlder(request: UUID, didPrepend: Bool = false) {
        guard olderRequest == request else { return }
        loadingOlder = false
        olderRequest = nil
        if didPrepend, !followsLatest, let olderFallbackID {
            position.scrollTo(id: SessionScrollTarget.message(olderFallbackID), anchor: .top)
            requestGeneration &+= 1
        }
        olderFallbackID = nil
    }
    mutating func jumpToLatest() {
        followsLatest = true
        olderFallbackID = nil
        position.scrollTo(id: SessionScrollTarget.latest, anchor: .bottom)
        requestGeneration &+= 1
    }
    mutating func followContent() {
        if shouldFollowContent { position.scrollTo(id: SessionScrollTarget.latest, anchor: .bottom); requestGeneration &+= 1 }
    }
    var visibleMessageID: String? {
        guard case .message(let id) = position.viewID(type: SessionScrollTarget.self) else { return nil }
        return id
    }
    var shouldFollowContent: Bool { followsLatest && !loadingOlder }
}

struct SessionDetailView: View {
    @Environment(ControllerStore.self) private var store
    let work: WorkRef
    @State private var action: WorkAction = .send
    @State private var confirmStop = false
    @State private var scrolling = SessionScrollState()
    @State private var readingAnchor: NativeTranscriptAnchor?
    @State private var firstMessageBelowTop = false
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
        .onChange(of: work) { _, _ in scrolling = SessionScrollState(); firstMessageBelowTop = false; readingAnchor = nil }
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
            NativeSessionTranscript(rows: nativeRows(selected), request: nativeRequest,
                followsLatest: scrolling.shouldFollowContent, readingAnchor: readingAnchor) { viewport in
                    guard store.selectedRef == work else { return }
                    readingAnchor = viewport.anchor
                    firstMessageBelowTop = viewport.firstMessageBelowTop
                    if viewport.userInitiated { scrolling.userScrolled(nearBottom: viewport.nearBottom) }
                    if !scrolling.followsLatest, case .message(let id) = viewport.anchor?.id {
                        scrolling.position.scrollTo(id: SessionScrollTarget.message(id), anchor: .top)
                    }
                }
                .id(work)
            .onChange(of: store.selectedHistoryReset) { _, _ in
                scrolling.jumpToLatest()
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
    private var nativeRequest: NativeTranscriptRequest {
        let target: NativeTranscriptID
        if case .message(let id) = scrolling.position.viewID(type: SessionScrollTarget.self) { target = .message(id) }
        else { target = .latest }
        return .init(generation: scrolling.requestGeneration, target: target)
    }
    private func nativeRows(_ selected: SelectedWork) -> [NativeTranscriptRow] {
        let headerValues = [String(selected.entry.revision), selected.entry.title, store.ownerName(work.ownerNodeId),
            selected.entry.workingDir ?? "", String(describing: selected.entry.updatedAt), selected.entry.state.title,
            String(describing: selected.entry.capabilities), store.selectedError ?? "", store.selectedHistoryNotice ?? "",
            String(store.selectedHistoryPaused), String(store.selectedLoading), String(store.sending),
            String(store.unfinishedCommand != nil), String(store.connection == .connected),
            String(selected.hasMoreHistory), String(selected.transcript.isEmpty)]
        var rows = [NativeTranscriptRow(id: .header, version: .init(values: headerValues, inputs: selected.pendingInputs),
            content: AnyView(header(selected).environment(store).foregroundStyle(Studio.ink)))]
        rows += selected.transcript.map { item in
            NativeTranscriptRow(id: .message(item.id), version: .init(message: item),
                content: AnyView(messagePanel(item).environment(store).foregroundStyle(Studio.ink)))
        }
        let pending = store.pendingCommands.last { $0.command.work == work }
        rows.append(.init(id: .latest, version: .init(values: [pending?.id.uuidString ?? "", pending?.receipt?.state.title ?? "",
            pending?.receipt?.reason ?? "", String(store.unfinishedCommand != nil), String(store.sending)]),
            content: AnyView(latestPanel.environment(store).foregroundStyle(Studio.ink))))
        return rows
    }
    private func header(_ selected: SelectedWork) -> some View {
        VStack(alignment: .leading, spacing: 16) {
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
        }
    }
    private func messagePanel(_ item: TranscriptItem) -> some View {
    StudioPanel {
        HStack {
            Text(item.role.capitalized).font(.caption.weight(.bold)).foregroundStyle(Studio.secondaryInk)
            Spacer()
            if let timestamp = item.timestamp { Text(timestamp, style: .time).font(.caption).foregroundStyle(Studio.secondaryInk) }
        }
        Text(item.text).font(work.kind == .shellJob ? .system(.body, design: .monospaced) : .body).textSelection(.enabled)
    }
    }
    private var latestPanel: some View {
        VStack(alignment: .leading, spacing: 16) {
        if let pending = store.pendingCommands.last(where: { $0.command.work == work }) {
            StudioPanel {
                Label(pending.receipt?.state.title ?? "Checking delivery", icon: .send).font(.headline)
                Text(pending.receipt?.reason ?? "Acceptance is separate from delivery. The owner’s receipt determines the result.").font(.subheadline).foregroundStyle(Studio.secondaryInk)
                if store.unfinishedCommand != nil { Button("Check delivery") { Task { await store.checkDelivery() } }.disabled(store.sending) }
            }
        }
            Color.clear.frame(height: 1).accessibilityHidden(true)
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

enum NativeTranscriptID: Hashable {
    case header
    case message(String)
    case latest
}
struct NativeTranscriptAnchor: Equatable {
    let id: NativeTranscriptID
    var offset: CGFloat
}
struct NativeTranscriptRequest: Equatable {
    let generation: UInt64
    let target: NativeTranscriptID
}
struct NativeTranscriptRowVersion: Equatable {
    var values: [String] = []
    var inputs: [PendingInput] = []
    var message: TranscriptItem? = nil
}
struct NativeTranscriptRow {
    let id: NativeTranscriptID
    let version: NativeTranscriptRowVersion
    let content: AnyView
}
struct NativeTranscriptViewport: Equatable {
    let anchor: NativeTranscriptAnchor?
    let nearBottom: Bool
    let firstMessageBelowTop: Bool
    let interacting: Bool
    let userInitiated: Bool
}

struct NativeSessionTranscript: UIViewControllerRepresentable {
    let rows: [NativeTranscriptRow]
    let request: NativeTranscriptRequest
    let followsLatest: Bool
    let readingAnchor: NativeTranscriptAnchor?
    let viewportChanged: (NativeTranscriptViewport) -> Void

    func makeUIViewController(context: Context) -> NativeTranscriptController { NativeTranscriptController() }
    func updateUIViewController(_ controller: NativeTranscriptController, context: Context) {
        controller.viewportChanged = viewportChanged
        controller.update(rows: rows, request: request, followsLatest: followsLatest, savedAnchor: readingAnchor)
    }
}

final class NativeTranscriptCell: UICollectionViewCell {
    override func preferredLayoutAttributesFitting(_ layoutAttributes: UICollectionViewLayoutAttributes) -> UICollectionViewLayoutAttributes {
        let fitted = super.preferredLayoutAttributesFitting(layoutAttributes)
        fitted.size.width = layoutAttributes.size.width
        let size = contentView.systemLayoutSizeFitting(CGSize(width: layoutAttributes.size.width, height: 0),
            withHorizontalFittingPriority: .required, verticalFittingPriority: .fittingSizeLevel)
        fitted.size.height = max(1, ceil(size.height))
        return fitted
    }
}

final class NativeTranscriptLayout: UICollectionViewFlowLayout {
    weak var transcript: NativeTranscriptController?
    private var measuredWidth: CGFloat = 0

    private func updateWidth(_ width: CGFloat) {
        guard width > 0, width != measuredWidth else { return }
        measuredWidth = width
        let scale = collectionView?.window?.screen.scale ?? collectionView?.traitCollection.displayScale ?? 2
        let rowWidth = max(1, floor((min(900, width) - 32) * scale) / scale)
        let margin = max(16, floor((width - rowWidth) * scale / 2) / scale)
        sectionInset = UIEdgeInsets(top: 16, left: margin, bottom: 16, right: margin)
        estimatedItemSize = CGSize(width: rowWidth, height: 120)
        itemSize = estimatedItemSize
        transcript?.widthWillChange()
    }
    override func shouldInvalidateLayout(forBoundsChange newBounds: CGRect) -> Bool {
        let widthChanged = newBounds.width != measuredWidth
        // Flow layout validates cached row widths during the bounds change, before the controller's layout callback.
        updateWidth(newBounds.width)
        return widthChanged || super.shouldInvalidateLayout(forBoundsChange: newBounds)
    }
    override func invalidationContext(forBoundsChange newBounds: CGRect) -> UICollectionViewLayoutInvalidationContext {
        let context = super.invalidationContext(forBoundsChange: newBounds)
        if newBounds.width != collectionView?.bounds.width, let flow = context as? UICollectionViewFlowLayoutInvalidationContext {
            flow.invalidateFlowLayoutDelegateMetrics = true
            flow.invalidateFlowLayoutAttributes = true
        }
        return context
    }
    override func prepare() {
        updateWidth(collectionView?.bounds.width ?? 0)
        super.prepare()
    }
    override func invalidationContext(forPreferredLayoutAttributes preferredAttributes: UICollectionViewLayoutAttributes,
        withOriginalAttributes originalAttributes: UICollectionViewLayoutAttributes) -> UICollectionViewLayoutInvalidationContext {
        let context = super.invalidationContext(forPreferredLayoutAttributes: preferredAttributes, withOriginalAttributes: originalAttributes)
        let delta = preferredAttributes.size.height - originalAttributes.size.height
        if let required = transcript?.offsetAdjustment(for: originalAttributes.indexPath, heightDelta: delta) {
            // Replace the vertical residual, rather than adding a delta UIKit may already supply.
            context.contentOffsetAdjustment.y += required - context.contentOffsetAdjustment.y
        }
        return context
    }
}

final class NativeTranscriptController: UIViewController, UICollectionViewDelegate {
    private let layout = NativeTranscriptLayout()
    private(set) var collectionView: UICollectionView!
    private var dataSource: UICollectionViewDiffableDataSource<Int, NativeTranscriptID>!
    private var rows: [NativeTranscriptRow] = []
    private var rowsByID: [NativeTranscriptID: NativeTranscriptRow] = [:]
    private var request: NativeTranscriptRequest?
    private var followsLatest = true
    private var anchor: NativeTranscriptAnchor?
    private var pendingAlignment = false
    private var needsRealization = false
    private var snapshotGeneration: UInt64 = 0
    private var interactionGeneration: UInt64 = 0
    private var reconciling = false
    private var gestureActive = false
    private var lastLayout: [CGFloat] = []
    private var lastViewport: NativeTranscriptViewport?
    var viewportChanged: ((NativeTranscriptViewport) -> Void)?
    var interacting: Bool { gestureActive || collectionView?.isTracking == true || collectionView?.isDragging == true || collectionView?.isDecelerating == true }

    override func viewDidLoad() {
        super.viewDidLoad()
        layout.transcript = self
        layout.minimumLineSpacing = 16
        layout.minimumInteritemSpacing = 0
        layout.sectionInset = UIEdgeInsets(top: 16, left: 16, bottom: 16, right: 16)
        layout.estimatedItemSize = CGSize(width: 300, height: 120)
        collectionView = UICollectionView(frame: .zero, collectionViewLayout: layout)
        collectionView.backgroundColor = .clear
        collectionView.contentInsetAdjustmentBehavior = .never
        collectionView.alwaysBounceVertical = true
        collectionView.keyboardDismissMode = .interactive
        collectionView.delegate = self
        collectionView.register(NativeTranscriptCell.self, forCellWithReuseIdentifier: "transcript")
        view = collectionView
        dataSource = UICollectionViewDiffableDataSource<Int, NativeTranscriptID>(collectionView: collectionView) { [weak self] collection, index, id in
            let cell = collection.dequeueReusableCell(withReuseIdentifier: "transcript", for: index)
            guard let row = self?.rowsByID[id] else { return cell }
            cell.contentConfiguration = UIHostingConfiguration { row.content }.margins(.all, 0)
            cell.backgroundColor = .clear
            return cell
        }
    }

    func update(rows newRows: [NativeTranscriptRow], request newRequest: NativeTranscriptRequest, followsLatest: Bool,
        savedAnchor: NativeTranscriptAnchor?) {
        loadViewIfNeeded()
        let firstUpdate = rows.isEmpty
        let newIntent = request?.generation != newRequest.generation
        if !self.followsLatest, !interacting, !pendingAlignment, !needsRealization { anchor = currentAnchor() ?? anchor ?? savedAnchor }
        self.followsLatest = followsLatest
        if firstUpdate { anchor = savedAnchor }
        if newIntent {
            request = newRequest
            if !firstUpdate, newRequest.target == .latest { self.followsLatest = true }
            else { anchor = .init(id: newRequest.target, offset: 0) }
            pendingAlignment = true
            needsRealization = true
            lastLayout = []
        }
        let oldIDs = rows.map(\.id), newIDs = newRows.map(\.id)
        let changed = newRows.filter { rowsByID[$0.id]?.version != $0.version }.map(\.id)
        rows = newRows
        rowsByID = Dictionary(uniqueKeysWithValues: newRows.map { ($0.id, $0) })
        if firstUpdate && !self.followsLatest, let savedAnchor {
            anchor = savedAnchor
            pendingAlignment = true
            needsRealization = true
        }
        guard oldIDs != newIDs || !changed.isEmpty else {
            if newIntent { realizeAndAlign() }
            return
        }
        snapshotGeneration &+= 1
        let snapshot = snapshotGeneration, gesture = interactionGeneration
        if !interacting { pendingAlignment = true; needsRealization = true }
        var update = NSDiffableDataSourceSnapshot<Int, NativeTranscriptID>()
        update.appendSections([0]); update.appendItems(newIDs)
        let retained = Set(oldIDs)
        update.reconfigureItems(changed.filter(retained.contains))
        dataSource.apply(update, animatingDifferences: false) { [weak self] in
            guard let self, snapshot == self.snapshotGeneration, gesture == self.interactionGeneration else { return }
            self.realizeAndAlign()
        }
    }

    func widthWillChange() {
        pendingAlignment = !interacting
        needsRealization = !interacting
        lastLayout = []
    }
    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        if needsRealization { realizeAndAlign() } else { reconcileLayout() }
        publishViewport()
    }

    private var alignmentID: NativeTranscriptID? { followsLatest ? .latest : anchor?.id }
    private func realizeAndAlign() {
        guard collectionView.bounds.width > 0, collectionView.bounds.height > 0, !interacting, !reconciling, let id = alignmentID, let index = dataSource.indexPath(for: id),
            collectionView.numberOfItems(inSection: index.section) > index.item else { return }
        if needsRealization {
            needsRealization = false
            collectionView.scrollToItem(at: index, at: followsLatest ? .bottom : .top, animated: false)
        }
        collectionView.layoutIfNeeded()
        reconcileLayout()
        publishViewport()
    }
    private func reconcileLayout() {
        guard !interacting, !reconciling, pendingAlignment || followsLatest,
            let id = alignmentID, let index = dataSource.indexPath(for: id),
            collectionView.cellForItem(at: index) != nil,
            let attributes = layout.layoutAttributesForItem(at: index) else { return }
        let frame = attributes.frame
        let signature = [frame.minY, frame.height, collectionView.bounds.width, collectionView.bounds.height, collectionView.contentSize.height]
        guard pendingAlignment || signature != lastLayout else { return }
        lastLayout = signature
        let desired: CGFloat
        if followsLatest {
            desired = frame.maxY + layout.sectionInset.bottom - collectionView.bounds.height
        } else {
            let offset = max(-(max(0, frame.height - 1)), anchor?.offset ?? 0)
            desired = frame.minY - offset
            anchor?.offset = offset
        }
        let maximum = max(0, collectionView.contentSize.height - collectionView.bounds.height)
        let clamped = max(0, min(maximum, desired))
        pendingAlignment = false
        let pixel = 1 / (view.window?.screen.scale ?? 2)
        guard abs(collectionView.contentOffset.y - clamped) > pixel else { return }
        reconciling = true
        collectionView.setContentOffset(CGPoint(x: 0, y: clamped), animated: false)
        reconciling = false
    }

    func offsetAdjustment(for index: IndexPath, heightDelta: CGFloat) -> CGFloat? {
        guard !interacting, abs(heightDelta) > 0.01 else { return nil }
        if followsLatest { return heightDelta }
        guard let id = anchor?.id, let anchored = dataSource?.indexPath(for: id), index.item < anchored.item else { return nil }
        return heightDelta
    }
    func frame(for id: NativeTranscriptID) -> CGRect? {
        guard let index = dataSource.indexPath(for: id), collectionView.cellForItem(at: index) != nil else { return nil }
        return layout.layoutAttributesForItem(at: index)?.frame.offsetBy(dx: -collectionView.contentOffset.x, dy: -collectionView.contentOffset.y)
    }
    private func currentAnchor() -> NativeTranscriptAnchor? {
        let viewport = collectionView.bounds
        let visible = collectionView.indexPathsForVisibleItems.compactMap { index -> (NativeTranscriptID, CGRect)? in
            guard let id = dataSource.itemIdentifier(for: index), let frame = layout.layoutAttributesForItem(at: index)?.frame,
                frame.intersects(viewport) else { return nil }
            return (id, frame)
        }.sorted { $0.1.minY < $1.1.minY }
        guard let first = visible.first else { return nil }
        return .init(id: first.0, offset: first.1.minY - viewport.minY)
    }
    private func publishViewport(userInitiated: Bool = false) {
        guard isViewLoaded, view.window != nil else { return }
        let current = currentAnchor()
        if interacting || (!followsLatest && !pendingAlignment && !needsRealization) { anchor = current ?? anchor }
        let tail = frame(for: .latest)
        let nearBottom = tail.map { $0.maxY <= collectionView.bounds.height + 80 } ?? false
        let firstMessage = rows.first { if case .message = $0.id { true } else { false } }
        let firstBelow = current?.id == .header || (firstMessage.flatMap { frame(for: $0.id) }.map { $0.minY >= 0 } ?? false)
        let viewport = NativeTranscriptViewport(anchor: current, nearBottom: nearBottom, firstMessageBelowTop: firstBelow, interacting: interacting, userInitiated: userInitiated)
        guard viewport != lastViewport else { return }
        lastViewport = viewport
        let generation = interactionGeneration, snapshot = snapshotGeneration, intent = request?.generation
        DispatchQueue.main.async { [weak self] in
            guard let self, generation == self.interactionGeneration, snapshot == self.snapshotGeneration, intent == self.request?.generation, self.view.window != nil else { return }
            self.viewportChanged?(viewport)
        }
    }
    func scrollViewWillBeginDragging(_ scrollView: UIScrollView) {
        gestureActive = true
        interactionGeneration &+= 1
        followsLatest = false
        pendingAlignment = false
        needsRealization = false
        anchor = currentAnchor()
        publishViewport(userInitiated: true)
    }
    func scrollViewDidScroll(_ scrollView: UIScrollView) {
        if interacting { publishViewport(userInitiated: true) }
    }
    func scrollViewDidEndDragging(_ scrollView: UIScrollView, willDecelerate decelerate: Bool) {
        if !decelerate { finishGesture() }
    }
    func scrollViewDidEndDecelerating(_ scrollView: UIScrollView) { finishGesture() }
    private func finishGesture() {
        gestureActive = false
        anchor = currentAnchor()
        followsLatest = frame(for: .latest).map { $0.maxY <= collectionView.bounds.height + 80 } ?? false
        publishViewport(userInitiated: true)
    }
}
