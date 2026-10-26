package meshfixture

import (
	"crypto/subtle"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/coder/websocket"
)

type controller struct {
	DeviceID string `json:"deviceID"`
	Token    string `json:"token"`
}

type commandRequest struct {
	CommandID string `json:"commandID"`
	Kind      string `json:"kind"`
	Payload   struct {
		Text string `json:"text"`
	} `json:"payload"`
	DropAcknowledgement bool `json:"dropAcknowledgement,omitempty"`
}

type commandReceipt struct {
	CommandID     string `json:"commandID"`
	Kind          string `json:"kind"`
	State         string `json:"state"`
	DeviceID      string `json:"deviceID"`
	DeliveryCount int    `json:"deliveryCount"`
	AcceptedAt    string `json:"acceptedAt"`
}

type commandRecord struct {
	Receipt commandReceipt
	Text    string
}

type event struct {
	Type     string       `json:"type"`
	RunID    string       `json:"runID"`
	OwnerID  string       `json:"ownerID"`
	Epoch    string       `json:"epoch"`
	Sequence uint64       `json:"sequence"`
	Payload  eventPayload `json:"payload"`
}

type eventPayload struct {
	Nonce    string          `json:"nonce"`
	DeviceID string          `json:"deviceID"`
	Command  *commandReceipt `json:"command,omitempty"`
}

type deviceEvents struct {
	Events  []event
	Changed chan struct{}
}

type fixture struct {
	RunID, OwnerID, Epoch, Nonce string
	Controllers                  map[string]controller
	OutputDir                    string
	mu                           sync.Mutex
	closing                      bool
	active                       sync.WaitGroup
	commands                     map[string]commandRecord
	deliveryCounts               map[string]int
	streams                      map[string]*deviceEvents
	auditFile                    *os.File
	failed                       chan error
}

func (f *fixture) identify(r *http.Request) string {
	value := r.Header.Get("Authorization")
	for _, c := range f.Controllers {
		if subtle.ConstantTimeCompare([]byte(value), []byte("Bearer "+c.Token)) == 1 {
			return c.DeviceID
		}
	}
	return ""
}

func (f *fixture) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	f.mu.Lock()
	if f.closing {
		f.mu.Unlock()
		http.Error(w, "fixture stopping", http.StatusServiceUnavailable)
		return
	}
	f.active.Add(1)
	f.mu.Unlock()
	defer f.active.Done()
	deviceID := f.identify(r)
	if err := f.audit(map[string]any{
		"type": "request", "method": r.Method, "path": r.URL.Path,
		"tokenValid": deviceID != "", "meshRemoteAddr": r.RemoteAddr, "deviceID": deviceID,
	}); err != nil {
		f.fail(err)
		http.Error(w, "fixture audit unavailable", http.StatusInternalServerError)
		return
	}
	w.Header().Set("Cache-Control", "no-store")
	if deviceID == "" {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}
	switch {
	case r.Method == http.MethodGet && r.URL.Path == "/health":
		f.reply(w, http.StatusOK, map[string]any{
			"type": "health", "runID": f.RunID, "ownerID": f.OwnerID,
			"epoch": f.Epoch, "nonce": f.Nonce, "deviceID": deviceID,
		})
	case r.Method == http.MethodGet && r.URL.Path == "/events":
		f.events(w, r, deviceID)
	case r.Method == http.MethodPost && r.URL.Path == "/commands":
		f.submit(w, r, deviceID)
	case r.Method == http.MethodGet && strings.HasPrefix(r.URL.Path, "/commands/"):
		f.lookup(w, r, deviceID)
	default:
		http.NotFound(w, r)
	}
}

func (f *fixture) beginShutdown() {
	f.mu.Lock()
	f.closing = true
	f.mu.Unlock()
}

var commandIDPattern = regexp.MustCompile(`^[a-zA-Z0-9-]{1,128}$`)

func (f *fixture) submit(w http.ResponseWriter, r *http.Request, deviceID string) {
	var request commandRequest
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil {
		http.Error(w, "invalid fixture command", http.StatusBadRequest)
		return
	}
	if err := decoder.Decode(new(any)); !errors.Is(err, io.EOF) ||
		!commandIDPattern.MatchString(request.CommandID) || request.Kind != "fixture-command" ||
		request.Payload.Text == "" || len(request.Payload.Text) > 1024 {
		http.Error(w, "invalid fixture command", http.StatusBadRequest)
		return
	}
	f.mu.Lock()
	record, duplicate := f.commands[request.CommandID]
	if duplicate && (record.Receipt.DeviceID != deviceID || record.Receipt.Kind != request.Kind || record.Text != request.Payload.Text) {
		f.mu.Unlock()
		http.Error(w, "command ID conflict", http.StatusConflict)
		return
	}
	if !duplicate {
		count, err := f.deliverLocked(request.CommandID, deviceID)
		if err != nil {
			f.mu.Unlock()
			f.fail(err)
			http.Error(w, "fixture delivery audit unavailable", http.StatusInternalServerError)
			return
		}
		record = commandRecord{Receipt: commandReceipt{
			CommandID: request.CommandID, Kind: request.Kind, State: "accepted",
			DeviceID: deviceID, DeliveryCount: count, AcceptedAt: time.Now().UTC().Format(time.RFC3339Nano),
		}, Text: request.Payload.Text}
		f.commands[request.CommandID] = record
		f.appendEventLocked(deviceID, "command_accepted", &record.Receipt)
	}
	counts := make([]commandReceipt, 0, len(f.commands))
	for _, cmd := range f.commands {
		counts = append(counts, cmd.Receipt)
	}
	err := writeJSON(filepath.Join(f.OutputDir, "command-receipts.json"), counts)
	f.mu.Unlock()
	if err != nil {
		f.fail(err)
		http.Error(w, "fixture receipt unavailable", http.StatusInternalServerError)
		return
	}
	drop := request.DropAcknowledgement && !duplicate
	if err := f.audit(map[string]any{
		"type": "command", "commandID": request.CommandID, "deviceID": deviceID,
		"deliveryCount": record.Receipt.DeliveryCount, "duplicate": duplicate,
		"simulatedAcknowledgementLoss": drop,
	}); err != nil {
		f.fail(err)
		http.Error(w, "fixture audit unavailable", http.StatusInternalServerError)
		return
	}
	if drop {
		conn, _, err := http.NewResponseController(w).Hijack()
		if err != nil {
			f.fail(err)
			http.Error(w, "fixture interruption unavailable", http.StatusInternalServerError)
			return
		}
		_ = conn.Close()
		return
	}
	status := http.StatusAccepted
	if duplicate {
		status = http.StatusOK
	}
	f.commandReply(w, status, record.Receipt, duplicate)
}

