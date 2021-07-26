from logger import fmt_bytes


def raw_decode(data):
    return fmt_bytes(data)


def decode_backlight(data):
    op = data[0]
    if op == 0x00:
        return 'OFF'
    elif op == 0x01:
        return 'ON'
    else:
        return 'UNKNOWN BACKLIGHT OPERATION'


def decode_display(data):
    return raw_decode(data)


KEYS = [str(c) for c in range(0,10)] + ['B', 'A', 'ENT', 'ESC', '*', '#']

def decode_ok_with_key(data):
    op = data[0]
    if op == 0x7F:
        return 'TAMPER'
    else:
        print(data)
        tamper = (op & 0x40) != 0x0
        key = KEYS[op&0xF]
        return 'KEY PRESS: {:s}{}'.format(
            key,
            ' TAMPER' if tamper else '',
        )


def decode_poll(data):
    return raw_decode(data[1:-1])


DECODERS = {
    0x07: ('DISP  ', decode_display),
    0x0D: ('BKLGHT', decode_backlight),
    0x19: ('POLL  ', decode_poll),
    0xF4: ('OK KEY', decode_ok_with_key),
}
