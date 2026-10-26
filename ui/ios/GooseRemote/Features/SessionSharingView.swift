import SwiftUI

struct SessionSharingView: View {
    @Environment(ControllerStore.self) private var store
    let request: SessionSharingRequest

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    Text(request.disclosure)
                    if let sharing = request.sharing {
                        reportedServices(sharing)
                    }
                    Text(request.recipientLimitation).font(.subheadline).foregroundStyle(Studio.secondaryInk)
                    StudioPanel {
                        Label(request.ownerName, icon: .laptop).font(.headline)
                        Text(request.actionTitle).font(.subheadline.weight(.semibold))
                        Text(request.reviewText).textSelection(.enabled)
                            .accessibilityIdentifier("sharing-action-review")
                    }
                    Text("Your permission applies to this action only.").font(.caption).foregroundStyle(Studio.secondaryInk)
                    Button("Allow sharing and \(request.actionTitle.lowercased())") {
                        Task { await store.confirmSharing(request.id) }
                    }.buttonStyle(StudioPrimaryButtonStyle()).accessibilityIdentifier("sharing-allow")
                    Button("Do not send", role: .cancel) { store.declineSharing() }
                        .buttonStyle(StudioSecondaryButtonStyle()).accessibilityIdentifier("sharing-decline")
                }.padding(20).frame(maxWidth: 700)
            }
            .studioScreen().navigationTitle("Allow data sharing?").navigationBarTitleDisplayMode(.inline)
        }
    }
    private func reportedServices(_ sharing: SessionSharingDescriptor) -> some View {
        StudioPanel {
            Text("Sharing details").font(.headline)
            Text(sharing.sourceDescription).font(.caption).foregroundStyle(Studio.secondaryInk)
            if sharing.showsReportedServices {
                if let provider = sharing.provider {
                    service(name: provider.name, origin: provider.origin, role: "AI provider")
                } else {
                    Text("AI provider and destination not reported.").font(.subheadline)
                }
                if sharing.tools.isEmpty {
                    Text("No tool destinations reported.").font(.subheadline)
                } else {
                    ForEach(Array(sharing.tools.enumerated()), id: \.offset) { _, tool in
                        service(name: tool.name, origin: tool.origin, role: "Tool")
                    }
                }
            }
        }.accessibilityIdentifier("sharing-services")
    }
    private func service(name: String, origin: String?, role: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("\(role): \(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "Name unavailable" : name)")
                .font(.subheadline.weight(.semibold))
            Text(origin ?? "Destination not reported").font(.caption.monospaced()).textSelection(.enabled)
                .foregroundStyle(Studio.secondaryInk)
        }
    }
}
