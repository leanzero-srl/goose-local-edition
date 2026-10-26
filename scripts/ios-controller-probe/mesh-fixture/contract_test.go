package meshfixture

import (
	"bytes"
	"context"
	"crypto/x509"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"
	"tailscale.com/tailcfg"
)

func TestFixtureCommandsAndReplay(t *testing.T) {
	f := newFixture(t, t.TempDir())
	server := httptest.NewServer(f)
	t.Cleanup(server.Close)
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	phone := f.Controllers["phone"]
	ipad := f.Controllers["ipad"]
	request := func(method, path, token, body string) (int, []byte, error) {
		t.Helper()
		req, err := http.NewRequestWithContext(ctx, method, server.URL+path, strings.NewReader(body))
		if err != nil {
			t.Fatal(err)
		}
		req.Header.Set("Authorization", "Bearer "+token)
		res, err := server.Client().Do(req)
		if err != nil {
			return 0, nil, err
		}
		defer res.Body.Close()
		data, err := io.ReadAll(res.Body)
		return res.StatusCode, data, err
	}
	connect := func(path, token string) *websocket.Conn {
		t.Helper()
		conn, _, err := websocket.Dial(ctx, "ws"+strings.TrimPrefix(server.URL, "http")+path,
			&websocket.DialOptions{HTTPHeader: http.Header{"Authorization": {"Bearer " + token}}})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = conn.CloseNow() })
		return conn
	}
	readEvent := func(conn *websocket.Conn) event {
		t.Helper()
		kind, data, err := conn.Read(ctx)
		if err != nil || kind != websocket.MessageText {
			t.Fatalf("event read: kind=%v err=%v", kind, err)
		}
		var e event
		if err := json.Unmarshal(data, &e); err != nil {
			t.Fatal(err)
		}
		if e.RunID != f.RunID || e.OwnerID != f.OwnerID || e.Epoch != f.Epoch || e.Payload.Nonce != f.Nonce {
			t.Fatal("event identity mismatch")
		}
		return e
	}
	for _, path := range []string{"/health", "/commands/command-1", "/events"} {
		code, _, err := request("GET", path, "wrong-token", "")
		if err != nil || code != http.StatusUnauthorized {
			t.Fatalf("wrong token %s: status=%d err=%v", path, code, err)
		}
	}
	code, health, err := request("GET", "/health", phone.Token, "")
	if err != nil || code != http.StatusOK || !bytes.Contains(health, []byte(f.Nonce)) || !bytes.Contains(health, []byte(phone.DeviceID)) {
		t.Fatalf("health: status=%d err=%v", code, err)
	}
	first := connect("/events", phone.Token)
	connected := readEvent(first)
	if connected.Type != "connected" || connected.Sequence != 1 || connected.Payload.DeviceID != phone.DeviceID {
		t.Fatal("first connection envelope mismatch")
	}
	_ = first.CloseNow()
	second := connect("/events", phone.Token)
	fresh := readEvent(second)
	if fresh.Type != "connected" || fresh.Sequence != connected.Sequence+1 {
		t.Fatalf("new connection without cursor replayed history: first=%d second=%d type=%s", connected.Sequence, fresh.Sequence, fresh.Type)
	}
	_ = second.CloseNow()
	for _, path := range []string{"/events?after=", "/events?after=1&after=2", "/events?after=invalid"} {
		code, _, err := request("GET", path, phone.Token, "")
		if err != nil || code != http.StatusBadRequest {
			t.Fatalf("invalid cursor %s: status=%d err=%v", path, code, err)
		}
	}
	body := `{"commandID":"command-1","kind":"fixture-command","payload":{"text":"fixture only"},"dropAcknowledgement":true}`
	code, _, err = request("POST", "/commands", "wrong-token", body)
	if err != nil || code != http.StatusUnauthorized {
		t.Fatalf("unauthorized command: status=%d err=%v", code, err)
	}
	code, _, err = request("POST", "/commands", phone.Token, body)
	if err == nil {
		t.Fatalf("lost ACK unexpectedly received HTTP status %d", code)
	}
	code, data, err := request("POST", "/commands", phone.Token, body)
	if err != nil || code != http.StatusOK {
		t.Fatalf("retry: status=%d err=%v", code, err)
	}
	var result struct {
		Command   commandReceipt `json:"command"`
		Duplicate bool           `json:"duplicate"`
	}
	if err := json.Unmarshal(data, &result); err != nil {
		t.Fatal(err)
	}
	if !result.Duplicate || result.Command.DeviceID != phone.DeviceID || result.Command.DeliveryCount != 1 {
		t.Fatalf("wrong retry receipt: %+v", result)
	}
	code, _, err = request("POST", "/commands", ipad.Token, body)
	if err != nil || code != http.StatusConflict {
		t.Fatalf("cross-device duplicate: status=%d err=%v", code, err)
	}
	code, _, err = request("POST", "/commands", phone.Token, strings.ReplaceAll(body, "fixture only", "changed"))
	if err != nil || code != http.StatusConflict {
		t.Fatalf("payload conflict: status=%d err=%v", code, err)
	}
	code, _, err = request("GET", "/commands/command-1", ipad.Token, "")
	if err != nil || code != http.StatusNotFound {
		t.Fatalf("cross-device status: status=%d err=%v", code, err)
	}
	code, _, err = request("GET", "/commands/command-1", phone.Token, "")
	if err != nil || code != http.StatusOK {
		t.Fatalf("own status: status=%d err=%v", code, err)
	}
	reconnected := connect("/events?after="+strconv.FormatUint(fresh.Sequence, 10), phone.Token)
	accepted := readEvent(reconnected)
	if accepted.Type != "command_accepted" || accepted.Sequence != fresh.Sequence+1 || accepted.Payload.Command == nil || accepted.Payload.Command.DeliveryCount != 1 {
		t.Fatal("lost-ACK command was not replayed exactly once")
	}
	if e := readEvent(reconnected); e.Type != "connected" || e.Sequence != accepted.Sequence+1 {
		t.Fatal("reconnect sequence mismatch")
	}
	fromStart := connect("/events?after=0", phone.Token)
	if e := readEvent(fromStart); e.Type != "connected" || e.Sequence != connected.Sequence {
		t.Fatal("explicit zero cursor no longer replays the first event")
	}
	_ = fromStart.CloseNow()
	code, _, err = request("GET", "/events?after=999", phone.Token, "")
	if err != nil || code != http.StatusConflict {
		t.Fatalf("future cursor: status=%d err=%v", code, err)
	}
	for _, invalid := range []string{
		`{"commandID":"command-2","kind":"fixture-command","payload":{"text":"x"},"deviceID":"spoofed"}`,
		`{"commandID":"command-2","kind":"run-shell","payload":{"text":"x"}}`,
	} {
		code, _, err = request("POST", "/commands", phone.Token, invalid)
		if err != nil || code != http.StatusBadRequest {
			t.Fatalf("invalid command accepted: status=%d err=%v", code, err)
		}
	}
	code, _, err = request("POST", "/commands", ipad.Token,
		`{"commandID":"ipad-command","kind":"fixture-command","payload":{"text":"ipad fixture"}}`)
	if err != nil || code != http.StatusAccepted {
		t.Fatalf("iPad own command: status=%d err=%v", code, err)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.commands) != 2 || f.commands["command-1"].Receipt.DeliveryCount != 1 ||
		f.commands["ipad-command"].Receipt.DeviceID != ipad.DeviceID ||
		len(f.deliveryCounts) != 2 || f.deliveryCounts["command-1"] != 1 || f.deliveryCounts["ipad-command"] != 1 {
		t.Fatal("duplicate or unauthorized command reached fixture delivery")
	}
}

