package main

import (
	"io"
	"log"
	"strings"
	"sync"
	"time"

	"github.com/tarm/serial"
)

var s *serial.Port
var mu sync.Mutex

func write(bs ...byte) []byte {
	mu.Lock()
	defer mu.Unlock()

	_, err := s.Write(bs)
	if err != nil {
		panic(err)
	}

	time.Sleep(time.Second * time.Duration((10*len(bs)+1)/9600))

	t := time.Now().Add(100 * time.Millisecond)

	reply := make([]byte, 128)
	var n int
	for {
		if time.Now().After(t) {
			break
		}

		got, err := s.Read(reply[n:])
		n += got
		if err != nil {
			if err == io.EOF {
				continue
			}

			panic(err)
		}
	}

	s.Flush()

	return reply[:n]
}

func logIn(name string, bs []byte) {
	log.Printf("< %s % 02X\n", name, bs)
}

func poller() {
	for {
		select {
		case lastMsg = <-screenMsg:
			screen()
		default:
		}

		if ackKey {
			sendAck()
			continue
		}

		log.Println("> PING")
		reply := write(0x10, 0x19, 0x01, 0xD4)
		logIn("< PONG", reply)

		switch reply[1] {
		case 0xF4:
			handleKeyTamper(reply[2])
		case 0xFE:
			tamper = false
		default:
			log.Printf("need to handle %d\n", reply[1])
		}
	}
}

func handleKeyTamper(b byte) {
	if b == 0x7F {
		tamper = true
	} else {
		tamper = b&0x40 == 0x40
		const keys = "0123456789BAEX*#"
		lastKey = keys[b&0xF]
		keyTime = time.Now()
		ackKey = true
	}
}

func sendAck() {
	logIn("< ACK", write(crc(0x10, 0x0B, ack)...))
	ack ^= 0x02
	ackKey = false
}

var toggle07 byte = 0x80
var screenMsg = make(chan [2]string)
var lastMsg = [2]string{"Hello World", "Testing 123"}

var tamper bool
var lastKey byte
var keyTime time.Time
var ack byte = 0x02
var ackKey bool

func screen() {
	log.Println("> SCREEN")
	flags := 0x01 | toggle07
	if ackKey {
		flags |= 0x10 | ack
		ack ^= 0x02
		ackKey = false
	}
	blink := []byte{0x10, 0x07, flags, 0x01, 0x07}

	send0 := []byte(lastMsg[0])
	send1 := []byte(lastMsg[1])

	if len(send0) > 16 {
		send0 = send0[:16]
	}
	for i := len(send0); i < 16; i++ {
		send0 = append(send0, 0x20)
	}
	if time.Since(keyTime) < 3*time.Second {
		send0[15] = byte(lastKey)
	}
	blink = append(blink, []byte(send0)...)

	if len(send1) > 16 {
		send1 = send1[:16]
	}

	blink = append(blink, 0x02)
	for i := len(send1); i < 16; i++ {
		send1 = append(send1, 0x20)
	}
	if tamper {
		send1[15] = 'T'
	}
	blink = append(blink, []byte(send1)...)

	logIn("SCRN", write(crc(blink...)...))

	toggle07 ^= 0x80
}

func crc(bs ...byte) []byte {
	c := 0xAA
	for _, b := range bs {
		c += int(b)
	}

	for c > 0xFF {
		c = (c >> 8) + (c & 0xFF)
	}

	return append(bs, byte(c))
}

func updateTime() {
	for range time.NewTicker(time.Second).C {
		d := strings.ToUpper(time.Now().Format("Mon 02 Jan"))
		t := time.Now().Format("15:04:05")
		screenMsg <- [2]string{d, t}
	}
}

func main() {
	var err error
	s, err = serial.OpenPort(&serial.Config{
		Name:        "/dev/ttyUSB0",
		Baud:        9600,
		ReadTimeout: 10 * time.Millisecond,
	})
	if err != nil {
		log.Fatal(err)
	}
	defer s.Close()

	logIn("START", write(0x10, 0x00, 0x0E, 0xC8))
	logIn("PONG", write(0x10, 0x19, 0x01, 0xD4))
	logIn("BKLT", write(crc(0x10, 0x0D, 0x01)...))
	logIn("BEEP", write(crc(0x10, 0x0C, 0x00, 0x00, 0x00)...))

	go poller()
	go updateTime()

	for {
	}
}
