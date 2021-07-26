package galaxy

import "github.com/tarm/serial"

func NewGalaxySerialBus(portName string) (serial.Port, error) {
	// With go.bug.st/serial library
	// -----------------------------
	//
	// return serial.Open(portName, &serial.Mode{
	// 	BaudRate: 9600,
	// 	DataBits: 8,
	// 	Parity:   serial.NoParity,
	// 	StopBits: serial.OneStopBit,
	// })

	panic("unimplemented")
}
