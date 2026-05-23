package response

import (
	"net/http/httptest"
	"testing"
)

type User struct {
	First string `json:"first"`
	Last  string `json:"last"`
}

func TestJSONPretty(t *testing.T) {
	Pretty = true
	res := httptest.NewRecorder()
	JSON(res, &User{"Tobi", "Ferret"})
	if res.Code != 200 {
		t.Fatalf("status = %d, want 200", res.Code)
	}
	if got, want := res.Body.String(), "{\n  \"first\": \"Tobi\",\n  \"last\": \"Ferret\"\n}"; got != want {
		t.Fatalf("body = %q, want %q", got, want)
	}
	if got := res.Result().Header.Get("Content-Type"); got != "application/json" {
		t.Fatalf("content type = %q, want application/json", got)
	}
}

func TestJSON(t *testing.T) {
	Pretty = false
	res := httptest.NewRecorder()
	JSON(res, &User{"Tobi", "Ferret"})
	if res.Code != 200 {
		t.Fatalf("status = %d, want 200", res.Code)
	}
	if got, want := res.Body.String(), `{"first":"Tobi","last":"Ferret"}`; got != want {
		t.Fatalf("body = %q, want %q", got, want)
	}
	if got := res.Result().Header.Get("Content-Type"); got != "application/json" {
		t.Fatalf("content type = %q, want application/json", got)
	}
}
