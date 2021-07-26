package main

// type Packet struct {
// 	Receiver byte
// 	Command  byte
// 	Data     []byte
// }
//
// type Command int
//
// const (
// 	InitCommand   Command = 0x00
// 	InitCommandOK         = 0xFF
// 	BadChecksum           = 0xF2
// 	PollCommand           = 0x19
// 	OK                    = 0xFE
// 	OKWithReply           = 0xF4
// 	CommandC0             = 0xC0 // what is this sent often to keypad?
// 	ScreenUpdate          = 0x07
// 	Backlight             = 0x0D
// )
//
// type Describer interface {
// 	Describe([]byte) string
// 	ExpectLen() int
// }
//
// type simpleDescriber struct {
// 	msg       string
// 	expectLen int
// }
//
// func newSimple(expectLen int, msg string) Describer {
// 	return &simpleDescriber{msg: msg, expectLen: expectLen}
// }
//
// func (d *simpleDescriber) Describe([]byte) string {
// 	return d.msg
// }
//
// func (d simpleDescriber) ExpectLen() int {
// 	return d.expectLen
// }
//
// type okWithReplyCommand struct{}
//
// func (*okWithReplyCommand) Describe(bs []byte) string {
// 	if len(bs) != 1 {
// 		panic("only one byte expected")
// 	}
//
// 	data := bs[0]
// 	if data == 0x7F {
// 		return "OK TAMPER"
// 	}
//
// 	// key press, maybe with tamper
// 	tamper := ""
// 	if data&0x40 == 0x40 {
// 		tamper = "TAMPER "
// 	}
//
// 	const keys = "01234567890BAEX*#"
// 	return fmt.Sprintf("OK %sKEY %s", tamper, string(keys[data&0xF]))
// }
//
// func (*okWithReplyCommand) ExpectLen() int {
// 	return 1
// }
//
// type backlightCommand struct{}
//
// func (*backlightCommand) Describe(bs []byte) string {
// 	state := ""
// 	switch data := bs[0]; data {
// 	case 0x00:
// 		state = "OFF"
// 	case 0x01:
// 		state = "ON"
// 	default:
// 		state = fmt.Sprintf("UNKNOWN %02X", data)
// 	}
//
// 	return fmt.Sprintf("BACKLIGHT %s", state)
// }
//
// func (*backlightCommand) ExpectLen() int {
// 	return 1
// }
//
// type screenUpdateCommand struct{}
//
// func (*screenUpdateCommand) Describe(bs []byte) string {
// 	return fmt.Sprintf("Screen update: %s", dump(bs))
// }
//
// func (*screenUpdateCommand) ExpectLen() int {
// 	return -1
// }
//
// var commandDataLen = map[Command]Describer{
// 	InitCommand:   newSimple(1, "INIT"),
// 	InitCommandOK: newSimple(0, "INIT OK"),
// 	BadChecksum:   newSimple(0, "BAD CHK"),
// 	PollCommand:   newSimple(1, "POLL"),
// 	OK:            newSimple(0, "OK"),
// 	OKWithReply:   &okWithReplyCommand{},
// 	CommandC0:     newSimple(0, "CURRENTLY UNKNOWN COMMAND C0"),
// 	ScreenUpdate:  &screenUpdateCommand{},
// 	Backlight:     &backlightCommand{},
// }
