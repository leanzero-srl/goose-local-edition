import Foundation

enum PendingInputPayload: Codable, Equatable, Sendable {
    case permission(requestId: String, toolName: String, arguments: JSONValue)
    case form(schema: JSONValue)
    case question(itemId: String, why: String, recommendedAnswer: String, originRunId: UUID?)
    case unsupported(kind: String)

    private enum Keys: String, CodingKey { case kind, requestId, toolName, arguments, schema, itemId, why, recommendedAnswer, originRunId }
    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: Keys.self)
        let kind = try values.decode(String.self, forKey: .kind)
        switch kind {
        case "permission":
            let requestId = try values.decode(String.self, forKey: .requestId)
            let toolName = try values.decode(String.self, forKey: .toolName)
            let arguments = try values.decode(JSONValue.self, forKey: .arguments)
            guard !requestId.isEmpty, !toolName.isEmpty, case .object = arguments else { throw ControllerError.malformedResponse }
            self = .permission(requestId: requestId, toolName: toolName, arguments: arguments)
        case "form": self = .form(schema: try values.decode(JSONValue.self, forKey: .schema))
        case "question":
            let itemId = try values.decode(String.self, forKey: .itemId)
            guard !itemId.isEmpty else { throw ControllerError.malformedResponse }
            self = .question(itemId: itemId, why: try values.decode(String.self, forKey: .why),
                             recommendedAnswer: try values.decode(String.self, forKey: .recommendedAnswer),
                             originRunId: try values.decodeIfPresent(UUID.self, forKey: .originRunId))
        default: self = .unsupported(kind: kind)
        }
    }
    func encode(to encoder: Encoder) throws {
        var values = encoder.container(keyedBy: Keys.self)
        switch self {
        case .permission(let requestId, let toolName, let arguments):
            try values.encode("permission", forKey: .kind)
            try values.encode(requestId, forKey: .requestId)
            try values.encode(toolName, forKey: .toolName)
            try values.encode(arguments, forKey: .arguments)
        case .form(let schema):
            try values.encode("form", forKey: .kind)
            try values.encode(schema, forKey: .schema)
        case .question(let itemId, let why, let recommendedAnswer, let originRunId):
            try values.encode("question", forKey: .kind)
            try values.encode(itemId, forKey: .itemId)
            try values.encode(why, forKey: .why)
            try values.encode(recommendedAnswer, forKey: .recommendedAnswer)
            try values.encodeIfPresent(originRunId, forKey: .originRunId)
        case .unsupported(let kind): try values.encode(kind, forKey: .kind)
        }
    }
}

extension PendingInput {
    func validateAnswer(option: InputOption, value: String?) throws {
        switch payload {
        case .permission(let requestId, let toolName, let arguments):
            guard runId != nil, !requestId.isEmpty, !toolName.isEmpty, case .object = arguments,
                  !option.acceptsValue, value == nil else { throw ControllerError.invalid("The permission request is incomplete or has an incompatible response.") }
        case .form(let schema):
            let form = try InputForm(schema: schema)
            if option.acceptsValue {
                guard let value else { throw ControllerError.invalid("Complete the form before submitting it.") }
                try form.validate(JSONDecoder().decode(JSONValue.self, from: Data(value.utf8)))
            } else if value != nil { throw ControllerError.invalid("This choice does not accept a form value.") }
        case .question(let itemId, _, _, _):
            guard runId == nil, !itemId.isEmpty else { throw ControllerError.invalid("Refresh this question before answering.") }
            if option.acceptsValue {
                guard let value, !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { throw ControllerError.invalid("Write an answer first.") }
            } else if value != nil { throw ControllerError.invalid("This choice does not accept extra text.") }
        case .unsupported: throw ControllerError.unsupported("This request type cannot be answered on this device. Open it on the owner Mac.")
        }
    }
}

struct InputForm: Sendable {
    let fields: [InputField]
    init(schema: JSONValue) throws {
        guard case .object(let root) = schema, root["type"] == .string("object"),
              case .object(let properties) = root["properties"] else { throw Self.unsupported }
        try Self.checkKeys(root, allowed: ["$schema", "$id", "title", "description", "type", "properties", "required", "additionalProperties"])
        if let additional = root["additionalProperties"], case .bool = additional {} else if root["additionalProperties"] != nil { throw Self.unsupported }
        let required: Set<String>
        if let value = root["required"] {
            guard case .array(let values) = value else { throw ControllerError.malformedResponse }
            let names = try values.map { value -> String in
                guard case .string(let name) = value else { throw ControllerError.malformedResponse }
                return name
            }
            guard Set(names).count == names.count, Set(names).isSubset(of: Set(properties.keys)) else { throw ControllerError.malformedResponse }
            required = Set(names)
        } else { required = [] }
        fields = try properties.keys.sorted().map { try InputField(id: $0, schema: properties[$0]!, required: required.contains($0)) }
    }
    func answer(_ drafts: [String: String]) throws -> String {
        var values: [String: JSONValue] = [:]
        for field in fields {
            if let draft = drafts[field.id] { values[field.id] = try field.value(draft) }
        }
        let result = JSONValue.object(values)
        try validate(result)
        return try result.displayJSON()
    }
    func validate(_ answer: JSONValue) throws {
        guard case .object(let values) = answer, Set(values.keys).isSubset(of: Set(fields.map(\.id))) else { throw ControllerError.invalid("The answer must contain only the fields in this form.") }
        for field in fields {
            guard let value = values[field.id] else {
                if field.required { throw ControllerError.invalid("\(field.title) is required.") }
                continue
            }
            try field.validate(value)
        }
    }
    static var unsupported: ControllerError { .unsupported("This form uses a schema this device cannot safely render. Open it on the owner Mac.") }
    static func checkKeys(_ schema: [String: JSONValue], allowed: Set<String>) throws {
        guard Set(schema.keys).isSubset(of: allowed) else { throw unsupported }
    }
}

