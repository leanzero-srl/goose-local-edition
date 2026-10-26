import Darwin
import SwiftUI
import TailscaleKit
import UIKit

#if !targetEnvironment(simulator)
#error("This diagnostic is only for the iOS Simulator.")
#endif

struct ProbeCheck: Encodable, Identifiable {
    let id: String
    let observed: Int32
    let expected: String
    let passed: Bool
}

struct ProbeReceipt: Encodable {
    let schemaVersion = 1
    let measuredAt: Date
    let platform = "iOS Simulator (arm64)"
    let systemName: String
    let systemVersion: String
    let deviceFamily: String
    let simulatorModel: String?
    let frameworkSHA256: String
    let scope = "C handle lifecycle only"
    let linkNetworkingTested = false
    let checks: [ProbeCheck]
    let passed: Bool

    @MainActor
    static func measure() throws -> ProbeReceipt {
        guard let frameworkSHA256 = Bundle.main.object(
            forInfoDictionaryKey: "ProbeFrameworkSHA256"
        ) as? String else {
            throw CocoaError(.fileReadCorruptFile)
        }

        let first = tailscale_new()
        let firstClose = tailscale_close(first)
        let duplicateClose = tailscale_close(first)
        let second = tailscale_new()
        let secondClose = tailscale_close(second)
        let checks = [
            ProbeCheck(id: "First handle", observed: first,
                       expected: "positive; accepted by close", passed: first > 0 && firstClose == 0),
            ProbeCheck(id: "First close", observed: firstClose,
                       expected: "0", passed: firstClose == 0),
            ProbeCheck(id: "Duplicate close", observed: duplicateClose,
                       expected: "EBADF (\(Darwin.EBADF))", passed: duplicateClose == Darwin.EBADF),
            ProbeCheck(id: "Fresh handle", observed: second,
                       expected: "positive; different from \(first)", passed: second > 0 && second != first),
            ProbeCheck(id: "Fresh handle close", observed: secondClose,
                       expected: "0", passed: secondClose == 0)
        ]
        let device = UIDevice.current
        let family: String
        switch device.userInterfaceIdiom {
        case .phone: family = "iPhone"
        case .pad: family = "iPad"
        default: family = "unsupported (\(device.userInterfaceIdiom.rawValue))"
        }
        return ProbeReceipt(
            measuredAt: Date(), systemName: device.systemName,
            systemVersion: device.systemVersion, deviceFamily: family,
            simulatorModel: ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"],
            frameworkSHA256: frameworkSHA256, checks: checks,
            passed: checks.allSatisfy(\.passed)
        )
    }

    func save() throws -> URL {
        let documents = try FileManager.default.url(
            for: .documentDirectory, in: .userDomainMask,
            appropriateFor: nil, create: true
        )
        let destination = documents.appendingPathComponent("probe-receipt.json")
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        try encoder.encode(self).write(to: destination, options: .atomic)
        return destination
    }
}

@MainActor
final class ProbeModel: ObservableObject {
    @Published private(set) var receipt: ProbeReceipt?
    @Published private(set) var receiptPath: String?
    @Published private(set) var error: String?

    init() {
        do {
            let measured = try ProbeReceipt.measure()
            receipt = measured
            receiptPath = try measured.save().path
        } catch {
            self.error = error.localizedDescription
        }
    }
}

@main
struct GooseLinkProbeApp: App {
    @StateObject private var model = ProbeModel()

    var body: some Scene {
        WindowGroup {
            NavigationStack {
                List {
                    Section {
                        Text("Link networking not tested")
                            .font(.headline)
                        Text("Simulator diagnostic: framework loading and C handle lifecycle only.")
                            .foregroundStyle(.secondary)
                    }
                    if let receipt = model.receipt {
                        Section("Measured results") {
                            Label(receipt.passed ? "Handle checks passed" : "Handle checks failed",
                                  systemImage: receipt.passed ? "checkmark.circle.fill" : "xmark.circle.fill")
                                .foregroundStyle(receipt.passed ? Color.green : Color.red)
                            ForEach(receipt.checks) { check in
                                VStack(alignment: .leading, spacing: 4) {
                                    Label(check.id, systemImage: check.passed ? "checkmark" : "xmark")
                                    Text("Observed: \(check.observed) · Expected: \(check.expected)")
                                        .font(.caption.monospaced())
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                        Section("Runtime") {
                            Text("\(receipt.deviceFamily) · \(receipt.systemName) \(receipt.systemVersion)")
                            Text(receipt.platform)
                            Text(receipt.measuredAt, format: .dateTime.year().month().day().hour().minute().second())
                        }
                    }
                    Section("Receipt") {
                        if let error = model.error {
                            Label("Probe or receipt error", systemImage: "exclamationmark.triangle.fill")
                                .foregroundStyle(.red)
                            Text(error)
                        } else if let path = model.receiptPath {
                            Text("Saved probe-receipt.json")
                            Text(path).font(.caption).textSelection(.enabled)
                        }
                    }
                }
                .navigationTitle("Goose Link Probe")
            }
        }
    }
}
