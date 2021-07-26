package main

import (
	"bytes"
	"fmt"
	"os"
	"time"

	"github.com/spf13/cobra"
	"go.tigermatt.uk/galaxy"
	"golang.org/x/sync/errgroup"
)

func dump(_ *cobra.Command, args []string) error {
	f, err := os.Open(args[0])
	if err != nil {
		return fmt.Errorf("reading file: %w", err)
	}
	defer f.Close()

	msgs := make(chan galaxy.Message, 100)

	var g errgroup.Group
	g.Go(func() error { return processMsgs(msgs) })
	g.Go(func() error { return galaxy.ReadIn(msgs, f) })

	return g.Wait()
}

func processMsgs(msgs <-chan galaxy.Message) error {
	var lastMsg, lastRead time.Time
	var thisMsg bytes.Buffer

	for msg := range msgs {
		if !lastMsg.IsZero() && msg.Timestamp.Sub(lastRead) > 5*time.Millisecond {
			bs := thisMsg.Bytes()
			fmt.Printf("%s: %X %s\n", lastMsg.Format("15:04:05.000"), bs, render(bs))
			thisMsg.Reset()
			lastMsg = msg.Timestamp
		}

		lastRead = msg.Timestamp
		if lastMsg.IsZero() {
			lastMsg = lastRead
		}

		_, err := thisMsg.Write(msg.Data)
		if err != nil {
			return err
		}
	}

	return nil
}

func render(bs []byte) string {
	pad := fmt.Sprintf("%*s", 20-(len(bs)*2), " ")
	return fmt.Sprintf("%s%s", pad, bs)
}
