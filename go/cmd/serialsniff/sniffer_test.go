package main

import "testing"

func TestCRC(t *testing.T) {
	for _, c := range []struct {
		in  []byte
		crc byte
	}{
		{
			in:  []byte{0x11, 0xfe},
			crc: 0xba,
		},
	} {
		var crc GalaxyCRC
		crc.Write(c.in)
		if sum := crc.Sum(); sum != c.crc {
			t.Errorf("Expected CRC %02X got %02X", c.crc, sum)
		}
	}
}
