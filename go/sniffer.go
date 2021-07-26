package galaxy

import (
	"context"
	"fmt"

	"go.bug.st/serial"
)

type Sniffer struct {
	Port      serial.Port
	OnReceive func([]byte)
}

func (s *Sniffer) Consume(ctx context.Context) error {
	for {
		select {
		case <-ctx.Done():
			return nil
		default:
		}

		bs := make([]byte, 8)

		n, err := s.Port.Read(bs)
		if err != nil {
			return fmt.Errorf("reading from serial port: %w", err)
		}

		if n > 0 {
			s.OnReceive(bs[:n])
		}
	}
}
