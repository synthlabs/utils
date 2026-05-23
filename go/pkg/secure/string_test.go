package secure

import "testing"

func TestGenerateRandomKey(t *testing.T) {
	key := GenerateRandomKey(64)
	if len(key) != 64 {
		t.Fatalf("len = %d, want 64", len(key))
	}

	for _, r := range key {
		if (r >= '0' && r <= '9') || (r >= 'A' && r <= 'Z') || (r >= 'a' && r <= 'z') || r == '-' {
			continue
		}
		t.Fatalf("key contains invalid character %q", r)
	}
}
