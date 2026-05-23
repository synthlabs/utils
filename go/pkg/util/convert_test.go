package util

import (
	"testing"
	"time"

	"google.golang.org/protobuf/types/known/durationpb"
)

type sourceConfig struct {
	Name    string `json:"name"`
	Created string `json:"created"`
	Timeout int64  `json:"timeout"`
}

type targetConfig struct {
	Name    string    `json:"name"`
	Created time.Time `json:"created"`
}

type protoTargetConfig struct {
	Name    string               `json:"name"`
	Timeout *durationpb.Duration `json:"timeout"`
}

func TestConvertToType(t *testing.T) {
	to, err := ConvertToType[sourceConfig, targetConfig](sourceConfig{
		Name:    "worker",
		Created: "2026-05-23T17:30:00Z",
	})
	if err != nil {
		t.Fatalf("ConvertToType returned error: %v", err)
	}

	if to.Name != "worker" {
		t.Fatalf("name = %q, want worker", to.Name)
	}
	if got, want := to.Created.Format(time.RFC3339), "2026-05-23T17:30:00Z"; got != want {
		t.Fatalf("created = %s, want %s", got, want)
	}
}

func TestConvertToTypeProto(t *testing.T) {
	to, err := ConvertToTypeProto[sourceConfig, protoTargetConfig](sourceConfig{
		Name:    "worker",
		Timeout: int64(3 * time.Second),
	})
	if err != nil {
		t.Fatalf("ConvertToTypeProto returned error: %v", err)
	}

	if to.Name != "worker" {
		t.Fatalf("name = %q, want worker", to.Name)
	}
	if got, want := to.Timeout.AsDuration(), 3*time.Second; got != want {
		t.Fatalf("timeout = %s, want %s", got, want)
	}
}
