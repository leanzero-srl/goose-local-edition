package meshfixture

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/hex"
	"encoding/pem"
	"errors"
	"fmt"
	"math/big"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"testing"
	"time"

	"tailscale.com/envknob"
	"tailscale.com/net/netns"
	"tailscale.com/tailcfg"
	"tailscale.com/tsnet"
	"tailscale.com/tstest/integration"
	"tailscale.com/tstest/integration/testcontrol"
	"tailscale.com/types/logger"
)

type runtimeConfig struct {
	SchemaVersion     int                   `json:"schemaVersion"`
	Fixture           bool                  `json:"fixture"`
	RunID             string                `json:"runID"`
	ControlURL        string                `json:"controlURL"`
	FixtureAuthKey    string                `json:"fixtureAuthKey"`
	PeerHTTPSURL      string                `json:"peerHTTPSURL"`
	PeerWSSURL        string                `json:"peerWSSURL"`
	Nonce             string                `json:"nonce"`
	OwnerID           string                `json:"ownerID"`
	Epoch             string                `json:"epoch"`
	CACertificatePath string                `json:"caCertificatePath"`
	StopFilePath      string                `json:"stopFilePath"`
	Controllers       map[string]controller `json:"controllers"`
}

// TestIOSMeshFixture is a deliberately long-lived fixture, not a success assertion.
// A clean stop proves teardown; simulator receipts prove transport behavior.
func TestIOSMeshFixture(t *testing.T) {
	out := os.Getenv("GOOSE_IOS_FIXTURE_OUT")
	if out == "" {
		t.Skip("set GOOSE_IOS_FIXTURE_OUT to a new disposable directory to start the mesh fixture")
	}
	out, err := filepath.Abs(out)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(out, 0700); err != nil {
		t.Fatalf("output directory must not already exist: %v", err)
	}
	var f *fixture
	configPath := filepath.Join(out, "runtime-config.json")
	t.Cleanup(func() {
		if err := os.Remove(configPath); err != nil && !errors.Is(err, os.ErrNotExist) {
			t.Errorf("remove expired fixture credentials: %v", err)
		}
		if f != nil {
			if err := writeJSON(filepath.Join(out, "stopped.json"), map[string]any{
				"runID": f.RunID, "stoppedAt": time.Now().UTC().Format(time.RFC3339Nano), "failed": t.Failed(),
			}); err != nil {
				t.Errorf("write stopped receipt: %v", err)
			}
		}
	})
	for _, name := range []string{
		"TS_AUTHKEY", "TS_AUTH_KEY", "TS_CLIENT_SECRET", "TS_CLIENT_ID", "TS_ID_TOKEN", "TS_AUDIENCE", "TS_CONTROL_URL",
	} {
		t.Setenv(name, "")
	}
	envknob.SetenvForTest(t, "TS_NO_LOGS_NO_SUPPORT", "true")
	netns.SetEnabled(false)
	t.Cleanup(func() { netns.SetEnabled(true) })
	ctx, cancel := signal.NotifyContext(t.Context(), os.Interrupt, syscall.SIGTERM)
	t.Cleanup(cancel)
	derpMap := integration.RunDERPAndSTUN(t, logger.Discard, "127.0.0.1")
	if err := validateLocalDERP(derpMap); err != nil {
		t.Fatal(err)
	}
	fixtureAuthKey := "goose-ios-disposable-" + randomID(t)
	control := &testcontrol.Server{
		DERPMap: derpMap, RequireAuthKey: fixtureAuthKey, Logf: logger.Discard,
	}
	control.HTTPTestServer = httptest.NewUnstartedServer(control)
	controlListener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	_ = control.HTTPTestServer.Listener.Close()
	control.HTTPTestServer.Listener = controlListener
	control.HTTPTestServer.Start()
	t.Cleanup(func() {
		control.HTTPTestServer.CloseClientConnections()
		control.HTTPTestServer.Close()
	})
	owner := &tsnet.Server{
		Dir: t.TempDir(), Hostname: "fixture-owner", ControlURL: control.HTTPTestServer.URL,
		AuthKey: fixtureAuthKey, Ephemeral: true, Logf: logger.Discard, UserLogf: logger.Discard,
	}
	t.Cleanup(func() {
		if err := owner.Close(); err != nil {
			t.Errorf("close fixture owner: %v", err)
		}
	})
	status, err := owner.Up(ctx)
	if err != nil {
		t.Fatal(err)
	}
	var peerIP netip.Addr
	for _, ip := range status.TailscaleIPs {
		if ip.Is4() {
			peerIP = ip
			break
		}
	}
	if !peerIP.IsValid() {
		t.Fatal("fixture owner has no IPv4 mesh address")
	}
	certificate, caPEM := fixtureCertificate(t, status.TailscaleIPs)
	caPath := filepath.Join(out, "ca.pem")
	if err := os.WriteFile(caPath, caPEM, 0600); err != nil {
		t.Fatal(err)
	}
	meshListener, err := owner.Listen("tcp", ":0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = meshListener.Close() })
	_, port, err := net.SplitHostPort(meshListener.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	f = newFixture(t, out)
	server := &http.Server{
		Handler: f, ReadHeaderTimeout: 10 * time.Second,
		BaseContext: func(net.Listener) context.Context { return ctx },
		ErrorLog:    logger.StdLogger(logger.Discard),
	}
	served := make(chan error, 1)
	go func() {
		served <- server.Serve(tls.NewListener(meshListener, &tls.Config{
			Certificates: []tls.Certificate{certificate}, MinVersion: tls.VersionTLS12,
		}))
	}()
	t.Cleanup(func() {
		f.beginShutdown()
		cancel()
		if err := server.Close(); err != nil {
			t.Errorf("close fixture HTTPS: %v", err)
		}
		f.active.Wait()
		select {
		case err := <-f.failed:
			t.Errorf("fixture handler failed during shutdown: %v", err)
		default:
		}
	})
	config := runtimeConfig{
		SchemaVersion: 1, Fixture: true, RunID: f.RunID, ControlURL: control.HTTPTestServer.URL,
		FixtureAuthKey: fixtureAuthKey, PeerHTTPSURL: "https://" + net.JoinHostPort(peerIP.String(), port),
		PeerWSSURL: "wss://" + net.JoinHostPort(peerIP.String(), port) + "/events",
		Nonce:      f.Nonce, OwnerID: f.OwnerID, Epoch: f.Epoch, CACertificatePath: caPath,
		StopFilePath: filepath.Join(out, "stop"), Controllers: f.Controllers,
	}
	if err := writeJSON(configPath, config); err != nil {
		t.Fatal(err)
	}
	t.Logf("fixture ready; runtime config: %s", configPath)
	t.Logf("stop by creating: %s", config.StopFilePath)
	ticker := time.NewTicker(250 * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case err := <-served:
			t.Fatalf("fixture HTTPS stopped unexpectedly: %v", err)
		case err := <-f.failed:
			t.Fatal(err)
		case <-ticker.C:
			_, err := os.Stat(config.StopFilePath)
			if err == nil {
				return
			}
			if !errors.Is(err, os.ErrNotExist) {
				t.Fatal(err)
			}
		}
	}
}

func newFixture(t *testing.T, out string) *fixture {
	t.Helper()
	audit, err := os.OpenFile(filepath.Join(out, "requests.jsonl"), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := audit.Close(); err != nil {
			t.Errorf("close fixture audit: %v", err)
		}
	})
	controllers := map[string]controller{
		"phone": {DeviceID: "fixture-phone", Token: randomID(t)},
		"ipad":  {DeviceID: "fixture-ipad", Token: randomID(t)},
	}
	f := &fixture{
		RunID: randomID(t), OwnerID: "fixture-owner", Epoch: randomID(t), Nonce: randomID(t),
		Controllers: controllers, OutputDir: out, commands: make(map[string]commandRecord),
		deliveryCounts: make(map[string]int),
		streams:        make(map[string]*deviceEvents), auditFile: audit, failed: make(chan error, 1),
	}
	for _, c := range controllers {
		f.streams[c.DeviceID] = &deviceEvents{Changed: make(chan struct{})}
	}
	return f
}

