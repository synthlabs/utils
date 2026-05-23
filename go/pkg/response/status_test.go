package response

import (
	"net/http/httptest"
	"testing"
)

func TestStatusFunctions(t *testing.T) {
	res := httptest.NewRecorder()
	NotFound(res)
	if res.Code != 404 {
		t.Fatalf("status = %d, want 404", res.Code)
	}
	if got, want := res.Body.String(), "Not Found\n"; got != want {
		t.Fatalf("body = %q, want %q", got, want)
	}
	if got := res.Result().Header.Get("Content-Type"); got != "text/plain; charset=utf-8" {
		t.Fatalf("content type = %q, want text/plain; charset=utf-8", got)
	}
}

func TestStatusFunctionsMessage(t *testing.T) {
	res := httptest.NewRecorder()
	NotFound(res, "can't find that")
	if res.Code != 404 {
		t.Fatalf("status = %d, want 404", res.Code)
	}
	if got, want := res.Body.String(), "can't find that\n"; got != want {
		t.Fatalf("body = %q, want %q", got, want)
	}
	if got := res.Result().Header.Get("Content-Type"); got != "text/plain; charset=utf-8" {
		t.Fatalf("content type = %q, want text/plain; charset=utf-8", got)
	}
}

func TestStatusFunctionsJSON(t *testing.T) {
	Pretty = false
	res := httptest.NewRecorder()
	Unauthorized(res, map[string]string{"error": "token_expired", "message": "Token expired!"})
	if res.Code != 401 {
		t.Fatalf("status = %d, want 401", res.Code)
	}
	if got, want := res.Body.String(), `{"error":"token_expired","message":"Token expired!"}`; got != want {
		t.Fatalf("body = %q, want %q", got, want)
	}
	if got := res.Result().Header.Get("Content-Type"); got != "application/json" {
		t.Fatalf("content type = %q, want application/json", got)
	}
}
