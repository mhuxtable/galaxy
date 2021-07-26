package galaxy

import (
	"encoding/gob"
	"errors"
	"fmt"
	"io"
	"sync"
	"time"
)

type Message struct {
	Data      []byte
	Timestamp time.Time
}

type Recorder struct {
	Dest io.Writer

	enc  *gob.Encoder
	once sync.Once
}

func (r *Recorder) Receive(msg Message) error {
	r.init()
	return r.enc.Encode(msg)
}

func (r *Recorder) init() {
	r.once.Do(func() {
		r.enc = gob.NewEncoder(r.Dest)
	})
}

func ReadIn(out chan<- Message, r io.Reader) error {
	defer close(out)

	dec := gob.NewDecoder(r)
	var msg Message

	for {
		if err := dec.Decode(&msg); err != nil {
			if errors.Is(err, io.EOF) {
				return nil
			}

			return fmt.Errorf("while decoding: %w", err)
		}

		out <- msg
	}
}
