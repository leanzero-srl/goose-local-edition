import SwiftUI

struct ComputeNodesSection: View {
    let model: ManagementModel
    let host: HostDescriptor
    @State private var removing: ManagedComputeNode?

    var body: some View {
        if let inventory = model.nodes {
            Section("Compute nodes on this Mac") {
                Text("Named ways to run a model. Saving a node does not start inference or change your session defaults.")
                    .font(.subheadline).foregroundStyle(Studio.secondaryInk)
                if inventory.nodes.isEmpty { Text("No compute nodes defined.").foregroundStyle(Studio.secondaryInk) }
                ForEach(inventory.nodes) { node in
                    VStack(alignment: .leading, spacing: 6) {
                        Text(node.name).font(.headline)
                        Text(node.model ?? "Model unavailable").textSelection(.enabled)
                        Text(nodeSummary(node)).font(.caption).foregroundStyle(Studio.secondaryInk)
                        if !node.editable {
                            Text("Managed by the Mac's inference pool").font(.caption).foregroundStyle(Studio.secondaryInk)
                        } else if inventory.capabilities.contains("node_remove") {
                            Button("Remove \(node.name)", role: .destructive) { removing = node }
                                .disabled(model.busy || model.pending != nil)
                        }
                    }.padding(.vertical, 4)
                }
                if inventory.capabilities.contains("node_create") {
                    NavigationLink("Add compute node") { NewComputeNodeView(model: model, host: host) }
                        .disabled(model.busy || model.pending != nil)
                } else {
                    Text("This Mac does not support creating compute nodes remotely.").foregroundStyle(Studio.secondaryInk)
                }
            }.listRowBackground(Studio.surface)
                .confirmationDialog("Remove this compute node?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), presenting: removing) { node in
                    Button("Remove \(node.name)", role: .destructive) {
                        Task {
                            await model.perform(.nodeRemove, target: node.name, payload: .object([
                                "id": .string(node.id), "expected_definition_revision": .string(node.definitionRevision)
                            ]))
                        }
                    }
                } message: { node in
                    Text("Removes only this definition from \(host.name). The Mac will refuse if it changed or is still used by sessions, defaults or strategies. Models and providers remain installed.")
                }
        }
    }
    private func nodeSummary(_ node: ManagedComputeNode) -> String {
        let kind = ComputeNodeDraft.kindTitle(node.kind)
        if let provider = node.provider { return "\(kind) · \(provider)" }
        return node.macs.isEmpty ? kind : "\(kind) · \(node.macs.map { $0 == "local" ? host.name : $0 }.joined(separator: ", "))"
    }
}

struct ComputeNodeDraft {
    var id = "remote-" + UUID().uuidString.lowercased()
    var name = ""
    var kind = "cloud"
    var provider = ""
    var model = ""
    static func kindTitle(_ kind: String) -> String {
        switch kind {
        case "cloud": "Cloud model"
        case "endpoint": "Configured endpoint"
        case "mlx": "On this Mac (MLX)"
        default: kind
        }
    }
    func payload(in inventory: ManagedComputeNodes) throws -> JSONValue {
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedName.isEmpty, !model.isEmpty,
              inventory.capabilities.contains("node_create"), ["cloud", "endpoint", "mlx"].contains(kind), inventory.supportedKinds.contains(kind) else {
            throw ControllerError.invalid("Choose a supported node type, model and name.")
        }
        guard !inventory.nodes.contains(where: { $0.name == trimmedName || $0.id == id }) else {
            throw ControllerError.invalid("A node already has this name. Choose another name.")
        }
        var fields: [String: JSONValue] = ["id": .string(id), "name": .string(trimmedName), "kind": .string(kind), "model": .string(model)]
        if kind == "mlx" {
            guard inventory.localModels.contains(model) else { throw ControllerError.invalid("Choose a model installed on this Mac.") }
        } else {
            guard inventory.providers.contains(where: { $0.id == provider && $0.models.contains(model) }) else {
                throw ControllerError.invalid("Choose a provider and one of its available models.")
            }
            fields["provider"] = .string(provider)
        }
        return .object(fields)
    }
}

struct NewComputeNodeView: View {
    @Environment(\.dismiss) private var dismiss
    let model: ManagementModel
    let host: HostDescriptor
    @State private var draft = ComputeNodeDraft()
    @State private var validationError: String?
    @State private var submittedCommand: UUID?