struct InputField: Identifiable, Sendable {
    enum Kind: String, Sendable { case string, number, integer, boolean }
    let id: String
    let title: String
    let detail: String?
    let required: Bool
    let kind: Kind
    let choices: [JSONValue]?
    private let schema: [String: JSONValue]

    init(id: String, schema: JSONValue, required: Bool) throws {
        guard case .object(let values) = schema, case .string(let type) = values["type"], let kind = Kind(rawValue: type) else { throw InputForm.unsupported }
        try InputForm.checkKeys(values, allowed: ["type", "title", "description", "enum", "default", "minLength", "maxLength", "minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"])
        self.id = id; self.required = required; self.kind = kind; self.schema = values
        if let value = values["title"] { guard case .string(let text) = value else { throw ControllerError.malformedResponse }; title = text } else { title = id }
        if let value = values["description"] { guard case .string(let text) = value else { throw ControllerError.malformedResponse }; detail = text } else { detail = nil }
        if let value = values["enum"] {
            guard case .array(let options) = value, !options.isEmpty, Set(options).count == options.count else { throw ControllerError.malformedResponse }
            choices = options
        } else { choices = nil }
        for key in ["minLength", "maxLength"] where values[key] != nil {
            guard kind == .string, let number = values[key]?.decimalValue, number >= 0, number <= Decimal(Int.max), number == number.roundedInteger else { throw InputForm.unsupported }
        }
        for key in ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"] where values[key] != nil {
            guard kind == .number || kind == .integer, values[key]?.decimalValue != nil else { throw InputForm.unsupported }
        }
        if let choices { for choice in choices { try validate(choice) } }
        if let value = values["default"] { try validate(value) }
    }
    func choiceLabel(_ value: JSONValue) -> String {
        if case .string(let text) = value { return text }
        return (try? value.displayJSON()) ?? "Invalid choice"
    }
    func value(_ text: String) throws -> JSONValue {
        if let choices {
            guard let index = Int(text), choices.indices.contains(index) else { throw ControllerError.invalid("Choose a value for \(title).") }
            return choices[index]
        }
        if kind == .string { return .string(text) }
        guard let value = try? JSONDecoder().decode(JSONValue.self, from: Data(text.utf8)) else { throw ControllerError.invalid("Enter a valid \(kind.rawValue) for \(title).") }
        try validate(value)
        return value
    }
    func validate(_ value: JSONValue) throws {
        let validType: Bool
        switch (kind, value) {
        case (.string, .string), (.boolean, .bool): validType = true
        case (.number, _): validType = value.decimalValue != nil
        case (.integer, _): validType = value.decimalValue.map { $0 == $0.roundedInteger } ?? false
        default: validType = false
        }
        guard validType else { throw ControllerError.invalid("\(title) must be a \(kind.rawValue).") }
        if let choices, !choices.contains(value) { throw ControllerError.invalid("Choose one of the supplied values for \(title).") }
        if case .string(let text) = value {
            let count = Decimal(text.unicodeScalars.count)
            if let limit = schema["minLength"]?.decimalValue, count < limit { throw ControllerError.invalid("\(title) is too short.") }
            if let limit = schema["maxLength"]?.decimalValue, count > limit { throw ControllerError.invalid("\(title) is too long.") }
        }
        if let number = value.decimalValue {
            if let limit = schema["minimum"]?.decimalValue, number < limit { throw ControllerError.invalid("\(title) is below the minimum.") }
            if let limit = schema["maximum"]?.decimalValue, number > limit { throw ControllerError.invalid("\(title) is above the maximum.") }
            if let limit = schema["exclusiveMinimum"]?.decimalValue, number <= limit { throw ControllerError.invalid("\(title) must exceed its lower bound.") }
            if let limit = schema["exclusiveMaximum"]?.decimalValue, number >= limit { throw ControllerError.invalid("\(title) must be below its upper bound.") }
        }
    }
}

private extension Decimal {
    var roundedInteger: Decimal { var input = self, result = Decimal(); NSDecimalRound(&result, &input, 0, .plain); return result }
}
private extension JSONValue {
    var decimalValue: Decimal? {
        switch self {
        case .integer(let value): Decimal(value)
        case .unsignedInteger(let value): Decimal(string: String(value), locale: Locale(identifier: "en_US_POSIX"))
        case .number(let value): value.isFinite ? Decimal(string: String(value), locale: Locale(identifier: "en_US_POSIX")) : nil
        default: nil
        }
    }
}
