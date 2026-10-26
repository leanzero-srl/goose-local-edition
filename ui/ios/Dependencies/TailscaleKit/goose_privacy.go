package main

import (
	"tailscale.com/envknob"
	"tailscale.com/logtail"
)

func disableDiagnosticUploads() {
	// Go caches its environment before Swift runs. Set the supported Go knob
	// before tsnet starts, and close both fresh and previously buffered uploads.
	envknob.SetNoLogsNoSupport()
	logtail.Disable()
}