func validateLocalDERP(m *tailcfg.DERPMap) error {
	if m == nil || len(m.Regions) == 0 {
		return errors.New("refusing nil or empty DERP map: nil would select production DERP")
	}
	for _, region := range m.Regions {
		if region == nil || len(region.Nodes) == 0 {
			return errors.New("refusing empty DERP region")
		}
		for _, node := range region.Nodes {
			if node == nil || node.HostName != "127.0.0.1" || node.IPv4 != "127.0.0.1" ||
				node.IPv6 != "none" || node.STUNTestIP != "127.0.0.1" || node.DERPPort <= 0 || node.STUNPort <= 0 {
				return errors.New("refusing non-loopback DERP/STUN endpoint")
			}
		}
	}
	return nil
}

func randomID(t *testing.T) string {
	t.Helper()
	data := make([]byte, 32)
	if _, err := rand.Read(data); err != nil {
		t.Fatal(err)
	}
	return hex.EncodeToString(data)
}

func fixtureCertificate(t *testing.T, addresses []netip.Addr) (tls.Certificate, []byte) {
	t.Helper()
	caKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	leafKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now()
	ca := &x509.Certificate{
		SerialNumber: big.NewInt(1), Subject: pkix.Name{CommonName: "Disposable Goose iOS fixture CA"},
		NotBefore: now.Add(-time.Minute), NotAfter: now.Add(24 * time.Hour),
		IsCA: true, BasicConstraintsValid: true, KeyUsage: x509.KeyUsageCertSign | x509.KeyUsageCRLSign,
	}
	caDER, err := x509.CreateCertificate(rand.Reader, ca, ca, &caKey.PublicKey, caKey)
	if err != nil {
		t.Fatal(err)
	}
	leaf := &x509.Certificate{
		SerialNumber: big.NewInt(2), Subject: pkix.Name{CommonName: "fixture-owner"},
		NotBefore: ca.NotBefore, NotAfter: ca.NotAfter,
		KeyUsage: x509.KeyUsageDigitalSignature, ExtKeyUsage: []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
	}
	for _, ip := range addresses {
		leaf.IPAddresses = append(leaf.IPAddresses, net.IP(ip.AsSlice()))
	}
	leafDER, err := x509.CreateCertificate(rand.Reader, leaf, ca, &leafKey.PublicKey, caKey)
	if err != nil {
		t.Fatal(err)
	}
	keyDER, err := x509.MarshalPKCS8PrivateKey(leafKey)
	if err != nil {
		t.Fatal(err)
	}
	certificate, err := tls.X509KeyPair(
		pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: leafDER}),
		pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: keyDER}),
	)
	if err != nil {
		t.Fatal(fmt.Errorf("fixture TLS certificate: %w", err))
	}
	return certificate, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: caDER})
}
