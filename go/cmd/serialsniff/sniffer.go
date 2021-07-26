package main

// func sniff(_ *cobra.Command, args []string) error {
// 	port, err := serial.Open(args[0], &serial.Mode{
// 		BaudRate: 9600,
// 		DataBits: 8,
// 		Parity:   serial.NoParity,
// 		StopBits: serial.OneStopBit,
// 	})
// 	if err != nil {
// 		return fmt.Errorf("while opening serial port %s: %w", args[0], err)
// 	}
//
// 	rcvd := make(chan byte, 10240)
//
// 	//go consume(rcvd)
// 	go consumeRaw(rcvd)
//
// 	bs := make([]byte, 1)
// 	for {
// 		n, err := port.Read(bs)
// 		if err != nil {
// 			log.Printf("While reading from port: %s", err)
// 		}
//
// 		if n > 0 {
// 			select {
// 			case rcvd <- bs[0]:
// 				fmt.Printf("RCV %02X\n", bs[0])
// 			default:
// 				panic("queue backed up")
// 			}
// 		}
// 	}
// }

//type state int
//
//const (
//	NoPacket state = iota
//	WaitCommand
//	ReceiveData
//)
//
//var suppress = map[Command]struct{}{
//	// OK:          struct{}{},
//	// PollCommand: struct{}{},
//	CommandC0: struct{}{},
//}
//
//// func dump(bs []byte) string {
////	dump := make([]string, len(bs))
////	for i, b := range bs {
////		dump[i] = fmt.Sprintf("%02X", b)
////	}
////
////	return strings.Join(dump, " ")
////}
//
//func consumeRaw(ch <-chan byte) {
//	for _ = range ch {
//		//if b == 0x10 || b == 0x11 || b == 0x90 {
//		//	fmt.Printf("END\n")
//		//}
//		//fmt.Printf("%02X ", b)
//	}
//}
//
//func consume(ch <-chan byte) {
//	i := 1
//
//	var buf bytes.Buffer
//	var state state
//
//	var d Describer
//	var remain int
//	ignoreNextReply := false
//	variableLen := false
//	for {
//		b := <-ch
//
//		switch state {
//		case NoPacket:
//			if b == 0x10 || b == 0x11 {
//				if b == 0x11 && ignoreNextReply {
//					ignoreNextReply = false
//					continue
//				}
//				// possibly a packet, wait for more
//				state = WaitCommand
//				variableLen = false
//				buf.Reset()
//			} else {
//				if b == 0x90 {
//					ignoreNextReply = true
//				}
//				continue
//			}
//		case WaitCommand:
//			var ok bool
//			if d, ok = commandDataLen[Command(b)]; !ok {
//				// unknown type of command
//				state = NoPacket
//				fmt.Printf("%d: %s UNKNOWN COMMAND %02X\n", i, dump(buf.Bytes()), b)
//				i += 1
//				continue
//			}
//
//			state = ReceiveData
//			remain = d.ExpectLen()
//			variableLen = remain == -1
//		case ReceiveData:
//			if remain < -32 {
//				state = NoPacket
//				continue
//			}
//			if remain <= 0 {
//				// at the CRC
//				bs := buf.Bytes()
//
//				var sb strings.Builder
//				sb.WriteString(fmt.Sprintf("%d: %s ", i, dump(bs)))
//
//				var crc byte
//				{
//					var c GalaxyCRC
//					c.Write(bs)
//					crc = c.Sum()
//				}
//				sb.WriteString(fmt.Sprintf("%02X ", crc))
//				if crc != b {
//					if variableLen {
//						remain--
//						continue
//					}
//					sb.WriteString("INVALID CRC ")
//				} else {
//					sb.WriteString(d.Describe(bs[2:]))
//				}
//				sb.WriteString("\n")
//
//				if _, exist := suppress[Command(bs[1])]; !exist {
//					fmt.Printf(sb.String())
//				}
//
//				i += 1
//				state = NoPacket
//				continue
//			}
//
//			remain--
//		}
//
//		buf.WriteByte(b)
//	}
//}
//
//type GalaxyCRC uint32
//
//func (c *GalaxyCRC) Write(p []byte) (int, error) {
//	for _, b := range p {
//		*c = GalaxyCRC(uint32(*c) + uint32(b))
//	}
//
//	return len(p), nil
//}
//
//func (c *GalaxyCRC) WriteByte(b byte) error {
//	_, err := c.Write([]byte{b})
//	return err
//}
//
//func (c GalaxyCRC) Sum() byte {
//	v := uint32(c) + 0xaa
//	for v > 0xFF {
//		v = (v >> 8) + (v & 0xFF)
//	}
//
//	return byte(v)
//}
//
//func (c GalaxyCRC) Reset() {
//	c = GalaxyCRC(0)
//}
