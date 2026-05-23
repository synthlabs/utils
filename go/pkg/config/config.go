package config

import (
	"reflect"
	"strings"

	logpkg "github.com/synthlabs/utils/go/pkg/log"

	"github.com/kelseyhightower/envconfig"
)

type Config struct {
	LogLevel    string `envconfig:"LOG_LEVEL" default:"info"`
	Port        string `envconfig:"PORT" default:"8080"`
	ShowSecrets bool   `envconfig:"SHOW_SECRETS" default:"false"`
	JSON        bool   `envconfig:"LOG_JSON" default:"false"`
}

// MustNew builds a new config object
func MustNew() Config {
	var cfg Config
	envconfig.MustProcess("", &cfg)

	return cfg
}

// New builds a new config object
func New() (Config, error) {
	var cfg Config

	err := envconfig.Process("", &cfg)
	return cfg, err
}

// Dump logs all the configured options
func Dump(config any, l logpkg.Logger) {
	s := reflect.Indirect(reflect.ValueOf(config))
	fields := []any{}

	for i := 0; i < s.Type().NumField(); i++ {
		// TODO: recurse embedded configs.
		name := s.Type().Field(i).Name
		if possiblyASecret(name) {
			fields = append(fields, name, "*****")
		} else {
			fields = append(fields, name, s.Field(i).Interface())
		}
	}

	l.Info("configuration", fields...)
}

func possiblyASecret(name string) bool {
	lowerName := strings.ToLower(name)

	switch {
	case name == "ShowSecrets":
		return false
	case strings.Contains(lowerName, "secret"):
		return true
	case strings.Contains(lowerName, "token"):
		return true
	case strings.Contains(lowerName, "password"):
		return true
	}

	return false
}
