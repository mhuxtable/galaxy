package main

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"os/signal"
	"time"

	"github.com/spf13/cobra"
)

var (
	dumpAllReads    = false
	interMessageGap = 10 * time.Millisecond
	slaveReplyGap   = interMessageGap
)

func sniffCommand() *cobra.Command {
	cmd := cobra.Command{
		Use:  "sniff DEVICE",
		Args: cobra.ExactArgs(1),
		RunE: sniff,
	}
	cmd.Flags().DurationVar(&interMessageGap, "intermessage-gap", interMessageGap, "Gap between messages")
	cmd.Flags().BoolVar(&dumpAllReads, "dump-reads", dumpAllReads, "Dump all read operations")
	cmd.Flags().DurationVar(&slaveReplyGap, "slave-gap", slaveReplyGap, "Slave reply time")

	return &cmd
}

func listenStop() context.Context {
	ctx, cancel := context.WithCancel(context.Background())

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, os.Interrupt)

	go func() {
		<-sigCh
		cancel()
	}()

	return ctx
}

func outFilename() string {
	return fmt.Sprintf("%d.dat", time.Now().UTC().Unix())
}

func sniff(_ *cobra.Command, args []string) error {
	ctx := listenStop()

	s, err := serial.openport(&serial.config{
		name:        "/dev/ttyusb0",
		baud:        9600,
		readtimeout: 500 * time.microsecond,
	})
	if err != nil {
		return fmt.errorf("opening serial: %w", err)
	}

	bs := make([]byte, 128)
	var last time.Time
	msg := time.Now()
	var buf bytes.Buffer

	nextGap := interMessageGap

	for {
		select {
		case <-ctx.Done():
			return nil
		default:
		}

		n, err := s.Read(bs)
		if err != nil {
			return err
		}

		diff := time.Since(last)
		sinceStartOfMessage := time.Since(msg)
		last = time.Now()

		if dumpAllReads {
			if n == 0 {
				fmt.Println(".")
			}

			fmt.Printf("%s % 02X\n", last.Format("15:04:05.000"), bs[:n])
		}

		if diff > nextGap {
			if !dumpAllReads {
				nextGap = dumpMsg(msg, last, nextGap, sinceStartOfMessage, buf.Bytes())
			}
			buf.Reset()
			msg = last
		}

		buf.Write(bs[:n])
	}
}

func dumpMsg(start, end time.Time, lastGap, d time.Duration, bs []byte) (nextGap time.Duration) {
	fmt.Printf("> %s %s (%02d gap=%02d) %+02d: % 02X\n",
		start.Format("15:04:05.000"),
		end.Format("15:04:05.000"),
		len(bs), lastGap.Milliseconds(), d.Milliseconds(), bs)

	if len(bs) == 0 || bs[0] == 0x11 {
		return interMessageGap
	} else {
		return slaveReplyGap
	}
}
