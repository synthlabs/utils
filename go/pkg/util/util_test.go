package util

import (
	"bytes"
	"errors"
	"io"
	"net/http"
	"strings"
	"sync"
	"testing"

	logpkg "github.com/synthlabs/utils/go/pkg/log"
)

func TestMapAndAsyncMapReturnValues(t *testing.T) {
	input := []int{1, 2, 3}
	want := []int{2, 4, 6}

	for name, fn := range map[string]func([]int, func(int) (int, error)) ([]int, error){
		"Map":      Map[int, int],
		"AsyncMap": AsyncMap[int, int],
	} {
		t.Run(name, func(t *testing.T) {
			got, err := fn(input, func(v int) (int, error) {
				return v * 2, nil
			})
			if err != nil {
				t.Fatalf("%s returned error: %v", name, err)
			}
			for i := range want {
				if got[i] != want[i] {
					t.Fatalf("%s[%d] = %d, want %d", name, i, got[i], want[i])
				}
			}
		})
	}
}

func TestMapJoinsErrors(t *testing.T) {
	sentinel := errors.New("sentinel")
	_, err := Map([]int{1}, func(int) (int, error) {
		return 0, sentinel
	})
	if !errors.Is(err, sentinel) {
		t.Fatalf("err = %v, want sentinel", err)
	}
}

func TestUnmarshalReaderToStructClosesInput(t *testing.T) {
	reader := &closeTrackingReader{Reader: strings.NewReader(`{"name":"test"}`)}
	var out struct {
		Name string `json:"name"`
	}

	if err := UnmarshalReaderToStruct(reader, &out); err != nil {
		t.Fatalf("unmarshal reader: %v", err)
	}
	if out.Name != "test" {
		t.Fatalf("name = %q, want test", out.Name)
	}
	if !reader.closed {
		t.Fatal("reader was not closed")
	}
}

func TestUnmarshalResponseToStructClosesBody(t *testing.T) {
	body := &closeTrackingReader{Reader: strings.NewReader(`{"name":"test"}`)}
	resp := &http.Response{Body: body}
	var out struct {
		Name string `json:"name"`
	}

	if err := UnmarshalResponseToStruct(resp, &out); err != nil {
		t.Fatalf("unmarshal response: %v", err)
	}
	if out.Name != "test" {
		t.Fatalf("name = %q, want test", out.Name)
	}
	if !body.closed {
		t.Fatal("body was not closed")
	}
}

func TestRecoverRunsCallbacks(t *testing.T) {
	var out bytes.Buffer
	logger, err := logpkg.NewWithOptions(logpkg.Options{Writer: &out})
	if err != nil {
		t.Fatalf("new logger: %v", err)
	}

	called := false
	func() {
		defer Recover(logger, func(any) {
			called = true
		})
		panic("boom")
	}()

	if !called {
		t.Fatal("recover callback was not called")
	}
	if !strings.Contains(out.String(), "recovered panic") {
		t.Fatalf("logs = %q, want recovered panic", out.String())
	}
}

func TestRunEConvertsPanicToError(t *testing.T) {
	logger, err := logpkg.NewWithOptions(logpkg.Options{Writer: io.Discard})
	if err != nil {
		t.Fatalf("new logger: %v", err)
	}

	var wg sync.WaitGroup
	err = RunE(&wg, logger, func() error {
		panic("boom")
	})
	wg.Wait()
	if err == nil {
		t.Fatal("RunE err = nil, want panic error")
	}
}

type closeTrackingReader struct {
	io.Reader
	closed bool
}

func (r *closeTrackingReader) Close() error {
	r.closed = true
	return nil
}
