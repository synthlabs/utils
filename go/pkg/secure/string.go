package secure

import (
	"crypto/rand"
	"fmt"
	"io"
	"math/big"
)

const APIKeyLen = 32

func init() {
	if err := isAvailablePRNG(); err != nil {
		panic(err)
	}
}

func isAvailablePRNG() error {
	// Assert that a cryptographically secure PRNG is available.
	// Panic otherwise.
	buf := make([]byte, 1)

	_, err := io.ReadFull(rand.Reader, buf)
	if err != nil {
		return fmt.Errorf("crypto/rand is unavailable: Read() failed with %#v", err)
	}
	return nil
}

func GenerateRandomKey(n int) string {
	const letters = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-"
	ret := make([]byte, n)
	for i := 0; i < n; i++ {
		num, _ := rand.Int(rand.Reader, big.NewInt(int64(len(letters))))
		ret[i] = letters[num.Int64()]
	}

	return string(ret)
}
