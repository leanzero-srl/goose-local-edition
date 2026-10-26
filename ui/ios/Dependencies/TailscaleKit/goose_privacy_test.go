package main

import (
	"context"
	"io"
	"net/http"
	"os"
	"os/exec"
	"strings"
	"sync/atomic"
	"testing"

	"tailscale.com/envknob"
	"tailscale.com/logtail"
	"tailscale.com/types/logger"
)

type privacyCapture struct{ calls atomic.Int32 }

func (p *privacyCapture) RoundTrip(r *http.Request) (*http.Response, error) {
	p.calls.Add(1)
	return &http.Response{StatusCode: 200, Header: http.Header{}, Body: io.NopCloser(strings.NewReader("")), Request: r}, nil
}

type privacyBuffer struct {
	logtail.Buffer
	writes atomic.Int32
}

func (b *privacyBuffer) Write(p []byte) (int, error) {
	b.writes.Add(1)
	return b.Buffer.Write(p)
}

func TestGooseDiagnosticUploadPolicy(t *testing.T) {
	mode := os.Getenv("GOOSE_PRIVACY_PROOF_MODE")
	if mode == "" {
		for _, variant := range []string{"control", "protected"} {
			cmd := exec.Command(os.Args[0], "-test.run=^TestGooseDiagnosticUploadPolicy$", "-test.v")
			cmd.Env = append(os.Environ(), "GOOSE_PRIVACY_PROOF_MODE="+variant, "TS_NO_LOGS_NO_SUPPORT=false", "TS_DEBUG_LOGTAIL=true")
			output, err := cmd.CombinedOutput()
			t.Log(strings.TrimSpace(string(output)))
			if err != nil {
				t.Fatalf("%s proof: %v", variant, err)
			}
		}
		return
	}
	backing := logtail.NewMemoryBuffer(16)
	if _, err := backing.Write([]byte("{\"text\":\"previously buffered diagnostic\"}\n")); err != nil {
		t.Fatal(err)
	}
	if mode == "protected" {
		// Exercise the same C-exported constructor Swift calls before start/up.
		node := TsnetNewServer()
		if node <= 0 || TsnetClose(node) != 0 {
			t.Fatal("node constructor/close failed")
		}
		if !envknob.NoLogsNoSupport() {
			t.Fatal("constructor did not set the Go-visible privacy policy")
		}
	}
	transport := &privacyCapture{}
	buffer := &privacyBuffer{Buffer: backing}
	log := logtail.NewLogger(logtail.Config{Collection: "privacy-proof", BaseURL: "https://capture.invalid",
		HTTPC: &http.Client{Transport: transport}, Buffer: buffer, Stderr: io.Discard}, logger.Discard)
	log.Logf("new diagnostic that must not leave the protected process")
	if err := log.Shutdown(context.Background()); err != nil {
		t.Fatal(err)
	}
	calls, writes := transport.calls.Load(), buffer.writes.Load()
	t.Logf("mode=%s upload_attempts=%d new_buffer_writes=%d", mode, calls, writes)
	if mode == "protected" && (calls != 0 || writes != 0) {
		t.Fatal("diagnostics reached upload transport or persistent buffer")
	}
	if mode == "control" && (calls == 0 || writes == 0) {
		t.Fatal("negative control failed to exercise both paths")
	}
}
