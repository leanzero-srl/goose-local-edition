import SwiftUI

struct ManagementView: View {
    @Environment(ControllerStore.self) private var store
    let kind: ManagementKind
    let host: HostDescriptor
    @State private var model: ManagementModel?
    @State private var setupError: String?
    @State private var repo = ""
    @State private var cancelDownload = false

    var body: some View {
        List {
            Section { Text("Changes apply to \(host.name).").font(.subheadline).foregroundStyle(Studio.secondaryInk) }.listRowBackground(Studio.surface)
            if let setupError { Section { Text(setupError).foregroundStyle(Studio.dangerInk) } }
            if let model {
                if let error = model.error { Section { Text(error).foregroundStyle(Studio.dangerInk).textSelection(.enabled) } }
                ManagementStatusView(model: model)
                if kind == .models { models(model) }
                else if kind == .providers {
                    Section("Providers") {
                        ForEach(model.providers) { provider in
                            NavigationLink { ProviderConfigurationView(provider: provider, model: model) } label: {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(provider.displayName).font(.headline)
                                    Text(provider.configured ? "Configured" : "Setup needed").foregroundStyle(Studio.secondaryInk)
                                }
                            }
                        }
                        NavigationLink("Add provider") { NewProviderView(model: model) }
                    }.listRowBackground(Studio.surface)
                }
            } else if setupError == nil { ProgressView("Connecting to \(host.name)…") }
        }.navigationTitle(kind.title).navigationBarTitleDisplayMode(.inline).studioScreen()
            .task {
                do {
                    if model == nil { model = try store.managementModel(for: host) }
                    await model?.load(providersOnly: kind == .providers)
                    while !Task.isCancelled {
                        try await Task.sleep(for: .seconds(2))
                        if let model, !model.trackedRepo.isEmpty { await model.refreshProgress() }
                    }
                } catch is CancellationError { }
                catch { setupError = error.localizedDescription }
            }
            .refreshable { await model?.load(providersOnly: kind == .providers); await model?.refreshProgress() }
            .confirmationDialog("Cancel this download on \(host.name)?", isPresented: $cancelDownload, titleVisibility: .visible) {
                Button("Cancel download", role: .destructive) {
                    if let model { Task { await downloadAction(.downloadCancel, model: model) } }
                }
            } message: { Text("The Mac will remove the incomplete download files.") }
    }
    @ViewBuilder private func models(_ model: ManagementModel) -> some View {
        Section("Download from Hugging Face") {
            TextField("organization/model", text: $repo).textInputAutocapitalization(.never).autocorrectionDisabled().accessibilityLabel("Hugging Face repository")
            Button("Download on \(host.name)") {
                Task { await model.perform(.download, target: repo.trimmingCharacters(in: .whitespacesAndNewlines), payload: .object(["repo_id": .string(repo.trimmingCharacters(in: .whitespacesAndNewlines))])); await model.refreshProgress() }
            }.disabled(repo.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.busy || model.pending != nil)
            Button("Check download") { Task { await model.watch(repo: repo) } }.disabled(repo.isEmpty || model.busy)
            if let models = model.models { LabeledContent("Disk available", value: bytes(models.diskAvailableBytes)) }
        }.listRowBackground(Studio.surface)
        if let progress = model.progress {
            Section("\(model.trackedRepo)") {
                LabeledContent("Status", value: progress.state.capitalized)
                if progress.totalBytes > 0 {
                    ProgressView(value: min(Double(progress.downloadedBytes), Double(progress.totalBytes)), total: Double(progress.totalBytes))
                    Text("\(bytes(progress.downloadedBytes)) of \(bytes(progress.totalBytes))").font(.caption)
                } else { Text("\(bytes(progress.downloadedBytes)) downloaded").font(.caption) }
                if let file = progress.currentFile { Text(file).font(.caption.monospaced()).textSelection(.enabled) }
                if let error = progress.error { Text(error).foregroundStyle(Studio.dangerInk).textSelection(.enabled) }
                RelativeSample(date: model.progressAt, prefix: "Updated")
                if progress.state == "paused" { Button("Resume") { Task { await downloadAction(.downloadResume, model: model) } }.disabled(model.busy || model.pending != nil) }
                if ["queued", "downloading"].contains(progress.state) { Button("Pause") { Task { await downloadAction(.downloadPause, model: model) } }.disabled(model.busy || model.pending != nil) }
                if ["queued", "downloading", "paused"].contains(progress.state) { Button("Cancel download", role: .destructive) { cancelDownload = true }.disabled(model.busy || model.pending != nil) }
            }.listRowBackground(Studio.surface)
        }
        if let models = model.models {
            Section("Models on this Mac") {
                if models.models.isEmpty { Text("No local models found.").foregroundStyle(Studio.secondaryInk) }
                ForEach(models.models) { item in
                    VStack(alignment: .leading, spacing: 4) {
                        Text(item.id).font(.headline).textSelection(.enabled)
                        Text("\(bytes(item.sizeBytes)) · \(item.complete ? "Ready" : "Incomplete: \(item.missingFiles) files")").font(.caption).foregroundStyle(Studio.secondaryInk)
                    }
                }
            }.listRowBackground(Studio.surface)
        }
    }
    private func downloadAction(_ action: ManagementAction, model: ManagementModel) async {
        await model.perform(action, target: model.trackedRepo, payload: .object(["repo_id": .string(model.trackedRepo)]))
        await model.refreshProgress()
    }
    private func bytes(_ value: UInt64) -> String { ByteCountFormatter.string(fromByteCount: Int64(clamping: value), countStyle: .file) }
}

