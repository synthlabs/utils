package log

import (
	"io"
	"os"
	"time"

	charm "github.com/charmbracelet/log"
)

type Logger interface {
	Debug(string, ...any)
	Info(string, ...any)
	Warn(string, ...any)
	Error(string, ...any)
	Panic(string, ...any)
	Fatal(string, ...any)

	With(...any) Logger
	Named(string) Logger
	Level() charm.Level

	SecretField(string, interface{}) []any
	SecretString(string) string
}

type Options struct {
	Level           string
	Namespace       string
	JSON            bool
	ShowSecrets     bool
	Writer          io.Writer
	ReportCaller    bool
	ReportTimestamp bool
	TimeFormat      string
	CallerOffset    int
}

type logger struct {
	l           *charm.Logger
	namespace   string
	opts        charm.Options
	showSecrets bool
}

var _ Logger = &logger{}

func ParseLevel(level string) (charm.Level, error) {
	return charm.ParseLevel(level)
}

func New(logLevel string, namespace string, json bool, showSecrets bool) Logger {
	logger, err := NewWithOptions(Options{
		Level:           logLevel,
		Namespace:       namespace,
		JSON:            json,
		ShowSecrets:     showSecrets,
		ReportCaller:    true,
		ReportTimestamp: true,
		CallerOffset:    1,
	})
	if err != nil {
		charm.Fatal("failed to parse log level", "level", logLevel, "err", err)
	}

	return logger
}

func NewWithOptions(options Options) (Logger, error) {
	logLevel := options.Level
	if logLevel == "" {
		logLevel = "info"
	}

	level, err := charm.ParseLevel(logLevel)
	if err != nil {
		return nil, err
	}

	timeFormat := options.TimeFormat
	if timeFormat == "" {
		timeFormat = time.Kitchen
	}

	writer := options.Writer
	if writer == nil {
		writer = os.Stderr
	}

	opts := charm.Options{
		Formatter:       charm.TextFormatter,
		ReportCaller:    options.ReportCaller,
		ReportTimestamp: options.ReportTimestamp,
		TimeFormat:      timeFormat,
		Level:           level,
		CallerOffset:    options.CallerOffset,
		Prefix:          options.Namespace,
	}

	if options.JSON {
		opts.Formatter = charm.JSONFormatter
	}

	l := charm.NewWithOptions(writer, opts)

	return &logger{l: l, namespace: options.Namespace, opts: opts, showSecrets: options.ShowSecrets}, nil
}

func (l *logger) Debug(msg string, fields ...any) {
	l.l.Debug(msg, fields...)
}

func (l *logger) Info(msg string, fields ...any) {
	l.l.Info(msg, fields...)
}

func (l *logger) Warn(msg string, fields ...any) {
	l.l.Warn(msg, fields...)
}

func (l *logger) Error(msg string, fields ...any) {
	l.l.Error(msg, fields...)
}

func (l *logger) Panic(msg string, fields ...any) {
	l.l.Fatal(msg, fields...)
}

func (l *logger) Fatal(msg string, fields ...any) {
	l.l.Fatal(msg, fields...)
}

func (l *logger) With(fields ...any) Logger {
	return &logger{l: l.l.With(fields...), namespace: l.namespace, opts: l.opts, showSecrets: l.showSecrets}
}

func (l *logger) Named(name string) Logger {
	namespace := name
	if l.namespace != "" {
		namespace = l.namespace + "/" + name
	}
	opts := l.opts
	opts.Prefix = namespace

	return &logger{l: l.l.WithPrefix(namespace), namespace: namespace, opts: opts, showSecrets: l.showSecrets}
}

func (l *logger) Level() charm.Level {
	return l.l.GetLevel()
}

func (l *logger) SecretField(key string, val any) []any {
	if l.showSecrets {
		return []any{key, val}
	}
	return []any{key, "*****"}
}

func (l *logger) SecretString(s string) string {
	if l.showSecrets {
		return s
	}
	return "*****"
}
