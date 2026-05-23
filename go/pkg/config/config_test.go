package config

import (
	"bytes"
	"strings"
	"testing"

	logpkg "github.com/synthlabs/utils/go/pkg/log"
)

func TestDumpMasksSecrets(t *testing.T) {
	var out bytes.Buffer
	logger, err := logpkg.NewWithOptions(logpkg.Options{Writer: &out})
	if err != nil {
		t.Fatalf("new logger: %v", err)
	}

	Dump(struct {
		Username    string
		APIToken    string
		Password    string
		ShowSecrets bool
	}{
		Username:    "jerod",
		APIToken:    "token-value",
		Password:    "password-value",
		ShowSecrets: true,
	}, logger)

	logs := out.String()
	for _, leaked := range []string{"token-value", "password-value"} {
		if strings.Contains(logs, leaked) {
			t.Fatalf("logs leaked %q: %s", leaked, logs)
		}
	}
	if !strings.Contains(logs, "Username=jerod") {
		t.Fatalf("logs = %q, want Username field", logs)
	}
	if !strings.Contains(logs, "ShowSecrets=true") {
		t.Fatalf("logs = %q, want ShowSecrets field", logs)
	}
}
