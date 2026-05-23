package util

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sync"

	logpkg "github.com/synthlabs/utils/go/pkg/log"

	"github.com/go-errors/errors"
)

// Hash a string
func Hash(in string) string {
	h := sha256.New()
	_, _ = h.Write([]byte(in))
	bs := h.Sum(nil)

	return fmt.Sprintf("%x", bs)
}

func Must[T any](obj T, err error) T {
	if err != nil {
		panic(fmt.Sprintf("[MUST] returned err: %s", err))
	}
	return obj
}

func Map[T any, O any](from []T, mapFn func(T) (O, error)) ([]O, error) {
	resultValues := make([]O, len(from))
	resultErrs := make([]error, len(from))
	for i, val := range from {
		res, err := mapFn(val)
		resultValues[i] = res
		resultErrs[i] = err
	}
	return resultValues, errors.Join(resultErrs...)
}

func AsyncMap[T any, O any](from []T, mapFn func(T) (O, error)) ([]O, error) {
	resultValues := make([]O, len(from))
	resultErrs := make([]error, len(from))

	var wg sync.WaitGroup
	for index, val := range from {
		wg.Add(1)
		go func(i int, v T) {
			defer wg.Done()

			res, err := mapFn(v)
			resultValues[i] = res
			resultErrs[i] = err
		}(index, val)
	}

	wg.Wait()
	return resultValues, errors.Join(resultErrs...)
}

// UnmarshalResponseToStruct is a simple helper to reduce response unmarshalling code everywhere
// Calls resp.Close for you
func UnmarshalResponseToStruct(resp *http.Response, s any) error {
	defer resp.Body.Close()

	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return err
	}

	return json.Unmarshal(data, s)
}

// UnmarshalReaderToStruct is a simple helper to reduce unmarshalling code everywhere
// Calls input.Close for you
func UnmarshalReaderToStruct(input io.ReadCloser, s any) error {
	defer input.Close()

	data, err := io.ReadAll(input)
	if err != nil {
		return err
	}

	return json.Unmarshal(data, s)
}

type RecoverCallback func(any)

func Recover(logger logpkg.Logger, cbs ...RecoverCallback) bool {
	return recoverPanic(logger, recover(), cbs...)
}

func recoverPanic(logger logpkg.Logger, r any, cbs ...RecoverCallback) bool {
	if r == nil {
		return false
	}

	logger.Error("recovered panic", "r", r, "stack", errors.New("stack trace").Stack())
	for _, fn := range cbs {
		fn(r)
	}
	return true
}

type (
	RunFunc  func()
	RunEFunc func() error
)

// RunE is an extended function executor that increments and calls done on the
// waitgroup, includes a panic recover handler, and returns an error.
func RunE(wg *sync.WaitGroup, log logpkg.Logger, fn RunEFunc) (err error) {
	wg.Add(1)
	defer func() {
		if recoverPanic(log, recover()) {
			err = errors.New("function panicked")
		}
	}()
	defer wg.Done()

	// run the given function
	return fn()
}

// Run is a function executor that increments and calls done on the
// waitgroup and includes a panic recover handler
func Run(wg *sync.WaitGroup, log logpkg.Logger, fn RunFunc) {
	wg.Add(1)
	defer Recover(log)
	defer wg.Done()

	// run the given function
	fn()
}