func TestFixtureCertificateAndDERPGuard(t *testing.T) {
	if validateLocalDERP(nil) == nil {
		t.Fatal("nil DERP map accepted")
	}
	local := &tailcfg.DERPMap{Regions: map[tailcfg.DERPRegionID]*tailcfg.DERPRegion{
		1: {Nodes: []*tailcfg.DERPNode{{HostName: "127.0.0.1", IPv4: "127.0.0.1", IPv6: "none", STUNTestIP: "127.0.0.1", DERPPort: 1234, STUNPort: 1235}}},
	}}
	if err := validateLocalDERP(local); err != nil {
		t.Fatal(err)
	}
	local.Regions[1].Nodes[0].IPv4 = "192.0.2.1"
	if validateLocalDERP(local) == nil {
		t.Fatal("non-loopback DERP accepted")
	}
	certificate, caPEM := fixtureCertificate(t, []netip.Addr{netip.MustParseAddr("100.64.0.1")})
	leaf, err := x509.ParseCertificate(certificate.Certificate[0])
	if err != nil {
		t.Fatal(err)
	}
	roots := x509.NewCertPool()
	if !roots.AppendCertsFromPEM(caPEM) {
		t.Fatal("fixture CA did not parse")
	}
	options := x509.VerifyOptions{Roots: roots, DNSName: "100.64.0.1"}
	if _, err := leaf.Verify(options); err != nil {
		t.Fatal(err)
	}
	options.DNSName = "100.64.0.2"
	if _, err := leaf.Verify(options); err == nil {
		t.Fatal("wrong IP accepted by fixture certificate")
	}
	options.DNSName = "100.64.0.1"
	options.Roots = x509.NewCertPool()
	if _, err := leaf.Verify(options); err == nil {
		t.Fatal("untrusted fixture certificate accepted")
	}
}