struct ManagementStatusView: View {
    let model: ManagementModel
    @State private var dismissUnknown = false
    var body: some View {
        Group {
            if let pending = model.pending {
                Section("Request status") {
                    Text(pending.target).textSelection(.enabled)
                    Text(model.receipt?.state.title ?? "Awaiting acknowledgement")
                    if let reason = model.receipt?.reason { Text(reason).textSelection(.enabled) }
                    Button("Check delivery") { Task { await model.checkReceipt() } }.disabled(model.busy)
                    if model.receipt == nil { Button("Retry saved request") { Task { await model.retry() } }.disabled(model.busy) }
                    if model.canDismissRequest { Button("Clear saved request") { dismissUnknown = true }.disabled(model.busy) }
                }.listRowBackground(Studio.surface)
            } else if let receipt = model.receipt {
                Section("Latest request") {
                    Text(receipt.target).textSelection(.enabled)
                    Text(receipt.state == .delivered ? "Applied on the Mac" : receipt.state.title)
                    if let reason = receipt.reason { Text(reason).textSelection(.enabled) }
                }.listRowBackground(Studio.surface)
            }
        }.confirmationDialog("Clear this saved request?", isPresented: $dismissUnknown, titleVisibility: .visible) {
            Button("Clear saved request", role: .destructive) { Task { await model.dismissUnknownRequest() } }
        } message: { Text("The Mac may already have applied it. Check the Mac's models or provider settings first. Clearing only removes this phone's retry record; it does not undo work on the Mac.") }
    }
}