// The diagnostic sink records each invocation independently of the receipt cache.
func (f *fixture) deliverLocked(commandID, deviceID string) (int, error) {
	f.deliveryCounts[commandID]++
	count := f.deliveryCounts[commandID]
	err := f.auditLocked(map[string]any{
		"type": "fixture-delivery", "commandID": commandID, "deviceID": deviceID, "deliveryCount": count,
	})
	return count, err
}

func (f *fixture) lookup(w http.ResponseWriter, r *http.Request, deviceID string) {
	f.mu.Lock()
	record, exists := f.commands[strings.TrimPrefix(r.URL.Path, "/commands/")]
	f.mu.Unlock()
	if !exists || record.Receipt.DeviceID != deviceID {
		http.NotFound(w, r)
		return
	}
	f.commandReply(w, http.StatusOK, record.Receipt, false)
}

func (f *fixture) commandReply(w http.ResponseWriter, status int, receipt commandReceipt, duplicate bool) {
	f.reply(w, status, map[string]any{
		"type": "command", "runID": f.RunID, "ownerID": f.OwnerID, "epoch": f.Epoch,
		"nonce": f.Nonce, "command": receipt, "duplicate": duplicate,
	})
}

func (f *fixture) appendEventLocked(deviceID, kind string, command *commandReceipt) {
	stream := f.streams[deviceID]
	stream.Events = append(stream.Events, event{
		Type: kind, RunID: f.RunID, OwnerID: f.OwnerID, Epoch: f.Epoch,
		Sequence: uint64(len(stream.Events)) + 1,
		Payload:  eventPayload{Nonce: f.Nonce, DeviceID: deviceID, Command: command},
	})
	close(stream.Changed)
	stream.Changed = make(chan struct{})
}

func (f *fixture) events(w http.ResponseWriter, r *http.Request, deviceID string) {
	var after uint64
	values, hasCursor := r.URL.Query()["after"]
	if hasCursor {
		if len(values) != 1 || values[0] == "" {
			http.Error(w, "invalid cursor", http.StatusBadRequest)
			return
		}
		var err error
		after, err = strconv.ParseUint(values[0], 10, 64)
		if err != nil {
			http.Error(w, "invalid cursor", http.StatusBadRequest)
			return
		}
	}
	f.mu.Lock()
	if after > uint64(len(f.streams[deviceID].Events)) {
		f.mu.Unlock()
		http.Error(w, "cursor exceeds fixture stream", http.StatusConflict)
		return
	}
	f.mu.Unlock()
	conn, err := websocket.Accept(w, r, nil)
	if err != nil {
		return
	}
	defer conn.CloseNow()
	ctx := conn.CloseRead(r.Context())
	f.mu.Lock()
	if !hasCursor {
		after = uint64(len(f.streams[deviceID].Events))
	}
	f.appendEventLocked(deviceID, "connected", nil)
	f.mu.Unlock()
	for {
		f.mu.Lock()
		stream := f.streams[deviceID]
		pending := append([]event(nil), stream.Events[after:]...)
		changed := stream.Changed
		f.mu.Unlock()
		for _, item := range pending {
			data, err := json.Marshal(item)
			if err != nil {
				f.fail(err)
				return
			}
			if err := conn.Write(ctx, websocket.MessageText, data); err != nil {
				return
			}
			after = item.Sequence
		}
		select {
		case <-ctx.Done():
			return
		case <-changed:
		}
	}
}

func (f *fixture) reply(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if err := json.NewEncoder(w).Encode(value); err != nil {
		// A lost response is recoverable through the command ID, not a failed acceptance.
		return
	}
}

func (f *fixture) audit(value map[string]any) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.auditLocked(value)
}

func (f *fixture) auditLocked(value map[string]any) error {
	value["at"] = time.Now().UTC().Format(time.RFC3339Nano)
	return json.NewEncoder(f.auditFile).Encode(value)
}

func (f *fixture) fail(err error) {
	select {
	case f.failed <- err:
	default:
	}
}

func writeJSON(path string, value any) error {
	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(path+".tmp", append(data, '\n'), 0600); err != nil {
		return err
	}
	return os.Rename(path+".tmp", path)
}