    private var choices: [String] {
        guard let inventory = model.nodes else { return [] }
        return draft.kind == "mlx" ? inventory.localModels : inventory.providers.first(where: { $0.id == draft.provider })?.models ?? []
    }
    private var canSave: Bool {
        guard let inventory = model.nodes, !model.busy, model.pending == nil,
              !(model.receipt?.commandId == submittedCommand && model.receipt?.state == .delivered) else { return false }
        return (try? draft.payload(in: inventory)) != nil
    }
    var body: some View {
        Form {
            if let inventory = model.nodes {
                Section("Run on \(host.name)") {
                    Picker("Node type", selection: $draft.kind) {
                        ForEach(inventory.supportedKinds.filter { ["cloud", "endpoint", "mlx"].contains($0) }, id: \.self) { kind in
                            Text(ComputeNodeDraft.kindTitle(kind)).tag(kind)
                        }
                    }
                    if draft.kind != "mlx" {
                        Picker("Provider", selection: $draft.provider) {
                            Text("Choose provider").tag("")
                            ForEach(inventory.providers) { provider in Text(provider.displayName).tag(provider.id) }
                        }
                        if inventory.providers.isEmpty { Text("Set up a provider on this Mac first.").foregroundStyle(Studio.secondaryInk) }
                    }
                    Picker("Model", selection: $draft.model) {
                        Text("Choose model").tag("")
                        ForEach(choices, id: \.self) { Text($0).tag($0) }
                    }
                    if choices.isEmpty {
                        Text(draft.kind == "mlx" ? "Download a model to this Mac first." : "Choose a configured provider with available models.")
                            .font(.subheadline).foregroundStyle(Studio.secondaryInk)
                    }
                    TextField("Node name", text: $draft.name)
                    Text("Saving creates a definition only. It does not load the model or change existing sessions.")
                        .font(.subheadline).foregroundStyle(Studio.secondaryInk)
                    if !inventory.capabilities.contains("node_create") {
                        Text("This Mac no longer supports creating compute nodes remotely.").foregroundStyle(Studio.warningInk)
                    }
                }.listRowBackground(Studio.surface)
            } else {
                Section { Text("Refresh this Mac's node list before creating a node.") }.listRowBackground(Studio.surface)
            }
            Section {
                Button("Save on \(host.name)") {
                    do {
                        guard let inventory = model.nodes else { throw ControllerError.unavailable("Refresh the node list first.") }
                        let payload = try draft.payload(in: inventory)
                        let target = draft.name.trimmingCharacters(in: .whitespacesAndNewlines)
                        validationError = nil
                        Task {
                            await model.perform(.nodeCreate, target: target, payload: payload)
                            submittedCommand = model.receipt?.commandId ?? model.pending?.command.commandId
                            closeIfDelivered()
                        }
                    } catch { validationError = error.localizedDescription }
                }.disabled(!canSave)
                if let validationError { Text(validationError).foregroundStyle(Studio.dangerInk) }
                if let error = model.error { Text(error).foregroundStyle(Studio.dangerInk) }
            }.listRowBackground(Studio.surface)
            ManagementStatusView(model: model)
        }.navigationTitle("Add compute node").studioScreen()
            .onAppear {
                if let inventory = model.nodes, !inventory.supportedKinds.contains(draft.kind),
                   let first = inventory.supportedKinds.first(where: { ["cloud", "endpoint", "mlx"].contains($0) }) { draft.kind = first }
            }
            .onChange(of: draft.kind) { _, _ in draft.provider = ""; draft.model = "" }
            .onChange(of: draft.provider) { _, _ in draft.model = "" }
            .onChange(of: model.nodes?.providers.map(\.id)) { _, ids in
                if let ids, !ids.contains(draft.provider) { draft.provider = ""; draft.model = "" }
            }
            .onChange(of: choices) { _, next in if !next.contains(draft.model) { draft.model = "" } }
            .onChange(of: model.receipt?.state) { _, _ in closeIfDelivered() }
    }
    private func closeIfDelivered() {
        guard let submittedCommand, model.receipt?.commandId == submittedCommand,
              model.receipt?.action == .nodeCreate, model.receipt?.state == .delivered else { return }
        dismiss()
    }
}
