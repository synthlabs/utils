package util

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/andybalholm/brotli"
)

func TestDoHTTPRequestDecodesPlainAndBrotliJSON(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/plain":
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"message":"plain"}`))
		case "/br":
			var buf bytes.Buffer
			bw := brotli.NewWriter(&buf)
			if _, err := bw.Write([]byte(`{"message":"brotli"}`)); err != nil {
				t.Errorf("write brotli payload: %v", err)
				return
			}
			if err := bw.Close(); err != nil {
				t.Errorf("close brotli writer: %v", err)
				return
			}
			w.Header().Set("Content-Type", "application/json")
			w.Header().Set("Content-Encoding", "br")
			_, _ = w.Write(buf.Bytes())
		default:
			http.NotFound(w, r)
		}
	}))
	defer srv.Close()

	for _, tc := range []struct {
		path string
		want string
	}{
		{path: "/plain", want: "plain"},
		{path: "/br", want: "brotli"},
	} {
		t.Run(tc.want, func(t *testing.T) {
			req, err := NewHTTPRequest(http.MethodGet, srv.URL+tc.path, "cid", "tok", nil)
			if err != nil {
				t.Fatalf("new request: %v", err)
			}
			var out struct {
				Message string `json:"message"`
			}
			if err := DoHTTPRequest(req, &out); err != nil {
				t.Fatalf("DoHTTPRequest(%s): %v", tc.path, err)
			}
			if out.Message != tc.want {
				t.Fatalf("message = %q, want %q", out.Message, tc.want)
			}
		})
	}
}
