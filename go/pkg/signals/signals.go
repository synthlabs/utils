package signals

import (
	"context"
	"log"
	"os"
	"os/signal"
	"syscall"
)

// SignalHandler returns a context that will be canceled when a SIGINT or SIGTERM is received
// it also returns a cancel function to self-initiate the canceling of the context
// if a second signal is received the package with exit the process with return code 1
func SignalHandler(ctx context.Context) (context.Context, context.CancelFunc) {
	c, cancel := context.WithCancel(ctx)

	gracefulStop := make(chan os.Signal, 2)
	signal.Notify(gracefulStop, syscall.SIGINT, syscall.SIGTERM)
	go func() {
		s := <-gracefulStop
		cancel()
		log.Printf("received signal: %s", s)
		<-gracefulStop
		os.Exit(0)
	}()

	return c, cancel
}
