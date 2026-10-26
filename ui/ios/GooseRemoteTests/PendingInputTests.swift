import XCTest
@testable import GooseRemote

final class PendingInputTests: XCTestCase {
    func json(_ text: String) throws -> JSONValue { try JSONDecoder().decode(JSONValue.self, from: Data(text.utf8)) }
    let schema = #"{"type":"object","properties":{"environment":{"type":"string","enum":["staging","production"],"title":"Deployment target"}},"required":["environment"],"additionalProperties":false}"#

    func testRequiredEnumShowsNamesChoicesAndEncodesJSONForOriginalCallback() throws {
        let form = try InputForm(schema: json(schema))
        XCTAssertEqual(form.fields[0].id, "environment")
        XCTAssertEqual(form.fields[0].title, "Deployment target")
        XCTAssertTrue(form.fields[0].required)
        XCTAssertEqual(form.fields[0].choices?.map(form.fields[0].choiceLabel), ["staging", "production"])
        XCTAssertThrowsError(try form.answer([:]))
        XCTAssertThrowsError(try form.answer(["environment": "invented"]))
        XCTAssertEqual(try form.answer(["environment": "0"]), #"{"environment":"staging"}"#)
        XCTAssertThrowsError(try form.validate(json(#"{"environment":"other"}"#)))
        XCTAssertThrowsError(try form.validate(json(#"{"environment":"staging","extra":"smuggled"}"#)))
    }
    func testRequiredBooleanDoesNotSilentlyDefaultFalse() throws {
        let form = try InputForm(schema: json(#"{"type":"object","properties":{"confirmed":{"type":"boolean"}},"required":["confirmed"]}"#))
        XCTAssertThrowsError(try form.answer([:]))
        XCTAssertEqual(try form.answer(["confirmed": "false"]), #"{"confirmed":false}"#)
        XCTAssertThrowsError(try form.answer(["confirmed": "0"]))
    }
    func testScalarConstraintsUseUnicodeCodePointsAndExactIntegerBounds() throws {
        let form = try InputForm(schema: json(#"{"type":"object","properties":{"name":{"type":"string","minLength":2,"maxLength":3},"count":{"type":"integer","minimum":9007199254740993,"maximum":9007199254740994}},"required":["name","count"]}"#))
        XCTAssertNoThrow(try form.answer(["name": "e\u{0301}", "count": "9007199254740993"]))
        XCTAssertThrowsError(try form.answer(["name": "x", "count": "9007199254740993"]))
        XCTAssertThrowsError(try form.answer(["name": "okay", "count": "9007199254740993"]))
        XCTAssertThrowsError(try form.answer(["name": "ok", "count": "9007199254740992"]))
        XCTAssertThrowsError(try form.answer(["name": "ok", "count": "1.5"]))
    }
    func testUnsupportedSchemaCannotBeSubmittedAsGenericText() throws {
        for schema in [#"{"type":"object","properties":{"target":{"type":"object","properties":{}}}}"#,
                       #"{"type":"object","properties":{"target":{"type":"string","pattern":"^[a-z]+$"}}}"#,
                       #"{"type":"object","properties":{},"allOf":[]}"#] {
            let input = PendingInput(inputId: UUID(), runId: UUID(), prompt: "Input", options: [], payload: .form(schema: try json(schema)))
            XCTAssertThrowsError(try InputForm(schema: json(schema)))
            XCTAssertThrowsError(try input.validateAnswer(option: .init(optionId: "accept", label: "Submit", acceptsValue: true), value: "{}"))
        }
    }
    func testPermissionPreservesExactOperationAndLargeIntegers() throws {
        let source = #"{"kind":"permission","request_id":"original-42","tool_name":"developer__shell","arguments":{"command":"printf hello && cat /tmp/project/config.json","job_id":18446744073709551615,"signed_id":-9007199254740993,"context":{"directory":"/tmp/project"}}}"#
        let payload = try ControllerCodec.decoder().decode(PendingInputPayload.self, from: Data(source.utf8))
        guard case .permission(let request, let tool, let args) = payload else { return XCTFail("Permission type lost") }
        XCTAssertEqual(request, "original-42"); XCTAssertEqual(tool, "developer__shell")
        let rendered = try args.displayJSON(pretty: true)
        XCTAssertTrue(rendered.contains("printf hello && cat /tmp/project/config.json"))
        XCTAssertTrue(rendered.contains("18446744073709551615"))
        XCTAssertTrue(rendered.contains("-9007199254740993"))
        XCTAssertTrue(rendered.contains("/tmp/project"))
        XCTAssertEqual(try ControllerCodec.decoder().decode(PendingInputPayload.self, from: ControllerCodec.encoder().encode(payload)), payload)
    }
    func testMissingAndUnknownPayloadFailClosed() throws {
        let missing = "{\"input_id\":\"\(UUID())\",\"run_id\":\"\(UUID())\",\"prompt\":\"Allow?\",\"options\":[]}"
        XCTAssertThrowsError(try ControllerCodec.decoder().decode(PendingInput.self, from: Data(missing.utf8)))
        let payload = try ControllerCodec.decoder().decode(PendingInputPayload.self, from: Data(#"{"kind":"future_request"}"#.utf8))
        let input = PendingInput(inputId: UUID(), runId: UUID(), prompt: "Unknown", options: [], payload: payload)
        XCTAssertThrowsError(try input.validateAnswer(option: .init(optionId: "allow", label: "Allow", acceptsValue: false), value: nil))
    }
    func testFormValueValidationCannotBeBypassedByCallingCommandBuilder() throws {
        let run = UUID(), choice = InputOption(optionId: "accept", label: "Submit", acceptsValue: true)
        let input = PendingInput(inputId: UUID(), runId: run, prompt: "Choose deployment target", options: [choice], payload: .form(schema: try json(schema)))
        let entry = CatalogEntry(work: .init(ownerNodeId: UUID(), kind: .chat, workId: UUID()), storeGeneration: UUID(), revision: 42,
            title: "Session", workingDir: nil, state: .waitingInput, currentRunId: run, capabilities: [.answerInput], parent: nil, related: [], updatedAt: Date(), lastEvent: nil, archived: false)
        XCTAssertThrowsError(try WorkCommand.answer(entry: entry, input: input, option: choice, value: "staging"))
        XCTAssertThrowsError(try WorkCommand.answer(entry: entry, input: input, option: choice, value: "{}"))
        let command = try WorkCommand.answer(entry: entry, input: input, option: choice, value: #"{"environment":"staging"}"#)
        XCTAssertEqual(command.expectedRunId, run)
        XCTAssertEqual(command.payload.inputId, input.id)
        XCTAssertEqual(command.payload.value, #"{"environment":"staging"}"#)
    }
}
