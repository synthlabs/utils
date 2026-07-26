package build

import (
	"fmt"
	"net/http"
	"runtime"

	logpkg "github.com/synthlabs/utils/go/pkg/log"
)

// These values are overridden by LDflags during build time
var (
	SHA       = "unknown"
	Branch    = "unknown"
	Version   = "unknown"
	BuildDate = "unknown"
	GoVersion = runtime.Version()
)

func Log(logger logpkg.Logger) {
	logger.Info("build info",
		"sha", SHA,
		"branch", Branch,
		"version", Version,
		"build_date", BuildDate,
		"go_version", GoVersion,
	)
}

func Fields() []any {
	fields := []any{
		"sha", SHA,
		"branch", Branch,
		"version", Version,
		"build_date", BuildDate,
		"go_version", GoVersion,
	}
	return fields
}

func Handler() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprintf(w, "SHA:\t\t%s\n", SHA)
		fmt.Fprintf(w, "Branch:\t\t%s\n", Branch)
		fmt.Fprintf(w, "Build Date:\t%s\n", BuildDate)
		fmt.Fprintf(w, "Go Version:\t%s\n", GoVersion)
	}
}
