import SwiftUI

struct AccountDeletionSection: View {
    @Environment(ControllerStore.self) private var store
    @State private var confirming = false

    var body: some View {
        @Bindable var store = store
        Section {
            if store.accountDeletionCompleted {
                Label("Your Link account has been deleted", systemImage: "checkmark.circle")
                Text("Local access and saved app data have been removed.")
                    .foregroundStyle(Studio.secondaryInk)
                ForEach(store.accountDeletionRetention, id: \.self) { Text($0).font(.footnote).foregroundStyle(Studio.secondaryInk) }
            } else if let saved = store.accountDeletion {
                switch saved.phase {
                case .requestingCode:
                    Text("Requesting a deletion code. If the connection was interrupted, retry to recover the same request.")
                    Button("Retry code request") { Task { await store.requestAccountDeletionCode() } }
                        .disabled(!store.canDeleteAccount)
                    cancelButton
                case .awaitingCode:
                    Text("Enter the code emailed to your Link account. Confirming permanently deletes the account and removes access for its connected devices.")
                    SecureField("Deletion code", text: $store.deletionCode)
                        .textContentType(.oneTimeCode).keyboardType(.numberPad)
                        .accessibilityLabel("Six-digit account deletion code")
                    if let challenge = saved.challenge {
                        Text("Code expires \(challenge.expiresAt, style: .time).")
                            .font(.caption).foregroundStyle(Studio.secondaryInk)
                    }
                    Button("Permanently delete account", role: .destructive) { confirming = true }
                        .disabled(!store.canDeleteAccount || store.deletionCode.trimmingCharacters(in: .whitespacesAndNewlines).count != 6)
                    Button("Request another code") { Task { await store.requestAccountDeletionCode() } }
                        .disabled(!store.canDeleteAccount)
                    cancelButton
                case .confirming:
                    Text("The deletion request may have been accepted. Check its status before using this account again.")
                    statusButton
                case .pending:
                    Text("Account access has been removed. Link is still finishing deletion. Retry to check cleanup.")
                    Text("Deletion request: \(saved.requestId.uuidString)").font(.footnote).textSelection(.enabled)
                    Link("Contact LeanZero support", destination: URL(string: "mailto:office@leanzero.net")!)
                    statusButton
                case .deleted:
                    Text("Your Link account has been deleted. Local cleanup still needs to finish on this device.")
                    statusButton
                }
            } else {
                Text("Permanently delete your Link account and remove access for all its connected devices. You will confirm with a code sent to your account email.")
                    .foregroundStyle(Studio.secondaryInk)
                Button("Email a deletion code", role: .destructive) { Task { await store.requestAccountDeletionCode() } }
                    .disabled(!store.canDeleteAccount)
            }
            if store.authBusy, store.accountDeletion != nil { ProgressView("Updating deletion request") }
        } header: {
            Text("Delete Link account")
        } footer: {
            if !store.accountDeletionCompleted {
                Text("A minimal security record is kept to prevent revoked access. Historical service records follow their existing retention. A shared email contact may remain for other LeanZero services after Link membership is removed.")
            }
        }.listRowBackground(Studio.surface)
            .confirmationDialog("Permanently delete your Link account?", isPresented: $confirming, titleVisibility: .visible) {
                Button("Delete Link account", role: .destructive) { Task { await store.confirmAccountDeletion() } }
            } message: {
                Text("This permanently deletes your Link account and removes access for all its connected devices. This cannot be undone.")
            }
    }

    private var cancelButton: some View {
        Button("Cancel deletion") {
            do { try store.cancelAccountDeletion() }
            catch { store.errorMessage = error.localizedDescription }
        }.disabled(store.authBusy)
    }
    private var statusButton: some View {
        Button("Check deletion status") { Task { await store.checkAccountDeletion() } }
            .disabled(store.authBusy)
    }
}