struct ProviderConfigurationView: View {
    let provider: ManagedProvider
    let model: ManagementModel
    @State private var values: [String: String] = [:]
    @State private var testModel = ""
    var body: some View {
        Form {
            Section {
                ForEach(provider.fields) { field in
                    VStack(alignment: .leading, spacing: 5) {
                        Text(field.key).font(.subheadline.weight(.semibold))
                        if field.secret { SecureField(field.isSet ? "Saved — enter to replace" : "Enter value", text: binding(field.key)) }
                        else { TextField(field.value ?? "Enter value", text: binding(field.key)).textInputAutocapitalization(.never).autocorrectionDisabled() }
                        if field.required && !field.isSet { Text("Required").font(.caption).foregroundStyle(Studio.warningInk) }
                    }
                }
                TextField("Model for connection test (optional)", text: $testModel).textInputAutocapitalization(.never).autocorrectionDisabled()
            }.listRowBackground(Studio.surface)
            Section {
                Button("Save on Mac") {
                    var payload: [String: JSONValue] = ["provider_id": .string(provider.id), "fields": .object(values.filter { !$0.value.isEmpty }.mapValues(JSONValue.string))]
                    if !testModel.isEmpty { payload["test_model"] = .string(testModel) }
                    Task { await model.perform(.providerConfigure, target: provider.displayName, payload: .object(payload)); if model.pending == nil && model.error == nil { values = [:] } }
                }.disabled(model.busy || model.pending != nil)
                if let error = model.error { Text(error).foregroundStyle(Studio.dangerInk) }

            }.listRowBackground(Studio.surface)
            ManagementStatusView(model: model)
        }.navigationTitle(provider.displayName).studioScreen()
    }
    private func binding(_ key: String) -> Binding<String> { .init(get: { values[key] ?? "" }, set: { values[key] = $0 }) }
}

struct NewProviderView: View {
    let model: ManagementModel
    @State private var engine = "anthropic"
    @State private var name = ""
    @State private var endpoint = ""
    @State private var key = ""
    @State private var models = ""
    @State private var requiresAuth = true
    var body: some View {
        Form {
            Section("Provider") {
                Picker("Compatible API", selection: $engine) { Text("Anthropic").tag("anthropic"); Text("OpenAI").tag("openai"); Text("Ollama").tag("ollama") }
                TextField("Provider name", text: $name)
                TextField("API URL", text: $endpoint).textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                Toggle("Requires an API key", isOn: $requiresAuth)
                if requiresAuth { SecureField("API key", text: $key) }
                TextField("Model IDs, separated by commas", text: $models, axis: .vertical).textInputAutocapitalization(.never).autocorrectionDisabled()
            }.listRowBackground(Studio.surface)
            Section {
                Button("Add on Mac") {
                    var payload: [String: JSONValue] = ["engine": .string(engine), "display_name": .string(name), "api_url": .string(endpoint), "models": .array(models.split(separator: ",").map { .string($0.trimmingCharacters(in: .whitespacesAndNewlines)) }), "requires_auth": .bool(requiresAuth)]
                    if requiresAuth { payload["api_key"] = .string(key) }
                    Task { await model.perform(.providerCreate, target: name, payload: .object(payload)); if model.pending == nil && model.error == nil { key = "" } }
                }.disabled(model.busy || model.pending != nil || name.isEmpty || endpoint.isEmpty || models.isEmpty || (requiresAuth && key.isEmpty))
                if let error = model.error { Text(error).foregroundStyle(Studio.dangerInk) }

            }.listRowBackground(Studio.surface)
            ManagementStatusView(model: model)
        }.navigationTitle("Add provider").studioScreen()
    }
}

struct AddMacView: View {
    @Environment(ControllerStore.self) private var store
    var body: some View {
        List {
            Section("Connect another Mac") {
                Text("1. Install Goose Swarm on the Mac.")
                Text("2. Open Link and sign in with the same account as this controller.")
                Text("3. Connect the Mac to Link and finish its controller-access setup.")
                Text("4. Sync here. The Mac appears after it has registered its secure connection.")
                Button("Sync registered Macs") { store.manualSync() }.disabled(store.syncRunning)
            }.listRowBackground(Studio.surface)
            Section("Connected Macs") { ForEach(store.hosts) { host in Label(host.name, icon: .laptop) } }.listRowBackground(Studio.surface)
            Section { Text("You can add more phones and tablets as controllers. They never join the inference pool.").foregroundStyle(Studio.secondaryInk) }.listRowBackground(Studio.surface)
        }.navigationTitle("Add a Mac").studioScreen()
    }
}
