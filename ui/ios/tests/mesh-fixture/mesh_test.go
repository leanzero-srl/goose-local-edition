package meshfixture

import (
	"context"
	"crypto/rand"
	"crypto/tls"
	"encoding/hex"
	"encoding/json"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/signal"
	"path/filepath"
	"sync/atomic"
	"syscall"
	"testing"
	"time"

	"github.com/coder/websocket"
	"tailscale.com/envknob"
	"tailscale.com/net/netns"
	"tailscale.com/tsnet"
	"tailscale.com/tstest/integration"
	"tailscale.com/tstest/integration/testcontrol"
	"tailscale.com/types/logger"
)

// This is a transport fixture only. No account authority or work executor is simulated.
func TestPinnedNativeMesh(t *testing.T) {
	out := os.Getenv("GOOSE_NATIVE_MESH_OUT")
	if out == "" {
		t.Skip("explicit disposable output directory required")
	}
	if err := os.Mkdir(out, 0700); err != nil {
		t.Fatal(err)
	}
	for _, key := range []string{"TS_AUTHKEY", "TS_AUTH_KEY", "TS_CLIENT_SECRET", "TS_CLIENT_ID", "TS_ID_TOKEN", "TS_AUDIENCE", "TS_CONTROL_URL"} {
		t.Setenv(key, "")
	}
	envknob.SetenvForTest(t, "TS_NO_LOGS_NO_SUPPORT", "true")
	netns.SetEnabled(false)
	t.Cleanup(func() { netns.SetEnabled(true) })
	ctx, cancel := signal.NotifyContext(t.Context(), os.Interrupt, syscall.SIGTERM)
	t.Cleanup(cancel)
	derp := integration.RunDERPAndSTUN(t, logger.Discard, "127.0.0.1")
	if derp == nil || len(derp.Regions) == 0 {
		t.Fatal("no local relay map")
	}
	for _, region := range derp.Regions {
		if region == nil || len(region.Nodes) == 0 {
			t.Fatal("empty relay region")
		}
		for _, node := range region.Nodes {
			if node == nil || node.HostName != "127.0.0.1" || node.IPv4 != "127.0.0.1" || node.IPv6 != "none" || node.STUNTestIP != "127.0.0.1" {
				t.Fatal("nonlocal relay refused")
			}
		}
	}
	random := func() string {
		b := make([]byte, 32)
		if _, err := rand.Read(b); err != nil {
			t.Fatal(err)
		}
		return hex.EncodeToString(b)
	}
	key, nonce := "disposable-native-"+random(), random()
	controlURL, ownerKey := "", key
	resumeWithoutKey := os.Getenv("GOOSE_NATIVE_HEADSCALE_BINARY") != ""
	if resumeWithoutKey {
		controlURL, ownerKey, key = isolatedHeadscale(t, ctx, derp)
	} else {
		control := &testcontrol.Server{DERPMap: derp, RequireAuthKey: key, Logf: logger.Discard}
		control.HTTPTestServer = httptest.NewUnstartedServer(control)
		listener, err := net.Listen("tcp4", "127.0.0.1:0")
		if err != nil {
			t.Fatal(err)
		}
		_ = control.HTTPTestServer.Listener.Close()
		control.HTTPTestServer.Listener = listener
		control.HTTPTestServer.Start()
		t.Cleanup(control.HTTPTestServer.Close)
		controlURL = control.HTTPTestServer.URL
	}
	owner := &tsnet.Server{Dir: t.TempDir(), Hostname: "native-pinned-owner", ControlURL: controlURL, AuthKey: ownerKey, Ephemeral: true, Logf: logger.Discard, UserLogf: logger.Discard}
	t.Cleanup(func() { _ = owner.Close() })
	status, err := owner.Up(ctx)
	if err != nil {
		t.Fatal(err)
	}
	var ip string
	for _, address := range status.TailscaleIPs {
		if address.Is4() {
			ip = address.String()
			break
		}
	}
	if ip == "" {
		t.Fatal("owner has no mesh address")
	}
	mesh, err := owner.Listen("tcp", ":0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = mesh.Close() })
	_, port, err := net.SplitHostPort(mesh.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	fixtures := os.Getenv("GOOSE_NATIVE_TLS_FIXTURES")
	cert, err := tls.LoadX509KeyPair(filepath.Join(fixtures, "valid.pem"), filepath.Join(fixtures, "disposable-key.pem"))
	if err != nil {
		t.Fatal(err)
	}
	requests, err := os.OpenFile(filepath.Join(out, "requests.jsonl"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0600)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = requests.Close() })
	var commands, lostAckCommands atomic.Int64
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Origin") != "" {
			w.WriteHeader(403)
			return
		}
		if r.Header.Get("Authorization") != "Bearer disposable-test-token" {
			w.WriteHeader(401)
			return
		}
		line, _ := json.Marshal(map[string]string{"remote": r.RemoteAddr, "path": r.URL.Path})
		if _, err := requests.Write(append(line, '\n')); err != nil {
			t.Error(err)
			w.WriteHeader(500)
			return
		}
		if r.URL.Path == "/command" || r.URL.Path == "/command-lost-ack" {
			body, err := io.ReadAll(io.LimitReader(r.Body, 1024))
			if err != nil || r.Method != "POST" || string(body) != nonce {
				w.WriteHeader(400)
				return
			}
			if r.URL.Path == "/command" {
				commands.Add(1)
			} else {
				lostAckCommands.Add(1)
				conn, _, err := w.(http.Hijacker).Hijack()
				if err == nil {
					_ = conn.Close()
				}
				return
			}
		}
		payload, _ := json.Marshal(map[string]any{"nonce": nonce, "commands": commands.Load(), "lost_ack_commands": lostAckCommands.Load()})
		if r.URL.Path == "/events" {
			conn, err := websocket.Accept(w, r, nil)
			if err != nil {
				return
			}
			defer conn.CloseNow()
			if err := conn.Write(r.Context(), websocket.MessageText, payload); err != nil {
				return
			}
			<-conn.CloseRead(r.Context()).Done()
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write(payload)
	})
	server := &http.Server{Handler: handler, ReadHeaderTimeout: 10 * time.Second, BaseContext: func(net.Listener) context.Context { return ctx }}
	t.Cleanup(func() { _ = server.Close() })
	go func() {
		_ = server.Serve(tls.NewListener(mesh, &tls.Config{MinVersion: tls.VersionTLS12, Certificates: []tls.Certificate{cert}}))
	}()
	config, _ := json.Marshal(map[string]any{"fixture": true, "controlURL": controlURL, "resumeWithoutKey": resumeWithoutKey, "authKey": key, "endpoint": "https://" + net.JoinHostPort(ip, port), "nonce": nonce})
	configPath := filepath.Join(out, "mesh-runtime.json")
	if err := os.WriteFile(configPath, config, 0600); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Remove(configPath) })
	t.Log("READY")
	<-ctx.Done()
}
