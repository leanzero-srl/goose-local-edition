import SwiftUI

struct PendingInputView: View {
    @Environment(ControllerStore.self) private var store
    let input: PendingInput
    let supported: Bool
    @State private var answer = ""
    @State private var drafts: [String: String] = [:]

    private var busy: Bool { store.selectedError != nil || store.sending || store.unfinishedCommand != nil }
    var body: some View {
        StudioPanel {
            Label("Needs your answer", systemImage: "person.crop.circle.badge.questionmark").font(.headline).foregroundStyle(Studio.warning)
            Text(input.prompt).textSelection(.enabled)
            if !supported {
                unavailable("This owner does not currently allow remote answers. Open the request on its Mac.")
            } else {
                switch input.payload {
                case .permission(let requestId, let toolName, let arguments):
                    permission(requestId: requestId, toolName: toolName, arguments: arguments)
                case .form(let schema): form(schema)
                case .question(_, let why, let recommendedAnswer, _): question(why: why, recommendedAnswer: recommendedAnswer)
                case .unsupported: unavailable("This request type is not supported on this device. Open it on the owner Mac.")
                }
            }
        }
        .onChange(of: input) { drafts = [:]; answer = "" }
    }

    @ViewBuilder private func question(why: String, recommendedAnswer: String) -> some View {
        if !why.isEmpty { Text(why).font(.subheadline).textSelection(.enabled) }
        if !recommendedAnswer.isEmpty {
            Text("Suggested answer").font(.subheadline.weight(.semibold))
            Text(recommendedAnswer).textSelection(.enabled)
        }
        if input.options.contains(where: \.acceptsValue) {
            TextField("Your answer", text: $answer, axis: .vertical)
                .textFieldStyle(.roundedBorder).disabled(busy)
                .accessibilityIdentifier("question-answer")
        }
        ForEach(input.options) { choice in
            Button(choice.label) {
                Task { await store.answer(input: input, option: choice, value: choice.acceptsValue ? answer : nil) }
            }.buttonStyle(.bordered).disabled(busy || (choice.acceptsValue && answer.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty))
        }
    }

    @ViewBuilder private func permission(requestId: String, toolName: String, arguments: JSONValue) -> some View {
        let rendered = Result { try arguments.displayJSON(pretty: true) }
        switch rendered {
        case .success(let text):
            Text(toolName).font(.headline.monospaced()).textSelection(.enabled)
            Text("Requested operation and arguments").font(.subheadline.weight(.semibold))
            Text(text).font(.body.monospaced()).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("permission-arguments")
            Text("Request \(requestId)").font(.caption.monospaced()).foregroundStyle(Studio.secondaryInk).textSelection(.enabled)
            ForEach(input.options) { choice in
                Button(choice.label) { Task { await store.answer(input: input, option: choice, value: nil) } }
                    .buttonStyle(.bordered).disabled(busy || choice.acceptsValue)
            }
        case .failure(let error): unavailable(error.localizedDescription)
        }
    }

    @ViewBuilder private func form(_ schema: JSONValue) -> some View {
        let parsed = Result { try InputForm(schema: schema) }
        switch parsed {
        case .failure(let error): unavailable(error.localizedDescription)
        case .success(let form):
            ForEach(form.fields) { field in fieldView(field) }
            let answer = Result { try form.answer(drafts) }
            switch answer {
            case .failure(let error): Text(error.localizedDescription).font(.caption).foregroundStyle(Studio.warning).accessibilityIdentifier("form-validation")
            case .success: EmptyView()
            }
            ForEach(input.options) { choice in
                Button(choice.label) {
                    let value: String?
                    if choice.acceptsValue {
                        guard case .success(let result) = answer else { return }
                        value = result
                    } else { value = nil }
                    Task { await store.answer(input: input, option: choice, value: value) }
                }.buttonStyle(.bordered).disabled(busy || (choice.acceptsValue && !answer.isSuccess))
            }
        }
    }

    @ViewBuilder private func fieldView(_ field: InputField) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(field.title).font(.subheadline.weight(.semibold))
                if field.required { Text("Required").font(.caption).foregroundStyle(Studio.secondaryInk) }
                else if drafts[field.id] != nil { Button("Clear") { drafts[field.id] = nil }.font(.caption) }
            }
            if field.title != field.id { Text(field.id).font(.caption.monospaced()).foregroundStyle(Studio.secondaryInk) }
            if let detail = field.detail { Text(detail).font(.caption).foregroundStyle(Studio.secondaryInk) }
            if let choices = field.choices {
                Picker(field.title, selection: Binding(get: { drafts[field.id] ?? "" }, set: { drafts[field.id] = $0.isEmpty ? nil : $0 })) {
                    Text("Choose a value").tag("")
                    ForEach(Array(choices.enumerated()), id: \.offset) { index, value in Text(field.choiceLabel(value)).tag(String(index)) }
                }.pickerStyle(.menu)
            } else if field.kind == .boolean {
                Picker(field.title, selection: Binding(get: { drafts[field.id] ?? "" }, set: { drafts[field.id] = $0.isEmpty ? nil : $0 })) {
                    Text("Choose a value").tag("")
                    Text("Yes").tag("true")
                    Text("No").tag("false")
                }.pickerStyle(.menu)
            } else {
                TextField(field.kind == .string ? "Enter text" : "Enter a number", text: Binding(get: { drafts[field.id] ?? "" }, set: { drafts[field.id] = $0 }), axis: .vertical)
                    .textFieldStyle(.roundedBorder).keyboardType(field.kind == .string ? .default : .numbersAndPunctuation)
                    .accessibilityLabel(field.title).accessibilityIdentifier("form-field-\(field.id)")
                if field.kind == .string, drafts[field.id] == nil { Button("Use empty text") { drafts[field.id] = "" }.font(.caption) }
            }
        }
    }
    private func unavailable(_ message: String) -> some View { Text(message).font(.subheadline).foregroundStyle(Studio.secondaryInk) }
}

private extension Result {
    var isSuccess: Bool { if case .success = self { return true }; return false }
}
