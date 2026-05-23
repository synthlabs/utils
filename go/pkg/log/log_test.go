package log

import (
	"bytes"
	"strings"
	"testing"
)

func TestNewWithOptionsUsesWriterAndNamespace(t *testing.T) {
	var out bytes.Buffer
	logger, err := NewWithOptions(Options{
		Level:     "debug",
		Namespace: "app",
		Writer:    &out,
	})
	if err != nil {
		t.Fatalf("new logger: %v", err)
	}

	logger.Named("worker").Debug("started", "id", 1)

	logs := out.String()
	for _, want := range []string{"app/worker", "started", "id=1"} {
		if !strings.Contains(logs, want) {
			t.Fatalf("logs = %q, want %q", logs, want)
		}
	}
}

func TestSecretFields(t *testing.T) {
	logger := New("info", "", false, false)

	field := logger.SecretField("token", "secret")
	if got, want := field[1], "*****"; got != want {
		t.Fatalf("secret field = %q, want %q", got, want)
	}
	if got, want := logger.SecretString("secret"), "*****"; got != want {
		t.Fatalf("secret string = %q, want %q", got, want)
	}
}
