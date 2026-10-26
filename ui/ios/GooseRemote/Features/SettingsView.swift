import SwiftUI

enum SettingsDestination: Hashable { case devices, privacy, iconLicense }

struct SettingsView: View {
    @Environment(ControllerStore.self) private var store
    @AppStorage(StudioTheme.storageKey) private var theme: StudioTheme = .system
    @State private var removingLocal = false
    @State private var deviceToRevoke: DeviceRecord?
    private var platform: String { UIDevice.current.userInterfaceIdiom == .pad ? "ipados" : "ios" }
    var body: some View {
        @Bindable var store = store
        Form {
            Section {
                StudioThemePicker(selection: $theme)
            } header: {
                Text("Appearance")
            } footer: { Text("System follows your device’s appearance.") }
                .listRowBackground(Studio.surface)
            if store.authenticated, let envelope = store.envelope {
                Section("This device") {
                    LabeledContent("Name", value: envelope.credentials.device.name)
                    LabeledContent("Role", value: "Controller")
                    LabeledContent("Link service", value: envelope.authority.host ?? "Unknown")
                    Text("Sessions continue on their owner Mac when this app closes or your device sleeps.")
                        .font(.subheadline).foregroundStyle(Studio.secondaryInk)
                }.listRowBackground(Studio.surface)
                Section("Access") {
                    NavigationLink("Connected devices", value: SettingsDestination.devices)
                    Button("Revoke this device", role: .destructive) { deviceToRevoke = envelope.credentials.device }
                        .disabled(store.authBusy || store.syncRunning)
                    Button("Remove local access", role: .destructive) { removingLocal = true }.disabled(store.authBusy || store.syncRunning || store.sending)
                }.listRowBackground(Studio.surface)
            } else if store.accountDeletion == nil {
                Section {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Connect to Link").font(.title2.bold())
                        Text("Use your Link service address and account email to authorize this device.")
                            .font(.subheadline).foregroundStyle(Studio.secondaryInk)
                    }.padding(.vertical, 8)
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Link service").font(.caption.weight(.semibold))
                        TextField("https://your-link-service", text: $store.authorityText)
                            .keyboardType(.URL).textInputAutocapitalization(.never).autocorrectionDisabled()
                            .accessibilityLabel("Link service HTTPS address")
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Email").font(.caption.weight(.semibold))
                        TextField("you@example.com", text: $store.email).keyboardType(.emailAddress)
                            .textContentType(.emailAddress).textInputAutocapitalization(.never).autocorrectionDisabled().accessibilityLabel("Email")
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Device name").font(.caption.weight(.semibold))
                        TextField("Name this device", text: $store.deviceName).textContentType(.nickname).accessibilityLabel("Device name")
                    }
                    Button { Task { await store.requestCode(platform: platform) } } label: {
                        HStack { Text(store.enrollment?.challenge == nil ? "Email a sign-in code" : "Request code again"); if store.authBusy { Spacer(); ProgressView() } }
                    }.buttonStyle(StudioPrimaryButtonStyle()).disabled(!store.canRequestCode)
                } footer: { Text("Credentials stay in this device’s Keychain. This app receives controller access only.") }.listRowBackground(Studio.surface)
                if let challenge = store.enrollment?.challenge {
                    Section("Check your email") {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Sign-in code").font(.caption.weight(.semibold))
                            SecureField("Code from your email", text: $store.code).textContentType(.oneTimeCode).keyboardType(.numberPad).accessibilityLabel("Sign-in code")
                        }
                        Button("Connect") { Task { await store.verify(platform: platform) } }.buttonStyle(StudioPrimaryButtonStyle()).disabled(store.authBusy || store.code.isEmpty)
                        Text("Code expires \(challenge.expiresAt, style: .time).").font(.caption).foregroundStyle(Studio.secondaryInk)
                    }.listRowBackground(Studio.surface)
                }
            }
            if store.authenticated || store.accountDeletion != nil || store.accountDeletionCompleted {
                AccountDeletionSection()
            }
            Section("About") {
                LabeledContent("Goose Swarm Remote", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "Unknown")
                Text("Native iPhone and iPad controller for Goose Swarm. Your device does not run inference, shell jobs, or model downloads.")
                    .font(.subheadline).foregroundStyle(Studio.secondaryInk)
                NavigationLink("Connection & privacy", value: SettingsDestination.privacy)
                NavigationLink("Icon license", value: SettingsDestination.iconLicense)
            }.listRowBackground(Studio.surface)
        }.navigationTitle("Settings").studioScreen()
            .navigationDestination(for: SettingsDestination.self) { destination in
                switch destination {
                case .devices: deviceList
                case .privacy: privacy
                case .iconLicense: LucideLicenseView()
                }
            }
            .onAppear { if store.deviceName.isEmpty { store.deviceName = platform == "ipados" ? "My iPad" : "My iPhone" } }
            .confirmationDialog("Remove access from this device?", isPresented: $removingLocal, titleVisibility: .visible) {
                Button("Remove local access", role: .destructive) {
                    Task { do { try await store.forgetDevice() } catch { store.errorMessage = error.localizedDescription } }
                }
            } message: { Text("This removes local credentials, drafts, and command recovery records. It does not revoke this device on the Link service. Revoke it first when you have a connection.") }
            .confirmationDialog("Revoke \(deviceToRevoke?.name ?? "device")?", isPresented: Binding(get: { deviceToRevoke != nil }, set: { if !$0 { deviceToRevoke = nil } }), titleVisibility: .visible) {
                if let device = deviceToRevoke {
                    Button("Revoke access", role: .destructive) { Task { await store.revoke(device); deviceToRevoke = nil } }
                }
            } message: { Text("This device will lose controller access. Work already running on the Macs continues.") }
    }
    private var deviceList: some View {
        List {
            ForEach(store.devices) { device in
                VStack(alignment: .leading, spacing: 8) {
                    Label(device.name, systemImage: device.platform == "ipados" ? "ipad" : "iphone").font(.headline)
                    Text(device.revokedAt == nil ? "Controller access" : "Revoked").font(.subheadline).foregroundStyle(Studio.secondaryInk)
                    if device.revokedAt == nil {
                        Button("Revoke access", role: .destructive) { deviceToRevoke = device }.disabled(store.authBusy)
                    }
                }.padding(.vertical, 8).listRowBackground(Studio.surface)
            }
            if store.devices.isEmpty { Text("No device list loaded. Pull to refresh.").foregroundStyle(Studio.secondaryInk) }
        }.navigationTitle("Connected devices").studioScreen()
            .task { await store.loadDevices() }.refreshable { await store.loadDevices() }
    }
    private var privacy: some View {
        List {
            Section("Policy & support") {
                Link("Privacy policy", destination: URL(string: "https://leanzero.net/privacy-policy")!)
                    .foregroundStyle(Studio.accentInk).accessibilityIdentifier("privacy-policy-link")
                Link("Support", destination: URL(string: "mailto:office@leanzero.net")!)
                    .foregroundStyle(Studio.accentInk).accessibilityIdentifier("support-link")
            }
            Section("On this device") {
                Text("Access and refresh credentials use device-bound Keychain storage. Drafts and session command receipts use protected app storage and are excluded from backup. Pending provider changes, including any key you enter, stay in device-bound Keychain until delivery is resolved or you clear the saved request.")
                Text("Your Mac owns session history and execution. This app displays history only for the session you select.")
            }
            Section("Connections") {
                Text("Sign-in contacts the HTTPS Link service you enter. Mac connections require verified mesh endpoints. Provider setup sends a key you enter to the selected Mac over that connection. Existing provider secrets are never read back. Session messages can be sent by your Mac to the AI and tool services configured for that session.")
                Text("This build does not include analytics, advertising, or tracking SDKs.")
            }
            Section("Remove data") {
                Text("Revoke this device to remove its server access, then remove local access to erase credentials, drafts, and command recovery records. Delete Link account removes your Link account and connected-device access. An unfinished account deletion keeps its recovery receipt in Keychain until cleanup completes. Deleting the app alone may leave Keychain items on iOS.")
            }
        }.navigationTitle("Connection & privacy").studioScreen()
    }
}
