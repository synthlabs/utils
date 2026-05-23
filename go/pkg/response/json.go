package response

import (
	"encoding/json"
	"net/http"
)

// taken from https://github.com/tj/go/blob/master/http/response/json.go

// JSON response with optional status code.
func JSON(w http.ResponseWriter, val interface{}, code ...int) {
	var b []byte
	var err error

	if Pretty {
		b, err = json.MarshalIndent(val, "", "  ")
	} else {
		b, err = json.Marshal(val)
	}

	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "application/json")

	if len(code) > 0 {
		w.WriteHeader(code[0])
	}

	_, err = w.Write(b)
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
}
