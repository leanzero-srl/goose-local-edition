package meshfixture

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"tailscale.com/tailcfg"
)

// The exact official binary and fresh directories keep this proof away from any installed mesh.
func isolatedHeadscale(t *testing.T, ctx context.Context, derp *tailcfg.DERPMap) (string, string, string) {
	t.Helper()
	binary := os.Getenv("GOOSE_NATIVE_HEADSCALE_BINARY")
	bytes, err := os.ReadFile(binary)
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(bytes)
	if hex.EncodeToString(sum[:]) != "fb55b6d3d1ef3b850fe02837299ac1853f383a06f561d7f180d36b9ce406904b" {
		t.Fatal("Headscale fixture requires verified v0.29.3 darwin arm64 release")
	}
	dir, err := os.MkdirTemp("/tmp", "goose-hs-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	write := func(name string, data []byte) string {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, data, 0600); err != nil {
			t.Fatal(err)
		}
		return path
	}
	derpData, err := json.Marshal(derp)
	if err != nil {
		t.Fatal(err)
	}
	derpServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write(derpData)
	}))
	t.Cleanup(derpServer.Close)
	aclPath := write("acl.json", []byte(`{"acls":[{"action":"accept","src":["*"],"dst":["*:*"]}]}`))
	address := func() string {
		listener, err := net.Listen("tcp4", "127.0.0.1:0")
		if err != nil {
			t.Fatal(err)
		}
		value := listener.Addr().String()
		if err := listener.Close(); err != nil {
			t.Fatal(err)
		}
		return value
	}
	listen, metrics, grpc := address(), address(), address()
	controlURL := "http://" + listen
	config := fmt.Sprintf(`server_url: %s
listen_addr: %s
metrics_listen_addr: %s
grpc_listen_addr: %s
grpc_allow_insecure: false
noise:
  private_key_path: %s
prefixes:
  v4: 100.64.0.0/10
  v6: fd7a:115c:a1e0::/48
  allocation: sequential
derp:
  server:
    enabled: false
  urls: [%s]
  paths: []
  auto_update_enabled: false
disable_check_updates: true
node:
  expiry: 0
  ephemeral:
    inactivity_timeout: 30m
database:
  type: sqlite
  sqlite:
    path: %s
    write_ahead_log: true
log:
  level: warn
  format: text
policy:
  mode: file
  path: %s
dns:
  magic_dns: false
  base_domain: mesh.fixture.invalid
  override_local_dns: false
unix_socket: %s
unix_socket_permission: '0600'
logtail:
  enabled: false
taildrop:
  enabled: false
auto_update:
  enabled: false
`, controlURL, listen, metrics, grpc, filepath.Join(dir, "noise.key"), derpServer.URL, filepath.Join(dir, "db.sqlite"), aclPath, filepath.Join(dir, "control.sock"))
	configPath := write("config.yaml", []byte(config))
	log, err := os.OpenFile(filepath.Join(dir, "headscale.log"), os.O_CREATE|os.O_WRONLY|os.O_EXCL, 0600)
	if err != nil {
		t.Fatal(err)
	}
	process := exec.CommandContext(ctx, binary, "-c", configPath, "serve")
	process.Stdout, process.Stderr = log, log
	if err := process.Start(); err != nil {
		t.Fatal(err)
	}
	ended := make(chan struct{})
	var processError error
	go func() { processError = process.Wait(); close(ended) }()
	t.Cleanup(func() {
		_ = process.Process.Signal(os.Interrupt)
		select {
		case <-ended:
		case <-time.After(5 * time.Second):
			_ = process.Process.Kill()
			<-ended
		}
		_ = log.Close()
	})
	client := &http.Client{Timeout: time.Second}
	deadline := time.Now().Add(20 * time.Second)
	ready := false
	for time.Now().Before(deadline) {
		response, err := client.Get(controlURL + "/health")
		if err == nil {
			_ = response.Body.Close()
			if response.StatusCode == http.StatusOK {
				ready = true
				break
			}
		}
		select {
		case <-ended:
			data, _ := os.ReadFile(filepath.Join(dir, "headscale.log"))
			t.Fatalf("Headscale fixture exited before health: %v; %s", processError, data)
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		case <-time.After(50 * time.Millisecond):
		}
	}
	if !ready {
		t.Fatal("Headscale fixture never became healthy")
	}
	cli := func(args ...string) []byte {
		request, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		command := exec.CommandContext(request, binary, append([]string{"-c", configPath, "--output", "json"}, args...)...)
		data, err := command.Output()
		if err != nil {
			t.Fatalf("Headscale fixture CLI %s failed: %v", args[0], err)
		}
		return data
	}
	var user struct {
		ID uint64 `json:"id"`
	}
	if err := json.Unmarshal(cli("users", "create", "native-fixture"), &user); err != nil {
		t.Fatal(err)
	}
	if user.ID == 0 {
		t.Fatal("Headscale user missing identity")
	}
	key := func(ephemeral bool) string {
		args := []string{"preauthkeys", "create", "--user", fmt.Sprint(user.ID), "--expiration", "1h"}
		if ephemeral {
			args = append(args, "--ephemeral")
		}
		var result struct {
			Key string `json:"key"`
		}
		if err := json.Unmarshal(cli(args...), &result); err != nil {
			t.Fatal(err)
		}
		if len(strings.TrimSpace(result.Key)) < 32 {
			t.Fatal("Headscale returned no enrollment key")
		}
		return result.Key
	}
	ownerKey, controllerKey := key(true), key(false)
	t.Log("isolated Headscale v0.29.3 ready, distinct one-use keys issued")
	return controlURL, ownerKey, controllerKey
}
