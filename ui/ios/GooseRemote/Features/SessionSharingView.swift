import SwiftUI

struct SessionSharingView: View {
    @Environment(ControllerStore.self) private var store
    let request: SessionSharingRequest

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    Text(request.disclosure)
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
}
