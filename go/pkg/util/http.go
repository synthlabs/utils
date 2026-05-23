// Random collection of helpers and functions
// that don't have a home anywhere else. As things
// grow and get bigger I move em into dedicated pkgs.
package util

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"sync"
	"time"

	logpkg "github.com/synthlabs/utils/go/pkg/log"

	"github.com/google/brotli/go/cbrotli"
)

// NewHTTPServer constructs a HTTP server with provided config
func NewHTTPServer(port string, handle http.Handler) *http.Server {
	return &http.Server{
		Addr:    fmt.Sprintf(":%s", port),
		Handler: handle,
	}
}

func ShutdownServer(srv *http.Server) {
	// use a fresh ctx so we can wait 30s to drain connections
	ctx, cancel := context.WithTimeout(context.Background(), time.Second*30)
	defer cancel()

	_ = srv.Shutdown(ctx)
}

func RunServer(name string, srv *http.Server, cancel context.CancelFunc, log logpkg.Logger, wg *sync.WaitGroup) {
	wg.Add(1)
	defer cancel()
	defer wg.Done()

	log = log.Named(name)
	log.Debug("starting")
	if err := srv.ListenAndServe(); err != nil {
		if !errors.Is(err, http.ErrServerClosed) {
			log.Error("server exited", "err", err)
		} else {
			log.Info("server exited")
		}
	}
}

func NewHTTPRequest(method string, url string, clientID string, token string, body io.Reader) (*http.Request, error) {
	req, err := http.NewRequest(method, url, body)
	if err != nil {
		return nil, err
	}

	req.Header.Add("Client-ID", clientID)
	req.Header.Add("Authorization", "Bearer "+token)

	return req, nil
}

func DoHTTPRequest(req *http.Request, result interface{}) error {
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return fmt.Errorf("client.Do: %s", err)
	}
	defer resp.Body.Close()

	var body io.Reader
	body = resp.Body

	switch resp.Header.Get("Content-Encoding") {
	case "br":
		// do br decoding
		brReader := cbrotli.NewReader(resp.Body)
		defer brReader.Close()
		body = brReader
	}

	if resp.StatusCode != http.StatusOK {
		log.Printf("path: %s", req.URL.String())
		log.Printf("response was not 200: %#v", resp)
		_, _ = io.Copy(os.Stdout, resp.Body)
	}

	return json.NewDecoder(body).Decode(result)
}
